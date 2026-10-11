//! Home page data (`GET /api/v1/home`, signed in or out), Steam header art for game tiles
//! (`/api/v1/art/<appid>.jpg`, fetched once and cached on disk so browsers never talk to
//! Steam), and the shareable signature image (`/api/v1/sig/<handle>.png`).
//!
//! Privacy: signed out, only aggregate numbers and people who turned on "public profile"
//! appear. Signed in, top players are people who share their activity (like the game
//! leaderboards) and live games come from friends plus public profiles.

use crate::auth::MaybeUser;
use crate::error::ApiResult;
use crate::{avatars, friends, names, presence, AppState};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/home", get(home))
        .route("/api/v1/art/{file}", get(art))
        .route("/api/v1/sig/{file}", get(crate::sig::serve))
}

/// Steam app id for game ids (`steam:570`, or a games row with steam_appid).
pub async fn appids(st: &AppState, ids: &[String]) -> ApiResult<HashMap<String, i32>> {
    let mut out: HashMap<String, i32> = ids
        .iter()
        .filter_map(|i| {
            i.strip_prefix("steam:")
                .and_then(|n| n.parse().ok())
                .map(|n| (i.clone(), n))
        })
        .collect();
    let rows: Vec<(String, i32)> = sqlx::query_as(
        "SELECT id, steam_appid FROM games WHERE steam_appid IS NOT NULL AND id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(&st.db)
    .await?;
    out.extend(rows);
    Ok(out)
}

fn public_cache() -> &'static Mutex<Option<(Instant, Value)>> {
    static C: std::sync::OnceLock<Mutex<Option<(Instant, Value)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

async fn home(State(st): State<AppState>, m: MaybeUser) -> ApiResult<Json<Value>> {
    let viewer = m.0.as_ref().map(|a| a.user.id);
    if viewer.is_none() {
        if let Some((t, v)) = public_cache().lock().unwrap().as_ref() {
            if t.elapsed() < Duration::from_secs(60) {
                return Ok(Json(v.clone()));
            }
        }
    }
    let v = build(&st, viewer).await?;
    if viewer.is_none() {
        *public_cache().lock().unwrap() = Some((Instant::now(), v.clone()));
    }
    Ok(Json(v))
}

const LIVE_USER: &str = "u.deleted_at IS NULL AND u.banned_at IS NULL AND u.delete_after IS NULL";

pub async fn build(st: &AppState, viewer: Option<Uuid>) -> ApiResult<Value> {
    let signed_in = viewer.is_some();
    let friend_ids = match viewer {
        Some(v) => friends::friend_ids(st, v).await?,
        None => vec![],
    };

    // ---- numbers
    let (registered, online, in_game, week_s): (i64, i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT (SELECT count(*) FROM users u WHERE {LIVE_USER}),
                (SELECT count(*) FROM presence p JOIN users u ON u.id = p.user_id WHERE {LIVE_USER} AND p.status IN ('online','away')),
                (SELECT count(*) FROM presence p JOIN users u ON u.id = p.user_id WHERE {LIVE_USER} AND p.status IN ('online','away') AND p.game_id IS NOT NULL),
                (SELECT coalesce(sum(duration_s), 0)::bigint FROM sessions WHERE started_at >= now() - interval '7 days')"
    )))
    .fetch_one(&st.db)
    .await?;

    // ---- most played this week (aggregate; unlisted/hidden games never shown)
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT s.game_id, (sum(s.duration_s) / 60)::bigint, count(DISTINCT s.user_id)
         FROM sessions s WHERE s.started_at >= now() - interval '7 days'
           AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
         GROUP BY s.game_id HAVING sum(s.duration_s) >= 60 ORDER BY 2 DESC LIMIT 40",
    )
    .fetch_all(&st.db)
    .await?;
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let named = names::public_names(st, &ids).await?;
    let apps = appids(st, &ids).await?;
    let most: Vec<Value> = rows
        .iter()
        .filter(|r| named.contains_key(&r.0))
        .take(10)
        .map(|(id, min, players)| {
            json!({ "game_id": id, "name": named[id], "minutes": min, "players": players,
                    "steam_appid": apps.get(id), "icon_path": crate::icons::icon_path(id, 64) })
        })
        .collect();

    // ---- who's visible here
    let audience = if signed_in {
        "(u.public_profile OR u.id = ANY($1))"
    } else {
        "(u.public_profile OR (false AND u.id = ANY($1)))"
    };

    // ---- now playing
    let cand: Vec<(Uuid, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT u.id, u.handle, u.display_name FROM presence p JOIN users u ON u.id = p.user_id
         WHERE {LIVE_USER} AND {audience} AND {np} AND p.status IN ('online','away') AND p.game_id IS NOT NULL
           AND NOT p.game_hidden
         ORDER BY p.session_started_at DESC NULLS LAST LIMIT 40",
        np = crate::account::sql_shows("now_playing")
    )))
    .bind(&friend_ids)
    .fetch_all(&st.db)
    .await?;
    let cids: Vec<Uuid> = cand.iter().map(|c| c.0).collect();
    let pres = presence::visible_presence_many(st, &cids).await?;
    let mut live_ids = vec![];
    for (id, _, _) in &cand {
        if let Some(gid) = pres.get(id).and_then(|p| p.game_id.clone()) {
            live_ids.push(gid);
        }
    }
    let lnamed = names::public_names(st, &live_ids).await?;
    let lapps = appids(st, &live_ids).await?;
    let avs = avatars::many(st, &cids).await?;
    let mut now_playing = vec![];
    for (id, handle, dn) in &cand {
        let Some(p) = pres.get(id) else { continue };
        let Some(gid) = p.game_id.as_ref() else {
            continue;
        };
        let Some(gname) = lnamed.get(gid) else {
            continue;
        };
        now_playing.push(json!({
            "handle": handle, "display_name": dn, "avatar": avs.get(id),
            "game_id": gid, "game_name": gname, "steam_appid": lapps.get(gid),
            "since": p.session_started_at, "friend": friend_ids.contains(id),
        }));
        if now_playing.len() >= 12 {
            break;
        }
    }

    // ---- top players this week
    // Leaderboard: public profiles that show games & hours and opted into leaderboards.
    let share = format!(
        "u.public_profile AND {} AND {}",
        crate::account::sql_shows("games"),
        crate::account::sql_shows("leaderboards")
    );
    let tops: Vec<(Uuid, String, String, i64, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "WITH w AS (
            SELECT s.user_id, s.game_id, sum(s.duration_s) AS secs FROM sessions s
            WHERE s.started_at >= now() - interval '7 days'
              AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
            GROUP BY s.user_id, s.game_id)
         SELECT u.id, u.handle, u.display_name, (sum(w.secs) / 60)::bigint AS m,
                (array_agg(w.game_id ORDER BY w.secs DESC))[1]
         FROM w JOIN users u ON u.id = w.user_id
         WHERE {LIVE_USER} AND {share}
         GROUP BY u.id, u.handle, u.display_name HAVING sum(w.secs) >= 600
         ORDER BY m DESC, u.handle LIMIT 8"
    )))
    .fetch_all(&st.db)
    .await?;
    let tids: Vec<Uuid> = tops.iter().map(|t| t.0).collect();
    let tgames: Vec<String> = tops.iter().map(|t| t.4.clone()).collect();
    let tnamed = names::public_names(st, &tgames).await?;
    let tav = avatars::many(st, &tids).await?;
    let top_players: Vec<Value> = tops
        .iter()
        .map(|(id, h, dn, m, g)| {
            json!({ "handle": h, "display_name": dn, "minutes": m, "avatar": tav.get(id),
            "top_game": tnamed.get(g) })
        })
        .collect();

    // ---- newest members
    let newest_q = "u.public_profile";
    let newest: Vec<(Uuid, String, String, DateTime<Utc>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT u.id, u.handle, u.display_name, u.created_at FROM users u WHERE {LIVE_USER} AND {newest_q}
         ORDER BY u.created_at DESC LIMIT 12"
    )))
    .fetch_all(&st.db)
    .await?;
    let nids: Vec<Uuid> = newest.iter().map(|n| n.0).collect();
    let nav = avatars::many(st, &nids).await?;
    let newest: Vec<Value> = newest
        .iter()
        .map(|(id, h, dn, at)| json!({ "handle": h, "display_name": dn, "joined": at, "avatar": nav.get(id) }))
        .collect();

    // ---- news (staff announcements)
    let news: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', id, 'message', message, 'link', link, 'at', created_at) ORDER BY created_at DESC), '[]'::jsonb)
         FROM (SELECT * FROM announcements ORDER BY created_at DESC LIMIT 5) a",
    )
    .fetch_one(&st.db)
    .await?;

    // ---- your week
    let mut mine = Value::Null;
    if let Some(v) = viewer {
        let (w, total): (i64, i64) = sqlx::query_as(
            "SELECT coalesce((SELECT sum(minutes_7d) FROM user_game_stats WHERE user_id = $1), 0)::bigint,
                    coalesce((SELECT sum(display_min) FROM user_game_stats WHERE user_id = $1), 0)::bigint",
        )
        .bind(v)
        .fetch_one(&st.db)
        .await?;
        let top: Option<(String, i64)> = sqlx::query_as(
            "SELECT game_id, minutes_7d FROM user_game_stats WHERE user_id = $1 AND minutes_7d > 0 ORDER BY minutes_7d DESC LIMIT 1",
        )
        .bind(v)
        .fetch_optional(&st.db)
        .await?;
        let top_name = match &top {
            Some((g, _)) => names::public_names(st, std::slice::from_ref(g))
                .await?
                .remove(g)
                .or(Some(names::UNLISTED_TITLE.to_string())),
            None => None,
        };
        let fp = presence::visible_presence_many(st, &friend_ids).await?;
        let fonline = fp.values().filter(|p| p.status != "offline").count();
        let fgame = fp
            .values()
            .filter(|p| p.status != "offline" && p.game_id.is_some())
            .count();
        mine = json!({ "minutes_7d": w, "total_min": total,
            "top_game": top_name, "top_game_min": top.map(|t| t.1),
            "friends_online": fonline, "friends_in_game": fgame, "friends": friend_ids.len() });
    }

    Ok(json!({
        "signed_in": signed_in,
        "stats": { "registered": registered, "online": online, "in_game": in_game, "minutes_7d": week_s / 60 },
        "most_played": most,
        "now_playing": now_playing,
        "top_players": top_players,
        "newest": newest,
        "news": news,
        "me": mine,
    }))
}

