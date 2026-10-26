//! The Fraglink /played WoW addon (`fraglink_core::wow_played`), installed and updated
//! from CurseForge: show whether each WoW flavour (`_retail_`, `_forever_`) has it.
//! The app doesn't read or send FraglinkPlayed.lua; players upload it on the website
//! (`/connect/wow`).

use crate::{get_setting, put_setting, AppState, LockOk};
use fraglink_core::wow_played as wp;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

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
