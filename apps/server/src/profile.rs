//! Profile page data for the website (`/u/<handle>`): one read-only call.
//!
//! `GET /api/v1/users/{handle}/profile` (signed in). Everyone signed in sees the name,
//! member-since date and friend count. Live presence is shown to friends only (same
//! rules as the friends list: invisible = offline, hidden games stay hidden). The
//! games table and recent sessions are shown to the user themselves, and to friends
//! when the user shares activity. Games the server can't name never expose their id.

use crate::auth::{AuthUser, MaybeUser};
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
pub const RECENT_LIMIT: i64 = 80;

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
    m: MaybeUser,
    headers: HeaderMap,
    Path(handle): Path<String>,
) -> ApiResult<Json<Value>> {
    let viewer = m.0.as_ref().map(|a| a.user.id);
    let key = viewer
        .map(|v| v.to_string())
        .unwrap_or_else(|| crate::client_ip(&headers));
    st.rate_limit(
        format!("profile:{key}"),
        240,
        std::time::Duration::from_secs(600),
    )?;
    let not_found = || {
        AppError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "No Fraglink user with that name.",
        )
    };
    let p = friends::find_by_handle(&st, &handle).await?;
    let is_self = viewer == Some(p.id);
    if let (Some(me), false) = (viewer, is_self) {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blocks WHERE (blocker = $1 AND blocked = $2) OR (blocker = $2 AND blocked = $1))",
        )
        .bind(me)
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
        if blocked {
            return Err(not_found());
        }
    }
    type Extra = (
        DateTime<Utc>,
        bool,
        bool,
        bool,
        Option<String>,
        Option<String>,
        Option<i16>,
        Option<String>,
        Option<String>,
        Option<String>,
        Value,
        Option<DateTime<Utc>>,
    );
    let (created, shares, public, show_friends, akind, ahash, apreset, about, location, style, rig, delete_after): Extra =
        sqlx::query_as(
            "SELECT created_at, share_activity, public_profile, show_friends, avatar_kind, avatar_hash, avatar_preset,
                    about, location, gaming_style, rig, delete_after FROM users WHERE id = $1",
        )
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
    if delete_after.is_some() && !is_self {
        return Err(not_found());
    }
    if viewer.is_none() && !public {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in to see this profile.",
        ));
    }
    let friend_count = friends::friend_ids(&st, p.id).await?.len();
    let relation = match viewer {
        None => "public",
        Some(_) if is_self => "self",
        Some(me) if friends::are_friends(&st, me, p.id).await? => "friend",
        Some(me) => {
            let mine = friends::list(&st, me).await?;
            if mine.incoming.iter().any(|r| r.handle == p.handle) {
                "incoming"
            } else if mine.outgoing.iter().any(|r| r.handle == p.handle) {
                "outgoing"
            } else {
                "none"
            }
        }
    };
    let can_see_presence = is_self || relation == "friend" || public;
    let can_see_history = is_self || ((relation == "friend" || public) && shares);
    let hidden = if is_self {
        vec![]
    } else {
        crate::account::hidden_games(&st, p.id).await?
    };

    let presence = if can_see_presence {
        let mut m = presence::visible_presence_many(&st, &[p.id]).await?;
        let mut v = m
            .remove(&p.id)
            .unwrap_or_else(presence::PresenceView::offline);
        if v.game_id.as_ref().is_some_and(|g| hidden.contains(g)) {
            v.game_id = None;
            v.game_name = None;
            v.game_detail = None;
            v.session_started_at = None;
        }
        Some(v)
    } else {
        None
    };

    let (mut games, mut recent) = (Vec::new(), Vec::new());
    let (mut total_min, mut min_7d) = (0i64, 0i64);
    if can_see_history {
        let rows = api::game_stats(&st, p.id).await?;
        let ids: Vec<String> = rows.iter().map(|r| r.game_id.clone()).collect();
        let named = names::public_names(&st, &ids).await?;
        let apps = crate::home::appids(&st, &ids).await?;
        let mine_hidden = if is_self {
            crate::account::hidden_games(&st, p.id).await?
        } else {
            vec![]
        };
        for g in rows {
            if hidden.contains(&g.game_id) {
                continue;
            }
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
                "steam_appid": if listed { apps.get(&g.game_id).copied() } else { None },
                "hidden": mine_hidden.contains(&g.game_id),
            }));
        }
        recent = recent_sessions(&st, p.id, is_self).await?;
        recent.retain(|r| {
            !r["game_id"]
                .as_str()
                .is_some_and(|g| hidden.iter().any(|h| h == g))
        });
    }

    // Friends grid: the user's own, friends' and public profiles (unless switched off).
    let mut friend_grid = Value::Null;
    if is_self || (show_friends && (relation == "friend" || public)) {
        let fl = friends::list(&st, p.id).await?;
        let mut list: Vec<Value> = fl
            .friends
            .iter()
            .map(|f| json!({ "handle": f.handle, "display_name": f.display_name, "avatar": f.avatar,
                "status": f.presence.status, "game_name": if f.presence.game_id.as_deref().is_some_and(|g| g != names::UNLISTED_ID) { f.presence.game_name.clone() } else { None } }))
            .collect();
        list.sort_by_key(|f| match (f["status"].as_str(), f["game_name"].is_null()) {
            (Some("online"), false) => 0,
            (Some("online"), true) => 1,
            (Some("away"), _) => 2,
            _ => 3,
        });
        list.truncate(12);
        friend_grid = json!(list);
    }

    let linked: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('provider', provider, 'display', display)), '[]'::jsonb)
         FROM linked_accounts WHERE user_id = $1 AND verified AND show_on_profile",
    )
    .bind(p.id)
    .fetch_one(&st.db)
    .await?;
    let has_rig = rig.as_object().is_some_and(|o| !o.is_empty());

    Ok(Json(json!({
        "handle": p.handle,
        "display_name": p.display_name,
        "member_since": created,
        "relation": relation,
        "friend_count": friend_count,
        "shares_activity": shares,
        "public_profile": public,
        "history_visible": can_see_history,
        "presence": presence,
        "games": games,
        "overall": { "total_min": total_min, "minutes_7d": min_7d },
        "recent": recent,
        "recent_days": RECENT_DAYS,
        "avatar": crate::avatars::avatar_json(p.id, akind.as_deref(), ahash.as_deref(), apreset),
        "about": about,
        "location": location,
        "gaming_style": style,
        "rig": if has_rig { rig } else { Value::Null },
        "friends": friend_grid,
        "linked": linked,
        "delete_after": if is_self { json!(delete_after) } else { Value::Null },
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
