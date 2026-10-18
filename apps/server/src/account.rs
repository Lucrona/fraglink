//! Account pages of the website v2: email verification, profile fields and gaming rig,
//! signed-in sessions (Security tab), played games (hide / reset hours), notification
//! preferences, and purging accounts whose 7-day deletion grace period is over.

use crate::auth::{self, AuthUser, User};
use crate::error::{ApiResult, AppError};
use crate::{api, avatars, mailer, mfa, AppState};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

const MINUTE: std::time::Duration = std::time::Duration::from_secs(60);
pub const RIG_FIELDS: &[&str] = &[
    "manufacturer",
    "cpu",
    "gpu",
    "memory",
    "storage",
    "motherboard",
    "monitor",
    "headset",
    "keyboard",
    "mouse",
    "mousepad",
    "os",
    "case",
];
pub const GAMING_STYLES: &[&str] = &[
    "Casual",
    "Weekend warrior",
    "Competitive",
    "Completionist",
    "MMO addict",
    "Sniper only",
    "Support main",
    "Speedrunner",
    "Explorer",
    "Clueless",
];

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/auth/verify", post(verify_email))
        .route("/api/v1/me/verify/resend", post(resend))
        .route("/api/v1/auth/forgot", post(forgot))
        .route("/api/v1/auth/reset", post(reset_password))
        .route("/api/v1/me/profile", post(save_profile))
        .route("/api/v1/me/sessions", get(sessions))
        .route("/api/v1/me/sessions/{id}", delete(revoke_session))
        .route("/api/v1/me/sessions/revoke_all", post(revoke_all))
        .route("/api/v1/me/games", get(my_games))
        .route("/api/v1/me/games/hide", post(hide_game))
        .route("/api/v1/me/games/reset", post(reset_game))
        .route("/api/v1/me/notifications", post(save_notifications))
        .route("/api/v1/me/mfa", get(mfa_status))
}

// ------------------------------------------------------------------ /me extras

pub async fn me_extra(st: &AppState, uid: Uuid) -> ApiResult<Value> {
    let (
        kind,
        hash,
        preset,
        about,
        location,
        style,
        rig,
        public,
        show_friends,
        prefs,
        delete_after,
        created,
        privacy,
    ): (
        Option<String>,
        Option<String>,
        Option<i16>,
        Option<String>,
        Option<String>,
        Option<String>,
        Value,
        bool,
        bool,
        Value,
        Option<DateTime<Utc>>,
        DateTime<Utc>,
        Value,
    ) = sqlx::query_as(
        "SELECT avatar_kind, avatar_hash, avatar_preset, about, location, gaming_style, rig,
                public_profile, show_friends, notify_prefs, delete_after, created_at, privacy
         FROM users WHERE id = $1",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    Ok(json!({
        "avatar": avatars::avatar_json(uid, kind.as_deref(), hash.as_deref(), preset),
        "about": about, "location": location, "gaming_style": style, "rig": rig,
        "public_profile": public, "show_friends": show_friends,
        "privacy": privacy_view(&privacy),
        "site_url": st.cfg.site_url.trim_end_matches('/'),
        "notify_prefs": notify_defaults(&prefs),
        "delete_after": delete_after, "created_at": created,
        "mfa": mfa::status(st, uid).await?,
        "details": details_json(st, uid).await?,
        "mail_live": mailer::live(st),
        "avatar_upload_allowed": avatars::upload_block_reason(st, uid).await?.is_none(),
        "avatar_upload_block": avatars::upload_block_reason(st, uid).await?,
    }))
}

fn notify_defaults(p: &Value) -> Value {
    let b = |k: &str, d: bool| p.get(k).and_then(Value::as_bool).unwrap_or(d);
    json!({
        "signin_email": b("signin_email", true),
        "friend_request_email": b("friend_request_email", true),
        "news_email": b("news_email", false),
        "friend_request_client": b("friend_request_client", true),
        "friend_online_client": b("friend_online_client", true),
    })
}

// ------------------------------------------------------------------ email verification

pub async fn send_verification(st: &AppState, user: &User) -> ApiResult<()> {
    if user.email.trim().is_empty() {
        return Ok(());
    }
    let (token, hash) = auth::new_token();
    sqlx::query(
        "INSERT INTO email_tokens(token_hash, user_id, purpose, email, expires_at)
         VALUES($1, $2, 'verify', $3, now() + interval '3 days')",
    )
    .bind(hash)
    .bind(user.id)
    .bind(&user.email)
    .execute(&st.db)
    .await?;
    mailer::send_email(
        st,
        &user.email,
        &crate::emails::confirm(
            &user.display_name,
            &format!("{}/verify?token={token}", st.cfg.site_url),
        ),
    );
    Ok(())
}

