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
        .route("/api/v1/users/{handle}/report", post(report_profile))
}

#[derive(Deserialize)]
struct DisplayNameReq {
    display_name: String,
}

/// Words a nickname may not use (staff titles and the service's own name), unless you're staff.
const RESERVED: [&str; 12] = [
    "admin",
    "administrator",
    "moderator",
    "mod",
    "staff",
    "fraglink",
    "official",
    "support",
    "owner",
    "system",
    "developer",
    "dev",
];

/// Nickname changes allowed per 24 hours.
pub const NICK_CHANGES_PER_DAY: i64 = 3;

/// Whether a nickname looks like a staff title or the service ("Fraglink Support", "[MOD] x").
pub fn looks_official(name: &str) -> bool {
    let lower = name.to_lowercase();
    let compact: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
    if compact.contains("fraglink") || compact.contains("admin") || compact.contains("moderator") {
        return true;
    }
    lower
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| RESERVED.contains(&w))
}

/// Clean a requested nickname: no control characters, trimmed, runs of spaces collapsed.
pub fn clean_nickname(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Change your nickname (the name people see; 1-32 characters; empty = your username).
/// At most 3 changes a day; it can't be someone else's username or nickname, or look
/// like staff. The previous nickname goes to the history ("Previously known as").
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
    let name = clean_nickname(&r.display_name);
    let name = if name.is_empty() {
        a.user.handle.clone()
    } else {
        name
    };
    if name.chars().count() > 32 {
        return Err(AppError::bad_request(
            "invalid_display_name",
            "Nicknames are up to 32 characters.",
        ));
    }
    let (current, role): (String, Option<String>) =
        sqlx::query_as("SELECT display_name, role FROM users WHERE id = $1")
            .bind(a.user.id)
            .fetch_one(&st.db)
            .await?;
    if current == name {
        return Ok(Json(
            json!({ "display_name": name, "changes_left": Value::Null }),
        ));
    }
    let own_handle = name.eq_ignore_ascii_case(&a.user.handle);
    if !own_handle && role.is_none() && looks_official(&name) {
        return Err(AppError::bad_request(
            "reserved_name",
            "Nicknames can't look like a staff title or Fraglink itself.",
        ));
    }
    if !own_handle {
        let taken: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM users WHERE id <> $1 AND deleted_at IS NULL
                             AND (lower(handle) = lower($2) OR lower(display_name) = lower($2)))
                 OR EXISTS(SELECT 1 FROM handle_redirects WHERE user_id <> $1 AND old_handle = lower($2))",
        )
        .bind(a.user.id)
        .bind(&name)
        .fetch_one(&st.db)
        .await?;
        if taken {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "name_taken",
                "Someone already uses that name. Pick another nickname.",
            ));
        }
    }
    let recent: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM name_history WHERE user_id = $1 AND changed_at > now() - interval '24 hours'",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    if recent >= NICK_CHANGES_PER_DAY {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_name_changes",
            "You can change your nickname 3 times a day. Try again tomorrow.",
        ));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("INSERT INTO name_history(user_id, name) VALUES($1, $2)")
        .bind(a.user.id)
        .bind(&current)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET display_name = $2 WHERE id = $1")
        .bind(a.user.id)
        .bind(&name)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    crate::sig::forget(&a.user.handle);
    Ok(Json(json!({
        "display_name": name,
        "changes_left": (NICK_CHANGES_PER_DAY - recent - 1).max(0),
    })))
}

/// Up to 5 earlier nicknames, newest first, without the current one or repeats.
pub async fn previous_names(st: &AppState, user: Uuid, current: &str) -> ApiResult<Vec<String>> {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM name_history WHERE user_id = $1 ORDER BY changed_at DESC, id DESC LIMIT 50",
    )
    .bind(user)
    .fetch_all(&st.db)
    .await?;
    let mut out: Vec<String> = Vec::new();
    for n in rows {
        if n != current && !out.iter().any(|o| o.eq_ignore_ascii_case(&n)) {
            out.push(n);
        }
        if out.len() == 5 {
            break;
        }
    }
    Ok(out)
}

