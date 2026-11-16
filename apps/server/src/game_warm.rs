//! Background warmer: walk the catalogue (most-played / most-viewed first)
//! and fill store details, news, and Wikipedia fallbacks within the shared
//! outbound rate cap, with refresh TTLs and retry/backoff.
//!
//! The next due game is picked with one indexed query against `game_warm_queue`
//! (priority `rank` + `next_due_at`). The queue is rebuilt on a timer, not
//! before every fetch, so DB work stays flat as the cache grows.

use crate::game_pages::{self, compute_warm_due_at, CacheTimes, DueAt, WarmTtls};
use crate::AppState;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::time::{sleep, Instant};

/// How many due games to claim in one pick query.
pub const PICK_BATCH: i64 = 8;
/// Rebuild ranks from play/view counts at most this often.
const QUEUE_REBUILD: Duration = Duration::from_secs(15 * 60);

/// Test instrumentation: incremented once per `pick_due_batch` SQL statement.
static PICK_SQL: AtomicU64 = AtomicU64::new(0);

pub fn pick_sql_count() -> u64 {
    PICK_SQL.load(Ordering::SeqCst)
}

pub fn reset_pick_sql_count() {
    PICK_SQL.store(0, Ordering::SeqCst);
}

/// Started once from `main`. No-ops when `FRAGLINK_GAME_WARMER=off`.
pub async fn warm_loop(st: AppState) {
    if !st.cfg.game_warmer {
        tracing::info!("game page warmer is off");
        return;
    }
    sleep(Duration::from_secs(3)).await;
    tracing::info!(
        "game page warmer started (cap {}/min, store ttl {}s, news ttl {}s)",
        st.cfg.game_feed_per_min,
        st.cfg.game_store_ttl_secs,
        st.cfg.game_warmer_news_ttl_secs
    );
    if let Err(e) = rebuild_queue(&st).await {
        tracing::debug!("game warmer queue: {e}");
    }
    let mut last_rebuild = Instant::now();
    let mut idle_passes = 0u32;
    loop {
        if last_rebuild.elapsed() >= QUEUE_REBUILD {
            if let Err(e) = rebuild_queue(&st).await {
                tracing::debug!("game warmer queue: {e}");
            }
            last_rebuild = Instant::now();
        }
        match warm_one(&st).await {
            Ok(true) => {
                idle_passes = 0;
            }
            Ok(false) => {
                idle_passes = idle_passes.saturating_add(1);
                let wait = if idle_passes < 3 { 15 } else { 60 };
                sleep(Duration::from_secs(wait)).await;
            }
            Err(e) => {
                tracing::debug!("game warmer: {e}");
                sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Refresh the next due batch. Returns whether work was done.
async fn warm_one(st: &AppState) -> anyhow::Result<bool> {
    let batch = pick_due_batch(st, PICK_BATCH).await?;
    if batch.is_empty() {
        return Ok(false);
    }
    let mut did = false;
    for (id, appid, name) in batch {
        let start = Instant::now();
        while !game_pages::gate_available(st) {
            if start.elapsed() > Duration::from_secs(30) {
                return Ok(did);
            }
            sleep(Duration::from_millis(500)).await;
        }
        game_pages::refresh_game(st, &id, appid, Some(&name)).await?;
        did = true;
    }
    Ok(did)
}

/// One indexed query: due games (`next_due_at <= now()`), highest priority first.
pub async fn pick_due_batch(
    st: &AppState,
    limit: i64,
) -> anyhow::Result<Vec<(String, Option<u32>, String)>> {
    PICK_SQL.fetch_add(1, Ordering::SeqCst);
    let rows: Vec<(String, Option<i32>, String)> = sqlx::query_as(
        "SELECT game_id, steam_appid, name
         FROM game_warm_queue
         WHERE next_due_at <= now()
         ORDER BY rank
         LIMIT $1",
    )
    .bind(limit.max(1))
    .fetch_all(&st.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, app, name)| (id, app.map(|n| n as u32), name))
        .collect())
}

/// Most-played and most-viewed first, then the rest of the catalogue.
pub async fn warm_order(st: &AppState) -> anyhow::Result<Vec<(String, Option<u32>, String)>> {
    Ok(warm_order_ranked(st)
        .await?
        .into_iter()
        .map(|r| (r.id, r.appid, r.name))
        .collect())
}

struct Ranked {
    id: String,
    appid: Option<u32>,
    name: String,
}

async fn warm_order_ranked(st: &AppState) -> anyhow::Result<Vec<Ranked>> {
    let cat = match crate::game_history::public_catalogue(st).await {
        Ok(c) => c,
        Err(_) => return Ok(Vec::new()),
    };
    let played: HashMap<String, (i64, i64)> = sqlx::query_as(
        "SELECT s.game_id, count(DISTINCT s.user_id)::bigint, coalesce(sum(s.duration_s), 0)::bigint
         FROM sessions s
         JOIN users u ON u.id = s.user_id
         WHERE coalesce(u.is_test, false) = false
         GROUP BY s.game_id",
    )
    .fetch_all(&st.db)
    .await?
    .into_iter()
    .map(|(id, players, secs): (String, i64, i64)| (id, (players, secs)))
    .collect();
    let views: HashMap<String, i64> = sqlx::query_as("SELECT game_id, views FROM game_page_views")
        .fetch_all(&st.db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(id, n): (String, i64)| (id, n))
        .collect();

    let mut rows: Vec<(i64, i64, i64, usize, Ranked)> = cat
        .into_iter()
        .enumerate()
        .map(|(i, g)| {
            let (players, secs) = played.get(&g.id).copied().unwrap_or((0, 0));
            let v = views.get(&g.id).copied().unwrap_or(0);
            // Higher is sooner: views, then distinct players, then hours, then catalogue order.
            (
                v,
                players,
                secs,
                usize::MAX - i,
                Ranked {
                    id: g.id,
                    appid: g.steam_appid,
                    name: g.name,
                },
            )
        })
        .collect();
    rows.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(b.2.cmp(&a.2))
            .then(b.3.cmp(&a.3))
    });
    Ok(rows.into_iter().map(|(_, _, _, _, r)| r).collect())
}

