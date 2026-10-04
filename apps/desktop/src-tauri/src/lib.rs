//! Fraglink tray app (Tauri 2): tray icon + menu, single instance, optional
//! autostart, the 290x381 classic window, a toast window, and the local tracker
//! thread from `fraglink-core`. No network calls and no telemetry by default;
//! the optional account sync (`sync.rs`) only runs after the user signs in.

mod commands;
mod friends;
mod icons;
mod sync;
mod updater;
mod windows;

use fraglink_core::engine::{keys, Engine, DEFAULT_POLL_SECS};
use fraglink_core::env::{Environment, SystemEnv};
use fraglink_core::sim::{SimEnv, DEFAULT_SCRIPT};
use fraglink_core::store::Store;
use fraglink_core::tracker::{local_day_fn, TrackerEvent};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::menu::{CheckMenuItem, IconMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

pub const EVENTS: &str = "fraglink://events";
pub const TOAST_EVENT: &str = "fraglink://toast";

#[derive(Debug, Clone, Serialize)]
pub struct ToastPayload {
    pub kind: String,
    pub title: String,
    pub line: String,
    pub game_id: String,
    pub game: String,
    /// `data:` URL of the game icon (filled in by `show_toast` if empty).
    pub icon: Option<String>,
}

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub data_dir: PathBuf,
    pub sim: bool,
    pub pending_toast: Mutex<Option<ToastPayload>>,
    pub main_pos: Mutex<Option<(i32, i32)>>,
    /// Dark-skin window size (logical px, without the info panel).
    pub main_size: Mutex<Option<(f64, f64)>>,
    pub side_open: Mutex<bool>,
    pub sync: sync::SyncShared,
    pub friends: friends::FriendsShared,
    /// Automatic Away: no keyboard/mouse input for [`AUTO_AWAY_SECS`].
    pub auto_away: std::sync::atomic::AtomicBool,
}

/// Like Discord: Away after 10 minutes without input (not a setting). Only replaces
/// "Online"; a status the user picked (Away, Invisible) is left alone.
///
/// TEMPORARY (0.2.0-dev.11 test build only): 20 seconds so auto-away is quick to test.
/// dev.12 MUST set this back to `10 * 60`.
pub const AUTO_AWAY_SECS: u64 = 20;

/// Update the automatic Away flag; tells the UI and friends when it flips.
fn update_auto_away(app: &AppHandle, idle_s: u64) {
    use std::sync::atomic::Ordering;
    let away = idle_s >= AUTO_AWAY_SECS;
    let st = app.state::<AppState>();
    if st.auto_away.swap(away, Ordering::SeqCst) != away {
        log::info!("auto-away: {}", if away { "away" } else { "back" });
        let _ = app.emit("fraglink://auto-away", away);
        friends::nudge(app);
    }
}

/// Running with the fake process source (`--sim`).
pub fn is_sim(app: &AppHandle) -> bool {
    app.try_state::<AppState>().is_some_and(|s| s.sim)
}

