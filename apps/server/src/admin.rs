//! Staff tools behind `/admin` on the website (JSON API under `/api/v1/admin/`).
//!
//! Roles (`users.role`): **owner** (exactly one; can't be removed, demoted or banned;
//! the only one who adds/removes admins or hands ownership over), **admin** (everything
//! else) and **moderator** (game approvals, user search, ban/unban, hours review).
//! Every endpoint checks the caller's role here on the server; anyone without a role
//! (or signed out, or using the browser cookie instead of a bearer token) gets 404.
//! Every change is written to `admin_audit` (who, what, when, before/after).
//!
//! `GET /api/v1/announcement` (public) is the site/app banner.

use crate::auth::{self, AuthUser, User};
use crate::error::{ApiResult, AppError};
use crate::{api, game_overrides, names, AppState};
use axum::body::Bytes;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/announcement", get(announcement_public))
        .route("/api/v1/admin/me", get(me))
        .route("/api/v1/admin/suggestions", get(suggestions))
        .route("/api/v1/admin/suggestions/approve", post(approve))
        .route("/api/v1/admin/suggestions/reject", post(reject))
        .route("/api/v1/admin/suggestions/undo", post(undo_suggestion))
        .route("/api/v1/admin/games", get(games_search))
        .route("/api/v1/admin/games/{id}", get(game_detail).post(game_save))
        .route(
            "/api/v1/admin/games/{id}/art",
            post(game_art_upload).delete(game_art_clear),
        )
        .route("/api/v1/admin/games/{id}/art/url", post(game_art_url))
        .route("/api/v1/admin/games/{id}/retire", post(game_retire))
        .route("/api/v1/admin/games/{id}/unretire", post(game_unretire))
        .route("/api/v1/admin/games/{id}/status", post(game_status_save))
        .route(
            "/api/v1/admin/games/{id}/icon",
            post(game_icon_upload).delete(game_icon_delete),
        )
        .route("/api/v1/admin/users", get(users_search))
        .route("/api/v1/admin/users/{id}", get(user_detail))
        .route(
            "/api/v1/admin/users/{id}/reset_password",
            post(reset_password),
        )
        .route("/api/v1/admin/users/{id}/ban", post(ban))
        .route("/api/v1/admin/users/{id}/unban", post(unban))
        .route("/api/v1/admin/users/{id}/hours", post(adjust_hours))
        .route(
            "/api/v1/admin/users/{id}/sessions/{sid}",
            delete(delete_session),
        )
        .route("/api/v1/admin/hours/flags", get(hours_flags))
        .route("/api/v1/admin/server", get(server))
        .route(
            "/api/v1/admin/announcements",
            get(announcements).post(announce),
        )
        .route("/api/v1/admin/announcements/clear", post(announce_clear))
        .route("/api/v1/admin/staff", get(staff_list).post(staff_set))
        .route("/api/v1/admin/staff/transfer", post(staff_transfer))
        .route("/api/v1/admin/audit", get(audit_list))
        .route("/api/v1/admin/avatar_reports", get(avatar_reports))
        .route(
            "/api/v1/admin/avatar_reports/{id}/dismiss",
            post(avatar_dismiss),
        )
        .route(
            "/api/v1/admin/users/{id}/avatar/remove",
            post(avatar_remove),
        )
        .route("/api/v1/admin/users/{id}/mfa/reset", post(mfa_reset))
}

static STARTED: OnceLock<std::time::Instant> = OnceLock::new();

/// Remember when the server started (for the Server tab's uptime).
pub fn mark_started() {
    STARTED.get_or_init(std::time::Instant::now);
}

// ---------------------------------------------------------------------------
// roles

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Moderator = 1,
    Admin = 2,
    Owner = 3,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "owner" => Some(Role::Owner),
            "admin" => Some(Role::Admin),
            "moderator" => Some(Role::Moderator),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Admin => "admin",
            Role::Moderator => "moderator",
        }
    }
}

/// Rank of an optional role (0 = not staff).
fn rank(r: Option<Role>) -> u8 {
    r.map(|r| r as u8).unwrap_or(0)
}

/// A signed-in staff member (bearer token only).
pub struct Staff {
    pub user: User,
    pub role: Role,
}

impl FromRequestParts<AppState> for Staff {
    type Rejection = AppError;
    async fn from_request_parts(parts: &mut Parts, st: &AppState) -> Result<Self, AppError> {
        // Signed out, cookie sessions and non-staff all look like "no such page".
        let a = AuthUser::from_request_parts(parts, st)
            .await
            .map_err(|_| AppError::not_found())?;
        if a.via_cookie {
            return Err(AppError::not_found());
        }
        let role: Option<String> = sqlx::query_scalar("SELECT role FROM users WHERE id = $1")
            .bind(a.user.id)
            .fetch_one(&st.db)
            .await?;
        let role = role
            .as_deref()
            .and_then(Role::parse)
            .ok_or_else(AppError::not_found)?;
        Ok(Staff { user: a.user, role })
    }
}

fn forbidden() -> AppError {
    AppError::new(
        StatusCode::FORBIDDEN,
        "forbidden",
        "Your staff role can't do that.",
    )
}

impl Staff {
    fn need(&self, min: Role) -> ApiResult<()> {
        if self.role >= min {
            Ok(())
        } else {
            Err(forbidden())
        }
    }
}

/// What each role may do (the website shows/hides tabs with this; every endpoint
/// checks again).
fn permissions(r: Role) -> Value {
    let admin = r >= Role::Admin;
    json!({
        "approvals": true,
        "users": true,
        "ban": true,
        "hours": true,
        "reset_password": admin,
        "games": admin,
        "server": admin,
        "announcements": admin,
        "staff": admin,
        "manage_admins": r == Role::Owner,
        "audit": admin,
        "avatars": true,
        "mfa_reset": admin,
    })
}

async fn audit(
    st: &AppState,
    s: &Staff,
    action: &str,
    target: &str,
    before: Value,
    after: Value,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO admin_audit(actor_id, actor_handle, action, target, before, after)
         VALUES($1, $2, $3, $4, $5, $6)",
    )
    .bind(s.user.id)
    .bind(&s.user.handle)
    .bind(action)
    .bind(target)
    .bind(if before.is_null() { None } else { Some(before) })
    .bind(if after.is_null() { None } else { Some(after) })
    .execute(&st.db)
    .await?;
    tracing::info!(actor = %s.user.handle, action, target, "admin action");
    Ok(())
}

fn bad(code: &'static str, msg: &str) -> AppError {
    AppError::bad_request(code, msg)
}

fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

fn like_pattern(q: &str) -> String {
    format!(
        "%{}%",
        q.to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    )
}

async fn me(s: Staff) -> Json<Value> {
    Json(json!({
        "handle": s.user.handle,
        "display_name": s.user.display_name,
        "role": s.role.as_str(),
        "can": permissions(s.role),
    }))
}

// ---------------------------------------------------------------------------
// 1. game approvals

#[derive(Deserialize)]
struct StatusQ {
    #[serde(default)]
    status: Option<String>,
}

async fn suggestions(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<StatusQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let decided = q.status.as_deref() == Some("decided");
    let rows: Vec<(String, Value, i64, DateTime<Utc>, DateTime<Utc>, String, Option<String>, i64)> =
        sqlx::query_as(
            "WITH g AS (
               SELECT lower(exe) AS exe,
                      jsonb_agg(jsonb_build_object('name', name, 'reports', reports) ORDER BY reports DESC, last_at DESC) AS names,
                      sum(reports)::bigint AS reports, min(first_at) AS first_at, max(last_at) AS last_at,
                      CASE WHEN bool_or(status = 'new') THEN 'new'
                           WHEN bool_or(status = 'accepted') THEN 'accepted' ELSE 'rejected' END AS status
               FROM game_suggestions GROUP BY lower(exe)
             )
             SELECT g.exe, g.names, g.reports, g.first_at, g.last_at, g.status,
                    (SELECT name FROM games WHERE id = 'exe:' || g.exe AND verified) AS approved_name,
                    (SELECT count(DISTINCT user_id) FROM sessions WHERE game_id = 'exe:' || g.exe) AS users
             FROM g
             WHERE ($1 AND g.status <> 'new') OR (NOT $1 AND g.status = 'new')
             ORDER BY CASE WHEN $1 THEN g.last_at END DESC, g.reports DESC, g.first_at
             LIMIT 300",
        )
        .bind(decided)
        .fetch_all(&st.db)
        .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(|(exe, names, reports, first, last, status, approved, users)| {
            json!({ "exe": exe, "names": names, "reports": reports, "first_seen": first,
                    "last_seen": last, "status": status, "approved_name": approved, "users": users })
        })
        .collect();
    Ok(Json(json!({ "suggestions": items })))
}

