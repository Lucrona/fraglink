//! Tauri commands called from the webview (`window.__TAURI__.core.invoke`).

use crate::{now, windows, AppState, ToastPayload};
use fraglink_core::detector::Candidate;
use fraglink_core::engine::keys;
use fraglink_core::stats::Glance;
use fraglink_core::store::{ActivityRow, GameRow};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[tauri::command]
pub fn get_glance(state: State<'_, AppState>) -> CmdResult<Glance> {
    state.engine.lock().unwrap().glance(now()).map_err(err)
}

#[tauri::command]
pub fn get_activity(state: State<'_, AppState>, limit: Option<i64>) -> CmdResult<Vec<ActivityRow>> {
    state
        .engine
        .lock()
        .unwrap()
        .tracker
        .store
        .recent_activity(limit.unwrap_or(50).clamp(1, 500))
        .map_err(err)
}

#[tauri::command]
pub fn get_games(state: State<'_, AppState>) -> CmdResult<Vec<GameRow>> {
    state
        .engine
        .lock()
        .unwrap()
        .tracker
        .store
        .list_games()
        .map_err(err)
}

#[tauri::command]
pub fn get_candidates(state: State<'_, AppState>) -> CmdResult<Vec<Candidate>> {
    Ok(state.engine.lock().unwrap().candidates())
}

#[tauri::command]
pub fn tag_process(
    app: AppHandle,
    state: State<'_, AppState>,
    exe: String,
    name: String,
) -> CmdResult<String> {
    let id = state
        .engine
        .lock()
        .unwrap()
        .tag_exe(&exe, &name, now())
        .map_err(err)?;
    log::info!("tagged {exe} as {id}");
    crate::handle_events(&app, vec![]);
    Ok(id)
}

#[tauri::command]
pub fn untag_process(state: State<'_, AppState>, exe: String) -> CmdResult<()> {
    state.engine.lock().unwrap().untag_exe(&exe).map_err(err)
}

#[tauri::command]
pub fn set_game_excluded(
    app: AppHandle,
    state: State<'_, AppState>,
    game_id: String,
    excluded: bool,
) -> CmdResult<()> {
    let evs = state
        .engine
        .lock()
        .unwrap()
        .set_game_excluded(&game_id, excluded, now())
        .map_err(err)?;
    crate::handle_events(&app, evs);
    Ok(())
}

#[derive(Serialize)]
pub struct SettingsView {
    pub settings: BTreeMap<String, String>,
    pub autostart: bool,
    pub sim: bool,
    pub data_dir: String,
    pub log_dir: String,
    pub version: String,
    pub signatures: usize,
    pub signatures_version: String,
    pub steam_apps: usize,
    pub epic_apps: usize,
    pub gog_apps: usize,
    pub idle_threshold_min: i64,
    pub min_session_min: i64,
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> CmdResult<SettingsView> {
    let e = state.engine.lock().unwrap();
    Ok(SettingsView {
        settings: e.tracker.store.all_settings().map_err(err)?,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        sim: state.sim,
        data_dir: state.data_dir.display().to_string(),
        log_dir: app
            .path()
            .app_log_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        version: env!("CARGO_PKG_VERSION").into(),
        signatures: e.detector.sigs.len(),
        signatures_version: e.detector.sigs.version.clone(),
        steam_apps: e.detector.steam.len(),
        epic_apps: e
            .detector
            .libs
            .count(fraglink_core::libraries::Launcher::Epic),
        gog_apps: e
            .detector
            .libs
            .count(fraglink_core::libraries::Launcher::Gog),
        idle_threshold_min: e.tracker.cfg.idle_threshold_s / 60,
        min_session_min: e.tracker.cfg.min_session_s / 60,
    })
}

const ALLOWED: &[&str] = &[
    keys::IDLE_THRESHOLD_MIN,
    keys::MIN_SESSION_MIN,
    keys::PAUSED,
    keys::TOASTS,
    keys::SKIN,
    keys::ZOOM,
    keys::STATUS,
];

#[tauri::command]
pub fn set_setting(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CmdResult<()> {
    if !ALLOWED.contains(&key.as_str()) || value.len() > 64 {
        return Err(format!("unknown setting {key}"));
    }
    match key.as_str() {
        k if k == keys::PAUSED => {
            crate::set_paused(&app, value == "1");
            return Ok(());
        }
        k if k == keys::STATUS => {
            crate::set_status(&app, &value);
            return Ok(());
        }
        _ => {}
    }
    let mut e = state.engine.lock().unwrap();
    e.tracker.store.set_setting(&key, &value).map_err(err)?;
    e.apply_settings().map_err(err)
}

#[tauri::command]
pub fn set_status(app: AppHandle, status: String) -> CmdResult<()> {
    if !["online", "away", "invisible"].contains(&status.as_str()) {
        return Err("bad status".into());
    }
    crate::set_status(&app, &status);
    Ok(())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> CmdResult<bool> {
    let al = app.autolaunch();
    if enabled { al.enable() } else { al.disable() }.map_err(err)?;
    al.is_enabled().map_err(err)
}

/// Write JSON + CSV exports to Documents\Fraglink (fallback: data folder) and reveal them.
pub fn do_export(app: &AppHandle) -> anyhow::Result<(PathBuf, PathBuf)> {
    let state = app.state::<AppState>();
    let dir = app
        .path()
        .document_dir()
        .map(|d| d.join("Fraglink"))
        .unwrap_or_else(|_| state.data_dir.join("exports"));
    let e = state.engine.lock().unwrap();
    let paths = fraglink_core::export::write_export(&e.tracker.store, &dir, now())?;
    drop(e);
    let _ = tauri_plugin_opener::reveal_item_in_dir(&paths.0);
    Ok(paths)
}

#[tauri::command]
pub fn export_data(app: AppHandle) -> CmdResult<Vec<String>> {
    let (j, c) = do_export(&app).map_err(err)?;
    Ok(vec![j.display().to_string(), c.display().to_string()])
}

#[tauri::command]
pub fn open_data_dir(state: State<'_, AppState>) -> CmdResult<()> {
    tauri_plugin_opener::open_path(&state.data_dir, None::<&str>).map_err(err)
}

#[tauri::command]
pub fn hide_main(app: AppHandle) {
    windows::hide_main(&app);
}

#[tauri::command]
pub fn set_window_scale(
    app: AppHandle,
    state: State<'_, AppState>,
    zoom: f64,
    side_open: bool,
) -> CmdResult<()> {
    let z = zoom.clamp(1.0, 2.0);
    state
        .engine
        .lock()
        .unwrap()
        .tracker
        .store
        .set_setting(keys::ZOOM, &z.to_string())
        .map_err(err)?;
    windows::resize_main(&app, z, side_open);
    Ok(())
}

#[tauri::command]
pub fn toast_payload(state: State<'_, AppState>) -> Option<ToastPayload> {
    state.pending_toast.lock().unwrap().clone()
}

#[tauri::command]
pub fn toast_clicked(app: AppHandle) {
    windows::show_main(&app);
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.close();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(&app);
}
