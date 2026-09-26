//! Fraglink tray app (Tauri 2): tray icon + menu, single instance, optional
//! autostart, the 290x381 classic window, a toast window, and the local tracker
//! thread from `fraglink-core`. No network calls and no telemetry by default;
//! the optional account sync (`sync.rs`) only runs after the user signs in.

mod commands;
mod friends;
mod sync;
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
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
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
}

pub struct TrayItems {
    pub pause: CheckMenuItem<tauri::Wry>,
    pub status: Vec<(String, CheckMenuItem<tauri::Wry>)>,
}

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub data_dir: PathBuf,
    pub sim: bool,
    pub pending_toast: Mutex<Option<ToastPayload>>,
    pub main_pos: Mutex<Option<(i32, i32)>>,
    pub tray: Mutex<Option<TrayItems>>,
    pub sync: sync::SyncShared,
    pub friends: friends::FriendsShared,
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

struct Flags {
    sim: bool,
    sim_script: String,
    minimized: bool,
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

pub fn update_tray_tooltip(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (np, paused) = {
        let e = state.engine.lock().unwrap();
        (e.tracker.now_playing(), e.tracker.is_paused())
    };
    let sim = if state.sim { " [simulation]" } else { "" };
    let tip = match (np, paused) {
        (_, true) => format!("Fraglink: tracking paused{sim}"),
        (Some(n), _) => format!(
            "Fraglink: playing {}{}{sim}",
            n.name,
            if n.idle { " (idle)" } else { "" }
        ),
        (None, _) => format!("Fraglink: watching for games{sim}"),
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(&tip));
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let (paused, status) = {
        let e = state.engine.lock().unwrap();
        (
            e.tracker.is_paused(),
            e.tracker
                .store
                .get_setting(keys::STATUS)
                .ok()
                .flatten()
                .unwrap_or_else(|| "online".into()),
        )
    };
    let open = MenuItem::with_id(app, "open", "Open Fraglink", true, None::<&str>)?;
    let st_items: Vec<(String, CheckMenuItem<tauri::Wry>)> = [
        ("online", "Online"),
        ("away", "Away"),
        ("invisible", "Invisible"),
    ]
    .iter()
    .map(|(id, label)| {
        CheckMenuItem::with_id(
            app,
            format!("status:{id}"),
            *label,
            true,
            status == *id,
            None::<&str>,
        )
        .map(|i| (id.to_string(), i))
    })
    .collect::<tauri::Result<_>>()?;
    let st_note = MenuItem::with_id(
        app,
        "status_note",
        "(shared with friends in a later update)",
        false,
        None::<&str>,
    )?;
    let mut st_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = st_items
        .iter()
        .map(|(_, i)| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let sep0 = PredefinedMenuItem::separator(app)?;
    st_refs.push(&sep0);
    st_refs.push(&st_note);
    let status_menu = Submenu::with_id_and_items(app, "status", "Status", true, &st_refs)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause tracking", true, paused, None::<&str>)?;
    let export = MenuItem::with_id(
        app,
        "export",
        "Export my data (zip: JSON + CSV)",
        true,
        None::<&str>,
    )?;
    let data = MenuItem::with_id(app, "data_dir", "Open data folder", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "log_dir", "Open logs folder", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Fraglink", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &sep1,
            &status_menu,
            &pause,
            &sep2,
            &export,
            &data,
            &logs,
            &sep3,
            &quit,
        ],
    )?;

    *state.tray.lock().unwrap() = Some(TrayItems {
        pause,
        status: st_items,
    });

    let icon = app.default_window_icon().cloned().expect("bundle icon");
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Fraglink")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                windows::toggle_main(tray.app_handle());
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
    if let Some(t) = state.tray.lock().unwrap().as_ref() {
        for (id, item) in &t.status {
            let _ = item.set_checked(id == status);
        }
    }
    let _ = app.emit("fraglink://status", status);
    friends::nudge(app);
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    let evs = {
        let mut e = state.engine.lock().unwrap();
        e.set_paused(paused).unwrap_or_default()
    };
    if let Some(t) = state.tray.lock().unwrap().as_ref() {
        let _ = t.pause.set_checked(paused);
    }
    handle_events(app, evs);
    let _ = app.emit("fraglink://paused", paused);
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => windows::show_main(app),
        "pause" => {
            let paused = app
                .state::<AppState>()
                .engine
                .lock()
                .unwrap()
                .tracker
                .is_paused();
            set_paused(app, !paused);
        }
        "export" => match commands::do_export(app) {
            Ok(paths) => log::info!("exported {:?}", paths),
            Err(e) => log::error!("export failed: {e:#}"),
        },
        "data_dir" => {
            let dir = app.state::<AppState>().data_dir.clone();
            let _ = tauri_plugin_opener::open_path(dir, None::<&str>);
        }
        "log_dir" => {
            if let Ok(dir) = app.path().app_log_dir() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = tauri_plugin_opener::open_path(dir, None::<&str>);
            }
        }
        "quit" => quit(app),
        s if s.starts_with("status:") => set_status(app, &s["status:".len()..]),
        _ => {}
    }
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
            let evs = {
                let state = app.state::<AppState>();
                let mut e = state.engine.lock().unwrap();
                e.step(now())
            };
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
            app.manage(AppState {
                engine: Mutex::new(engine),
                data_dir,
                sim: fl.sim,
                pending_toast: Mutex::new(None),
                main_pos: Mutex::new(main_pos),
                tray: Mutex::new(None),
                sync: sync::SyncShared::default(),
                friends: friends::FriendsShared::default(),
            });
            build_tray(&handle)?;
            if !fl.minimized {
                windows::show_main(&handle);
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
            commands::toast_payload,
            commands::toast_clicked,
            commands::set_status,
            commands::quit_app,
            commands::sync_status,
            commands::sync_sign_in,
            commands::sync_cancel,
            commands::sync_now,
            commands::sync_sign_out,
            commands::friends_state,
            commands::friends_refresh,
            commands::get_server_stats,
            commands::friend_add,
            commands::friend_respond,
            commands::friend_remove,
            commands::friend_block,
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