#[derive(Deserialize)]
struct ApproveReq {
    exe: String,
    #[serde(default)]
    name: String,
}

async fn game_row(st: &AppState, id: &str) -> ApiResult<Value> {
    let r: Option<(String, bool)> =
        sqlx::query_as("SELECT name, verified FROM games WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    Ok(r.map(|(n, v)| json!({ "name": n, "verified": v }))
        .unwrap_or(Value::Null))
}

async fn approve(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<ApproveReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let exe = r.exe.trim().to_lowercase();
    let id = format!("exe:{exe}");
    let before = game_row(&st, &id).await?;
    let id = names::approve(&st.db, &exe, &r.name)
        .await
        .map_err(|e| bad("invalid", &e.to_string()))?;
    audit(
        &st,
        &s,
        "game.approve",
        &id,
        before,
        json!({ "name": r.name.trim(), "verified": true }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

async fn reject(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<ApproveReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let exe = r.exe.trim().to_lowercase();
    if !crate::games::valid_exe(&exe) {
        return Err(bad("invalid_exe", "Not a program name like Game.exe."));
    }
    let n = sqlx::query(
        "UPDATE game_suggestions SET status = 'rejected' WHERE lower(exe) = $1 AND status = 'new'",
    )
    .bind(&exe)
    .execute(&st.db)
    .await?
    .rows_affected();
    audit(
        &st,
        &s,
        "game.reject",
        &format!("exe:{exe}"),
        json!({ "status": "new" }),
        json!({ "status": "rejected", "rows": n }),
    )
    .await?;
    Ok(Json(json!({ "updated": n })))
}

/// Put a decided suggestion back in the queue (and take an approval back).
async fn undo_suggestion(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<ApproveReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let exe = r.exe.trim().to_lowercase();
    if !crate::games::valid_exe(&exe) {
        return Err(bad("invalid_exe", "Not a program name like Game.exe."));
    }
    let id = format!("exe:{exe}");
    let before = game_row(&st, &id).await?;
    names::unapprove(&st.db, &exe).await?;
    sqlx::query("UPDATE game_suggestions SET status = 'new' WHERE lower(exe) = $1")
        .bind(&exe)
        .execute(&st.db)
        .await?;
    audit(
        &st,
        &s,
        "game.undo_decision",
        &id,
        before,
        json!({ "status": "new", "verified": false }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 2. games

#[derive(Deserialize)]
struct SearchQ {
    #[serde(default)]
    q: String,
}

fn valid_game_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && !id.chars().any(char::is_control)
}

/// Where a game's name comes from, for the Games tab.
async fn game_info(st: &AppState, id: &str) -> ApiResult<Option<Value>> {
    let list = names::list_name(st, id);
    let row: Option<(String, bool, Option<i32>)> =
        sqlx::query_as("SELECT name, verified, steam_appid FROM games WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    if list.is_none() && row.is_none() {
        return Ok(None);
    }
    let ov = game_overrides::current(st).await?;
    let o = ov.get(id).cloned();
    let source = if list.is_some() {
        "games list"
    } else if row.as_ref().is_some_and(|r| r.1) {
        if id.starts_with("exe:") {
            "approved"
        } else {
            "steam"
        }
    } else {
        "unlisted"
    };
    let base_name = list
        .clone()
        .or_else(|| row.as_ref().map(|r| r.0.clone()))
        .unwrap_or_default();
    let public = names::public_name(st, id).await?;
    let retired: Option<(String, DateTime<Utc>, Vec<String>, bool)> = sqlx::query_as(
        "SELECT successor_id, retired_at, exes, partial FROM game_retirements WHERE game_id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let retired_from: Option<String> =
        sqlx::query_scalar("SELECT game_id FROM game_retirements WHERE successor_id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    Ok(Some(json!({
        "retired": retired.map(|(s, at, exes, partial)| json!({ "successor": s, "at": at, "exes": exes, "partial": partial })),
        "exes": crate::retire::all_exes(st, id).await?,
        "status": crate::game_status::get(st, id).await?,
        "retired_from": retired_from,
        "id": id,
        "name": base_name,
        "public_name": public,
        "source": source,
        "override": o,
        "icon": crate::icons::icon_path(id, 32),
        "icon64": crate::icons::icon_path(id, 64),
    })))
}

async fn games_search(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<SearchQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let q = q.q.trim().to_lowercase();
    let ov = game_overrides::current(&st).await?;
    let mut ids: Vec<String> = Vec::new();
    if q.is_empty() {
        // edited games first, then the most played
        let mut e: Vec<String> = ov.keys().cloned().collect();
        e.extend(
            sqlx::query_scalar::<_, String>(
                "SELECT game_id FROM game_status UNION SELECT game_id FROM game_retirements",
            )
            .fetch_all(&st.db)
            .await?,
        );
        e.sort();
        e.dedup();
        ids.extend(e);
        let top: Vec<String> = sqlx::query_scalar(
            "SELECT game_id FROM sessions WHERE started_at > now() - interval '30 days'
             GROUP BY game_id ORDER BY count(DISTINCT user_id) DESC, sum(duration_s) DESC LIMIT 40",
        )
        .fetch_all(&st.db)
        .await?;
        ids.extend(top);
    } else {
        if let Some(b) = st.games.current() {
            let mut hits: Vec<(&String, &String)> = b
                .names
                .iter()
                .filter(|(id, n)| n.to_lowercase().contains(&q) || id.contains(&q))
                .collect();
            // exact and prefix matches first, then shorter names
            hits.sort_by_key(|(id, n)| {
                let n = n.to_lowercase();
                (n != q && **id != q, !n.starts_with(&q), n.len())
            });
            ids.extend(hits.into_iter().take(40).map(|(id, _)| id.clone()));
        }
        let db: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM games WHERE lower(name) LIKE $1 OR lower(id) LIKE $1
             ORDER BY verified DESC, length(name) LIMIT 40",
        )
        .bind(like_pattern(&q))
        .fetch_all(&st.db)
        .await?;
        ids.extend(db);
        for (id, o) in ov.iter() {
            if o.name
                .as_deref()
                .is_some_and(|n| n.to_lowercase().contains(&q))
                || o.extra_exes.iter().any(|e| e.contains(&q))
            {
                ids.push(id.clone());
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    ids.retain(|i| !i.starts_with("manual:") && seen.insert(i.clone()));
    ids.truncate(60);
    let counts: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT game_id, count(DISTINCT user_id), (sum(duration_s) / 60)::bigint FROM sessions
         WHERE game_id = ANY($1) GROUP BY game_id",
    )
    .bind(&ids)
    .fetch_all(&st.db)
    .await?;
    let counts: std::collections::HashMap<String, (i64, i64)> =
        counts.into_iter().map(|(g, u, m)| (g, (u, m))).collect();
    let mut out = Vec::new();
    for id in &ids {
        if let Some(mut v) = game_info(&st, id).await? {
            let (u, m) = counts.get(id).copied().unwrap_or((0, 0));
            v["players"] = json!(u);
            v["tracked_min"] = json!(m);
            out.push(v);
        }
    }
    Ok(Json(json!({ "games": out })))
}

async fn game_detail(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let mut v = game_info(&st, &id).await?.ok_or_else(AppError::not_found)?;
    let (u, m): (i64, i64) = sqlx::query_as(
        "SELECT count(DISTINCT user_id), coalesce(sum(duration_s) / 60, 0)::bigint FROM sessions WHERE game_id = $1",
    )
    .bind(&id)
    .fetch_one(&st.db)
    .await?;
    v["players"] = json!(u);
    v["tracked_min"] = json!(m);
    v["art_source"] = json!(crate::art::source_of(&st, &id).await);
    Ok(Json(v))
}

#[derive(Deserialize)]
struct GameSave {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    extra_exes: Vec<String>,
}

async fn game_save(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    Json(r): Json<GameSave>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !valid_game_id(&id) || game_info(&st, &id).await?.is_none() {
        return Err(AppError::not_found());
    }
    let name = r.name.map(|n| clean(&n, 200)).filter(|n| !n.is_empty());
    if name.as_ref().is_some_and(|n| n.chars().count() > 80) {
        return Err(bad("invalid_name", "Names are up to 80 characters."));
    }
    let mut exes: Vec<String> = Vec::new();
    for e in &r.extra_exes {
        let e = e.trim().to_lowercase();
        if e.is_empty() {
            continue;
        }
        if !crate::games::valid_exe(&e) {
            return Err(bad(
                "invalid_exe",
                &format!("{e:?} isn't a program name like Game.exe."),
            ));
        }
        if !exes.contains(&e) {
            exes.push(e);
        }
    }
    if exes.len() > 20 {
        return Err(bad("too_many", "Up to 20 extra exe names."));
    }
    let before = game_overrides::current(&st).await?.get(&id).cloned();
    sqlx::query(
        "INSERT INTO game_overrides(game_id, name, hidden, extra_exes, updated_by)
         VALUES($1, $2, $3, $4, $5)
         ON CONFLICT (game_id) DO UPDATE SET name = excluded.name, hidden = excluded.hidden,
           extra_exes = excluded.extra_exes, updated_at = now(), updated_by = excluded.updated_by",
    )
    .bind(&id)
    .bind(&name)
    .bind(r.hidden)
    .bind(&exes)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    // nothing left to keep: drop the row
    sqlx::query(
        "DELETE FROM game_overrides WHERE game_id = $1 AND name IS NULL AND NOT hidden
           AND extra_exes = '{}' AND NOT has_icon",
    )
    .bind(&id)
    .execute(&st.db)
    .await?;
    let m = game_overrides::reload(&st).await?;
    let after = m.get(&id).cloned();
    audit(&st, &s, "game.edit", &id, json!(before), json!(after)).await?;
    Ok(Json(game_info(&st, &id).await?.unwrap_or(Value::Null)))
}

/// Retire: freeze this entry (name, icon, hours stay) and record future play of its
/// program under a new `exe:` entry that goes to game approvals.
#[derive(Deserialize, Default)]
struct RetireBody {
    /// Programs to detach; empty / missing = all of them (the whole entry retires).
    #[serde(default)]
    exes: Vec<String>,
}

async fn game_retire(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    body: Option<Json<RetireBody>>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !valid_game_id(&id) || id.starts_with("manual:") || game_info(&st, &id).await?.is_none() {
        return Err(AppError::not_found());
    }
    let already: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM game_retirements WHERE game_id = $1)")
            .bind(&id)
            .fetch_one(&st.db)
            .await?;
    if already {
        return Err(bad("already_retired", "This game is already retired."));
    }
    let all = crate::retire::all_exes(&st, &id).await?;
    let mut pick: Vec<String> = body
        .map(|b| b.0.exes)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.trim().to_lowercase())
        .collect();
    pick.dedup();
    if let Some(e) = pick.iter().find(|e| !all.contains(e)) {
        return Err(bad(
            "unknown_exe",
            &format!("{e} isn't one of this game's programs."),
        ));
    }
    if !pick.is_empty() && pick.len() < all.len() {
        return retire_partly(st, s, id, pick).await;
    }
    let Some(exe) = crate::retire::successor_exe(&st, &id).await? else {
        return Err(bad(
            "no_exe",
            "Fraglink doesn't know a program name for this game, so there is nothing to detach. Add an extra exe name first.",
        ));
    };
    let successor = format!("exe:{exe}");
    if successor == id {
        // an approved program: its next entry needs an id of its own
        return Err(bad(
            "is_program",
            "This entry is a program's own entry; take the approval back instead.",
        ));
    }
    let used: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sessions WHERE game_id = $1)
             OR EXISTS(SELECT 1 FROM game_retirements WHERE successor_id = $1)",
    )
    .bind(&successor)
    .fetch_one(&st.db)
    .await?;
    if used {
        return Err(bad(
            "successor_taken",
            &format!("{exe} already has its own entry with hours; it can't start fresh."),
        ));
    }
    let name = names::public_name(&st, &id)
        .await?
        .unwrap_or_else(|| id.clone());
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "INSERT INTO game_retirements(game_id, successor_id, retired_by, exes) VALUES($1, $2, $3, $4)",
    )
    .bind(&id)
    .bind(&successor)
    .bind(s.user.id)
    .bind(&all)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO games(id, name, verified) VALUES($1, $2, false) ON CONFLICT (id) DO NOTHING",
    )
    .bind(&successor)
    .bind(names::UNLISTED_TITLE)
    .execute(&mut *tx)
    .await?;
    // shows up in Game approvals, suggested under the old name
    let sugg: String = name.chars().take(80).collect();
    sqlx::query(
        "INSERT INTO game_suggestions(exe, name, reports) VALUES($1, $2, 1)
         ON CONFLICT ((lower(exe)), (lower(name))) DO UPDATE SET status = 'new', last_at = now()",
    )
    .bind(&exe)
    .bind(&sugg)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    crate::retire::reload(&st).await?;
    audit(
        &st,
        &s,
        "game.retire",
        &id,
        json!({ "retired": false }),
        json!({ "retired": true, "successor": successor, "exe": exe, "name": name }),
    )
    .await?;
    Ok(Json(game_info(&st, &id).await?.unwrap_or(Value::Null)))
}

