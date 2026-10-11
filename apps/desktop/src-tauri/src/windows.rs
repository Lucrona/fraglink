//! Main window (destroyed when hidden to keep idle memory low) and toast window.

use crate::{AppState, ToastPayload, TOAST_EVENT};
use fraglink_core::engine::keys;
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Size, WebviewUrl,
    WebviewWindowBuilder, Window, WindowEvent,
};

/// Classic chassis size in CSS px (the prototype's 290x381), and with the side panel open.
pub const BASE_W: f64 = 290.0;
pub const SIDE_W: f64 = 468.0;
pub const BASE_H: f64 = 381.0;
/// Dark skin: resizable window. Minimum = default = 333x555, like the 2013-era
/// client this layout follows; +12 px for the info-panel nub on the right.
pub const DARK_MIN_W: f64 = 345.0;
pub const DARK_MIN_H: f64 = 555.0;
/// The docked info panel adds this much width while it is open. The window keeps its full height;
/// the panel itself is inset vertically in CSS (under the title bar, above the bottom edge, dev.18),
/// and the transparent window shows nothing in the strips above and below it.
pub const DARK_SIDE_W: f64 = 168.0; // the 180px panel takes over the 12px nub strip
/// The info panel can be dragged wider (0.2.0-dev.9); its width is remembered.
pub const SIDE_PANEL_MIN: f64 = 180.0;
pub const SIDE_PANEL_MAX: f64 = 560.0;

/// Options > Appearance > Text size: the dark window's content is zoomed by this.
pub fn text_zoom(app: &AppHandle) -> f64 {
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    match e
        .tracker
        .store
        .get_setting("text_size")
        .ok()
        .flatten()
        .as_deref()
    {
        Some("large") => 1.15,
        Some("larger") => 1.3,
        _ => 1.0,
    }
}

/// Extra window width for the open info panel (panel width minus the nub strip),
/// in window pixels (so scaled by the text size).
pub fn side_extra(app: &AppHandle) -> f64 {
    let tz = text_zoom(app);
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    let w = e
        .tracker
        .store
        .get_setting("side_w")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite())
        .unwrap_or(SIDE_PANEL_MIN)
        .clamp(SIDE_PANEL_MIN, SIDE_PANEL_MAX);
    (DARK_SIDE_W + (w - SIDE_PANEL_MIN)) * tz
}
const TOAST_W: f64 = 226.0;
// Tall enough for the Steam-style 3-line toast with a 2-line game name (dev.18); the
// window is transparent and the toast sits at its bottom, as tall as its text.
const TOAST_H: f64 = 100.0;

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

/// Current skin ("dark" by default). Only "dark" gets a resizable window;
/// the chassis skins are drawn at a fixed size.
pub fn is_dark(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    let skin = e.tracker.store.get_setting(keys::SKIN).ok().flatten();
    skin.is_none_or(|s| s == "dark")
}

/// A "1"/"0" checkbox setting from Options.
pub fn flag(app: &AppHandle, key: &str) -> bool {
    let state = app.state::<AppState>();
    let e = state.engine.lock().unwrap();
    e.tracker.store.get_setting(key).ok().flatten().as_deref() == Some("1")
}

/// Saved dark-window size (logical px, without the info panel).
pub fn parse_size(s: &str) -> Option<(f64, f64)> {
    let (w, h) = s.split_once(',')?;
    let (w, h): (f64, f64) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w.is_finite() && h.is_finite())
        .then_some((w.clamp(DARK_MIN_W, 4000.0), h.clamp(DARK_MIN_H, 3000.0)))
}

