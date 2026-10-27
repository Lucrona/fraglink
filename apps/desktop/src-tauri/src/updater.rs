//! Updates, Discord/Xfire style.
//!
//! * **Startup window** (0.2.0-dev.10): when Fraglink starts with its window, a
//!   small borderless window ("Checking for updates…") checks first. A newer build is
//!   downloaded (progress bar), its signature verified, installed, and Fraglink
//!   relaunches into it. Otherwise the window closes after about a second and the
//!   main window opens ("Detecting games…" first waits up to 2 s for the installed-games
//!   scan). Offline or a slow server: the check gives up after
//!   [`SPLASH_TIMEOUT`] and Fraglink opens normally.
//! * **Started to the tray** (Windows autostart, "start minimised"): no window, like
//!   Discord's "Start Minimized". The background check runs 20 s later; an update
//!   left ready last time is applied straight away.
//! * **Background**: every hour; a new build is downloaded and verified quietly, then a
//!   small green arrow in the title bar and a tray dot / "Restart to Update" install it.
//! * **Check for Updates** (Help menu, tray): checks now; "You're up to date" or the
//!   update downloads, installs and restarts.
use crate::LockOk;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const UPDATE_EVENT: &str = "fraglink://update";
pub const SPLASH_EVENT: &str = "fraglink://splash";
pub const FOUND_EVENT: &str = "fraglink://update-found";
const FIRST_CHECK: Duration = Duration::from_secs(20);
pub const EVERY: Duration = Duration::from_secs(3600);
/// Give up on the startup check after this (offline, slow server).
pub const SPLASH_TIMEOUT: Duration = Duration::from_secs(5);
/// Keep the startup window up at least this long so it doesn't just flash.
const SPLASH_MIN: Duration = Duration::from_millis(1000);
/// Longest the startup window waits for "Detecting games…".
const DETECT_MAX: Duration = Duration::from_secs(2);
/// Startup / manual download limit before Fraglink gives up and opens normally.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Default)]
pub struct UpdateState {
    /// A downloaded, verified update: (update, installer bytes).
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    installing: Mutex<bool>,
    checking: Mutex<bool>,
}

/// Version of the downloaded update waiting for a restart, if any.
pub fn ready_version(app: &AppHandle) -> Option<String> {
    app.try_state::<UpdateState>()?
        .ready
        .lock_ok()
        .as_ref()
        .map(|(u, _)| u.version.clone())
}

/// Marker so an ignored update is applied on the next start.
fn marker(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("update-ready"))
}

fn clear_marker(app: &AppHandle) {
    if let Some(p) = marker(app) {
        let _ = std::fs::remove_file(p);
    }
}

/// True when the last run left a downloaded update: the caller keeps the main
/// window hidden while [`start`] applies it (shown again if that fails).
pub fn pending_on_start(app: &AppHandle) -> bool {
    let Some(v) = marker(app).and_then(|p| std::fs::read_to_string(p).ok()) else {
        return false;
    };
    let cur = app.package_info().version.to_string();
    if v.trim().is_empty() || v.trim() == cur {
        clear_marker(app);
        return false;
    }
    true
}

/// Started to the tray (no startup window). `apply_now`: the previous run left an
/// update ready; install it straight away.
pub fn start(app: AppHandle, apply_now: bool, show_after: bool) {
    tauri::async_runtime::spawn(async move {
        if apply_now {
            if check(&app).await && install(&app).await.is_ok() {
                return; // restarting
            }
            clear_marker(&app);
            if show_after {
                crate::windows::show_main(&app);
            }
        } else {
            sleep(FIRST_CHECK).await;
        }
        background(app).await;
    });
}

async fn background(app: AppHandle) -> ! {
    loop {
        check(&app).await;
        sleep(EVERY).await;
    }
}

async fn sleep(d: Duration) {
    tokio::time::sleep(d).await;
}

#[derive(Clone, Serialize)]
struct SplashMsg {
    text: String,
    /// 0-100, or None for the moving "busy" bar.
    pct: Option<u32>,
}

fn splash(app: &AppHandle, text: &str, pct: Option<u32>) {
    let _ = app.emit_to(
        "splash",
        SPLASH_EVENT,
        SplashMsg {
            text: text.into(),
            pct,
        },
    );
}

/// Startup window flow (see the module docs). The caller has shown the window.
pub fn start_with_splash(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let t0 = Instant::now();
        let found = if crate::is_sim(&app) {
            None // simulation: show the window, never update
        } else {
            match tokio::time::timeout(SPLASH_TIMEOUT, find(&app)).await {
                Ok(Ok(u)) => u,
                Ok(Err(e)) => {
                    log::info!("startup update check failed: {e}");
                    None
                }
                Err(_) => {
                    log::info!("startup update check timed out");
                    None
                }
            }
        };
        if let Some(up) = found {
            log::info!("startup: update {} -> {}", up.current_version, up.version);
            if download_with_progress(&app, up, true).await {
                splash(&app, "Installing update…", Some(100));
                match install(&app).await {
                    Ok(()) => return, // the installer relaunches Fraglink
                    Err(e) => log::warn!("startup update install failed: {e}"),
                }
            }
        } else {
            clear_marker(&app);
        }
        detect_games(&app).await;
        splash(&app, "Starting…", Some(100));
        let shown = t0.elapsed();
        if shown < SPLASH_MIN {
            sleep(SPLASH_MIN - shown).await;
        }
        crate::windows::show_main(&app);
        sleep(Duration::from_millis(250)).await; // let the main window paint first
        crate::windows::close_splash(&app);
        sleep(EVERY).await;
        background(app).await;
    });
}