/// Rebuild ranks and due times for the whole catalogue. Not on the per-fetch path.
pub async fn rebuild_queue(st: &AppState) -> anyhow::Result<usize> {
    let order = warm_order_ranked(st).await?;
    if order.is_empty() {
        sqlx::query("DELETE FROM game_warm_queue")
            .execute(&st.db)
            .await?;
        return Ok(0);
    }

    let cache: Vec<(
        String,
        Option<bool>,
        Option<DateTime<Utc>>,
        i32,
        Option<bool>,
        Option<DateTime<Utc>>,
        i32,
    )> = sqlx::query_as(
        "SELECT game_id, details_ok, details_fetched_at, details_fail_count,
                news_ok, news_fetched_at, news_fail_count
         FROM game_feed_cache",
    )
    .fetch_all(&st.db)
    .await?;
    let cache: HashMap<
        String,
        (
            Option<bool>,
            Option<DateTime<Utc>>,
            i32,
            Option<bool>,
            Option<DateTime<Utc>>,
            i32,
        ),
    > = cache
        .into_iter()
        .map(|(id, a, b, c, d, e, f)| (id, (a, b, c, d, e, f)))
        .collect();

    let meta: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT game_id, news_feed_url FROM game_page_meta")
            .fetch_all(&st.db)
            .await
            .unwrap_or_default();
    let meta: HashMap<String, Option<String>> = meta.into_iter().collect();

    let ttl = WarmTtls {
        wikipedia: st.cfg.wikipedia_url.is_some(),
        store_ttl: st.cfg.game_store_ttl_secs,
        news_ttl: st.cfg.game_warmer_news_ttl_secs,
        miss_ttl: st.cfg.game_feed_miss_ttl_secs,
    };

    let mut ids: Vec<String> = Vec::with_capacity(order.len());
    let mut names: Vec<String> = Vec::with_capacity(order.len());
    let mut appids: Vec<Option<i32>> = Vec::with_capacity(order.len());
    let mut ranks: Vec<i32> = Vec::with_capacity(order.len());
    let mut dues: Vec<String> = Vec::with_capacity(order.len());

    for (rank, g) in order.into_iter().enumerate() {
        let admin = meta.get(&g.id).and_then(|s| s.as_deref());
        let row = cache.get(&g.id).cloned().map(CacheTimes::from_row);
        let due = compute_warm_due_at(&g.id, g.appid, admin, row, &ttl);
        ids.push(g.id);
        names.push(g.name);
        appids.push(g.appid.map(|n| n as i32));
        ranks.push(rank as i32);
        dues.push(due.to_sql());
    }

    sqlx::query(
        "INSERT INTO game_warm_queue (game_id, name, steam_appid, rank, next_due_at)
         SELECT game_id, name, steam_appid, rank, due::timestamptz
         FROM unnest($1::text[], $2::text[], $3::int[], $4::int[], $5::text[])
             AS t(game_id, name, steam_appid, rank, due)
         ON CONFLICT (game_id) DO UPDATE SET
            name = excluded.name,
            steam_appid = excluded.steam_appid,
            rank = excluded.rank,
            next_due_at = excluded.next_due_at",
    )
    .bind(&ids)
    .bind(&names)
    .bind(&appids)
    .bind(&ranks)
    .bind(&dues)
    .execute(&st.db)
    .await?;

    sqlx::query("DELETE FROM game_warm_queue WHERE NOT (game_id = ANY($1::text[]))")
        .bind(&ids)
        .execute(&st.db)
        .await?;

    Ok(ids.len())
}

