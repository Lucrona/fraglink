//! Tauri commands called from the webview (`window.__TAURI__.core.invoke`).

use crate::{now, windows, AppState, ToastPayload};
use fraglink_core::catalog;
use fraglink_core::detector::Candidate;
use fraglink_core::engine::keys;
use fraglink_core::stats::Glance;
use fraglink_core::store::{ActivityRow, GameRow};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
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

/// The pending "Track <exe> as a game?" question, if any.
#[tauri::command]
pub fn track_prompt(state: State<'_, AppState>) -> Option<Candidate> {
    state.track_prompt.lock().unwrap().clone()
}

/// `OddGame.exe` -> `OddGame`, `my_cool-game.exe` -> `my cool game`.
pub fn name_from_exe(exe: &str) -> String {
    let stem = exe
        .rsplit_once('.')
        .filter(|(_, ext)| ext.eq_ignore_ascii_case("exe"))
        .map(|(s, _)| s)
        .unwrap_or(exe);
    let n = stem.replace(['_', '-'], " ");
    let n = n.split_whitespace().collect::<Vec<_>>().join(" ");
    if n.is_empty() {
        exe.to_string()
    } else {
        n.chars().take(80).collect()
    }
}

/// Answer the question. Yes: track it from now on (manual tag) and report the exe and
/// name to the server for review. No: never ask about this exe again.
#[tauri::command]
pub fn track_prompt_answer(
    app: AppHandle,
    state: State<'_, AppState>,
    exe: String,
    yes: bool,
    name: Option<String>,
) -> CmdResult<Option<String>> {
    let exe = exe.trim().to_string();
    if exe.is_empty() {
        return Err("no program".into());
    }
    let mut out = None;
    if yes {
        let name = name
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| name_from_exe(&exe));
        let id = state
            .engine
            .lock()
            .unwrap()
            .tag_exe(&exe, &name, now())
            .map_err(err)?;
        log::info!("track prompt: {exe} is a game ({id})");
        let app2 = app.clone();
        let (e2, n2) = (exe.clone(), name.clone());
        std::thread::spawn(move || {
            let c = crate::sync::client(&app2)
                .unwrap_or_else(|| fraglink_sync::Client::new(&crate::sync::server(&app2), None));
            if let Err(e) = c.suggest_game(&e2, &n2) {
                log::info!("game suggestion not sent: {e}");
            }
        });
        out = Some(id);
    } else {
        state
            .engine
            .lock()
            .unwrap()
            .dismiss_prompt(&exe)
            .map_err(err)?;
        log::info!("track prompt: {exe} is not a game");
    }
    {
        let mut p = state.track_prompt.lock().unwrap();
        if p.as_ref().is_some_and(|c| c.exe.eq_ignore_ascii_case(&exe)) {
            *p = None;
        }
    }
    let toast_is_prompt = state
        .pending_toast
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|t| t.kind.starts_with("track:"));
    if toast_is_prompt {
        if let Some(w) = app.get_webview_window("toast") {
            let _ = w.close();
        }
    }
    let _ = app.emit(crate::TRACK_PROMPT_EVENT, None::<Candidate>);
    let _ = app.emit("fraglink://catalog", ());
    crate::handle_events(&app, vec![]);
    Ok(out)
}

#[cfg(test)]
mod track_tests {
    #[test]
    fn names_from_exes() {
        use super::name_from_exe;
        assert_eq!(name_from_exe("OddGame.exe"), "OddGame");
        assert_eq!(name_from_exe("my_cool-game.EXE"), "my cool game");
        assert_eq!(name_from_exe("noext"), "noext");
        assert_eq!(name_from_exe(".exe"), ".exe");
    }
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
    crate::refresh_tray(&app);
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
}

#[tauri::command]
pub fn get_settings(app: AppHandle, state: State<'_, AppState>) -> CmdResult<SettingsView> {
    let e = state.engine.lock().unwrap();
    Ok(SettingsView {
        settings: {
            let mut s = e.tracker.store.all_settings().map_err(err)?;
            s.retain(|k, _| !crate::sync::keys::HIDDEN.contains(&k.as_str()));
            s.remove(fraglink_core::folderscan::CACHE_KEY);
            s
        },
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
    })
}

