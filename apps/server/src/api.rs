//! JSON API (`/api/v1/...`). Auth: `Authorization: Bearer <token>`.

use crate::auth::{self, AuthUser, User};
use crate::error::{ApiResult, AppError};
use crate::friends;
use crate::hours::{self, Era};
use crate::steam::{self, ProfileRef};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fraglink_core::outbox::UploadBatch;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const MAX_UPLOAD_SESSIONS: usize = 500;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/auth/signup", post(signup))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/me", get(me).delete(delete_me))
        .route("/api/v1/me/password", post(change_password))
        .route("/api/v1/me/email", post(change_email))
        .route("/api/v1/me/privacy", post(set_privacy))
        .route("/api/v1/device/start", post(device_start))
        .route("/api/v1/device/signup", post(device_signup))
        .route("/api/v1/device/login", post(device_login))
        .route("/api/v1/users/search", get(user_search))
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
        .route("/api/v1/export.zip", get(export_zip))
        .route("/api/v1/friends", get(friends_list))
        .route("/api/v1/friends/request", post(friend_request))
        .route("/api/v1/friends/respond", post(friend_respond))
        .route("/api/v1/friends/activity", get(friends_activity))
        .route("/api/v1/friends/{handle}", delete(friend_remove))
        .route("/api/v1/friends/{handle}/summary", get(friend_summary))
        .route("/api/v1/blocks", post(block_user))
        .route("/api/v1/blocks/{handle}", delete(unblock_user))
        .route("/api/v1/ws", get(crate::presence::ws_handler))
        .route("/api/v1/games/icons", get(game_icons))
        .route("/icons/games/{size}/{file}", get(crate::icons::serve))
        .route("/api/v1/games/detectable", get(crate::games::detectable))
        .route("/api/v1/games/overrides", get(crate::games::overrides))
        .route("/api/v1/games/suggest", post(crate::games::suggest))
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
    headers: HeaderMap,
    Json(r): Json<SignupReq>,
) -> ApiResult<(StatusCode, Json<TokenResp>)> {
    signup_limit(&st, &headers)?;
    let user = auth::create_user(&st, &r.email, &r.password, &r.handle).await?;
    let token = auth::issue_token(&st, user.id, "web", None).await?;
    Ok((StatusCode::CREATED, Json(TokenResp { token, user })))
}

#[derive(Deserialize)]
pub struct LoginReq {
    /// Email or username.
    #[serde(alias = "login", alias = "username", alias = "handle")]
    pub email: String,
    pub password: String,
}

async fn login(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<LoginReq>,
) -> ApiResult<Json<TokenResp>> {
    login_limit(&st, &headers)?;
    let user = auth::check_login(&st, &r.email, &r.password).await?;
    let token = auth::issue_token(&st, user.id, "web", None).await?;
    Ok(Json(TokenResp { token, user }))
}

const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);

/// 5 new accounts per IP per hour.
pub fn signup_limit(st: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let ip = crate::client_ip(headers);
    if ip == "local" {
        return Ok(()); // direct localhost calls (tests, admin); public traffic comes via Caddy
    }
    st.rate_limit(format!("signup:{ip}"), 5, 60 * MINUTE)
}

/// 30 sign-in attempts per IP per 15 minutes (on top of the per-account failure limit).
pub fn login_limit(st: &AppState, headers: &HeaderMap) -> ApiResult<()> {
    let ip = crate::client_ip(headers);
    if ip == "local" {
        return Ok(()); // direct localhost calls (tests, admin); public traffic comes via Caddy
    }
    st.rate_limit(format!("login:{ip}"), 30, 15 * MINUTE)
}

fn device_name(n: Option<String>) -> String {
    let n: String = n.unwrap_or_default().trim().chars().take(64).collect();
    if n.is_empty() {
        "Fraglink desktop".into()
    } else {
        n
    }
}

#[derive(Deserialize)]
struct DeviceSignupReq {
    handle: String,
    password: String,
    #[serde(default)]
    email: String,
    device_name: Option<String>,
}

/// Desktop client "Create Account": makes the account and returns a device token.
async fn device_signup(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<DeviceSignupReq>,
) -> ApiResult<(StatusCode, Json<TokenResp>)> {
    signup_limit(&st, &headers)?;
    let user = auth::create_user(&st, &r.email, &r.password, &r.handle).await?;
    let token =
        auth::issue_token(&st, user.id, "device", Some(&device_name(r.device_name))).await?;
    Ok((StatusCode::CREATED, Json(TokenResp { token, user })))
}

