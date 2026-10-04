//! Fraglink server (M2): accounts, desktop pairing, session sync, trust-tier
//! hours and Steam import. Plain axum + Postgres (sqlx); see `docs/decisions.md`.

// sqlx row tuples are long but clearer inline than named aliases.
#![allow(clippy::type_complexity)]

pub mod activity;
pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod friends;
pub mod games;
pub mod hours;
pub mod presence;
pub mod steam;
pub mod web;
pub mod xfire_icons;

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
    /// Fallback Xfire icons (FRAGLINK_XFIRE_ICONS=1), else None.
    pub xfire: Arc<Option<xfire_icons::XfireIcons>>,
    pub sigs: Arc<fraglink_core::signatures::SignatureDb>,
    /// Detection lists served to the desktop app.
    pub games: Arc<games::GameLists>,
}

impl AppState {
    pub fn new(db: PgPool, cfg: Config) -> Self {
        let steam = steam::SteamClient::new(&cfg);
        let xfire =
            cfg.xfire_icons_dir
                .as_ref()
                .and_then(|d| match xfire_icons::XfireIcons::load(d) {
                    Ok(x) => {
                        tracing::info!("xfire icons: {} games from {}", x.len(), d.display());
                        Some(x)
                    }
                    Err(e) => {
                        tracing::warn!("xfire icons disabled: {e:#}");
                        None
                    }
                });
        AppState {
            db,
            cfg: Arc::new(cfg),
            steam,
            login_failures: Arc::new(Mutex::new(HashMap::new())),
            export_log: Arc::new(Mutex::new(HashMap::new())),
            hub: Arc::new(presence::Hub::default()),
            limits: Arc::new(Mutex::new(HashMap::new())),
            xfire: Arc::new(xfire),
            sigs: Arc::new(fraglink_core::signatures::SignatureDb::builtin()),
            games: Arc::new(games::GameLists::default()),
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
    Router::new()
        .merge(api::routes())
        .merge(web::routes())
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
}
