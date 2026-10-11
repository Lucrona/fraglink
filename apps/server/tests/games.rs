mod common;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::io::Read;

fn get(path: &str, hdrs: &[(&str, &str)]) -> Request<Body> {
    let mut b = Request::builder().method("GET").uri(path);
    for (k, v) in hdrs {
        b = b.header(*k, *v);
    }
    b.body(Body::empty()).unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn detectable_list_is_gzipped_and_cacheable(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let r = send(
        &app,
        get(
            "/api/v1/games/detectable",
            &[("accept-encoding", "gzip, br")],
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers[header::CONTENT_ENCODING], "gzip");
    let etag = r.headers[header::ETAG].to_str().unwrap().to_string();
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(&r.bytes[..])
        .read_to_end(&mut json)
        .unwrap();
    let db = fraglink_core::detectable::DetectableDb::from_json(&json).unwrap();
    assert!(db.len() > 5000);
    assert!(db
        .match_process(
            "StarCitizen.exe",
            Some(r"C:\RSI\StarCitizen\LIVE\Bin64\StarCitizen.exe")
        )
        .is_some());

    // plain JSON for clients that don't take gzip
    let plain = send(&app, get("/api/v1/games/detectable", &[])).await;
    assert!(plain.headers.get(header::CONTENT_ENCODING).is_none());
    assert_eq!(plain.bytes, json);

    let again = send(
        &app,
        get("/api/v1/games/detectable", &[("if-none-match", &etag)]),
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert!(again.bytes.is_empty());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn overrides_from_the_data_dir_win(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let app = fraglink_server::router(state_with(pool, |c| c.data_dir = d.clone()));
    let r = send(&app, get("/api/v1/games/overrides", &[])).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.text, fraglink_core::overrides::BUILTIN_JSON);
    let etag = r.headers[header::ETAG].to_str().unwrap().to_string();

    let custom = r#"{"schema":1,"version":"x","rules":[{"id":"wow_forever","name":"World of Warcraft: Forever","exes":["wowf.exe"]}]}"#;
    std::fs::write(d.join("detect-overrides.json"), custom).unwrap();
    let r2 = send(
        &app,
        get("/api/v1/games/overrides", &[("if-none-match", &etag)]),
    )
    .await;
    assert_eq!(r2.status, StatusCode::OK, "changed file, new etag");
    assert_eq!(r2.text, custom);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn suggestions_are_stored_for_review(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    for name in ["Odd Game", "odd game"] {
        let r = api(
            &app,
            "POST",
            "/api/v1/games/suggest",
            None,
            Some(json!({ "exe": "OddGame.exe", "name": name })),
        )
        .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.text);
    }
    let bad = api(
        &app,
        "POST",
        "/api/v1/games/suggest",
        None,
        Some(json!({ "exe": "C:\\x\\OddGame.exe", "name": "x" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let rows: Vec<(String, String, i32, String)> =
        sqlx::query_as("SELECT exe, name, reports, status FROM game_suggestions")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![("OddGame.exe".into(), "Odd Game".into(), 2, "new".into())]
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn staff_renames_reach_the_overrides_list(pool: PgPool) {
    sqlx::query("INSERT INTO game_overrides(game_id, name) VALUES('wow_forever', 'World of Warcraft: Forever - Beta')")
        .execute(&pool)
        .await
        .unwrap();
    // an approved program's own entry, renamed by staff afterwards
    sqlx::query(
        "INSERT INTO games(id, name, verified) VALUES('exe:oddgame.exe', 'Odd Game', true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO game_overrides(game_id, name) VALUES('exe:oddgame.exe', 'Odd Game Renamed')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let app = fraglink_server::router(state_with(pool, |c| c.data_dir = d.clone()));
    let r = send(&app, get("/api/v1/games/overrides", &[])).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_ne!(r.text, fraglink_core::overrides::BUILTIN_JSON);
    let o = fraglink_core::overrides::Overrides::from_json(&r.text).unwrap();
    let rule = o
        .match_process(
            "WowB.exe",
            Some(r"E:\Games\World of Warcraft\_beta_\WowB.exe"),
        )
        .expect("renamed game gets a rule");
    assert_eq!(rule.id, "wow_forever");
    assert_eq!(rule.name, "World of Warcraft: Forever - Beta");
    let odd = o
        .match_process("OddGame.exe", None)
        .expect("approved program rule");
    assert_eq!(
        (odd.id.as_str(), odd.name.as_str()),
        ("exe:oddgame.exe", "Odd Game Renamed")
    );
    // other games are untouched
    assert!(o
        .match_process(
            "Wow.exe",
            Some(r"E:\Games\World of Warcraft\_retail_\Wow.exe")
        )
        .is_none());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn wow_realms_are_served_from_the_cache_with_an_etag(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let app = fraglink_server::router(state_with(pool, |c| c.data_dir = d.clone()));
    let r = send(&app, get("/api/v1/games/wow-realms", &[])).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE, "no list yet");
    let body =
        r#"{"v":1,"updated":"x","source":"test","flavours":{"retail":{"us":["Frostmourne"]}}}"#;
    std::fs::write(d.join("wow-realms.json"), body).unwrap();
    let r = send(&app, get("/api/v1/games/wow-realms", &[])).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.text, body);
    let etag = r.headers[header::ETAG].to_str().unwrap().to_string();
    let again = send(
        &app,
        get("/api/v1/games/wow-realms", &[("if-none-match", &etag)]),
    )
    .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
}
