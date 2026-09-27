//! Main window (destroyed when hidden to keep idle memory low) and toast window.

use crate::{AppState, ToastPayload, TOAST_EVENT};
use fraglink_core::engine::keys;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
    Window, WindowEvent,
};

/// Classic chassis size in CSS px (the prototype's 290x381), and with the side panel open.
pub const BASE_W: f64 = 290.0;
pub const SIDE_W: f64 = 468.0;
pub const BASE_H: f64 = 381.0;
const TOAST_W: f64 = 226.0;
const TOAST_H: f64 = 78.0;

pub fn zoom(app: &AppHandle) -> f64 {
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    e.tracker
        .store
        .get_setting(keys::ZOOM)
        .ok()
        .flatten()
        .and_then(|z| z.parse::<f64>().ok())
        .unwrap_or(1.5)
        .clamp(1.0, 2.0)
}

fn on_some_monitor(app: &AppHandle, x: i32, y: i32) -> bool {
    app.available_monitors()
        .map(|ms| {
            ms.iter().any(|m| {
                let p = m.position();
                let s = m.size();
                x >= p.x - 50
                    && y >= p.y - 50
                    && x < p.x + s.width as i32 - 50
                    && y < p.y + s.height as i32 - 50
            })
        })
        .unwrap_or(false)
}

fn create_main(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let cfg = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .cloned()
        .expect("main window config");
    let z = zoom(app);
    let w = WebviewWindowBuilder::from_config(app, &cfg)?
        .inner_size(BASE_W * z, BASE_H * z)
        .build()?;
    let pos = *app.state::<AppState>().main_pos.lock().unwrap();
    if let Some((x, y)) = pos {
        if on_some_monitor(app, x, y) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    }
    Ok(w)
}

pub fn show_main(app: &AppHandle) {
    let w = match app.get_webview_window("main") {
        Some(w) => w,
        None => match create_main(app) {
            Ok(w) => w,
            Err(e) => {
                log::error!("could not create main window: {e}");
                return;
            }
        },
    };
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// Close (destroy) the main window; the app keeps running in the tray.
pub fn hide_main(app: &AppHandle) {
    persist_main_pos(app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.close();
    }
}

pub fn toggle_main(app: &AppHandle) {
    match app.get_webview_window("main") {
        Some(w) if w.is_visible().unwrap_or(false) => hide_main(app),
        _ => show_main(app),
    }
}

pub fn persist_main_pos(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pos = *state.main_pos.lock().unwrap();
    if let Some((x, y)) = pos {
        let e = state.engine.lock().unwrap();
        let _ = e.tracker.store.set_setting("main_pos", &format!("{x},{y}"));
    }
}

pub fn resize_main(app: &AppHandle, z: f64, side_open: bool) {
    if let Some(w) = app.get_webview_window("main") {
        let width = if side_open { SIDE_W } else { BASE_W };
        let _ = w.set_size(LogicalSize::new(width * z, BASE_H * z));
    }
}

pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != "main" {
        return;
    }
    match event {
        WindowEvent::Moved(p) => {
            if p.x > -10_000 && p.y > -10_000 {
                // minimised windows report -32000 on Windows
                *window
                    .app_handle()
                    .state::<AppState>()
                    .main_pos
                    .lock()
                    .unwrap() = Some((p.x, p.y));
            }
        }
        WindowEvent::Destroyed => persist_main_pos(window.app_handle()),
        _ => {}
    }
}

/// Bottom-right toast (stand-in for friend toasts until M3). Reuses the window if open.
pub fn show_toast(app: &AppHandle, payload: ToastPayload) {
    *app.state::<AppState>().pending_toast.lock().unwrap() = Some(payload.clone());
    if app.get_webview_window("toast").is_some() {
        let _ = app.emit_to("toast", TOAST_EVENT, payload);
        return;
    }
    let z = zoom(app);
    let (w, h) = (TOAST_W * z, TOAST_H * z);
    let mut builder = WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast.html".into()))
        .title("Fraglink")
        .inner_size(w, h)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(true);
    if let Ok(Some(m)) = app.primary_monitor() {
        let sf = m.scale_factor();
        let wa = m.work_area();
        let x = (wa.position.x as f64 + wa.size.width as f64) / sf - w - 8.0;
        let y = (wa.position.y as f64 + wa.size.height as f64) / sf - h - 8.0;
        builder = builder.position(x, y);
    }
    match builder.build() {
        // Some desktops enforce a bigger minimum window size than requested; keep
        // the whole toast inside the work area using the size we actually got.
        Ok(win) => {
            if let (Ok(Some(m)), Ok(sz)) = (app.primary_monitor(), win.outer_size()) {
                let wa = m.work_area();
                let pad = (8.0 * m.scale_factor()) as i32;
                let x = wa.position.x + wa.size.width as i32 - sz.width as i32 - pad;
                let y = wa.position.y + wa.size.height as i32 - sz.height as i32 - pad;
                let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
            }
        }
        Err(e) => log::warn!("toast window failed: {e}"),
    }
}