/// "0.2.0-10" -> "0.2.0-dev.10" (how the release is named).
pub fn dev_version(v: &str) -> String {
    match v.split_once('-') {
        Some((base, n)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => {
            format!("{base}-dev.{n}")
        }
        _ => v.to_string(),
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

struct Flags {
    sim: bool,
    sim_script: String,
    minimized: bool,
    /// `--no-splash`: open the main window straight away (development).
    no_splash: bool,
}

fn flags() -> Flags {
    let args: Vec<String> = std::env::args().collect();
    let sim =
        args.iter().any(|a| a == "--sim") || std::env::var("FRAGLINK_SIM").is_ok_and(|v| v == "1");
    let sim_script = args
        .iter()
        .position(|a| a == "--sim-script")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("FRAGLINK_SIM_SCRIPT").ok())
        .unwrap_or_else(|| DEFAULT_SCRIPT.to_string());
    Flags {
        sim,
        sim_script,
        minimized: args.iter().any(|a| a == "--minimized"),
        no_splash: args.iter().any(|a| a == "--no-splash"),
    }
}

fn fmt_hm(secs: i64) -> String {
    let m = secs / 60;
    if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

/// React to tracker events: notify the UI, show toasts, update the tray tooltip.
pub fn handle_events(app: &AppHandle, evs: Vec<TrackerEvent>) {
    let state = app.state::<AppState>();
    if !evs.is_empty() {
        let _ = app.emit(EVENTS, &evs);
        friends::nudge(app);
    }
    let toasts_on = {
        let e = state.engine.lock().unwrap();
        e.tracker
            .store
            .get_setting(keys::TOASTS)
            .ok()
            .flatten()
            .as_deref()
            != Some("0")
    };
    for ev in &evs {
        let toast = match ev {
            TrackerEvent::Started {
                game_id,
                name,
                resumed: false,
                first_time,
                ..
            } => Some(ToastPayload {
                kind: "started".into(),
                title: "You".into(),
                line: if *first_time {
                    "started playing (first time on Fraglink)".into()
                } else {
                    "have started playing".into()
                },
                game_id: game_id.clone(),
                game: name.clone(),
                icon: None,
            }),
            TrackerEvent::Milestone {
                game_id,
                name,
                hours,
            } => Some(ToastPayload {
                kind: "milestone".into(),
                title: "You".into(),
                line: format!("hit {hours} hours in"),
                game_id: game_id.clone(),
                game: name.clone(),
                icon: None,
            }),
            TrackerEvent::Stopped {
                game_id,
                name,
                active_seconds,
                kept: true,
                ..
            } => Some(ToastPayload {
                kind: "stopped".into(),
                title: "You".into(),
                line: format!("played {} of", fmt_hm(*active_seconds)),
                game_id: game_id.clone(),
                game: name.clone(),
                icon: None,
            }),
            _ => None,
        };
        if let (true, Some(t)) = (toasts_on, toast) {
            windows::show_toast(app, t);
        }
    }
    if evs
        .iter()
        .any(|e| matches!(e, TrackerEvent::Stopped { kept: true, .. }))
    {
        sync::nudge(app);
    }
    update_tray_tooltip(app);
}

/// The tray tooltip is just "Fraglink" (0.2.0-dev.9: the playing / paused detail
/// lives in the main window; a simulation build is still labelled).
pub fn update_tray_tooltip(app: &AppHandle) {
    let tip = if app.state::<AppState>().sim {
        "Fraglink [simulation]"
    } else {
        "Fraglink"
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// A 16 px menu icon from a cached game icon (`data:image/png;base64,...`).
pub fn menu_icon(data_url: &str) -> Option<tauri::image::Image<'static>> {
    use base64::Engine as _;
    let b64 = data_url.strip_prefix("data:image/png;base64,")?;
    let png = base64::engine::general_purpose::STANDARD.decode(b64).ok()?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let img = if img.width() == 16 && img.height() == 16 {
        img
    } else {
        image::imageops::resize(&img, 16, 16, image::imageops::FilterType::Triangle)
    };
    Some(tauri::image::Image::new_owned(img.into_raw(), 16, 16))
}

fn get_setting(app: &AppHandle, k: &str) -> Option<String> {
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    e.tracker.store.get_setting(k).ok().flatten()
}

/// Save a setting from the tray and let the windows follow it.
fn put_setting(app: &AppHandle, k: &str, v: &str) {
    {
        let state = app.state::<AppState>();
        let mut e = state.engine.lock().unwrap();
        let _ = e.tracker.store.set_setting(k, v);
        let _ = e.apply_settings();
    }
    let _ = app.emit("fraglink://settings", (k, v));
}

/// "Looking to Play" is a preset custom status (shown with the Online status).
pub const LOOKING: &str = "Looking to Play";

const TRAY_SKINS: &[(&str, &str)] = &[("dark", "Dark"), ("classic", "Classic Silver")];

/// The classic tray menu: Launch >, Skin >, Status >, Log Off..., Exit.
type Item = Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>;
fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    fn refs(v: &[Item]) -> Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> {
        v.iter().map(|b| b.as_ref()).collect()
    }
    // Launch: installed games A-Z, then Detect Games
    let mut games: Vec<(String, String)> = commands::catalog_now(&app.state::<AppState>())
        .into_iter()
        .filter(|g| g.installed && g.can_launch && !g.excluded)
        .map(|g| (g.name, g.id))
        .collect();
    games.sort_by_key(|g| g.0.to_lowercase());
    let mut launch: Vec<Item> = Vec::new();
    if games.is_empty() {
        launch.push(Box::new(MenuItem::with_id(
            app,
            "none",
            "No games found",
            false,
            None::<&str>,
        )?));
    }
    // Game icons next to the entries (cached icons only: never blocks on the network).
    let ids: Vec<String> = games.iter().take(40).map(|g| g.1.clone()).collect();
    let icons = icons::resolve(app, &ids, false);
    for (name, id) in games.iter().take(40) {
        let icon = icons.get(id).cloned().flatten().and_then(|u| menu_icon(&u));
        launch.push(Box::new(IconMenuItem::with_id(
            app,
            format!("launch:{id}"),
            name,
            true,
            icon,
            None::<&str>,
        )?));
    }
    launch.push(Box::new(PredefinedMenuItem::separator(app)?));
    launch.push(Box::new(MenuItem::with_id(
        app,
        "detect",
        "Detect Games",
        true,
        None::<&str>,
    )?));
    let launch = Submenu::with_id_and_items(app, "launch", "Launch", true, &refs(&launch))?;
    // Skin
    let cur = get_setting(app, "skin").unwrap_or_else(|| "dark".into());
    let mut skins: Vec<Item> = Vec::new();
    for (id, label) in TRAY_SKINS {
        skins.push(Box::new(CheckMenuItem::with_id(
            app,
            format!("skin:{id}"),
            *label,
            true,
            cur == *id,
            None::<&str>,
        )?));
    }
    skins.push(Box::new(PredefinedMenuItem::separator(app)?));
    skins.push(Box::new(MenuItem::with_id(
        app,
        "more_skins",
        "More Skins...",
        true,
        None::<&str>,
    )?));
    let skin = Submenu::with_id_and_items(app, "skin", "Skin", true, &refs(&skins))?;
    // Status
    let status = get_setting(app, keys::STATUS).unwrap_or_else(|| "online".into());
    let custom = get_setting(app, "custom_status").unwrap_or_default();
    let custom_on = get_setting(app, "custom_on").as_deref() == Some("1") && !custom.is_empty();
    let hist: Vec<String> = get_setting(app, "status_history")
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_default();
    let online = status == "online";
    let looking = online && custom_on && custom == LOOKING;
    let mut st: Vec<Item> = vec![
        Box::new(CheckMenuItem::with_id(
            app,
            "st:reset",
            "Online",
            true,
            online && !custom_on,
            None::<&str>,
        )?),
        Box::new(CheckMenuItem::with_id(
            app,
            "st:away",
            "Away",
            true,
            status == "away",
            None::<&str>,
        )?),
        Box::new(CheckMenuItem::with_id(
            app,
            "st:invisible",
            "Invisible",
            true,
            status == "invisible",
            None::<&str>,
        )?),
        Box::new(CheckMenuItem::with_id(
            app,
            "st:looking",
            LOOKING,
            true,
            looking,
            None::<&str>,
        )?),
        Box::new(MenuItem::with_id(
            app,
            "st:music",
            "Music Status (coming soon)",
            false,
            None::<&str>,
        )?),
    ];
    let hist: Vec<&String> = hist.iter().filter(|h| h.as_str() != LOOKING).collect();
    if !hist.is_empty() {
        st.push(Box::new(PredefinedMenuItem::separator(app)?));
    }
    for (i, h) in hist.iter().take(5).enumerate() {
        let on = online && custom_on && **h == custom;
        st.push(Box::new(CheckMenuItem::with_id(
            app,
            format!("st:h:{i}"),
            *h,
            true,
            on,
            None::<&str>,
        )?));
    }
    let status = Submenu::with_id_and_items(app, "status", "Status", true, &refs(&st))?;
    let sep = PredefinedMenuItem::separator(app)?;
    let logoff = MenuItem::with_id(app, "logoff", "Log Off...", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Fraglink", true, None::<&str>)?;
    let sep0 = PredefinedMenuItem::separator(app)?;
    // "Check for Updates" (dev.10); once an update is downloaded it becomes
    // "Restart to Update" at the top instead.
    if updater::ready_version(app).is_some() {
        let upd = MenuItem::with_id(app, "update", "Restart to Update", true, None::<&str>)?;
        return Menu::with_items(
            app,
            &[
                &open, &upd, &sep0, &launch, &skin, &status, &sep, &logoff, &exit,
            ],
        );
    }
    let chk = MenuItem::with_id(
        app,
        "check_updates",
        "Check for Updates",
        true,
        None::<&str>,
    )?;
    Menu::with_items(
        app,
        &[
            &open, &sep0, &launch, &skin, &status, &sep, &chk, &logoff, &exit,
        ],
    )
}

/// Rebuild the tray menu (games, skin or statuses changed).
pub fn refresh_tray(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// Tray icons: the silver app-icon design, drawn for each size Windows uses for the
/// notification area (16 px at 100 %, 20 at 125 %, 24 at 150 %, 32 at 200 % scaling).
const TRAY_PNGS: [(u32, &[u8]); 4] = [
    (16, include_bytes!("../icons/tray-16.png")),
    (20, include_bytes!("../icons/tray-20.png")),
    (24, include_bytes!("../icons/tray-24.png")),
    (32, include_bytes!("../icons/tray-32.png")),
];

/// The small-icon size of the primary display (what the tray draws at).
fn tray_size() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
        let n = unsafe { GetSystemMetrics(SM_CXSMICON) };
        if n > 0 {
            return n as u32;
        }
    }
    32
}

/// The tray image for the current display scale, with a small green dot in the
/// bottom-right corner while an update is ready.
fn tray_image(update_ready: bool) -> Option<tauri::image::Image<'static>> {
    let want = tray_size();
    // exact size, else the next bigger one (Windows scales down more cleanly than up)
    let (_, bytes) = TRAY_PNGS
        .iter()
        .find(|(n, _)| *n >= want)
        .unwrap_or(&TRAY_PNGS[3]);
    let mut img = image::load_from_memory(bytes).ok()?.to_rgba8();
    if update_ready {
        let (w, h) = img.dimensions();
        let n = w.min(h) as f32;
        let r = (n * 0.2).max(3.0); // 3.2 px at 16, 6.4 at 32
        let ring = (n / 16.0).max(1.0);
        let (cx, cy) = (w as f32 - r - ring, h as f32 - r - ring);
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                // anti-aliased: green inside, a dark ring so it reads on light and dark taskbars
                let cov = |edge: f32| (edge - d + 0.5).clamp(0.0, 1.0);
                let (outer, inner) = (cov(r + ring), cov(r));
                if outer <= 0.0 {
                    continue;
                }
                let top = y as f32 + 0.5 < cy - r * 0.2; // lighter top half, like the title-bar arrow
                let g: [f32; 3] = if top {
                    [96.0, 226.0, 124.0]
                } else {
                    [52.0, 196.0, 88.0]
                };
                let dark = [12.0, 30.0, 18.0];
                let p = img.get_pixel_mut(x, y);
                for ((ch, d), gc) in p.0.iter_mut().take(3).zip(dark).zip(g) {
                    let c = d * (1.0 - inner) + gc * inner;
                    *ch = (*ch as f32 * (1.0 - outer) + c * outer).round() as u8;
                }
                p.0[3] = (p.0[3] as f32 + (255.0 - p.0[3] as f32) * outer).round() as u8;
            }
        }
    }
    let (w, h) = img.dimensions();
    Some(tauri::image::Image::new_owned(img.into_raw(), w, h))
}

