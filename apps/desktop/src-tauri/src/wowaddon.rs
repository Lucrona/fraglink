//! The Fraglink /played WoW addon (`fraglink_core::wow_played`), installed and updated
//! from CurseForge: show whether each WoW flavour (`_retail_`, `_forever_`) has it and,
//! after WoW closes, send the characters whose
//! /played changed. The first upload is the full import; later ones are small deltas
//! (Fraglink's own tracking does the rest; /played catches up on play while the app was
//! off, and the profile keeps the largest of the two). Account folder names never leave
//! the PC: only the per-character entries are sent.

use crate::{get_setting, put_setting, AppState, LockOk};
use fraglink_core::wow_played as wp;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const SENT_KEY: &str = "wow_played_sent";

/// WoW flavour folders known on this PC: running / seen exe paths and launcher installs.
pub fn flavours(app: &AppHandle) -> Vec<(String, PathBuf)> {
    let state = app.state::<AppState>();
    let e = state.engine.lock_ok();
    let mut paths: Vec<PathBuf> = Vec::new();
    for (id, s) in &e.seen {
        if fraglink_core::realm::is_wow(id) {
            if let Some(p) = &s.exe_path {
                paths.push(PathBuf::from(p));
            }
        }
    }
    for g in e.detector.libs.games() {
        if g.sig_id
            .as_deref()
            .is_some_and(fraglink_core::realm::is_wow)
        {
            let d = PathBuf::from(&g.install_dir);
            paths.push(d.clone());
            if let Some(p) = d.parent() {
                paths.push(p.to_path_buf());
            }
        }
    }
    drop(e);
    if let Some(saved) = get_setting(app, "wow_dirs") {
        paths.extend(
            saved
                .split('\n')
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    let out = wp::flavour_dirs(&paths);
    put_setting(
        app,
        "wow_dirs",
        &out.iter()
            .map(|(_, d)| d.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    out
}

#[derive(serde::Serialize)]
pub struct FlavourView {
    game_id: String,
    path: String,
    installed: Option<String>,
}

pub fn status(app: &AppHandle) -> Vec<FlavourView> {
    flavours(app)
        .into_iter()
        .map(|(g, d)| FlavourView {
            installed: wp::installed_version(&d),
            path: d.display().to_string(),
            game_id: g,
        })
        .collect()
}

fn key(game: &str, e: &wp::CharEntry) -> String {
    format!(
        "{game}/{}/{}/{}",
        e.region,
        e.realm.to_lowercase(),
        e.name.to_lowercase()
    )
}

/// Entries newer than what was last sent (the delta).
pub fn pending(
    sent: &serde_json::Map<String, serde_json::Value>,
    game: &str,
    entries: Vec<wp::CharEntry>,
) -> Vec<wp::CharEntry> {
    entries
        .into_iter()
        .filter(|e| {
            sent.get(&key(game, e))
                .and_then(|v| v.as_i64())
                .is_none_or(|t| e.at > t)
        })
        .collect()
}

/// Read every flavour's saved file and send what changed. Runs off the UI thread.
pub fn upload(app: &AppHandle) {
    let Some(client) = crate::sync::client(app) else {
        return;
    };
    let mut sent: serde_json::Map<String, serde_json::Value> = get_setting(app, SENT_KEY)
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    for (game, dir) in flavours(app) {
        let todo = pending(&sent, &game, wp::read_flavour(&dir));
        if todo.is_empty() {
            continue;
        }
        let body = serde_json::to_value(&todo).unwrap_or_default();
        match client.wow_played(&game, body) {
            Ok(r) => {
                log::info!(
                    "wow /played: sent {} character(s) for {game}: {} accepted, {} refused",
                    todo.len(),
                    r["accepted"],
                    r["rejected"].as_array().map_or(0, Vec::len)
                );
                // refused ones aren't retried until the game writes a newer answer
                for e in &todo {
                    sent.insert(key(&game, e), e.at.into());
                }
            }
            Err(e) => log::warn!("wow /played upload failed: {e}"),
        }
    }
    put_setting(app, SENT_KEY, &serde_json::Value::Object(sent).to_string());
}

/// WoW just closed: the game writes SavedVariables on exit, so wait a little, then send.
pub fn after_session(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(20));
        upload(&app);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_only() {
        let e = |name: &str, at| wp::CharEntry {
            region: "us".into(),
            realm: "Area 52".into(),
            name: name.into(),
            class: String::new(),
            level: 1,
            played: 10,
            at,
            sv: 1,
            battletag: String::new(),
            sig: String::new(),
        };
        let mut sent = serde_json::Map::new();
        sent.insert("wow/us/area 52/a".into(), 100.into());
        let got = pending(&sent, "wow", vec![e("A", 100), e("B", 5), e("a", 101)]);
        assert_eq!(
            got.iter()
                .map(|x| (x.name.as_str(), x.at))
                .collect::<Vec<_>>(),
            vec![("B", 5), ("a", 101)]
        );
    }
}
