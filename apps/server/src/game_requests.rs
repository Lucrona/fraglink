//! Public "Missing a game?" requests on /games, and the Admin requests queue.
//! Does not touch hours, sessions, uploads or merge behaviour.

use crate::admin::{self, Role, Staff};
use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::game_history;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::Duration;
use uuid::Uuid;

const NAME_MAX: usize = 80;
const LINK_MAX: usize = 240;
const NOTE_MAX: usize = 500;
const DAY_LIMIT: i64 = 5;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/games/requests", post(submit))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/game-requests", get(admin_list))
        .route(
            "/api/v1/admin/game-requests/{id}/approve",
            post(admin_approve),
        )
        .route(
            "/api/v1/admin/game-requests/{id}/status",
            post(admin_status),
        )
}

/// Lower-case, marks stripped, punctuation turned into spaces, collapsed.
pub fn name_norm(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        // Tabs, marks and punctuation become spaces so "OF\tWarcraft" matches "of warcraft".
        if c == '™' || c == '®' || c == '©' {
            continue;
        }
        if c.is_alphanumeric() {
            for d in c.to_lowercase() {
                out.push(d);
            }
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim().to_string()
}

fn clean_line(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn already_listed(games: &[(String, String)], norm: &str) -> Option<(String, String)> {
    games.iter().find_map(|(id, name)| {
        (name_norm(name) == *norm || name_norm(id) == *norm).then(|| (id.clone(), name.clone()))
    })
}

#[derive(Deserialize)]
struct SubmitReq {
    #[serde(default)]
    name: String,
    #[serde(default)]
    exe_or_link: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    turnstile: Option<String>,
}

async fn submit(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<SubmitReq>,
) -> ApiResult<Json<Value>> {
    crate::turnstile::check(&st, &headers, r.turnstile.as_deref()).await?;
    let name = clean_line(&r.name, NAME_MAX);
    let norm = name_norm(&name);
    if name.is_empty() || norm.is_empty() {
        return Err(AppError::bad_request(
            "name",
            "Please enter the game's name.",
        ));
    }
    let exe_or_link = clean_line(&r.exe_or_link, LINK_MAX);
    let note = clean_line(&r.note, NOTE_MAX);
    let exe_or_link = (!exe_or_link.is_empty()).then_some(exe_or_link);
    let note = (!note.is_empty()).then_some(note);

    let listed = game_history::public_catalogue(&st).await?;
    if let Some((id, listed_name)) = already_listed(&listed, &norm) {
        return Ok(Json(json!({
            "already_listed": true,
            "game": { "id": id, "name": listed_name },
        })));
    }

    if let Some(id) = existing_open(&st.db, &norm).await? {
        sqlx::query(
            "UPDATE game_requests SET reports = reports + 1 WHERE id = $1 AND status = 'open'",
        )
        .bind(id)
        .execute(&st.db)
        .await?;
        return Ok(Json(
            json!({ "ok": true, "already_requested": true, "id": id }),
        ));
    }

    st.rate_limit(
        format!("game-request:{}", a.user.id),
        DAY_LIMIT as usize,
        Duration::from_secs(24 * 3600),
    )?;
    let today: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_requests
         WHERE user_id = $1 AND created_at > now() - interval '1 day'",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    if today >= DAY_LIMIT {
        return Err(AppError::new(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "You can request up to 5 games a day. Try again tomorrow.",
        ));
    }

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO game_requests (user_id, name, name_norm, exe_or_link, note)
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(a.user.id)
    .bind(&name)
    .bind(&norm)
    .bind(&exe_or_link)
    .bind(&note)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn existing_open(db: &PgPool, norm: &str) -> ApiResult<Option<i64>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM game_requests WHERE name_norm = $1 AND status = 'open'")
            .bind(norm)
            .fetch_optional(db)
            .await?,
    )
}

#[derive(Deserialize, Default)]
struct ListQ {
    status: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}

type ReqRow = (
    i64,
    DateTime<Utc>,
    Uuid,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    i32,
    Option<DateTime<Utc>>,
    Option<String>,
    String,
    String,
);