/// Detach only some programs: each gets a new `exe:<name>` entry (unapproved, in Game
/// approvals); the game itself stays live with its other programs.
async fn retire_partly(
    st: AppState,
    s: Staff,
    id: String,
    exes: Vec<String>,
) -> ApiResult<Json<Value>> {
    let ids: Vec<String> = exes.iter().map(|e| format!("exe:{e}")).collect();
    if ids.contains(&id) {
        return Err(bad(
            "is_program",
            "This entry is a program's own entry; take the approval back instead.",
        ));
    }
    let used: Option<String> = sqlx::query_scalar(
        "SELECT g FROM unnest($1::text[]) g WHERE EXISTS(SELECT 1 FROM sessions WHERE game_id = g)
             OR EXISTS(SELECT 1 FROM game_retirements WHERE successor_id = g) LIMIT 1",
    )
    .bind(&ids)
    .fetch_optional(&st.db)
    .await?;
    if let Some(u) = used {
        return Err(bad(
            "successor_taken",
            &format!(
                "{} already has its own entry with hours; it can't start fresh.",
                u.trim_start_matches("exe:")
            ),
        ));
    }
    let name = names::public_name(&st, &id)
        .await?
        .unwrap_or_else(|| id.clone());
    let sugg: String = name.chars().take(80).collect();
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "INSERT INTO game_retirements(game_id, successor_id, retired_by, exes, partial) VALUES($1, $2, $3, $4, true)",
    )
    .bind(&id)
    .bind(&ids[0])
    .bind(s.user.id)
    .bind(&exes)
    .execute(&mut *tx)
    .await?;
    for (exe, sid) in exes.iter().zip(&ids) {
        sqlx::query("INSERT INTO games(id, name, verified) VALUES($1, $2, false) ON CONFLICT (id) DO NOTHING")
            .bind(sid)
            .bind(names::UNLISTED_TITLE)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO game_suggestions(exe, name, reports) VALUES($1, $2, 1)
             ON CONFLICT ((lower(exe)), (lower(name))) DO UPDATE SET status = 'new', last_at = now()",
        )
        .bind(exe)
        .bind(&sugg)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    crate::retire::reload(&st).await?;
    audit(
        &st,
        &s,
        "game.retire",
        &id,
        json!({ "retired": false }),
        json!({ "retired": "partly", "exes": exes, "new_entries": ids, "name": name }),
    )
    .await?;
    Ok(Json(game_info(&st, &id).await?.unwrap_or(Value::Null)))
}

