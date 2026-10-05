//! Small game icons for the Friends, Stats and Activity tabs and toasts.
//! Order (every game, 0.2.0-dev.15): the official icon from the Fraglink server
//! (it fetches and caches them; the app never downloads icons from anyone else)
//! → Steam's local library cache → the game's exe (Windows) → none (the UI draws
//! a generic icon). Everything is cached on disk so it works offline. See
//! `fraglink_core::icons`.

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

/// Local sources for each id, gathered under one short engine lock. A game that
/// hasn't run in this session still gets its exe (the one Options > Games shows
/// as found in its install folder) and Steam app id from the catalog (dev.10), so
/// installed games the server has no icon for still get their own icon.
fn sources(app: &AppHandle, ids: &[String]) -> HashMap<String, LocalSources> {
    let st = app.state::<AppState>();
    let steam_root = if st.sim {
        None
    } else {
        fraglink_core::steam::steam_root()
    };
    let cat: HashMap<String, (Option<PathBuf>, Option<u32>)> = if st.sim {
        HashMap::new()
    } else {
        crate::commands::catalog_now(&st)
            .into_iter()
            .filter(|c| c.installed && ids.contains(&c.id))
            .map(|c| {
                let exe = Some(c.detect_path)
                    .filter(|p| p.to_ascii_lowercase().ends_with(".exe"))
                    .filter(|p| p.contains(['\\', '/']))
                    .map(PathBuf::from);
                (c.id, (exe, c.steam_appid))
            })
            .collect()
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
            let (cat_exe, cat_appid) = cat.get(id).cloned().unwrap_or_default();
            let src = LocalSources {
                exe_path: seen
                    .and_then(|s| s.exe_path.as_ref().map(PathBuf::from))
                    .or(cat_exe),
                steam_appid: seen
                    .and_then(|s| s.steam_appid)
                    .or_else(|| rows.get(id).copied().flatten())
                    .or(cat_appid),
                steam_root: steam_root.clone(),
            };
            (id.clone(), src)
        })
        .collect()
}

const SV: &str = "sv~"; // cached official icon from the Fraglink server
const ST: &str = "st~"; // cached copy of Steam's local library icon
/// Server icons are checked again after this long (kept if the server is unreachable).
const SERVER_REFRESH: std::time::Duration = std::time::Duration::from_secs(14 * 24 * 3600);

fn key(prefix: &str, id: &str) -> String {
    format!("{prefix}{id}")
}

/// Icons cached by apps before dev.15 (`xf~` from the server's old icon set, `st~`
/// downloaded from a CDN) are dropped once so the official ones replace them.
fn drop_old_icons(cache: &IconCache) {
    let marker = cache.dir().join(".dev15");
    if marker.exists() {
        return;
    }
    if let Ok(rd) = std::fs::read_dir(cache.dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("xf_") || n.starts_with("st_") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let _ = std::fs::create_dir_all(cache.dir());
    let _ = std::fs::write(marker, b"");
}

/// Games whose server icon was replaced since the last release: the cached copy is
/// fetched again once (dev.16: Star Citizen's official emblem).
const REFRESHED_ICONS: (&str, &[&str]) = (".dev16", &["star_citizen"]);

fn drop_refreshed_icons(cache: &IconCache) {
    let marker = cache.dir().join(REFRESHED_ICONS.0);
    if marker.exists() {
        return;
    }
    for id in REFRESHED_ICONS.1 {
        let _ = std::fs::remove_file(cache.png_path(&key(SV, id)));
    }
    let _ = std::fs::create_dir_all(cache.dir());
    let _ = std::fs::write(marker, b"");
}

/// Resolve icons as `data:` URLs (None = use the generic icon). Priority for every
/// game, installed or not: 1) the official icon from the Fraglink server, 2) Steam's
/// local library icon, 3) the local exe icon, 4) generic. Server answers are cached
/// on disk, so icons keep working offline. `remote` allows asking the server.
pub fn resolve(app: &AppHandle, ids: &[String], remote: bool) -> HashMap<String, Option<String>> {
    let sim = app.state::<AppState>().sim;
    let cache = cache(app);
    drop_old_icons(&cache);
    drop_refreshed_icons(&cache);
    let srcs = sources(app, ids);
    let now = SystemTime::now();
    let mut out: HashMap<String, Option<String>> = HashMap::new();
    // 1) cached server icons; ask the server about games it hasn't answered lately
    // (manual tags are this PC's own names: the server has nothing for them)
    let mut ask = vec![];
    for id in ids {
        if id.starts_with("manual:") {
            continue;
        }
        let k = key(SV, id);
        if let Some(png) = cache.get(&k) {
            out.insert(id.clone(), Some(data_url(&png)));
            if cache.age(&k, now).is_some_and(|a| a > SERVER_REFRESH) {
                ask.push(id.clone());
            }
        } else if !cache.recent_miss(&k, now) {
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
                    let png = v
                        .get(id)
                        .and_then(|e| e.get("icon_path"))
                        .and_then(|p| p.as_str())
                        .filter(|p| p.starts_with("/icons/games/"))
                        .and_then(|p| client.fetch_server_icon(p).ok())
                        .and_then(|b| icons::normalize(&b));
                    match png {
                        Some(p) => {
                            let _ = cache.put(&key(SV, id), &p);
                            out.insert(id.clone(), Some(data_url(&p)));
                        }
                        // keep a cached icon if the server has none right now
                        None if out.contains_key(id) => {}
                        None => cache.put_miss(&key(SV, id)),
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
        // 2) Steam's local library cache (this PC only)
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
