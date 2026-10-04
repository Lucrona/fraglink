//! Optional account sync. Off by default: nothing here touches the network
//! until the user presses "Sign in to sync" in Options and approves the pairing
//! code in their browser. Since 0.2.0-dev.14 hours only count while connected:
//! the server records them from live presence (friends.rs), so nothing is queued
//! or uploaded from here. The worker just refreshes the account totals.

use crate::{now, AppState};
use fraglink_sync::{
    secret, Client, PollResult, SyncError, DEFAULT_SERVER, FALLBACK_SERVER, LEGACY_DEFAULT_SERVER,
};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const SYNC_EVENT: &str = "fraglink://sync";
const REFRESH_EVERY: Duration = Duration::from_secs(5 * 60);

pub mod keys {
    pub const SERVER: &str = "sync_server";
    pub const TOKEN: &str = "sync_token";
    pub const USER: &str = "sync_user";
    pub const LAST_OK: &str = "sync_last_ok";
    pub const LAST_ERROR: &str = "sync_last_error";
    /// Settings the webview must never see.
    pub const HIDDEN: &[&str] = &[TOKEN];
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingView {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_at: i64,
}

#[derive(Default)]
pub struct SyncShared {
    nudge: Mutex<Option<Sender<()>>>,
    worker_running: AtomicBool,
    pairing: Mutex<Option<PairingView>>,
    /// Bumped to cancel an in-flight pairing poll.
    pairing_gen: AtomicU64,
    pairing_error: Mutex<Option<String>>,
    /// Latest `/api/v1/stats` response (per-game totals by tier). None when
    /// signed out or the last fetch failed: the Stats tab then uses local data.
    server_stats: Mutex<Option<(i64, serde_json::Value)>>,
}

#[derive(Debug, Serialize)]
pub struct SyncView {
    pub signed_in: bool,
    pub server: String,
    pub user: Option<String>,
    pub pending: i64,
    pub last_ok: Option<i64>,
    pub last_error: Option<String>,
    pub pairing: Option<PairingView>,
    pub pairing_error: Option<String>,
}

fn setting(app: &AppHandle, k: &str) -> Option<String> {
    let st = app.state::<AppState>();
    let e = st.engine.lock().unwrap();
    e.tracker
        .store
        .get_setting(k)
        .ok()
        .flatten()
        .filter(|v| !v.is_empty())
}

fn set(app: &AppHandle, k: &str, v: &str) {
    let st = app.state::<AppState>();
    let e = st.engine.lock().unwrap();
    if let Err(err) = e.tracker.store.set_setting(k, v) {
        log::error!("sync: saving {k} failed: {err:#}");
    }
}

fn token(app: &AppHandle) -> Option<String> {
    setting(app, keys::TOKEN).and_then(|s| {
        let t = secret::unprotect(&s);
        if t.is_none() {
            log::warn!("sync: stored token can't be decrypted (copied database?); sign in again");
        }
        t
    })
}

pub fn signed_in(app: &AppHandle) -> bool {
    setting(app, keys::TOKEN).is_some()
}

/// An authenticated client, or None when signed out.
pub fn client(app: &AppHandle) -> Option<Client> {
    token(app).map(|t| Client::new(&server(app), Some(t)))
}

/// Server totals for the Stats tab: `{fetched_at, games, overall}` or null.
pub fn server_stats(app: &AppHandle) -> Option<serde_json::Value> {
    if !signed_in(app) {
        return None;
    }
    let st = app.state::<AppState>();
    let g = st.sync.server_stats.lock().unwrap();
    g.as_ref().map(|(at, v)| {
        let mut v = v.clone();
        v["fetched_at"] = serde_json::json!(at);
        v
    })
}

pub fn server(app: &AppHandle) -> String {
    let saved = setting(app, keys::SERVER);
    let s = pick_server(saved.clone(), signed_in(app));
    // dev.10 and older saved the IP; move the saved setting to the domain (same server).
    if saved.as_deref().is_some_and(is_ip_default) {
        set(app, keys::SERVER, &s);
    }
    s
}

/// The production server by IP (the default up to dev.10).
fn is_ip_default(s: &str) -> bool {
    s.trim_end_matches('/') == FALLBACK_SERVER
}

/// The saved server, except that the old localhost default (saved by every sign-in before
/// dev.6) moves to the production server unless this PC is signed in to it, and the
/// production server's IP (dev.6 to dev.10) moves to its domain.
fn pick_server(saved: Option<String>, signed_in: bool) -> String {
    match saved {
        Some(s) if is_ip_default(&s) => DEFAULT_SERVER.to_string(),
        Some(s) if s.trim_end_matches('/') == LEGACY_DEFAULT_SERVER && !signed_in => {
            DEFAULT_SERVER.to_string()
        }
        Some(s) => s,
        None => DEFAULT_SERVER.to_string(),
    }
}

fn emit(app: &AppHandle) {
    let _ = app.emit(SYNC_EVENT, ());
}

pub fn view(app: &AppHandle) -> SyncView {
    let st = app.state::<AppState>();
    // Nothing waits to upload any more (dev.14: no offline sessions).
    let pending = 0;
    let pairing = st.sync.pairing.lock().unwrap().clone();
    let pairing_error = st.sync.pairing_error.lock().unwrap().clone();
    SyncView {
        signed_in: token(app).is_some(),
        server: server(app),
        user: setting(app, keys::USER),
        pending,
        last_ok: setting(app, keys::LAST_OK).and_then(|v| v.parse().ok()),
        last_error: setting(app, keys::LAST_ERROR),
        pairing,
        pairing_error,
    }
}

/// Called at startup: only starts the sync thread if the user signed in before.
pub fn init(app: &AppHandle) {
    if token(app).is_some() {
        start_worker(app.clone());
        crate::friends::start(app);
    } else {
        log::info!("sync: not signed in; no network activity");
    }
}

/// Ask the worker to refresh now (after a session closes, or "Sync now").
pub fn nudge(app: &AppHandle) {
    let st = app.state::<AppState>();
    let tx = st.sync.nudge.lock().unwrap().clone();
    if let Some(tx) = tx {
        let _ = tx.send(());
    }
}

fn start_worker(app: AppHandle) {
    let st = app.state::<AppState>();
    if st.sync.worker_running.swap(true, Ordering::SeqCst) {
        nudge(&app);
        return;
    }
    let (tx, rx) = channel::<()>();
    *st.sync.nudge.lock().unwrap() = Some(tx);
    let worker_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("fraglink-sync".into())
        .spawn(move || {
            let app = worker_app;
            log::info!("sync: worker started");
            while let Some(tok) = token(&app) {
                let client = Client::new(&server(&app), Some(tok));
                match client.stats() {
                    Ok(v) => {
                        set(&app, keys::LAST_OK, &now().to_string());
                        set(&app, keys::LAST_ERROR, "");
                        *app.state::<AppState>().sync.server_stats.lock().unwrap() =
                            Some((now(), v));
                    }
                    Err(SyncError::Unauthorized) => {
                        log::warn!("sync: the server signed this device out");
                        clear_local(
                            &app,
                            "Signed out from the website. Sign in again to keep syncing.",
                        );
                        emit(&app);
                        crate::friends::emit(&app);
                        break;
                    }
                    Err(e) => {
                        log::warn!("sync: {e}");
                        set(&app, keys::LAST_ERROR, &e.to_string());
                        *app.state::<AppState>().sync.server_stats.lock().unwrap() = None;
                    }
                }
                emit(&app);
                match rx.recv_timeout(REFRESH_EVERY) {
                    Ok(()) => std::thread::sleep(Duration::from_secs(2)), // let a burst of nudges settle
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                while rx.try_recv().is_ok() {}
            }
            let st = app.state::<AppState>();
            *st.sync.nudge.lock().unwrap() = None;
            st.sync.worker_running.store(false, Ordering::SeqCst);
            log::info!("sync: worker stopped");
        });
    if let Err(e) = spawned {
        log::error!("sync: can't start worker: {e}");
        app.state::<AppState>()
            .sync
            .worker_running
            .store(false, Ordering::SeqCst);
    }
}

fn clear_local(app: &AppHandle, reason: &str) {
    set(app, keys::TOKEN, "");
    set(app, keys::USER, "");
    set(app, keys::LAST_ERROR, reason);
    let st = app.state::<AppState>();
    *st.sync.server_stats.lock().unwrap() = None;
    let e = st.engine.lock().unwrap();
    if let Err(err) = e.tracker.store.reset_sync_state() {
        log::error!("sync: reset failed: {err:#}");
    }
}

fn device_label() -> String {
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    format!(
        "Fraglink on {}",
        if name.is_empty() { "this PC" } else { &name }
    )
}

fn finish_sign_in(app: &AppHandle, token: &str, handle: &str) {
    let st = app.state::<AppState>();
    set(app, keys::TOKEN, &secret::protect(token));
    set(app, keys::USER, handle);
    set(app, keys::LAST_ERROR, "");
    *st.sync.pairing.lock().unwrap() = None;
    *st.sync.pairing_error.lock().unwrap() = None;
    log::info!("sync: signed in as {handle}");
    start_worker(app.clone());
    crate::friends::start(app);
    emit(app);
}

/// Username + password sign-in (or account creation when `create`). Blocking: call off the UI thread.
pub fn password_sign_in(
    app: &AppHandle,
    server_url: &str,
    login: &str,
    password: &str,
    create: Option<&str>,
) -> Result<(), String> {
    let base = fraglink_sync::normalize_server_url(server_url)
        .ok_or_else(|| "Enter a server address starting with http:// or https://".to_string())?;
    cancel_pairing(app);
    let client = Client::new(&base, None);
    let res = match create {
        Some(email) => client.password_signup(login, password, email, &device_label()),
        None => client.password_login(login, password, &device_label()),
    };
    let (token, user) = res.map_err(|e| match e {
        SyncError::Unauthorized => "Wrong username or password.".to_string(),
        SyncError::Server { message, .. } if !message.is_empty() => message,
        SyncError::Network(_) => format!("Can't reach {base}. Check your connection."),
        e => e.to_string(),
    })?;
    set(app, keys::SERVER, &base);
    finish_sign_in(app, &token, &user.handle);
    Ok(())
}

/// Start device pairing against `server_url`. Runs in the background: opens the
/// browser on the approval page and polls until approved, expired or cancelled.
pub fn start_pairing(app: &AppHandle, server_url: &str) -> Result<(), String> {
    let base = fraglink_sync::normalize_server_url(server_url)
        .ok_or_else(|| "Enter a server address starting with http:// or https://".to_string())?;
    let st = app.state::<AppState>();
    let gen = st.sync.pairing_gen.fetch_add(1, Ordering::SeqCst) + 1;
    *st.sync.pairing_error.lock().unwrap() = None;
    *st.sync.pairing.lock().unwrap() = None;
    set(app, keys::SERVER, &base);
    let app = app.clone();
    std::thread::Builder::new()
        .name("fraglink-pair".into())
        .spawn(move || {
            let st = app.state::<AppState>();
            let fail = |msg: String| {
                if st.sync.pairing_gen.load(Ordering::SeqCst) == gen {
                    *st.sync.pairing.lock().unwrap() = None;
                    *st.sync.pairing_error.lock().unwrap() = Some(msg);
                    emit(&app);
                }
            };
            let client = Client::new(&base, None);
            let start = match client.start_pairing(&device_label()) {
                Ok(s) => s,
                Err(e) => return fail(format!("Couldn't start sign-in: {e}")),
            };
            *st.sync.pairing.lock().unwrap() = Some(PairingView {
                user_code: start.user_code.clone(),
                verification_uri: start.verification_uri.clone(),
                verification_uri_complete: start.verification_uri_complete.clone(),
                expires_at: now() + start.expires_in,
            });
            emit(&app);
            // Only open http(s) pages on the server the user typed.
            if start
                .verification_uri_complete
                .starts_with(&format!("{base}/"))
            {
                let _ =
                    tauri_plugin_opener::open_url(&start.verification_uri_complete, None::<&str>);
            }
            let deadline = now() + start.expires_in;
            let interval = Duration::from_secs(start.interval.clamp(2, 30) as u64);
            loop {
                std::thread::sleep(interval);
                if st.sync.pairing_gen.load(Ordering::SeqCst) != gen {
                    return; // cancelled or restarted
                }
                match client.poll(&start.device_code) {
                    Ok(PollResult::Pending) if now() < deadline => continue,
                    Ok(PollResult::Pending) | Ok(PollResult::Expired) => {
                        return fail("The code expired. Press Sign in to try again.".into())
                    }
                    Ok(PollResult::Approved { token, user }) => {
                        finish_sign_in(&app, &token, &user.handle);
                        return;
                    }
                    Err(e) if e.is_transient() && now() < deadline => continue,
                    Err(e) => return fail(format!("Sign-in failed: {e}")),
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn cancel_pairing(app: &AppHandle) {
    let st = app.state::<AppState>();
    st.sync.pairing_gen.fetch_add(1, Ordering::SeqCst);
    *st.sync.pairing.lock().unwrap() = None;
    *st.sync.pairing_error.lock().unwrap() = None;
    emit(app);
}

/// Sign out: revoke the token on the server (best effort, in the background) and
/// forget it locally. Local data stays; everything is re-sent after the next sign-in.
pub fn sign_out(app: &AppHandle) {
    let tok = token(app);
    let base = server(app);
    clear_local(app, "");
    nudge(app); // worker sees no token and exits
    emit(app);
    if let Some(t) = tok {
        let _ = std::thread::Builder::new().name("fraglink-signout".into()).spawn(move || {
            if let Err(e) = Client::new(&base, Some(t)).logout() {
                log::warn!("sync: server sign-out failed ({e}); the token stays valid until revoked on the website");
            }
        });
    }
}

#[cfg(test)]
mod server_default_tests {
    use super::*;
    #[test]
    fn legacy_localhost_migrates_unless_signed_in() {
        assert_eq!(pick_server(None, false), DEFAULT_SERVER);
        assert_eq!(DEFAULT_SERVER, "https://playfraglink.com");
        for ip in ["http://119.42.54.86", "http://119.42.54.86/"] {
            assert_eq!(pick_server(Some(ip.into()), true), DEFAULT_SERVER);
            assert_eq!(pick_server(Some(ip.into()), false), DEFAULT_SERVER);
        }
        assert_eq!(
            pick_server(Some("http://localhost:8787".into()), false),
            DEFAULT_SERVER
        );
        assert_eq!(
            pick_server(Some("http://localhost:8787".into()), true),
            "http://localhost:8787"
        );
        assert_eq!(
            pick_server(Some("https://my.example".into()), false),
            "https://my.example"
        );
    }
}