/// Switch the tray icon's update dot on or off.
pub fn set_tray_update_dot(app: &AppHandle, on: bool) {
    if let (Some(tray), Some(img)) = (app.tray_by_id("main"), tray_image(on)) {
        let _ = tray.set_icon(Some(img));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app)?;
    // Same silver design as the app icon, at the size the taskbar wants.
    let mut tb = TrayIconBuilder::with_id("main");
    if let Some(icon) = tray_image(updater::ready_version(app).is_some()) {
        tb = tb.icon(icon);
    }
    tb.tooltip("Fraglink")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Single or double left click always brings the window up (created if
            // it was closed to the tray, restored if minimised, then focused).
            // It used to toggle, so the two clicks of a double-click opened and
            // closed it again, and a minimised window counted as "visible".
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => windows::show_main(tray.app_handle()),
                _ => {}
            }
        })
        .build(app)?;
    update_tray_tooltip(app);
    Ok(())
}

pub fn set_status(app: &AppHandle, status: &str) {
    let state = app.state::<AppState>();
    {
        let e = state.engine.lock().unwrap();
        let _ = e.tracker.store.set_setting(keys::STATUS, status);
    }
    refresh_tray(app);
    let _ = app.emit("fraglink://status", status);
    friends::nudge(app);
}

