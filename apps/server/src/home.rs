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
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/home", get(home))
        .route("/api/v1/home/week", get(home_week))
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

/// Signed out, the page is cached and its "this week" figures use the default timezone
/// (Australia/Sydney); signed in, they use `?tz=` (the browser's), like `/home/week`.
async fn home(
    State(st): State<AppState>,
    m: MaybeUser,
    axum::extract::Query(q): axum::extract::Query<WeekQuery>,
) -> ApiResult<Json<Value>> {
    let viewer = m.0.as_ref().map(|a| a.user.id);
    if viewer.is_none() {
        if let Some((t, v)) = st
            .home_public
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if t.elapsed() < Duration::from_secs(60) {
                return Ok(Json(v.clone()));
            }
        }
    }
    let tz = crate::activity::tz_or_default(&st, viewer.and(q.tz.as_deref())).await;
    let v = build(&st, viewer, &tz).await?;
    if viewer.is_none() {
        *st.home_public.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((Instant::now(), v.clone()));
    }
    Ok(Json(v))
}

#[derive(serde::Deserialize, Default)]
pub struct WeekQuery {
    pub tz: Option<String>,
}

/// Signed-in home "this week" block: headline hours, daily bars, per-game totals and the
/// friends leaderboard, all from [`crate::week`] (see there for the definition).
async fn home_week(
    State(st): State<AppState>,
    a: crate::auth::AuthUser,
    axum::extract::Query(q): axum::extract::Query<WeekQuery>,
) -> ApiResult<Json<Value>> {
    st.rate_limit(
        format!("homeweek:{}", a.user.id),
        120,
        Duration::from_secs(600),
    )?;
    let tz = crate::activity::tz_or_default(&st, q.tz.as_deref()).await;
    Ok(Json(
        week_json(&st, a.user.id, &tz, chrono::Utc::now()).await?,
    ))
}

fn hm(min: i64) -> String {
    format!("{}h {:02}m", min / 60, min % 60)
}

/// The week block for `me` at `now` (public so tests can pin the clock).
pub async fn week_json(
    st: &AppState,
    me: Uuid,
    tz_name: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> ApiResult<Value> {
    use crate::week;
    let tz = week::parse_tz(tz_name);
    let w = week::window(tz, now);
    let friend_ids = friends::public_friend_ids(st, me).await?;
    // leaderboard: you, plus friends whose games are visible to friends
    let shown: Vec<(Uuid, String, String)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT u.id, u.handle, u.display_name FROM users u
         WHERE (u.id = $1 OR (u.id = ANY($2) AND {}))
           AND u.deleted_at IS NULL AND u.banned_at IS NULL",
        crate::account::sql_shows("games")
    )))
    .bind(me)
    .bind(&friend_ids)
    .fetch_all(&st.db)
    .await?;
    let ids: Vec<Uuid> = shown.iter().map(|r| r.0).collect();
    let data = week::load(st, w.clone(), &ids, Some(me)).await?;
    let mine = data.user(me);
    let secs = mine.seconds();
    let total_t = week::tenths(secs);
    let total_m = week::minutes(secs);
    let day_s = mine.days();
    let day_t = week::apportion(total_t, &day_s);
    let day_m = week::apportion(total_m, &day_s);
    let days: Vec<Value> = (0..week::DAYS)
        .map(|i| {
            let d = w.dates[i];
            json!({
                "date": d.to_string(),
                "dow": d.format("%a").to_string(),
                "label": d.format("%a %-d %b").to_string(),
                "today": i == week::DAYS - 1,
                "seconds": day_s[i],
                "tenths": day_t[i],
                "minutes": day_m[i],
                "hm": hm(day_m[i]),
            })
        })
        .collect();
    // games in the home page's order with their tenths (the same split every surface uses)
    let shown_wk = mine.shown(|_| true);
    let games = &shown_wk.games;
    let gids: Vec<String> = games.iter().map(|g| g.game_id.clone()).collect();
    let named = names::public_names(st, &gids).await?;
    let own = presence::own_names_for(st, me, &gids).await?;
    let apps = appids(st, &gids).await?;
    let name_of = |id: &str| {
        named
            .get(id)
            .or_else(|| own.get(id))
            .cloned()
            .unwrap_or_else(|| names::UNLISTED_TITLE.to_string())
    };
    let games_json: Vec<Value> = games
        .iter()
        .map(|g| {
            json!({
                "game_id": g.game_id, "name": name_of(&g.game_id), "steam_appid": apps.get(&g.game_id),
                "seconds": g.seconds, "tenths": g.tenths, "minutes": g.minutes, "last_played_at": g.last_played,
            })
        })
        .collect();
    let top = games
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.seconds.cmp(&b.1.seconds).then(b.0.cmp(&a.0)))
        .map(|(_, g)| json!({ "game_id": g.game_id, "name": name_of(&g.game_id), "seconds": g.seconds, "tenths": g.tenths }));
    let mut board: Vec<Value> = Vec::new();
    for (id, handle, name) in &shown {
        let s = data.user(*id).seconds();
        if *id != me && s <= 0 {
            continue;
        }
        let t = if *id == me { total_t } else { week::tenths(s) };
        board.push(json!({ "handle": handle, "display_name": name, "me": *id == me, "seconds": s, "tenths": t }));
    }
    board.sort_by(|a, b| {
        b["seconds"]
            .as_i64()
            .cmp(&a["seconds"].as_i64())
            .then(a["display_name"].as_str().cmp(&b["display_name"].as_str()))
    });
    // All time: each person's lifetime total exactly as their Profile shows it to you
    // (tracked + imported, games they hide from others left out; your own hidden games count).
    let all_rows: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT g.user_id, coalesce(sum(g.display_min), 0)::bigint FROM user_game_stats g
         WHERE g.user_id = ANY($1)
           AND (g.user_id = $2 OR NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = g.user_id AND h.game_id = g.game_id))
         GROUP BY g.user_id",
    )
    .bind(&ids)
    .bind(me)
    .fetch_all(&st.db)
    .await?;
    let all_min: std::collections::HashMap<Uuid, i64> = all_rows.into_iter().collect();
    let mut board_all: Vec<Value> = Vec::new();
    for (id, handle, name) in &shown {
        let m = all_min.get(id).copied().unwrap_or(0);
        if *id != me && m <= 0 {
            continue;
        }
        board_all
            .push(json!({ "handle": handle, "display_name": name, "me": *id == me, "minutes": m }));
    }
    board_all.sort_by(|a, b| {
        b["minutes"]
            .as_i64()
            .cmp(&a["minutes"].as_i64())
            .then(a["display_name"].as_str().cmp(&b["display_name"].as_str()))
    });
    let first = w.dates[0];
    let last = w.dates[week::DAYS - 1];
    Ok(json!({
        "tz": tz.name(),
        "start": chrono::DateTime::from_timestamp(w.bounds[0], 0),
        "end": chrono::DateTime::from_timestamp(w.bounds[week::DAYS], 0),
        "label": format!("{} – {}", first.format("%a %-d %b"), last.format("%a %-d %b")),
        "seconds": secs, "tenths": total_t, "minutes": total_m,
        "days": days,
        "games": games_json,
        "top_game": top,
        "board": board,
        "board_all": board_all,
        "friends": friend_ids.len(),
    }))
}

