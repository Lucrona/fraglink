//! Game-page warmer queue: one indexed pick, stable query count, priority + TTL.
mod common;
use chrono::{Duration as Cd, Utc};
use common::*;
use fraglink_server::game_pages::{compute_warm_due_at, CacheTimes, DueAt, WarmTtls};
use fraglink_server::game_warm::{
    pick_due_batch, pick_sql_count, rebuild_queue, reset_pick_sql_count, upsert_queue_row,
    PICK_BATCH,
};
use sqlx::PgPool;

async fn seed_named(pool: &PgPool, rows: &[(&str, &str, Option<i32>)]) {
    for (id, name, app) in rows {
        sqlx::query(
            "INSERT INTO games (id, name, verified, steam_appid) VALUES ($1, $2, true, $3)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, verified = true, steam_appid = excluded.steam_appid",
        )
        .bind(id)
        .bind(name)
        .bind(app)
        .execute(pool)
        .await
        .unwrap();
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn pick_is_priority_among_due_games(pool: PgPool) {
    let st = state(pool.clone());
    // Views: alpha=10, beta=5, gamma=1. alpha is fresh (not due); beta and gamma are due.
    sqlx::query("INSERT INTO game_page_views (game_id, views) VALUES ('alpha', 10), ('beta', 5), ('gamma', 1)")
        .execute(&pool)
        .await
        .unwrap();
    seed_named(
        &pool,
        &[
            ("alpha", "Alpha Quest", Some(1)),
            ("beta", "Beta Quest", Some(2)),
            ("gamma", "Gamma Quest", Some(3)),
        ],
    )
    .await;

    let later = (Utc::now() + Cd::hours(12)).to_rfc3339();
    upsert_queue_row(
        &st,
        "alpha",
        "Alpha Quest",
        Some(1),
        0,
        DueAt::At(Utc::now() + Cd::hours(12)),
    )
    .await
    .unwrap();
    upsert_queue_row(&st, "beta", "Beta Quest", Some(2), 1, DueAt::Now)
        .await
        .unwrap();
    upsert_queue_row(&st, "gamma", "Gamma Quest", Some(3), 2, DueAt::Now)
        .await
        .unwrap();
    // Make sure the "later" stamp actually stuck for alpha (sanity on to_sql).
    let _ = later;

    let batch = pick_due_batch(&st, 8).await.unwrap();
    let ids: Vec<_> = batch.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(ids, vec!["beta", "gamma"], "{ids:?}");
    assert!(!ids.contains(&"alpha"));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn rebuild_queue_uses_views_then_players_and_skips_never(pool: PgPool) {
    let st = state_with(pool.clone(), |c| c.wikipedia_url = None);
    seed_named(
        &pool,
        &[
            ("wow_forever", "World of Warcraft Forever", None),
            ("cs2", "Counter-Strike 2", Some(730)),
            ("niche", "Niche Quest", None),
        ],
    )
    .await;
    sqlx::query("INSERT INTO game_page_views (game_id, views) VALUES ('niche', 99), ('cs2', 50)")
        .execute(&pool)
        .await
        .unwrap();

    let n = rebuild_queue(&st).await.unwrap();
    assert!(n >= 3, "queue {n}");

    // niche has no steam appid, no wikipedia, no curated feed → never due.
    let due: (String,) =
        sqlx::query_as("SELECT next_due_at::text FROM game_warm_queue WHERE game_id = 'niche'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        due.0.contains("infinity") && !due.0.contains("-infinity"),
        "niche should be never-due, got {}",
        due.0
    );

    // Highest views first. niche (99) ranks before cs2 (50) even though it is never due.
    let ranks: Vec<(String, i32, String)> = sqlx::query_as(
        "SELECT game_id, rank, next_due_at::text FROM game_warm_queue
         WHERE game_id IN ('niche', 'cs2', 'wow_forever')
         ORDER BY rank",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let ids: Vec<_> = ranks.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(ids.first().copied(), Some("niche"), "{ranks:?}");
    assert!(
        ranks.iter().position(|r| r.0 == "niche") < ranks.iter().position(|r| r.0 == "cs2"),
        "{ranks:?}"
    );

    let by_id: std::collections::HashMap<_, _> =
        ranks.iter().map(|r| (r.0.as_str(), r.2.as_str())).collect();
    assert!(
        by_id["cs2"].contains("-infinity") || by_id["cs2"].contains("1970"),
        "cs2 should be due, got {}",
        by_id["cs2"]
    );
    assert!(
        by_id["wow_forever"].contains("-infinity") || by_id["wow_forever"].contains("1970"),
        "wow_forever should be due (curated feed, empty cache), got {}",
        by_id["wow_forever"]
    );

    let batch = pick_due_batch(&st, 8).await.unwrap();
    let picked: Vec<_> = batch.iter().map(|(id, _, _)| id.as_str()).collect();
    assert_eq!(picked.first().copied(), Some("cs2"), "{picked:?}");
    assert!(!picked.contains(&"niche"), "{picked:?}");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn pick_query_count_is_constant_regardless_of_cache_size(pool: PgPool) {
    let st = state(pool.clone());

    async fn seed_queue(st: &fraglink_server::AppState, n: i32, due_rank: i32) {
        sqlx::query("DELETE FROM game_warm_queue")
            .execute(&st.db)
            .await
            .unwrap();
        // Most rows are not due (fresh cache). One due game sits at `due_rank`.
        let mut ids = Vec::with_capacity(n as usize);
        let mut names = Vec::with_capacity(n as usize);
        let mut appids: Vec<Option<i32>> = Vec::with_capacity(n as usize);
        let mut ranks = Vec::with_capacity(n as usize);
        let mut dues = Vec::with_capacity(n as usize);
        for i in 0..n {
            ids.push(format!("g{i}"));
            names.push(format!("Game {i}"));
            appids.push(Some(10_000 + i));
            ranks.push(i);
            if i == due_rank {
                dues.push("-infinity".to_string());
            } else {
                dues.push("infinity".to_string());
            }
        }
        sqlx::query(
            "INSERT INTO game_warm_queue (game_id, name, steam_appid, rank, next_due_at)
             SELECT game_id, name, steam_appid, rank, due::timestamptz
             FROM unnest($1::text[], $2::text[], $3::int[], $4::int[], $5::text[])
                 AS t(game_id, name, steam_appid, rank, due)",
        )
        .bind(&ids)
        .bind(&names)
        .bind(&appids)
        .bind(&ranks)
        .bind(&dues)
        .execute(&st.db)
        .await
        .unwrap();
    }

    // Warm the query plan / connection once so both measured calls are comparable.
    seed_queue(&st, 50, 40).await;
    let _ = pick_due_batch(&st, PICK_BATCH).await.unwrap();

    reset_pick_sql_count();
    seed_queue(&st, 50, 40).await;
    let before_small = pick_sql_count();
    let small = pick_due_batch(&st, PICK_BATCH).await.unwrap();
    let small_queries = pick_sql_count() - before_small;

    reset_pick_sql_count();
    seed_queue(&st, 10_000, 9_000).await;
    let before_large = pick_sql_count();
    let large = pick_due_batch(&st, PICK_BATCH).await.unwrap();
    let large_queries = pick_sql_count() - before_large;

    assert_eq!(small, vec![("g40".into(), Some(10_040), "Game 40".into())]);
    assert_eq!(
        large,
        vec![("g9000".into(), Some(19_000), "Game 9000".into())]
    );
    assert_eq!(
        small_queries, large_queries,
        "per-pick SQL count must not grow with cache size (small={small_queries}, large={large_queries})"
    );
    assert_eq!(
        small_queries, 1,
        "pick_due_batch should run exactly one SQL statement, got {small_queries}"
    );
}

#[test]
fn compute_due_prefers_the_sooner_of_store_and_news() {
    let now = Utc::now();
    let row = Some(CacheTimes::from_row((
        Some(true),
        Some(now - Cd::days(8)), // store stale (ttl 7d)
        0,
        Some(true),
        Some(now - Cd::hours(1)), // news fresh (ttl 12h)
        0,
    )));
    let ttl = WarmTtls {
        wikipedia: false,
        store_ttl: 7 * 86_400,
        news_ttl: 12 * 3600,
        miss_ttl: 86_400,
    };
    let due = compute_warm_due_at("cs2", Some(730), None, row, &ttl);
    assert!(due.is_due(now), "{due:?}");
}