fn on_menu(app: &AppHandle, id: &str) {
    let app2 = app.clone();
    match id {
        "exit" => quit(app),
        "open" => windows::show_main(app),
        "st:looking" => {
            put_setting(app, "custom_status", LOOKING);
            put_setting(app, "custom_on", "1");
            set_status(app, "online");
            refresh_tray(app);
        }
        // native popup menus from the main window (friend right-click, status)
        s if s.starts_with("pm:") => {
            let _ = app.emit("fraglink://menu-pick", &s[3..]);
        }
        "update" => updater::install_from_tray(app),
        "check_updates" => updater::check_from_tray(app),
        "logoff" => {
            windows::show_main(app);
            let _ = app.emit("fraglink://logoff", ());
        }
        "detect" => {
            tauri::async_runtime::spawn(async move {
                commands::detect_games(app2.clone()).await;
                let _ = app2.emit("fraglink://catalog", ());
            });
        }
        "more_skins" => {
            tauri::async_runtime::spawn(commands::open_website("skins".into()));
        }
        "st:reset" => {
            put_setting(app, "custom_on", "0");
            set_status(app, "online");
        }
        "st:away" => set_status(app, "away"),
        "st:invisible" => set_status(app, "invisible"),
        s if s.starts_with("st:h:") => {
            let hist: Vec<String> = get_setting(app, "status_history")
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default();
            if let Some(h) = s[5..].parse::<usize>().ok().and_then(|i| hist.get(i)) {
                put_setting(app, "custom_status", h);
                put_setting(app, "custom_on", "1");
                set_status(app, "online");
            }
        }
        s if s.starts_with("skin:") => {
            put_setting(app, "skin", &s[5..]);
            refresh_tray(app);
        }
        s if s.starts_with("launch:") => {
            let id = s[7..].to_string();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::launch_game(app2, id).await {
                    log::warn!("tray launch: {e}");
                }
            });
        }
        _ => {}
    }
}