/// "Detecting games…": wait for the tracker's first installed-games scan (it starts with the
/// app), at most [`DETECT_MAX`]; a slower scan just finishes in the background.
async fn detect_games(app: &AppHandle) {
    splash(app, "Detecting games…", None);
    let t0 = Instant::now();
    loop {
        let done = {
            let st = app.state::<crate::AppState>();
            let r = st.engine.try_lock().map(|e| e.installs_scanned());
            r.unwrap_or(false) // locked = the scan is still running
        };
        if done {
            log::info!("startup: games detected in {} ms", t0.elapsed().as_millis());
            return;
        }
        if t0.elapsed() >= DETECT_MAX {
            log::info!("startup: game detection continues in the background");
            return;
        }
        sleep(Duration::from_millis(50)).await;
    }
}

/// Ask the server; `Ok(Some)` = a newer build exists (not downloaded yet).
async fn find(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater_builder()
        .version_comparator(|current, remote| is_newer(&current, &remote.version))
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

/// Download + verify `up` (progress on the startup window when `to_splash`).
/// True = it's ready to install.
async fn download_with_progress(app: &AppHandle, up: Update, to_splash: bool) -> bool {
    let what = format!("Downloading update {}…", crate::dev_version(&up.version));
    if to_splash {
        splash(app, &what, Some(0));
    }
    let (app2, what2) = (app.clone(), what.clone());
    let mut got: u64 = 0;
    let mut last_pct = u32::MAX;
    let fut = up.download(
        move |chunk, total| {
            got += chunk as u64;
            if let Some(t) = total.filter(|t| *t > 0) {
                let pct = ((got * 100) / t).min(100) as u32;
                if pct != last_pct {
                    last_pct = pct;
                    if to_splash {
                        splash(&app2, &what2, Some(pct));
                    }
                    let _ = app2.emit("fraglink://update-progress", pct);
                }
            }
        },
        || {},
    );
    // `download` verifies the minisign signature against the configured public key.
    let bytes = match tokio::time::timeout(DOWNLOAD_TIMEOUT, fut).await {
        Ok(Ok(b)) => b,
        Ok(Err(e)) => {
            log::info!("update download failed: {e}");
            return false;
        }
        Err(_) => {
            log::info!("update download timed out");
            return false;
        }
    };
    log::info!("update {} downloaded and verified", up.version);
    *app.state::<UpdateState>().ready.lock_ok() = Some((up, bytes));
    true
}

/// Background check and, when there's a newer build, download + verify it quietly.
/// True = one is ready.
async fn check(app: &AppHandle) -> bool {
    {
        let st = app.state::<UpdateState>();
        let mut busy = st.checking.lock_ok();
        if *busy {
            return ready_version(app).is_some();
        }
        *busy = true;
    }
    let r = check_inner(app).await;
    *app.state::<UpdateState>().checking.lock_ok() = false;
    r
}

async fn check_inner(app: &AppHandle) -> bool {
    let up = match find(app).await {
        Ok(Some(up)) => up,
        Ok(None) => {
            log::debug!("no update");
            return ready_version(app).is_some();
        }
        Err(e) => {
            log::info!("update check failed: {e}");
            return ready_version(app).is_some();
        }
    };
    if ready_version(app).as_deref() == Some(up.version.as_str()) {
        return true;
    }
    log::info!(
        "update {} -> {}: downloading",
        up.current_version,
        up.version
    );
    if !download_with_progress(app, up, false).await {
        return ready_version(app).is_some();
    }
    announce_ready(app);
    true
}

/// Title-bar arrow, tray dot and "Restart to Update" for the ready update.
fn announce_ready(app: &AppHandle) {
    let Some(version) = ready_version(app) else {
        return;
    };
    if let Some(p) = marker(app) {
        let _ = std::fs::write(p, &version);
    }
    crate::refresh_tray(app);
    crate::set_tray_update_dot(app, true);
    let _ = app.emit(UPDATE_EVENT, &version);
    // beta.9: one "Update ready" toast per version (not affected by friend pop-up mutes).
    static TOASTED: Mutex<Option<String>> = Mutex::new(None);
    let mut last = TOASTED.lock_ok();
    if last.as_deref() != Some(version.as_str()) {
        *last = Some(version.clone());
        drop(last);
        crate::windows::show_toast(
            app,
            crate::ToastPayload {
                kind: format!("update:{version}"),
                title: "Update ready".into(),
                line: "is ready to install".into(),
                game: format!("v{}", crate::dev_version(&version)),
                icon: Some(String::new()),
                ..Default::default()
            },
        );
    }
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<UpdateState>();
    {
        let mut busy = st.installing.lock_ok();
        if *busy {
            return Ok(());
        }
        *busy = true;
    }
    let ready = st.ready.lock_ok().clone();
    let Some((up, bytes)) = ready else {
        *st.installing.lock_ok() = false;
        return Err("no update ready".into());
    };
    log::info!("installing update {}", up.version);
    clear_marker(app);
    crate::stop_for_update(app); // close open sessions before the installer takes over
    if let Err(e) = up.install(bytes) {
        // Windows: install() exits and the installer relaunches the app
        *st.installing.lock_ok() = false;
        return Err(e.to_string());
    }
    app.restart();
}

/// Result of "Check for Updates".
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum Manual {
    UpToDate {
        version: String,
    },
    /// An update was found (or already ready) and is installing; Fraglink restarts.
    Installing {
        version: String,
    },
    Failed {
        message: String,
    },
}

/// "Check for Updates": check now; install straight away when there is one.
pub async fn check_now(app: &AppHandle) -> Manual {
    let current = crate::dev_version(&app.package_info().version.to_string());
    if crate::is_sim(app) {
        return Manual::UpToDate { version: current };
    }
    if ready_version(app).is_none() {
        let found = match tokio::time::timeout(Duration::from_secs(20), find(app)).await {
            Ok(Ok(u)) => u,
            Ok(Err(e)) => return Manual::Failed { message: e },
            Err(_) => {
                return Manual::Failed {
                    message: "The update server didn't answer.".into(),
                }
            }
        };
        let Some(up) = found else {
            return Manual::UpToDate { version: current };
        };
        let _ = app.emit(FOUND_EVENT, crate::dev_version(&up.version));
        if !download_with_progress(app, up, false).await {
            return Manual::Failed {
                message: "The update couldn't be downloaded.".into(),
            };
        }
    }
    let version = ready_version(app)
        .map(|v| crate::dev_version(&v))
        .unwrap_or_default();
    let app2 = app.clone();
    // install after this answer reaches the UI
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_millis(600)).await;
        if let Err(e) = install(&app2).await {
            log::warn!("update install failed: {e}");
            announce_ready(&app2);
        }
    });
    Manual::Installing { version }
}

