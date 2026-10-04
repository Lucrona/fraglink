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
