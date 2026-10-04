//! Profile page data for the website (`/u/<handle>`): one read-only call.
//!
//! `GET /api/v1/users/{handle}/profile` (signed in). Everyone signed in sees the name,
//! member-since date and friend count. Live presence is shown to friends only (same
//! rules as the friends list: invisible = offline, hidden games stay hidden). The
//! games table and recent sessions are shown to the user themselves, and to friends
//! when the user shares activity. Games the server can't name never expose their id.

use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::{api, friends, names, presence, AppState};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const RECENT_DAYS: i64 = 14;
pub const RECENT_LIMIT: i64 = 20;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/users/{handle}/profile", get(profile))
        .route("/api/v1/me/display_name", post(set_display_name))
}

#[derive(Deserialize)]
struct DisplayNameReq {
    display_name: String,
}

/// Change the name shown next to the username (1-32 characters; empty = the username).
async fn set_display_name(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<DisplayNameReq>,
) -> ApiResult<Json<Value>> {
    if a.via_cookie {
        api::same_origin(&headers, &st)?;
    }
    st.rate_limit(
        format!("displayname:{}", a.user.id),
        20,
        std::time::Duration::from_secs(900),
    )?;
    let name: String = r
        .display_name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    let name = if name.is_empty() {
        a.user.handle.clone()
    } else {
        name
    };
    if name.chars().count() > 32 {
        return Err(AppError::bad_request(
            "invalid_display_name",
            "Display names are up to 32 characters.",
        ));
    }
    sqlx::query("UPDATE users SET display_name = $2 WHERE id = $1")
        .bind(a.user.id)
        .bind(&name)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "display_name": name })))
}

async fn profile(
    State(st): State<AppState>,
    a: AuthUser,
    Path(handle): Path<String>,
) -> ApiResult<Json<Value>> {
    st.rate_limit(
        format!("profile:{}", a.user.id),
        240,
        std::time::Duration::from_secs(600),
    )?;
    let p = friends::find_by_handle(&st, &handle).await?;
    let me = a.user.id;
    let is_self = p.id == me;
    if !is_self {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blocks WHERE (blocker = $1 AND blocked = $2) OR (blocker = $2 AND blocked = $1))",
        )
        .bind(me)
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
        if blocked {
            return Err(AppError::new(
                StatusCode::NOT_FOUND,
                "user_not_found",
                "No Fraglink user with that name.",
            ));
        }
    }
    let (created, shares): (DateTime<Utc>, bool) =
        sqlx::query_as("SELECT created_at, share_activity FROM users WHERE id = $1")
            .bind(p.id)
            .fetch_one(&st.db)
            .await?;
    let friend_count = friends::friend_ids(&st, p.id).await?.len();
    let relation = if is_self {
        "self"
    } else if friends::are_friends(&st, me, p.id).await? {
        "friend"
    } else {
        let mine = friends::list(&st, me).await?;
        if mine.incoming.iter().any(|r| r.handle == p.handle) {
            "incoming"
        } else if mine.outgoing.iter().any(|r| r.handle == p.handle) {
            "outgoing"
        } else {
            "none"
        }
    };
    let can_see_presence = is_self || relation == "friend";
    let can_see_history = is_self || (relation == "friend" && shares);

    let presence = if can_see_presence {
        let mut m = presence::visible_presence_many(&st, &[p.id]).await?;
        Some(
            m.remove(&p.id)
                .unwrap_or_else(presence::PresenceView::offline),
        )
    } else {
        None
    };

    let (mut games, mut recent) = (Vec::new(), Vec::new());
    let (mut total_min, mut min_7d) = (0i64, 0i64);
    if can_see_history {
        let rows = api::game_stats(&st, p.id).await?;
        let ids: Vec<String> = rows.iter().map(|r| r.game_id.clone()).collect();
        let named = names::public_names(&st, &ids).await?;
        for g in rows {
            total_min += g.display_min;
            min_7d += g.minutes_7d;
            let listed = named.contains_key(&g.game_id);
            if g.display_min == 0 && g.minutes_7d == 0 && g.estimated_min == 0 {
                continue;
            }
            games.push(json!({
                "game_id": if listed || is_self { g.game_id.clone() } else { names::UNLISTED_ID.to_string() },
                "name": g.name,
                "listed": listed,
                "minutes_7d": g.minutes_7d,
                "total_min": g.display_min,
                "estimated_min": g.estimated_min,
                "tier": g.display_tier,
                "last_played_at": g.last_played_at,
            }));
        }
        recent = recent_sessions(&st, p.id, is_self).await?;
    }

    Ok(Json(json!({
        "handle": p.handle,
        "display_name": p.display_name,
        "member_since": created,
        "relation": relation,
        "friend_count": friend_count,
        "shares_activity": shares,
        "history_visible": can_see_history,
        "presence": presence,
        "games": games,
        "overall": { "total_min": total_min, "minutes_7d": min_7d },
        "recent": recent,
        "recent_days": RECENT_DAYS,
    })))
}

async fn recent_sessions(st: &AppState, user: Uuid, is_self: bool) -> ApiResult<Vec<Value>> {
    let rows: Vec<(String, DateTime<Utc>, i64)> = sqlx::query_as(
        "SELECT game_id, ended_at, duration_s::bigint FROM sessions
         WHERE user_id = $1 AND ended_at >= $2 AND duration_s >= 60
         ORDER BY ended_at DESC LIMIT $3",
    )
    .bind(user)
    .bind(Utc::now() - Duration::days(RECENT_DAYS))
    .bind(RECENT_LIMIT)
    .fetch_all(&st.db)
    .await?;
    let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
    let named = names::public_names(st, &ids).await?;
    Ok(rows
        .into_iter()
        .map(|(gid, ended, secs)| {
            let (id, name) = match named.get(&gid) {
                Some(n) => (gid, n.clone()),
                None if is_self => (gid, names::UNLISTED_TITLE.to_string()),
                None => (
                    names::UNLISTED_ID.to_string(),
                    names::UNLISTED_NAME.to_string(),
                ),
            };
            json!({ "game_id": id, "game_name": name, "ended_at": ended, "duration_s": secs })
        })
        .collect())
}
