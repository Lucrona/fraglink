//! Friends' recent activity (side panel "Activity Report") and per-friend summaries.
//! Only accepted friends are visible, and history is gated by each user's `share_activity`.
//! Live presence keeps its own rules (invisible/offline are never revealed).

use crate::error::{ApiResult, AppError};
use crate::friends;
use crate::presence;
use crate::AppState;
use axum::http::StatusCode;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use uuid::Uuid;

pub const ACTIVITY_DAYS: i64 = 7;
pub const ACTIVITY_LIMIT: i64 = 30;

#[derive(Debug, Serialize)]
pub struct ActivityItem {
    /// "playing" (live, from presence) or "played" (finished session).
    pub kind: &'static str,
    pub handle: String,
    pub display_name: String,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub at: DateTime<Utc>,
    /// For "played": total time on that game that day (all sessions added up).
    pub duration_s: Option<i64>,
    /// For "played": local calendar day (YYYY-MM-DD, in the requested time zone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,
    /// For "played": how many sessions were added up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<i64>,
    /// Per-session mode: when that session started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
}

/// Time zone used to split days when the caller doesn't send one.
pub const DEFAULT_TZ: &str = "Australia/Sydney";

/// A valid IANA zone name (checked against Postgres), else the default.
pub async fn tz_or_default(st: &AppState, tz: Option<&str>) -> String {
    let Some(tz) = tz.filter(|t| t.len() <= 64) else {
        return DEFAULT_TZ.into();
    };
    match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pg_timezone_names WHERE name = $1)",
    )
    .bind(tz)
    .fetch_one(&st.db)
    .await
    {
        Ok(true) => tz.to_string(),
        _ => DEFAULT_TZ.into(),
    }
}

#[derive(Debug, Serialize)]
pub struct FriendSummary {
    pub handle: String,
    pub display_name: String,
    pub presence: presence::PresenceView,
    pub shares_activity: bool,
    /// Tracked seconds since Monday 00:00 UTC (None when the friend doesn't share history).
    pub week_seconds: Option<i64>,
    pub last_7d_seconds: Option<i64>,
    pub top_game_7d: Option<String>,
}

type SessRow = (
    String,
    String,
    Option<String>,
    String,
    DateTime<Utc>,
    i64,
    i64,
    Option<String>,
);

/// Friends' activity: who's playing now, then one line per friend per game per day with
/// the day's total (`tz` splits days; IANA name).
pub async fn recent(st: &AppState, me: Uuid, tz: &str) -> ApiResult<Vec<ActivityItem>> {
    recent_mode(st, me, tz, false).await
}

/// Home page feed: games friends are playing now plus one row per finished session
/// (`duration_s` = that session's played time). A finished row that is the same session
/// as a live one (same friend and game, started within a minute) is left out, and a
/// session stored twice shows once.
pub async fn recent_sessions(st: &AppState, me: Uuid, tz: &str) -> ApiResult<Vec<ActivityItem>> {
    recent_mode(st, me, tz, true).await
}