/// Undo a retirement, only while the new entry has no hours (nothing would be lost).
async fn game_unretire(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let row: Option<(String, Vec<String>, bool)> = sqlx::query_as(
        "SELECT successor_id, exes, partial FROM game_retirements WHERE game_id = $1",
    )
    .bind(&id)
    .fetch_optional(&st.db)
    .await?;
    let Some((successor, exes, partial)) = row else {
        return Err(bad("not_retired", "This game isn't retired."));
    };
    let mut new_ids = vec![successor.clone()];
    if partial {
        new_ids = exes.iter().map(|e| format!("exe:{e}")).collect();
    }
    let used: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions WHERE game_id = ANY($1))")
            .bind(&new_ids)
            .fetch_one(&st.db)
            .await?;
    if used {
        return Err(bad(
            "successor_has_hours",
            "The new entry already has hours, so this can't be undone safely.",
        ));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM game_retirements WHERE game_id = $1")
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    for sid in &new_ids {
        let exe = sid.trim_start_matches("exe:");
        sqlx::query("DELETE FROM game_suggestions WHERE lower(exe) = lower($1) AND status = 'new'")
            .bind(exe)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM games WHERE id = $1 AND NOT verified AND NOT EXISTS (SELECT 1 FROM sessions WHERE game_id = $1)")
            .bind(sid)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    crate::retire::reload(&st).await?;
    audit(
        &st,
        &s,
        "game.unretire",
        &id,
        json!({ "retired": true, "successor": successor }),
        json!({ "retired": false }),
    )
    .await?;
    Ok(Json(game_info(&st, &id).await?.unwrap_or(Value::Null)))
}

async fn game_icon_upload(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !valid_game_id(&id) || game_info(&st, &id).await?.is_none() {
        return Err(AppError::not_found());
    }
    if body.is_empty() || body.len() > 1024 * 1024 {
        return Err(bad("invalid_image", "Upload a PNG or JPEG up to 1 MB."));
    }
    let raw = body.to_vec();
    let pngs = tokio::task::spawn_blocking(move || {
        crate::icons::SIZES
            .iter()
            .map(|px| fraglink_core::icons::normalize_px(&raw, *px).map(|p| (*px, p)))
            .collect::<Option<Vec<_>>>()
    })
    .await
    .map_err(AppError::internal)?
    .ok_or_else(|| bad("invalid_image", "That file isn't a PNG or JPEG image."))?;
    let dir = game_overrides::custom_icon_dir(&st);
    std::fs::create_dir_all(&dir).map_err(AppError::internal)?;
    let k = crate::icons::key(&id);
    for (px, png) in &pngs {
        let tmp = dir.join(format!("{k}-{px}.tmp"));
        std::fs::write(&tmp, png).map_err(AppError::internal)?;
        std::fs::rename(&tmp, dir.join(format!("{k}-{px}.png"))).map_err(AppError::internal)?;
    }
    let before = game_overrides::current(&st)
        .await?
        .get(&id)
        .map(|o| o.has_icon);
    sqlx::query(
        "INSERT INTO game_overrides(game_id, has_icon, updated_by) VALUES($1, true, $2)
         ON CONFLICT (game_id) DO UPDATE SET has_icon = true, updated_at = now(), updated_by = excluded.updated_by",
    )
    .bind(&id)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    game_overrides::reload(&st).await?;
    audit(
        &st,
        &s,
        "game.icon_upload",
        &id,
        json!({ "custom_icon": before.unwrap_or(false) }),
        json!({ "custom_icon": true, "bytes": body.len() }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "sizes": crate::icons::SIZES })))
}

async fn game_icon_delete(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let dir = game_overrides::custom_icon_dir(&st);
    let k = crate::icons::key(&id);
    for px in crate::icons::SIZES {
        let _ = std::fs::remove_file(dir.join(format!("{k}-{px}.png")));
    }
    sqlx::query(
        "UPDATE game_overrides SET has_icon = false, updated_at = now() WHERE game_id = $1",
    )
    .bind(&id)
    .execute(&st.db)
    .await?;
    sqlx::query(
        "DELETE FROM game_overrides WHERE game_id = $1 AND name IS NULL AND NOT hidden
           AND extra_exes = '{}' AND NOT has_icon",
    )
    .bind(&id)
    .execute(&st.db)
    .await?;
    game_overrides::reload(&st).await?;
    audit(
        &st,
        &s,
        "game.icon_remove",
        &id,
        json!({ "custom_icon": true }),
        json!({ "custom_icon": false }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 3. users

async fn users_search(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<SearchQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let q = q.q.trim().to_lowercase();
    let rows: Vec<(
        Uuid,
        String,
        String,
        String,
        DateTime<Utc>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<String>,
        Option<DateTime<Utc>>,
    )> = sqlx::query_as(
        "SELECT u.id, u.handle, u.display_name, u.email, u.created_at, u.role, u.banned_at,
                    p.status, p.heartbeat_at
             FROM users u LEFT JOIN presence p ON p.user_id = u.id
             WHERE u.deleted_at IS NULL
               AND ($1 = '' OR lower(u.handle) LIKE $2 OR lower(u.display_name) LIKE $2
                    OR (u.email <> '' AND lower(u.email) LIKE $2))
             ORDER BY (lower(u.handle) = $1) DESC, u.created_at DESC
             LIMIT 50",
    )
    .bind(&q)
    .bind(like_pattern(&q))
    .fetch_all(&st.db)
    .await?;
    let users: Vec<Value> = rows
        .into_iter()
        .map(|(id, h, d, e, c, role, banned, status, hb)| {
            json!({ "id": id, "handle": h, "display_name": d, "email": e, "created_at": c,
                    "role": role, "banned": banned.is_some(), "status": status.unwrap_or_else(|| "offline".into()),
                    "last_seen": hb })
        })
        .collect();
    Ok(Json(json!({ "users": users })))
}

/// (handle, role) of a live account, or 404.
async fn target(st: &AppState, id: Uuid) -> ApiResult<(String, Option<Role>, bool)> {
    let r: Option<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT handle, role, banned_at IS NOT NULL FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (h, role, banned) = r.ok_or_else(AppError::not_found)?;
    Ok((h, role.as_deref().and_then(Role::parse), banned))
}

/// Staff may act on accounts ranked below them (never on the owner).
fn outranks(s: &Staff, target_id: Uuid, target_role: Option<Role>) -> ApiResult<()> {
    if s.user.id == target_id || rank(Some(s.role)) <= rank(target_role) {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "You can't do that to this account.",
        ));
    }
    Ok(())
}

async fn user_detail(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let r: Option<(String, String, String, DateTime<Utc>, Option<String>, Option<DateTime<Utc>>, Option<String>, bool, bool)> =
        sqlx::query_as(
            "SELECT handle, display_name, email, created_at, role, banned_at, ban_reason, share_activity, email_verified
             FROM users WHERE id = $1 AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&st.db)
        .await?;
    let (handle, display, email, created, role, banned_at, reason, share, verified) =
        r.ok_or_else(AppError::not_found)?;
    let presence: Option<(String, Option<String>, Option<String>, Option<String>, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT status, game_id, game_name, game_detail, heartbeat_at, game_hidden FROM presence WHERE user_id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let presence = match presence {
        Some((status, gid, _gname, detail, hb, hidden)) => {
            let name = match &gid {
                Some(g) => names::public_name(&st, g).await?,
                None => None,
            };
            json!({ "status": status, "game_id": gid, "game_name": name, "game_detail": detail,
                    "heartbeat_at": hb, "game_hidden": hidden, "connected": st.hub.is_online(id) })
        }
        None => json!({ "status": "offline", "connected": false }),
    };
    let devices: Vec<(Uuid, String, Option<String>, DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)> =
        sqlx::query_as(
            "SELECT id, kind, device_name, created_at, last_used_at, expires_at FROM auth_tokens
             WHERE user_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
             ORDER BY last_used_at DESC",
        )
        .bind(id)
        .fetch_all(&st.db)
        .await?;
    let devices: Vec<Value> = devices
        .into_iter()
        .map(|(i, k, n, c, l, e)| json!({ "id": i, "kind": k, "name": n, "created_at": c, "last_used_at": l, "expires_at": e }))
        .collect();
    let games = api::game_stats(&st, id).await?;
    // admin view: show the stored name for games the public can't see
    let raw: Vec<(String, String)> =
        sqlx::query_as("SELECT g.id, g.name FROM games g WHERE g.id = ANY($1)")
            .bind(games.iter().map(|g| g.game_id.clone()).collect::<Vec<_>>())
            .fetch_all(&st.db)
            .await?;
    let raw: std::collections::HashMap<String, String> = raw.into_iter().collect();
    let games: Vec<Value> = games
        .into_iter()
        .map(|g| {
            let mut v = json!(g);
            v["stored_name"] = json!(raw.get(&g.game_id));
            v
        })
        .collect();
    let sessions: Vec<(
        Uuid,
        String,
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        i32,
        String,
    )> = sqlx::query_as(
        "SELECT s.id, s.game_id, g.name, s.started_at, s.ended_at, s.duration_s, s.source
         FROM sessions s JOIN games g ON g.id = s.game_id
         WHERE s.user_id = $1 ORDER BY s.started_at DESC LIMIT 40",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let sessions: Vec<Value> = sessions
        .into_iter()
        .map(|(i, g, n, a, b, d, src)| json!({ "id": i, "game_id": g, "name": n, "started_at": a, "ended_at": b, "duration_s": d, "source": src }))
        .collect();
    let friends = crate::friends::friend_ids(&st, id).await?.len();
    let linked: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT provider, coalesce(display, provider_uid), verified FROM linked_accounts WHERE user_id = $1",
    )
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "id": id, "handle": handle, "display_name": display, "email": email, "email_verified": verified,
        "created_at": created, "role": role, "banned_at": banned_at, "ban_reason": reason,
        "share_activity": share, "friend_count": friends, "presence": presence,
        "devices": devices, "games": games, "sessions": sessions,
        "linked": linked.into_iter().map(|(p, d, v)| json!({"provider": p, "display": d, "verified": v})).collect::<Vec<_>>(),
    })))
}