fn dark_base(app: &AppHandle) -> (f64, f64) {
    app.state::<AppState>()
        .main_size
        .lock()
        .unwrap()
        .unwrap_or((DARK_MIN_W, DARK_MIN_H))
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
    let b = WebviewWindowBuilder::from_config(app, &cfg)?;
    *app.state::<AppState>().side_open.lock().unwrap() = false;
    let w = if is_dark(app) {
        let (bw, bh) = dark_base(app);
        let tz = text_zoom(app);
        b.inner_size(bw.max(DARK_MIN_W * tz), bh.max(DARK_MIN_H * tz))
            .min_inner_size(DARK_MIN_W * tz, DARK_MIN_H * tz)
            .resizable(true)
            .build()?
    } else {
        let z = zoom(app);
        b.inner_size(BASE_W * z, BASE_H * z)
            .resizable(false)
            .build()?
    };
    if flag(app, "always_on_top") {
        let _ = w.set_always_on_top(true);
    }
    let pos = *app.state::<AppState>().main_pos.lock().unwrap();
    if let Some((x, y)) = pos {
        if on_some_monitor(app, x, y) {
            let _ = w.set_position(PhysicalPosition::new(x, y));
        }
    }
    Ok(w)
}

/// Build windows on a worker thread. On Windows, creating a WebView2 window on
/// the main thread while it is busy (a synchronous `#[tauri::command]` runs
/// there, and so do tray, menu and single-instance callbacks) deadlocks: the
/// window stays blank white and the whole app hangs. From another thread the
/// builder hands the work to the event loop and waits there, which is safe.
fn off_main(app: &AppHandle, what: &'static str, f: impl FnOnce(&AppHandle) + Send + 'static) {
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new()
        .name(format!("create-{what}"))
        .spawn(move || f(&app))
    {
        log::error!("could not start {what} window thread: {e}");
    }
}

fn focus(w: &tauri::WebviewWindow) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
}

/// Show, restore and focus the main window, creating it if it was closed.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        focus(&w);
        return;
    }
    off_main(app, "main", |app| {
        // a second request may have created it meanwhile
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
        focus(&w);
    });
}

/// Startup window ("Checking for updates…"), Discord/Xfire style: small, borderless,
/// centred. See `updater::start_with_splash`.
pub fn show_splash(app: &AppHandle) {
    off_main(app, "splash", |app| {
        if app.get_webview_window("splash").is_some() {
            return;
        }
        let r = WebviewWindowBuilder::new(app, "splash", WebviewUrl::App("splash.html".into()))
            .title("Fraglink")
            .inner_size(SPLASH_W, SPLASH_H)
            .decorations(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .center()
            .focused(true)
            .build();
        if let Err(e) = r {
            log::warn!("startup window failed: {e}");
        }
    });
}

pub const SPLASH_W: f64 = 300.0;
pub const SPLASH_H: f64 = 330.0;

pub fn close_splash(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("splash") {
        let _ = w.close();
    }
}

/// Close (destroy) the main window; the app keeps running in the tray.
pub fn hide_main(app: &AppHandle) {
    persist_main_pos(app);
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.close();
    }
}

pub fn persist_main_pos(app: &AppHandle) {
    let state = app.state::<AppState>();
    let pos = *state.main_pos.lock().unwrap();
    let size = *state.main_size.lock().unwrap();
    let e = state.engine.lock().unwrap();
    if let Some((x, y)) = pos {
        let _ = e.tracker.store.set_setting("main_pos", &format!("{x},{y}"));
    }
    if let Some((w, h)) = size {
        let _ = e
            .tracker
            .store
            .set_setting("main_size", &format!("{w:.0},{h:.0}"));
    }
}

/// Fit the main window to the skin: dark = resizable (remembered size, the info
/// panel adds width); chassis skins = fixed size scaled by `z`.
pub fn resize_main(app: &AppHandle, z: f64, side_open: bool) {
    let Some(w) = app.get_webview_window("main") else {
        return;
    };
    *app.state::<AppState>().side_open.lock().unwrap() = side_open;
    if is_dark(app) {
        let side = if side_open { side_extra(app) } else { 0.0 };
        let tz = text_zoom(app);
        let (bw, bh) = dark_base(app);
        let (bw, bh) = (bw.max(DARK_MIN_W * tz), bh.max(DARK_MIN_H * tz));
        let _ = w.set_resizable(true);
        let _ = w.set_min_size(Some(Size::Logical(LogicalSize::new(
            DARK_MIN_W * tz + side,
            DARK_MIN_H * tz,
        ))));
        let _ = w.set_size(LogicalSize::new(bw + side, bh));
    } else {
        let width = if side_open { SIDE_W } else { BASE_W };
        let _ = w.set_min_size(None::<Size>);
        let _ = w.set_resizable(false);
        let _ = w.set_size(LogicalSize::new(width * z, BASE_H * z));
    }
}

