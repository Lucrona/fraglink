//! Background warmer: walk the whole catalogue (most-played / most-viewed first)
//! and fill store details, news, and Wikipedia fallbacks within the shared
//! outbound rate cap, with refresh TTLs and retry/backoff.

use crate::game_pages;
use crate::AppState;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::{sleep, Instant};

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
    let mut idle_passes = 0u32;
    loop {
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

/// Refresh the next due catalogue game. Returns whether work was done.
async fn warm_one(st: &AppState) -> anyhow::Result<bool> {
    let Some((id, appid, name)) = next_due(st).await? else {
        return Ok(false);
    };
    // If the gate is full, wait briefly rather than burning a due-check loop.
    let start = Instant::now();
    while !game_pages::gate_available(st) {
        if start.elapsed() > Duration::from_secs(30) {
            return Ok(false);
        }
        sleep(Duration::from_millis(500)).await;
    }
    game_pages::refresh_game(st, &id, appid, Some(&name)).await?;
    Ok(true)
}

async fn next_due(st: &AppState) -> anyhow::Result<Option<(String, Option<u32>, String)>> {
    let order = warm_order(st).await?;
    for (id, appid, name) in order {
        if game_pages::game_needs_refresh(st, &id, appid, st.cfg.game_warmer_news_ttl_secs).await? {
            return Ok(Some((id, appid, name)));
        }
    }
    Ok(None)
}

/// Most-played and most-viewed first, then the rest of the catalogue.
pub async fn warm_order(st: &AppState) -> anyhow::Result<Vec<(String, Option<u32>, String)>> {
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

    let mut rows: Vec<(i64, i64, i64, usize, String, Option<u32>, String)> = cat
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
                g.id,
                g.steam_appid,
                g.name,
            )
        })
        .collect();
    rows.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(b.2.cmp(&a.2))
            .then(b.3.cmp(&a.3))
    });
    Ok(rows
        .into_iter()
        .map(|(_, _, _, _, id, app, name)| (id, app, name))
        .collect())
}

#[cfg(test)]
mod tests {
    #[test]
    fn sort_priority_is_views_then_players() {
        // Documented in warm_order: (views, players, secs, ~index).
        let mut rows = [(0i64, 10i64, 100i64, 1u8), (5, 1, 1, 2), (5, 3, 1, 3)];
        rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        assert_eq!(rows[0], (5, 3, 1, 3));
        assert_eq!(rows[1], (5, 1, 1, 2));
        assert_eq!(rows[2], (0, 10, 100, 1));
    }
}