/// Sign the account out everywhere (tokens and live sockets).
async fn sign_out_everywhere(st: &AppState, id: Uuid) -> ApiResult<u64> {
    let n = sqlx::query(
        "UPDATE auth_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&st.db)
    .await?
    .rows_affected();
    st.hub.kick(id);
    Ok(n)
}

fn temp_password() -> String {
    use rand::Rng;
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut r = rand::thread_rng();
    let s: String = (0..15)
        .map(|_| A[r.gen_range(0..A.len())] as char)
        .collect();
    format!("{}-{}-{}", &s[0..5], &s[5..10], &s[10..15])
}

async fn reset_password(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (handle, role, _) = target(&st, id).await?;
    outranks(&s, id, role)?;
    let pw = temp_password();
    let p2 = pw.clone();
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&p2))
        .await
        .map_err(AppError::internal)??;
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(&st.db)
        .await?;
    let n = sign_out_everywhere(&st, id).await?;
    audit(
        &st,
        &s,
        "user.reset_password",
        &format!("user:{handle}"),
        Value::Null,
        json!({ "signed_out_tokens": n }),
    )
    .await?;
    Ok(Json(json!({ "temp_password": pw, "signed_out_tokens": n })))
}

#[derive(Deserialize)]
struct BanReq {
    #[serde(default)]
    reason: String,
}

