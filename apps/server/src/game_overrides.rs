//! Staff edits to games (`game_overrides`), applied on top of the daily games list
//! so they survive every refresh: a display name, "hidden from public" (shown as an
//! unlisted game to everyone), extra exe names (served with the detection rules) and
//! an uploaded icon (`<data dir>/icon-custom/`, served before the official one).
//!
//! Kept in memory (a few rows) and reloaded after every staff edit.

use crate::error::ApiResult;
use crate::AppState;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Debug, Default, Serialize, sqlx::FromRow)]
pub struct GameOverride {
    pub game_id: String,
    pub name: Option<String>,
    pub hidden: bool,
    pub extra_exes: Vec<String>,
    pub has_icon: bool,
    pub icon_pref: Option<String>,
    pub art_pref: Option<String>,
    pub steam_appid: Option<i32>,
}

pub type Map = Arc<HashMap<String, GameOverride>>;

#[derive(Default)]
pub struct OverrideCache {
    map: RwLock<Option<Map>>,
}

impl OverrideCache {
    pub fn snapshot(&self) -> Option<Map> {
        self.map.read().unwrap().clone()
    }
    fn set(&self, m: Map) {
        *self.map.write().unwrap() = Some(m);
    }
}

/// Re-read every override from the database.
pub async fn reload(st: &AppState) -> ApiResult<Map> {
    let rows: Vec<GameOverride> =
        sqlx::query_as("SELECT game_id, name, hidden, extra_exes, has_icon, icon_pref, art_pref, steam_appid FROM game_overrides")
            .fetch_all(&st.db)
            .await?;
    let m: Map = Arc::new(rows.into_iter().map(|o| (o.game_id.clone(), o)).collect());
    st.overrides.set(m.clone());
    Ok(m)
}

/// The current overrides (loaded on first use).
pub async fn current(st: &AppState) -> ApiResult<Map> {
    match st.overrides.snapshot() {
        Some(m) => Ok(m),
        None => reload(st).await,
    }
}

pub fn custom_icon_dir(st: &AppState) -> std::path::PathBuf {
    st.cfg.data_dir.join("icon-custom")
}

/// Media choices for a game: (icon source, art source, Steam app id), all optional.
pub async fn media(st: &AppState, id: &str) -> (Option<String>, Option<String>, Option<u32>) {
    match current(st).await.ok().and_then(|m| m.get(id).cloned()) {
        Some(o) => (o.icon_pref, o.art_pref, o.steam_appid.map(|a| a as u32)),
        None => (None, None, None),
    }
}
