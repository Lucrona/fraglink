//! JSON API (`/api/v1/...`). Auth: `Authorization: Bearer <token>`.

use crate::auth::{self, AuthUser, User};
use crate::error::{ApiResult, AppError};
use crate::hours::{self, Era};
use crate::steam::{self, ProfileRef};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeZone, Utc};
use fraglink_core::outbox::UploadBatch;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const MAX_UPLOAD_SESSIONS: usize = 500;
const MAX_SESSION_SECS: i64 = 24 * 3600;
const CLOCK_SKEW_SECS: i64 = 300;
const OVERLAP_TOLERANCE_SECS: i64 = 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/signup", post(signup))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(me).delete(delete_me))
        .route("/api/v1/device/start", post(device_start))
        .route("/api/v1/device/poll", post(device_poll))
        .route("/api/v1/device/approve", post(device_approve))
        .route("/api/v1/devices", get(devices))
        .route("/api/v1/devices/{id}", delete(revoke_device))
        .route("/api/v1/sessions/upload", post(upload_sessions))
        .route("/api/v1/stats", get(stats))
        .route("/api/v1/leaderboard/{game_id}", get(leaderboard))
        .route("/api/v1/hours/estimate", post(save_estimate))
        .route("/api/v1/hours/import", post(save_import))
        .route("/api/v1/hours/{id}", delete(delete_hours))
        .route("/api/v1/steam/link", post(steam_link))
        .route("/api/v1/steam/sync", post(steam_sync))
        .route("/api/v1/export", get(export))
}

/// Cookie-authenticated writes must come from our own pages (CSRF guard; SameSite=Lax
/// already blocks most cross-site posts). Bearer-token calls are not affected.
pub fn check_origin(auth: &AuthUser, headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    if !auth.via_cookie {
        return Ok(());
    }
    same_origin(headers, state)
}

pub fn same_origin(headers: &HeaderMap, state: &AppState) -> ApiResult<()> {
    let origin = state.cfg.public_origin();
    let bad = || {
        AppError::new(
            StatusCode::FORBIDDEN,
            "bad_origin",
            "Request came from another site.",
        )
    };
    if let Some(o) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        return if o == origin { Ok(()) } else { Err(bad()) };
    }
    if let Some(r) = headers.get("referer").and_then(|v| v.to_str().ok()) {
        return if r == origin || r.starts_with(&format!("{origin}/")) {
            Ok(())
        } else {
            Err(bad())
        };
    }
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

#[derive(Deserialize)]
pub struct SignupReq {
    pub email: String,
    pub password: String,
    pub handle: String,
}

#[derive(Serialize)]
pub struct TokenResp {
    pub token: String,
    pub user: User,
}

async fn signup(
    State(st): State<AppState>,
    Json(r): Json<SignupReq>,
) -> ApiResult<(StatusCode, Json<TokenResp>)> {
    let user = auth::create_user(&st, &r.email, &r.password, &r.handle).await?;
    let token = auth::issue_token(&st, user.id, "web", None).await?;
    Ok((StatusCode::CREATED, Json(TokenResp { token, user })))
}

#[derive(Deserialize)]
pub struct LoginReq {
    pub email: String,
    pub password: String,
}

async fn login(State(st): State<AppState>, Json(r): Json<LoginReq>) -> ApiResult<Json<TokenResp>> {
    let user = auth::check_login(&st, &r.email, &r.password).await?;
    let token = auth::issue_token(&st, user.id, "web", None).await?;
    Ok(Json(TokenResp { token, user }))
}

async fn logout(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    sqlx::query("UPDATE auth_tokens SET revoked_at = now() WHERE id = $1")
        .bind(a.token_id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn me(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let linked: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('provider', provider, 'provider_uid', provider_uid,
                 'verified', verified, 'last_sync_at', last_sync_at, 'sync_status', sync_status)), '[]'::jsonb)
         FROM linked_accounts WHERE user_id = $1",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(
        json!({ "user": a.user, "token_kind": a.kind, "linked_accounts": linked }),
    ))
}

#[derive(Deserialize)]
struct DeleteMeReq {
    password: String,
}