/// Open (or focus) the Options window: its own resizable window with the same
/// custom dark title bar as the main window (0.2.0-dev.9; drawn by options.html).
pub fn show_options(app: &AppHandle, page: Option<String>) {
    let page = page.filter(|p| p.len() < 24 && p.chars().all(|c| c.is_ascii_lowercase()));
    if let Some(w) = app.get_webview_window("options") {
        focus(&w);
        if let Some(p) = page {
            let _ = app.emit_to("options", "fraglink://options-page", p);
        }
        return;
    }
    let url = match &page {
        Some(p) => format!("options.html?page={p}"),
        None => "options.html".into(),
    };
    off_main(app, "options", move |app| {
        if app.get_webview_window("options").is_some() {
            return;
        }
        let r = WebviewWindowBuilder::new(app, "options", WebviewUrl::App(url.into()))
            .title("Fraglink Options")
            .decorations(false)
            .inner_size(600.0, 580.0)
            .min_inner_size(520.0, 480.0)
            .resizable(true)
            .maximizable(true)
            .center()
            .build();
        if let Err(e) = r {
            log::warn!("options window failed: {e}");
        }
    });
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
        WindowEvent::Resized(sz) => {
            let app = window.app_handle();
            if window.is_minimized().unwrap_or(false) {
                // Options > General: "Minimise to the system tray"
                if flag(app, "minimize_to_tray") {
                    let _ = window.hide();
                }
                return;
            }
            let sf = window.scale_factor().unwrap_or(1.0);
            let (w, h) = (sz.width as f64 / sf, sz.height as f64 / sf);
            // ignore minimised (0x0) and chassis-skin sizes
            if w >= DARK_MIN_W - 1.0 && h >= DARK_MIN_H - 1.0 && is_dark(app) {
                let open = *app.state::<AppState>().side_open.lock().unwrap();
                let side = if open { side_extra(app) } else { 0.0 };
                let st = app.state::<AppState>();
                *st.main_size.lock().unwrap() = Some(((w - side).max(DARK_MIN_W), h));
            }
        }
        WindowEvent::Destroyed => persist_main_pos(window.app_handle()),
        _ => {}
    }
}

/// Bottom-right toast (stand-in for friend toasts until M3). Reuses the window if open.
pub fn show_toast(app: &AppHandle, mut payload: ToastPayload) {
    if payload.icon.is_none() && !payload.kind.starts_with("request:") {
        payload.icon = crate::icons::one(app, &payload.game_id, false);
    }
    *app.state::<AppState>().pending_toast.lock().unwrap() = Some(payload.clone());
    if app.get_webview_window("toast").is_some() {
        let _ = app.emit_to("toast", TOAST_EVENT, payload);
        return;
    }
    // the toast page fetches the payload itself (`toast_payload`) once loaded
    off_main(app, "toast", |app| {
        if app.get_webview_window("toast").is_some() {
            return;
        }
        // the dark skin's popup is drawn at 1x; the chassis skins follow their scale
        let z = if is_dark(app) { 1.0 } else { zoom(app) };
        let (w, h) = (TOAST_W * z, TOAST_H * z);
        let mut builder =
            WebviewWindowBuilder::new(app, "toast", WebviewUrl::App("toast.html".into()))
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
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_size_is_clamped_to_the_minimum() {
        assert_eq!(parse_size("520,700"), Some((520.0, 700.0)));
        assert_eq!(parse_size("100, 100"), Some((DARK_MIN_W, DARK_MIN_H)));
        assert_eq!(parse_size("NaN,700"), None);
        assert_eq!(parse_size("junk"), None);
    }
}