// ------------------------------------------------------------------ Steam header art

fn art_dir(st: &AppState) -> std::path::PathBuf {
    st.cfg.data_dir.join("art")
}

async fn art(State(st): State<AppState>, Path(file): Path<String>) -> Response {
    let Some(appid) = file
        .strip_suffix(".jpg")
        .and_then(|n| n.parse::<u32>().ok())
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let d = art_dir(&st);
    let p = d.join(format!("{appid}.jpg"));
    let miss = d.join(format!("{appid}.miss"));
    let fresh = |path: &std::path::Path, secs: u64| {
        std::fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|e| e < Duration::from_secs(secs))
    };
    let send = |b: Vec<u8>| {
        (
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::CACHE_CONTROL, "public, max-age=604800"),
            ],
            Body::from(b),
        )
            .into_response()
    };
    if fresh(&p, 30 * 86400) {
        if let Ok(b) = tokio::fs::read(&p).await {
            return send(b);
        }
    }
    if fresh(&miss, 86400) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let url = format!("{}/{appid}/header.jpg", st.cfg.steam_art_cdn);
    let got = async {
        let r = reqwest::Client::new()
            .get(&url)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .ok()?;
        if !r.status().is_success() {
            return None;
        }
        let b = r.bytes().await.ok()?;
        (b.len() < 1_500_000 && b.starts_with(&[0xff, 0xd8])).then(|| b.to_vec())
    }
    .await;
    let _ = std::fs::create_dir_all(&d);
    match got {
        Some(b) => {
            let _ = std::fs::write(&p, &b);
            send(b)
        }
        None => {
            if let Ok(b) = tokio::fs::read(&p).await {
                return send(b); // stale copy beats nothing
            }
            let _ = std::fs::write(&miss, b"");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}