const ALLOWED: &[&str] = &[
    keys::TOASTS,
    keys::SKIN,
    keys::ZOOM,
    keys::STATUS,
    crate::friends::keys::FRIEND_TOASTS,
    crate::friends::keys::FRIEND_TOASTS_DND,
    // 0.2.0-dev.4 Options / menus
    "always_on_top",
    "start_minimized",
    "minimize_to_tray",
    "show_offline",
    "show_fof",
    "share_game",
    "theme",
    // stored but not used yet (see CHANGELOG)
    "nickname",
    "language",
    "share_profile",
    "share_fof",
    "sound_signin",
    "sound_signout",
    "sound_pack",
    "custom_status",
    "custom_groups",
    "hidden_groups",
    "collapsed_groups",
    "games_detected_once",
    "login_history",
    "login_last",
    "save_password",
    "auto_login",
    "status_history",
    "favorite_friends",
    "friend_groups",
    "friend_toasts_logon",
    "connection_alerts",
    // 0.2.0-dev.9
    "side_w",
    "text_size",
    "first_detect_done",
];

/// Automatic Away is on (10 minutes without keyboard/mouse input).
#[tauri::command]
pub fn get_auto_away(state: State<'_, AppState>) -> bool {
    state.auto_away.load(std::sync::atomic::Ordering::SeqCst)
}

/// Per-game settings from Options > Games (`game_path:<id>`, `launch_args:<id>`).
fn per_game_key(key: &str) -> bool {
    [catalog::PATH_KEY, catalog::ARGS_KEY, "detect_mode:"]
        .iter()
        .any(|p| {
            key.strip_prefix(p).is_some_and(|id| {
                !id.is_empty() && id.len() <= 80 && !id.contains(char::is_whitespace)
            })
        })
}

#[tauri::command]
pub fn set_setting(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> CmdResult<()> {
    let per_game = per_game_key(&key);
    let max = if key.ends_with("_groups") || key == "favorite_friends" {
        4096 // small JSON lists / maps
    } else if per_game
        || key.starts_with("custom_")
        || key == "status_history"
        || key == "login_history"
    {
        512
    } else {
        64
    };
    if !(ALLOWED.contains(&key.as_str()) || per_game) || value.len() > max {
        return Err(format!("unknown setting {key}"));
    }
    if key == "always_on_top" {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_always_on_top(value == "1");
        }
    }
    match key.as_str() {
        k if k == keys::STATUS => {
            crate::set_status(&app, &value);
            return Ok(());
        }
        _ => {}
    }
    {
        let mut e = state.engine.lock().unwrap();
        e.tracker.store.set_setting(&key, &value).map_err(err)?;
        e.apply_settings().map_err(err)?;
    }
    if key == "text_size" {
        let open = *state.side_open.lock().unwrap();
        windows::resize_main(&app, windows::zoom(&app), open);
    }
    // let the other windows (main <-> Options) follow the change
    let _ = app.emit("fraglink://settings", (&key, &value));
    if matches!(
        key.as_str(),
        "skin" | "status_history" | "custom_status" | "custom_on"
    ) || key.starts_with("exclude:")
        || key.starts_with("game_path:")
    {
        crate::refresh_tray(&app);
    }
    Ok(())
}

// Commands that can create windows are async: sync commands run on the main
// thread, and building a WebView2 window there deadlocks on Windows.
#[tauri::command]
pub async fn open_options(app: AppHandle, page: Option<String>) {
    windows::show_options(&app, page);
}

/// Drag the info panel wider: remember the width and grow the window to fit.
#[tauri::command]
pub fn set_side_width(app: AppHandle, state: State<'_, AppState>, width: f64) -> CmdResult<()> {
    if !width.is_finite() {
        return Err("bad width".into());
    }
    let w = width.clamp(windows::SIDE_PANEL_MIN, windows::SIDE_PANEL_MAX);
    state
        .engine
        .lock()
        .unwrap()
        .tracker
        .store
        .set_setting("side_w", &format!("{w:.0}"))
        .map_err(err)?;
    let open = *state.side_open.lock().unwrap();
    if open {
        windows::resize_main(&app, windows::zoom(&app), true);
    }
    Ok(())
}

/// One entry of a native popup menu built by the page (friend right-click, status).
#[derive(serde::Deserialize)]
pub struct PopItem {
    #[serde(default)]
    id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    disabled: bool,
    checked: Option<bool>,
    #[serde(default)]
    sep: bool,
    #[serde(default)]
    items: Vec<PopItem>,
}

type MenuItemBox = Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>;

