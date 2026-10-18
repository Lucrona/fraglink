//! Fraglink server (M2): accounts, desktop pairing, session sync, trust-tier
//! hours and Steam import. Plain axum + Postgres (sqlx); see `docs/decisions.md`.

// sqlx row tuples are long but clearer inline than named aliases.
#![allow(clippy::type_complexity)]

pub mod account;
pub mod achievements;
pub mod activity;
pub mod admin;
pub mod alerts;
pub mod api;
pub mod art;
pub mod auth;
pub mod avatars;
pub mod config;
pub mod connect;
pub mod emails;
pub mod error;
pub mod features;
pub mod friends;
pub mod game_overrides;
pub mod game_status;
pub mod games;
pub mod geo;
pub mod home;
pub mod hours;
pub mod icons;
pub mod mailer;
pub mod mfa;
pub mod names;
pub mod news;
pub mod paging;
pub mod presence;
pub mod profile;
pub mod realms;
pub mod retire;
pub mod seo;
pub mod sig;
pub mod steam;
pub mod turnstile;
pub mod web;

use axum::Router;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub use config::Config;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub cfg: Arc<Config>,
    pub steam: steam::SteamClient,
    /// Failed logins per lower-case email (in-memory, per process).
    pub login_failures: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    /// Last zip export per user (at most one per hour).
    pub export_log: Arc<Mutex<HashMap<uuid::Uuid, Instant>>>,
    /// Live WebSocket connections for presence fan-out.
    pub hub: Arc<presence::Hub>,
    /// Rate-limit hits per key (`signup:<ip>`, `login:<ip>`, `friendreq:<user>` ...).
    pub limits: Arc<Mutex<HashMap<String, Vec<Instant>>>>,
    pub sigs: Arc<fraglink_core::signatures::SignatureDb>,
    /// Detection lists served to the desktop app.
    pub games: Arc<games::GameLists>,
    /// Staff edits to games (see `game_overrides`).
    pub overrides: Arc<game_overrides::OverrideCache>,
    /// Retired games -> successor ids (see `retire`).
    pub retired: Arc<retire::RetireCache>,
    /// WoW realm names (see `realms`).
    pub realms: Arc<realms::RealmCache>,
    /// Signed-out `GET /api/v1/home` response, short-lived (see `home`).
    pub home_public: Arc<std::sync::Mutex<Option<(std::time::Instant, serde_json::Value)>>>,
}

impl AppState {
    pub fn new(db: PgPool, cfg: Config) -> Self {
        let steam = steam::SteamClient::new(&cfg);
        AppState {
            db,
            cfg: Arc::new(cfg),
            steam,
            login_failures: Arc::new(Mutex::new(HashMap::new())),
            export_log: Arc::new(Mutex::new(HashMap::new())),
            hub: Arc::new(presence::Hub::default()),
            limits: Arc::new(Mutex::new(HashMap::new())),
            sigs: Arc::new(fraglink_core::signatures::SignatureDb::builtin()),
            games: Arc::new(games::GameLists::default()),
            overrides: Arc::new(game_overrides::OverrideCache::default()),
            retired: Arc::new(retire::RetireCache::default()),
            realms: Arc::new(realms::RealmCache::default()),
            home_public: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Sliding-window rate limit: records a hit for `key` and returns an error once more than
    /// `max` hits fell inside `window`. In memory, per process (one server).
    pub fn rate_limit(
        &self,
        key: String,
        max: usize,
        window: std::time::Duration,
    ) -> error::ApiResult<()> {
        let mut m = self.limits.lock().unwrap();
        if m.len() > 50_000 {
            m.retain(|_, v| {
                v.last()
                    .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(3600))
            });
        }
        let v = m.entry(key).or_default();
        v.retain(|t| t.elapsed() < window);
        if v.len() >= max {
            return Err(error::AppError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many attempts. Wait a few minutes and try again.",
            ));
        }
        v.push(Instant::now());
        Ok(())
    }
}

/// The caller's IP. The server only listens on localhost behind Caddy, which replaces any
/// client-sent X-Forwarded-For with the real address.
pub fn client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "local".into())
}

/// The whole HTTP surface.
pub fn router(state: AppState) -> Router {
    admin::mark_started();
    Router::new()
        .merge(api::routes())
        .merge(web::routes())
        .merge(profile::routes())
        .merge(admin::routes())
        .merge(news::routes())
        .merge(mfa::routes())
        .merge(account::routes())
        .merge(avatars::routes())
        .merge(home::routes())
        .merge(art::routes())
        .merge(connect::routes())
        .merge(seo::routes())
        .route("/api/v1/features", axum::routing::get(features::public))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            features::gate,
        ))
        .layer(axum::middleware::from_fn(alerts::track))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
}
