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
    // without Admin fix-ups the file goes out exactly as written
    sqlx::query("DELETE FROM detection_aliases")
        .execute(&pool)
        .await
        .unwrap();
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
async fn detection_fixups_reach_the_apps(pool: PgPool) {
    sqlx::query(
        "INSERT INTO detection_aliases(kind, pattern, game_id) VALUES('path', 'world of warcraft forever/wow.exe', 'wow_forever')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let app = fraglink_server::router(state(pool));
    let r = send(&app, get("/api/v1/games/overrides", &[])).await;
    assert_eq!(r.status, StatusCode::OK);
    let f: fraglink_core::overrides::OverrideFile = serde_json::from_str(&r.text).unwrap();
    assert!(f
        .aliases
        .iter()
        .any(|a| a.from == "steam:793460" && a.to == "112_operator"));
    assert_eq!(f.rules[0].id, "wow_forever");
    let ov = fraglink_core::overrides::Overrides::from_json(&r.text).unwrap();
    assert_eq!(ov.resolve("steam:793460"), "112_operator");
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
    // Forever's real client is Wow.exe in `_forever_`: the rename's rules keep the
    // flavour folder, so they claim that path and leave retail's to the signatures
    let fe = o
        .match_process(
            "Wow.exe",
            Some(r"E:\Games\World of Warcraft\_forever_\Wow.exe"),
        )
        .expect("forever's Wow.exe");
    assert_eq!(
        (fe.id.as_str(), fe.name.as_str()),
        ("wow_forever", "World of Warcraft: Forever - Beta")
    );
    assert!(o
        .match_process(
            "Wow.exe",
            Some(r"E:\Games\World of Warcraft\_retail_\Wow.exe")
        )
        .is_none());
    // the Forever beta's WowB.exe is never Forever, whatever its folder
    for p in [
        r"E:\Games\World of Warcraft\_classic_beta_\WowB.exe",
        r"E:\Games\World of Warcraft\_forever_\WowB.exe",
    ] {
        let r = o.match_process("WowB.exe", Some(p)).expect("bundled rule");
        assert_eq!(r.id, "exe:wowb.exe", "{p}");
    }
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

/// Live since 2026-10-28: the WoW Forever beta (`wow_forever_beta`, WowB.exe) is fully retired
/// and its hours frozen. After a redeploy the overrides must not send anything to it (no
/// rename rule for its old folders) and its WowB.exe goes to the successor entry.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn fully_retired_game_publishes_no_rules_of_its_own(pool: PgPool) {
    sqlx::query("INSERT INTO game_overrides(game_id, name) VALUES('wow_forever_beta', 'World of Warcraft: Forever Beta')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO game_retirements(game_id, successor_id, exes) VALUES('wow_forever_beta', 'exe:wowb.exe', '{wowb.exe,wowb-64.exe,wow.exe}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let app = fraglink_server::router(state_with(pool, |c| c.data_dir = d.clone()));
    let r = send(&app, get("/api/v1/games/overrides", &[])).await;
    assert_eq!(r.status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&r.text).unwrap();
    let rules = v["rules"].as_array().unwrap();
    assert!(
        !rules
            .iter()
            .any(|r| r["id"] == "wow_forever_beta" || r["id"] == "wow_forever"),
        "{rules:?}"
    );
    let o = fraglink_core::overrides::Overrides::from_json(&r.text).unwrap();
    for exe in ["WowB.exe", "WowB-64.exe"] {
        let p = format!(r"E:\Games\World of Warcraft\_forever_\{exe}");
        assert_eq!(o.match_process(exe, Some(&p)).unwrap().id, "exe:wowb.exe");
    }
    // Wow.exe is retail's too: left to the signatures, never the successor's
    for f in ["_forever_", "_retail_"] {
        let p = format!(r"E:\Games\World of Warcraft\{f}\Wow.exe");
        assert!(o.match_process("Wow.exe", Some(&p)).is_none(), "{p}");
    }
}

/// Migration 0050 on production data: the retired beta that held the `wow_forever` id moves
/// to `wow_forever_beta` with every session and record, and `wow_forever` is left free (live
/// Forever, no hours, not retired). Running it again changes nothing.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_beta_moves_to_its_own_id(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    signup(&app, "fb@example.com", "fbeta").await;
    for q in [
        "INSERT INTO games(id, name) VALUES('wow_forever', 'World of Warcraft: Forever'), ('exe:wowb.exe', 'Unlisted game') ON CONFLICT (id) DO NOTHING",
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, 'wow_forever', now() - interval '3 hours', now() - interval '1 hour', 7200
         FROM users WHERE handle = 'fbeta'",
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, 'wow_forever', now() - interval '30 hours', now() - interval '29 hours', 1234
         FROM users WHERE handle = 'fbeta'",
        "INSERT INTO game_overrides(game_id, name) VALUES('wow_forever', 'World of Warcraft: Forever - Beta')",
        "INSERT INTO game_status(game_id, lifecycle) VALUES('wow_forever', 'ended')",
        "INSERT INTO game_retirements(game_id, successor_id, exes) VALUES('wow_forever', 'exe:wowb.exe', '{wowb.exe,wowb-64.exe}')",
    ] {
        sqlx::query(q).execute(&pool).await.unwrap();
    }
    let sql = include_str!("../migrations/0050_wow_forever_beta.sql");
    for _ in 0..2 {
        sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    }
    let hours = |id: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_as::<_, (i64, Option<i64>)>(
                "SELECT count(*), sum(duration_s)::bigint FROM sessions WHERE game_id = $1",
            )
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(hours("wow_forever_beta").await, (2, Some(8434)));
    assert_eq!(hours("wow_forever").await, (0, None));
    let one = |q: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(q)
                .fetch_optional(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        one("SELECT name FROM games WHERE id = 'wow_forever_beta'")
            .await
            .as_deref(),
        Some("World of Warcraft: Forever Beta")
    );
    assert_eq!(
        one("SELECT name FROM games WHERE id = 'wow_forever'")
            .await
            .as_deref(),
        Some("World of Warcraft: Forever")
    );
    assert_eq!(
        one("SELECT name FROM game_overrides WHERE game_id = 'wow_forever_beta'")
            .await
            .as_deref(),
        Some("World of Warcraft: Forever Beta")
    );
    assert_eq!(
        one("SELECT successor_id FROM game_retirements WHERE game_id = 'wow_forever_beta' AND NOT partial AND NOT merged").await.as_deref(),
        Some("exe:wowb.exe")
    );
    assert_eq!(
        one("SELECT lifecycle FROM game_status WHERE game_id = 'wow_forever_beta'")
            .await
            .as_deref(),
        Some("ended")
    );
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM game_overrides WHERE game_id = 'wow_forever')
              + (SELECT count(*) FROM game_status WHERE game_id = 'wow_forever')
              + (SELECT count(*) FROM game_retirements WHERE game_id = 'wow_forever')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, 0);
}
