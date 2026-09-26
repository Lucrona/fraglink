//! Fraglink server (M2): accounts, desktop pairing, session sync, trust-tier
//! hours and Steam import. Plain axum + Postgres (sqlx); see `docs/decisions.md`.

// sqlx row tuples are long but clearer inline than named aliases.
#![allow(clippy::type_complexity)]

pub mod api;
pub mod auth;
pub mod config;
pub mod error;
pub mod friends;
pub mod hours;
pub mod presence;
pub mod steam;
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
        }
    }
}

/// The whole HTTP surface.
pub fn router(state: AppState) -> Router {
    Router::new()
        .merge(api::routes())
        .merge(web::routes())
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024))
        .with_state(state)
}
