mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::io::Read;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn zip_export_has_csvs_and_json(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let t = signup(&app, "z@example.com", "zipper").await;
    let dev = pair(&app, &t).await;
    let sid = uuid();
    api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&dev),
        Some(batch(
            json!([{ "id": "cs2", "name": "Counter-Strike 2", "steam_appid": 730 }]),
            vec![session(&sid, "cs2", now() - 7200, 3600)],
        )),
    )
    .await;
    api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({ "game_id": "cs2", "minutes": 6000 })),
    )
    .await;
    api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({ "game_name": "Quake, \"Arena\"", "eras": [{ "from_year": 2000, "to_year": 2001, "hours_per_week": 5 }] }))).await;

    let r = api(&app, "GET", "/api/v1/export.zip", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.headers["content-type"], "application/zip");
    assert!(r.headers["content-disposition"]
        .to_str()
        .unwrap()
        .contains("fraglink-zipper-"));
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(r.bytes.clone())).unwrap();
    let mut names: Vec<String> = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "README.txt",
            "fraglink-account.json",
            "friends.json",
            "games.csv",
            "hours_entries.json",
            "profile.json",
            "sessions.csv",
            "steam_playtime.csv"
        ]
    );
    let read = |z: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, n: &str| {
        let mut s = String::new();
        z.by_name(n).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    let sessions = read(&mut z, "sessions.csv");
    assert_eq!(sessions.lines().count(), 2);
    assert!(sessions
        .lines()
        .nth(1)
        .unwrap()
        .starts_with(&format!("{sid},cs2,Counter-Strike 2,")));
    assert!(sessions.contains(",3600,0,tracked,0.2.0-test,"));
    let games = read(&mut z, "games.csv");
    let cs2 = games.lines().find(|l| l.starts_with("cs2,")).unwrap();
    assert!(
        cs2.starts_with("cs2,Counter-Strike 2,60,6000,0,6000,imported,60,"),
        "{cs2}"
    );
    assert!(games.contains("\"Quake, \"\"Arena\"\"\""), "{games}");
    let hours: Value = serde_json::from_str(&read(&mut z, "hours_entries.json")).unwrap();
    assert_eq!(hours.as_array().unwrap().len(), 2);
    let profile: Value = serde_json::from_str(&read(&mut z, "profile.json")).unwrap();
    assert_eq!(profile["user"]["handle"], "zipper");
    assert_eq!(profile["devices"].as_array().unwrap().len(), 1);
    let all = read(&mut z, "fraglink-account.json");
    assert!(!all.contains("password") && !all.contains("token_hash"));

    // one per hour
    let r = api(&app, "GET", "/api/v1/export.zip", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    // the JSON export stays unlimited
    assert_eq!(
        api(&app, "GET", "/api/v1/export", Some(&t), None)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/export.zip", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}
