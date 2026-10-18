//! Optional two-step sign-in: TOTP (RFC 6238, SHA-1, 6 digits, 30 s) plus ten one-time
//! backup codes (only SHA-256 stored). With it on, a correct password returns
//! `{"mfa_required": true, "mfa_token": ...}` instead of a session; the second call
//! (`POST /api/v1/auth/mfa`) trades the ticket and a code for the real token.

use crate::auth::{self, AuthUser, User};
use crate::error::{ApiResult, AppError};
use crate::geo::ReqCtx;
use crate::{api, mailer, AppState};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const ISSUER: &str = "Fraglink";
pub const BACKUP_CODES: usize = 10;
const TICKET_MINUTES: i64 = 5;
const TICKET_ATTEMPTS: i32 = 5;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/mfa", post(second_step))
        .route("/api/v1/me/mfa/setup", post(setup))
        .route("/api/v1/me/mfa/enable", post(enable))
        .route("/api/v1/me/mfa/disable", post(disable))
        .route("/api/v1/me/mfa/backup_codes", post(regenerate))
}

// ------------------------------------------------------------------ TOTP primitives

const B32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buf = (buf << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            out.push(B32[((buf >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(B32[((buf << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn totp_at(secret: &[u8], step: u64) -> u32 {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret).expect("any key length");
    mac.update(&step.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let o = (h[19] & 0x0f) as usize;
    let v = ((h[o] as u32 & 0x7f) << 24)
        | ((h[o + 1] as u32) << 16)
        | ((h[o + 2] as u32) << 8)
        | (h[o + 3] as u32);
    v % 1_000_000
}

pub fn current_step() -> u64 {
    (chrono::Utc::now().timestamp() as u64) / 30
}

/// Matching step within ±1 step (clock drift), newer than `last_step`.
pub fn check_totp(secret: &[u8], code: &str, last_step: i64) -> Option<u64> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        return None;
    }
    let want: u32 = code.parse().ok()?;
    let now = current_step();
    [now.saturating_sub(1), now, now + 1]
        .into_iter()
        .find(|&s| s as i64 > last_step && totp_at(secret, s) == want)
}

fn norm_backup(code: &str) -> String {
    code.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn hash_backup(code: &str) -> Vec<u8> {
    Sha256::digest(norm_backup(code).as_bytes()).to_vec()
}

fn new_backup_code() -> String {
    const A: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut r = rand::thread_rng();
    let s: String = (0..10)
        .map(|_| A[(r.next_u32() as usize) % A.len()] as char)
        .collect();
    format!("{}-{}", &s[..5], &s[5..])
}

// ------------------------------------------------------------------ state helpers

pub async fn enabled(st: &AppState, user: Uuid) -> ApiResult<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_mfa WHERE user_id = $1 AND enabled_at IS NOT NULL)",
    )
    .bind(user)
    .fetch_one(&st.db)
    .await?)
}

pub async fn status(st: &AppState, user: Uuid) -> ApiResult<Value> {
    let on = enabled(st, user).await?;
    let left: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM mfa_backup_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user)
    .fetch_one(&st.db)
    .await?;
    Ok(json!({ "enabled": on, "backup_codes_left": left }))
}

/// Check a TOTP or backup code for an account with MFA on. Consumes backup codes and
/// records the TOTP step so a code can't be replayed.
pub async fn verify_code(st: &AppState, user: Uuid, code: &str) -> ApiResult<bool> {
    let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT secret, last_step FROM user_mfa WHERE user_id = $1 AND enabled_at IS NOT NULL",
    )
    .bind(user)
    .fetch_optional(&st.db)
    .await?;
    let Some((secret, last)) = row else {
        return Ok(false);
    };
    if let Some(step) = check_totp(&secret, code, last) {
        let n =
            sqlx::query("UPDATE user_mfa SET last_step = $2 WHERE user_id = $1 AND last_step < $2")
                .bind(user)
                .bind(step as i64)
                .execute(&st.db)
                .await?
                .rows_affected();
        return Ok(n == 1);
    }
    if norm_backup(code).len() == 10 {
        let n = sqlx::query(
            "UPDATE mfa_backup_codes SET used_at = now()
             WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
        )
        .bind(user)
        .bind(hash_backup(code))
        .execute(&st.db)
        .await?
        .rows_affected();
        return Ok(n == 1);
    }
    Ok(false)
}

fn code_limit(st: &AppState, user: Uuid) -> ApiResult<()> {
    st.rate_limit(
        format!("mfa:{user}"),
        10,
        std::time::Duration::from_secs(15 * 60),
    )
}

fn bad_code() -> AppError {
    AppError::new(
        StatusCode::UNAUTHORIZED,
        "bad_code",
        "That code isn't right. Try the newest code from your authenticator app, or a backup code.",
    )
}

// ------------------------------------------------------------------ sign-in

/// After a correct password: cancel a pending deletion, then either issue the token or,
/// with MFA on, a short-lived ticket for the second step.
pub async fn finish_login(
    st: &AppState,
    user: User,
    kind: &str,
    device_name: Option<&str>,
    ctx: &ReqCtx,
) -> ApiResult<Value> {
    let cancelled = sqlx::query(
        "UPDATE users SET delete_after = NULL WHERE id = $1 AND delete_after IS NOT NULL",
    )
    .bind(user.id)
    .execute(&st.db)
    .await?
    .rows_affected()
        > 0;
    if enabled(st, user.id).await? {
        let (ticket, hash) = auth::new_token();
        sqlx::query("DELETE FROM mfa_tickets WHERE expires_at < now()")
            .execute(&st.db)
            .await?;
        sqlx::query(
            "INSERT INTO mfa_tickets(ticket_hash, user_id, kind, device_name, ip, user_agent, expires_at)
             VALUES($1, $2, $3, $4, $5, $6, now() + make_interval(mins => $7))",
        )
        .bind(hash)
        .bind(user.id)
        .bind(kind)
        .bind(device_name)
        .bind(&ctx.ip)
        .bind(&ctx.user_agent)
        .bind(TICKET_MINUTES as i32)
        .execute(&st.db)
        .await?;
        return Ok(
            json!({ "mfa_required": true, "mfa_token": ticket, "deletion_cancelled": cancelled }),
        );
    }
    let token = auth::issue_token_ctx(st, user.id, kind, device_name, ctx).await?;
    new_signin_notice(st, &user, ctx).await;
    Ok(json!({ "token": token, "user": user, "deletion_cancelled": cancelled }))
}

/// "New sign-in from Windows · Chrome, Australia" email (logged while mail is off).
pub async fn new_signin_notice(st: &AppState, user: &User, ctx: &ReqCtx) {
    if user.email.is_empty() {
        return;
    }
    let want: bool = sqlx::query_scalar(
        "SELECT coalesce((notify_prefs->>'signin_email')::boolean, true) FROM users WHERE id = $1",
    )
    .bind(user.id)
    .fetch_one(&st.db)
    .await
    .unwrap_or(true);
    if !want {
        return;
    }
    let place = ctx
        .location
        .as_deref()
        .map(|l| format!(", {l}"))
        .unwrap_or_default();
    let what = format!("{}{place}", ctx.device());
    mailer::send_email(
        st,
        &user.email,
        &crate::emails::signin(
            &user.display_name,
            &what,
            &format!("{}/settings#security", st.cfg.site_url),
        ),
    );
}

#[derive(Deserialize)]
struct SecondStepReq {
    mfa_token: String,
    code: String,
}

async fn second_step(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<SecondStepReq>,
) -> ApiResult<Json<Value>> {
    api::login_limit(&st, &headers)?;
    let hash = auth::hash_token(r.mfa_token.trim());
    let row: Option<(
        Uuid,
        String,
        Option<String>,
        i32,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "UPDATE mfa_tickets SET attempts = attempts + 1
             WHERE ticket_hash = $1 AND expires_at > now()
             RETURNING user_id, kind, device_name, attempts, ip, user_agent",
    )
    .bind(&hash)
    .fetch_optional(&st.db)
    .await?;
    let Some((uid, kind, device, attempts, ip, ua)) = row else {
        return Err(AppError::new(
            StatusCode::UNAUTHORIZED,
            "mfa_expired",
            "That sign-in took too long. Enter your password again.",
        ));
    };
    if attempts > TICKET_ATTEMPTS {
        sqlx::query("DELETE FROM mfa_tickets WHERE ticket_hash = $1")
            .bind(&hash)
            .execute(&st.db)
            .await?;
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "mfa_expired",
            "Too many wrong codes. Enter your password again.",
        ));
    }
    code_limit(&st, uid)?;
    if !verify_code(&st, uid, &r.code).await? {
        return Err(bad_code());
    }
    sqlx::query("DELETE FROM mfa_tickets WHERE ticket_hash = $1")
        .bind(&hash)
        .execute(&st.db)
        .await?;
    let user: User = sqlx::query_as(
        "SELECT id, email, handle, display_name FROM users
         WHERE id = $1 AND deleted_at IS NULL AND banned_at IS NULL",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?
    .ok_or_else(AppError::unauthorized)?;
    let mut ctx = ReqCtx::from_headers(&st, &headers);
    if ctx.ip.is_none() {
        ctx.ip = ip;
    }
    if ctx.user_agent.is_none() {
        ctx.user_agent = ua;
    }
    let token = auth::issue_token_ctx(&st, uid, &kind, device.as_deref(), &ctx).await?;
    new_signin_notice(&st, &user, &ctx).await;
    Ok(Json(json!({ "token": token, "user": user })))
}

// ------------------------------------------------------------------ settings

#[derive(Deserialize)]
struct PasswordReq {
    password: String,
}

async fn setup(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<PasswordReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(
        format!("mfasetup:{}", a.user.id),
        10,
        std::time::Duration::from_secs(900),
    )?;
    api::check_current_password(&st, a.user.id, &r.password).await?;
    if enabled(&st, a.user.id).await? {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "mfa_on",
            "Two-step sign-in is already on.",
        ));
    }
    let mut secret = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut secret);
    sqlx::query(
        "INSERT INTO user_mfa(user_id, secret) VALUES($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET secret = EXCLUDED.secret, enabled_at = NULL, last_step = 0, created_at = now()",
    )
    .bind(a.user.id)
    .bind(secret.to_vec())
    .execute(&st.db)
    .await?;
    let key = base32(&secret);
    let label =
        url::form_urlencoded::byte_serialize(format!("{ISSUER}:{}", a.user.handle).as_bytes())
            .collect::<String>()
            .replace('+', "%20");
    let uri = format!(
        "otpauth://totp/{label}?secret={key}&issuer={ISSUER}&algorithm=SHA1&digits=6&period=30"
    );
    let qr = qrcode::QrCode::new(uri.as_bytes())
        .map_err(|e| AppError::internal(anyhow::anyhow!("qr: {e}")))?
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(180, 180)
        .quiet_zone(true)
        .dark_color(qrcode::render::svg::Color("#000000"))
        .light_color(qrcode::render::svg::Color("#ffffff"))
        .build();
    let grouped = key
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Json(
        json!({ "secret": grouped, "otpauth_uri": uri, "qr_svg": qr }),
    ))
}

