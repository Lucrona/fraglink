//! Passwords (Argon2id), opaque bearer tokens (only SHA-256 stored), the
//! `AuthUser` extractor (Bearer header or `fl_session` cookie) and the desktop
//! pairing codes.

use crate::error::{ApiResult, AppError};
use crate::AppState;
use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const SESSION_COOKIE: &str = "fl_session";
pub const WEB_TOKEN_DAYS: i64 = 30;
pub const DEVICE_CODE_MINUTES: i64 = 10;
const MAX_FAILED_LOGINS: usize = 10;
const FAILED_LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

/// A fixed hash so unknown-email logins cost the same as wrong-password logins.
pub fn dummy_hash() -> &'static str {
    static H: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    H.get_or_init(|| hash_password("not-a-real-password-just-timing").unwrap())
}

/// 256-bit random token, URL-safe base64. Returns (token, sha256(token)).
pub fn new_token() -> (String, Vec<u8>) {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    let t = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b);
    let h = hash_token(&t);
    (t, h)
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

/// Short code the user types/confirms in the browser: `XXXX-XXXX` (no 0/O/1/I/vowels).
pub fn new_user_code() -> String {
    const ALPHA: &[u8] = b"BCDFGHJKLMNPQRSTVWXZ23456789";
    let mut rng = rand::thread_rng();
    let mut s = String::with_capacity(9);
    for i in 0..8 {
        if i == 4 {
            s.push('-');
        }
        s.push(ALPHA[(rng.next_u32() as usize) % ALPHA.len()] as char);
    }
    s
}

pub fn normalize_user_code(s: &str) -> String {
    let c: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if c.len() == 8 {
        format!("{}-{}", &c[..4], &c[4..])
    } else {
        c
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub handle: String,
    pub display_name: String,
}

/// Authenticated caller (from `Authorization: Bearer <token>` or the session cookie).
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user: User,
    pub token_id: Uuid,
    pub kind: String,
    /// True when the token came from the cookie (browser forms need an Origin check).
    pub via_cookie: bool,
}

pub fn bearer_or_cookie(parts: &Parts) -> Option<(String, bool)> {
    if let Some(v) = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(t) = v.strip_prefix("Bearer ") {
            return Some((t.trim().to_string(), false));
        }
    }
    cookie_value(&parts.headers, SESSION_COOKIE).map(|t| (t, true))
}

pub fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

pub async fn user_for_token(
    state: &AppState,
    token: &str,
) -> ApiResult<Option<(User, Uuid, String)>> {
    let row: Option<(Uuid, String, Uuid, String, String, String)> = sqlx::query_as(
        "SELECT t.id, t.kind, u.id, u.email, u.handle, u.display_name
         FROM auth_tokens t JOIN users u ON u.id = t.user_id
         WHERE t.token_hash = $1 AND t.revoked_at IS NULL
           AND (t.expires_at IS NULL OR t.expires_at > now()) AND u.deleted_at IS NULL",
    )
    .bind(hash_token(token))
    .fetch_optional(&state.db)
    .await?;
    let Some((tid, kind, id, email, handle, display_name)) = row else {
        return Ok(None);
    };
    sqlx::query(
        "UPDATE auth_tokens SET last_used_at = now()
         WHERE id = $1 AND last_used_at < now() - interval '5 minutes'",
    )
    .bind(tid)
    .execute(&state.db)
    .await?;
    Ok(Some((
        User {
            id,
            email,
            handle,
            display_name,
        },
        tid,
        kind,
    )))
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let (token, via_cookie) = bearer_or_cookie(parts).ok_or_else(AppError::unauthorized)?;
        let (user, token_id, kind) = user_for_token(state, &token)
            .await?
            .ok_or_else(AppError::unauthorized)?;
        Ok(AuthUser {
            user,
            token_id,
            kind,
            via_cookie,
        })
    }
}

/// Optional variant for pages that work signed in or out.
pub struct MaybeUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for MaybeUser {
    type Rejection = AppError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(u) => Ok(MaybeUser(Some(u))),
            Err(e) if e.status == StatusCode::UNAUTHORIZED => Ok(MaybeUser(None)),
            Err(e) => Err(e),
        }
    }
}

/// Issue a token for `user_id`. Web tokens expire after 30 days; device tokens
/// live until revoked (Settings → Devices, or signing out in the tray).
pub async fn issue_token(
    state: &AppState,
    user_id: Uuid,
    kind: &str,
    device_name: Option<&str>,
) -> ApiResult<String> {
    let (token, hash) = new_token();
    let expires: Option<chrono::DateTime<chrono::Utc>> =
        (kind == "web").then(|| chrono::Utc::now() + chrono::Duration::days(WEB_TOKEN_DAYS));
    sqlx::query(
        "INSERT INTO auth_tokens(id, user_id, token_hash, kind, device_name, expires_at)
         VALUES($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(hash)
    .bind(kind)
    .bind(device_name)
    .bind(expires)
    .execute(&state.db)
    .await?;
    Ok(token)
}

pub fn validate_signup(email: &str, password: &str, handle: &str) -> Result<(), AppError> {
    let email = email.trim();
    let at = email.find('@');
    if !email.is_empty()
        && (email.len() > 254
            || at.is_none_or(|i| i == 0 || i == email.len() - 1)
            || email.contains(char::is_whitespace))
    {
        return Err(AppError::bad_request(
            "invalid_email",
            "Enter a valid email address (or leave it empty).",
        ));
    }
    let n = password.chars().count();
    if !(10..=128).contains(&n) {
        return Err(AppError::bad_request(
            "weak_password",
            "Use a password of at least 10 characters.",
        ));
    }
    let h = handle.trim();
    if !(3..=20).contains(&h.len())
        || !h
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::bad_request(
            "invalid_handle",
            "Handles are 3-20 characters: letters, digits, _ or -.",
        ));
    }
    Ok(())
}