fn pop_items(app: &AppHandle, items: &[PopItem], depth: u8) -> tauri::Result<Vec<MenuItemBox>> {
    use tauri::menu::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};
    let mut out: Vec<MenuItemBox> = Vec::new();
    for it in items.iter().take(40) {
        let id = format!("pm:{}", it.id);
        let label: String = it.label.chars().take(80).collect();
        if it.sep {
            out.push(Box::new(PredefinedMenuItem::separator(app)?));
        } else if !it.items.is_empty() && depth < 2 {
            let sub = pop_items(app, &it.items, depth + 1)?;
            let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
                sub.iter().map(|b| b.as_ref()).collect();
            out.push(Box::new(Submenu::with_items(
                app,
                &label,
                !it.disabled,
                &refs,
            )?));
        } else if let Some(c) = it.checked {
            out.push(Box::new(CheckMenuItem::with_id(
                app,
                id,
                &label,
                !it.disabled,
                c,
                None::<&str>,
            )?));
        } else {
            out.push(Box::new(MenuItem::with_id(
                app,
                id,
                &label,
                !it.disabled,
                None::<&str>,
            )?));
        }
    }
    Ok(out)
}

/// Show a real Windows popup menu at (x, y) in the window (logical px), or at the
/// cursor. The pick comes back as a `fraglink://menu-pick` event with the item id.
#[tauri::command]
pub async fn popup_menu(
    app: AppHandle,
    window: tauri::WebviewWindow,
    items: Vec<PopItem>,
    x: Option<f64>,
    y: Option<f64>,
) -> CmdResult<()> {
    let built = pop_items(&app, &items, 0).map_err(err)?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        built.iter().map(|b| b.as_ref()).collect();
    let menu = tauri::menu::Menu::with_items(&app, &refs).map_err(err)?;
    match (x, y) {
        (Some(x), Some(y)) if x.is_finite() && y.is_finite() => window
            .popup_menu_at(&menu, tauri::LogicalPosition::new(x, y))
            .map_err(err),
        _ => window.popup_menu(&menu).map_err(err),
    }
}

/// Side panel "Activity Report": friends' recent sessions and what they're playing now.
#[tauri::command]
pub async fn friends_activity(app: AppHandle) -> CmdResult<serde_json::Value> {
    account_call(app, |c| c.friends_activity()).await
}

/// Side panel friend details: current game, hours this week.
#[tauri::command]
pub async fn friend_summary(app: AppHandle, handle: String) -> CmdResult<serde_json::Value> {
    let h = clean_handle(&handle)?;
    account_call(app, move |c| c.friend_summary(&h)).await
}

/// Side panel friend details: total hours and recent games (the friend's profile).
#[tauri::command]
pub async fn friend_profile(app: AppHandle, handle: String) -> CmdResult<serde_json::Value> {
    let h = clean_handle(&handle)?;
    account_call(app, move |c| c.user_profile(&h)).await
}

/// Connections > Tracking: share recent play history and weekly hours with friends.
#[tauri::command]
pub async fn set_share_activity(app: AppHandle, on: bool) -> CmdResult<serde_json::Value> {
    account_call(app, move |c| c.set_share_activity(on)).await
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

/// Write the export zip (JSON + CSVs) to Documents\Fraglink (fallback: data folder) and reveal it.
pub fn do_export(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let state = app.state::<AppState>();
    let dir = app
        .path()
        .document_dir()
        .map(|d| d.join("Fraglink"))
        .unwrap_or_else(|_| state.data_dir.join("exports"));
    let e = state.engine.lock().unwrap();
    let paths = fraglink_core::export::write_export(&e.tracker.store, &dir, now())?;
    drop(e);
    let _ = tauri_plugin_opener::reveal_item_in_dir(&paths);
    Ok(paths)
}

#[tauri::command]
pub fn export_data(app: AppHandle) -> CmdResult<Vec<String>> {
    let z = do_export(&app).map_err(err)?;
    Ok(vec![z.display().to_string()])
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
pub async fn toast_clicked(app: AppHandle) {
    let (kind, link) = app
        .state::<AppState>()
        .pending_toast
        .lock()
        .unwrap()
        .as_ref()
        .map(|t| (t.kind.clone(), t.game_id.clone()))
        .unwrap_or_default();
    if kind.starts_with("news:") {
        // staff announcement: open its link (https only), else just close
        if link.starts_with("https://") {
            let _ = tauri_plugin_opener::open_url(link, None::<&str>);
        }
        if let Some(w) = app.get_webview_window("toast") {
            let _ = w.close();
        }
        return;
    }
    windows::show_main(&app);
    if let Some(h) = kind
        .strip_prefix("friend:")
        .or_else(|| kind.strip_prefix("request:"))
    {
        use tauri::Emitter;
        let _ = app.emit("fraglink://show-friend", h);
    }
    if let Some(w) = app.get_webview_window("toast") {
        let _ = w.close();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    crate::quit(&app);
}

// ------------------------------------------------------------ optional sync

#[tauri::command]
pub fn sync_status(app: AppHandle) -> crate::sync::SyncView {
    crate::sync::view(&app)
}

#[tauri::command]
pub fn sync_sign_in(app: AppHandle, server: String) -> CmdResult<()> {
    crate::sync::start_pairing(&app, &server)
}

/// Login screen: username (or email) + password.
#[tauri::command]
pub async fn sync_login(
    app: AppHandle,
    server: String,
    username: String,
    password: String,
) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::sync::password_sign_in(&app, &server, username.trim(), &password, None)
    })
    .await
    .map_err(err)?
}

