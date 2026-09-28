//! Quiet auto-update: check at startup and every 6 hours; when there's a newer signed build,
//! the main window asks "An update is available. Restart to update?" and installs on Yes.
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

pub const UPDATE_EVENT: &str = "fraglink://update";
const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(6 * 3600);

#[derive(Default)]
pub struct UpdateState {
    /// Version of an update that is waiting for the user's Yes.
    pub available: std::sync::Mutex<Option<String>>,
}

pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio_sleep(FIRST_CHECK).await;
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

async fn check(app: &AppHandle) {
    let res = match app.updater() {
        Ok(u) => u.check().await,
        Err(e) => {
            log::warn!("updater unavailable: {e}");
            return;
        }
    };
    match res {
        Ok(Some(up)) => {
            log::info!("update available: {} -> {}", up.current_version, up.version);
            *app.state::<UpdateState>().available.lock().unwrap() = Some(up.version.clone());
            let _ = app.emit(UPDATE_EVENT, &up.version);
        }
        Ok(None) => log::debug!("no update"),
        Err(e) => log::info!("update check failed: {e}"),
    }
}

/// Version waiting to be installed, if any (the main window asks on load).
#[tauri::command]
pub fn update_available(app: AppHandle) -> Option<String> {
    app.state::<UpdateState>().available.lock().unwrap().clone()
}

/// Yes: download, verify the signature, run the installer and restart.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let up = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("no update available")?;
    log::info!("installing update {}", up.version);
    let bytes = up
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    crate::stop_for_update(&app); // close open sessions before the installer takes over
    up.install(bytes).map_err(|e| e.to_string())?; // Windows: exits here and the installer relaunches
    app.restart();
}
