mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn profile_visibility(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await;
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&a),
        Some(json!({"handle": "bob"})),
    )
    .await;
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&b),
        Some(json!({"handle": "alice"})),
    )
    .await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true), ('manual:secret', 'My secret thing', false)")
        .execute(&pool).await.unwrap();
    for gid in ["wow", "manual:secret"] {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             SELECT gen_random_uuid(), id, $1, now() - interval '95 minutes', now() - interval '5 minutes', 5400
             FROM users WHERE handle = 'bob'",
        )
        .bind(gid).execute(&pool).await.unwrap();
    }

    // friend sees presence + games, unnamed game id is hidden
    let r = api(&app, "GET", "/api/v1/users/bob/profile", Some(&a), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert_eq!(v["relation"], "friend");
    assert_eq!(v["history_visible"], true);
    assert!(v["presence"].is_object());
    let games = v["games"].as_array().unwrap();
    assert!(
        games
            .iter()
            .any(|g| g["name"] == "World of Warcraft" && g["minutes_7d"] == 90),
        "{v}"
    );
    assert!(
        !r.text.contains("manual:secret") && !r.text.contains("My secret thing"),
        "{v}"
    );
    assert_eq!(v["recent"].as_array().unwrap().len(), 2);

    // public by default: a stranger sees the profile
    let v = api(&app, "GET", "/api/v1/users/bob/profile", Some(&c), None)
        .await
        .json();
    assert_eq!(v["relation"], "none");
    assert!(v["presence"].is_object());
    assert_eq!(v["games"].as_array().unwrap().len(), 2);
    assert_eq!(v["friend_count"], 1);

    // private profile: stranger gets name + picture only, friend still sees it all
    sqlx::query("UPDATE users SET public_profile = false WHERE handle = 'bob'")
        .execute(&pool)
        .await
        .unwrap();
    let v = api(&app, "GET", "/api/v1/users/bob/profile", Some(&c), None)
        .await
        .json();
    assert_eq!(v["private"], true);
    assert!(v["presence"].is_null() && v["games"].is_null() && v["friends"].is_null());
    let v = api(&app, "GET", "/api/v1/users/bob/profile", None, None)
        .await
        .json();
    assert_eq!(v["private"], true);
    assert_eq!(
        api(&app, "GET", "/api/v1/users/bob/profile", Some(&a), None)
            .await
            .json()["games"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // bob stops sharing: friend loses history, keeps presence
    sqlx::query("UPDATE users SET share_activity = false WHERE handle = 'bob'")
        .execute(&pool)
        .await
        .unwrap();
    let v = api(&app, "GET", "/api/v1/users/bob/profile", Some(&a), None)
        .await
        .json();
    assert_eq!(v["history_visible"], false);
    assert!(v["presence"].is_object());
    assert_eq!(v["recent"].as_array().unwrap().len(), 0);

    // self always sees everything
    let v = api(&app, "GET", "/api/v1/users/bob/profile", Some(&b), None)
        .await
        .json();
    assert_eq!(v["relation"], "self");
    assert_eq!(v["games"].as_array().unwrap().len(), 2);

    // unknown
    assert_eq!(
        api(&app, "GET", "/api/v1/users/nobody/profile", Some(&a), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // display name
    let r = api(
        &app,
        "POST",
        "/api/v1/me/display_name",
        Some(&b),
        Some(json!({"display_name": "  Bobby  "})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(&app, "GET", "/api/v1/users/bob/profile", Some(&a), None)
            .await
            .json()["display_name"],
        "Bobby"
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/me/display_name",
        Some(&b),
        Some(json!({"display_name": "x".repeat(33)})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/display_name",
        Some(&b),
        Some(json!({"display_name": ""})),
    )
    .await;
    assert_eq!(r.json()["display_name"], "bob");
}