/// Create Account: username, password, optional email; signs this PC in.
#[tauri::command]
pub async fn sync_signup(
    app: AppHandle,
    server: String,
    username: String,
    password: String,
    email: String,
) -> CmdResult<()> {
    let h = clean_handle(&username)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::sync::password_sign_in(&app, &server, &h, &password, Some(email.trim()))
    })
    .await
    .map_err(err)?
}

/// Add Friend search by username / display name / exact email.
#[tauri::command]
pub async fn friend_search(app: AppHandle, query: String) -> CmdResult<serde_json::Value> {
    let q: String = query.trim().chars().take(64).collect();
    let client = crate::sync::client(&app).ok_or("Log in first.")?;
    tauri::async_runtime::spawn_blocking(move || client.search_users(&q).map_err(|e| e.to_string()))
        .await
        .map_err(err)?
}

#[tauri::command]
pub fn sync_cancel(app: AppHandle) {
    crate::sync::cancel_pairing(&app);
}

#[tauri::command]
pub fn sync_now(app: AppHandle) {
    crate::sync::nudge(&app);
}

#[tauri::command]
pub fn sync_sign_out(app: AppHandle) {
    crate::sync::sign_out(&app);
}

// ------------------------------------------------------------------ friends (M3)

#[tauri::command]
pub fn friends_state(app: AppHandle) -> serde_json::Value {
    crate::friends::view(&app)
}

/// Ask the presence thread to re-fetch the friends list.
#[tauri::command]
pub fn friends_refresh(app: AppHandle) {
    crate::friends::request_refresh(&app);
}

#[tauri::command]
pub fn get_server_stats(app: AppHandle) -> Option<serde_json::Value> {
    crate::sync::server_stats(&app)
}

fn clean_handle(h: &str) -> CmdResult<String> {
    let h = h.trim().trim_start_matches('@').to_string();
    if h.is_empty()
        || h.len() > 40
        || !h
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("Enter a Fraglink username (letters, numbers, _ or -).".into());
    }
    Ok(h)
}

/// Run a friends API call off the UI thread, then refresh the cached list.
async fn friend_call<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&fraglink_sync::Client) -> Result<T, fraglink_sync::SyncError> + Send + 'static,
) -> CmdResult<T> {
    let client = crate::sync::client(&app).ok_or("Sign in to sync first (Options).")?;
    tauri::async_runtime::spawn_blocking(move || {
        let r = f(&client).map_err(|e| e.to_string());
        if r.is_ok() {
            if let Ok(v) = client.friends() {
                *app.state::<AppState>().friends.list.lock().unwrap() = Some(v);
                crate::friends::emit(&app);
            }
        }
        r
    })
    .await
    .map_err(err)?
}

/// Account calls: the server's own message on a refusal ("Your current password isn't
/// right.") rather than "server error 403: ...".
async fn account_call<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&fraglink_sync::Client) -> Result<T, fraglink_sync::SyncError> + Send + 'static,
) -> CmdResult<T> {
    let client = crate::sync::client(&app).ok_or("Sign in first.")?;
    tauri::async_runtime::spawn_blocking(move || {
        f(&client).map_err(|e| match e {
            fraglink_sync::SyncError::Server { message, .. } if !message.is_empty() => message,
            e => e.to_string(),
        })
    })
    .await
    .map_err(err)?
}

