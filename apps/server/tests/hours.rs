mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn estimates_replace_and_validate(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "h@example.com", "hours").await;
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({
        "game_name": "World of Warcraft", "note": "vanilla + TBC",
        "eras": [{ "from_year": 2005, "to_year": 2007, "hours_per_week": 30 }, { "from_year": 2007, "to_year": 2008, "hours_per_week": 50 }]
    }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert_eq!(v["tier"], "estimated");
    assert_eq!(v["minutes"], (3 * 52 * 30 + 2 * 52 * 50) * 60);
    assert!(
        !v["warnings"].as_array().unwrap().is_empty(),
        "overlapping 2007 and >40 h/week multi-year should warn"
    );
    // a typed name picks the game from the server's list; nobody creates or names games
    let gid = v["game_id"].as_str().unwrap().to_string();
    assert_eq!(gid, "wow");
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_name": "My Totally Made Up Game", "eras": [{ "from_year": 2010, "to_year": 2010, "hours_per_week": 1 }] }))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert_eq!(r.json()["error"], "unknown_game", "{}", r.text);

    // saving again replaces (one estimate per game)
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_id": gid, "eras": [{ "from_year": 2010, "to_year": 2010, "hours_per_week": 10 }] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE tier = 'estimated'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    for eras in [
        json!([]),
        json!([{ "from_year": 2010, "to_year": 2009, "hours_per_week": 10 }]),
        json!([{ "from_year": 2010, "to_year": 2011, "hours_per_week": 81 }]),
        json!([{ "from_year": 1960, "to_year": 1961, "hours_per_week": 1 }]),
        json!([{ "from_year": 2010, "to_year": 2099, "hours_per_week": 1 }]),
    ] {
        let r = api(
            &app,
            "POST",
            "/api/v1/hours/estimate",
            Some(&t),
            Some(json!({ "game_name": "X", "eras": eras })),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{eras}: {}", r.text);
    }
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_name": "  ", "eras": [{ "from_year": 2010, "to_year": 2010, "hours_per_week": 1 }] }))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_id": "no-such-game", "eras": [{ "from_year": 2010, "to_year": 2010, "hours_per_week": 1 }] }))).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_name": "X", "note": "x".repeat(141), "eras": [{ "from_year": 2010, "to_year": 2010, "hours_per_week": 1 }] }))).await;
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