#[derive(Deserialize)]
struct DeviceLoginReq {
    #[serde(alias = "username", alias = "handle", alias = "email")]
    login: String,
    password: String,
    device_name: Option<String>,
}

/// Desktop client sign-in with username (or email) and password; returns a device token.
async fn device_login(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<DeviceLoginReq>,
) -> ApiResult<Json<TokenResp>> {
    login_limit(&st, &headers)?;
    let user = auth::check_login(&st, &r.login, &r.password).await?;
    let token =
        auth::issue_token(&st, user.id, "device", Some(&device_name(r.device_name))).await?;
    Ok(Json(TokenResp { token, user }))
}

#[derive(Deserialize)]
struct SearchQ {
    q: String,
}

/// Add Friend search: usernames that start with (or equal) the text, up to 20.
async fn user_search(
    State(st): State<AppState>,
    a: AuthUser,
    Query(q): Query<SearchQ>,
) -> ApiResult<Json<Value>> {
    st.rate_limit(format!("search:{}", a.user.id), 60, MINUTE)?;
    let q = q.q.trim().to_lowercase();
    if q.is_empty() || q.len() > 64 {
        return Ok(Json(json!({ "users": [] })));
    }
    let like = format!(
        "{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT u.handle, u.display_name FROM users u
         WHERE u.deleted_at IS NULL AND u.id <> $1
           AND (lower(u.handle) LIKE $2 OR lower(u.display_name) LIKE $2 OR (u.email <> '' AND lower(u.email) = $3))
           AND NOT EXISTS (SELECT 1 FROM blocks b WHERE (b.blocker = u.id AND b.blocked = $1) OR (b.blocker = $1 AND b.blocked = u.id))
         ORDER BY (lower(u.handle) = $3) DESC, lower(u.handle) LIMIT 20",
    )
    .bind(a.user.id)
    .bind(&like)
    .bind(&q)
    .fetch_all(&st.db)
    .await?;
    let users: Vec<Value> = rows
        .into_iter()
        .map(|(h, d)| json!({ "handle": h, "display_name": d }))
        .collect();
    Ok(Json(json!({ "users": users })))
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
    let (verified, share): (bool, bool) =
        sqlx::query_as("SELECT email_verified, share_activity FROM users WHERE id = $1")
            .bind(a.user.id)
            .fetch_one(&st.db)
            .await?;
    let mut user = json!(a.user);
    user["email_verified"] = json!(verified);
    user["share_activity"] = json!(share);
    Ok(Json(
        json!({ "user": user, "token_kind": a.kind, "linked_accounts": linked }),
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
    auth::check_login(&st, &a.user.handle, &r.password).await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(a.user.id)
        .execute(&st.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ account management

/// The caller's current password, checked against the stored Argon2id hash.
async fn check_current_password(st: &AppState, user_id: Uuid, password: &str) -> ApiResult<()> {
    let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_one(&st.db)
        .await?;
    let pw = password.to_string();
    let ok = tokio::task::spawn_blocking(move || auth::verify_password(&pw, &hash))
        .await
        .unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(AppError::new(
            StatusCode::FORBIDDEN,
            "wrong_password",
            "Your current password isn't right.",
        ))
    }
}

#[derive(Deserialize)]
struct ChangePasswordReq {
    current_password: String,
    new_password: String,
}

/// Change the password (needs the current one). Every other sign-in (other PCs, browser
/// sessions) is signed out; the token making this call stays valid.
async fn change_password(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ChangePasswordReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("pwchange:{}", a.user.id), 5, 15 * MINUTE)?;
    check_current_password(&st, a.user.id, &r.current_password).await?;
    let n = r.new_password.chars().count();
    if !(10..=128).contains(&n) {
        return Err(AppError::bad_request(
            "weak_password",
            "Use a new password of at least 10 characters.",
        ));
    }
    if r.new_password == r.current_password {
        return Err(AppError::bad_request(
            "same_password",
            "The new password is the same as the current one.",
        ));
    }
    let pw = r.new_password.clone();
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&pw))
        .await
        .map_err(|e| anyhow::anyhow!("hash task: {e}"))??;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(hash)
        .bind(a.user.id)
        .execute(&mut *tx)
        .await?;
    let revoked = sqlx::query(
        "UPDATE auth_tokens SET revoked_at = now()
         WHERE user_id = $1 AND id <> $2 AND revoked_at IS NULL",
    )
    .bind(a.user.id)
    .bind(a.token_id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    tx.commit().await?;
    tracing::info!(user = %a.user.id, revoked, "password changed");
    Ok(Json(
        json!({ "ok": true, "signed_out_other_sessions": revoked }),
    ))
}

#[derive(Deserialize)]
struct ChangeEmailReq {
    current_password: String,
    new_email: String,
}

/// Change the email (needs the current password). Stored unverified: there's no email
/// provider yet to send a confirmation link.
async fn change_email(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ChangeEmailReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("emailchange:{}", a.user.id), 5, 15 * MINUTE)?;
    check_current_password(&st, a.user.id, &r.current_password).await?;
    let email = r.new_email.trim().to_string();
    if email.is_empty() || auth::validate_email(&email).is_err() {
        return Err(AppError::bad_request(
            "invalid_email",
            "Enter a valid email address.",
        ));
    }
    if email == a.user.email {
        return Err(AppError::bad_request(
            "same_email",
            "That's already your email address.",
        ));
    }
    let res = sqlx::query("UPDATE users SET email = $1, email_verified = false WHERE id = $2")
        .bind(&email)
        .bind(a.user.id)
        .execute(&st.db)
        .await;
    match res {
        Ok(_) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "taken",
                "That email is already used by another account.",
            ))
        }
        Err(e) => return Err(e.into()),
    }
    tracing::info!(user = %a.user.id, "email changed");
    Ok(Json(
        json!({ "ok": true, "email": email, "email_verified": false }),
    ))
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

