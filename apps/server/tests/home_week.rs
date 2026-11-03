//! Home "this week": the headline hours, the daily bars, the per-game weekly totals and the
//! friends leaderboard come from one function (`fraglink_server::week`) and must agree,
//! including sessions across local midnight, across the start of the week and across a
//! daylight-saving change.
mod common;
use chrono::{DateTime, TimeZone, Utc};
use chrono_tz::Australia::Sydney;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

async fn befriend(app: &axum::Router, a: &str, b: &str, bh: &str, ah: &str) {
    for (t, h) in [(a, bh), (b, ah)] {
        api(
            app,
            "POST",
            "/api/v1/friends/request",
            Some(t),
            Some(json!({ "handle": h })),
        )
        .await;
    }
}

async fn sess(pool: &PgPool, handle: &str, game: &str, start: &str, end: &str, secs: i32) {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, $2, $3::timestamptz, $4::timestamptz, $5 FROM users WHERE handle = $1",
    )
    .bind(handle)
    .bind(game)
    .bind(start)
    .bind(end)
    .bind(secs)
    .execute(pool)
    .await
    .unwrap();
}

fn n(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// Headline = sum of days = sum of games, in seconds, in minutes and in the tenths on screen.
fn assert_consistent(v: &Value) {
    let days = v["days"].as_array().unwrap();
    let games = v["games"].as_array().unwrap();
    assert_eq!(days.len(), 7);
    for k in ["seconds", "tenths", "minutes"] {
        let d: i64 = days.iter().map(|d| n(&d[k])).sum();
        assert_eq!(n(&v[k]), d, "{k}: headline vs days: {v}");
    }
    for k in ["seconds", "tenths"] {
        let g: i64 = games.iter().map(|g| n(&g[k])).sum();
        assert_eq!(n(&v[k]), g, "{k}: headline vs games: {v}");
    }
    let me = v["board"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["me"] == true)
        .unwrap();
    assert_eq!(n(&me["seconds"]), n(&v["seconds"]));
    assert_eq!(n(&me["tenths"]), n(&v["tenths"]));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn week_adds_up_across_midnight_week_start_and_dst(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let d = signup(&app, "d@example.com", "dave").await;
    signup(&app, "c@example.com", "carol").await;
    befriend(&app, &a, &b, "bob", "alice").await;
    befriend(&app, &a, &d, "dave", "alice").await;
    sqlx::query("UPDATE users SET privacy = '{\"games\": false}' WHERE handle = 'dave'")
        .execute(&pool)
        .await
        .unwrap();
    for g in ["wow", "cs2", "quake", "secret", "secret2"] {
        sqlx::query("INSERT INTO games (id, name) VALUES ($1, $1) ON CONFLICT DO NOTHING")
            .bind(g)
            .execute(&pool)
            .await
            .unwrap();
    }
    // Window: Wed 30 Sep .. Tue 6 Oct 2026 (Sydney), now = Tue 6 Oct 12:00 AEDT.
    // Daylight saving starts Sun 4 Oct (02:00 AEST -> 03:00 AEDT).
    // across the week start: only 00:00-02:00 Wed counts (2 h of 4)
    sess(
        &pool,
        "alice",
        "wow",
        "2026-09-29 22:00+10",
        "2026-09-30 02:00+10",
        4 * 3600,
    )
    .await;
    // across local midnight Thu -> Fri: 30 m + 45 m
    sess(
        &pool,
        "alice",
        "cs2",
        "2026-10-01 23:30+10",
        "2026-10-02 00:45+10",
        75 * 60,
    )
    .await;
    // idle trimmed: 3 h on the clock, 1 h played, counted as the last hour (Sat)
    sess(
        &pool,
        "alice",
        "quake",
        "2026-10-02 22:00+10",
        "2026-10-03 01:00+10",
        3600,
    )
    .await;
    // across the DST jump: 01:30 AEST -> 04:30 AEDT is 2 real hours (Sun)
    sess(
        &pool,
        "alice",
        "wow",
        "2026-10-04 01:30+10",
        "2026-10-04 04:30+11",
        2 * 3600,
    )
    .await;
    // odd seconds (Mon) and a game alice hid from others (still counts for her)
    sess(
        &pool,
        "alice",
        "cs2",
        "2026-10-05 10:00:00+11",
        "2026-10-05 10:17:13+11",
        1033,
    )
    .await;
    sess(
        &pool,
        "alice",
        "secret",
        "2026-10-05 20:00+11",
        "2026-10-05 21:00+11",
        3600,
    )
    .await;
    // today (Tue) up to 11:20
    sess(
        &pool,
        "alice",
        "wow",
        "2026-10-06 09:00+11",
        "2026-10-06 11:20+11",
        140 * 60,
    )
    .await;
    // outside: before the week, and after "now"
    sess(
        &pool,
        "alice",
        "wow",
        "2026-09-28 10:00+10",
        "2026-09-28 12:00+10",
        7200,
    )
    .await;
    sess(
        &pool,
        "alice",
        "wow",
        "2026-10-06 13:00+11",
        "2026-10-06 14:00+11",
        3600,
    )
    .await;
    // friends: bob 3 h (+1 h of a game he hid), dave hides games, carol isn't a friend
    sess(
        &pool,
        "bob",
        "wow",
        "2026-10-05 18:00+11",
        "2026-10-05 21:00+11",
        3 * 3600,
    )
    .await;
    sess(
        &pool,
        "bob",
        "secret2",
        "2026-10-03 18:00+10",
        "2026-10-03 19:00+10",
        3600,
    )
    .await;
    sess(
        &pool,
        "dave",
        "wow",
        "2026-10-05 18:00+11",
        "2026-10-05 22:00+11",
        4 * 3600,
    )
    .await;
    sess(
        &pool,
        "carol",
        "wow",
        "2026-10-05 18:00+11",
        "2026-10-05 23:00+11",
        5 * 3600,
    )
    .await;
    sqlx::query(
        "INSERT INTO hidden_games (user_id, game_id) SELECT id, g FROM users,
         (VALUES ('alice', 'secret'), ('bob', 'secret2')) v(h, g) WHERE handle = v.h",
    )
    .execute(&pool)
    .await
    .unwrap();

    let alice: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let now: DateTime<Utc> = Sydney
        .with_ymd_and_hms(2026, 10, 6, 12, 0, 0)
        .unwrap()
        .with_timezone(&Utc);
    let v = fraglink_server::home::week_json(&st, alice, "Australia/Sydney", now)
        .await
        .unwrap();
    println!("week: {v}");
    assert_consistent(&v);

    let days: Vec<i64> = v["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| n(&d["seconds"]))
        .collect();
    assert_eq!(days, vec![7200, 1800, 2700, 3600, 7200, 4633, 8400]);
    assert_eq!(v["days"][0]["date"], "2026-09-30");
    assert_eq!(v["days"][6]["date"], "2026-10-06");
    assert_eq!(v["days"][6]["today"], true);
    assert_eq!(n(&v["seconds"]), 35533); // 9h 52m 13s
    assert_eq!(n(&v["tenths"]), 99); // shown as 9.9 h
    let tenths: Vec<i64> = v["days"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| n(&d["tenths"]))
        .collect();
    assert_eq!(tenths.iter().sum::<i64>(), 99, "{tenths:?}");
    let g: std::collections::HashMap<String, i64> = v["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| (g["game_id"].as_str().unwrap().to_string(), n(&g["seconds"])))
        .collect();
    assert_eq!(g["wow"], 7200 + 7200 + 8400);
    assert_eq!(g["cs2"], 1800 + 2700 + 1033);
    assert_eq!(g["quake"], 3600);
    assert_eq!(g["secret"], 3600);
    assert_eq!(v["games"][0]["game_id"], "wow"); // most recently played first
    assert_eq!(v["top_game"]["game_id"], "wow");
    assert_eq!(v["start"], "2026-09-29T14:00:00Z"); // Wed 00:00 AEST
    assert_eq!(v["label"], "Wed 30 Sep – Tue 6 Oct");

    let board: Vec<(String, i64)> = v["board"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["handle"].as_str().unwrap().to_string(), n(&r["seconds"])))
        .collect();
    assert_eq!(board, vec![("alice".into(), 35533), ("bob".into(), 10800)]);

    // same instant seen from UTC: different day edges, same rule, still adds up
    let u = fraglink_server::home::week_json(&st, alice, "UTC", now)
        .await
        .unwrap();
    assert_consistent(&u);
    assert_eq!(u["days"][6]["date"], "2026-10-06");
    assert_eq!(u["start"], "2026-09-30T00:00:00Z");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn week_endpoint_counts_the_game_running_now(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let a = signup(&app, "a@example.com", "alice").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games (id, name) VALUES ('wow', 'World of Warcraft')")
        .execute(&pool)
        .await
        .unwrap();
    sess(&pool, "alice", "wow", "now", "now", 0).await; // zero length: ignored
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         VALUES (gen_random_uuid(), $1, 'wow', now() - interval '3 hours', now() - interval '2 hours', 3600)",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    fraglink_server::presence::set_from(
        &st,
        uid,
        "alice",
        "online",
        Some(Some(fraglink_server::presence::GameRef {
            id: "wow".into(),
            name: None,
            started_at: None,
            detail: None,
            hidden: false,
        })),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE presence SET session_started_at = now() - interval '20 minutes', heartbeat_at = now() WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "GET",
        "/api/v1/home/week?tz=Mars/Olympus",
        Some(&a),
        None,
    )
    .await;
    let v = r.json();
    assert_eq!(v["tz"], "Australia/Sydney", "{v}"); // unknown zone: site default
    assert_consistent(&v);
    let s = n(&v["seconds"]);
    // 1 h stored (may straddle a local midnight) + ~20 min live
    assert!((3600 + 20 * 60 - 5..=3600 + 21 * 60).contains(&s), "{v}");
    assert_eq!(v["games"].as_array().unwrap().len(), 1);
    assert_eq!(
        api(&app, "GET", "/api/v1/home/week", None, None)
            .await
            .status,
        axum::http::StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn activity_per_session_has_live_and_finished_rows_once(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    befriend(&app, &a, &b, "bob", "alice").await;
    sqlx::query("INSERT INTO games (id, name) VALUES ('wow', 'World of Warcraft'), ('cs2', 'Counter-Strike 2')")
        .execute(&pool)
        .await
        .unwrap();
    let bob: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'bob'")
        .fetch_one(&pool)
        .await
        .unwrap();
    // two sessions of the same game the same day, one of them stored twice
    for (s, e, secs) in [
        ("5 hours", "4 hours", 3600),
        ("3 hours", "2 hours", 1800),
        ("3 hours", "2 hours", 1800),
    ] {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             VALUES (gen_random_uuid(), $1, 'wow', date_trunc('second', now()) - $2::interval, now() - $3::interval, $4)",
        )
        .bind(bob)
        .bind(s)
        .bind(e)
        .bind(secs)
        .execute(&pool)
        .await
        .unwrap();
    }
    // playing cs2 now
    fraglink_server::presence::set_from(
        &st,
        bob,
        "bob",
        "online",
        Some(Some(fraglink_server::presence::GameRef {
            id: "cs2".into(),
            name: None,
            started_at: None,
            detail: None,
            hidden: false,
        })),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE presence SET session_started_at = date_trunc('second', now()) - interval '30 minutes', heartbeat_at = now() WHERE user_id = $1")
        .bind(bob)
        .execute(&pool)
        .await
        .unwrap();
    // the same live session already written once (e.g. a reconnect): must not show twice
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         VALUES (gen_random_uuid(), $1, 'cs2', date_trunc('second', now()) - interval '30 minutes', now() - interval '10 minutes', 1200)",
    )
    .bind(bob)
    .execute(&pool)
    .await
    .unwrap();
    let v = api(
        &app,
        "GET",
        "/api/v1/friends/activity?per=session",
        Some(&a),
        None,
    )
    .await
    .json();
    let rows: Vec<(String, String, Option<i64>)> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["kind"].as_str().unwrap().to_string(),
                i["game_id"].as_str().unwrap().to_string(),
                i["duration_s"].as_i64(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            ("playing".into(), "cs2".into(), None),
            ("played".into(), "wow".into(), Some(1800)),
            ("played".into(), "wow".into(), Some(3600)),
        ],
        "{v}"
    );
    // the friends page keeps its per-game-per-day rows
    let v = api(&app, "GET", "/api/v1/friends/activity", Some(&a), None)
        .await
        .json();
    let played: Vec<&Value> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "played")
        .collect();
    assert!(
        played
            .iter()
            .any(|i| i["game_id"] == "wow" && (i["sessions"] == 3 || i["sessions"] == 2)),
        "{v}"
    );
}
