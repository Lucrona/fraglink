mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

fn games() -> Value {
    json!([
        { "id": "cs2", "name": "Counter-Strike 2", "steam_appid": 730 },
        { "id": "manual:my-mod", "name": "My Mod", "steam_appid": null }
    ])
}

/// Session uploads were removed (hours only come from live presence): 410, nothing stored.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn uploads_are_gone(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let web = signup(&app, "s@example.com", "syncer").await;
    let dev = pair(&app, &web).await;
    let b = batch(games(), vec![session(&uuid(), "cs2", now() - 86400, 3600)]);
    for t in [Some(dev.as_str()), None] {
        let r = api(&app, "POST", "/api/v1/sessions/upload", t, Some(b.clone())).await;
        assert_eq!(r.status, StatusCode::GONE, "{}", r.text);
        assert_eq!(r.json()["error"], "uploads_removed");
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

/// A session as the server records it from live presence.
async fn live_session(pool: &PgPool, handle: &str, game: &str, start: i64, secs: i64) {
    sqlx::query("INSERT INTO games(id, name) VALUES($1, $1) ON CONFLICT (id) DO NOTHING")
        .bind(game)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
         SELECT gen_random_uuid(), id, $2, to_timestamp($3), to_timestamp($3 + $4), $4::int, 'live'
         FROM users WHERE handle = $1",
    )
    .bind(handle)
    .bind(game)
    .bind(start as f64)
    .bind(secs as f64)
    .execute(pool)
    .await
    .unwrap();
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn tiers_and_rankings(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await;
    let t = now() - 2 * 86400;
    let _ = (&a, &c);
    // alice: 2h tracked CS2
    live_session(&pool, "alice", "cs2", t, 7200).await;
    // bob: 1h tracked, plus a huge self-reported counter and an old (pre-removal) estimate
    live_session(&pool, "bob", "cs2", t, 3600).await;
    let _ = &b;
    seed_imported(&pool, "bob", "cs2", "cs2", 600000).await;
    // carol: an old estimate only
    for h in ["bob", "carol"] {
        sqlx::query("INSERT INTO hours_entries (id, user_id, game_id, tier, source, minutes) SELECT gen_random_uuid(), id, 'cs2', 'estimated', 'estimator', 1123200 FROM users WHERE handle = $1")
            .bind(h)
            .execute(&pool)
            .await
            .unwrap();
    }
    // Steam counter for alice larger than her sessions: tracked = max(sessions, steam)
    let alice: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO steam_playtime(user_id, steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at) VALUES($1, 730, 'cs2', 300, 0, now())")
        .bind(alice).execute(&pool).await.unwrap();

    // Leaderboards count like profiles (Oct 2026): all time is display_min (largest of sessions,
    // Steam and Imported); 7 days counts Imported only by how much the counter grew this week.
    let ranks = |period: &'static str| {
        let app = app.clone();
        let c = c.clone();
        async move {
            let r = api(
                &app,
                "GET",
                &format!("/api/v1/leaderboard/cs2?period={period}"),
                Some(&c),
                None,
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text);
            r.json()["ranks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    (
                        r["handle"].as_str().unwrap().to_string(),
                        r["minutes"].as_i64().unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        }
    };
    let all = ranks("all").await;
    assert_eq!(
        all,
        vec![("bob".to_string(), 600000), ("alice".to_string(), 300)],
        "old estimates never rank"
    );
    // a first import isn't this week's play: 7 days is still the app's sessions
    let week = ranks("7d").await;
    assert_eq!(
        week,
        vec![("alice".to_string(), 120), ("bob".to_string(), 60)]
    );
    // bob's counter was 599,700 three days ago: it grew 300 minutes this week, which beats his
    // 60 minutes of sessions (the larger one counts, not both)
    sqlx::query("INSERT INTO hours_entries (id, user_id, game_id, tier, source, minutes, created_at, captured_at) SELECT gen_random_uuid(), id, 'cs2', 'imported', 'manual', 599700, now() - interval '3 days', now() - interval '3 days' FROM users WHERE handle = 'bob'")
        .execute(&pool).await.unwrap();
    let week = ranks("7d").await;
    assert_eq!(
        week,
        vec![("bob".to_string(), 300), ("alice".to_string(), 120)]
    );
    // turning Leaderboards off takes you off the board
    sqlx::query("UPDATE users SET privacy = privacy || '{\"leaderboards\": false}'::jsonb WHERE handle = 'bob'")
        .execute(&pool).await.unwrap();
    assert_eq!(ranks("all").await, vec![("alice".to_string(), 300)]);
    assert_eq!(
        api(&app, "GET", "/api/v1/leaderboard/cs2", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            &app,
            "GET",
            "/api/v1/leaderboard/cs2?period=year",
            Some(&a),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );

    // bob's own stats show both tiers separately; the old estimate is left out
    let s = api(&app, "GET", "/api/v1/stats", Some(&b), None)
        .await
        .json();
    let g = &s["games"][0];
    assert_eq!(g["game_id"], "cs2");
    assert_eq!(g["tracked_min"], 60);
    assert_eq!(g["imported_min"], 600000);
    assert_eq!(g["display_min"], 600000);
    assert_eq!(g["display_tier"], "imported");
    assert_eq!(g["estimated_min"], 0);
    assert_eq!(s["overall"]["verified_min"], 600000);
    assert_eq!(s["overall"]["estimated_min"], 0);
    assert!(s["overall"].get("estimated_display").is_none());
    // carol (estimate only) has nothing to show
    let s = api(&app, "GET", "/api/v1/stats", Some(&c), None)
        .await
        .json();
    assert_eq!(s["games"].as_array().unwrap().len(), 0, "{s}");
    let s = api(&app, "GET", "/api/v1/stats", Some(&a), None)
        .await
        .json();
    assert_eq!(s["games"][0]["display_tier"], "tracked");
    assert_eq!(s["games"][0]["display_min"], 300);
    assert_eq!(s["games"][0]["minutes_7d"], 120);
}

async fn display_min(pool: &PgPool, handle: &str, game: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT g.display_min FROM user_game_stats g JOIN users u ON u.id = g.user_id
         WHERE u.handle = $1 AND g.game_id = $2",
    )
    .bind(handle)
    .bind(game)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Import 10h, then play 5h: shows 15h. Only the part of a session after the import counts,
/// and a fresh import (which already includes that play) replaces the add-on.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn play_after_import_adds_on(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    signup(&app, "w@example.com", "wower").await;
    let id = seed_imported(&pool, "wower", "wow", "World of Warcraft", 600).await;
    sqlx::query("UPDATE hours_entries SET captured_at = now() - interval '6 hours' WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(display_min(&pool, "wower", "wow").await, 600);
    // 5h played after the import, plus a session straddling it (1h 30m before, 30m after)
    live_session(&pool, "wower", "wow", now() - 5 * 3600 - 60, 5 * 3600).await;
    live_session(&pool, "wower", "wow", now() - 7 * 3600 - 1800, 7200).await;
    assert_eq!(display_min(&pool, "wower", "wow").await, 600 + 300 + 30);
    // app-tracked play counts on the all-time leaderboard too (same rule as the total)
    let viewer = signup(&app, "v@example.com", "viewer").await;
    let r = api(
        &app,
        "GET",
        "/api/v1/leaderboard/wow?period=all",
        Some(&viewer),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["ranks"][0]["minutes"], 930, "{}", r.text);

    // re-import: the counter now includes that play, so it replaces the add-on
    seed_imported(&pool, "wower", "wow", "World of Warcraft", 930).await;
    assert_eq!(display_min(&pool, "wower", "wow").await, 930);
    // and later play adds on top again
    live_session(&pool, "wower", "wow", now() + 10, 3600).await;
    assert_eq!(display_min(&pool, "wower", "wow").await, 990);
}

/// Steam is imported by hand: play the app tracks after an import adds on top of Steam's counter, and the
/// next sync (which already includes it) replaces the add-on. Never counted twice.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn play_after_steam_sync_adds_on(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    signup(&app, "st@example.com", "steamer").await;
    live_session(&pool, "steamer", "cs2", now() - 30 * 86400, 60).await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'steamer'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let sync = |m: i32, ago_h: i32| {
        let pool = pool.clone();
        async move {
            sqlx::query("INSERT INTO steam_playtime(user_id, steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at)
                VALUES($1, 730, 'cs2', $2, 0, now() - make_interval(hours => $3))
                ON CONFLICT (user_id, steam_appid) DO UPDATE SET minutes_forever = excluded.minutes_forever, fetched_at = excluded.fetched_at")
                .bind(uid).bind(m).bind(ago_h).execute(&pool).await.unwrap();
        }
    };
    sync(600, 6).await;
    assert_eq!(display_min(&pool, "steamer", "cs2").await, 600);
    live_session(&pool, "steamer", "cs2", now() - 3 * 3600, 2 * 3600).await;
    assert_eq!(display_min(&pool, "steamer", "cs2").await, 720);
    sync(720, 0).await;
    assert_eq!(display_min(&pool, "steamer", "cs2").await, 720);
}
