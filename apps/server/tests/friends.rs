mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

async fn req(app: &axum::Router, t: &str, handle: &str) -> (StatusCode, Value) {
    let r = api(
        app,
        "POST",
        "/api/v1/friends/request",
        Some(t),
        Some(json!({ "handle": handle })),
    )
    .await;
    (r.status, r.json())
}

async fn list(app: &axum::Router, t: &str) -> Value {
    api(app, "GET", "/api/v1/friends", Some(t), None)
        .await
        .json()
}

fn handles(v: &Value, k: &str) -> Vec<String> {
    v[k].as_array()
        .unwrap()
        .iter()
        .map(|x| x.get("handle").unwrap_or(x).as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn request_accept_decline_remove(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "Alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await;

    assert_eq!(req(&app, &a, "nobody").await.0, StatusCode::NOT_FOUND);
    assert_eq!(
        req(&app, &a, "alice").await.0,
        StatusCode::BAD_REQUEST,
        "can't add yourself"
    );
    let (s, v) = req(&app, &a, "@BOB").await; // handle lookup is case-insensitive, @ optional
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["status"], "pending");
    assert_eq!(
        req(&app, &a, "bob").await.1["status"],
        "pending",
        "repeat is a no-op"
    );
    assert_eq!(handles(&list(&app, &a).await, "outgoing"), ["bob"]);
    assert_eq!(handles(&list(&app, &b).await, "incoming"), ["Alice"]);
    assert!(handles(&list(&app, &a).await, "friends").is_empty());

    // only the recipient can accept
    let r = api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&a),
        Some(json!({ "handle": "bob", "accept": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&b),
        Some(json!({ "handle": "alice", "accept": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.text);
    let la = list(&app, &a).await;
    assert_eq!(handles(&la, "friends"), ["bob"]);
    assert_eq!(la["friends"][0]["presence"]["status"], "offline");
    assert!(la["friends"][0]["since"].is_string());
    assert_eq!(handles(&list(&app, &b).await, "friends"), ["Alice"]);

    // mutual requests auto-accept
    req(&app, &c, "alice").await;
    assert_eq!(req(&app, &a, "carol").await.1["status"], "accepted");
    assert_eq!(handles(&list(&app, &a).await, "friends"), ["bob", "carol"]);

    // decline
    let d = signup(&app, "d@example.com", "dave").await;
    req(&app, &d, "alice").await;
    let r = api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&a),
        Some(json!({ "handle": "dave", "accept": false })),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(handles(&list(&app, &a).await, "incoming").is_empty());
    assert!(handles(&list(&app, &d).await, "outgoing").is_empty());

    // remove
    assert_eq!(
        api(&app, "DELETE", "/api/v1/friends/bob", Some(&a), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(handles(&list(&app, &a).await, "friends"), ["carol"]);
    assert_eq!(
        handles(&list(&app, &b).await, "friends"),
        Vec::<String>::new()
    );

    // friends in the account export
    let e = api(&app, "GET", "/api/v1/export", Some(&a), None)
        .await
        .json();
    assert_eq!(e["friends"]["friends"][0]["handle"], "carol");
    // account deletion removes friendships
    api(
        &app,
        "DELETE",
        "/api/v1/me",
        Some(&c),
        Some(json!({ "password": "correct horse battery" })),
    )
    .await;
    assert!(handles(&list(&app, &a).await, "friends").is_empty());
    sqlx::query("UPDATE users SET delete_after = now() - interval '1 minute' WHERE delete_after IS NOT NULL")
        .execute(&pool)
        .await
        .unwrap();
    let st = fraglink_server::AppState::new(
        pool.clone(),
        fraglink_server::Config::for_tests("postgres://unused"),
    );
    fraglink_server::account::purge_due(&st).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM friendships")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn blocks_are_silent_and_final(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    req(&app, &a, "bob").await;
    api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&b),
        Some(json!({ "handle": "alice", "accept": true })),
    )
    .await;

    // bob blocks alice: friendship gone on both sides
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/blocks",
            Some(&b),
            Some(json!({ "handle": "alice" }))
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert!(handles(&list(&app, &a).await, "friends").is_empty());
    assert_eq!(handles(&list(&app, &b).await, "blocked"), ["alice"]);
    // alice asking again looks like a normal pending request, but nothing reaches bob
    assert_eq!(req(&app, &a, "bob").await.1["status"], "pending");
    assert!(handles(&list(&app, &b).await, "incoming").is_empty());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM friendships")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    // bob can't request alice while blocking her
    assert_eq!(req(&app, &b, "alice").await.0, StatusCode::CONFLICT);
    // alice's view of blocks never shows bob
    assert!(handles(&list(&app, &a).await, "blocked").is_empty());
    // unblock, then they can be friends again
    assert_eq!(
        api(&app, "DELETE", "/api/v1/blocks/alice", Some(&b), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(req(&app, &b, "alice").await.1["status"], "pending");
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/blocks",
            Some(&b),
            Some(json!({ "handle": "bob" }))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn request_rate_limit(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "spam@example.com", "spammer").await;
    let me: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    // 30 requests in the last hour, inserted directly to keep the test fast
    for i in 0..30 {
        let id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO users(id, email, handle, display_name, password_hash) VALUES($1, $2, $3, $3, 'x')")
            .bind(id).bind(format!("u{i}@example.com")).bind(format!("user{i}")).execute(&pool).await.unwrap();
        let (x, y) = if me < id { (me, id) } else { (id, me) };
        sqlx::query("INSERT INTO friendships(user_a, user_b, status, requested_by) VALUES($1, $2, 'pending', $3)")
            .bind(x).bind(y).bind(me).execute(&pool).await.unwrap();
    }
    signup(&app, "t@example.com", "target").await;
    assert_eq!(
        req(&app, &a, "target").await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}