/// Session uploads from apps before 0.2.0-dev.14. Hours now come only from live
/// presence (see `presence::set`), so uploads are acknowledged (the app clears its
/// queue and stops retrying) and dropped: nothing a client claims is stored.
async fn upload_sessions(
    State(_st): State<AppState>,
    _a: AuthUser,
    Json(b): Json<UploadBatch>,
) -> ApiResult<Json<UploadResp>> {
    if b.sessions.len() > MAX_UPLOAD_SESSIONS {
        return Err(AppError::bad_request(
            "too_many",
            format!("Send at most {MAX_UPLOAD_SESSIONS} sessions per request."),
        ));
    }
    Ok(Json(UploadResp {
        accepted: b.sessions.iter().map(|s| s.id.clone()).collect(),
        rejected: vec![],
    }))
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

/// Per-game totals with the server's names (unknown games: "Unlisted game").
pub async fn game_stats(st: &AppState, user_id: Uuid) -> ApiResult<Vec<GameStat>> {
    let mut rows: Vec<GameStat> = sqlx::query_as(
        "SELECT game_id, name, tracked_min, imported_min, estimated_min, display_min, display_tier, minutes_7d, last_played_at
         FROM user_game_stats WHERE user_id = $1 ORDER BY display_min DESC, estimated_min DESC, name",
    )
    .bind(user_id)
    .fetch_all(&st.db)
    .await?;
    let ids: Vec<String> = rows.iter().map(|r| r.game_id.clone()).collect();
    let names = crate::names::public_names(st, &ids).await?;
    for r in &mut rows {
        r.name = names
            .get(&r.game_id)
            .cloned()
            .unwrap_or_else(|| crate::names::UNLISTED_TITLE.to_string());
    }
    Ok(rows)
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

/// Resolve the game for hours entries: an id on our lists (or an existing row), or a
/// free-text name that exactly matches a listed game. Users can't create or name games.
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
        if crate::names::list_name(st, id).is_some() && steam::sigs().get(id).is_none() {
            crate::names::ensure_game_row(st, id).await?;
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
    // Free text only picks a game from the server's lists: nobody names (or renames)
    // games, so there are no private `manual:` games any more (dev.14).
    let name = game_name.unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err(AppError::bad_request(
            "missing_game",
            "Pick a game or type its name.",
        ));
    }
    match known_game_by_name(st, &name) {
        Some(id) => {
            crate::names::ensure_game_row(st, &id).await?;
            Ok(id)
        }
        None => Err(AppError::bad_request(
            "unknown_game",
            "That game isn't on Fraglink's list yet. Pick it from the list (exact name).",
        )),
    }
}

/// A game id from the server's lists by its exact (case-insensitive) name.
pub fn known_game_by_name(st: &AppState, name: &str) -> Option<String> {
    let n = name.trim().to_lowercase();
    if let Some(g) = steam::sigs()
        .games()
        .iter()
        .find(|g| g.name.to_lowercase() == n)
    {
        return Some(g.id.clone());
    }
    let b = st.games.current()?;
    let mut hits: Vec<&String> = b
        .names
        .iter()
        .filter(|(_, v)| v.to_lowercase() == n)
        .map(|(k, _)| k)
        .collect();
    hits.sort();
    hits.first().map(|s| s.to_string())
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
        "friends": crate::friends::export(st, user.id).await?,
    }))
}

async fn export(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    Ok(Json(export_json(&st, &a.user).await?))
}

