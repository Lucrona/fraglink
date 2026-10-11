//! Profile pictures for the UI (dev.19). The server sends each user's avatar as JSON
//! (`kind: default | preset | upload`, upload URLs at 184/64/32 px); defaults and presets
//! are drawn by the UI. Uploaded files are fetched here (the window's CSP only allows
//! local images), kept in memory and on disk under `<data dir>/avatar-cache/` (max
//! [`DISK_LIMIT`], oldest files dropped first) and handed to the UI as data URLs. The
//! file name on the server carries a content hash, so a new picture is a new URL and
//! old entries simply age out.

use base64::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const DISK_LIMIT: u64 = 20 * 1024 * 1024;
const MEM_LIMIT: usize = 400;

static MEM: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn dir(app: &AppHandle) -> PathBuf {
    app.state::<crate::AppState>().data_dir.join("avatar-cache")
}

/// `/api/v1/avatars/<uuid>/<hash>-<px>.webp` -> `<uuid>_<hash>-<px>.webp`
fn file_name(url: &str) -> Option<String> {
    let rest = url.strip_prefix("/api/v1/avatars/")?;
    let (user, file) = rest.split_once('/')?;
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
    };
    (ok(user) && ok(file) && !file.contains("..") && file.ends_with(".webp"))
        .then(|| format!("{user}_{file}"))
}

fn data_url(b: &[u8]) -> String {
    format!(
        "data:image/webp;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(b)
    )
}

fn trim_disk(d: &Path) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= DISK_LIMIT {
        return;
    }
    files.sort();
    for (_, len, p) in files {
        if total <= DISK_LIMIT * 3 / 4 {
            break;
        }
        if std::fs::remove_file(&p).is_ok() {
            total -= len;
        }
    }
}

/// Data URLs for the given avatar URLs (unknown / failed ones are left out).
pub fn resolve(app: &AppHandle, urls: &[String]) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut ask = vec![];
    {
        let mut m = MEM.lock().unwrap();
        let m = m.get_or_insert_with(HashMap::new);
        for u in urls {
            match m.get(u) {
                Some(d) => {
                    out.insert(u.clone(), d.clone());
                }
                None => ask.push(u.clone()),
            }
        }
    }
    if ask.is_empty() {
        return out;
    }
    let d = dir(app);
    let _ = std::fs::create_dir_all(&d);
    let client = crate::sync::client(app);
    let mut wrote = false;
    for u in ask {
        let Some(name) = file_name(&u) else { continue };
        let path = d.join(&name);
        let bytes = match std::fs::read(&path) {
            Ok(b) if !b.is_empty() => Some(b),
            _ => client
                .as_ref()
                .and_then(|c| c.fetch_avatar(&u).ok())
                .filter(|b| b.starts_with(b"RIFF"))
                .inspect(|b| {
                    wrote |= std::fs::write(&path, b).is_ok();
                }),
        };
        if let Some(b) = bytes {
            let du = data_url(&b);
            let mut m = MEM.lock().unwrap();
            let m = m.get_or_insert_with(HashMap::new);
            if m.len() >= MEM_LIMIT {
                m.clear();
            }
            m.insert(u.clone(), du.clone());
            out.insert(u, du);
        }
    }
    if wrote {
        trim_disk(&d);
    }
    out
}

/// The avatar JSON the server sent for `handle` (friends, incoming / outgoing requests).
fn avatar_of(app: &AppHandle, handle: &str) -> Option<serde_json::Value> {
    let st = app.state::<crate::AppState>();
    let list = st.friends.list.lock().unwrap().clone()?;
    ["friends", "incoming", "outgoing"]
        .iter()
        .filter_map(|k| list.get(*k).and_then(|v| v.as_array()))
        .flatten()
        .find(|f| {
            f["handle"]
                .as_str()
                .is_some_and(|h| h.eq_ignore_ascii_case(handle))
        })
        .map(|f| f["avatar"].clone())
        .filter(|a| a.is_object())
}

/// Put `handle`'s picture into a toast (uploaded image, else colour / preset).
pub fn fill_toast(app: &AppHandle, handle: &str, p: &mut crate::ToastPayload) {
    let Some(a) = avatar_of(app, handle) else {
        return;
    };
    p.avatar_color = (a["color"].as_u64().unwrap_or(0) % 6) as u8;
    match a["kind"].as_str() {
        Some("upload") => {
            if let Some(u) = a["url_64"].as_str() {
                p.avatar = resolve(app, &[u.to_string()]).remove(u);
            }
        }
        Some("preset") => p.avatar_preset = a["preset"].as_u64().map(|x| (x % 12) as u8),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        assert_eq!(
            file_name("/api/v1/avatars/0b5a-11/abc123-64.webp").as_deref(),
            Some("0b5a-11_abc123-64.webp")
        );
        assert!(file_name("/api/v1/avatars/x/../y.webp").is_none());
        assert!(file_name("/icons/x.webp").is_none());
        assert!(file_name("/api/v1/avatars/x/y.png").is_none());
    }
}