/// Close sessions cleanly before an update replaces the app.
pub fn stop_for_update(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut e) = state.engine.lock() {
        let _ = e.tracker.stop();
    }
    windows::persist_main_pos(app);
}

pub fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut e) = state.engine.lock() {
        match e.tracker.stop() {
            Ok(evs) => log::info!("quit: closed {} session(s)", evs.len()),
            Err(err) => log::error!("quit: {err:#}"),
        }
    }
    windows::persist_main_pos(app);
    app.exit(0);
}

fn start_tracker(app: AppHandle) {
    std::thread::Builder::new()
        .name("fraglink-tracker".into())
        .spawn(move || loop {
            let (evs, idle) = {
                let state = app.state::<AppState>();
                let mut e = state.engine.lock().unwrap();
                let evs = e.step(now());
                (evs, e.idle_seconds())
            };
            update_auto_away(&app, idle);
            match evs {
                Ok(evs) => handle_events(&app, evs),
                Err(err) => log::error!("tracker step failed: {err:#}"),
            }
            std::thread::sleep(Duration::from_secs(DEFAULT_POLL_SECS));
        })
        .expect("spawn tracker thread");
}

pub fn run() {
    let fl = flags();
    let log_level = if cfg!(debug_assertions) {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            windows::show_main(app)
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("fraglink".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .level(log_level)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updater::UpdateState::default())
        .setup(move |app| {
            let handle = app.handle().clone();
            let base = app.path().app_data_dir()?;
            let data_dir = if fl.sim { base.join("sim") } else { base };
            std::fs::create_dir_all(&data_dir)?;
            let store = Store::open(&data_dir.join("fraglink.db"))?;
            let env: Box<dyn Environment> = if fl.sim {
                log::warn!(
                    "SIMULATION MODE: fake processes, script = {}",
                    fl.sim_script
                );
                Box::new(SimEnv::realtime(&fl.sim_script)?)
            } else {
                Box::new(SystemEnv::new())
            };
            let mut engine = Engine::new(store, env, local_day_fn())?;
            let recovered = engine.tracker.recover()?;
            log::info!(
                "Fraglink {} starting: data={} env={} signatures={} ({}) recovered={}",
                env!("CARGO_PKG_VERSION"),
                data_dir.display(),
                engine.env_label(),
                engine.detector.sigs.len(),
                engine.detector.sigs.version,
                recovered.len()
            );
            let main_pos = engine
                .tracker
                .store
                .get_setting("main_pos")
                .ok()
                .flatten()
                .and_then(|s| {
                    let (x, y) = s.split_once(',')?;
                    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
                });
            // 0.2.0-dev.2: the dark resizable window becomes the default skin once
            // (earlier builds saved "classic" on every start).
            if engine
                .tracker
                .store
                .get_setting("ui_rev")
                .ok()
                .flatten()
                .is_none()
            {
                let _ = engine
                    .tracker
                    .store
                    .set_setting(fraglink_core::engine::keys::SKIN, "dark");
                let _ = engine.tracker.store.set_setting("ui_rev", "2");
            }
            let main_size = engine
                .tracker
                .store
                .get_setting("main_size")
                .ok()
                .flatten()
                .and_then(|s| windows::parse_size(&s));
            app.manage(AppState {
                engine: Mutex::new(engine),
                data_dir,
                sim: fl.sim,
                pending_toast: Mutex::new(None),
                main_pos: Mutex::new(main_pos),
                main_size: Mutex::new(main_size),
                side_open: Mutex::new(false),
                sync: sync::SyncShared::default(),
                friends: friends::FriendsShared::default(),
                auto_away: Default::default(),
            });
            build_tray(&handle)?;
            let start_min = windows::flag(&handle, "start_minimized");
            let show = !fl.minimized && !start_min;
            if show && !fl.no_splash {
                // Startup window: check for updates first, then open the client.
                windows::show_splash(&handle);
                updater::start_with_splash(handle.clone());
            } else {
                // Started to the tray (autostart / start minimised), like Discord's
                // "Start Minimized": no startup window. An update downloaded last time
                // (and ignored) is applied now; if that fails the window is shown.
                let apply_now = !fl.sim && updater::pending_on_start(&handle);
                if !fl.sim {
                    updater::start(handle.clone(), apply_now, show);
                }
                if show && !apply_now {
                    windows::show_main(&handle);
                }
            }
            sync::init(&handle);
            start_tracker(handle);
            Ok(())
        })
        .on_window_event(windows::on_window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_glance,
            commands::get_activity,
            commands::get_games,
            commands::get_candidates,
            commands::tag_process,
            commands::untag_process,
            commands::set_game_excluded,
            commands::get_settings,
            commands::set_setting,
            commands::set_autostart,
            commands::export_data,
            commands::open_data_dir,
            commands::hide_main,
            commands::set_window_scale,
            commands::open_options,
            commands::games_catalog,
            commands::detect_games,
            commands::launch_game,
            commands::pick_game_exe,
            commands::open_website,
            commands::join_game,
            commands::toast_payload,
            commands::toast_clicked,
            commands::set_status,
            commands::set_side_width,
            commands::popup_menu,
            commands::friends_activity,
            commands::friend_summary,
            commands::set_share_activity,
            commands::sync_login,
            commands::sync_signup,
            commands::friend_search,
            updater::update_available,
            updater::install_update,
            updater::check_for_updates,
            commands::get_auto_away,
            commands::quit_app,
            commands::sync_status,
            commands::sync_sign_in,
            commands::sync_cancel,
            commands::sync_now,
            commands::sync_sign_out,
            commands::friends_state,
            commands::game_icons,
            commands::friends_refresh,
            commands::get_server_stats,
            commands::friend_add,
            commands::friend_respond,
            commands::friend_remove,
            commands::friend_block,
            commands::account_info,
            commands::account_change_password,
            commands::account_change_email,
            commands::friend_mute,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Fraglink");

    app.run(|app, event| {
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            // Last window closed: keep running in the tray.
            api.prevent_exit();
            let _ = app;
        }
    });
}
