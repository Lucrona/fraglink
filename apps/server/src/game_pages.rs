//! Public `/games/<slug>` pages, the Games tab extras, and a cached Steam / RSS
//! feed for game info and news (plus a Wikipedia/Wikidata fallback).
//!
//! Hours and player lists are hidden until a configurable number of distinct
//! players have contributed, so one person's time is never published as a
//! community figure. Outbound Steam, Wikipedia and news fetches are limited to
//! catalogue app ids (or an admin-set / curated feed URL) and share a
//! process-wide rate cap. A background warmer walks the catalogue so pages
//! fill in without waiting for a first view.

use crate::admin::{Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::week;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use url::Url;

const SEO_START: &str = "<!--seo-->";
const SEO_END: &str = "<!--/seo-->";
const PAGE_HTML: &str = "games-one.html";
pub(crate) const MAX_NEWS: usize = 8;
pub(crate) const MAX_FEED_BYTES: usize = 512_000;
pub(crate) const FEED_UA: &str = "Fraglink/1.0 (https://playfraglink.com; hello@playfraglink.com)";
const VIEW_WAIT: Duration = Duration::from_secs(2);
const BACKOFF_CAP_SECS: u64 = 7 * 86_400;
const MOST_PLAYED: i64 = 8;
const NEW_TO_LIST: i64 = 8;
const TOP_PLAYERS: i64 = 10;

/// In-memory slug <-> id map, rebuilt whenever the public catalogue is.
#[derive(Default, Clone)]
pub struct SlugIndex {
    /// Any accepted slug (canonical, id kebab, unique name kebab, admin).
    pub by_slug: HashMap<String, String>,
    /// game_id -> canonical slug used in links.
    pub canonical: HashMap<String, String>,
}

/// Process-wide cap on outbound store / news fetches (like the art proxy).
#[derive(Default)]
pub struct FeedGate {
    hits: Mutex<Vec<Instant>>,
}

impl FeedGate {
    /// Records a hit and returns whether it is allowed.
    pub fn try_acquire(&self, max: usize, window: Duration) -> bool {
        let mut v = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|t| t.elapsed() < window);
        if v.len() >= max {
            return false;
        }
        v.push(Instant::now());
        true
    }

    /// Whether a new fetch would be allowed right now (does not consume a slot).
    pub fn available(&self, max: usize, window: Duration) -> bool {
        let mut v = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        v.retain(|t| t.elapsed() < window);
        v.len() < max
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/games/tab", get(tab))
        .route("/api/v1/games/page/{slug}", get(page_api))
        .route("/games/{slug}", get(page_html))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new().route(
        "/api/v1/admin/games/{id}/page",
        get(admin_get_page).post(admin_save_page),
    )
}

// ------------------------------------------------------------------ slugs

/// Lower-case kebab form used in `/games/<slug>`. Apostrophes are dropped so
/// "Baldur's Gate 3" becomes `baldurs-gate-3`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if c == '\'' || c == '’' || c == '`' {
            continue;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "game".into()
    } else {
        out
    }
}

pub fn id_slug(id: &str) -> String {
    slugify(&id.replace('_', "-"))
}

pub fn name_slug(name: &str) -> String {
    slugify(name)
}

/// Assign a canonical slug to each catalogue game and a full lookup map.
///
/// Preference: an admin override, then a unique name kebab, then the id kebab.
/// The id kebab (and `steam-<appid>` when present) are always accepted as
/// aliases so old links keep working.
pub fn build_slug_index(
    games: &[(String, String, Option<u32>)],
    admin: &HashMap<String, String>,
) -> SlugIndex {
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let mut id_slugs: HashMap<String, String> = HashMap::new();
    for (id, name, _) in games {
        let ns = name_slug(name);
        *name_counts.entry(ns).or_default() += 1;
        id_slugs.insert(id.clone(), id_slug(id));
    }
    let mut reserved: HashSet<String> = HashSet::new();
    for s in admin.values() {
        if !s.is_empty() {
            reserved.insert(s.clone());
        }
    }
    // Reserve id slugs so a name kebab cannot steal another game's id path.
    for s in id_slugs.values() {
        reserved.insert(s.clone());
    }

    let mut index = SlugIndex::default();
    let mut used_canonical: HashSet<String> = HashSet::new();

    for (id, name, appid) in games {
        let ids = id_slugs[id].clone();
        let ns = name_slug(name);
        let mut canon = if let Some(a) = admin.get(id).filter(|s| !s.is_empty()) {
            a.clone()
        } else if name_counts.get(&ns).copied().unwrap_or(0) == 1
            && (!reserved.contains(&ns) || ns == ids)
        {
            ns.clone()
        } else {
            ids.clone()
        };
        if used_canonical.contains(&canon) && canon != ids {
            canon = ids.clone();
        }
        if used_canonical.contains(&canon) {
            let mut n = 2u32;
            loop {
                let cand = format!("{ids}-{n}");
                if !used_canonical.contains(&cand) && !reserved.contains(&cand) {
                    canon = cand;
                    break;
                }
                n += 1;
                if n > 100 {
                    canon = format!("{ids}-{id}");
                    break;
                }
            }
        }
        used_canonical.insert(canon.clone());
        index.canonical.insert(id.clone(), canon.clone());
        index.by_slug.insert(canon.clone(), id.clone());
        index.by_slug.insert(ids.clone(), id.clone());
        if name_counts.get(&ns).copied().unwrap_or(0) == 1 {
            index.by_slug.entry(ns).or_insert_with(|| id.clone());
        }
        if let Some(a) = *appid {
            if a > 0 {
                index
                    .by_slug
                    .entry(format!("steam-{a}"))
                    .or_insert_with(|| id.clone());
            }
        }
        if let Some(a) = admin.get(id).filter(|s| !s.is_empty()) {
            index.by_slug.insert(a.clone(), id.clone());
        }
    }
    index
}

pub fn remember_slugs(st: &AppState, index: SlugIndex) {
    *st.game_slugs.lock().unwrap_or_else(|e| e.into_inner()) = index;
}

async fn admin_slugs(db: &PgPool) -> ApiResult<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT game_id, slug FROM game_page_meta WHERE slug IS NOT NULL AND slug <> ''",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Rebuild the slug index from the current public catalogue.
pub async fn refresh_slugs(
    st: &AppState,
    games: &[(String, String, Option<u32>)],
) -> ApiResult<()> {
    let admin = admin_slugs(&st.db).await.unwrap_or_default();
    remember_slugs(st, build_slug_index(games, &admin));
    Ok(())
}

fn slugs_of(st: &AppState) -> SlugIndex {
    st.game_slugs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

async fn ensure_slugs(st: &AppState) -> ApiResult<SlugIndex> {
    {
        let cur = slugs_of(st);
        if !cur.canonical.is_empty() {
            return Ok(cur);
        }
    }
    let _ = crate::game_history::public_catalogue(st).await?;
    Ok(slugs_of(st))
}

pub fn slug_for(index: &SlugIndex, id: &str) -> String {
    index
        .canonical
        .get(id)
        .cloned()
        .unwrap_or_else(|| id_slug(id))
}

fn resolve_slug<'a>(index: &'a SlugIndex, slug: &str) -> Option<&'a str> {
    let key = slugify(slug);
    index.by_slug.get(&key).map(|s| s.as_str())
}

// ------------------------------------------------------------------ config helpers

fn min_players(st: &AppState) -> i64 {
    st.cfg.game_stats_min_players.max(1) as i64
}

fn week_tz(st: &AppState) -> chrono_tz::Tz {
    week::parse_tz(&st.cfg.public_week_tz)
}

fn windows(st: &AppState) -> (week::Window, week::Window) {
    let tz = week_tz(st);
    let now = Utc::now();
    let this = week::window(tz, now);
    let prev_end = DateTime::from_timestamp(this.bounds[0].saturating_sub(1), 0).unwrap_or(now);
    let prev = week::window(tz, prev_end);
    (this, prev)
}

// ------------------------------------------------------------------ privacy + stats (read-only)

const COUNTED_USERS: &str =
    "u.deleted_at IS NULL AND u.banned_at IS NULL AND u.delete_after IS NULL AND NOT u.is_test";

const PUBLIC_BOARD: &str =
    "u.deleted_at IS NULL AND u.banned_at IS NULL AND u.delete_after IS NULL AND NOT u.is_test
         AND u.public_profile AND coalesce((u.privacy->>'games')::boolean, true)
         AND coalesce((u.privacy->>'leaderboards')::boolean, true)
         AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = u.id AND h.game_id = $1)";