pub async fn create_user(
    state: &AppState,
    email: &str,
    password: &str,
    handle: &str,
) -> ApiResult<User> {
    validate_signup(email, password, handle)?;
    let hash = hash_password(password)?;
    let user = User {
        id: Uuid::new_v4(),
        email: email.trim().to_string(),
        handle: handle.trim().to_string(),
        display_name: handle.trim().to_string(),
    };
    let r = sqlx::query(
        "INSERT INTO users(id, email, handle, display_name, password_hash) VALUES($1, $2, $3, $4, $5)",
    )
    .bind(user.id)
    .bind(&user.email)
    .bind(&user.handle)
    .bind(&user.display_name)
    .bind(hash)
    .execute(&state.db)
    .await;
    match r {
        Ok(_) => Ok(user),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(AppError::new(
            StatusCode::CONFLICT,
            "taken",
            "That email or handle is already registered.",
        )),
        Err(e) => Err(e.into()),
    }
}

fn too_many_failures(state: &AppState, key: &str) -> bool {
    let mut m = state.login_failures.lock().unwrap();
    let v = m.entry(key.to_string()).or_default();
    v.retain(|t| t.elapsed() < FAILED_LOGIN_WINDOW);
    v.len() >= MAX_FAILED_LOGINS
}

fn record_failure(state: &AppState, key: &str) {
    state
        .login_failures
        .lock()
        .unwrap()
        .entry(key.to_string())
        .or_default()
        .push(Instant::now());
}

/// Email-or-username + password check with a per-account failure limit (10 per 15 min).
pub async fn check_login(state: &AppState, email: &str, password: &str) -> ApiResult<User> {
    let key = email.trim().to_lowercase();
    if too_many_failures(state, &key) {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "Too many failed sign-ins. Try again in 15 minutes.",
        ));
    }
    let row: Option<(Uuid, String, String, String, String)> = sqlx::query_as(
        "SELECT id, email, handle, display_name, password_hash FROM users
         WHERE ((lower(email) = $1 AND email <> '') OR lower(handle) = $1) AND deleted_at IS NULL
         ORDER BY (lower(handle) = $1) DESC LIMIT 1",
    )
    .bind(&key)
    .fetch_optional(&state.db)
    .await?;
    let pw = password.to_string();
    let (user, hash) = match row {
        Some((id, email, handle, display_name, hash)) => (
            Some(User {
                id,
                email,
                handle,
                display_name,
            }),
            hash,
        ),
        None => (None, dummy_hash().to_string()),
    };
    // Argon2 is CPU-heavy: keep it off the async workers.
    let ok = tokio::task::spawn_blocking(move || verify_password(&pw, &hash))
        .await
        .map_err(AppError::internal)?;
    match (user, ok) {
        (Some(u), true) => Ok(u),
        _ => {
            record_failure(state, &key);
            Err(AppError::new(
                StatusCode::UNAUTHORIZED,
                "bad_credentials",
                "Wrong username or password.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &h));
        assert!(!verify_password("wrong", &h));
        assert!(!verify_password("x", "not a hash"));
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let (a, ha) = new_token();
        let (b, _) = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(ha, hash_token(&a));
        assert_eq!(ha.len(), 32);
    }

    #[test]
    fn user_codes() {
        let c = new_user_code();
        assert_eq!(c.len(), 9);
        assert_eq!(&c[4..5], "-");
        assert_eq!(normalize_user_code(" bcdf ghjk "), "BCDF-GHJK");
        assert_eq!(normalize_user_code("bcdf-ghjk"), "BCDF-GHJK");
    }

    #[test]
    fn signup_validation() {
        assert!(validate_signup("a@b.co", "0123456789", "Lucrona").is_ok());
        assert_eq!(
            validate_signup("nope", "0123456789", "abc")
                .unwrap_err()
                .code,
            "invalid_email"
        );
        assert_eq!(
            validate_signup("a@b.co", "short", "abc").unwrap_err().code,
            "weak_password"
        );
        assert_eq!(
            validate_signup("a@b.co", "0123456789", "a b")
                .unwrap_err()
                .code,
            "invalid_handle"
        );
        assert_eq!(
            validate_signup("a@b.co", "0123456789", "ab")
                .unwrap_err()
                .code,
            "invalid_handle"
        );
    }
}