/// Delete the account and everything attached to it, immediately.
async fn delete_me(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<DeleteMeReq>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    auth::check_login(&st, &a.user.email, &r.password).await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(a.user.id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ device pairing

#[derive(Deserialize)]
pub struct DeviceStartReq {
    #[serde(default)]
    pub device_name: String,
}

#[derive(Serialize, Deserialize)]
pub struct DeviceStartResp {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

async fn device_start(
    State(st): State<AppState>,
    Json(r): Json<DeviceStartReq>,
) -> ApiResult<Json<DeviceStartResp>> {
    let name: String = r
        .device_name
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    let name = if name.is_empty() {
        "Fraglink desktop".to_string()
    } else {
        name
    };
    // Housekeeping: old codes are useless.
    sqlx::query("DELETE FROM device_codes WHERE expires_at < now() - interval '1 day'")
        .execute(&st.db)
        .await?;
    for _ in 0..5 {
        let (device_code, hash) = auth::new_token();
        let user_code = auth::new_user_code();
        let r = sqlx::query(
            "INSERT INTO device_codes(device_code_hash, user_code, device_name, expires_at)
             VALUES($1, $2, $3, now() + make_interval(mins => $4))",
        )
        .bind(hash)
        .bind(&user_code)
        .bind(&name)
        .bind(auth::DEVICE_CODE_MINUTES as i32)
        .execute(&st.db)
        .await;
        match r {
            Ok(_) => {
                let base = &st.cfg.public_url;
                return Ok(Json(DeviceStartResp {
                    verification_uri: format!("{base}/pair"),
                    verification_uri_complete: format!("{base}/pair?code={user_code}"),
                    device_code,
                    user_code,
                    expires_in: auth::DEVICE_CODE_MINUTES * 60,
                    interval: 5,
                }));
            }
            Err(sqlx::Error::Database(e)) if e.is_unique_violation() => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(AppError::internal("could not allocate a pairing code"))
}

#[derive(Deserialize)]
pub struct DevicePollReq {
    pub device_code: String,
}

async fn device_poll(
    State(st): State<AppState>,
    Json(r): Json<DevicePollReq>,
) -> ApiResult<Json<Value>> {
    let hash = auth::hash_token(&r.device_code);
    let mut tx = st.db.begin().await?;
    let row: Option<(String, bool, Option<Uuid>, bool)> = sqlx::query_as(
        "SELECT device_name, expires_at < now(), approved_by, consumed_at IS NOT NULL
         FROM device_codes WHERE device_code_hash = $1 FOR UPDATE",
    )
    .bind(&hash)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((name, expired, approved_by, consumed)) = row else {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "invalid_device_code",
            "Unknown pairing code.",
        ));
    };
    if consumed {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "invalid_device_code",
            "This pairing code was already used.",
        ));
    }
    let Some(user_id) = approved_by else {
        if expired {
            return Err(AppError::new(
                StatusCode::GONE,
                "expired_token",
                "The pairing code expired. Start again.",
            ));
        }
        return Ok(Json(json!({ "status": "pending" })));
    };
    sqlx::query("UPDATE device_codes SET consumed_at = now() WHERE device_code_hash = $1")
        .bind(&hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let token = auth::issue_token(&st, user_id, "device", Some(&name)).await?;
    let user: User =
        sqlx::query_as("SELECT id, email, handle, display_name FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&st.db)
            .await?;
    Ok(Json(
        json!({ "status": "approved", "token": token, "user": user }),
    ))
}

/// Approve a pairing code for the signed-in user. Returns the device name.
pub async fn approve_code(st: &AppState, user_id: Uuid, code: &str) -> ApiResult<String> {
    let code = auth::normalize_user_code(code);
    let name: Option<String> = sqlx::query_scalar(
        "UPDATE device_codes SET approved_by = $2, approved_at = now()
         WHERE user_code = $1 AND expires_at > now() AND approved_by IS NULL AND consumed_at IS NULL
         RETURNING device_name",
    )
    .bind(&code)
    .bind(user_id)
    .fetch_optional(&st.db)
    .await?;
    name.ok_or_else(|| {
        AppError::bad_request(
            "invalid_user_code",
            "That code is wrong or expired. Check the code in the Fraglink app.",
        )
    })
}

#[derive(Deserialize)]
struct ApproveReq {
    user_code: String,
}

async fn device_approve(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ApproveReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    let name = approve_code(&st, a.user.id, &r.user_code).await?;
    Ok(Json(json!({ "approved": true, "device_name": name })))
}

async fn devices(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let v: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_agg(jsonb_build_object('id', id, 'device_name', device_name, 'created_at', created_at,
                 'last_used_at', last_used_at) ORDER BY created_at DESC), '[]'::jsonb)
         FROM auth_tokens WHERE user_id = $1 AND kind = 'device' AND revoked_at IS NULL",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({ "devices": v })))
}

async fn revoke_device(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    let n = sqlx::query("UPDATE auth_tokens SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND kind = 'device' AND revoked_at IS NULL")
        .bind(id)
        .bind(a.user.id)
        .execute(&st.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ session sync

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct UploadResp {
    pub accepted: Vec<String>,
    pub rejected: Vec<Rejected>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Rejected {
    pub id: String,
    pub reason: String,
}

fn ts(secs: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(secs, 0).single()
}

/// Validate one uploaded session (spec §9 anti-abuse rules). `Err(reason)` = reject for good.
pub fn validate_session(
    s: &fraglink_core::outbox::UploadSession,
    now: i64,
) -> Result<Uuid, String> {
    let id = Uuid::parse_str(&s.id).map_err(|_| "id must be a UUID".to_string())?;
    if s.ended_at < s.started_at {
        return Err("ends before it starts".into());
    }
    if s.ended_at - s.started_at > MAX_SESSION_SECS {
        return Err("longer than 24 hours".into());
    }
    if s.ended_at > now + CLOCK_SKEW_SECS {
        return Err("ends in the future".into());
    }
    if s.started_at < 946_684_800 {
        return Err("starts before 2000".into());
    }
    if s.duration_s < 0 || s.idle_trimmed_s < 0 {
        return Err("negative duration".into());
    }
    if s.duration_s > s.ended_at - s.started_at + 5 {
        return Err("active time longer than the session".into());
    }
    Ok(id)
}

async fn upload_sessions(
    State(st): State<AppState>,
    a: AuthUser,
    Json(b): Json<UploadBatch>,
) -> ApiResult<Json<UploadResp>> {
    if b.sessions.len() > MAX_UPLOAD_SESSIONS {
        return Err(AppError::bad_request(
            "too_many",
            format!("Send at most {MAX_UPLOAD_SESSIONS} sessions per request."),
        ));
    }
    let uid = a.user.id;
    let now = Utc::now().timestamp();
    let mut out = UploadResp::default();
    let mut tx = st.db.begin().await?;
    // Games first (names are only set on first sight; clients can't rename shared games).
    let mut game_ids = std::collections::HashMap::new();
    for g in &b.games {
        let Some(sid) = hours::server_game_id(uid, &g.id) else {
            continue;
        };
        let name: String = g
            .name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        let name = if name.is_empty() { g.id.clone() } else { name };
        sqlx::query(
            "INSERT INTO games(id, name, steam_appid) VALUES($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET steam_appid = COALESCE(games.steam_appid, excluded.steam_appid)",
        )
        .bind(&sid)
        .bind(&name)
        .bind(g.steam_appid.map(|x| x as i32))
        .execute(&mut *tx)
        .await?;
        game_ids.insert(g.id.clone(), sid);
    }
    for s in &b.sessions {
        let reject = |reason: &str| Rejected {
            id: s.id.clone(),
            reason: reason.to_string(),
        };
        let id = match validate_session(s, now) {
            Ok(id) => id,
            Err(r) => {
                out.rejected.push(reject(&r));
                continue;
            }
        };
        let Some(gid) = game_ids.get(&s.game_id) else {
            out.rejected
                .push(reject("unknown game (include it in `games`)"));
            continue;
        };
        let (start, end) = (ts(s.started_at).unwrap(), ts(s.ended_at).unwrap());
        let overlap: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM sessions WHERE user_id = $1 AND id <> $2
               AND started_at < $4 - make_interval(secs => $5) AND ended_at > $3 + make_interval(secs => $5) LIMIT 1",
        )
        .bind(uid)
        .bind(id)
        .bind(start)
        .bind(end)
        .bind(OVERLAP_TOLERANCE_SECS as f64)
        .fetch_optional(&mut *tx)
        .await?;
        if overlap.is_some() {
            out.rejected.push(reject("overlaps another session"));
            continue;
        }
        let r = sqlx::query(
            "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, idle_trimmed_s, client_version)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (id) DO UPDATE SET game_id = excluded.game_id, started_at = excluded.started_at,
                 ended_at = excluded.ended_at, duration_s = excluded.duration_s,
                 idle_trimmed_s = excluded.idle_trimmed_s, client_version = excluded.client_version, uploaded_at = now()
             WHERE sessions.user_id = excluded.user_id",
        )
        .bind(id)
        .bind(uid)
        .bind(gid)
        .bind(start)
        .bind(end)
        .bind(s.duration_s.min(i32::MAX as i64) as i32)
        .bind(s.idle_trimmed_s.min(i32::MAX as i64) as i32)
        .bind(b.client_version.chars().take(32).collect::<String>())
        .execute(&mut *tx)
        .await?;
        if r.rows_affected() == 1 {
            out.accepted.push(s.id.clone());
        } else {
            out.rejected.push(reject("id belongs to another account"));
        }
    }
    tx.commit().await?;
    Ok(Json(out))
}

// ------------------------------------------------------------------ stats & ranks

#[derive(Serialize, Deserialize, Debug, sqlx::FromRow)]
pub struct GameStat {
    pub game_id: String,
    pub name: String,
    pub tracked_min: i64,
    pub imported_min: i64,
    pub estimated_min: i64,
    pub display_min: i64,
    pub display_tier: String,
    pub minutes_7d: i64,
    pub last_played_at: Option<DateTime<Utc>>,
}

pub async fn game_stats(st: &AppState, user_id: Uuid) -> ApiResult<Vec<GameStat>> {
    Ok(sqlx::query_as(
        "SELECT game_id, name, tracked_min, imported_min, estimated_min, display_min, display_tier, minutes_7d, last_played_at
         FROM user_game_stats WHERE user_id = $1 ORDER BY display_min DESC, estimated_min DESC, name",
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await?)
}

async fn stats(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let games = game_stats(&st, a.user.id).await?;
    let verified: i64 = games.iter().map(|g| g.display_min).sum();
    let estimated: i64 = games.iter().map(|g| g.estimated_min).sum();
    Ok(Json(json!({
        "games": games,
        "overall": { "verified_min": verified, "estimated_min": estimated,
                     "estimated_display": format!("~{}h", hours::group(hours::round_estimate_hours(estimated / 60))) }
    })))
}

#[derive(Deserialize)]
struct LbQuery {
    #[serde(default)]
    period: Option<String>,
}

/// Ranks use **Tracked only** (sessions + Steam playtime). Imported and Estimated
/// hours never count (spec §5, §9). Profiles aren't public before M4, so this needs
/// a signed-in caller.
async fn leaderboard(
    State(st): State<AppState>,
    _a: AuthUser,
    Path(game_id): Path<String>,
    Query(q): Query<LbQuery>,
) -> ApiResult<Json<Value>> {
    let rows: Vec<(String, i64)> = match q.period.as_deref().unwrap_or("7d") {
        "7d" => {
            sqlx::query_as(
                "SELECT u.handle, (sum(s.duration_s) / 60)::bigint AS m FROM sessions s JOIN users u ON u.id = s.user_id
                 WHERE s.game_id = $1 AND s.started_at >= now() - interval '7 days' AND u.deleted_at IS NULL
                 GROUP BY u.handle HAVING sum(s.duration_s) >= 60 ORDER BY m DESC, u.handle LIMIT 50",
            )
            .bind(&game_id)
            .fetch_all(&st.db)
            .await?
        }
        "all" => {
            sqlx::query_as(
                "SELECT u.handle, g.tracked_min FROM user_game_stats g JOIN users u ON u.id = g.user_id
                 WHERE g.game_id = $1 AND g.tracked_min > 0 AND u.deleted_at IS NULL
                 ORDER BY g.tracked_min DESC, u.handle LIMIT 50",
            )
            .bind(&game_id)
            .fetch_all(&st.db)
            .await?
        }
        _ => return Err(AppError::bad_request("invalid_period", "period is 7d or all")),
    };
    let ranks: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, (h, m))| json!({ "rank": i + 1, "handle": h, "tracked_min": m }))
        .collect();
    Ok(Json(
        json!({ "game_id": game_id, "tier": "tracked", "ranks": ranks }),
    ))
}

// ------------------------------------------------------------------ imported / estimated

fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').chars().take(60).collect()
}

/// Find or create the game for hours entries: a known id (signature or existing row),
/// or a free-text name that becomes the user's own `manual:` game.
async fn ensure_game(
    st: &AppState,
    user_id: Uuid,
    game_id: Option<&str>,
    game_name: Option<&str>,
) -> ApiResult<String> {
    if let Some(id) = game_id.map(str::trim).filter(|s| !s.is_empty()) {
        // Another user's private `manual:` game is off limits.
        if id.starts_with("manual:") && !id.starts_with(&format!("manual:{}:", user_id.simple())) {
            return Err(AppError::bad_request(
                "unknown_game",
                "Unknown game id; send game_name instead.",
            ));
        }
        if let Some(name) = sqlx::query_scalar::<_, String>("SELECT name FROM games WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?
        {
            let _ = name;
            return Ok(id.to_string());
        }
        if let Some(sig) = steam::sigs().games().iter().find(|g| g.id == id) {
            sqlx::query("INSERT INTO games(id, name, steam_appid) VALUES($1, $2, $3) ON CONFLICT (id) DO NOTHING")
                .bind(&sig.id)
                .bind(&sig.name)
                .bind(sig.steam_appids.first().map(|x| *x as i32))
                .execute(&st.db)
                .await?;
            return Ok(sig.id.clone());
        }
        return Err(AppError::bad_request(
            "unknown_game",
            "Unknown game id; send game_name instead.",
        ));
    }
    let name: String = game_name
        .unwrap_or("")
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let sl = slug(&name);
    if sl.is_empty() {
        return Err(AppError::bad_request(
            "missing_game",
            "Pick a game or type its name.",
        ));
    }
    let id = hours::server_game_id(user_id, &format!("manual:{sl}")).unwrap();
    sqlx::query("INSERT INTO games(id, name) VALUES($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(&id)
        .bind(&name)
        .execute(&st.db)
        .await?;
    Ok(id)
}

fn clean_note(n: Option<String>) -> ApiResult<Option<String>> {
    let n = n.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if n.as_ref().is_some_and(|s| s.chars().count() > 140) {
        return Err(AppError::bad_request(
            "note_too_long",
            "Notes are at most 140 characters.",
        ));
    }
    Ok(n)
}

#[derive(Deserialize)]
pub struct EstimateReq {
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub eras: Vec<Era>,
    pub note: Option<String>,
}

/// History calculator (Estimated tier). One estimate per game; saving again replaces it.
async fn save_estimate(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<EstimateReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    Ok(Json(estimate_for(&st, a.user.id, r).await?))
}

pub async fn estimate_for(st: &AppState, user_id: Uuid, r: EstimateReq) -> ApiResult<Value> {
    let est = hours::estimate(&r.eras, hours::current_year())?;
    let note = clean_note(r.note)?;
    let gid = ensure_game(st, user_id, r.game_id.as_deref(), r.game_name.as_deref()).await?;
    let id = Uuid::new_v4();
    let mut tx = st.db.begin().await?;
    sqlx::query(
        "DELETE FROM hours_entries WHERE user_id = $1 AND game_id = $2 AND tier = 'estimated'",
    )
    .bind(user_id)
    .bind(&gid)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, inputs, note)
         VALUES($1, $2, $3, 'estimated', 'estimator', $4, $5, $6)",
    )
    .bind(id)
    .bind(user_id)
    .bind(&gid)
    .bind(est.minutes as i32)
    .bind(json!({ "eras": r.eras }))
    .bind(note)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(
        json!({ "id": id, "game_id": gid, "tier": "estimated", "minutes": est.minutes, "display": est.display, "warnings": est.warnings }),
    )
}

#[derive(Deserialize)]
pub struct ImportReq {
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub minutes: i64,
    #[serde(default = "default_import_source")]
    pub source: String,
    pub note: Option<String>,
}

fn default_import_source() -> String {
    "manual".into()
}

/// A game's own counter typed in by the user (Imported tier, shown as "self-reported").
async fn save_import(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ImportReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    Ok(Json(import_for(&st, a.user.id, r).await?))
}

pub async fn import_for(st: &AppState, user_id: Uuid, r: ImportReq) -> ApiResult<Value> {
    if !["manual", "wow_manual", "xbox_recap"].contains(&r.source.as_str()) {
        return Err(AppError::bad_request(
            "invalid_source",
            "source is manual, wow_manual or xbox_recap",
        ));
    }
    // 40 years of 24/7 play is the outer bound of plausible.
    if !(1..=40 * 365 * 24 * 60).contains(&r.minutes) {
        return Err(AppError::bad_request(
            "invalid_minutes",
            "That number of minutes isn't plausible.",
        ));
    }
    let note = clean_note(r.note)?;
    let gid = ensure_game(st, user_id, r.game_id.as_deref(), r.game_name.as_deref()).await?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, note)
         VALUES($1, $2, $3, 'imported', $4, $5, $6)",
    )
    .bind(id)
    .bind(user_id)
    .bind(&gid)
    .bind(&r.source)
    .bind(r.minutes as i32)
    .bind(note)
    .execute(&st.db)
    .await?;
    Ok(
        json!({ "id": id, "game_id": gid, "tier": "imported", "minutes": r.minutes, "self_reported": true }),
    )
}

