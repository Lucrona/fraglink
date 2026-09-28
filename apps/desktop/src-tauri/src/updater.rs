//! Quiet auto-update, Discord style: check at startup and every 6 hours; a newer
//! signed build is downloaded (and its signature verified) in the background. Then
//! the main window shows a small green arrow in the title bar, the tray gets a
//! "Restart to Update" item and a green dot; one click installs and restarts. An
//! update that is ignored is applied on the next app start.
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const UPDATE_EVENT: &str = "fraglink://update";
const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct UpdateState {
    /// A downloaded, verified update: (update, installer bytes).
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    installing: Mutex<bool>,
}

/// Version of the downloaded update waiting for a restart, if any.
pub fn ready_version(app: &AppHandle) -> Option<String> {
    app.try_state::<UpdateState>()?
        .ready
        .lock()
        .unwrap()
        .as_ref()
        .map(|(u, _)| u.version.clone())
}

/// Marker so an ignored update is applied on the next start.
fn marker(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("update-ready"))
}

/// True when the last run left a downloaded update: the caller keeps the main
/// window hidden while [`start`] applies it (shown again if that fails).
pub fn pending_on_start(app: &AppHandle) -> bool {
    let Some(v) = marker(app).and_then(|p| std::fs::read_to_string(p).ok()) else {
        return false;
    };
    let cur = app.package_info().version.to_string();
    if v.trim().is_empty() || v.trim() == cur {
        if let Some(p) = marker(app) {
            let _ = std::fs::remove_file(p);
        }
        return false;
    }
    true
}

/// `apply_now`: the previous run left an update ready; install it straight away.
pub fn start(app: AppHandle, apply_now: bool, show_after: bool) {
    tauri::async_runtime::spawn(async move {
        if apply_now {
            if check(&app).await && install(&app).await.is_ok() {
                return; // restarting
            }
            if let Some(p) = marker(&app) {
                let _ = std::fs::remove_file(p);
            }
            if show_after {
                crate::windows::show_main(&app);
            }
        } else {
            tokio_sleep(FIRST_CHECK).await;
        }
        loop {
            check(&app).await;
            tokio_sleep(EVERY).await;
        }
    });
}

async fn tokio_sleep(d: Duration) {
    // tauri's runtime is tokio; avoid a direct tokio dependency
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}

/// Check and, when there's a newer build, download + verify it. True = one is ready.
async fn check(app: &AppHandle) -> bool {
    let res = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => {
            log::warn!("updater unavailable: {e}");
            return ready_version(app).is_some();
        }
    };
    let up = match res {
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
    // `download` verifies the minisign signature against the configured public key.
    let bytes = match up.download(|_, _| {}, || {}).await {
        Ok(b) => b,
        Err(e) => {
            log::info!("update download failed: {e}");
            return ready_version(app).is_some();
        }
    };
    log::info!("update {} downloaded and verified", up.version);
    let version = up.version.clone();
    *app.state::<UpdateState>().ready.lock().unwrap() = Some((up, bytes));
    if let Some(p) = marker(app) {
        let _ = std::fs::write(p, &version);
    }
    crate::refresh_tray(app);
    crate::set_tray_update_dot(app, true);
    let _ = app.emit(UPDATE_EVENT, &version);
    true
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<UpdateState>();
    {
        let mut busy = st.installing.lock().unwrap();
        if *busy {
            return Ok(());
        }
        *busy = true;
    }
    let ready = st.ready.lock().unwrap().clone();
    let Some((up, bytes)) = ready else {
        *st.installing.lock().unwrap() = false;
        return Err("no update ready".into());
    };
    log::info!("installing update {}", up.version);
    if let Some(p) = marker(app) {
        let _ = std::fs::remove_file(p);
    }
    crate::stop_for_update(app); // close open sessions before the installer takes over
    if let Err(e) = up.install(bytes) {
        // Windows: install() exits and the installer relaunches the app
        *st.installing.lock().unwrap() = false;
        return Err(e.to_string());
    }
    app.restart();
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

/// Tray "Restart to Update".
pub fn install_from_tray(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = install(&app).await {
            log::warn!("update install failed: {e}");
        }
    });
}