#[derive(Deserialize)]
struct CodeReq {
    code: String,
}

async fn new_backup_codes(st: &AppState, user: Uuid) -> ApiResult<Vec<String>> {
    let codes: Vec<String> = (0..BACKUP_CODES).map(|_| new_backup_code()).collect();
    let mut tx = st.db.begin().await?;
    sqlx::query("DELETE FROM mfa_backup_codes WHERE user_id = $1")
        .bind(user)
        .execute(&mut *tx)
        .await?;
    for c in &codes {
        sqlx::query("INSERT INTO mfa_backup_codes(user_id, code_hash) VALUES($1, $2)")
            .bind(user)
            .bind(hash_backup(c))
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(codes)
}

async fn enable(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<CodeReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    code_limit(&st, a.user.id)?;
    let row: Option<(Vec<u8>, Option<chrono::DateTime<chrono::Utc>>)> =
        sqlx::query_as("SELECT secret, enabled_at FROM user_mfa WHERE user_id = $1")
            .bind(a.user.id)
            .fetch_optional(&st.db)
            .await?;
    let Some((secret, on)) = row else {
        return Err(AppError::bad_request(
            "mfa_not_setup",
            "Start the setup first.",
        ));
    };
    if on.is_some() {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "mfa_on",
            "Two-step sign-in is already on.",
        ));
    }
    let Some(step) = check_totp(&secret, &r.code, 0) else {
        return Err(bad_code());
    };
    sqlx::query("UPDATE user_mfa SET enabled_at = now(), last_step = $2 WHERE user_id = $1")
        .bind(a.user.id)
        .bind(step as i64)
        .execute(&st.db)
        .await?;
    let codes = new_backup_codes(&st, a.user.id).await?;
    tracing::info!(user = %a.user.id, "two-step sign-in enabled");
    Ok(Json(json!({ "enabled": true, "backup_codes": codes })))
}