async fn admin_list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let status = q.status.as_deref().unwrap_or("open");
    if !matches!(
        status,
        "open" | "approved" | "done" | "duplicate" | "dismissed" | "all"
    ) {
        return Err(AppError::bad_request("status", "Unknown status."));
    }
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let offset = q.offset.unwrap_or(0).max(0);
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM game_requests WHERE status = 'open'")
        .fetch_one(&st.db)
        .await?;
    let (rows, total): (Vec<ReqRow>, i64) = if status == "all" {
        let total = sqlx::query_scalar("SELECT count(*) FROM game_requests")
            .fetch_one(&st.db)
            .await?;
        let rows = sqlx::query_as(
            "SELECT r.id, r.created_at, r.user_id, r.name, r.name_norm, r.exe_or_link, r.note,
                    r.status, r.reports, r.decided_at, r.game_id, u.handle,
                    coalesce(d.handle, '')
             FROM game_requests r
             JOIN users u ON u.id = r.user_id
             LEFT JOIN users d ON d.id = r.decided_by
             ORDER BY r.created_at DESC, r.id DESC
             LIMIT $1 OFFSET $2",
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&st.db)
        .await?;
        (rows, total)
    } else {
        let total = sqlx::query_scalar("SELECT count(*) FROM game_requests WHERE status = $1")
            .bind(status)
            .fetch_one(&st.db)
            .await?;
        let rows = sqlx::query_as(
            "SELECT r.id, r.created_at, r.user_id, r.name, r.name_norm, r.exe_or_link, r.note,
                    r.status, r.reports, r.decided_at, r.game_id, u.handle,
                    coalesce(d.handle, '')
             FROM game_requests r
             JOIN users u ON u.id = r.user_id
             LEFT JOIN users d ON d.id = r.decided_by
             WHERE r.status = $1
             ORDER BY r.created_at DESC, r.id DESC
             LIMIT $2 OFFSET $3",
        )
        .bind(status)
        .bind(limit)
        .bind(offset)
        .fetch_all(&st.db)
        .await?;
        (rows, total)
    };
    let requests: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.0,
                "at": r.1,
                "user_id": r.2,
                "name": r.3,
                "name_norm": r.4,
                "exe_or_link": r.5,
                "note": r.6,
                "status": r.7,
                "reports": r.8,
                "decided_at": r.9,
                "game_id": r.10,
                "by": r.11,
                "decided_by": if r.12.is_empty() { Value::Null } else { json!(r.12) },
            })
        })
        .collect();
    Ok(Json(json!({
        "requests": requests,
        "total": total,
        "open": open,
        "offset": offset,
        "limit": limit,
    })))
}

#[derive(Deserialize)]
struct StatusReq {
    status: String,
}

async fn admin_status(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(r): Json<StatusReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !matches!(r.status.as_str(), "done" | "duplicate" | "dismissed") {
        return Err(AppError::bad_request(
            "status",
            "Use done, duplicate or dismissed.",
        ));
    }
    let n = sqlx::query(
        "UPDATE game_requests
         SET status = $2, decided_at = now(), decided_by = $3
         WHERE id = $1 AND status = 'open'",
    )
    .bind(id)
    .bind(&r.status)
    .bind(s.user.id)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    admin::audit(
        &st,
        &s,
        "game_request.status",
        &format!("game_request:{id}"),
        Value::Null,
        json!({ "status": r.status }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "id": id, "status": r.status })))
}

#[derive(Deserialize)]
struct ApproveReq {
    #[serde(default)]
    name: String,
    #[serde(default)]
    exe: String,
    #[serde(default)]
    game_id: String,
}

