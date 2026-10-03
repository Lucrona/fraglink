mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn change_password_and_email(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t1 = signup(&app, "ana@example.com", "ana_1").await;
    let login = |pw: &'static str| {
        let app = app.clone();
        async move {
            api(
                &app,
                "POST",
                "/api/v1/auth/login",
                None,
                Some(json!({"email":"ana_1","password":pw})),
            )
            .await
        }
    };
    let t2 = login("correct horse battery").await.json()["token"]
        .as_str()
        .unwrap()
        .to_string();

    // wrong current password, too short, same as current
    let pw = |cur: &str, new: &str| json!({"current_password": cur, "new_password": new});
    let r = api(
        &app,
        "POST",
        "/api/v1/me/password",
        Some(&t1),
        Some(pw("nope nope nope", "brand new password")),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/password",
        Some(&t1),
        Some(pw("correct horse battery", "short")),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/password",
        Some(&t1),
        Some(pw("correct horse battery", "correct horse battery")),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // success: argon2id hash, other token signed out, this one kept
    let r = api(
        &app,
        "POST",
        "/api/v1/me/password",
        Some(&t1),
        Some(pw("correct horse battery", "brand new password")),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["signed_out_other_sessions"], 1);
    let h: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(h.starts_with("$argon2id$"));
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t1), None).await.status,
        StatusCode::OK
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t2), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login("correct horse battery").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login("brand new password").await.status, StatusCode::OK);

    // email: wrong password, bad format, taken, success (unverified)
    signup(&app, "bob@example.com", "bob_1").await;
    let em = |cur: &str, e: &str| json!({"current_password": cur, "new_email": e});
    let r = api(
        &app,
        "POST",
        "/api/v1/me/email",
        Some(&t1),
        Some(em("wrong password!", "new@example.com")),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/email",
        Some(&t1),
        Some(em("brand new password", "not-an-email")),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/email",
        Some(&t1),
        Some(em("brand new password", "BOB@example.com")),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/email",
        Some(&t1),
        Some(em("brand new password", "ana.new@example.com")),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let me = api(&app, "GET", "/api/v1/me", Some(&t1), None).await.json();
    assert_eq!(me["user"]["email"], "ana.new@example.com");
    assert_eq!(me["user"]["email_verified"], false);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn account_changes_are_rate_limited(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let t = signup(&app, "c@example.com", "cat_1").await;
    let mut last = StatusCode::OK;
    for _ in 0..6 {
        last = api(
            &app,
            "POST",
            "/api/v1/me/password",
            Some(&t),
            Some(
                json!({"current_password":"wrong wrong wrong","new_password":"whatever long pass"}),
            ),
        )
        .await
        .status;
    }
    assert_eq!(last, StatusCode::TOO_MANY_REQUESTS);
}