async fn profile(
    State(st): State<AppState>,
    m: MaybeUser,
    headers: HeaderMap,
    Path(handle): Path<String>,
    axum::extract::Query(pq): axum::extract::Query<crate::paging::PageQuery>,
) -> ApiResult<Json<Value>> {
    let viewer = m.0.as_ref().map(|a| a.user.id);
    let games_only = pq.show.as_deref() == Some("games");
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
    let p = friends::find_visible(&st, viewer, &handle).await?;
    let is_self = viewer == Some(p.id);
    // Admin > Site settings > Public profiles off: only yourself and friends
    if !is_self && !crate::features::on(&st, "public_profiles").await {
        let friend = match viewer {
            Some(v) => friends::are_friends(&st, v, p.id).await?,
            None => false,
        };
        if !friend {
            return Err(AppError::new(
                StatusCode::FORBIDDEN,
                "feature_off",
                "Profiles are only visible to friends right now.",
            ));
        }
    }
    // test accounts never show in friend counts, grids or events on profiles
    let test_handles = friends::test_handles(&st).await?;
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
        Value,
    );
    let (created, shares, public, show_friends, akind, ahash, apreset, about, location, style, rig, delete_after, privacy): Extra =
        sqlx::query_as(
            "SELECT created_at, share_activity, public_profile, show_friends, avatar_kind, avatar_hash, avatar_preset,
                    about, location, gaming_style, rig, delete_after, privacy FROM users WHERE id = $1",
        )
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
    if delete_after.is_some() && !is_self {
        return Err(not_found());
    }
    let friend_count = if test_handles.is_empty() {
        friends::friend_ids(&st, p.id).await?.len()
    } else {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM friendships f
             JOIN users u ON u.id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
             WHERE (f.user_a = $1 OR f.user_b = $1) AND f.status = 'accepted' AND NOT u.is_test",
        )
        .bind(p.id)
        .fetch_one(&st.db)
        .await? as usize
    };
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
    let avatar = crate::avatars::avatar_json(p.id, akind.as_deref(), ahash.as_deref(), apreset);
    // Private profile: only friends (and the owner) see more than name and picture.
    let full = is_self || relation == "friend" || public;
    if !full {
        // Friend actions on the private card: anyone but test accounts (blocked pairs never
        // get here) while friend requests are switched on. Signed out it's a sign-in prompt.
        let is_test: bool = sqlx::query_scalar("SELECT is_test FROM users WHERE id = $1")
            .bind(p.id)
            .fetch_one(&st.db)
            .await?;
        let can_add = !is_test
            && matches!(relation, "none" | "public")
            && crate::features::on(&st, "friend_requests").await;
        return Ok(Json(json!({
            "handle": p.handle,
            "display_name": p.display_name,
            "relation": relation,
            "can_add": can_add,
            "private": true,
            "avatar": avatar,
            "public_profile": false,
        })));
    }
    // Per-feature switches apply to everyone but the owner.
    let show = |k: &str| is_self || crate::account::shows(&privacy, k);
    let can_see_presence = true;
    let show_game = show("now_playing");
    let can_see_games = show("games");
    let can_see_history = is_self || shares;
    let previous = if show("name_history") {
        previous_names(&st, p.id, &p.display_name).await?
    } else {
        vec![]
    };
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
        if !show_game || v.game_id.as_ref().is_some_and(|g| hidden.contains(g)) {
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
    if can_see_games {
        let rows = api::game_stats(&st, p.id).await?;
        let ids: Vec<String> = rows.iter().map(|r| r.game_id.clone()).collect();
        let named = names::public_names(&st, &ids).await?;
        let apps = crate::home::appids(&st, &ids).await?;
        let own = presence::own_names_for(&st, p.id, &ids).await?;
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
            if g.display_min == 0 && g.minutes_7d == 0 {
                continue;
            }
            games.push(json!({
                "game_id": if listed || is_self || own.contains_key(&g.game_id) { g.game_id.clone() } else { names::UNLISTED_ID.to_string() },
                "name": own.get(&g.game_id).cloned().unwrap_or(g.name),
                "listed": listed,
                "minutes_7d": g.minutes_7d,
                "total_min": g.display_min,
                "tier": g.display_tier,
                "last_played_at": g.last_played_at,
                "steam_appid": if listed { apps.get(&g.game_id).copied() } else { None },
                "hidden": mine_hidden.contains(&g.game_id),
            }));
        }
    }
    // One page of the games table (search/sort on the server). `show=games` returns only that.
    let games_all = games.len();
    let games_page = {
        let mut g = games.clone();
        if pq.sort.is_some() {
            crate::paging::sort_games(&mut g, pq.sort.as_deref(), pq.dir.as_deref());
        }
        let default = if games_only { 50 } else { 8 };
        crate::paging::page(g, &pq.term(), &["name"], pq.offset(), pq.limit(default))
    };
    if games_only {
        return Ok(Json(json!({
            "games": games_page.0, "total": games_page.1, "all": games_all, "offset": pq.offset(),
            "visible": can_see_games,
        })));
    }
    // Extra details (each behind its own switch) and age from the birthday.
    let (real_name, gender, birth, country): (
        Option<String>,
        Option<String>,
        Option<chrono::NaiveDate>,
        Option<String>,
    ) = sqlx::query_as("SELECT real_name, gender, birth_date, country FROM users WHERE id = $1")
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
    let age = birth.map(|b| {
        let t = Utc::now().date_naive();
        use chrono::Datelike;
        t.year() - b.year() - i32::from((t.month(), t.day()) < (b.month(), b.day()))
    });
    // Mini activity events for the last 3 days: new friends and first plays.
    let mut events: Vec<Value> = Vec::new();
    if is_self || show_friends {
        let rows: Vec<(String, String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT u.handle, u.display_name, f.accepted_at FROM friendships f
             JOIN users u ON u.id = CASE WHEN f.user_a = $1 THEN f.user_b ELSE f.user_a END
             WHERE (f.user_a = $1 OR f.user_b = $1) AND f.status = 'accepted'
               AND f.accepted_at > now() - interval '3 days' AND u.delete_after IS NULL AND u.banned_at IS NULL
               AND NOT u.is_test
             ORDER BY f.accepted_at DESC LIMIT 10",
        )
        .bind(p.id)
        .fetch_all(&st.db)
        .await?;
        events.extend(rows.into_iter().map(
            |(h, d, at)| json!({ "kind": "friend", "at": at, "handle": h, "display_name": d }),
        ));
    }
    if can_see_history && can_see_games {
        let rows: Vec<(String, DateTime<Utc>)> = sqlx::query_as(
            "SELECT game_id, min(started_at) FROM sessions WHERE user_id = $1 AND NOT (game_id = ANY($2))
             GROUP BY game_id HAVING min(started_at) > now() - interval '3 days' ORDER BY 2 DESC LIMIT 10",
        )
        .bind(p.id)
        .bind(&hidden)
        .fetch_all(&st.db)
        .await?;
        events.extend(
            rows.into_iter()
                // only games the viewer can see by name (never leak an unlisted id)
                .filter(|(g, _)| {
                    games
                        .iter()
                        .any(|x| x["game_id"] == g.as_str() && x["listed"] == true)
                })
                .map(|(g, at)| {
                    let name = games
                        .iter()
                        .find(|x| x["game_id"] == g.as_str())
                        .map(|x| x["name"].clone());
                    json!({ "kind": "first_play", "at": at, "game_id": g, "name": name })
                }),
        );
    }
    let achievements = if can_see_games && show("achievements") {
        crate::achievements::for_user(&st, p.id, &hidden).await?
    } else {
        Value::Null
    };
    // WoW characters from the addon: only Battle.net-verified ones are shown and counted
    let wow_characters = if can_see_games
        && show("wow_characters")
        && crate::features::on(&st, "wow_characters").await
    {
        let mut c = crate::wow::characters(&st, p.id, is_self, true).await?;
        c.retain(|x| {
            !x["game_id"]
                .as_str()
                .is_some_and(|g| hidden.iter().any(|h| h == g))
        });
        json!(c)
    } else {
        Value::Null
    };
    if can_see_history {
        recent = recent_sessions(&st, p.id, is_self).await?;
        recent.retain(|r| {
            !r["game_id"]
                .as_str()
                .is_some_and(|g| hidden.iter().any(|h| h == g))
        });
    }

    // Friends grid: the user's own, friends' and public profiles (unless switched off).
    let mut friend_grid = Value::Null;
    if is_self || show_friends {
        let mut fl = friends::list(&st, p.id).await?;
        fl.friends.retain(|f| !test_handles.contains(&f.handle));
        // Friends who hide their game (or are private to this viewer) show no game here.
        let fids: Vec<String> = fl.friends.iter().map(|f| f.handle.clone()).collect();
        let quiet: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT u.handle FROM users u WHERE u.handle = ANY($1) AND NOT ({} AND (u.public_profile OR u.id = ANY($2)))",
            crate::account::sql_shows("now_playing")
        )))
        .bind(&fids)
        .bind(match viewer {
            Some(v) => friends::friend_ids(&st, v).await?.into_iter().chain([v]).collect::<Vec<_>>(),
            None => vec![],
        })
        .fetch_all(&st.db)
        .await?;
        let mut list: Vec<Value> = fl
            .friends
            .iter()
            .map(|f| json!({ "handle": f.handle, "display_name": f.display_name, "avatar": f.avatar,
                "status": f.presence.status, "game_name": if !quiet.contains(&f.handle) && f.presence.game_id.as_deref().is_some_and(|g| g != names::UNLISTED_ID) { f.presence.game_name.clone() } else { None } }))
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

    // Never expose a raw SteamID as a name: until the persona name is known, display is null
    // (and a lookup runs in the background).
    let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
        "SELECT provider, display, provider_uid FROM linked_accounts
         WHERE user_id = $1 AND verified AND show_on_profile ORDER BY array_position(ARRAY['steam','bnet','discord'], provider)",
    )
    .bind(p.id)
    .fetch_all(&st.db)
    .await?;
    // "Show connected account names" off: other viewers get the provider and the verified
    // tick only (no name, no link that would identify the account).
    let names = show("connection_names");
    let linked: Value = rows
        .into_iter()
        .map(|(prov, display, puid)| {
            if !names {
                return json!({ "provider": prov, "display": null, "verified": true });
            }
            let raw = display.as_deref().is_none_or(|d| d == puid);
            if prov == "steam" {
                if raw {
                    if let Ok(sid) = puid.parse::<u64>() {
                        let (st2, uid) = (st.clone(), p.id);
                        tokio::spawn(
                            async move { crate::steam::refresh_persona(&st2, uid, sid).await },
                        );
                    }
                }
                json!({ "provider": prov, "display": if raw { None } else { display }, "verified": true,
                        "url": format!("https://steamcommunity.com/profiles/{puid}") })
            } else {
                json!({ "provider": prov, "display": display, "verified": true })
            }
        })
        .collect();
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
        "games_visible": can_see_games,
        "previous_names": previous,
        "privacy": if is_self { crate::account::privacy_view(&privacy) } else { Value::Null },
        "presence": presence,
        "games": games_page.0,
        "games_total": games_all,
        "overall": { "total_min": total_min, "minutes_7d": min_7d },
        "recent": recent,
        "recent_days": RECENT_DAYS,
        "avatar": avatar,
        "about": if show("about") { about } else { None },
        "location": if show("location") { location } else { None },
        "gaming_style": if show("gaming_style") { style } else { None },
        "real_name": if show("real_name") { real_name } else { None },
        "gender": if show("gender") { gender } else { None },
        "age": if show("age") { age } else { None },
        "country": if show("location") { country } else { None },
        "events": events,
        "achievements": achievements,
        "wow_characters": wow_characters,
        "rig": if has_rig && show("rig") { rig } else { Value::Null },
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
    let own = presence::own_names_for(st, user, &ids).await?;
    Ok(rows
        .into_iter()
        .map(|(gid, ended, secs)| {
            let (id, name) = match named.get(&gid) {
                Some(n) => (gid, n.clone()),
                None if own.contains_key(&gid) => {
                    let n = own[&gid].clone();
                    (gid, n)
                }
                None if is_self => {
                    let n = own.get(&gid).cloned();
                    (gid, n.unwrap_or_else(|| names::UNLISTED_TITLE.to_string()))
                }
                None => (
                    names::UNLISTED_ID.to_string(),
                    names::UNLISTED_NAME.to_string(),
                ),
            };
            json!({ "game_id": id, "game_name": name, "ended_at": ended, "duration_s": secs })
        })
        .collect())
}

#[derive(Deserialize)]
struct ReportReq {
    reason: Option<String>,
}

/// Report a profile to the moderators (one open report per reporter and profile).
async fn report_profile(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(handle): Path<String>,
    Json(r): Json<ReportReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(
        format!("profile-report:{}", a.user.id),
        10,
        std::time::Duration::from_secs(3600),
    )?;
    let p = friends::find_visible(&st, Some(a.user.id), &handle).await?;
    if p.id == a.user.id {
        return Err(AppError::bad_request(
            "own_profile",
            "You can't report yourself.",
        ));
    }
    let reason: Option<String> = r
        .reason
        .map(|s| {
            s.chars()
                .filter(|c| !c.is_control())
                .take(300)
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty());
    sqlx::query(
        "INSERT INTO profile_reports(target, reporter, reason) VALUES($1, $2, $3)
         ON CONFLICT (target, reporter) WHERE resolved_at IS NULL DO UPDATE SET reason = excluded.reason",
    )
    .bind(p.id)
    .bind(a.user.id)
    .bind(reason)
    .execute(&st.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}