async fn admin_approve(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(r): Json<ApproveReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT name, status, coalesce(exe_or_link, '') FROM game_requests WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let Some((req_name, status, submitted)) = row else {
        return Err(AppError::not_found());
    };
    if status != "open" {
        return Err(AppError::bad_request(
            "not_open",
            "That request has already been decided.",
        ));
    }
    let name = {
        let n = clean_line(&r.name, NAME_MAX);
        if n.is_empty() {
            req_name
        } else {
            n
        }
    };
    let exe_in = clean_line(&r.exe, LINK_MAX);
    let exe = if crate::games::valid_exe(&exe_in) {
        exe_in.clone()
    } else if crate::games::valid_exe(submitted.trim()) {
        submitted.trim().to_string()
    } else {
        String::new()
    };
    let steam_src = if exe_in.is_empty() {
        submitted.as_str()
    } else {
        exe_in.as_str()
    };
    let game_id_in = r.game_id.trim().to_string();

    let mut tx = st.db.begin().await?;
    let (game_id, created) = if !game_id_in.is_empty() {
        let in_db: bool = sqlx::query_scalar("SELECT exists(SELECT 1 FROM games WHERE id = $1)")
            .bind(&game_id_in)
            .fetch_one(&mut *tx)
            .await?;
        if !in_db {
            let listed = game_history::public_catalogue(&st).await?;
            if !listed.iter().any(|(id, _)| id == &game_id_in) {
                return Err(AppError::bad_request(
                    "game_id",
                    "That game id isn't on the list.",
                ));
            }
        }
        (game_id_in, false)
    } else if !exe.is_empty() {
        let new_id = crate::names::approve_tx(&mut tx, &exe, &name)
            .await
            .map_err(|e| AppError::bad_request("exe", e.to_string()))?;
        game_history::insert(
            &mut tx,
            &st,
            game_history::New::admin(&s, "added", format!("Added {name}"))
                .game(&new_id, Some(name.clone()))
                .diff(json!({ "name": name, "exe": exe, "from": "request" }))
                .public(true),
        )
        .await?;
        (new_id, true)
    } else if let Some(appid) = parse_steam_appid(steam_src) {
        let new_id = format!("steam:{appid}");
        let existed: bool = sqlx::query_scalar("SELECT exists(SELECT 1 FROM games WHERE id = $1)")
            .bind(&new_id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO games(id, name, steam_appid, verified) VALUES($1, $2, $3, true)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, verified = true",
        )
        .bind(&new_id)
        .bind(&name)
        .bind(appid)
        .execute(&mut *tx)
        .await?;
        if !existed {
            game_history::insert(
                &mut tx,
                &st,
                game_history::New::admin(&s, "added", format!("Added {name}"))
                    .game(&new_id, Some(name.clone()))
                    .diff(json!({ "name": name, "steam_appid": appid, "from": "request" }))
                    .public(true),
            )
            .await?;
        }
        (new_id, !existed)
    } else {
        return Err(AppError::bad_request(
            "need_game",
            "Add a program name (Game.exe), a Steam store link, or an existing game id.",
        ));
    };

    sqlx::query(
        "UPDATE game_requests
         SET status = 'approved', decided_at = now(), decided_by = $2, game_id = $3
         WHERE id = $1 AND status = 'open'",
    )
    .bind(id)
    .bind(s.user.id)
    .bind(&game_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    admin::audit(
        &st,
        &s,
        "game_request.approve",
        &format!("game_request:{id}"),
        Value::Null,
        json!({ "game_id": game_id, "created": created }),
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "id": id,
        "status": "approved",
        "game_id": game_id,
        "created": created,
    })))
}

/// Steam app id from a store URL, `steam:123` id, or a bare number.
pub fn parse_steam_appid(s: &str) -> Option<i32> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("steam:") {
        return rest.parse().ok().filter(|n: &i32| *n > 0);
    }
    let lower = s.to_ascii_lowercase();
    for pat in ["/app/", "steam://store/", "steam://run/"] {
        if let Some(i) = lower.find(pat) {
            let rest = &s[i + pat.len()..];
            let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
            return digits.parse().ok().filter(|n: &i32| *n > 0);
        }
    }
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        return s.parse().ok().filter(|n: &i32| *n > 0);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{name_norm, parse_steam_appid};

    #[test]
    fn name_norm_collapses_and_strips_marks() {
        assert_eq!(name_norm("World of Warcraft™"), "world of warcraft");
        assert_eq!(name_norm("  WORLD   OF\tWarcraft "), "world of warcraft");
        assert_eq!(name_norm("Game 2"), "game 2");
        assert_eq!(name_norm("10 Miles to Safety"), "10 miles to safety");
    }

    #[test]
    fn steam_appid_from_url_and_id() {
        assert_eq!(
            parse_steam_appid("https://store.steampowered.com/app/570/Dota_2/"),
            Some(570)
        );
        assert_eq!(parse_steam_appid("steam:2732960"), Some(2732960));
        assert_eq!(parse_steam_appid("570"), Some(570));
        assert_eq!(parse_steam_appid("Game.exe"), None);
        assert_eq!(parse_steam_appid(""), None);
    }
}
