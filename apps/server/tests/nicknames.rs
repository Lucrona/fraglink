mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn nick(app: &axum::Router, t: &str, n: &str) -> common::Resp {
    api(
        app,
        "POST",
        "/api/v1/me/display_name",
        Some(t),
        Some(json!({ "display_name": n })),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn nicknames_rules_history_and_privacy(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bobby").await;
    // impersonation: another user's username or nickname (any case), staff titles
    assert_eq!(nick(&app, &a, "BOBBY").await.status, StatusCode::CONFLICT);
    assert_eq!(
        nick(&app, &b, "Bob the Builder").await.status,
        StatusCode::OK
    );
    assert_eq!(
        nick(&app, &a, "bob the builder").await.status,
        StatusCode::CONFLICT
    );
    for bad in [
        "Admin",
        "[MOD] Alice",
        "Fraglink Support",
        "xX_fraglink_Xx",
        "Staff",
    ] {
        let r = nick(&app, &a, bad).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}: {}", r.text);
    }
    assert_eq!(
        nick(&app, &a, &"x".repeat(33)).await.status,
        StatusCode::BAD_REQUEST
    );
    // three changes a day, then refused
    for n in ["Ally", "Ally  Cat", "Alice W"] {
        let r = nick(&app, &a, n).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    assert_eq!(
        nick(&app, &a, "Fourth").await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    // same name again is not a change
    assert_eq!(nick(&app, &a, "Alice W").await.status, StatusCode::OK);
    // history on the profile, newest first, current one left out
    let p = api(&app, "GET", "/api/v1/users/alice/profile", Some(&b), None)
        .await
        .json();
    assert_eq!(p["display_name"], "Alice W");
    assert_eq!(p["previous_names"], json!(["Ally Cat", "Ally", "alice"]));
    // search finds people by nickname too
    let s = api(
        &app,
        "GET",
        "/api/v1/users/search?q=builder",
        Some(&a),
        None,
    )
    .await;
    assert!(s.text.contains("bobby"), "{}", s.text);
    // hideable
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&a),
        Some(json!({ "privacy": { "name_history": false } })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = api(&app, "GET", "/api/v1/users/alice/profile", Some(&b), None)
        .await
        .json();
    assert_eq!(p["previous_names"], json!([]));
    let p = api(&app, "GET", "/api/v1/users/alice/profile", Some(&a), None)
        .await
        .json();
    assert_eq!(
        p["previous_names"].as_array().unwrap().len(),
        3,
        "you always see your own"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn admins_change_usernames_and_old_links_redirect(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "o@example.com", "olive").await;
    let u = signup(&app, "u@example.com", "oldname").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'olive'")
        .execute(&pool)
        .await
        .unwrap();
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'oldname'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let path = format!("/api/v1/admin/users/{id}/handle");
    // users themselves can't
    assert_ne!(
        api(
            &app,
            "POST",
            &path,
            Some(&u),
            Some(json!({ "handle": "x_new" }))
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        api(
            &app,
            "POST",
            &path,
            Some(&boss),
            Some(json!({ "handle": "olive" }))
        )
        .await
        .status,
        StatusCode::CONFLICT
    );
    assert_eq!(
        api(
            &app,
            "POST",
            &path,
            Some(&boss),
            Some(json!({ "handle": "bad name" }))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = api(
        &app,
        "POST",
        &path,
        Some(&boss),
        Some(json!({ "handle": "newname" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // the old profile link finds the renamed user (the site then redirects to the new URL)
    let p = api(
        &app,
        "GET",
        "/api/v1/users/oldname/profile",
        Some(&boss),
        None,
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.text);
    assert_eq!(p.json()["handle"], "newname");
    // nobody can sign up with the old one
    let s = api(&app, "POST", "/api/v1/auth/signup", None, Some(json!({ "email": "z@example.com", "password": "correct horse battery", "handle": "OldName" }))).await;
    assert_eq!(s.status, StatusCode::CONFLICT);
    // logged in the audit log
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = 'user.handle'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
}