/// Insert helper used by tests: seed a queue row without a full catalogue rebuild.
pub async fn upsert_queue_row(
    st: &AppState,
    game_id: &str,
    name: &str,
    appid: Option<u32>,
    rank: i32,
    due: DueAt,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO game_warm_queue (game_id, name, steam_appid, rank, next_due_at)
         VALUES ($1, $2, $3, $4, $5::timestamptz)
         ON CONFLICT (game_id) DO UPDATE SET
            name = excluded.name,
            steam_appid = excluded.steam_appid,
            rank = excluded.rank,
            next_due_at = excluded.next_due_at",
    )
    .bind(game_id)
    .bind(name)
    .bind(appid.map(|n| n as i32))
    .bind(rank)
    .bind(due.to_sql())
    .execute(&st.db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_pages::{field_due_at, WarmTtls};
    use chrono::{Duration as Cd, Utc};

    #[test]
    fn sort_priority_is_views_then_players() {
        // Documented in warm_order: (views, players, secs, ~index).
        let mut rows = [(0i64, 10i64, 100i64, 1u8), (5, 1, 1, 2), (5, 3, 1, 3)];
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        assert_eq!(rows[0], (5, 3, 1, 3));
        assert_eq!(rows[1], (5, 1, 1, 2));
        assert_eq!(rows[2], (0, 10, 100, 1));
    }

    fn ttls() -> WarmTtls {
        WarmTtls {
            wikipedia: false,
            store_ttl: 7 * 86_400,
            news_ttl: 12 * 3600,
            miss_ttl: 86_400,
        }
    }

    #[test]
    fn field_due_respects_ttl_and_backoff() {
        let now = Utc::now();
        let ttl = 12 * 3600;
        let miss = 86_400;
        assert_eq!(field_due_at(None, None, 0, ttl, miss), DueAt::Now);
        assert!(field_due_at(Some(true), Some(now - Cd::hours(13)), 0, ttl, miss).is_due(now));
        assert!(!field_due_at(Some(true), Some(now - Cd::hours(1)), 0, ttl, miss).is_due(now));
        // After one recorded failure, wait is 2^1 * miss_ttl = 2 days.
        assert!(!field_due_at(Some(false), Some(now - Cd::hours(25)), 1, ttl, miss).is_due(now));
        assert!(field_due_at(Some(false), Some(now - Cd::hours(49)), 1, ttl, miss).is_due(now));
        // fails=3 → 2^3 = 8 days, capped only at 7 days (BACKOFF_CAP).
        assert!(!field_due_at(Some(false), Some(now - Cd::days(6)), 3, ttl, miss).is_due(now));
        assert!(field_due_at(Some(false), Some(now - Cd::days(8)), 3, ttl, miss).is_due(now));
    }

    #[test]
    fn compute_due_never_without_a_source() {
        let due = compute_warm_due_at("obscure", None, None, None, &ttls());
        assert_eq!(due, DueAt::Never);
    }

    #[test]
    fn compute_due_now_when_cache_is_empty() {
        let due = compute_warm_due_at("cs2", Some(730), None, None, &ttls());
        assert_eq!(due, DueAt::Now);
        let wow = compute_warm_due_at("wow_forever", None, None, None, &ttls());
        assert_eq!(wow, DueAt::Now);
    }
}