const LIVE_USER: &str =
    "u.deleted_at IS NULL AND u.banned_at IS NULL AND u.delete_after IS NULL AND NOT u.is_test";
/// Sessions of test accounts never count in public totals.
const NO_TEST_SESSIONS: &str = "s.user_id NOT IN (SELECT id FROM users WHERE is_test)";

/// The home page. Every "this week" figure is tracked play only, over the [`crate::week`]
/// window in `tz` (imported hours never count in a week), with the same per-session rule and
/// rounding as `/home/week`, so a person's figures here equal their own home headline.
pub async fn build(st: &AppState, viewer: Option<Uuid>, tz: &str) -> ApiResult<Value> {
    let signed_in = viewer.is_some();
    let w = crate::week::window(crate::week::parse_tz(tz), chrono::Utc::now());
    let (wa, wb) = (w.bounds[0], w.bounds[crate::week::DAYS]);
    let secs = crate::week::secs_sql("$1", "$2");
    let inw = crate::week::in_window_sql("$1", "$2");
    let friend_ids = match viewer {
        Some(v) => friends::public_friend_ids(st, v).await?,
        None => vec![],
    };

    // ---- numbers
    let (registered, online, in_game, week_s): (i64, i64, i64, i64) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT (SELECT count(*) FROM users u WHERE {LIVE_USER}),
                (SELECT count(*) FROM presence p JOIN users u ON u.id = p.user_id WHERE {LIVE_USER} AND p.status IN ('online','away')),
                (SELECT count(*) FROM presence p JOIN users u ON u.id = p.user_id WHERE {LIVE_USER} AND p.status IN ('online','away') AND p.game_id IS NOT NULL),
                (SELECT coalesce(sum({secs}), 0)::bigint FROM play_sessions s WHERE {inw} AND {NO_TEST_SESSIONS})"
    )))
    .bind(wa)
    .bind(wb)
    .fetch_one(&st.db)
    .await?;

    // ---- most played this week (aggregate; unlisted/hidden games never shown)
    // (tracked play only, crate::week window: imported hours never count in a week)
    // Private profiles still count (it's an anonymous total), but deleted, banned, pending-delete
    // and test accounts don't. No player count is shown, so a tile can't point at one person.
    let rows: Vec<(String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT s.game_id, sum({secs})::bigint
         FROM play_sessions s JOIN users u ON u.id = s.user_id
         WHERE {inw} AND {LIVE_USER}
           AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
         GROUP BY s.game_id
         HAVING sum({secs}) >= 30
         ORDER BY 2 DESC, 1 LIMIT 40"
    )))
    .bind(wa)
    .bind(wb)
    .fetch_all(&st.db)
    .await?;
    let rows: Vec<(String, i64)> = rows
        .into_iter()
        .map(|(g, s)| (g, crate::week::minutes(s)))
        .collect();
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let named = names::public_names(st, &ids).await?;
    let apps = appids(st, &ids).await?;
    let most: Vec<Value> = rows
        .iter()
        .filter(|r| named.contains_key(&r.0))
        .take(10)
        .map(|(id, min)| {
            json!({ "game_id": id, "name": named[id], "minutes": min,
                    "steam_appid": apps.get(id), "icon_path": crate::icons::icon_path(id, 64) })
        })
        .collect();

    // ---- who's visible here (public profiles, plus your friends when signed in)
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
        // beta.6: a program they added comes with the name they gave it
        let own = gid
            .starts_with("exe:")
            .then_some(())
            .and(p.game_name.as_ref());
        let Some(gname) = lnamed.get(gid).or(own) else {
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
            SELECT s.user_id, s.game_id, sum({secs})::bigint AS secs FROM play_sessions s
            WHERE {inw}
              AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
            GROUP BY 1, 2)
         SELECT u.id, u.handle, u.display_name, sum(w.secs)::bigint AS secs,
                (array_agg(w.game_id ORDER BY w.secs DESC, w.game_id))[1]
         FROM w JOIN users u ON u.id = w.user_id
         WHERE {LIVE_USER} AND {share}
         GROUP BY u.id, u.handle, u.display_name HAVING sum(w.secs) >= 570
         ORDER BY secs DESC, u.handle LIMIT 8"
    )))
    .bind(wa)
    .bind(wb)
    .fetch_all(&st.db)
    .await?;
    let tids: Vec<Uuid> = tops.iter().map(|t| t.0).collect();
    let tgames: Vec<String> = tops.iter().map(|t| t.4.clone()).collect();
    let tnamed = names::public_names(st, &tgames).await?;
    let tav = avatars::many(st, &tids).await?;
    let top_players: Vec<Value> = tops
        .iter()
        .map(|(id, h, dn, secs, g)| {
            json!({ "handle": h, "display_name": dn, "minutes": crate::week::minutes(*secs),
            "seconds": secs, "tenths": crate::week::tenths(*secs), "avatar": tav.get(id),
            "top_game": tnamed.get(g) })
        })
        .collect();

    // ---- news (Admin > News posts; pinned first)
    let news = crate::news::latest(st, 4).await?;

    // ---- your week
    let mut mine = Value::Null;
    if let Some(v) = viewer {
        // your own week, as on /home/week (your hidden games count for you)
        let wk = crate::week::load(st, w.clone(), &[v], Some(v))
            .await?
            .user(v)
            .shown(|_| true);
        let week_min = wk.minutes;
        let (total,): (i64,) = sqlx::query_as(
            "SELECT coalesce((SELECT sum(display_min) FROM user_game_stats WHERE user_id = $1), 0)::bigint",
        )
        .bind(v)
        .fetch_one(&st.db)
        .await?;
        let top: Option<(String, i64)> = wk
            .games
            .iter()
            .enumerate()
            .filter(|(_, g)| g.seconds > 0)
            .max_by(|a, b| a.1.seconds.cmp(&b.1.seconds).then(b.0.cmp(&a.0)))
            .map(|(_, g)| (g.game_id.clone(), g.minutes));
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
        mine = json!({ "minutes_7d": week_min, "tenths_7d": wk.tenths, "total_min": total,
            "top_game": top_name, "top_game_min": top.map(|t| t.1),
            "friends_online": fonline, "friends_in_game": fgame, "friends": friend_ids.len() });
    }

    // ---- bottom stats strip: all-time hours (no test accounts)
    let (total_s,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT coalesce(sum(s.duration_s), 0)::bigint FROM play_sessions s WHERE {NO_TEST_SESSIONS}"
    )))
    .fetch_one(&st.db)
    .await?;

    Ok(json!({
        "signed_in": signed_in,
        "counts_style": crate::features::home_counts(st).await,
        "stats_strip": crate::features::on(st, "home_stats").await,
        "stats": { "registered": registered, "online": online, "in_game": in_game, "minutes_7d": crate::week::minutes(week_s), "minutes_total": total_s / 60 },
        "most_played": most,
        "now_playing": now_playing,
        "top_players": top_players,
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