async fn ban(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
    Json(r): Json<BanReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, banned) = target(&st, id).await?;
    outranks(&s, id, role)?;
    if banned {
        return Err(bad("already_banned", "This account is already banned."));
    }
    let reason = clean(&r.reason, 200);
    sqlx::query("UPDATE users SET banned_at = now(), ban_reason = $2 WHERE id = $1")
        .bind(id)
        .bind(if reason.is_empty() {
            None
        } else {
            Some(&reason)
        })
        .execute(&st.db)
        .await?;
    let n = sign_out_everywhere(&st, id).await?;
    audit(
        &st,
        &s,
        "user.ban",
        &format!("user:{handle}"),
        json!({ "banned": false }),
        json!({ "banned": true, "reason": reason, "signed_out_tokens": n }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn unban(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, banned) = target(&st, id).await?;
    outranks(&s, id, role)?;
    if !banned {
        return Err(bad("not_banned", "This account isn't banned."));
    }
    let reason: Option<String> = sqlx::query_scalar(
        "UPDATE users u SET banned_at = NULL, ban_reason = NULL FROM users old
         WHERE u.id = $1 AND old.id = u.id RETURNING old.ban_reason",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    audit(
        &st,
        &s,
        "user.unban",
        &format!("user:{handle}"),
        json!({ "banned": true, "reason": reason }),
        json!({ "banned": false }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct HoursReq {
    game_id: String,
    /// "remove" (all hours for the game) or "cap" (lower tracked hours to `hours`)
    action: String,
    #[serde(default)]
    hours: Option<f64>,
}

async fn tiers(st: &AppState, user: Uuid, game: &str) -> ApiResult<Value> {
    let r: Option<(i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT tracked_min, imported_min, estimated_min, display_min FROM user_game_stats
         WHERE user_id = $1 AND game_id = $2",
    )
    .bind(user)
    .bind(game)
    .fetch_optional(&st.db)
    .await?;
    let sess: (i64, i64) = sqlx::query_as(
        "SELECT count(*), coalesce(sum(duration_s), 0)::bigint FROM sessions WHERE user_id = $1 AND game_id = $2",
    )
    .bind(user)
    .bind(game)
    .fetch_one(&st.db)
    .await?;
    Ok(match r {
        Some((t, i, e, d)) => json!({ "tracked_min": t, "imported_min": i, "estimated_min": e,
                                      "display_min": d, "sessions": sess.0, "session_min": sess.1 / 60 }),
        None => json!({ "display_min": 0, "sessions": 0, "session_min": 0 }),
    })
}

/// Staff may change their own hours, or those of accounts ranked below them.
fn may_edit_hours(s: &Staff, id: Uuid, role: Option<Role>) -> ApiResult<()> {
    if s.user.id == id {
        return Ok(());
    }
    outranks(s, id, role)
}

async fn adjust_hours(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
    Json(r): Json<HoursReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, _) = target(&st, id).await?;
    may_edit_hours(&s, id, role)?;
    let game = r.game_id.trim().to_string();
    if !valid_game_id(&game) {
        return Err(bad("invalid_game", "Unknown game."));
    }
    let before = tiers(&st, id, &game).await?;
    let mut tx = st.db.begin().await?;
    match r.action.as_str() {
        "remove" => {
            for q in [
                "DELETE FROM sessions WHERE user_id = $1 AND game_id = $2",
                "DELETE FROM hours_entries WHERE user_id = $1 AND game_id = $2",
                "DELETE FROM steam_playtime WHERE user_id = $1 AND game_id = $2",
            ] {
                sqlx::query(q)
                    .bind(id)
                    .bind(&game)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        "cap" => {
            let h = r
                .hours
                .filter(|h| h.is_finite() && *h >= 0.0)
                .ok_or_else(|| bad("invalid_hours", "Enter the new total in hours."))?;
            let target_s = (h * 3600.0).round() as i64;
            let cur: i64 = sqlx::query_scalar(
                "SELECT coalesce(sum(duration_s), 0)::bigint FROM sessions WHERE user_id = $1 AND game_id = $2",
            )
            .bind(id)
            .bind(&game)
            .fetch_one(&mut *tx)
            .await?;
            if target_s >= cur {
                return Err(bad(
                    "only_lower",
                    "Hours can only be lowered (the new total must be below the tracked total).",
                ));
            }
            // shrink every session by the same factor, keeping start times
            let f = target_s as f64 / cur as f64;
            sqlx::query(
                "UPDATE sessions SET duration_s = floor(duration_s * $3)::int,
                        ended_at = started_at + make_interval(secs => floor(duration_s * $3)),
                        idle_trimmed_s = 0
                 WHERE user_id = $1 AND game_id = $2",
            )
            .bind(id)
            .bind(&game)
            .bind(f)
            .execute(&mut *tx)
            .await?;
            // self-reported counters above the new total go too
            sqlx::query(
                "DELETE FROM hours_entries WHERE user_id = $1 AND game_id = $2 AND tier = 'imported' AND minutes > $3",
            )
            .bind(id)
            .bind(&game)
            .bind((target_s / 60) as i32)
            .execute(&mut *tx)
            .await?;
        }
        _ => return Err(bad("invalid_action", "action must be remove or cap")),
    }
    tx.commit().await?;
    let after = tiers(&st, id, &game).await?;
    audit(
        &st,
        &s,
        &format!("hours.{}", r.action),
        &format!("user:{handle} game:{game}"),
        before,
        after.clone(),
    )
    .await?;
    Ok(Json(after))
}

async fn delete_session(
    State(st): State<AppState>,
    s: Staff,
    Path((id, sid)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, _) = target(&st, id).await?;
    may_edit_hours(&s, id, role)?;
    let row: Option<(String, DateTime<Utc>, DateTime<Utc>, i32, String)> = sqlx::query_as(
        "DELETE FROM sessions WHERE id = $1 AND user_id = $2
         RETURNING game_id, started_at, ended_at, duration_s, source",
    )
    .bind(sid)
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (g, a, b, d, src) = row.ok_or_else(AppError::not_found)?;
    audit(
        &st,
        &s,
        "hours.delete_session",
        &format!("user:{handle} session:{sid}"),
        json!({ "game_id": g, "started_at": a, "ended_at": b, "duration_s": d, "source": src }),
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// 4. hours review

#[derive(Deserialize)]
struct FlagsQ {
    #[serde(default)]
    days: Option<i32>,
}

async fn hours_flags(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<FlagsQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let days = q.days.unwrap_or(30).clamp(1, 365);
    let mut flags: Vec<Value> = Vec::new();

    // more than 20 h of play on one (UTC) day
    let rows: Vec<(Uuid, String, chrono::NaiveDate, i64, i64)> = sqlx::query_as(
        "SELECT s.user_id, u.handle, (s.started_at AT TIME ZONE 'UTC')::date AS d,
                sum(s.duration_s)::bigint, count(*)
         FROM sessions s JOIN users u ON u.id = s.user_id
         WHERE s.started_at > now() - make_interval(days => $1) AND u.deleted_at IS NULL
         GROUP BY 1, 2, 3 HAVING sum(s.duration_s) > 20 * 3600
         ORDER BY 3 DESC LIMIT 100",
    )
    .bind(days)
    .fetch_all(&st.db)
    .await?;
    for (u, h, d, secs, n) in rows {
        flags.push(
            json!({ "kind": "day_over_20h", "user_id": u, "handle": h, "when": d,
                           "minutes": secs / 60, "detail": format!("{n} sessions on {d} (UTC)") }),
        );
    }

    // single sessions over 18 h
    let rows: Vec<(Uuid, String, Uuid, String, String, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT s.user_id, u.handle, s.id, s.game_id, g.name, s.started_at, s.duration_s
         FROM sessions s JOIN users u ON u.id = s.user_id JOIN games g ON g.id = s.game_id
         WHERE s.started_at > now() - make_interval(days => $1) AND s.duration_s > 18 * 3600
           AND u.deleted_at IS NULL
         ORDER BY s.started_at DESC LIMIT 100",
    )
    .bind(days)
    .fetch_all(&st.db)
    .await?;
    for (u, h, sid, g, n, at, d) in rows {
        flags.push(
            json!({ "kind": "session_over_18h", "user_id": u, "handle": h, "session_id": sid,
                           "game_id": g, "game_name": n, "when": at, "minutes": d / 60,
                           "detail": "one session longer than 18 hours" }),
        );
    }

    // overlapping sessions: hours counted twice for the same stretch of time
    let rows: Vec<(Uuid, String, i64, i64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT a.user_id, u.handle, count(*),
                sum(EXTRACT(EPOCH FROM (LEAST(a.ended_at, b.ended_at) - GREATEST(a.started_at, b.started_at))))::bigint,
                max(a.started_at)
         FROM sessions a
         JOIN sessions b ON b.user_id = a.user_id AND b.id > a.id
              AND b.started_at < a.ended_at AND a.started_at < b.ended_at
         JOIN users u ON u.id = a.user_id
         WHERE a.started_at > now() - make_interval(days => $1)
           AND b.started_at > now() - make_interval(days => $1 + 1)
           AND LEAST(a.ended_at, b.ended_at) - GREATEST(a.started_at, b.started_at) > interval '5 minutes'
           AND u.deleted_at IS NULL
         GROUP BY 1, 2 ORDER BY 4 DESC LIMIT 100",
    )
    .bind(days)
    .fetch_all(&st.db)
    .await?;
    for (u, h, n, secs, at) in rows {
        flags.push(json!({ "kind": "overlapping_sessions", "user_id": u, "handle": h, "when": at,
                           "minutes": secs / 60,
                           "detail": format!("{n} overlapping session pair(s): hours growing faster than real time") }));
    }

    // tracked total for a game larger than the time since its first session
    let rows: Vec<(Uuid, String, String, String, i64, DateTime<Utc>)> = sqlx::query_as(
        "SELECT s.user_id, u.handle, s.game_id, g.name, sum(s.duration_s)::bigint, min(s.started_at)
         FROM sessions s JOIN users u ON u.id = s.user_id JOIN games g ON g.id = s.game_id
         WHERE u.deleted_at IS NULL
         GROUP BY 1, 2, 3, 4
         HAVING sum(s.duration_s) > EXTRACT(EPOCH FROM now() - min(s.started_at)) + 600
         LIMIT 100",
    )
    .fetch_all(&st.db)
    .await?;
    for (u, h, g, n, secs, first) in rows {
        flags.push(
            json!({ "kind": "faster_than_real_time", "user_id": u, "handle": h, "game_id": g,
                           "game_name": n, "when": first, "minutes": secs / 60,
                           "detail": "more hours than time passed since the first session" }),
        );
    }
    Ok(Json(json!({ "days": days, "flags": flags })))
}

// ---------------------------------------------------------------------------
// 5. server

fn read(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

fn meminfo() -> Value {
    let m = read("/proc/meminfo");
    let get = |k: &str| -> i64 {
        m.lines()
            .find(|l| l.starts_with(&format!("{k}:")))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0)
            * 1024
    };
    json!({ "total": get("MemTotal"), "available": get("MemAvailable"),
            "swap_total": get("SwapTotal"), "swap_free": get("SwapFree") })
}

fn cpu_times() -> Option<(u64, u64)> {
    let s = read("/proc/stat");
    let v: Vec<u64> = s
        .lines()
        .next()?
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    let idle = v.get(3)? + v.get(4).copied().unwrap_or(0);
    Some((v.iter().sum(), idle))
}

async fn cpu_percent() -> Option<f64> {
    let (t1, i1) = cpu_times()?;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (t2, i2) = cpu_times()?;
    let dt = t2.saturating_sub(t1) as f64;
    (dt > 0.0).then(|| ((dt - i2.saturating_sub(i1) as f64) / dt * 1000.0).round() / 10.0)
}

async fn cmd(prog: &str, args: &[&str]) -> Option<String> {
    let prog = prog.to_string();
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let run =
        tokio::task::spawn_blocking(move || std::process::Command::new(prog).args(args).output());
    let out = tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .ok()?
        .ok()?
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

pub const SERVICES: [&str; 5] = [
    "fraglink-server",
    "caddy",
    "postgresql@16-main",
    "fraggle",
    "fraglink-backup.timer",
];

async fn server(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (accounts, banned, d1, d7, d30): (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE banned_at IS NOT NULL),
                count(*) FILTER (WHERE created_at > now() - interval '1 day'),
                count(*) FILTER (WHERE created_at > now() - interval '7 days'),
                count(*) FILTER (WHERE created_at > now() - interval '30 days')
         FROM users WHERE deleted_at IS NULL",
    )
    .fetch_one(&st.db)
    .await?;
    let (playing, sess24, min24): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM presence WHERE status IN ('online','away') AND game_id IS NOT NULL),
                count(*), coalesce(sum(duration_s) / 60, 0)::bigint
         FROM sessions WHERE ended_at > now() - interval '1 day'",
    )
    .fetch_one(&st.db)
    .await?;
    let db_size: i64 = sqlx::query_scalar("SELECT pg_database_size(current_database())")
        .fetch_one(&st.db)
        .await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT lower(exe)) FROM game_suggestions WHERE status = 'new'",
    )
    .fetch_one(&st.db)
    .await?;
    let load: Vec<f64> = read("/proc/loadavg")
        .split_whitespace()
        .take(3)
        .filter_map(|x| x.parse().ok())
        .collect();
    let disk = cmd("df", &["-B1", "-P", "/"]).await.and_then(|o| {
        let l = o
            .lines()
            .nth(1)?
            .split_whitespace()
            .collect::<Vec<_>>()
            .clone();
        Some(
            json!({ "total": l.get(1)?.parse::<i64>().ok()?, "used": l.get(2)?.parse::<i64>().ok()?,
                     "available": l.get(3)?.parse::<i64>().ok()? }),
        )
    });
    let mut services = Vec::new();
    for svc in SERVICES {
        let state = cmd("systemctl", &["is-active", svc])
            .await
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown".into());
        services.push(json!({ "name": svc, "state": state }));
    }
    let uptime_s: f64 = read("/proc/uptime")
        .split_whitespace()
        .next()
        .and_then(|x| x.parse().ok())
        .unwrap_or(0.0);
    // recent warnings and errors from the server's journal
    let journal = cmd(
        "journalctl",
        &[
            "-u",
            "fraglink-server",
            "--since",
            "-48h",
            "-n",
            "4000",
            "-o",
            "short-iso",
            "--no-pager",
            "-q",
        ],
    )
    .await;
    let errors: Option<Vec<String>> = journal.map(|j| {
        let mut v: Vec<String> = j
            .lines()
            .map(strip_ansi)
            .filter(|l| l.contains(" WARN ") || l.contains(" ERROR ") || l.contains("panicked"))
            .map(|l| l.chars().take(400).collect())
            .collect();
        v.reverse();
        v.truncate(60);
        v
    });
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "process_uptime_s": STARTED.get().map(|t| t.elapsed().as_secs()).unwrap_or(0),
        "system_uptime_s": uptime_s as u64,
        "online": { "users": st.hub.online_users(), "sockets": st.hub.connection_count(), "playing": playing },
        "accounts": { "total": accounts, "banned": banned, "signups_24h": d1, "signups_7d": d7, "signups_30d": d30 },
        "sessions_24h": { "count": sess24, "minutes": min24 },
        "pending_approvals": pending,
        "cpu": { "percent": cpu_percent().await, "load": load,
                 "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) },
        "memory": meminfo(),
        "disk": disk,
        "database_bytes": db_size,
        "services": services,
        "errors": errors,
        "checked_at": Utc::now(),
    })))
}

