//! Linking outside accounts from the website (Settings > Connections).
//!
//! The site keeps its sign-in token in localStorage, so a provider redirect can't carry
//! it. `POST /api/v1/connect/{provider}/start` (signed in) returns a provider URL with a
//! one-time `state` (10 minutes, in memory); the provider sends the browser back to
//! `GET /api/v1/connect/{provider}/return`, which links the account to the user who
//! started it and redirects to `<site>/settings?connected=...#connections`.
//!
//! - Steam: OpenID 2.0 ("Sign in through Steam"), no keys needed. Hours import needs
//!   `STEAM_API_KEY`.
//! - Discord: OAuth2 `identify`, needs `DISCORD_CLIENT_ID` + `DISCORD_CLIENT_SECRET`.
//! - Battle.net: OAuth2 `openid`, needs `BNET_CLIENT_ID` + `BNET_CLIENT_SECRET`.
//!
//! Redirect URL to register with Discord / Blizzard: `<FRAGLINK_SITE_URL>/api/v1/connect/<discord|bnet>/return`.

use crate::api;
use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use rand::RngCore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const PROVIDERS: [&str; 3] = ["steam", "discord", "bnet"];
const STATE_TTL: Duration = Duration::from_secs(600);

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/connect", get(status))
        .route("/api/v1/connect/{provider}/start", post(start))
        .route("/api/v1/connect/{provider}/return", get(finish))
        .route("/api/v1/connect/{provider}", delete(unlink))
}

fn pending() -> &'static Mutex<HashMap<String, (Uuid, String, Instant)>> {
    static P: std::sync::OnceLock<Mutex<HashMap<String, (Uuid, String, Instant)>>> =
        std::sync::OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}

fn creds(provider: &str) -> Option<(String, String)> {
    match provider {
        "discord" => Some((env("DISCORD_CLIENT_ID")?, env("DISCORD_CLIENT_SECRET")?)),
        "bnet" => Some((env("BNET_CLIENT_ID")?, env("BNET_CLIENT_SECRET")?)),
        _ => None,
    }
}

fn site(st: &AppState) -> String {
    st.cfg.site_url.trim_end_matches('/').to_string()
}

pub fn return_url(st: &AppState, provider: &str) -> String {
    format!("{}/api/v1/connect/{provider}/return", site(st))
}

fn available(provider: &str) -> bool {
    provider == "steam" || creds(provider).is_some()
}

async fn status(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let linked: Value = sqlx::query_scalar(
        "SELECT coalesce(jsonb_object_agg(provider, jsonb_build_object('uid', provider_uid, 'display', display,
                'verified', verified, 'show_on_profile', show_on_profile, 'linked_at', linked_at,
                'last_sync_at', last_sync_at, 'sync_status', sync_status)), '{}'::jsonb)
         FROM linked_accounts WHERE user_id = $1",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    let mut out = serde_json::Map::new();
    for p in PROVIDERS {
        // a switched-off service (Admin > Site settings) can't be linked; unlinking still works
        let off = !crate::features::on(&st, &format!("connect_{p}")).await;
        out.insert(
            p.into(),
            json!({ "available": available(p) && !off, "off": off, "linked": linked.get(p),
                    "import": p == "steam" && st.steam.configured() && !off }),
        );
    }
    Ok(Json(Value::Object(out)))
}

async fn start(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(provider): Path<String>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(AppError::not_found());
    }
    if !crate::features::on(&st, &format!("connect_{provider}")).await {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "feature_off",
            "Linking this service is turned off right now.",
        ));
    }
    if !available(&provider) {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "not_configured",
            "Linking this account is coming soon.",
        ));
    }
    st.rate_limit(
        format!("connect:{}", a.user.id),
        20,
        Duration::from_secs(600),
    )?;
    let mut b = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut b);
    let state: String = b.iter().map(|x| format!("{x:02x}")).collect();
    {
        let mut p = pending().lock().unwrap();
        p.retain(|_, v| v.2.elapsed() < STATE_TTL);
        p.insert(state.clone(), (a.user.id, provider.clone(), Instant::now()));
    }
    let ret = return_url(&st, &provider);
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let url = match provider.as_str() {
        "steam" => st
            .steam
            .openid_redirect_to(&format!("{ret}?state={state}"), &site(&st)),
        "discord" => {
            let (id, _) = creds("discord").unwrap();
            format!(
                "https://discord.com/oauth2/authorize?client_id={}&redirect_uri={}&response_type=code&scope=identify&prompt=none&state={state}",
                enc(&id),
                enc(&ret)
            )
        }
        _ => {
            let (id, _) = creds("bnet").unwrap();
            format!(
                "https://oauth.battle.net/authorize?client_id={}&redirect_uri={}&response_type=code&scope=openid&state={state}",
                enc(&id),
                enc(&ret)
            )
        }
    };
    Ok(Json(json!({ "url": url })))
}

