mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn estimates_are_gone_and_old_rows_never_count(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "h@example.com", "hours").await;
    // the history calculator was removed: 410, nothing stored
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/estimate",
        Some(&t),
        Some(json!({
            "game_name": "World of Warcraft",
            "eras": [{ "from_year": 2005, "to_year": 2007, "hours_per_week": 30 }]
        })),
    )
    .await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.text);
    assert_eq!(r.json()["error"], "estimates_removed");
    // an estimate saved before the removal stays in the table but is left out everywhere
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO hours_entries (id, user_id, game_id, tier, source, minutes) SELECT gen_random_uuid(), id, 'wow', 'estimated', 'estimator', 9000 FROM users WHERE handle = 'hours'")
        .execute(&pool)
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE tier = 'estimated'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "old rows are kept");
    let s = api(&app, "GET", "/api/v1/stats", Some(&t), None)
        .await
        .json();
    assert_eq!(s["games"].as_array().unwrap().len(), 0, "{s}");
    assert_eq!(s["overall"]["verified_min"], 0);
    assert_eq!(
        s["overall"]["estimated_min"], 0,
        "kept at 0 for old app builds"
    );
    let p = api(&app, "GET", "/api/v1/users/hours/profile", Some(&t), None).await;
    assert!(!p.text.contains("estimated"), "{}", p.text);
    // with real hours the row shows only those, and old builds still get estimated_min = 0
    api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_id": "wow", "minutes": 600 })),
    )
    .await;
    let s = api(&app, "GET", "/api/v1/stats", Some(&t), None)
        .await
        .json();
    assert_eq!(s["games"][0]["display_min"], 600, "{s}");
    assert_eq!(s["games"][0]["display_tier"], "imported");
    assert_eq!(s["games"][0]["estimated_min"], 0);

    // game lookup and note checks (shared with imports)
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_name": "My Totally Made Up Game", "minutes": 60 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert_eq!(r.json()["error"], "unknown_game", "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_name": "  ", "minutes": 60 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_id": "no-such-game", "minutes": 60 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_id": "wow", "note": "x".repeat(141), "minutes": 60 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn imports_and_delete(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "i@example.com", "importer").await;
    let other = signup(&app, "o@example.com", "other").await;
    // known signature id works even before anyone uploaded that game
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_id": "wow", "minutes": 120000, "source": "wow_manual" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert_eq!(v["self_reported"], true);
    let id = v["id"].as_str().unwrap().to_string();
    for (minutes, source) in [
        (0, "manual"),
        (100, "wow_addon"),
        (100, "estimator"),
        (30_000_000, "manual"),
    ] {
        let r = api(
            &app,
            "POST",
            "/api/v1/hours/import",
            Some(&t),
            Some(json!({ "game_id": "wow", "minutes": minutes, "source": source })),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{minutes} {source}");
    }
    // another user's manual game can't be targeted
    let mine = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_name": "Secret Game", "minutes": 60 })),
    )
    .await
    .json();
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&other),
        Some(json!({ "game_id": mine["game_id"], "minutes": 60 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/v1/hours/{id}"),
            Some(&other),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/v1/hours/{id}"),
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE game_id = 'wow'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}
