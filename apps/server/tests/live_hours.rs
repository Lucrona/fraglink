//! Live hours: the game running now counts in totals right away, and exactly once after it
//! ends (presence.rs writes the session and clears the open one together).
mod common;
use common::*;
use fraglink_server::presence::{self, GameRef};
use serde_json::Value;
use sqlx::PgPool;

fn game(id: &str, hidden: bool) -> GameRef {
    GameRef {
        id: id.into(),
        name: None,
        started_at: None,
        detail: None,
        hidden,
    }
}

async fn totals(app: &axum::Router, tok: &str) -> (i64, i64, i64) {
    let p = api(app, "GET", "/api/v1/users/erin/profile", Some(tok), None)
        .await
        .json();
    let s = api(app, "GET", "/api/v1/friends/erin/summary", Some(tok), None)
        .await
        .json();
    let n = |v: &Value| v.as_i64().unwrap_or(0);
    (
        n(&p["overall"]["total_min"]),
        n(&p["overall"]["minutes_7d"]),
        n(&s["week_seconds"]) / 60,
    )
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn open_session_counts_live_and_once(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let tok = signup(&app, "erin@example.com", "erin").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'erin'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await, (0, 0, 0));

    presence::set_from(&st, uid, "erin", "online", Some(Some(game("wow", false))))
        .await
        .unwrap();
    // 20 minutes in, last heartbeat just now
    sqlx::query("UPDATE presence SET session_started_at = now() - interval '20 minutes', heartbeat_at = now() WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await, (20, 20, 20));
    let me: Value = api(&app, "GET", "/api/v1/me/games", Some(&tok), None)
        .await
        .json();
    assert!(me.to_string().contains("\"total_min\":20"), "{me}");

    // stale (no heartbeat for over 150 s): not counted until it's recorded
    sqlx::query(
        "UPDATE presence SET heartbeat_at = now() - interval '3 minutes' WHERE user_id = $1",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(totals(&app, &tok).await, (0, 0, 0));
    sqlx::query("UPDATE presence SET heartbeat_at = now() WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

    // game closes: the same 20 minutes, now as a stored session, not twice
    presence::set_from(&st, uid, "erin", "online", Some(None))
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await, (20, 20, 20));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let open: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT session_started_at FROM presence WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(open.is_none());

    // switching games records the old one and starts the new one without overlap
    presence::set_from(&st, uid, "erin", "online", Some(Some(game("wow", false))))
        .await
        .unwrap();
    sqlx::query("UPDATE presence SET session_started_at = now() - interval '10 minutes', heartbeat_at = now() WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await.0, 30);
    presence::set_from(
        &st,
        uid,
        "erin",
        "online",
        Some(Some(game("steam:570", false))),
    )
    .await
    .unwrap();
    assert_eq!(totals(&app, &tok).await.0, 30);

    // hidden game ("Show my game to my friends" off): counts once it ends, never live
    presence::set_from(&st, uid, "erin", "online", Some(Some(game("wow", true))))
        .await
        .unwrap();
    sqlx::query("UPDATE presence SET session_started_at = now() - interval '5 minutes', heartbeat_at = now() WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await.0, 30);
    presence::set_from(&st, uid, "erin", "offline", Some(None))
        .await
        .unwrap();
    assert_eq!(totals(&app, &tok).await.0, 35);
}
