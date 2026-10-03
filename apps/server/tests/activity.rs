mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn befriend(app: &axum::Router, a: &str, b: &str, bh: &str, ah: &str) {
    api(
        app,
        "POST",
        "/api/v1/friends/request",
        Some(a),
        Some(json!({"handle": bh})),
    )
    .await;
    let r = api(
        app,
        "POST",
        "/api/v1/friends/request",
        Some(b),
        Some(json!({"handle": ah})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn activity_and_summary(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await;
    befriend(&app, &a, &b, "bob", "alice").await;

    sqlx::query("INSERT INTO games (id, name) VALUES ('wow', 'World of Warcraft')")
        .execute(&pool)
        .await
        .unwrap();
    for (h, mins) in [("bob", 90i32), ("carol", 30)] {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             SELECT gen_random_uuid(), id, 'wow', now() - make_interval(mins => $2 + 5), now() - interval '5 minutes', $2 * 60
             FROM users WHERE handle = $1",
        )
        .bind(h)
        .bind(mins)
        .execute(&pool)
        .await
        .unwrap();
    }

    let v = api(&app, "GET", "/api/v1/friends/activity", Some(&a), None)
        .await
        .json();
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{v}"); // carol isn't a friend
    assert_eq!(items[0]["handle"], "bob");
    assert_eq!(items[0]["kind"], "played");
    assert_eq!(items[0]["game_name"], "World of Warcraft");
    assert_eq!(items[0]["duration_s"], 5400);

    let s = api(&app, "GET", "/api/v1/friends/bob/summary", Some(&a), None).await;
    assert_eq!(s.status, StatusCode::OK, "{}", s.text);
    let s = s.json();
    assert_eq!(s["last_7d_seconds"], 5400);
    assert_eq!(s["top_game_7d"], "World of Warcraft");
    assert!(s["week_seconds"].as_i64().unwrap() <= 5400);
    assert_eq!(
        api(&app, "GET", "/api/v1/friends/carol/summary", Some(&a), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // bob opts out: history disappears, summary hides hours
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&b),
        Some(json!({"share_activity": false})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&b), None).await.json()["user"]["share_activity"],
        false
    );
    let v = api(&app, "GET", "/api/v1/friends/activity", Some(&a), None)
        .await
        .json();
    assert_eq!(v["items"].as_array().unwrap().len(), 0);
    let s = api(&app, "GET", "/api/v1/friends/bob/summary", Some(&a), None)
        .await
        .json();
    assert_eq!(s["shares_activity"], false);
    assert!(s["week_seconds"].is_null());
    let _ = c;
}
