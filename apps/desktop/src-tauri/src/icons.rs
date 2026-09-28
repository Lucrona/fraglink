//! Small game icons for the Friends, Stats and Activity tabs and toasts.
//! Order: icon cache → the detected game's exe (Windows) → Steam's library
//! cache → (signed in only) Steam's public community icon, found via the
//! Fraglink server → none (the UI draws a generic icon). See `fraglink_core::icons`.

use crate::{sync, AppState};
use base64::Engine as _;
use fraglink_core::icons::{self, IconCache, LocalSources};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

pub fn cache(app: &AppHandle) -> IconCache {
    IconCache::new(app.state::<AppState>().data_dir.join("icons"))
}

pub fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// Local sources for each id, gathered under one short engine lock.
fn sources(app: &AppHandle, ids: &[String]) -> HashMap<String, LocalSources> {
    let st = app.state::<AppState>();
    let steam_root = if st.sim {
        None
    } else {
        fraglink_core::steam::steam_root()
    };
    let e = st.engine.lock().unwrap();
    let rows: HashMap<String, Option<u32>> = e
        .tracker
        .store
        .list_games()
        .unwrap_or_default()
        .into_iter()
        .map(|g| (g.id, g.steam_appid))
        .collect();
    ids.iter()
        .map(|id| {
            let seen = e.seen.get(id);
            let src = LocalSources {
                exe_path: seen.and_then(|s| s.exe_path.as_ref().map(PathBuf::from)),
                steam_appid: seen
                    .and_then(|s| s.steam_appid)
                    .or_else(|| rows.get(id).copied().flatten()),
                steam_root: steam_root.clone(),
            };
            (id.clone(), src)
        })
        .collect()
}

/// Resolve icons as `data:` URLs (None = use the generic icon).
/// `remote` allows asking the server + Steam CDN for games not on this PC.
pub fn resolve(app: &AppHandle, ids: &[String], remote: bool) -> HashMap<String, Option<String>> {
    let sim = app.state::<AppState>().sim;
    let cache = cache(app);
    let srcs = sources(app, ids);
    let extract = move |p: &Path| {
        if sim {
            // Simulation: a placeholder stands in for the exe icon.
            let stem = p
                .to_string_lossy()
                .rsplit(['\\', '/'])
                .next()
                .unwrap_or("game")
                .to_string();
            Some(icons::placeholder(&stem))
        } else {
            icons::extract_exe_icon(p)
        }
    };
    let now = SystemTime::now();
    let mut out = HashMap::new();
    let mut missing = vec![];
    for id in ids {
        // Cache (unless the game's exe changed since), then local sources: a game
        // detected since the last miss gets its exe icon.
        let src = srcs.get(id).cloned().unwrap_or_default();
        match icons::resolve_local(&cache, id, &src, &extract) {
            Some((png, _)) => {
                out.insert(id.clone(), Some(data_url(&png)));
            }
            None if cache.recent_miss(id, now) => {
                out.insert(id.clone(), None);
            }
            None => missing.push(id.clone()),
        }
    }
    if missing.is_empty() {
        return out;
    }
    let client = if remote { sync::client(app) } else { None };
    match client.as_ref().map(|c| c.game_icons(&missing)) {
        Some(Ok(v)) => {
            for id in &missing {
                let png = server_png(client.as_ref().unwrap(), v.get(id));
                match png {
                    Some(p) => {
                        let _ = cache.put(id, &p);
                        out.insert(id.clone(), Some(data_url(&p)));
                    }
                    None => {
                        cache.put_miss(id);
                        out.insert(id.clone(), None);
                    }
                }
            }
        }
        Some(Err(e)) => {
            // offline: try again next time, don't remember a miss
            log::debug!("icons: server lookup failed: {e}");
            for id in missing {
                out.insert(id, None);
            }
        }
        None => {
            // signed out (or local-only request): only remember misses for games
            // this PC knows about, so a later sign-in can still fetch friends' icons
            for id in missing {
                if !remote && srcs.get(&id).is_some_and(|s| s.exe_path.is_some()) {
                    cache.put_miss(&id);
                }
                out.insert(id, None);
            }
        }
    }
    out
}

/// Steam CDN icon, else an icon hosted by the Fraglink server (the Xfire fallback).
fn server_png(client: &fraglink_sync::Client, v: Option<&serde_json::Value>) -> Option<Vec<u8>> {
    let v = v?;
    v.get("icon_url")
        .and_then(|u| u.as_str())
        .and_then(|u| fraglink_sync::fetch_icon(u).ok())
        .or_else(|| {
            v.get("icon_path")
                .and_then(|p| p.as_str())
                .and_then(|p| client.fetch_server_icon(p).ok())
        })
        .and_then(|b| icons::normalize(&b))
}

/// Friends' games: the server's icon (Steam / Xfire) wins, cached separately; anything
/// the server has no icon for falls back to `resolve` (local exe icon etc.).
pub fn resolve_friends(app: &AppHandle, ids: &[String]) -> HashMap<String, Option<String>> {
    let cache = cache(app);
    let key = |id: &str| format!("srv~{id}");
    let now = SystemTime::now();
    let mut out = HashMap::new();
    let mut ask = vec![];
    for id in ids {
        match cache.get(&key(id)) {
            Some(png) => {
                out.insert(id.clone(), Some(data_url(&png)));
            }
            None if cache.recent_miss(&key(id), now) => {}
            None => ask.push(id.clone()),
        }
    }
    if let (false, Some(client)) = (ask.is_empty(), sync::client(app)) {
        if let Ok(v) = client.game_icons(&ask) {
            for id in &ask {
                match server_png(&client, v.get(id)) {
                    Some(p) => {
                        let _ = cache.put(&key(id), &p);
                        out.insert(id.clone(), Some(data_url(&p)));
                    }
                    None => cache.put_miss(&key(id)),
                }
            }
        }
    }
    let rest: Vec<String> = ids
        .iter()
        .filter(|i| !out.contains_key(*i))
        .cloned()
        .collect();
    if !rest.is_empty() {
        out.extend(resolve(app, &rest, true));
    }
    out
}

pub fn one(app: &AppHandle, id: &str, remote: bool) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    resolve(app, &[id.to_string()], remote).remove(id).flatten()
}
