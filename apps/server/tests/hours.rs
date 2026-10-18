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
    seed_imported(&pool, "hours", "wow", "World of Warcraft", 600).await;
    let s = api(&app, "GET", "/api/v1/stats", Some(&t), None)
        .await
        .json();
    assert_eq!(s["games"][0]["display_min"], 600, "{s}");
    assert_eq!(s["games"][0]["display_tier"], "imported");
    assert_eq!(s["games"][0]["estimated_min"], 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn manual_imports_are_disabled_and_old_rows_stay(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "i@example.com", "importer").await;
    let other = signup(&app, "o@example.com", "other").await;
    let id = seed_imported(&pool, "importer", "wow", "World of Warcraft", 120000).await;
    // 410 for every body, signed in or not; nothing is written or changed
    for (tok, body) in [
        (
            Some(t.as_str()),
            json!({ "game_id": "wow", "minutes": 600, "source": "wow_manual" }),
        ),
        (
            Some(t.as_str()),
            json!({ "game_name": "World of Warcraft", "minutes": 3_000_000 }),
        ),
        (Some(t.as_str()), json!({ "nonsense": true })),
        (None, json!({ "game_id": "wow", "minutes": 60 })),
    ] {
        let r = api(&app, "POST", "/api/v1/hours/import", tok, Some(body)).await;
        assert_eq!(r.status, StatusCode::GONE, "{}", r.text);
        assert_eq!(r.json()["error"], "imports_disabled");
        assert_eq!(r.json()["message"], "Manual hour imports are disabled.");
    }
    let rows: Vec<(uuid::Uuid, i32)> = sqlx::query_as("SELECT id, minutes FROM hours_entries")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(
        rows,
        vec![(id, 120000)],
        "the old row is untouched, nothing new"
    );
    // the old row still counts, and its owner (only) can still delete it
    let s = api(&app, "GET", "/api/v1/stats", Some(&t), None)
        .await
        .json();
    assert_eq!(s["games"][0]["display_min"], 120000, "{s}");
    let del = |tok: String| {
        let app = app.clone();
        async move {
            api(
                &app,
                "DELETE",
                &format!("/api/v1/hours/{id}"),
                Some(&tok),
                None,
            )
            .await
            .status
        }
    };
    assert_eq!(del(other).await, StatusCode::NOT_FOUND);
    assert_eq!(del(t).await, StatusCode::NO_CONTENT);
}

/// The import checks stay for a future validated WoW addon import (not routed today).
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn import_checks_kept_for_a_future_addon_import(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    signup(&app, "i@example.com", "importer").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'importer'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let imp = |v: serde_json::Value| {
        let st = st.clone();
        async move {
            fraglink_server::api::import_for(&st, uid, serde_json::from_value(v).unwrap()).await
        }
    };
    // a known signature id works even before anyone played that game
    let v = imp(json!({ "game_id": "wow", "minutes": 120000, "source": "wow_manual" }))
        .await
        .unwrap();
    assert_eq!(v["self_reported"], true);
    // exactly 50,000 hours is the most one game takes
    assert!(imp(json!({ "game_id": "wow", "minutes": 3_000_000 }))
        .await
        .is_ok());
    for (v, code) in [
        (json!({ "game_id": "wow", "minutes": 0 }), "invalid_minutes"),
        (
            json!({ "game_id": "wow", "minutes": 3_000_001 }),
            "invalid_minutes",
        ),
        (
            json!({ "game_id": "wow", "minutes": 30_000_000 }),
            "invalid_minutes",
        ),
        (
            json!({ "game_id": "wow", "minutes": 100, "source": "wow_addon" }),
            "invalid_source",
        ),
        (
            json!({ "game_id": "wow", "minutes": 100, "source": "estimator" }),
            "invalid_source",
        ),
        (
            json!({ "game_name": "My Totally Made Up Game", "minutes": 60 }),
            "unknown_game",
        ),
        (
            json!({ "game_id": "no-such-game", "minutes": 60 }),
            "unknown_game",
        ),
    ] {
        let e = imp(v.clone()).await.unwrap_err();
        assert_eq!(e.status, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(e.code, code, "{v}");
    }
    let e = imp(json!({ "game_id": "wow", "note": "x".repeat(141), "minutes": 60 }))
        .await
        .unwrap_err();
    assert_eq!(e.status, StatusCode::BAD_REQUEST);
    assert!(imp(json!({ "game_name": "  ", "minutes": 60 }))
        .await
        .is_err());
}