// ---------------------------------------------------------------------------
// 6. announcements

type AnnRow = (
    i64,
    String,
    String,
    Option<String>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn ann_json(r: AnnRow) -> Value {
    let (id, message, style, link, ends_at, created_at, cleared_at) = r;
    json!({ "id": id, "message": message, "style": style, "link": link, "ends_at": ends_at,
            "created_at": created_at, "cleared_at": cleared_at })
}

pub async fn active_announcement(st: &AppState) -> ApiResult<Option<Value>> {
    let r: Option<AnnRow> = sqlx::query_as(
        "SELECT id, message, style, link, ends_at, created_at, cleared_at FROM announcements
         WHERE cleared_at IS NULL AND (ends_at IS NULL OR ends_at > now())
         ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    Ok(r.map(|r| {
        let mut v = ann_json(r);
        if let Some(o) = v.as_object_mut() {
            o.remove("cleared_at");
            o.remove("created_at");
        }
        v
    }))
}

/// `GET /api/v1/announcement`: the banner to show (or null). Public, cached 60 s.
async fn announcement_public(State(st): State<AppState>) -> ApiResult<impl IntoResponse> {
    let a = active_announcement(&st).await?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(json!({ "announcement": a })),
    ))
}

async fn announcements(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows: Vec<AnnRow> = sqlx::query_as(
        "SELECT id, message, style, link, ends_at, created_at, cleared_at FROM announcements
         ORDER BY created_at DESC LIMIT 20",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "active": active_announcement(&st).await?,
        "recent": rows.into_iter().map(ann_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct AnnounceReq {
    message: String,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    ends_at: Option<DateTime<Utc>>,
}

async fn announce(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<AnnounceReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let message = clean(&r.message, 400);
    if message.is_empty() || message.chars().count() > 300 {
        return Err(bad(
            "invalid_message",
            "Write a message of up to 300 characters.",
        ));
    }
    let style = r.style.unwrap_or_else(|| "info".into());
    if style != "info" && style != "warning" {
        return Err(bad("invalid_style", "Style is info or warning."));
    }
    let link = r
        .link
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty());
    if let Some(l) = &link {
        let ok = (l.starts_with("https://") || (l.starts_with('/') && !l.starts_with("//")))
            && l.len() <= 300
            && !l.chars().any(|c| c.is_whitespace() || c.is_control());
        if !ok {
            return Err(bad("invalid_link", "Links start with https:// or /."));
        }
    }
    if r.ends_at.is_some_and(|e| e <= Utc::now()) {
        return Err(bad("invalid_end", "The end time is in the past."));
    }
    let before = active_announcement(&st).await?;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE announcements SET cleared_at = now() WHERE cleared_at IS NULL")
        .execute(&mut *tx)
        .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO announcements(message, style, link, ends_at, created_by) VALUES($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&message)
    .bind(&style)
    .bind(&link)
    .bind(r.ends_at)
    .bind(s.user.id)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    let after = active_announcement(&st).await?;
    audit(
        &st,
        &s,
        "announcement.create",
        &format!("announcement:{id}"),
        json!(before),
        json!(after),
    )
    .await?;
    Ok(Json(json!({ "announcement": after })))
}

async fn announce_clear(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let before = active_announcement(&st).await?;
    let n = sqlx::query("UPDATE announcements SET cleared_at = now() WHERE cleared_at IS NULL")
        .execute(&st.db)
        .await?
        .rows_affected();
    audit(
        &st,
        &s,
        "announcement.clear",
        "announcement",
        json!(before),
        json!({ "cleared": n }),
    )
    .await?;
    Ok(Json(json!({ "cleared": n })))
}

// ---------------------------------------------------------------------------
// 7. staff

async fn staff_list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows: Vec<(Uuid, String, String, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, handle, display_name, role, created_at FROM users
         WHERE role IS NOT NULL AND deleted_at IS NULL
         ORDER BY CASE role WHEN 'owner' THEN 0 WHEN 'admin' THEN 1 ELSE 2 END, lower(handle)",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "you": { "handle": s.user.handle, "role": s.role.as_str() },
        "staff": rows.into_iter().map(|(i, h, d, r, c)| json!({"id": i, "handle": h, "display_name": d, "role": r, "member_since": c})).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct StaffReq {
    handle: String,
    /// "admin", "moderator" or "none"
    role: String,
}

async fn staff_set(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<StaffReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let new = match r.role.as_str() {
        "admin" => Some(Role::Admin),
        "moderator" => Some(Role::Moderator),
        "none" => None,
        _ => return Err(bad("invalid_role", "Role is admin, moderator or none.")),
    };
    let row: Option<(Uuid, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, handle, role, banned_at IS NOT NULL FROM users
         WHERE lower(handle) = lower($1) AND deleted_at IS NULL",
    )
    .bind(r.handle.trim().trim_start_matches('@'))
    .fetch_optional(&st.db)
    .await?;
    let (id, handle, cur, banned) = row.ok_or_else(|| {
        AppError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "No Fraglink user with that name.",
        )
    })?;
    let cur = cur.as_deref().and_then(Role::parse);
    if cur == Some(Role::Owner) {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "owner_locked",
            "The owner can't be changed here (use Transfer ownership).",
        ));
    }
    if id == s.user.id {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "You can't change your own role.",
        ));
    }
    // only the owner adds, removes or changes admins
    if (new == Some(Role::Admin) || cur == Some(Role::Admin)) && s.role != Role::Owner {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Only the owner can add or remove admins.",
        ));
    }
    if banned && new.is_some() {
        return Err(bad("banned", "Unban this account first."));
    }
    if new == cur {
        return Ok(Json(
            json!({ "handle": handle, "role": new.map(Role::as_str) }),
        ));
    }
    sqlx::query("UPDATE users SET role = $2 WHERE id = $1")
        .bind(id)
        .bind(new.map(Role::as_str))
        .execute(&st.db)
        .await?;
    audit(
        &st,
        &s,
        "staff.set_role",
        &format!("user:{handle}"),
        json!({ "role": cur.map(Role::as_str) }),
        json!({ "role": new.map(Role::as_str) }),
    )
    .await?;
    Ok(Json(
        json!({ "handle": handle, "role": new.map(Role::as_str) }),
    ))
}

#[derive(Deserialize)]
struct TransferReq {
    handle: String,
    /// The owner types their own username to confirm.
    confirm: String,
}

async fn staff_transfer(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<TransferReq>,
) -> ApiResult<Json<Value>> {
    if s.role != Role::Owner {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Only the owner can transfer ownership.",
        ));
    }
    if !r.confirm.trim().eq_ignore_ascii_case(&s.user.handle) {
        return Err(bad("confirm", "Type your own username to confirm."));
    }
    let row: Option<(Uuid, String, Option<String>, bool)> = sqlx::query_as(
        "SELECT id, handle, role, banned_at IS NOT NULL FROM users
         WHERE lower(handle) = lower($1) AND deleted_at IS NULL",
    )
    .bind(r.handle.trim().trim_start_matches('@'))
    .fetch_optional(&st.db)
    .await?;
    let (id, handle, cur, banned) = row.ok_or_else(|| {
        AppError::new(
            StatusCode::NOT_FOUND,
            "user_not_found",
            "No Fraglink user with that name.",
        )
    })?;
    if id == s.user.id || banned {
        return Err(bad("invalid_target", "Pick another, active account."));
    }
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE users SET role = 'admin' WHERE id = $1")
        .bind(s.user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET role = 'owner' WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit(
        &st,
        &s,
        "staff.transfer_ownership",
        &format!("user:{handle}"),
        json!({ "owner": s.user.handle, "new_owner_role": cur }),
        json!({ "owner": handle, "previous_owner_role": "admin" }),
    )
    .await?;
    Ok(Json(json!({ "owner": handle })))
}