async fn distinct_all_time(db: &PgPool, game_id: &str) -> ApiResult<i64> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT s.user_id) FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.game_id = $1 AND s.duration_s > 0 AND u.deleted_at IS NULL
           AND u.banned_at IS NULL AND u.delete_after IS NULL AND NOT u.is_test",
    )
    .bind(game_id)
    .fetch_one(db)
    .await?;
    Ok(n)
}

async fn aggregates(db: &PgPool, game_id: &str, w: &week::Window) -> ApiResult<(i64, i64, i64)> {
    let total_s: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(GREATEST(s.duration_s, 0)), 0)::bigint
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.game_id = $1 AND u.deleted_at IS NULL AND u.banned_at IS NULL
           AND u.delete_after IS NULL AND NOT u.is_test",
    )
    .bind(game_id)
    .fetch_one(db)
    .await?;
    let sessions: i64 = sqlx::query_scalar(
        "SELECT count(*)::bigint FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.game_id = $1 AND GREATEST(s.duration_s, 0) > 0
           AND u.deleted_at IS NULL AND u.banned_at IS NULL
           AND u.delete_after IS NULL AND NOT u.is_test",
    )
    .bind(game_id)
    .fetch_one(db)
    .await?;
    let secs = week::secs_sql("$2", "$3");
    let inw = week::in_window_sql("$2", "$3");
    let sql = format!(
        "SELECT count(DISTINCT s.user_id)::bigint FROM play_sessions s
         JOIN users u ON u.id = s.user_id
         WHERE s.game_id = $1 AND {inw} AND {counted} AND {secs} >= 30",
        counted = COUNTED_USERS,
    );
    let players_week: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(game_id)
        .bind(w.bounds[0])
        .bind(w.bounds[week::DAYS])
        .fetch_one(db)
        .await?;
    Ok((total_s, sessions, players_week))
}

async fn top_players(
    db: &PgPool,
    game_id: &str,
    w: &week::Window,
    limit: i64,
) -> ApiResult<Vec<Value>> {
    let secs = week::secs_sql("$2", "$3");
    let inw = week::in_window_sql("$2", "$3");
    let sql = format!(
        "SELECT u.handle, u.display_name, sum({secs})::bigint AS s
         FROM play_sessions s JOIN users u ON u.id = s.user_id
         WHERE s.game_id = $1 AND {inw} AND {who}
         GROUP BY u.handle, u.display_name
         HAVING sum({secs}) >= 30
         ORDER BY s DESC, u.handle
         LIMIT $4",
        who = PUBLIC_BOARD,
    );
    let rows: Vec<(String, Option<String>, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(game_id)
        .bind(w.bounds[0])
        .bind(w.bounds[week::DAYS])
        .bind(limit)
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .enumerate()
        .map(|(i, (handle, display_name, s))| {
            let minutes = week::minutes(s);
            json!({
                "rank": i + 1,
                "handle": handle,
                "display_name": display_name,
                "minutes": minutes,
                "seconds": s,
            })
        })
        .collect())
}

async fn public_board_count(db: &PgPool, game_id: &str, w: &week::Window) -> ApiResult<i64> {
    let secs = week::secs_sql("$2", "$3");
    let inw = week::in_window_sql("$2", "$3");
    let sql = format!(
        "SELECT count(*)::bigint FROM (
            SELECT u.id FROM play_sessions s JOIN users u ON u.id = s.user_id
            WHERE s.game_id = $1 AND {inw} AND {who}
            GROUP BY u.id HAVING sum({secs}) >= 30
         ) t",
        who = PUBLIC_BOARD,
    );
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(game_id)
        .bind(w.bounds[0])
        .bind(w.bounds[week::DAYS])
        .fetch_one(db)
        .await?)
}

// ------------------------------------------------------------------ tab (most played, trending, new, genres)

async fn tab(State(st): State<AppState>) -> ApiResult<Json<Value>> {
    let index = ensure_slugs(&st).await?;
    let (this_w, prev_w) = windows(&st);
    let min = min_players(&st);
    maybe_refresh_feeds(&st);
    let most = most_played(&st, &index, &this_w, min).await?;
    let trending = trending_ids(&st, &this_w, &prev_w, min).await?;
    let new_to_list = new_to_list(&st, &index).await?;
    let genres = genre_counts(&st).await?;
    Ok(Json(json!({
        "week_tz": st.cfg.public_week_tz,
        "min_players": min,
        "most_played": most,
        "trending": trending,
        "new_to_list": new_to_list,
        "genres": genres,
    })))
}

async fn most_played(
    st: &AppState,
    index: &SlugIndex,
    w: &week::Window,
    min: i64,
) -> ApiResult<Vec<Value>> {
    let secs = week::secs_sql("$2", "$3");
    let inw = week::in_window_sql("$2", "$3");
    let sql = format!(
        "SELECT s.game_id, sum({secs})::bigint AS secs, count(DISTINCT s.user_id)::bigint AS n
         FROM play_sessions s JOIN users u ON u.id = s.user_id
         WHERE {inw} AND {counted} AND s.game_id IS NOT NULL AND s.game_id <> ''
         GROUP BY s.game_id
         HAVING count(DISTINCT s.user_id) >= $1 AND sum({secs}) >= 30
         ORDER BY secs DESC, s.game_id
         LIMIT $4",
        counted = COUNTED_USERS,
    );
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(min)
        .bind(w.bounds[0])
        .bind(w.bounds[week::DAYS])
        .bind(MOST_PLAYED)
        .fetch_all(&st.db)
        .await?;
    let names = names_for(st, rows.iter().map(|(id, _, _)| id.as_str())).await?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _)| names.contains_key(id))
        .map(|(id, s, n)| {
            json!({
                "game_id": id,
                "name": names[&id],
                "slug": slug_for(index, &id),
                "minutes": week::minutes(s),
                "seconds": s,
                "players": n,
            })
        })
        .collect())
}

async fn trending_ids(
    st: &AppState,
    this_w: &week::Window,
    prev_w: &week::Window,
    min: i64,
) -> ApiResult<Vec<String>> {
    let secs = week::secs_sql("$2", "$3");
    let inw = week::in_window_sql("$2", "$3");
    // One scan for this week, one for last week; compared in Rust.
    let sql = format!(
        "SELECT s.game_id, sum({secs})::bigint, count(DISTINCT s.user_id)::bigint
         FROM play_sessions s JOIN users u ON u.id = s.user_id
         WHERE {inw} AND {counted} AND s.game_id IS NOT NULL AND s.game_id <> ''
         GROUP BY s.game_id
         HAVING count(DISTINCT s.user_id) >= $1",
        counted = COUNTED_USERS,
    );
    let this_rows: Vec<(String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.clone()))
        .bind(min)
        .bind(this_w.bounds[0])
        .bind(this_w.bounds[week::DAYS])
        .fetch_all(&st.db)
        .await?;
    // Prior week is not threshold-gated on its own player count: a game that
    // jumped from a handful of hours still qualifies if this week meets `min`.
    let secs_prev = week::secs_sql("$1", "$2");
    let inw_prev = week::in_window_sql("$1", "$2");
    let sql_prev = format!(
        "SELECT s.game_id, sum({secs_prev})::bigint
         FROM play_sessions s JOIN users u ON u.id = s.user_id
         WHERE {inw_prev} AND {counted} AND s.game_id IS NOT NULL AND s.game_id <> ''
         GROUP BY s.game_id",
        counted = COUNTED_USERS,
    );
    let prev_rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(sql_prev))
        .bind(prev_w.bounds[0])
        .bind(prev_w.bounds[week::DAYS])
        .fetch_all(&st.db)
        .await?;
    let prev: HashMap<String, i64> = prev_rows.into_iter().collect();
    let ratio = st.cfg.game_trending_ratio;
    let min_secs = st.cfg.game_trending_min_secs;
    let mut out: Vec<(String, i64)> = this_rows
        .into_iter()
        .filter(|(id, now, _)| {
            is_trending(*now, prev.get(id).copied().unwrap_or(0), ratio, min_secs)
        })
        .map(|(id, now, _)| (id, now))
        .collect();
    out.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(out.into_iter().take(24).map(|(id, _)| id).collect())
}

pub fn is_trending(now: i64, prev: i64, ratio: f64, min_secs: i64) -> bool {
    if now < min_secs.max(0) {
        return false;
    }
    let delta = now.saturating_sub(prev.max(0));
    if delta < min_secs.max(0) {
        return false;
    }
    if prev <= 0 {
        return true;
    }
    (now as f64) >= (prev as f64) * ratio.max(1.0)
}