async fn delete_hours(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    let n = sqlx::query("DELETE FROM hours_entries WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(a.user.id)
        .execute(&st.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ Steam

#[derive(Deserialize)]
struct SteamLinkReq {
    profile: String,
}

/// Link by SteamID64 / profile URL / custom URL. Unverified until the user signs in
/// through Steam (`/auth/steam/start`), but import works either way for public profiles.
async fn steam_link(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<SteamLinkReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    let id = match steam::parse_profile(&r.profile) {
        Some(ProfileRef::Id(id)) => id,
        Some(ProfileRef::Vanity(v)) => st.steam.resolve_vanity(&v).await?,
        None => {
            return Err(AppError::bad_request(
                "invalid_steam_profile",
                "Paste your SteamID64 or steamcommunity.com profile link.",
            ))
        }
    };
    steam::link(&st, a.user.id, id, false).await?;
    Ok(Json(
        json!({ "steamid": id.to_string(), "verified": false }),
    ))
}

async fn steam_sync(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<Json<steam::ImportResult>> {
    check_origin(&a, &headers, &st)?;
    Ok(Json(steam::import(&st, a.user.id).await?))
}

// ------------------------------------------------------------------ export

pub async fn export_json(st: &AppState, user: &User) -> ApiResult<Value> {
    let q = |sql: &'static str| {
        sqlx::query_scalar::<_, Value>(sql)
            .bind(user.id)
            .fetch_one(&st.db)
    };
    Ok(json!({
        "format": "fraglink-server-export",
        "schema": 1,
        "exported_at": Utc::now(),
        "user": user,
        "linked_accounts": q("SELECT coalesce(jsonb_agg(to_jsonb(l) - 'user_id'), '[]'::jsonb) FROM linked_accounts l WHERE user_id = $1").await?,
        "sessions": q("SELECT coalesce(jsonb_agg(to_jsonb(s) - 'user_id' ORDER BY started_at), '[]'::jsonb) FROM sessions s WHERE user_id = $1").await?,
        "steam_playtime": q("SELECT coalesce(jsonb_agg(to_jsonb(p) - 'user_id'), '[]'::jsonb) FROM steam_playtime p WHERE user_id = $1").await?,
        "hours_entries": q("SELECT coalesce(jsonb_agg(to_jsonb(h) - 'user_id' ORDER BY captured_at), '[]'::jsonb) FROM hours_entries h WHERE user_id = $1").await?,
        "game_stats": q("SELECT coalesce(jsonb_agg(to_jsonb(g) - 'user_id'), '[]'::jsonb) FROM user_game_stats g WHERE user_id = $1").await?,
        "devices": q("SELECT coalesce(jsonb_agg(jsonb_build_object('device_name', device_name, 'created_at', created_at, 'last_used_at', last_used_at, 'revoked_at', revoked_at)), '[]'::jsonb) FROM auth_tokens WHERE user_id = $1 AND kind = 'device'").await?,
    }))
}

async fn export(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    Ok(Json(export_json(&st, &a.user).await?))
}
