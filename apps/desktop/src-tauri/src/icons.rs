//! Small game icons for the Friends, Stats and Activity tabs and toasts.
//! Order (every game): the classic Xfire icon matched by the Fraglink server →
//! Steam's icon (server CDN answer or Steam's library cache) → the game's exe
//! (Windows) → none (the UI draws a generic icon). Everything fetched is cached
//! on disk so it works offline. See `fraglink_core::icons`.

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

const XF: &str = "xf~"; // cached Xfire icon from the server
const ST: &str = "st~"; // cached Steam icon (server CDN answer or Steam's library cache)

fn key(prefix: &str, id: &str) -> String {
    format!("{prefix}{id}")
}

/// Resolve icons as `data:` URLs (None = use the generic icon). Priority for every
/// game, installed or not: 1) the server's matched Xfire icon, 2) Steam's icon,
/// 3) the local exe icon, 4) generic. Server answers are cached on disk, so icons
/// keep working offline after the first fetch. `remote` allows asking the server.
pub fn resolve(app: &AppHandle, ids: &[String], remote: bool) -> HashMap<String, Option<String>> {
    let sim = app.state::<AppState>().sim;
    let cache = cache(app);
    let srcs = sources(app, ids);
    let now = SystemTime::now();
    let mut out: HashMap<String, Option<String>> = HashMap::new();
    // 1) cached Xfire icons; ask the server for games it hasn't answered lately
    let mut ask = vec![];
    for id in ids {
        if let Some(png) = cache.get(&key(XF, id)) {
            out.insert(id.clone(), Some(data_url(&png)));
        } else if !cache.recent_miss(&key(XF, id), now) {
            ask.push(id.clone());
        }
    }
    let client = if remote && !ask.is_empty() {
        sync::client(app)
    } else {
        None
    };
    let mut answered: std::collections::HashSet<String> = Default::default();
    if let Some(client) = &client {
        match client.game_icons(&ask) {
            Ok(v) => {
                for id in &ask {
                    answered.insert(id.clone());
                    let e = v.get(id);
                    let xf = e
                        .and_then(|e| e.get("icon_path"))
                        .and_then(|p| p.as_str())
                        .and_then(|p| client.fetch_server_icon(p).ok())
                        .and_then(|b| icons::normalize(&b));
                    match xf {
                        Some(p) => {
                            let _ = cache.put(&key(XF, id), &p);
                            out.insert(id.clone(), Some(data_url(&p)));
                        }
                        None => cache.put_miss(&key(XF, id)),
                    }
                    if out.contains_key(id) || cache.get(&key(ST, id)).is_some() {
                        continue;
                    }
                    if let Some(p) = e
                        .and_then(|e| e.get("icon_url"))
                        .and_then(|u| u.as_str())
                        .and_then(|u| fraglink_sync::fetch_icon(u).ok())
                        .and_then(|b| icons::normalize(&b))
                    {
                        let _ = cache.put(&key(ST, id), &p);
                    }
                }
            }
            // offline: fall through to the cached/local icons, ask again next time
            Err(e) => log::debug!("icons: server lookup failed: {e}"),
        }
    }
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
    for id in ids {
        if out.contains_key(id) {
            continue;
        }
        let src = srcs.get(id).cloned().unwrap_or_default();
        // 2) Steam: the server's CDN icon (cached) or Steam's local library cache
        let steam = cache.get(&key(ST, id)).or_else(|| {
            let appid = src.steam_appid.or_else(|| icons::appid_from_game_id(id))?;
            let p = icons::steam_library_icon(src.steam_root.as_deref()?, appid)?;
            let png = std::fs::read(p).ok().and_then(|b| icons::normalize(&b))?;
            let _ = cache.put(&key(ST, id), &png);
            Some(png)
        });
        if let Some(png) = steam {
            out.insert(id.clone(), Some(data_url(&png)));
            continue;
        }
        // 3) the local exe icon (cached unless the exe changed)
        let local = if cache.recent_miss(id, now) && src.exe_path.is_none() {
            None
        } else {
            icons::resolve_local(&cache, id, &src, &extract).map(|(png, _)| png)
        };
        match local {
            Some(png) => {
                out.insert(id.clone(), Some(data_url(&png)));
            }
            None => {
                // remember a miss only once the server has answered too
                if answered.contains(id) || src.exe_path.is_some() {
                    cache.put_miss(id);
                }
                out.insert(id.clone(), None);
            }
        }
    }
    out
}

pub fn one(app: &AppHandle, id: &str, remote: bool) -> Option<String> {
    if id.is_empty() {
        return None;
    }
    resolve(app, &[id.to_string()], remote).remove(id).flatten()
}