#[derive(Deserialize)]
struct TokenReq {
    token: String,
}

async fn verify_email(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<TokenReq>,
) -> ApiResult<Json<Value>> {
    api::login_limit(&st, &headers)?;
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "UPDATE email_tokens SET used_at = now()
         WHERE token_hash = $1 AND purpose = 'verify' AND used_at IS NULL AND expires_at > now()
         RETURNING user_id, email",
    )
    .bind(auth::hash_token(r.token.trim()))
    .fetch_optional(&st.db)
    .await?;
    let Some((uid, email)) = row else {
        return Err(AppError::bad_request(
            "bad_token",
            "That link has expired or was already used. Send a new one from Settings.",
        ));
    };
    // Only if the account still has that address.
    let n = sqlx::query(
        "UPDATE users SET email_verified = true WHERE id = $1 AND lower(email) = lower($2)",
    )
    .bind(uid)
    .bind(&email)
    .execute(&st.db)
    .await?
    .rows_affected();
    Ok(Json(json!({ "verified": n == 1 })))
}

async fn resend(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("verify:{}", a.user.id), 3, 60 * MINUTE)?;
    let verified: bool = sqlx::query_scalar("SELECT email_verified FROM users WHERE id = $1")
        .bind(a.user.id)
        .fetch_one(&st.db)
        .await?;
    if !verified {
        send_verification(&st, &a.user).await?;
    }
    Ok(Json(
        json!({ "sent": !verified && !a.user.email.is_empty(), "mail_live": mailer::live(&st) }),
    ))
}

// ------------------------------------------------------------------ forgot password

#[derive(Deserialize)]
struct ForgotReq {
    email: String,
    #[serde(default)]
    turnstile: Option<String>,
}

/// Always answers the same (no account enumeration). Sends a reset link to accounts with
/// that email; while mail is off the link is only logged.
async fn forgot(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ForgotReq>,
) -> ApiResult<Json<Value>> {
    api::login_limit(&st, &headers)?;
    crate::turnstile::check(&st, &headers, r.turnstile.as_deref()).await?;
    let email = r.email.trim().to_lowercase();
    st.rate_limit(format!("forgot:{email}"), 3, 60 * MINUTE)?;
    let user: Option<User> = sqlx::query_as(
        "SELECT id, email, handle, display_name FROM users
         WHERE lower(email) = $1 AND email <> '' AND deleted_at IS NULL AND banned_at IS NULL",
    )
    .bind(&email)
    .fetch_optional(&st.db)
    .await?;
    if let Some(u) = user {
        let (token, hash) = auth::new_token();
        sqlx::query(
            "INSERT INTO email_tokens(token_hash, user_id, purpose, email, expires_at)
             VALUES($1, $2, 'reset', $3, now() + interval '1 hour')",
        )
        .bind(hash)
        .bind(u.id)
        .bind(&u.email)
        .execute(&st.db)
        .await?;
        mailer::send_email(
            &st,
            &u.email,
            &crate::emails::reset(
                &u.display_name,
                &format!("{}/reset?token={token}", st.cfg.site_url),
            ),
        );
    }
    Ok(Json(json!({ "ok": true, "mail_live": mailer::live(&st) })))
}

#[derive(Deserialize)]
struct ResetReq2 {
    token: String,
    password: String,
}