async fn new_to_list(st: &AppState, index: &SlugIndex) -> ApiResult<Vec<Value>> {
    let rows: Vec<(String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT DISTINCT ON (game_id) game_id, game_name, created_at
         FROM game_catalogue_history
         WHERE public AND NOT hidden_from_public
           AND action IN ('added', 'manual')
           AND game_id IS NOT NULL AND game_id <> ''
         ORDER BY game_id, created_at DESC",
    )
    .fetch_all(&st.db)
    .await?;
    let mut rows = rows;
    rows.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    rows.truncate(NEW_TO_LIST as usize);
    // Only games still on the public list.
    let live: HashSet<String> = index.canonical.keys().cloned().collect();
    let names = names_for(st, rows.iter().map(|(id, _, _)| id.as_str())).await?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _)| live.contains(id) && names.contains_key(id))
        .map(|(id, hist_name, at)| {
            let name = names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| hist_name.unwrap_or_else(|| id.clone()));
            json!({
                "game_id": id,
                "name": name,
                "slug": slug_for(index, &id),
                "at": at,
            })
        })
        .collect())
}

async fn genre_counts(st: &AppState) -> ApiResult<Vec<Value>> {
    let mut counts: HashMap<String, Vec<String>> = HashMap::new();
    let live: HashSet<String> = slugs_of(st).canonical.keys().cloned().collect();
    let overrides: Vec<(String, Option<String>)> = sqlx::query_as(
        "SELECT game_id, genres FROM game_page_meta WHERE genres IS NOT NULL AND genres <> ''",
    )
    .fetch_all(&st.db)
    .await?;
    let mut overridden = HashSet::new();
    for (id, g) in &overrides {
        if !live.contains(id) {
            continue;
        }
        overridden.insert(id.clone());
        for part in split_list(g.as_deref().unwrap_or("")) {
            counts.entry(part).or_default().push(id.clone());
        }
    }
    let cached: Vec<(String, Option<Value>)> = sqlx::query_as(
        "SELECT game_id, details_json FROM game_feed_cache
         WHERE details_ok IS TRUE AND details_json IS NOT NULL",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, details) in cached {
        if !live.contains(&id) || overridden.contains(&id) {
            continue;
        }
        if let Some(v) = details {
            for part in genres_from_details(&v) {
                counts.entry(part).or_default().push(id.clone());
            }
        }
    }
    let mut list: Vec<(String, Vec<String>)> = counts.into_iter().collect();
    list.sort_by(|a, b| {
        b.1.len()
            .cmp(&a.1.len())
            .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
    });
    Ok(list
        .into_iter()
        .map(|(name, mut ids)| {
            ids.sort();
            ids.dedup();
            json!({ "name": name, "count": ids.len() as i64, "ids": ids })
        })
        .collect())
}

async fn names_for(
    st: &AppState,
    ids: impl IntoIterator<Item = &str>,
) -> ApiResult<HashMap<String, String>> {
    let wanted: Vec<String> = ids.into_iter().map(|s| s.to_string()).collect();
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }
    let cat = crate::game_history::public_catalogue(st).await?;
    let mut map = HashMap::new();
    for g in cat {
        if wanted.contains(&g.id) {
            map.insert(g.id, g.name);
        }
    }
    Ok(map)
}

// ------------------------------------------------------------------ game page API + HTML

async fn page_api(State(st): State<AppState>, Path(slug): Path<String>) -> ApiResult<Json<Value>> {
    let index = ensure_slugs(&st).await?;
    let Some(game_id) = resolve_slug(&index, &slug).map(|s| s.to_string()) else {
        return Err(AppError::not_found());
    };
    Ok(Json(page_payload(&st, &index, &game_id).await?))
}

