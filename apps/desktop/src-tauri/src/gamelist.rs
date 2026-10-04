//! Keeps the detection lists fresh (0.2.0-dev.12): Discord's detectable list and
//! Fraglink's override rules, from the Fraglink server.
//!
//! Start-up: the cached copies in the data dir are loaded (when newer than the
//! snapshot bundled into the app), then both lists are re-fetched with
//! `If-None-Match`, and again every 24 hours. Offline, the app keeps using the
//! cache, else the bundled snapshot plus the built-in signatures.
//! Only public lists are downloaded; nothing about this PC is sent.

use crate::AppState;
use fraglink_core::detectable::DetectableDb;
use fraglink_core::overrides::Overrides;
use fraglink_sync::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager};

const LIST_PATH: &str = "/api/v1/games/detectable";
const OVERRIDES_PATH: &str = "/api/v1/games/overrides";
const REFRESH: Duration = Duration::from_secs(24 * 3600);
const RETRY: Duration = Duration::from_secs(3600);

struct Files {
    list: PathBuf,
    list_etag: PathBuf,
    overrides: PathBuf,
    overrides_etag: PathBuf,
}

fn files(data: &Path) -> Files {
    Files {
        list: data.join("detectable.json.gz"),
        list_etag: data.join("detectable.etag"),
        overrides: data.join("detect-overrides.json"),
        overrides_etag: data.join("detect-overrides.etag"),
    }
}

/// Load the cached lists (if usable and newer than the bundled snapshot).
fn load_cache(app: &AppHandle, f: &Files) {
    let st = app.state::<AppState>();
    if let Ok(b) = std::fs::read(&f.list) {
        match DetectableDb::from_bytes(&b) {
            Ok(db) if !db.is_empty() => {
                let mut e = st.engine.lock().unwrap();
                if db.updated >= e.detector.detectable.updated {
                    log::info!(
                        "detectable list: {} games from cache ({})",
                        db.len(),
                        db.updated
                    );
                    e.set_detectable(db);
                }
            }
            Ok(_) => {}
            Err(err) => log::warn!("detectable cache unusable: {err:#}"),
        }
    }
    if let Ok(s) = std::fs::read_to_string(&f.overrides) {
        match Overrides::from_json(&s) {
            Ok(o) => {
                log::info!("detection overrides {} from cache", o.version);
                st.engine.lock().unwrap().set_overrides(o);
            }
            Err(err) => log::warn!("overrides cache unusable: {err:#}"),
        }
    }
}

fn read_etag(p: &Path, data: &Path) -> Option<String> {
    // the tag only counts while the file it describes is still there
    data.exists().then_some(())?;
    std::fs::read_to_string(p)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

fn save(path: &Path, bytes: &[u8], etag_path: &Path, etag: Option<&str>) {
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, path))
        .is_err()
    {
        log::warn!("could not save {}", path.display());
        return;
    }
    match etag {
        Some(t) => {
            let _ = std::fs::write(etag_path, t);
        }
        None => {
            let _ = std::fs::remove_file(etag_path);
        }
    }
}

/// One refresh of both lists. Ok(()) when the server answered.
fn refresh(app: &AppHandle, f: &Files) -> Result<(), String> {
    let c = Client::new(&crate::sync::server(app), None);
    let st = app.state::<AppState>();
    match c.fetch_list(LIST_PATH, read_etag(&f.list_etag, &f.list).as_deref()) {
        Ok(None) => log::info!("detectable list: up to date"),
        Ok(Some((body, etag))) => match DetectableDb::from_bytes(&body) {
            Ok(db) if db.len() > 1000 => {
                log::info!(
                    "detectable list: downloaded {} games ({})",
                    db.len(),
                    db.updated
                );
                save(&f.list, &body, &f.list_etag, etag.as_deref());
                st.engine.lock().unwrap().set_detectable(db);
            }
            Ok(db) => log::warn!("detectable list: only {} games, ignored", db.len()),
            Err(e) => log::warn!("detectable list: bad download: {e:#}"),
        },
        Err(e) => return Err(e.to_string()),
    }
    match c.fetch_list(
        OVERRIDES_PATH,
        read_etag(&f.overrides_etag, &f.overrides).as_deref(),
    ) {
        Ok(None) => {}
        Ok(Some((body, etag))) => {
            let s = String::from_utf8_lossy(&body).into_owned();
            match Overrides::from_json(&s) {
                Ok(o) => {
                    log::info!("detection overrides {} downloaded", o.version);
                    save(
                        &f.overrides,
                        s.as_bytes(),
                        &f.overrides_etag,
                        etag.as_deref(),
                    );
                    st.engine.lock().unwrap().set_overrides(o);
                }
                Err(e) => log::warn!("detection overrides: bad download: {e:#}"),
            }
        }
        Err(e) => return Err(e.to_string()),
    }
    Ok(())
}

/// Load the cache now, then refresh in the background (start-up + every 24 h).
pub fn start(app: &AppHandle) {
    let data = app.state::<AppState>().data_dir.clone();
    let f = files(&data);
    load_cache(app, &f);
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("fraglink-gamelist".into())
        .spawn(move || loop {
            let wait = match refresh(&app, &f) {
                Ok(()) => REFRESH,
                Err(e) => {
                    log::info!(
                        "game lists: server not reachable ({e}); using the cached/bundled lists"
                    );
                    RETRY
                }
            };
            std::thread::sleep(wait);
        });
}