/// Version of the update that is downloaded and ready, if any (the UI asks on load).
#[tauri::command]
pub fn update_available(app: AppHandle) -> Option<String> {
    ready_version(&app)
}

/// Title-bar arrow / tray item: install the ready update and restart, no questions.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

/// Help > Check for Updates.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> Manual {
    check_now(&app).await
}

/// Tray "Restart to Update".
pub fn install_from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = install(&app).await {
            log::warn!("update install failed: {e}");
        }
    });
}

/// Tray "Check for Updates": same as the Help menu, answered with a small message box.
pub fn check_from_tray(app: &AppHandle) {
    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (kind, text) = match check_now(&app).await {
            Manual::UpToDate { version } => (
                MessageDialogKind::Info,
                format!("You're up to date.\n\nFraglink {version}"),
            ),
            Manual::Installing { .. } => return, // the installer window takes over
            Manual::Failed { message } => (
                MessageDialogKind::Warning,
                format!("Couldn't check for updates.\n\n{message}"),
            ),
        };
        app.dialog()
            .message(text)
            .title("Check for Updates")
            .kind(kind)
            .show(|_| {});
    });
}

/// Whether the server's build is newer than this one: plain semver order, so the beta
/// channel (0.3.0-beta.N) is newer than every 0.2.0-N test build and beta.10 > beta.9.
pub fn is_newer(current: &semver::Version, remote: &semver::Version) -> bool {
    remote > current
}

#[cfg(test)]
mod tests {
    use super::is_newer;
    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn beta_channel_is_newer_than_the_old_test_builds() {
        assert!(is_newer(&v("0.2.0-21"), &v("0.3.0-beta.1")));
        assert!(is_newer(&v("0.2.0-9"), &v("0.3.0-beta.1")));
        assert!(is_newer(&v("0.3.0-beta.1"), &v("0.3.0-beta.2")));
        assert!(is_newer(&v("0.3.0-beta.9"), &v("0.3.0-beta.10")));
        assert!(is_newer(&v("0.3.0-beta.9"), &v("0.3.0-beta.9.1")));
        assert!(is_newer(&v("0.3.0-beta.9.1"), &v("0.3.0-beta.10")));
        assert!(!is_newer(&v("0.3.0-beta.9.1"), &v("0.3.0-beta.9")));
        assert!(is_newer(&v("0.3.0-beta.12"), &v("0.3.0")));
        assert!(!is_newer(&v("0.3.0-beta.1"), &v("0.2.0-21")));
        assert!(!is_newer(&v("0.3.0-beta.2"), &v("0.3.0-beta.2")));
        assert!(!is_newer(&v("0.3.0"), &v("0.3.0-beta.5")));
    }
}
