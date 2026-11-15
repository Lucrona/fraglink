//! Catalogue change history: each games-list mutation writes the right entry
//! in the same transaction, and a failed edit writes none.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

async fn set_admin(pool: &PgPool, handle: &str) {
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = $1")
        .bind(handle)
        .execute(pool)
        .await
        .unwrap();
}

async fn count_hist(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM game_catalogue_history")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn actions(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT action FROM game_catalogue_history WHERE action <> 'history_started' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn last(pool: &PgPool) -> Value {
    let (action, summary, public, game_id, diff, by): (
        String,
        String,
        bool,
        Option<String>,
        Option<Value>,
        String,
    ) = sqlx::query_as(
        "SELECT action, summary, public, game_id, diff, actor_label
         FROM game_catalogue_history ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    json!({ "action": action, "summary": summary, "public": public, "game_id": game_id, "diff": diff, "by": by })
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn history_starts_with_a_marker_and_is_admin_only(pool: PgPool) {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_catalogue_history WHERE action = 'history_started'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
    let app = fraglink_server::router(state(pool.clone()));
    let user = signup(&app, "h@example.com", "histuser").await;
    let admin = signup(&app, "a@example.com", "histadmin").await;
    set_admin(&pool, "histadmin").await;
    assert_eq!(
        api(&app, "GET", "/api/v1/admin/games-history", None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_ne!(
        api(
            &app,
            "GET",
            "/api/v1/admin/games-history",
            Some(&user),
            None
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/games-history",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["total"], 1);
    assert_eq!(r.json()["entries"][0]["action"], "history_started");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn game_edits_write_field_diffs_and_failed_edits_write_none(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let admin = signup(&app, "e@example.com", "editadmin").await;
    set_admin(&pool, "editadmin").await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('steam:570', 'Dota 2', true)")
        .execute(&pool)
        .await
        .unwrap();
    let before = count_hist(&pool).await;

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:570",
        Some(&admin),
        Some(json!({"name": "DOTA Two", "hidden": false, "extra_exes": ["Dota2Beta.exe"], "aliases": ["Dota"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "edited");
    assert_eq!(e["by"], "editadmin");
    assert_eq!(e["game_id"], "steam:570");
    assert_eq!(e["diff"]["name"]["old"], Value::Null);
    assert_eq!(e["diff"]["name"]["new"], "DOTA Two");
    assert_eq!(e["diff"]["extra_exes"]["new"][0], "dota2beta.exe");
    assert_eq!(e["diff"]["aliases"]["new"][0], "Dota");
    assert_eq!(e["public"], true, "a rename is public-safe");

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:570",
        Some(&admin),
        Some(json!({"name": "DOTA Two", "hidden": true, "extra_exes": ["Dota2Beta.exe"], "aliases": ["Dota"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "hidden");
    assert_eq!(e["public"], false);
    assert_eq!(e["diff"]["hidden"]["old"], false);
    assert_eq!(e["diff"]["hidden"]["new"], true);

    // invalid exe: no new history row
    let n = count_hist(&pool).await;
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:570",
        Some(&admin),
        Some(json!({"name": "DOTA Two", "hidden": true, "extra_exes": ["not an exe"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(count_hist(&pool).await, n);
    // unknown game
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/nope:1",
        Some(&admin),
        Some(json!({"name": "Nope"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(count_hist(&pool).await, n);
    assert!(count_hist(&pool).await > before);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn approve_and_delete_and_failed_delete(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let admin = signup(&app, "ap@example.com", "apadmin").await;
    set_admin(&pool, "apadmin").await;
    api(
        &app,
        "POST",
        "/api/v1/games/suggest",
        None,
        Some(json!({"exe": "OddGame.exe", "name": "Odd Game"})),
    )
    .await;
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/suggestions/approve",
        Some(&admin),
        Some(json!({"exe": "oddgame.exe", "name": "Odd Game"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "added");
    assert_eq!(e["game_id"], "exe:oddgame.exe");
    assert_eq!(e["public"], true);
    assert!(e["summary"].as_str().unwrap().contains("Odd Game"));

    // unlisted deletable game
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('bogus', 'Bogus', false)")
        .execute(&pool)
        .await
        .unwrap();
    let n = count_hist(&pool).await;
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/bogus/delete",
        Some(&admin),
        Some(json!({"confirm": "nope"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(count_hist(&pool).await, n);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/bogus/delete",
        Some(&admin),
        Some(json!({"confirm": "bogus"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "removed");
    assert_eq!(e["game_id"], "bogus");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn merge_writes_history_and_failed_confirm_writes_none(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "mg@example.com", "mghist").await;
    set_admin(&pool, "mghist").await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('oldgame', 'Old Game', true), ('newgame', 'New Game', true)")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO game_overrides (game_id, extra_exes) VALUES ('oldgame', '{oldgame.exe}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'mghist'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s) VALUES (gen_random_uuid(), $1, 'oldgame', now() - interval '1 hour', now(), 3600)")
        .bind(uid).execute(&pool).await.unwrap();

    let n = count_hist(&pool).await;
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/oldgame/merge",
        Some(&t),
        Some(json!({"into": "newgame", "confirm": "nope"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(count_hist(&pool).await, n);
    let sessions: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = 'oldgame'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(sessions, 1, "a failed merge must not move sessions");

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/oldgame/merge",
        Some(&t),
        Some(json!({"into": "newgame", "confirm": "oldgame"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "merged");
    assert_eq!(e["game_id"], "newgame");
    assert_eq!(e["diff"]["from"], "oldgame");
    assert_eq!(e["diff"]["into"], "newgame");
    let moved: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = 'newgame'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(moved, 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn detection_rules_and_failed_rule_write_none(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let admin = signup(&app, "d@example.com", "detadmin").await;
    set_admin(&pool, "detadmin").await;
    sqlx::query(
        "INSERT INTO games (id, name, verified) VALUES ('marvel_rivals', 'Marvel Rivals', true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let n = count_hist(&pool).await;
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/detection",
        Some(&admin),
        Some(json!({"kind": "id", "pattern": "old_slug", "game_id": "nope"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(count_hist(&pool).await, n);

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/detection",
        Some(&admin),
        Some(json!({"kind": "id", "pattern": "old_slug", "game_id": "marvel_rivals"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let id = r.json()["id"].as_i64().unwrap();
    let e = last(&pool).await;
    assert_eq!(e["action"], "detection");
    assert_eq!(e["game_id"], "marvel_rivals");
    assert!(e["summary"].as_str().unwrap().contains("Added"));

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/detection/{id}"),
        Some(&admin),
        Some(json!({"game_id": "marvel_rivals", "note": "alias"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(last(&pool).await["summary"]
        .as_str()
        .unwrap()
        .contains("Changed"));

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/detection/{id}/delete"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(last(&pool).await["summary"]
        .as_str()
        .unwrap()
        .contains("Removed"));
    assert_eq!(
        actions(&pool)
            .await
            .iter()
            .filter(|a| *a == "detection")
            .count(),
        3
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn icon_and_media_and_retire(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let app = fraglink_server::router(state_with(pool.clone(), |c| {
        c.data_dir = dir.path().to_path_buf()
    }));
    let admin = signup(&app, "i@example.com", "iconadmin").await;
    set_admin(&pool, "iconadmin").await;
    sqlx::query(
        "INSERT INTO games (id, name, verified) VALUES ('hearthstone', 'Hearthstone', true)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let mut png = Vec::new();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([20, 30, 40, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/admin/games/hearthstone/icon")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "image/png")
        .body(axum::body::Body::from(png))
        .unwrap();
    let r = send(&app, req).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "icon");
    assert_eq!(e["public"], true);
    assert_eq!(e["game_id"], "hearthstone");

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/hearthstone/media",
        Some(&admin),
        Some(json!({"steam_appid": 1234})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let e = last(&pool).await;
    assert_eq!(e["action"], "edited");
    assert_eq!(e["diff"]["steam_appid"]["new"], 1234);

    // retire needs a known exe — extra exe first
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/hearthstone",
        Some(&admin),
        Some(json!({"hidden": false, "extra_exes": ["Hearthstone.exe"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/hearthstone/retire",
        Some(&admin),
        Some(json!({"exes": ["hearthstone.exe"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(last(&pool).await["action"], "retired");
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/hearthstone/unretire",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(last(&pool).await["action"], "unretired");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn list_sync_and_public_updates_and_manual_hide(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("detectable")).unwrap();
    let data = dir.path().to_path_buf();
    let st = state_with(pool.clone(), move |c| c.data_dir = data);
    let list1 = r#"[{"id":"1","name":"Alpha","executables":[{"name":"a.exe","os":"win32","is_launcher":false}]},
 {"id":"2","name":"Beta","executables":[{"name":"b.exe","os":"win32","is_launcher":false}]}]"#;
    let list2 = r#"[{"id":"1","name":"Alpha Two","executables":[{"name":"a.exe","os":"win32","is_launcher":false}]},
 {"id":"3","name":"Gamma","executables":[{"name":"c.exe","os":"win32","is_launcher":false}]}]"#;
    std::fs::write(dir.path().join("detectable/discord.json"), list1).unwrap();
    st.games
        .install(fraglink_server::games::build_from_cache(dir.path(), &st.sigs).unwrap());
    fraglink_server::all_games::track_list(&st).await;
    // baseline: no catalogue history besides the start marker
    assert!(actions(&pool).await.is_empty());

    std::fs::write(dir.path().join("detectable/discord.json"), list2).unwrap();
    st.games
        .install(fraglink_server::games::build_from_cache(dir.path(), &st.sigs).unwrap());
    fraglink_server::all_games::track_list(&st).await;
    let acts = actions(&pool).await;
    assert!(acts.contains(&"list_sync".into()), "{acts:?}");
    assert!(acts.contains(&"added".into()), "{acts:?}");
    assert!(acts.contains(&"renamed".into()), "{acts:?}");
    assert!(acts.contains(&"removed".into()), "{acts:?}");

    let app = fraglink_server::router(st.clone());
    let admin = signup(&app, "p@example.com", "pubadmin").await;
    set_admin(&pool, "pubadmin").await;

    let pubu = api(&app, "GET", "/api/v1/games/updates", None, None)
        .await
        .json();
    let ups = pubu["updates"].as_array().unwrap();
    assert!(!ups.is_empty());
    for u in ups {
        let a = u["action"].as_str().unwrap();
        assert!(
            matches!(
                a,
                "added" | "renamed" | "icon" | "manual" | "list_sync" | "steam_sync"
            ),
            "non-public action leaked: {a}"
        );
        assert!(u.get("by").is_none(), "actor must stay off the public feed");
        assert!(u.get("diff").is_none());
    }

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games-history",
        Some(&admin),
        Some(
            json!({"summary": "Added support notes for Gamma", "game_id": "gamma", "public": true}),
        ),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let id = r.json()["id"].as_i64().unwrap();
    let e = last(&pool).await;
    assert_eq!(e["action"], "manual");
    assert_eq!(e["public"], true);

    let pubu = api(&app, "GET", "/api/v1/games/updates", None, None)
        .await
        .json();
    assert_eq!(
        pubu["updates"][0]["summary"],
        "Added support notes for Gamma"
    );

    api(
        &app,
        "POST",
        &format!("/api/v1/admin/games-history/{id}/visibility"),
        Some(&admin),
        Some(json!({"hidden": true})),
    )
    .await;
    let pubu = api(&app, "GET", "/api/v1/games/updates", None, None)
        .await
        .json();
    let summaries: Vec<&str> = pubu["updates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["summary"].as_str().unwrap())
        .collect();
    assert!(!summaries.iter().any(|s| s.contains("support notes")));

    // filters
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/games-history?type=renamed",
        Some(&admin),
        None,
    )
    .await
    .json();
    assert!(r["total"].as_i64().unwrap() >= 1);
    for e in r["entries"].as_array().unwrap() {
        assert_eq!(e["action"], "renamed");
    }

    // catalogue: no hours or user fields, hidden games omitted, search works
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('shown', 'Zebra Quest', true), ('hideme', 'Secret Game', true)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO game_overrides (game_id, hidden) VALUES ('hideme', true)")
        .execute(&pool)
        .await
        .unwrap();
    let cat = api(&app, "GET", "/api/v1/games/catalogue?q=Zebra", None, None)
        .await
        .json();
    let games = cat["games"].as_array().unwrap();
    assert!(games.iter().any(|g| g["name"] == "Zebra Quest"));
    assert!(games.iter().all(|g| g.get("hours").is_none()
        && g.get("players").is_none()
        && g.get("total_min").is_none()));
    let all = api(&app, "GET", "/api/v1/games/catalogue?q=Secret", None, None)
        .await
        .json();
    assert_eq!(all["total"], 0, "hidden games stay off the public list");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn steam_sync_is_summarised_per_run(pool: PgPool) {
    // Local mock of the Steam Web API, same shape as tests/steam.rs.
    async fn vanity() -> axum::Json<Value> {
        axum::Json(json!({ "response": { "steamid": "76561197960287930", "success": 1 } }))
    }
    async fn owned() -> &'static str {
        include_str!("fixtures/steam_owned_games.json")
    }
    let mock = axum::Router::new()
        .route(
            "/ISteamUser/ResolveVanityURL/v1/",
            axum::routing::get(vanity),
        )
        .route(
            "/IPlayerService/GetOwnedGames/v1/",
            axum::routing::get(owned),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let base = format!("http://{addr}");
    let b = base.clone();
    let st = state_with(pool.clone(), move |c| {
        c.steam_api_key = Some("test-key-not-real".into());
        c.steam_api_base = b.clone();
        c.steam_openid_url = format!("{b}/openid/login");
    });
    let app = fraglink_server::router(st);
    let t = signup(&app, "s@example.com", "steamhist").await;
    let r = api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": "https://steamcommunity.com/id/gabelogannewell/" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n = count_hist(&pool).await;
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(count_hist(&pool).await > n);
    let e = last(&pool).await;
    assert_eq!(e["action"], "steam_sync");
    assert_eq!(e["by"], "system: steam sync");
    assert!(e["summary"].as_str().unwrap().starts_with("Steam sync:"));
    assert!(e["diff"]["total"].as_i64().unwrap() > 0);

    // unchanged re-import adds none
    sqlx::query("UPDATE linked_accounts SET last_sync_at = now() - interval '1 day'")
        .execute(&pool)
        .await
        .unwrap();
    let n = count_hist(&pool).await;
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        count_hist(&pool).await,
        n,
        "unchanged re-import writes no history"
    );
}