async fn page_html(State(st): State<AppState>, Path(slug): Path<String>) -> Response {
    let index = match ensure_slugs(&st).await {
        Ok(i) => i,
        Err(e) => return e.into_response(),
    };
    let Some(game_id) = resolve_slug(&index, &slug).map(|s| s.to_string()) else {
        return missing_game_page(&st, true);
    };
    let canon = slug_for(&index, &game_id);
    if slugify(&slug) != canon {
        return Redirect::permanent(&format!("/games/{canon}")).into_response();
    }
    let body = match std::fs::read_to_string(st.cfg.site_dir.join(PAGE_HTML)) {
        Ok(p) => p,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let name = match crate::game_history::public_catalogue(&st).await {
        Ok(c) => c
            .into_iter()
            .find(|g| g.id == game_id)
            .map(|g| g.name)
            .unwrap_or_else(|| game_id.clone()),
        Err(_) => game_id.clone(),
    };
    let base = st.cfg.canonical_url.trim_end_matches('/');
    let url = format!("{base}/games/{canon}");
    let title = format!("{name} · Fraglink");
    let desc = format!("Hours, players, news and info for {name} on Fraglink.");
    let head = format!(
        "<title>{t}</title><meta name=\"description\" content=\"{d}\">\n\
         <link rel=\"canonical\" href=\"{u}\"><meta property=\"og:type\" content=\"website\">\
         <meta property=\"og:site_name\" content=\"Fraglink\"><meta property=\"og:title\" content=\"{t}\">\
         <meta property=\"og:description\" content=\"{d}\"><meta property=\"og:url\" content=\"{u}\">",
        t = crate::web::esc(&title),
        d = crate::web::esc(&desc),
        u = crate::web::esc(&url),
    );
    let html = match (body.find(SEO_START), body.find(SEO_END)) {
        (Some(a), Some(z)) if a < z => {
            format!("{}{head}{}", &body[..a], &body[z + SEO_END.len()..])
        }
        _ => body,
    };
    let mut r = (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

fn missing_game_page(st: &AppState, not_found: bool) -> Response {
    let body = match std::fs::read_to_string(st.cfg.site_dir.join(PAGE_HTML)) {
        Ok(p) => p,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let title = "Game not found · Fraglink";
    let desc = "That game is not on the Fraglink list.";
    let url = format!("{}/games", st.cfg.canonical_url.trim_end_matches('/'));
    let head = format!(
        "<title>{t}</title><meta name=\"robots\" content=\"noindex\"><meta name=\"description\" content=\"{d}\">\
         <link rel=\"canonical\" href=\"{u}\">",
        t = crate::web::esc(title),
        d = crate::web::esc(desc),
        u = crate::web::esc(&url),
    );
    let html = match (body.find(SEO_START), body.find(SEO_END)) {
        (Some(a), Some(z)) if a < z => {
            format!("{}{head}{}", &body[..a], &body[z + SEO_END.len()..])
        }
        _ => body,
    };
    let status = if not_found {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    let mut r = (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response();
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r.headers_mut()
        .insert("x-robots-tag", HeaderValue::from_static("noindex"));
    r
}

async fn page_payload(st: &AppState, index: &SlugIndex, game_id: &str) -> ApiResult<Value> {
    let cat = crate::game_history::public_catalogue(st).await?;
    let Some(game) = cat.into_iter().find(|g| g.id == game_id) else {
        return Err(AppError::not_found());
    };
    increment_views(&st.db, game_id).await;
    ensure_fresh(st, &game.id, game.steam_appid, &game.name).await;
    let (this_w, _) = windows(st);
    let min = min_players(st);
    let n = distinct_all_time(&st.db, game_id).await?;
    let show_stats = n >= min;
    let stats = if show_stats {
        let (total_s, sessions, players_week) = aggregates(&st.db, game_id, &this_w).await?;
        json!({
            "total_seconds": total_s,
            "total_minutes": week::minutes(total_s),
            "sessions": sessions,
            "players_this_week": players_week,
        })
    } else {
        Value::Null
    };
    let board_n = public_board_count(&st.db, game_id, &this_w).await?;
    let show_top = board_n >= min;
    let top = if show_top {
        Value::Array(top_players(&st.db, game_id, &this_w, TOP_PLAYERS).await?)
    } else {
        Value::Null
    };
    let (info, info_source) = game_info(st, game_id, game.steam_appid).await?;
    let (news, news_source) = game_news(st, game_id).await?;
    let (description, description_source, description_url) = game_blurb(st, game_id).await?;
    Ok(json!({
        "game_id": game.id,
        "name": game.name,
        "slug": slug_for(index, &game.id),
        "steam_appid": game.steam_appid,
        "stats_visible": show_stats,
        "stats": stats,
        "top_players_visible": show_top,
        "top_players": top,
        "info": info,
        "info_source": info_source,
        "description": description,
        "description_source": description_source,
        "description_url": description_url,
        "news": news,
        "news_source": news_source,
        "week_tz": st.cfg.public_week_tz,
        "min_players": min,
    }))
}

async fn increment_views(db: &PgPool, game_id: &str) {
    let _ = sqlx::query(
        "INSERT INTO game_page_views (game_id, views, last_viewed_at)
         VALUES ($1, 1, now())
         ON CONFLICT (game_id) DO UPDATE SET
           views = game_page_views.views + 1,
           last_viewed_at = now()",
    )
    .bind(game_id)
    .execute(db)
    .await;
}

async fn ensure_fresh(st: &AppState, game_id: &str, appid: Option<u32>, name: &str) {
    let due = game_needs_refresh(st, game_id, appid, st.cfg.game_news_ttl_secs)
        .await
        .unwrap_or(true);
    if !due {
        return;
    }
    let empty = cache_is_empty(st, game_id).await && can_wait_for_fetch(st, game_id, appid);
    let st2 = st.clone();
    let id = game_id.to_string();
    let n = name.to_string();
    let handle = tokio::spawn(async move {
        if let Err(e) = refresh_game(&st2, &id, appid, Some(&n)).await {
            tracing::debug!("game feed {id}: {e}");
        }
    });
    if empty {
        let _ = tokio::time::timeout(VIEW_WAIT, handle).await;
    }
}

fn can_wait_for_fetch(st: &AppState, game_id: &str, appid: Option<u32>) -> bool {
    let https = |u: &str| u.starts_with("https://");
    (appid.is_some() && st.cfg.steam_store_url.as_deref().is_some_and(https))
        || (appid.is_some() && https(&st.cfg.steam_api_base))
        || (appid.is_none() && st.cfg.wikipedia_url.as_deref().is_some_and(https))
        || crate::game_feed_defaults::official_news_url(game_id).is_some()
}

async fn cache_is_empty(st: &AppState, game_id: &str) -> bool {
    let row: Option<(Option<bool>, Option<bool>)> =
        sqlx::query_as("SELECT details_ok, news_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(game_id)
            .fetch_optional(&st.db)
            .await
            .ok()
            .flatten();
    !matches!(row, Some((Some(true), _)) | Some((_, Some(true))))
}

// ------------------------------------------------------------------ info + news

#[derive(sqlx::FromRow)]
struct MetaRow {
    slug: Option<String>,
    developer: Option<String>,
    publisher: Option<String>,
    release_date: Option<String>,
    genres: Option<String>,
    platforms: Option<String>,
    news_feed_url: Option<String>,
    description: Option<String>,
}

async fn load_meta(db: &PgPool, game_id: &str) -> ApiResult<Option<MetaRow>> {
    Ok(sqlx::query_as(
        "SELECT slug, developer, publisher, release_date, genres, platforms, news_feed_url, description
         FROM game_page_meta WHERE game_id = $1",
    )
    .bind(game_id)
    .fetch_optional(db)
    .await?)
}

async fn game_info(
    st: &AppState,
    game_id: &str,
    appid: Option<u32>,
) -> ApiResult<(Value, Option<String>)> {
    let meta = load_meta(&st.db, game_id).await?;
    let cached: Option<(Option<Value>, Option<bool>)> =
        sqlx::query_as("SELECT details_json, details_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(game_id)
            .fetch_optional(&st.db)
            .await?;
    let details = cached
        .as_ref()
        .and_then(|(j, ok)| if *ok == Some(true) { j.clone() } else { None });
    let parsed = details.as_ref().map(info_from_details).unwrap_or_default();
    let wiki = details
        .as_ref()
        .and_then(crate::game_wiki::bits_from_cached);
    let over = |field: Option<&String>, fallback: Option<String>| {
        field
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or(fallback)
    };
    let dev = over(
        meta.as_ref().and_then(|m| m.developer.as_ref()),
        parsed.developer,
    );
    let pub_ = over(
        meta.as_ref().and_then(|m| m.publisher.as_ref()),
        parsed.publisher,
    );
    let rel = over(
        meta.as_ref().and_then(|m| m.release_date.as_ref()),
        parsed.release_date,
    );
    let genres = over(meta.as_ref().and_then(|m| m.genres.as_ref()), parsed.genres);
    let platforms = over(
        meta.as_ref().and_then(|m| m.platforms.as_ref()),
        parsed.platforms,
    );
    let has_over = meta.as_ref().is_some_and(|m| {
        m.developer.as_ref().is_some_and(|s| !s.trim().is_empty())
            || m.publisher.as_ref().is_some_and(|s| !s.trim().is_empty())
            || m.release_date
                .as_ref()
                .is_some_and(|s| !s.trim().is_empty())
            || m.genres.as_ref().is_some_and(|s| !s.trim().is_empty())
            || m.platforms.as_ref().is_some_and(|s| !s.trim().is_empty())
    });
    let info = json!({
        "developer": dev,
        "publisher": pub_,
        "release_date": rel,
        "genres": genres,
        "platforms": platforms,
    });
    let empty = [&dev, &pub_, &rel, &genres, &platforms]
        .iter()
        .all(|v| v.is_none());
    let source = if empty {
        None
    } else if has_over {
        Some("Edited".to_string())
    } else if appid.is_some() && details.is_some() && wiki.is_none() {
        Some("Store listing".to_string())
    } else if wiki.is_some() {
        Some("Wikipedia".to_string())
    } else if details.is_some() {
        Some("Store listing".to_string())
    } else {
        None
    };
    Ok((info, source))
}

async fn game_blurb(
    st: &AppState,
    game_id: &str,
) -> ApiResult<(Option<String>, Option<String>, Option<String>)> {
    let meta = load_meta(&st.db, game_id).await?;
    if let Some(d) = meta
        .as_ref()
        .and_then(|m| m.description.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return Ok((Some(d.to_string()), Some("Edited".into()), None));
    }
    let cached: Option<(Option<Value>, Option<bool>)> =
        sqlx::query_as("SELECT details_json, details_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(game_id)
            .fetch_optional(&st.db)
            .await?;
    let details = cached
        .as_ref()
        .and_then(|(j, ok)| if *ok == Some(true) { j.clone() } else { None });
    let Some(v) = details else {
        return Ok((None, None, None));
    };
    if let Some(w) = crate::game_wiki::bits_from_cached(&v) {
        if let Some(extract) = w.extract {
            return Ok((Some(extract), Some("Wikipedia".into()), w.url));
        }
    }
    if let Some(s) = short_description(&v) {
        return Ok((Some(s), Some("Store listing".into()), None));
    }
    Ok((None, None, None))
}

#[derive(Default, Clone)]
pub struct InfoBits {
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub release_date: Option<String>,
    pub genres: Option<String>,
    pub platforms: Option<String>,
}

fn info_from_details(v: &Value) -> InfoBits {
    let data = if v.get("developers").is_some() || v.get("genres").is_some() {
        v
    } else {
        // raw appdetails wrapper: { "730": { "success": true, "data": {..} } }
        v.as_object()
            .and_then(|m| m.values().next())
            .and_then(|x| x.get("data"))
            .unwrap_or(v)
    };
    let list = |key: &str| -> Option<String> {
        if let Some(s) = data.get(key).and_then(|x| x.as_str()) {
            let t = s.trim();
            return if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            };
        }
        let arr = data.get(key)?.as_array()?;
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    };
    let genres = data.get("genres").and_then(|g| {
        let arr = g.as_array()?;
        let parts: Vec<String> = arr
            .iter()
            .filter_map(|x| x.get("description").and_then(|d| d.as_str()))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(", "))
        }
    });
    let platforms = data.get("platforms").and_then(|p| {
        let mut out = Vec::new();
        if p.get("windows").and_then(|x| x.as_bool()) == Some(true) {
            out.push("Windows");
        }
        if p.get("mac").and_then(|x| x.as_bool()) == Some(true) {
            out.push("macOS");
        }
        if p.get("linux").and_then(|x| x.as_bool()) == Some(true) {
            out.push("Linux");
        }
        if out.is_empty() {
            None
        } else {
            Some(out.join(", "))
        }
    });
    let release = data
        .pointer("/release_date/date")
        .and_then(|d| d.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    InfoBits {
        developer: list("developers").or_else(|| list("developer")),
        publisher: list("publishers").or_else(|| list("publisher")),
        release_date: release.or_else(|| {
            data.get("release_date")
                .and_then(|d| d.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        }),
        genres: genres.or_else(|| list("genres")),
        platforms: platforms.or_else(|| list("platforms")),
    }
}

pub fn short_description(v: &Value) -> Option<String> {
    let data = if v.get("short_description").is_some() || v.get("extract").is_some() {
        v
    } else {
        v.as_object()
            .and_then(|m| m.values().next())
            .and_then(|x| x.get("data"))
            .unwrap_or(v)
    };
    data.get("short_description")
        .or_else(|| data.get("extract"))
        .and_then(|s| s.as_str())
        .map(crate::game_wiki::trim_extract)
        .filter(|s| !s.is_empty())
}

pub fn genres_from_details(v: &Value) -> Vec<String> {
    info_from_details(v)
        .genres
        .map(|s| split_list(&s))
        .unwrap_or_default()
}

pub fn split_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in s.split([',', ';', '/', '|']) {
        let t = part.trim();
        if t.is_empty() {
            continue;
        }
        if !out.iter().any(|x: &String| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_string());
        }
    }
    out
}

async fn game_news(st: &AppState, game_id: &str) -> ApiResult<(Vec<Value>, Option<String>)> {
    let meta = load_meta(&st.db, game_id).await?;
    let custom = meta
        .as_ref()
        .and_then(|m| m.news_feed_url.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let row: Option<(Option<Value>, Option<bool>, Option<String>)> = sqlx::query_as(
        "SELECT news_json, news_ok, news_kind FROM game_feed_cache WHERE game_id = $1",
    )
    .bind(game_id)
    .fetch_optional(&st.db)
    .await?;
    let items = row
        .as_ref()
        .filter(|r| r.1 == Some(true))
        .and_then(|r| r.0.as_ref())
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok((Vec::new(), None));
    }
    let kind = row.and_then(|r| r.2);
    let source = if custom.is_some() {
        Some("News feed".to_string())
    } else if kind.as_deref() == Some("steam") {
        Some("Official news".to_string())
    } else {
        Some("News".to_string())
    };
    Ok((items, source))
}

// ------------------------------------------------------------------ outbound fetch (catalogue app ids only)

fn maybe_refresh_feeds(st: &AppState) {
    // tab load: refresh a handful of stale catalogue games in the background.
    let st = st.clone();
    tokio::spawn(async move {
        if let Err(e) = refresh_stale(&st, 4).await {
            tracing::debug!("game feed refresh: {e}");
        }
    });
}

async fn refresh_stale(st: &AppState, n: usize) -> anyhow::Result<()> {
    let cat = match crate::game_history::public_catalogue(st).await {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let store_ttl = Duration::from_secs(st.cfg.game_store_ttl_secs);
    let news_ttl = Duration::from_secs(st.cfg.game_news_ttl_secs);
    let miss_ttl = Duration::from_secs(st.cfg.game_feed_miss_ttl_secs);
    let now = Utc::now();
    let stale = |ok: Option<bool>, at: Option<DateTime<Utc>>, ttl: Duration| -> bool {
        match (ok, at) {
            (None, _) | (_, None) => true,
            (Some(true), Some(t)) => now.signed_duration_since(t).to_std().unwrap_or(ttl) >= ttl,
            (Some(false), Some(t)) => {
                now.signed_duration_since(t).to_std().unwrap_or(miss_ttl) >= miss_ttl
            }
        }
    };
    let mut did = 0;
    for g in cat {
        if did >= n {
            break;
        }
        let meta = load_meta(&st.db, &g.id).await.ok().flatten();
        let has_feed = meta
            .as_ref()
            .and_then(|m| m.news_feed_url.as_deref())
            .is_some_and(|s| !s.trim().is_empty());
        if g.steam_appid.is_none() && !has_feed {
            continue;
        }
        let row: Option<(
            Option<bool>,
            Option<DateTime<Utc>>,
            Option<bool>,
            Option<DateTime<Utc>>,
        )> = sqlx::query_as(
            "SELECT details_ok, details_fetched_at, news_ok, news_fetched_at
             FROM game_feed_cache WHERE game_id = $1",
        )
        .bind(&g.id)
        .fetch_optional(&st.db)
        .await
        .ok()
        .flatten();
        let need = match row {
            None => true,
            Some((dok, dat, nok, nat)) => {
                (g.steam_appid.is_some() && stale(dok, dat, store_ttl))
                    || ((g.steam_appid.is_some() || has_feed) && stale(nok, nat, news_ttl))
            }
        };
        if !need {
            continue;
        }
        refresh_game(st, &g.id, g.steam_appid, Some(&g.name)).await?;
        did += 1;
    }
    Ok(())
}

pub(crate) async fn refresh_game(
    st: &AppState,
    game_id: &str,
    appid: Option<u32>,
    name: Option<&str>,
) -> anyhow::Result<()> {
    let meta = load_meta(&st.db, game_id).await.ok().flatten();
    let admin_feed = meta
        .as_ref()
        .and_then(|m| m.news_feed_url.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let curated = crate::game_feed_defaults::official_news_url(game_id).map(|s| s.to_string());
    if let Some(a) = appid {
        if !allowed_appid(st, a) {
            anyhow::bail!("appid {a} is not on the catalogue allow-list");
        }
        refresh_store(st, game_id, a).await?;
        let _ = copy_steam_details(st, game_id, a).await;
    } else {
        let label = name.unwrap_or(game_id);
        let _ = crate::game_wiki::refresh_wiki(st, game_id, label).await;
    }
    let mut news_ok = false;
    if let Some(url) = admin_feed.as_deref() {
        news_ok =
            refresh_rss(st, game_id, url, false).await.is_ok() && news_cache_ok(st, game_id).await;
    }
    if !news_ok {
        if let Some(url) = curated.as_deref() {
            if admin_feed.as_deref() != Some(url) {
                news_ok = refresh_rss(st, game_id, url, true).await.is_ok()
                    && news_cache_ok(st, game_id).await;
            }
        }
    }
    if !news_ok {
        if let Some(a) = appid {
            refresh_steam_news(st, game_id, a).await?;
            let _ = copy_steam_news(st, game_id, a).await;
        }
    }
    Ok(())
}

async fn news_cache_ok(st: &AppState, game_id: &str) -> bool {
    let ok: Option<bool> =
        sqlx::query_scalar("SELECT news_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(game_id)
            .fetch_optional(&st.db)
            .await
            .ok()
            .flatten()
            .flatten();
    ok == Some(true)
}

async fn sibling_ids(st: &AppState, appid: u32, except: &str) -> Vec<String> {
    let cat = crate::game_history::public_catalogue(st)
        .await
        .unwrap_or_default();
    cat.into_iter()
        .filter(|g| g.steam_appid == Some(appid) && g.id != except)
        .map(|g| g.id)
        .collect()
}

async fn copy_steam_details(st: &AppState, from_id: &str, appid: u32) -> anyhow::Result<()> {
    let row: Option<(Option<Value>, Option<bool>)> =
        sqlx::query_as("SELECT details_json, details_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(from_id)
            .fetch_optional(&st.db)
            .await?;
    let Some((Some(data), Some(true))) = row else {
        return Ok(());
    };
    for id in sibling_ids(st, appid, from_id).await {
        upsert_details(
            st,
            &id,
            Some(appid as i32),
            Some(&data),
            true,
            Some("steam"),
        )
        .await?;
    }
    Ok(())
}

async fn copy_steam_news(st: &AppState, from_id: &str, appid: u32) -> anyhow::Result<()> {
    let row: Option<(Option<Value>, Option<bool>)> =
        sqlx::query_as("SELECT news_json, news_ok FROM game_feed_cache WHERE game_id = $1")
            .bind(from_id)
            .fetch_optional(&st.db)
            .await?;
    let Some((Some(items), Some(true))) = row else {
        return Ok(());
    };
    let arr = items.as_array().cloned().unwrap_or_default();
    for id in sibling_ids(st, appid, from_id).await {
        upsert_news(st, &id, &arr, true, "steam").await?;
    }
    Ok(())
}

pub(crate) fn gate_available(st: &AppState) -> bool {
    st.feed_gate
        .available(st.cfg.game_feed_per_min.max(1), Duration::from_secs(60))
}

/// Whether store, news, or (for non-Steam) Wikipedia data is missing or stale.
pub(crate) async fn game_needs_refresh(
    st: &AppState,
    game_id: &str,
    appid: Option<u32>,
    news_ttl: u64,
) -> anyhow::Result<bool> {
    let meta = load_meta(&st.db, game_id).await.ok().flatten();
    let has_feed = meta
        .as_ref()
        .and_then(|m| m.news_feed_url.as_deref())
        .is_some_and(|s| !s.trim().is_empty())
        || crate::game_feed_defaults::official_news_url(game_id).is_some();
    let need_details = appid.is_some() || st.cfg.wikipedia_url.is_some();
    let need_news = appid.is_some() || has_feed;
    if !need_details && !need_news {
        return Ok(false);
    }
    if need_details
        && cache_due(
            st,
            game_id,
            "details_ok",
            "details_fetched_at",
            st.cfg.game_store_ttl_secs,
        )
        .await?
    {
        return Ok(true);
    }
    if need_news && cache_due(st, game_id, "news_ok", "news_fetched_at", news_ttl).await? {
        return Ok(true);
    }
    Ok(false)
}

fn allowed_appid(st: &AppState, appid: u32) -> bool {
    let g = st.art_appids.lock().unwrap_or_else(|e| e.into_inner());
    g.contains(appid)
}

async fn refresh_store(st: &AppState, game_id: &str, appid: u32) -> anyhow::Result<()> {
    if !cache_due(
        st,
        game_id,
        "details_ok",
        "details_fetched_at",
        st.cfg.game_store_ttl_secs,
    )
    .await?
    {
        return Ok(());
    }
    let Some(base) = st.cfg.steam_store_url.clone() else {
        return Ok(());
    };
    if !acquire(st) {
        return Ok(());
    }
    let url = format!("{base}/api/appdetails?appids={appid}");
    let body = fetch_text(&url).await;
    match body.and_then(|t| parse_appdetails(&t, appid)) {
        Ok(data) => {
            upsert_details(
                st,
                game_id,
                Some(appid as i32),
                Some(&data),
                true,
                Some("steam"),
            )
            .await?;
        }
        Err(e) => {
            tracing::debug!("store {appid}: {e}");
            upsert_details(st, game_id, Some(appid as i32), None, false, Some("steam")).await?;
        }
    }
    Ok(())
}

pub fn parse_appdetails(text: &str, appid: u32) -> anyhow::Result<Value> {
    let v: Value = serde_json::from_str(text)?;
    let d = v
        .get(appid.to_string())
        .ok_or_else(|| anyhow::anyhow!("no app entry"))?;
    if d.get("success").and_then(|s| s.as_bool()) != Some(true) {
        anyhow::bail!("success=false");
    }
    Ok(d.get("data").cloned().unwrap_or(json!({})))
}

async fn refresh_steam_news(st: &AppState, game_id: &str, appid: u32) -> anyhow::Result<()> {
    if !cache_due(
        st,
        game_id,
        "news_ok",
        "news_fetched_at",
        st.cfg.game_news_ttl_secs,
    )
    .await?
    {
        return Ok(());
    }
    if !acquire(st) {
        return Ok(());
    }
    let url = format!(
        "{}/ISteamNews/GetNewsForApp/v2/?appid={appid}&count={MAX_NEWS}&maxlength=400&format=json",
        st.cfg.steam_api_base.trim_end_matches('/')
    );
    match fetch_text(&url)
        .await
        .and_then(|t| parse_steam_news(&t, appid))
    {
        Ok(items) => {
            upsert_news(st, game_id, &items, true, "steam").await?;
        }
        Err(e) => {
            tracing::debug!("steam news {appid}: {e}");
            upsert_news(st, game_id, &[], false, "steam").await?;
        }
    }
    Ok(())
}

pub fn parse_steam_news(text: &str, appid: u32) -> anyhow::Result<Vec<Value>> {
    let v: Value = serde_json::from_str(text)?;
    let items = v
        .pointer("/appnews/newsitems")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for it in items.into_iter().take(MAX_NEWS) {
        let title = it
            .get("title")
            .and_then(|t| t.as_str())
            .unwrap_or("Update")
            .trim();
        if title.is_empty() {
            continue;
        }
        let gid = it.get("gid").and_then(|g| g.as_str()).unwrap_or("");
        let url = if !gid.is_empty() {
            format!("https://store.steampowered.com/news/app/{appid}/view/{gid}")
        } else {
            it.get("url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string()
        };
        let date = it
            .get("date")
            .and_then(|d| d.as_i64())
            .and_then(|s| DateTime::from_timestamp(s, 0).map(|t| t.to_rfc3339()));
        out.push(json!({
            "title": title.chars().take(200).collect::<String>(),
            "url": url,
            "at": date,
        }));
    }
    Ok(out)
}

async fn refresh_rss(
    st: &AppState,
    game_id: &str,
    raw_url: &str,
    force: bool,
) -> anyhow::Result<()> {
    if !force
        && !cache_due(
            st,
            game_id,
            "news_ok",
            "news_fetched_at",
            st.cfg.game_news_ttl_secs,
        )
        .await?
    {
        return Ok(());
    }
    let url = parse_public_http_url(raw_url).ok_or_else(|| anyhow::anyhow!("unsafe news URL"))?;
    if !acquire(st) {
        return Ok(());
    }
    match fetch_follow(url.as_str())
        .await
        .and_then(|t| parse_feed(&t).map_err(|e| anyhow::anyhow!(e)))
    {
        Ok(items) => upsert_news(st, game_id, &items, true, "rss").await?,
        Err(e) => {
            tracing::debug!("rss {game_id}: {e}");
            upsert_news(st, game_id, &[], false, "rss").await?;
        }
    }
    Ok(())
}

pub(crate) fn acquire(st: &AppState) -> bool {
    st.feed_gate
        .try_acquire(st.cfg.game_feed_per_min.max(1), Duration::from_secs(60))
}

pub(crate) async fn cache_due(
    st: &AppState,
    game_id: &str,
    ok_col: &str,
    at_col: &str,
    ttl: u64,
) -> anyhow::Result<bool> {
    // Column names are from a fixed allow-list, never user input.
    let sql = if ok_col == "details_ok" && at_col == "details_fetched_at" {
        "SELECT details_ok, details_fetched_at, details_fail_count FROM game_feed_cache WHERE game_id = $1"
    } else if ok_col == "news_ok" && at_col == "news_fetched_at" {
        "SELECT news_ok, news_fetched_at, news_fail_count FROM game_feed_cache WHERE game_id = $1"
    } else {
        anyhow::bail!("unknown cache columns");
    };
    let row: Option<(Option<bool>, Option<DateTime<Utc>>, i32)> = sqlx::query_as(sql)
        .bind(game_id)
        .fetch_optional(&st.db)
        .await?;
    let Some((ok, at, fails)) = row else {
        return Ok(true);
    };
    let Some(at) = at else {
        return Ok(true);
    };
    let age = Utc::now()
        .signed_duration_since(at)
        .to_std()
        .unwrap_or(Duration::from_secs(0));
    if ok == Some(true) {
        Ok(age >= Duration::from_secs(ttl))
    } else {
        let exp = fails.max(0) as u32;
        let factor = 2u64.saturating_pow(exp.min(5));
        let wait = st
            .cfg
            .game_feed_miss_ttl_secs
            .saturating_mul(factor)
            .min(BACKOFF_CAP_SECS);
        Ok(age >= Duration::from_secs(wait))
    }
}

pub(crate) async fn upsert_details(
    st: &AppState,
    game_id: &str,
    appid: Option<i32>,
    data: Option<&Value>,
    ok: bool,
    kind: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO game_feed_cache (
            game_id, steam_appid, details_json, details_fetched_at, details_ok, details_kind, details_fail_count
         ) VALUES ($1, $2, $3, now(), $4, $5, CASE WHEN $4 THEN 0 ELSE 1 END)
         ON CONFLICT (game_id) DO UPDATE SET
           steam_appid = coalesce(excluded.steam_appid, game_feed_cache.steam_appid),
           details_json = CASE WHEN $4 THEN excluded.details_json ELSE game_feed_cache.details_json END,
           details_fetched_at = excluded.details_fetched_at,
           details_ok = excluded.details_ok,
           details_kind = CASE WHEN $4 THEN excluded.details_kind ELSE game_feed_cache.details_kind END,
           details_fail_count = CASE WHEN $4 THEN 0 ELSE game_feed_cache.details_fail_count + 1 END",
    )
    .bind(game_id)
    .bind(appid)
    .bind(data)
    .bind(ok)
    .bind(kind)
    .execute(&st.db)
    .await?;
    Ok(())
}

pub(crate) async fn upsert_news(
    st: &AppState,
    game_id: &str,
    items: &[Value],
    ok: bool,
    kind: &str,
) -> anyhow::Result<()> {
    let payload = if ok { Some(json!(items)) } else { None };
    sqlx::query(
        "INSERT INTO game_feed_cache (game_id, news_json, news_fetched_at, news_ok, news_kind, news_fail_count)
         VALUES ($1, $2, now(), $3, $4, CASE WHEN $3 THEN 0 ELSE 1 END)
         ON CONFLICT (game_id) DO UPDATE SET
           news_json = CASE WHEN $3 THEN excluded.news_json ELSE game_feed_cache.news_json END,
           news_fetched_at = excluded.news_fetched_at,
           news_ok = excluded.news_ok,
           news_kind = excluded.news_kind,
           news_fail_count = CASE WHEN $3 THEN 0 ELSE game_feed_cache.news_fail_count + 1 END",
    )
    .bind(game_id)
    .bind(payload)
    .bind(ok)
    .bind(kind)
    .execute(&st.db)
    .await?;
    Ok(())
}

fn feed_client(follow: bool) -> anyhow::Result<reqwest::Client> {
    let redirect = if follow {
        reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.error(anyhow::anyhow!("too many redirects"));
            }
            if parse_public_http_url(attempt.url().as_str()).is_none() {
                return attempt.error(anyhow::anyhow!("redirect to a non-public URL"));
            }
            attempt.follow()
        })
    } else {
        reqwest::redirect::Policy::none()
    };
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(redirect)
        .user_agent(FEED_UA)
        .build()?)
}

async fn fetch_text(url: &str) -> anyhow::Result<String> {
    fetch_with(url, false).await
}

/// Follow a small number of redirects, but only to other public http(s) URLs.
pub(crate) async fn fetch_follow(url: &str) -> anyhow::Result<String> {
    fetch_with(url, true).await
}

async fn fetch_with(url: &str, follow: bool) -> anyhow::Result<String> {
    let res = feed_client(follow)?.get(url).send().await?;
    if !res.status().is_success() {
        anyhow::bail!("HTTP {}", res.status());
    }
    let bytes = res.bytes().await?;
    if bytes.len() > MAX_FEED_BYTES {
        anyhow::bail!("response too large");
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// http(s) URLs to public hosts only (blocks loopback / private / link-local).
pub fn parse_public_http_url(raw: &str) -> Option<Url> {
    let u = Url::parse(raw.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    if u.username() != "" || u.password().is_some() {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return None;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if !ip_is_public(ip) {
            return None;
        }
    }
    Some(u)
}

pub fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_multicast()
                || v.is_unspecified()
                || v.is_broadcast()
                || o[0] == 0
                || (o[0] == 100 && o[1] >= 64 && o[1] <= 127)
                || o[0] == 169 && o[1] == 254)
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            !(v.is_loopback()
                || v.is_multicast()
                || v.is_unspecified()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80)
        }
    }
}

pub fn parse_feed(xml: &str) -> Result<Vec<Value>, &'static str> {
    let body = xml;
    let mut items = Vec::new();
    for (tag, end) in [("<item", "</item>"), ("<entry", "</entry>")] {
        let mut rest = body;
        while let Some(start) = rest.find(tag) {
            let after = &rest[start..];
            let Some(close) = after.find('>') else { break };
            // skip if it's a self-close that isn't the body
            let inner_start = close + 1;
            let Some(stop) = after.find(end) else { break };
            if stop < inner_start {
                break;
            }
            let inner = &after[inner_start..stop];
            rest = &after[stop + end.len()..];
            if let Some(it) = feed_item(inner, tag == "<entry") {
                items.push(it);
                if items.len() >= MAX_NEWS {
                    return Ok(items);
                }
            }
        }
        if !items.is_empty() {
            break;
        }
    }
    Ok(items)
}

fn feed_item(inner: &str, atom: bool) -> Option<Value> {
    let title = text_tag(inner, "title")?
        .chars()
        .take(200)
        .collect::<String>();
    if title.trim().is_empty() {
        return None;
    }
    let url = if atom {
        atom_link(inner).unwrap_or_default()
    } else {
        text_tag(inner, "link")
            .or_else(|| text_tag(inner, "guid"))
            .unwrap_or_default()
    };
    let at = text_tag(inner, "pubDate")
        .or_else(|| text_tag(inner, "published"))
        .or_else(|| text_tag(inner, "updated"))
        .and_then(|s| parse_feed_date(&s));
    Some(json!({ "title": title.trim(), "url": url, "at": at }))
}

fn text_tag(s: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let i = s.to_ascii_lowercase().find(&open.to_ascii_lowercase())?;
    let after = &s[i + open.len()..];
    let gt = after.find('>')?;
    let rest = &after[gt + 1..];
    let j = rest
        .to_ascii_lowercase()
        .find(&close.to_ascii_lowercase())?;
    Some(unescape(&strip_cdata(&rest[..j])))
}

fn atom_link(s: &str) -> Option<String> {
    let low = s.to_ascii_lowercase();
    // prefer rel="alternate"
    let mut rest = s;
    let mut low_rest = low.as_str();
    let mut fallback = None;
    while let Some(i) = low_rest.find("<link") {
        let chunk = &rest[i..];
        let Some(end) = chunk.find('>') else { break };
        let tag = &chunk[..end];
        let href = attr(tag, "href");
        let rel = attr(tag, "rel");
        if rel.as_deref() == Some("alternate") {
            return href;
        }
        if fallback.is_none() {
            fallback = href;
        }
        rest = &chunk[end + 1..];
        low_rest = &low_rest[i + end + 1..];
    }
    fallback
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let low = tag.to_ascii_lowercase();
    let key = format!("{name}=");
    let i = low.find(&key)?;
    let after = tag[i + key.len()..].trim_start();
    let quote = after.chars().next()?;
    if quote == '"' || quote == '\'' {
        let rest = &after[1..];
        let j = rest.find(quote)?;
        Some(unescape(&rest[..j]))
    } else {
        let j = after
            .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .unwrap_or(after.len());
        Some(after[..j].to_string())
    }
}

fn strip_cdata(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t
        .strip_prefix("<![CDATA[")
        .and_then(|x| x.strip_suffix("]]>"))
    {
        inner.trim().to_string()
    } else {
        t.to_string()
    }
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn parse_feed_date(s: &str) -> Option<String> {
    let s = s.trim();
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.with_timezone(&Utc).to_rfc3339());
    }
    // Some feeds still say "GMT"; chrono's RFC 2822 parser wants a numeric offset.
    let rfc2822 = s.replace(" GMT", " +0000").replace(" UT", " +0000");
    if let Ok(t) = DateTime::parse_from_rfc2822(&rfc2822) {
        return Some(t.with_timezone(&Utc).to_rfc3339());
    }
    None
}

// ------------------------------------------------------------------ admin

async fn admin_get_page(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let meta = load_meta(&st.db, &id).await?;
    Ok(Json(json!({
        "game_id": id,
        "slug": meta.as_ref().and_then(|m| m.slug.clone()),
        "developer": meta.as_ref().and_then(|m| m.developer.clone()),
        "publisher": meta.as_ref().and_then(|m| m.publisher.clone()),
        "release_date": meta.as_ref().and_then(|m| m.release_date.clone()),
        "genres": meta.as_ref().and_then(|m| m.genres.clone()),
        "platforms": meta.as_ref().and_then(|m| m.platforms.clone()),
        "news_feed_url": meta.as_ref().and_then(|m| m.news_feed_url.clone()),
        "description": meta.as_ref().and_then(|m| m.description.clone()),
    })))
}

#[derive(Deserialize, Default)]
struct PageSave {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    developer: Option<String>,
    #[serde(default)]
    publisher: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    genres: Option<String>,
    #[serde(default)]
    platforms: Option<String>,
    #[serde(default)]
    news_feed_url: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

fn clean_field(s: Option<String>, max: usize) -> ApiResult<Option<String>> {
    let Some(s) = s else { return Ok(None) };
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    if t.chars().count() > max || t.chars().any(|c| c.is_control() && c != '\n') {
        return Err(AppError::bad_request(
            "invalid_field",
            format!("That value is too long or contains control characters (up to {max})."),
        ));
    }
    Ok(Some(t.to_string()))
}

async fn admin_save_page(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    Json(r): Json<PageSave>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    // Catalogue or games table must know this id.
    let cat = crate::game_history::public_catalogue(&st).await?;
    let known = cat.iter().any(|g| g.id == id)
        || sqlx::query_scalar::<_, bool>("SELECT exists(SELECT 1 FROM games WHERE id = $1)")
            .bind(&id)
            .fetch_one(&st.db)
            .await?;
    if !known {
        return Err(AppError::not_found());
    }
    let slug = match r.slug {
        None => None,
        Some(raw) => {
            let t = raw.trim();
            if t.is_empty() {
                None
            } else {
                let s = slugify(t);
                if s == "game" || s.len() < 2 {
                    return Err(AppError::bad_request(
                        "invalid_slug",
                        "Pick a longer page address made of letters and numbers.",
                    ));
                }
                Some(s)
            }
        }
    };
    if let Some(ref sl) = slug {
        let taken: Option<String> = sqlx::query_scalar(
            "SELECT game_id FROM game_page_meta WHERE slug = $1 AND game_id <> $2",
        )
        .bind(sl)
        .bind(&id)
        .fetch_optional(&st.db)
        .await?;
        if taken.is_some() {
            return Err(AppError::bad_request(
                "slug_taken",
                "That page address is already used by another game.",
            ));
        }
    }
    let news_url = match clean_field(r.news_feed_url, 400)? {
        None => None,
        Some(u) => {
            if parse_public_http_url(&u).is_none() {
                return Err(AppError::bad_request(
                    "invalid_url",
                    "News feed must be a public http or https URL.",
                ));
            }
            Some(u)
        }
    };
    let description = clean_field(r.description, 2000)?;
    sqlx::query(
        "INSERT INTO game_page_meta (
            game_id, slug, developer, publisher, release_date, genres, platforms, news_feed_url, description, updated_at, updated_by
         ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9, now(), $10)
         ON CONFLICT (game_id) DO UPDATE SET
            slug = excluded.slug,
            developer = excluded.developer,
            publisher = excluded.publisher,
            release_date = excluded.release_date,
            genres = excluded.genres,
            platforms = excluded.platforms,
            news_feed_url = excluded.news_feed_url,
            description = excluded.description,
            updated_at = now(),
            updated_by = excluded.updated_by",
    )
    .bind(&id)
    .bind(&slug)
    .bind(clean_field(r.developer, 120)?)
    .bind(clean_field(r.publisher, 120)?)
    .bind(clean_field(r.release_date, 80)?)
    .bind(clean_field(r.genres, 200)?)
    .bind(clean_field(r.platforms, 120)?)
    .bind(&news_url)
    .bind(&description)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    // Invalidate news cache when the feed URL changes so the next view refetches.
    if news_url.is_some() {
        sqlx::query(
            "UPDATE game_feed_cache SET news_fetched_at = NULL, news_ok = NULL WHERE game_id = $1",
        )
        .bind(&id)
        .execute(&st.db)
        .await?;
    }
    let _ = refresh_slugs(
        &st,
        &cat.iter()
            .map(|g| (g.id.clone(), g.name.clone(), g.steam_appid))
            .collect::<Vec<_>>(),
    )
    .await;
    admin_get_page(State(st), s, Path(id)).await
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_common_titles() {
        assert_eq!(
            slugify("World of Warcraft Forever"),
            "world-of-warcraft-forever"
        );
        assert_eq!(id_slug("wow_forever"), "wow-forever");
        assert_eq!(slugify("Baldur's Gate 3"), "baldurs-gate-3");
        assert_eq!(slugify("Counter-Strike 2"), "counter-strike-2");
        assert_eq!(slugify("  --Foo__Bar--  "), "foo-bar");
        assert_eq!(slugify("!!!"), "game");
    }

    #[test]
    fn slug_index_aliases_and_canonical() {
        let games = vec![
            (
                "wow_forever".into(),
                "World of Warcraft Forever".into(),
                None,
            ),
            ("cs2".into(), "Counter-Strike 2".into(), Some(730)),
            ("steam:1".into(), "Twin Name".into(), Some(1)),
            ("steam:2".into(), "Twin Name".into(), Some(2)),
        ];
        let idx = build_slug_index(&games, &HashMap::new());
        assert_eq!(idx.canonical["wow_forever"], "world-of-warcraft-forever");
        assert_eq!(idx.by_slug["wow-forever"], "wow_forever");
        assert_eq!(idx.by_slug["world-of-warcraft-forever"], "wow_forever");
        assert_eq!(idx.canonical["cs2"], "counter-strike-2");
        assert_eq!(idx.by_slug["cs2"], "cs2");
        assert_eq!(idx.by_slug["steam-730"], "cs2");
        // duplicate names fall back to the id kebab
        assert_eq!(idx.canonical["steam:1"], "steam-1");
        assert_eq!(idx.canonical["steam:2"], "steam-2");
    }

    #[test]
    fn admin_slug_wins() {
        let games = vec![("cs2".into(), "Counter-Strike 2".into(), Some(730))];
        let mut admin = HashMap::new();
        admin.insert("cs2".into(), "csgo".into());
        let idx = build_slug_index(&games, &admin);
        assert_eq!(idx.canonical["cs2"], "csgo");
        assert_eq!(idx.by_slug["counter-strike-2"], "cs2");
        assert_eq!(idx.by_slug["cs2"], "cs2");
    }

    #[test]
    fn trending_needs_jump_and_floor() {
        assert!(!is_trending(100, 10, 1.5, 1_000));
        assert!(is_trending(10_000, 1_000, 1.5, 1_000));
        assert!(!is_trending(1_200, 1_000, 1.5, 100));
        assert!(is_trending(5_000, 0, 1.5, 1_000));
    }

    #[test]
    fn public_urls_only() {
        assert!(parse_public_http_url("https://worldofwarcraft.blizzard.com/en-us/news").is_some());
        assert!(parse_public_http_url("http://example.com/feed.xml").is_some());
        assert!(parse_public_http_url("https://127.0.0.1/secret").is_none());
        assert!(parse_public_http_url("https://10.0.0.4/x").is_none());
        assert!(parse_public_http_url("https://localhost/x").is_none());
        assert!(parse_public_http_url("file:///etc/passwd").is_none());
        assert!(parse_public_http_url("https://user:pass@example.com/x").is_none());
    }

    #[test]
    fn rss_and_atom_parse() {
        let rss = r#"<?xml version="1.0"?><rss><channel>
            <item><title>Hotfixes</title><link>https://example.com/a</link><pubDate>Fri, 13 Nov 2026 00:00:00 GMT</pubDate></item>
            <item><title><![CDATA[Now Live!]]></title><guid>https://example.com/b</guid></item>
          </channel></rss>"#;
        let items = parse_feed(rss).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["title"], "Hotfixes");
        assert_eq!(items[0]["url"], "https://example.com/a");
        assert!(items[0]["at"].as_str().unwrap().starts_with("2026-11-13"));
        assert_eq!(items[1]["title"], "Now Live!");

        let atom = r#"<feed><entry><title>Ping</title><link rel="alternate" href="https://ex.com/p"/><updated>2026-11-04T00:00:00Z</updated></entry></feed>"#;
        let a = parse_feed(atom).unwrap();
        assert_eq!(a[0]["title"], "Ping");
        assert_eq!(a[0]["url"], "https://ex.com/p");
    }

    #[test]
    fn appdetails_and_news_parse() {
        let raw = r#"{"730":{"success":true,"data":{"developers":["Valve"],"publishers":["Valve"],
            "release_date":{"date":"21 Aug, 2012"},"genres":[{"description":"Action"},{"description":"Free To Play"}],
            "platforms":{"windows":true,"mac":false,"linux":true}}}}"#;
        let d = parse_appdetails(raw, 730).unwrap();
        let info = info_from_details(&d);
        assert_eq!(info.developer.as_deref(), Some("Valve"));
        assert_eq!(info.genres.as_deref(), Some("Action, Free To Play"));
        assert_eq!(info.platforms.as_deref(), Some("Windows, Linux"));

        let news = r#"{"appnews":{"newsitems":[{"title":"Counter-Strike 2 Update","gid":"123","date":1731369600}]}}"#;
        let n = parse_steam_news(news, 730).unwrap();
        assert_eq!(n[0]["title"], "Counter-Strike 2 Update");
        assert!(n[0]["url"]
            .as_str()
            .unwrap()
            .contains("/news/app/730/view/123"));
    }
}