async fn recent_mode(
    st: &AppState,
    me: Uuid,
    tz: &str,
    per_session: bool,
) -> ApiResult<Vec<ActivityItem>> {
    let ids = friends::public_friend_ids(st, me).await?;
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let people: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, handle, display_name FROM users WHERE id = ANY($1) AND deleted_at IS NULL",
    )
    .bind(&ids)
    .fetch_all(&st.db)
    .await?;
    let pres = presence::visible_presence_many(st, &ids).await?;
    let mut out = Vec::new();
    for (id, handle, name) in &people {
        if let Some(p) = pres.get(id) {
            if let (Some(started), true) = (p.session_started_at, p.game_name.is_some()) {
                out.push(ActivityItem {
                    kind: "playing",
                    handle: handle.clone(),
                    display_name: name.clone(),
                    game_id: p.game_id.clone(),
                    game_name: p.game_name.clone(),
                    at: started,
                    duration_s: None,
                    day: None,
                    sessions: None,
                    started_at: Some(started),
                });
            }
        }
    }
    let since = Utc::now() - Duration::days(ACTIVITY_DAYS);
    if per_session {
        type One = (
            String,
            String,
            String,
            DateTime<Utc>,
            DateTime<Utc>,
            i32,
            String,
            Option<String>,
        );
        let rows: Vec<One> = sqlx::query_as(
            "SELECT * FROM (
               SELECT DISTINCT ON (s.user_id, s.game_id, date_trunc('second', s.started_at))
                      u.handle, u.display_name, s.game_id, s.started_at, s.ended_at, s.duration_s,
                      (s.ended_at AT TIME ZONE $4)::date::text AS day,
                      (SELECT o.name FROM own_game_names o WHERE o.user_id = u.id AND o.game_id = s.game_id)
               FROM sessions s JOIN users u ON u.id = s.user_id
               WHERE s.user_id = ANY($1) AND u.share_activity AND u.deleted_at IS NULL
                 AND s.ended_at >= $2 AND s.duration_s >= 60
                 AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
               ORDER BY s.user_id, s.game_id, date_trunc('second', s.started_at), s.ended_at DESC
             ) x ORDER BY x.ended_at DESC LIMIT $3",
        )
        .bind(&ids)
        .bind(since)
        .bind(ACTIVITY_LIMIT)
        .bind(tz)
        .fetch_all(&st.db)
        .await?;
        let gids: Vec<String> = rows.iter().map(|r| r.2.clone()).collect();
        let names = crate::names::public_names(st, &gids).await?;
        let live: Vec<(String, Option<String>, DateTime<Utc>)> = out
            .iter()
            .map(|i| (i.handle.clone(), i.game_id.clone(), i.at))
            .collect();
        for (handle, name, g, started, ended, secs, day, own) in rows {
            let (gid, gname) = if !names.contains_key(&g) && own.is_some() {
                (g, own)
            } else {
                let (i, n) = crate::names::public_pair(&g, &names);
                (i, Some(n))
            };
            let dup = live.iter().any(|(h, lg, at)| {
                *h == handle
                    && lg.as_deref() == Some(gid.as_str())
                    && (*at - started).num_seconds().abs() <= 60
            });
            if dup {
                continue;
            }
            out.push(ActivityItem {
                kind: "played",
                handle,
                display_name: name,
                game_id: Some(gid),
                game_name: gname,
                at: ended,
                duration_s: Some(secs as i64),
                day: Some(day),
                sessions: Some(1),
                started_at: Some(started),
            });
        }
        out.sort_by_key(|x| std::cmp::Reverse(x.at));
        out.truncate(ACTIVITY_LIMIT as usize);
        return Ok(out);
    }
    let rows: Vec<SessRow> = sqlx::query_as(
        "SELECT u.handle, u.display_name, s.game_id, (s.ended_at AT TIME ZONE $4)::date::text AS day,
                max(s.ended_at), sum(s.duration_s)::bigint, count(*),
                (SELECT o.name FROM own_game_names o WHERE o.user_id = u.id AND o.game_id = s.game_id)
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.user_id = ANY($1) AND u.share_activity AND u.deleted_at IS NULL
           AND s.ended_at >= $2 AND s.duration_s >= 60
           AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
         GROUP BY u.id, u.handle, u.display_name, s.game_id, day
         ORDER BY max(s.ended_at) DESC LIMIT $3",
    )
    .bind(&ids)
    .bind(since)
    .bind(ACTIVITY_LIMIT)
    .bind(tz)
    .fetch_all(&st.db)
    .await?;
    let gids: Vec<String> = rows.iter().filter_map(|r| r.2.clone()).collect();
    let names = crate::names::public_names(st, &gids).await?;
    for (handle, name, gid, day, ended, secs, n, own) in rows {
        let (gid, gname) = match gid {
            // beta.6: a program they added shows the name they gave it
            Some(g) if !names.contains_key(&g) && own.is_some() => (Some(g), own),
            Some(g) => {
                let (i, n) = crate::names::public_pair(&g, &names);
                (Some(i), Some(n))
            }
            None => (None, None),
        };
        out.push(ActivityItem {
            kind: "played",
            handle,
            display_name: name,
            game_id: gid,
            game_name: gname,
            at: ended,
            duration_s: Some(secs),
            day: Some(day),
            sessions: Some(n),
            started_at: None,
        });
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.at));
    out.truncate(ACTIVITY_LIMIT as usize);
    Ok(out)
}

/// A friend's card. `week_seconds` and `last_7d_seconds` are both the friend's week exactly as
/// the home page counts it ([`crate::week`] in `tz`: tracked play only, today and the 6 days
/// before, games they hide from others left out), so the card matches their headline.
/// `top_game_7d` is the game they played most in it.
pub async fn summary(st: &AppState, me: Uuid, handle: &str, tz: &str) -> ApiResult<FriendSummary> {
    let p = friends::find_visible(st, Some(me), handle).await?;
    if p.id != me && !friends::are_friends(st, me, p.id).await? {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "not_friends",
            "You can only see friends' activity.",
        ));
    }
    let shares: bool = sqlx::query_scalar("SELECT share_activity FROM users WHERE id = $1")
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
    let presence = if p.id == me {
        let v = presence::visible_presence(st, me)
            .await?
            .unwrap_or_else(presence::PresenceView::offline);
        v
    } else {
        presence::visible_presence_many(st, &[p.id])
            .await?
            .remove(&p.id)
            .unwrap_or_else(presence::PresenceView::offline)
    };
    let (mut week, mut d7, mut top) = (None, None, None);
    if shares || p.id == me {
        // the game running now counts too (week::load reads play_sessions)
        let wk = crate::week::for_user(st, p.id, tz, p.id == me)
            .await?
            .shown(|_| true);
        week = Some(wk.seconds);
        d7 = Some(wk.seconds);
        let top_id: Option<String> = wk
            .games
            .iter()
            .enumerate()
            .filter(|(_, g)| g.seconds > 0)
            .max_by(|a, b| a.1.seconds.cmp(&b.1.seconds).then(b.0.cmp(&a.0)))
            .map(|(_, g)| g.game_id.clone());
        if let Some(id) = top_id {
            top = Some(
                crate::names::public_name(st, &id)
                    .await?
                    .unwrap_or_else(|| crate::names::UNLISTED_TITLE.to_string()),
            );
        }
    }
    Ok(FriendSummary {
        handle: p.handle,
        display_name: p.display_name,
        presence,
        shares_activity: shares,
        week_seconds: week,
        last_7d_seconds: d7,
        top_game_7d: top,
    })
}