#[derive(Deserialize)]
struct AuditQ {
    #[serde(default)]
    limit: Option<i64>,
}

async fn audit_list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<AuditQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows: Vec<(
        i64,
        String,
        String,
        Option<String>,
        Option<Value>,
        Option<Value>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT id, actor_handle, action, target, before, after, created_at FROM admin_audit
             ORDER BY created_at DESC, id DESC LIMIT $1",
    )
    .bind(q.limit.unwrap_or(100).clamp(1, 500))
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "entries": rows.into_iter().map(|(i, a, act, t, b, af, c)| json!({"id": i, "actor": a, "action": act, "target": t, "before": b, "after": af, "at": c})).collect::<Vec<_>>(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_rank() {
        assert!(Role::Owner > Role::Admin && Role::Admin > Role::Moderator);
        assert_eq!(Role::parse("admin"), Some(Role::Admin));
        assert_eq!(Role::parse("root"), None);
        assert_eq!(rank(None), 0);
        let p = permissions(Role::Moderator);
        assert_eq!(p["games"], false);
        assert_eq!(p["ban"], true);
        assert_eq!(permissions(Role::Admin)["manage_admins"], false);
        assert_eq!(permissions(Role::Owner)["manage_admins"], true);
    }

    #[test]
    fn temp_passwords() {
        let p = temp_password();
        assert_eq!(p.len(), 17);
        assert!(p.chars().count() >= 10);
        assert_ne!(p, temp_password());
        assert_eq!(strip_ansi("\u{1b}[2mx\u{1b}[0m WARN y"), "x WARN y");
    }
}

#[derive(Deserialize)]
struct StatusBody {
    lifecycle: String,
    /// `YYYY-MM-DD` or empty
    #[serde(default)]
    date: Option<String>,
    #[serde(default)]
    note: Option<String>,
    /// also stamp "last checked: now, by me"
    #[serde(default)]
    checked: bool,
}

/// Edit Game > Status: lifecycle, expected end/launch date, note (and "checked").
async fn game_status_save(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !valid_game_id(&id) || game_info(&st, &id).await?.is_none() {
        return Err(AppError::not_found());
    }
    let upd = crate::game_status::Update {
        lifecycle: Some(b.lifecycle),
        date: Some(b.date.unwrap_or_default()),
        note: Some(b.note.unwrap_or_default()),
        checked_by: b.checked.then(|| s.user.handle.clone()),
    };
    let before = crate::game_status::get(&st, &id).await?;
    let after = crate::game_status::set(&st.db, &id, upd)
        .await
        .map_err(|e| bad("bad_status", &e))?;
    audit(
        &st,
        &s,
        "game.status",
        &id,
        before.unwrap_or(Value::Null),
        after,
    )
    .await?;
    Ok(Json(game_info(&st, &id).await?.unwrap_or(Value::Null)))
}

// ------------------------------------------------------------------ avatars & two-step

/// Open avatar reports, grouped per reported user (newest first).
async fn avatar_reports(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let rows: Vec<(Uuid, String, Option<String>, Option<String>, i64, DateTime<Utc>, Option<String>, Vec<i64>)> = sqlx::query_as(
        "SELECT u.id, u.handle, u.avatar_kind, u.avatar_hash, count(*), max(r.created_at),
                (array_agg(r.reason ORDER BY r.created_at DESC) FILTER (WHERE r.reason IS NOT NULL))[1],
                array_agg(r.id)
         FROM avatar_reports r JOIN users u ON u.id = r.user_id
         WHERE r.resolved_at IS NULL AND u.deleted_at IS NULL
         GROUP BY u.id, u.handle, u.avatar_kind, u.avatar_hash ORDER BY max(r.created_at) DESC LIMIT 200",
    )
    .fetch_all(&st.db)
    .await?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, h, k, hash, n, at, reason, ids)| {
            json!({ "user_id": id, "handle": h, "reports": n, "last_at": at, "reason": reason, "report_ids": ids,
                    "avatar": crate::avatars::avatar_json(id, k.as_deref(), hash.as_deref(), None) })
        })
        .collect();
    Ok(Json(json!({ "reports": list })))
}

async fn avatar_dismiss(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    // `id` is the reported user's id: dismiss all their open reports.
    s.need(Role::Moderator)?;
    let (handle, _, _) = target(&st, id).await?;
    let n = sqlx::query(
        "UPDATE avatar_reports SET resolved_at = now(), resolved_by = $2, resolution = 'dismissed'
         WHERE user_id = $1 AND resolved_at IS NULL",
    )
    .bind(id)
    .bind(s.user.id)
    .execute(&st.db)
    .await?
    .rows_affected();
    audit(
        &st,
        &s,
        "avatar.dismiss",
        &format!("user:{handle}"),
        Value::Null,
        json!({ "reports": n }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "dismissed": n })))
}

async fn avatar_remove(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, _) = target(&st, id).await?;
    if s.user.id != id {
        outranks(&s, id, role)?;
    }
    let blocked = crate::avatars::remove_and_block(&st, id, s.user.id).await?;
    audit(
        &st,
        &s,
        "avatar.remove",
        &format!("user:{handle}"),
        json!({ "avatar": "upload" }),
        json!({ "avatar": "default", "blocked_hash": blocked }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn mfa_reset(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (handle, role, _) = target(&st, id).await?;
    outranks(&s, id, role)?;
    let was = crate::mfa::reset(&st, id).await?;
    audit(
        &st,
        &s,
        "user.mfa_reset",
        &format!("user:{handle}"),
        json!({ "mfa": was }),
        json!({ "mfa": false }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "was_enabled": was })))
}

// ------------------------------------------------------------------ game art overrides

async fn art_game_exists(st: &AppState, id: &str) -> ApiResult<()> {
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id = $1)")
        .bind(id)
        .fetch_one(&st.db)
        .await?;
    if ok || st.games.icon_index().by_slug.contains_key(id) {
        Ok(())
    } else {
        Err(AppError::new(
            StatusCode::NOT_FOUND,
            "game_not_found",
            "No such game.",
        ))
    }
}

async fn art_saved(
    st: &AppState,
    s: &Staff,
    id: &str,
    how: &str,
    bytes: &[u8],
) -> ApiResult<Json<Value>> {
    crate::art::set_override(st, id, bytes).map_err(|m| AppError::bad_request("bad_image", m))?;
    audit(
        st,
        s,
        "game.art",
        &format!("game:{id}"),
        json!({}),
        json!({ "art": how }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "source": "override" })))
}

async fn game_art_upload(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    art_game_exists(&st, &id).await?;
    art_saved(&st, &s, &id, "upload", &body).await
}

#[derive(Deserialize)]
struct ArtUrl {
    url: String,
}

async fn game_art_url(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
    Json(r): Json<ArtUrl>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    art_game_exists(&st, &id).await?;
    let u = reqwest::Url::parse(r.url.trim())
        .map_err(|_| AppError::bad_request("bad_url", "That isn't a link."))?;
    let host = u.host_str().unwrap_or("");
    if u.scheme() != "https"
        || host.is_empty()
        || host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok()
    {
        return Err(AppError::bad_request(
            "bad_url",
            "Use an https link to a picture.",
        ));
    }
    let b = crate::art::fetch(u.as_str())
        .await
        .ok_or_else(|| AppError::bad_request("bad_url", "Couldn't download that picture."))?;
    art_saved(&st, &s, &id, &format!("url {}", u.as_str()), &b).await
}

async fn game_art_clear(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let had = crate::art::clear_override(&st, &id);
    if had {
        audit(
            &st,
            &s,
            "game.art",
            &format!("game:{id}"),
            json!({ "art": "override" }),
            json!({ "art": "auto" }),
        )
        .await?;
    }
    Ok(Json(
        json!({ "ok": true, "source": crate::art::source_of(&st, &id).await }),
    ))
}