#[derive(Deserialize)]
struct PasswordCodeReq {
    password: String,
    code: String,
}

async fn disable(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<PasswordCodeReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    code_limit(&st, a.user.id)?;
    api::check_current_password(&st, a.user.id, &r.password).await?;
    if !verify_code(&st, a.user.id, &r.code).await? {
        return Err(bad_code());
    }
    sqlx::query("DELETE FROM user_mfa WHERE user_id = $1")
        .bind(a.user.id)
        .execute(&st.db)
        .await?;
    sqlx::query("DELETE FROM mfa_backup_codes WHERE user_id = $1")
        .bind(a.user.id)
        .execute(&st.db)
        .await?;
    tracing::info!(user = %a.user.id, "two-step sign-in disabled");
    Ok(Json(json!({ "enabled": false })))
}

async fn regenerate(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<PasswordCodeReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    code_limit(&st, a.user.id)?;
    api::check_current_password(&st, a.user.id, &r.password).await?;
    if !verify_code(&st, a.user.id, &r.code).await? {
        return Err(bad_code());
    }
    let codes = new_backup_codes(&st, a.user.id).await?;
    Ok(Json(json!({ "backup_codes": codes })))
}

/// Staff reset (admin section): removes MFA so the user can sign in with the password.
pub async fn reset(st: &AppState, user: Uuid) -> ApiResult<bool> {
    let n = sqlx::query("DELETE FROM user_mfa WHERE user_id = $1")
        .bind(user)
        .execute(&st.db)
        .await?
        .rows_affected();
    sqlx::query("DELETE FROM mfa_backup_codes WHERE user_id = $1")
        .bind(user)
        .execute(&st.db)
        .await?;
    sqlx::query("DELETE FROM mfa_tickets WHERE user_id = $1")
        .bind(user)
        .execute(&st.db)
        .await?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vectors() {
        // RFC 6238 appendix B, SHA-1, secret "12345678901234567890", 8 digits -> last 6.
        let s = b"12345678901234567890";
        assert_eq!(totp_at(s, 59 / 30), 287082);
        assert_eq!(totp_at(s, 1111111109 / 30), 81804);
        assert_eq!(totp_at(s, 1234567890 / 30), 5924);
    }

    #[test]
    fn base32_encoding() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(
            base32(b"12345678901234567890"),
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        );
    }

    #[test]
    fn backup_codes_normalise() {
        let c = new_backup_code();
        assert_eq!(c.len(), 11);
        assert_eq!(
            hash_backup(&c),
            hash_backup(&c.to_uppercase().replace('-', " "))
        );
    }
}
