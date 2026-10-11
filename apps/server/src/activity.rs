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
);

/// Friends' activity: who's playing now, then one line per friend per game per day with
/// the day's total (`tz` splits days; IANA name).
pub async fn recent(st: &AppState, me: Uuid, tz: &str) -> ApiResult<Vec<ActivityItem>> {
    let ids = friends::friend_ids(st, me).await?;
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
                });
            }
        }
    }
    let since = Utc::now() - Duration::days(ACTIVITY_DAYS);
    let rows: Vec<SessRow> = sqlx::query_as(
        "SELECT u.handle, u.display_name, s.game_id, (s.ended_at AT TIME ZONE $4)::date::text AS day,
                max(s.ended_at), sum(s.duration_s)::bigint, count(*)
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.user_id = ANY($1) AND u.share_activity AND u.deleted_at IS NULL
           AND s.ended_at >= $2 AND s.duration_s >= 60
           AND NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id)
         GROUP BY u.handle, u.display_name, s.game_id, day
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
    for (handle, name, gid, day, ended, secs, n) in rows {
        let (gid, gname) = match gid {
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
        });
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.at));
    out.truncate(ACTIVITY_LIMIT as usize);
    Ok(out)
}

fn week_start(now: DateTime<Utc>) -> DateTime<Utc> {
    use chrono::{Datelike, TimeZone};
    let d = now.date_naive() - Duration::days(now.weekday().num_days_from_monday() as i64);
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}

pub async fn summary(st: &AppState, me: Uuid, handle: &str) -> ApiResult<FriendSummary> {
    let p = friends::find_by_handle(st, handle).await?;
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
        presence::visible_presence(st, me)
            .await?
            .unwrap_or_else(presence::PresenceView::offline)
    } else {
        presence::visible_presence_many(st, &[p.id])
            .await?
            .remove(&p.id)
            .unwrap_or_else(presence::PresenceView::offline)
    };
    let (mut week, mut d7, mut top) = (None, None, None);
    if shares || p.id == me {
        let now = Utc::now();
        // Clip sessions to the window so a session that started before it only counts its tail.
        let secs = |from: DateTime<Utc>| {
            sqlx::query_scalar::<_, Option<i64>>(
                "SELECT sum(LEAST(duration_s, GREATEST(0, EXTRACT(EPOCH FROM (ended_at - $2))::bigint)))::bigint
                 FROM sessions WHERE user_id = $1 AND ended_at > $2",
            )
            .bind(p.id)
            .bind(from)
            .fetch_one(&st.db)
        };
        week = Some(secs(week_start(now)).await?.unwrap_or(0));
        d7 = Some(secs(now - Duration::days(7)).await?.unwrap_or(0));
        let top_id: Option<String> = sqlx::query_scalar(
            "SELECT s.game_id FROM sessions s
             WHERE s.user_id = $1 AND s.ended_at > $2
             GROUP BY 1 ORDER BY sum(s.duration_s) DESC LIMIT 1",
        )
        .bind(p.id)
        .bind(now - Duration::days(7))
        .fetch_optional(&st.db)
        .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    #[test]
    fn week_starts_monday_utc() {
        let t = Utc.with_ymd_and_hms(2026, 10, 3, 15, 0, 0).unwrap(); // Saturday
        assert_eq!(
            week_start(t),
            Utc.with_ymd_and_hms(2026, 9, 28, 0, 0, 0).unwrap()
        );
        let m = Utc.with_ymd_and_hms(2026, 9, 28, 0, 0, 1).unwrap();
        assert_eq!(
            week_start(m),
            Utc.with_ymd_and_hms(2026, 9, 28, 0, 0, 0).unwrap()
        );
    }
}