fn back(st: &AppState, q: &str) -> Response {
    Redirect::to(&format!("{}/settings?{q}#connections", site(st))).into_response()
}

async fn finish(
    State(st): State<AppState>,
    Path(provider): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    let taken = q
        .get("state")
        .and_then(|s| pending().lock().unwrap().remove(s));
    let Some((uid, p, at)) = taken.filter(|t| t.1 == provider && t.2.elapsed() < STATE_TTL) else {
        return back(&st, "connect_error=expired");
    };
    let _ = (p, at);
    if q.get("openid.mode").map(String::as_str) == Some("cancel") || q.contains_key("error") {
        return back(&st, "connect_error=cancelled");
    }
    let r: ApiResult<()> = async {
        match provider.as_str() {
            "steam" => {
                let id = st
                    .steam
                    .verify_openid_at(&return_url(&st, "steam"), &q)
                    .await?;
                crate::steam::link(&st, uid, id, true).await?;
                crate::steam::refresh_persona(&st, uid, id).await;
                Ok(())
            }
            "discord" | "bnet" => {
                let code = q.get("code").ok_or_else(|| {
                    AppError::bad_request("no_code", "The sign-in didn't return a code.")
                })?;
                let (puid, display) = oauth_identity(&st, &provider, code).await?;
                link_oauth(&st, uid, &provider, &puid, &display).await
            }
            _ => Err(AppError::not_found()),
        }
    }
    .await;
    match r {
        Ok(()) => back(&st, &format!("connected={provider}")),
        Err(e) => back(&st, &format!("connect_error={}", enc(&e.message))),
    }
}

async fn oauth_identity(st: &AppState, provider: &str, code: &str) -> ApiResult<(String, String)> {
    let (id, secret) = creds(provider)
        .ok_or_else(|| AppError::bad_request("not_configured", "Not configured."))?;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(AppError::internal)?;
    let up = |e: reqwest::Error| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "provider_unreachable",
            e.without_url().to_string(),
        )
    };
    let bad = || {
        AppError::bad_request(
            "link_failed",
            "The account couldn't be verified. Try again.",
        )
    };
    let (token_url, me_url) = match provider {
        "discord" => (
            "https://discord.com/api/oauth2/token",
            "https://discord.com/api/users/@me",
        ),
        _ => (
            "https://oauth.battle.net/token",
            "https://oauth.battle.net/userinfo",
        ),
    };
    let ret = return_url(st, provider);
    let tok: Value = http
        .post(token_url)
        .basic_auth(&id, Some(&secret))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", ret.as_str()),
        ])
        .send()
        .await
        .map_err(up)?
        .json()
        .await
        .map_err(|_| bad())?;
    let at = tok["access_token"].as_str().ok_or_else(bad)?;
    let me: Value = http
        .get(me_url)
        .bearer_auth(at)
        .send()
        .await
        .map_err(up)?
        .json()
        .await
        .map_err(|_| bad())?;
    let (uid, display) = match provider {
        "discord" => (
            me["id"].as_str().map(String::from),
            me["global_name"]
                .as_str()
                .or(me["username"].as_str())
                .map(String::from),
        ),
        _ => (
            me["id"]
                .as_i64()
                .map(|i| i.to_string())
                .or(me["sub"].as_str().map(String::from)),
            me["battletag"].as_str().map(String::from),
        ),
    };
    let uid = uid.ok_or_else(bad)?;
    Ok((uid.clone(), display.unwrap_or(uid)))
}

async fn link_oauth(
    st: &AppState,
    uid: Uuid,
    provider: &str,
    puid: &str,
    display: &str,
) -> ApiResult<()> {
    let other: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM linked_accounts WHERE provider = $1 AND provider_uid = $2 AND verified AND user_id <> $3",
    )
    .bind(provider)
    .bind(puid)
    .bind(uid)
    .fetch_optional(&st.db)
    .await?;
    if other.is_some() {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            "already_linked",
            "That account is already linked to another Fraglink account.",
        ));
    }
    sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified)
         VALUES($1, $2, $3, $4, true)
         ON CONFLICT (user_id, provider) DO UPDATE
           SET provider_uid = excluded.provider_uid, display = excluded.display, verified = true, linked_at = now()",
    )
    .bind(uid)
    .bind(provider)
    .bind(puid)
    .bind(display.chars().take(64).collect::<String>())
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn unlink(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(provider): Path<String>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(AppError::not_found());
    }
    sqlx::query("DELETE FROM linked_accounts WHERE user_id = $1 AND provider = $2")
        .bind(a.user.id)
        .bind(&provider)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