async fn reset_password(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ResetReq2>,
) -> ApiResult<Json<Value>> {
    api::login_limit(&st, &headers)?;
    let n = r.password.chars().count();
    if !(10..=128).contains(&n) {
        return Err(AppError::bad_request(
            "weak_password",
            "Use a password of at least 10 characters.",
        ));
    }
    let uid: Option<Uuid> = sqlx::query_scalar(
        "UPDATE email_tokens SET used_at = now()
         WHERE token_hash = $1 AND purpose = 'reset' AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(auth::hash_token(r.token.trim()))
    .fetch_optional(&st.db)
    .await?;
    let Some(uid) = uid else {
        return Err(AppError::bad_request(
            "bad_token",
            "That reset link has expired or was already used.",
        ));
    };
    let pw = r.password.clone();
    let hash = tokio::task::spawn_blocking(move || auth::hash_password(&pw))
        .await
        .map_err(AppError::internal)??;
    sqlx::query("UPDATE users SET password_hash = $2 WHERE id = $1")
        .bind(uid)
        .bind(hash)
        .execute(&st.db)
        .await?;
    sqlx::query(
        "UPDATE auth_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(uid)
    .execute(&st.db)
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ profile fields

#[derive(Deserialize)]
struct ProfileReq {
    about: Option<String>,
    location: Option<String>,
    gaming_style: Option<String>,
    rig: Option<Map<String, Value>>,
    // Absent = unchanged; "" or null = cleared.
    real_name: Option<Value>,
    gender: Option<Value>,
    birth_date: Option<Value>,
    country: Option<Value>,
}

fn clean(s: Option<String>, max: usize) -> Option<String> {
    s.map(|v| {
        v.chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(max)
            .collect::<String>()
            .trim()
            .to_string()
    })
    .filter(|v| !v.is_empty())
}

async fn save_profile(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ProfileReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("profile-save:{}", a.user.id), 30, 15 * MINUTE)?;
    let style = clean(r.gaming_style, 30);
    if let Some(s) = &style {
        if !GAMING_STYLES.contains(&s.as_str()) {
            return Err(AppError::bad_request(
                "invalid_style",
                "Pick a gaming style from the list.",
            ));
        }
    }
    let mut rig = Map::new();
    if let Some(m) = r.rig {
        for k in RIG_FIELDS {
            if let Some(v) = clean(m.get(*k).and_then(Value::as_str).map(String::from), 60) {
                rig.insert(k.to_string(), json!(v));
            }
        }
    }
    sqlx::query(
        "UPDATE users SET about = $2, location = $3, gaming_style = $4, rig = $5 WHERE id = $1",
    )
    .bind(a.user.id)
    .bind(clean(r.about, 300))
    .bind(clean(r.location, 40))
    .bind(&style)
    .bind(Value::Object(rig))
    .execute(&st.db)
    .await?;
    let txt = |v: &Value| v.as_str().map(String::from);
    if let Some(v) = &r.real_name {
        sqlx::query("UPDATE users SET real_name = $2 WHERE id = $1")
            .bind(a.user.id)
            .bind(clean(txt(v), 40))
            .execute(&st.db)
            .await?;
    }
    if let Some(v) = &r.gender {
        sqlx::query("UPDATE users SET gender = $2 WHERE id = $1")
            .bind(a.user.id)
            .bind(clean(txt(v), 20))
            .execute(&st.db)
            .await?;
    }
    if let Some(v) = &r.country {
        let c = clean(txt(v), 2).map(|c| c.to_ascii_uppercase());
        if c.as_ref()
            .is_some_and(|c| c.len() != 2 || !c.chars().all(|x| x.is_ascii_uppercase()))
        {
            return Err(AppError::bad_request(
                "invalid_country",
                "Pick a country from the list.",
            ));
        }
        sqlx::query("UPDATE users SET country = $2 WHERE id = $1")
            .bind(a.user.id)
            .bind(c)
            .execute(&st.db)
            .await?;
    }
    if let Some(v) = &r.birth_date {
        let d = match clean(txt(v), 10) {
            None => None,
            Some(s) => {
                let d = chrono::NaiveDate::parse_from_str(&s, "%Y-%m-%d").map_err(|_| {
                    AppError::bad_request("invalid_birth_date", "Birthday must be a date.")
                })?;
                let age = (Utc::now().date_naive() - d).num_days() / 365;
                if !(13..=120).contains(&age) {
                    return Err(AppError::bad_request(
                        "invalid_birth_date",
                        "You must be at least 13 to give a birthday.",
                    ));
                }
                Some(d)
            }
        };
        sqlx::query("UPDATE users SET birth_date = $2 WHERE id = $1")
            .bind(a.user.id)
            .bind(d)
            .execute(&st.db)
            .await?;
    }
    me_extra(&st, a.user.id).await.map(Json)
}

// ------------------------------------------------------------------ sessions

fn mask_ip(ip: &str) -> String {
    if let Ok(std::net::IpAddr::V4(v)) = ip.parse::<std::net::IpAddr>() {
        let o = v.octets();
        return format!("{}.{}.x.x", o[0], o[1]);
    }
    ip.split(':').take(3).collect::<Vec<_>>().join(":") + ":…"
}

async fn sessions(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let rows: Vec<(Uuid, String, Option<String>, DateTime<Utc>, DateTime<Utc>, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, kind, device_name, created_at, last_used_at, ip, user_agent, location FROM auth_tokens
             WHERE user_id = $1 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
             ORDER BY last_used_at DESC LIMIT 100",
        )
        .bind(a.user.id)
        .fetch_all(&st.db)
        .await?;
    let list: Vec<Value> = rows
        .into_iter()
        .map(|(id, kind, name, created, used, ip, ua, loc)| {
            let device = if kind == "device" {
                name.unwrap_or_else(|| "Fraglink desktop".into())
            } else {
                crate::geo::describe_ua(ua.as_deref().unwrap_or(""))
            };
            json!({
                "id": id, "kind": kind, "device": device, "created_at": created, "last_used_at": used,
                "ip": ip.as_deref().map(mask_ip), "location": loc, "current": id == a.token_id,
            })
        })
        .collect();
    Ok(Json(json!({ "sessions": list })))
}

async fn revoke_session(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    api::check_origin(&a, &headers, &st)?;
    let n = sqlx::query(
        "UPDATE auth_tokens SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
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

#[derive(Deserialize, Default)]
struct RevokeAllReq {
    #[serde(default)]
    include_current: bool,
}

/// "Log out of all devices": every web session and desktop app, except this one unless asked.
async fn revoke_all(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    body: Option<Json<RevokeAllReq>>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let all = body.map(|b| b.0.include_current).unwrap_or(false);
    let n = sqlx::query(
        "UPDATE auth_tokens SET revoked_at = now()
         WHERE user_id = $1 AND revoked_at IS NULL AND ($3 OR id <> $2)",
    )
    .bind(a.user.id)
    .bind(a.token_id)
    .bind(all)
    .execute(&st.db)
    .await?
    .rows_affected();
    Ok(Json(json!({ "revoked": n })))
}

// ------------------------------------------------------------------ played games

pub async fn hidden_games(st: &AppState, uid: Uuid) -> ApiResult<Vec<String>> {
    Ok(
        sqlx::query_scalar("SELECT game_id FROM hidden_games WHERE user_id = $1")
            .bind(uid)
            .fetch_all(&st.db)
            .await?,
    )
}

async fn my_games(
    State(st): State<AppState>,
    a: AuthUser,
    axum::extract::Query(pq): axum::extract::Query<crate::paging::PageQuery>,
) -> ApiResult<Json<Value>> {
    let rows = api::game_stats(&st, a.user.id).await?;
    let hidden = hidden_games(&st, a.user.id).await?;
    let ids: Vec<String> = rows.iter().map(|r| r.game_id.clone()).collect();
    let named = crate::names::public_names(&st, &ids).await?;
    let games: Vec<Value> = rows
        .into_iter()
        .filter(|g| g.display_min > 0 || g.minutes_7d > 0 || g.estimated_min > 0)
        .map(|g| {
            json!({
                "game_id": g.game_id, "name": named.get(&g.game_id).cloned().unwrap_or(g.name),
                "total_min": g.display_min, "minutes_7d": g.minutes_7d, "last_played_at": g.last_played_at,
                "hidden": hidden.contains(&g.game_id),
            })
        })
        .collect();
    // Paging is opt-in (limit given); without it the whole list comes back as before.
    if pq.limit.is_none() && pq.q.is_none() {
        return Ok(Json(json!({ "games": games })));
    }
    let all = games.len();
    let hidden_n = games.iter().filter(|g| g["hidden"] == true).count();
    let mut games: Vec<Value> = games
        .into_iter()
        .filter(|g| match pq.show.as_deref() {
            Some("hidden") => g["hidden"] == true,
            Some("shown") => g["hidden"] != true,
            _ => true,
        })
        .collect();
    crate::paging::sort_games(&mut games, pq.sort.as_deref());
    let (games, total) =
        crate::paging::page(games, &pq.term(), &["name"], pq.offset(), pq.limit(50));
    Ok(Json(
        json!({ "games": games, "total": total, "all": all, "hidden": hidden_n, "offset": pq.offset() }),
    ))
}

#[derive(Deserialize)]
struct HideReq {
    game_id: String,
    hidden: bool,
}

async fn hide_game(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<HideReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let id: String = r.game_id.chars().take(120).collect();
    if r.hidden {
        sqlx::query(
            "INSERT INTO hidden_games(user_id, game_id) VALUES($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(a.user.id)
        .bind(&id)
        .execute(&st.db)
        .await?;
    } else {
        sqlx::query("DELETE FROM hidden_games WHERE user_id = $1 AND game_id = $2")
            .bind(a.user.id)
            .bind(&id)
            .execute(&st.db)
            .await?;
    }
    Ok(Json(json!({ "game_id": id, "hidden": r.hidden })))
}

#[derive(Deserialize)]
struct ResetReq {
    game_id: String,
    password: String,
}

/// Delete every hour recorded for one game (sessions, imports, estimates, Steam playtime).
async fn reset_game(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ResetReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(format!("reset:{}", a.user.id), 10, 60 * MINUTE)?;
    api::check_current_password(&st, a.user.id, &r.password).await?;
    let mut tx = st.db.begin().await?;
    let mut n = 0;
    for q in [
        "DELETE FROM sessions WHERE user_id = $1 AND game_id = $2",
        "DELETE FROM hours_entries WHERE user_id = $1 AND game_id = $2",
        "DELETE FROM steam_playtime WHERE user_id = $1 AND game_id = $2",
    ] {
        n += sqlx::query(q)
            .bind(a.user.id)
            .bind(&r.game_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    }
    tx.commit().await?;
    tracing::info!(user = %a.user.id, game = %r.game_id, rows = n, "hours reset by user");
    Ok(Json(json!({ "game_id": r.game_id, "deleted_rows": n })))
}

// ------------------------------------------------------------------ notifications

async fn save_notifications(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<Map<String, Value>>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let mut m = Map::new();
    for k in [
        "signin_email",
        "friend_request_email",
        "news_email",
        "friend_request_client",
        "friend_online_client",
    ] {
        if let Some(b) = r.get(k).and_then(Value::as_bool) {
            m.insert(k.into(), json!(b));
        }
    }
    let v: Value = sqlx::query_scalar(
        "UPDATE users SET notify_prefs = notify_prefs || $2 WHERE id = $1 RETURNING notify_prefs",
    )
    .bind(a.user.id)
    .bind(Value::Object(m))
    .fetch_one(&st.db)
    .await?;
    Ok(Json(notify_defaults(&v)))
}

async fn mfa_status(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    mfa::status(&st, a.user.id).await.map(Json)
}

// ------------------------------------------------------------------ deletion purge

/// Delete accounts whose grace period ended (and their avatar files). Returns how many.
pub async fn purge_due(st: &AppState) -> ApiResult<u64> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "DELETE FROM users WHERE delete_after IS NOT NULL AND delete_after < now() RETURNING id",
    )
    .fetch_all(&st.db)
    .await?;
    for id in &ids {
        avatars::remove_files(st, *id);
        tracing::info!(user = %id, "account purged after grace period");
    }
    Ok(ids.len() as u64)
}

pub async fn purge_loop(st: AppState) {
    loop {
        if let Err(e) = purge_due(&st).await {
            tracing::warn!("account purge failed: {}", e.message);
        }
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
    }
}

/// Real name, gender, birthday and country as stored (owner's own view).
pub async fn details_json(st: &AppState, uid: Uuid) -> ApiResult<Value> {
    let (rn, g, b, c): (
        Option<String>,
        Option<String>,
        Option<chrono::NaiveDate>,
        Option<String>,
    ) = sqlx::query_as("SELECT real_name, gender, birth_date, country FROM users WHERE id = $1")
        .bind(uid)
        .fetch_one(&st.db)
        .await?;
    Ok(json!({ "real_name": rn, "gender": g, "birth_date": b, "country": c }))
}

// ------------------------------------------------------------------ per-feature privacy

/// Per-feature visibility switches stored in `users.privacy` (missing key = shown).
pub const PRIVACY_KEYS: [&str; 12] = [
    "now_playing",
    "games",
    "rig",
    "about",
    "location",
    "gaming_style",
    "leaderboards",
    "name_history",
    "real_name",
    "gender",
    "age",
    "achievements",
];

/// Whether `key` is shown (true unless explicitly switched off).
pub fn shows(privacy: &Value, key: &str) -> bool {
    privacy.get(key).and_then(Value::as_bool).unwrap_or(true)
}

/// All keys with their effective value.
pub fn privacy_view(privacy: &Value) -> Value {
    let mut m = serde_json::Map::new();
    for k in PRIVACY_KEYS {
        m.insert(k.to_string(), json!(shows(privacy, k)));
    }
    Value::Object(m)
}

/// SQL expression: is privacy key `k` shown for the row aliased `u`. `k` must be a constant.
pub fn sql_shows(k: &str) -> String {
    debug_assert!(PRIVACY_KEYS.contains(&k));
    format!("coalesce((u.privacy->>'{k}')::boolean, true)")
}