/// Options > Account: `{handle, email, email_verified}` from the server.
#[tauri::command]
pub async fn account_info(app: AppHandle) -> CmdResult<serde_json::Value> {
    account_call(app, |c| {
        let v = c.me_raw()?;
        let u = &v["user"];
        Ok(serde_json::json!({
            "handle": u["handle"], "email": u["email"], "email_verified": u["email_verified"],
            "share_activity": u["share_activity"],
        }))
    })
    .await
}

#[tauri::command]
pub async fn account_change_password(
    app: AppHandle,
    current: String,
    new: String,
) -> CmdResult<serde_json::Value> {
    if new.chars().count() < 10 {
        return Err("Use a new password of at least 10 characters.".into());
    }
    account_call(app, move |c| c.change_password(&current, &new)).await
}

#[tauri::command]
pub async fn account_change_email(
    app: AppHandle,
    current: String,
    email: String,
) -> CmdResult<serde_json::Value> {
    account_call(app, move |c| c.change_email(&current, email.trim())).await
}

#[tauri::command]
pub async fn friend_add(app: AppHandle, handle: String) -> CmdResult<String> {
    let h = clean_handle(&handle)?;
    friend_call(app, move |c| c.friend_request(&h)).await
}

#[tauri::command]
pub async fn friend_respond(app: AppHandle, handle: String, accept: bool) -> CmdResult<()> {
    let h = clean_handle(&handle)?;
    friend_call(app, move |c| c.friend_respond(&h, accept)).await
}

#[tauri::command]
pub async fn friend_remove(app: AppHandle, handle: String) -> CmdResult<()> {
    let h = clean_handle(&handle)?;
    friend_call(app, move |c| c.friend_remove(&h)).await
}

#[tauri::command]
pub async fn friend_block(app: AppHandle, handle: String, block: bool) -> CmdResult<()> {
    let h = clean_handle(&handle)?;
    friend_call(
        app,
        move |c| if block { c.block(&h) } else { c.unblock(&h) },
    )
    .await
}

#[tauri::command]
pub fn friend_mute(app: AppHandle, handle: String, mute: bool) -> CmdResult<()> {
    let h = clean_handle(&handle)?;
    crate::friends::set_muted(&app, &h, mute);
    crate::friends::emit(&app);
    Ok(())
}

/// Game icons as `data:` URLs (null = draw the generic icon). Max 60 ids.
#[tauri::command]
pub async fn game_icons(
    app: AppHandle,
    ids: Vec<String>,
    friends: Option<bool>,
) -> CmdResult<std::collections::HashMap<String, Option<String>>> {
    let ids: Vec<String> = ids
        .into_iter()
        .filter(|i| !i.is_empty() && i.len() <= 120)
        .take(60)
        .collect();
    // Same priority for friends' games and our own (server, Steam, exe); `friends`
    // is kept for older UI builds.
    let _ = friends;
    tauri::async_runtime::spawn_blocking(move || crate::icons::resolve(&app, &ids, true))
        .await
        .map_err(err)
}

pub(crate) fn catalog_now(state: &AppState) -> Vec<catalog::Entry> {
    let e = state.engine.lock().unwrap();
    let games = e.tracker.store.list_games().unwrap_or_default();
    let settings = e.tracker.store.all_settings().unwrap_or_default();
    let mut out = catalog::build(
        &e.detector.sigs,
        &e.detector.steam,
        &e.detector.libs,
        &games,
        &settings,
    );
    // staff renames (override rules) win over the bundled signature names
    for g in out.iter_mut() {
        if let Some(n) = e.override_name(&g.id) {
            if g.source != "manual" && n != g.name {
                g.name = n.to_string();
            }
        }
    }
    out
}

/// Options > Games and Tools > Launch: known games, installed first.
#[tauri::command]
pub fn games_catalog(state: State<'_, AppState>) -> Vec<catalog::Entry> {
    catalog_now(&state)
}

/// "Detect Games": re-read the Steam / Epic / GOG install lists.
#[tauri::command]
pub async fn detect_games(app: AppHandle) -> Vec<catalog::Entry> {
    let state = app.state::<AppState>();
    fraglink_core::installs::clear_exe_cache();
    if !state.sim {
        state.engine.lock().unwrap().refresh_steam(now());
    }
    crate::refresh_tray(&app);
    catalog_now(&state)
}

static SCANNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// "Scan for games": walk the usual game folders (background priority, bounded,
/// never holding the engine lock while reading the disk) and cache what matches.
/// Local only. Returns None when a scan is already running.
pub(crate) fn run_folder_scan(app: &AppHandle) -> Option<usize> {
    use std::sync::atomic::Ordering;
    if SCANNING.swap(true, Ordering::SeqCst) {
        return None;
    }
    let state = app.state::<AppState>();
    let roots = state.engine.lock().unwrap().folder_scan_roots();
    fraglink_core::folderscan::background_priority();
    let started = std::time::Instant::now();
    let (exes, dirs) = fraglink_core::folderscan::walk_exes(
        &roots,
        &fraglink_core::folderscan::ScanOpts::default(),
    );
    log::info!(
        "folder scan: {} roots, {} folders in {:.1}s",
        roots.len(),
        dirs,
        started.elapsed().as_secs_f32()
    );
    let n = state
        .engine
        .lock()
        .unwrap()
        .apply_folder_scan(&exes, dirs, now());
    SCANNING.store(false, Ordering::SeqCst);
    crate::refresh_tray(app);
    let _ = app.emit("games-changed", ());
    Some(n)
}

/// First start: one background folder scan after the app has settled.
pub(crate) fn start_first_folder_scan(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.sim || state.engine.lock().unwrap().folder_scan_cache().is_some() {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("folder-scan".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(45));
            run_folder_scan(&app);
        })
        .ok();
}

/// Options > Games "Scan for games".
#[tauri::command]
pub async fn scan_games(app: AppHandle) -> CmdResult<Vec<catalog::Entry>> {
    if app.state::<AppState>().sim {
        return Ok(catalog_now(&app.state::<AppState>()));
    }
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || run_folder_scan(&a))
        .await
        .map_err(err)?
        .ok_or("A scan is already running.")?;
    Ok(catalog_now(&app.state::<AppState>()))
}

/// Start an installed game (Steam/Epic via their launchers, else its exe).
#[tauri::command]
pub async fn launch_game(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let entries = catalog_now(&state);
    let entry = entries.iter().find(|e| e.id == id).ok_or("unknown game")?;
    let how = {
        let e = state.engine.lock().unwrap();
        catalog::launch_for(entry, &e.detector.libs)
    };
    match how.ok_or(
        "Fraglink doesn't know how to start this game yet. Set its path in Options > Games.",
    )? {
        catalog::Launch::Url(u) => tauri_plugin_opener::open_url(u, None::<&str>).map_err(err),
        catalog::Launch::Exe { path, args } => {
            let p = std::path::Path::new(&path);
            let mut c = std::process::Command::new(p);
            c.args(&args);
            if let Some(dir) = p.parent() {
                c.current_dir(dir);
            }
            c.spawn()
                .map(|_| ())
                .map_err(|e| format!("could not start {path}: {e}"))
        }
    }
}

/// Options > Games "Browse…": pick the game's .exe (async, so the dialog runs
/// off the main thread).
#[tauri::command]
pub async fn pick_game_exe(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .file()
        .set_title("Choose the game's program")
        .add_filter("Programs", &["exe"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
}

/// Placeholder address of the Fraglink website (Fraglink > My Stats).
pub const WEBSITE: &str = "https://playfraglink.com";

/// Open a page of the website in the default browser.
#[tauri::command]
pub async fn open_website(page: String) -> CmdResult<()> {
    let page: String = page
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_'))
        .take(100)
        .collect();
    tauri_plugin_opener::open_url(
        format!("{WEBSITE}/{}", page.trim_start_matches('/')),
        None::<&str>,
    )
    .map_err(err)
}

/// Right-click > Join Game: `steam://connect/<ip:port>` for a friend's server.
#[tauri::command]
pub async fn join_game(address: String) -> CmdResult<()> {
    let ok = address.len() <= 64
        && address.split_once(':').is_some_and(|(ip, port)| {
            ip.parse::<std::net::Ipv4Addr>().is_ok() && port.parse::<u16>().is_ok()
        });
    if !ok {
        return Err("not a joinable server address".into());
    }
    tauri_plugin_opener::open_url(format!("steam://connect/{address}"), None::<&str>).map_err(err)
}