/// Seconds between zip exports per user (spec W14: at most one per hour).
pub const EXPORT_ZIP_INTERVAL_SECS: u64 = 3600;

const ZIP_README: &str = "Fraglink account export\n\n\
profile.json         your account, linked accounts and paired devices\n\
sessions.csv         every session uploaded by the Fraglink app (tier: tracked)\n\
games.csv            per-game minutes by tier: tracked, imported (self-reported), estimated, and what is displayed\n\
hours_entries.json   imported counters and history estimates, with the estimator inputs\n\
steam_playtime.csv   Steam's own playtime counters (tracked tier)\n\
friends.json         friends, pending requests and blocks\n\
fraglink-account.json  everything above in one JSON document\n\n\
Times are UTC (RFC 3339). Minutes are whole minutes; seconds where the column says _s.\n\
Estimated hours are never added to totals or rankings.\n";

fn csv_line(fields: &[String]) -> String {
    let mut l = fields
        .iter()
        .map(|f| fraglink_core::export::csv_field(f))
        .collect::<Vec<_>>()
        .join(",");
    l.push('\n');
    l
}

async fn export_zip(
    State(st): State<AppState>,
    a: AuthUser,
) -> ApiResult<axum::response::Response> {
    use axum::response::IntoResponse;
    {
        let mut log = st.export_log.lock().unwrap();
        if let Some(t) = log.get(&a.user.id) {
            let age = t.elapsed().as_secs();
            if age < EXPORT_ZIP_INTERVAL_SECS {
                return Err(AppError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "export_cooldown",
                    format!("One zip export per hour. Try again in {} min (the JSON export has no limit).", (EXPORT_ZIP_INTERVAL_SECS - age) / 60 + 1),
                ));
            }
        }
        log.insert(a.user.id, std::time::Instant::now());
    }
    let uid = a.user.id;
    let full = export_json(&st, &a.user).await?;

    let rows: Vec<(Uuid, String, String, DateTime<Utc>, DateTime<Utc>, i32, i32, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT s.id, s.game_id, g.name, s.started_at, s.ended_at, s.duration_s, s.idle_trimmed_s, s.client_version, s.uploaded_at
         FROM sessions s JOIN games g ON g.id = s.game_id WHERE s.user_id = $1 ORDER BY s.started_at",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await?;
    let mut sessions = String::from("session_id,game_id,game_name,started_at_utc,ended_at_utc,active_seconds,idle_trimmed_seconds,tier,client_version,uploaded_at_utc\n");
    for (id, gid, name, sa, ea, d, idle, cv, up) in rows {
        sessions += &csv_line(&[
            id.to_string(),
            gid,
            name,
            sa.to_rfc3339(),
            ea.to_rfc3339(),
            d.to_string(),
            idle.to_string(),
            "tracked".into(),
            cv.unwrap_or_default(),
            up.to_rfc3339(),
        ]);
    }
    let mut games = String::from("game_id,game_name,tracked_minutes,imported_minutes,estimated_minutes,display_minutes,display_tier,minutes_last_7_days,last_played_utc\n");
    for g in game_stats(&st, uid).await? {
        games += &csv_line(&[
            g.game_id,
            g.name,
            g.tracked_min.to_string(),
            g.imported_min.to_string(),
            g.estimated_min.to_string(),
            g.display_min.to_string(),
            g.display_tier,
            g.minutes_7d.to_string(),
            g.last_played_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
        ]);
    }
    let sp: Vec<(i32, String, i32, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at FROM steam_playtime WHERE user_id = $1 ORDER BY steam_appid",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await?;
    let mut steam_csv =
        String::from("steam_appid,game_id,minutes_forever,minutes_2weeks,fetched_at_utc\n");
    for (app, gid, f, w, at) in sp {
        steam_csv += &csv_line(&[
            app.to_string(),
            gid,
            f.to_string(),
            w.to_string(),
            at.to_rfc3339(),
        ]);
    }
    let profile = json!({ "user": full["user"], "linked_accounts": full["linked_accounts"], "devices": full["devices"] });
    let pretty = |v: &Value| serde_json::to_vec_pretty(v).unwrap_or_default();
    let bytes = fraglink_core::export::zip_bytes(&[
        ("README.txt", ZIP_README.as_bytes().to_vec()),
        ("profile.json", pretty(&profile)),
        ("sessions.csv", sessions.into_bytes()),
        ("games.csv", games.into_bytes()),
        ("hours_entries.json", pretty(&full["hours_entries"])),
        ("steam_playtime.csv", steam_csv.into_bytes()),
        ("friends.json", pretty(&full["friends"])),
        ("fraglink-account.json", pretty(&full)),
    ])
    .map_err(AppError::internal)?;
    let fname = format!(
        "fraglink-{}-{}.zip",
        a.user.handle,
        Utc::now().format("%Y%m%d")
    );
    Ok((
        [
            (
                axum::http::header::CONTENT_TYPE,
                "application/zip".to_string(),
            ),
            (
                axum::http::header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{fname}\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

// ------------------------------------------------------------------ friends (M3)

pub fn person(a: &AuthUser) -> friends::Person {
    friends::Person {
        id: a.user.id,
        handle: a.user.handle.clone(),
        display_name: a.user.display_name.clone(),
    }
}

#[derive(Deserialize)]
pub struct HandleReq {
    pub handle: String,
}

#[derive(Deserialize)]
pub struct RespondReq {
    pub handle: String,
    pub accept: bool,
}

async fn friends_list(
    State(st): State<AppState>,
    a: AuthUser,
) -> ApiResult<Json<friends::FriendsList>> {
    Ok(Json(friends::list(&st, a.user.id).await?))
}

async fn friend_request(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<HandleReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("friendreq:{}", a.user.id), 30, 60 * MINUTE)?;
    let outcome = friends::request(&st, &person(&a), &r.handle).await?;
    Ok(Json(json!({ "status": outcome })))
}

async fn friend_respond(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<RespondReq>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    friends::respond(&st, &person(&a), &r.handle, r.accept).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn friends_activity(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    st.rate_limit(format!("activity:{}", a.user.id), 120, 10 * MINUTE)?;
    let items = crate::activity::recent(&st, a.user.id).await?;
    Ok(Json(
        json!({ "items": items, "days": crate::activity::ACTIVITY_DAYS }),
    ))
}

async fn friend_summary(
    State(st): State<AppState>,
    a: AuthUser,
    Path(handle): Path<String>,
) -> ApiResult<Json<crate::activity::FriendSummary>> {
    st.rate_limit(format!("summary:{}", a.user.id), 240, 10 * MINUTE)?;
    Ok(Json(
        crate::activity::summary(&st, a.user.id, &handle).await?,
    ))
}

#[derive(Deserialize)]
struct PrivacyReq {
    share_activity: bool,
}

/// Whether friends can see your recent play history and weekly hours.
async fn set_privacy(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<PrivacyReq>,
) -> ApiResult<Json<Value>> {
    check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("privacy:{}", a.user.id), 30, 15 * MINUTE)?;
    sqlx::query("UPDATE users SET share_activity = $2 WHERE id = $1")
        .bind(a.user.id)
        .bind(r.share_activity)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "share_activity": r.share_activity })))
}

async fn friend_remove(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(handle): Path<String>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    friends::remove(&st, &person(&a), &handle).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn block_user(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<HandleReq>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    friends::block(&st, &person(&a), &r.handle).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unblock_user(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(handle): Path<String>,
) -> ApiResult<StatusCode> {
    check_origin(&a, &headers, &st)?;
    friends::unblock(&st, &person(&a), &handle).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------------ game icons

#[derive(Deserialize)]
struct IconsQ {
    ids: String,
}

/// Where a client can get a game's icon:
/// `{ "<game id>": { "icon_path": "/icons/games/32/<id>.png", "preferred": "server",
///   "steam_appid": 570 | null, "icon_url": null } }` for games this server has (or
/// can fetch) an official icon for; other ids are left out and the app uses the
/// exe icon on its own PC. `icon_url` stays null: apps never download icons from
/// anyone but this server (older apps used it for a CDN link).
async fn game_icons(
    State(st): State<AppState>,
    _a: AuthUser,
    Query(q): Query<IconsQ>,
) -> ApiResult<Json<Value>> {
    let ids: Vec<String> = q
        .ids
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s.len() <= 120 && !s.starts_with("manual:"))
        .take(50)
        .collect();
    let mut out = serde_json::Map::new();
    for id in &ids {
        let Some(cid) = crate::names::canonical_id(Uuid::nil(), id) else {
            continue;
        };
        if !crate::icons::available(&st, &cid).await {
            continue;
        }
        let appid = fraglink_core::icons::appid_from_game_id(&cid).or_else(|| {
            st.sigs
                .get(&cid)
                .and_then(|g| g.steam_appids.first().copied())
        });
        out.insert(
            id.clone(),
            json!({
                "icon_path": crate::icons::icon_path(&cid, 32),
                "preferred": "server",
                "steam_appid": appid,
                "icon_url": null,
            }),
        );
    }
    Ok(Json(Value::Object(out)))
}
