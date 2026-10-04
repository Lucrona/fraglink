//! Friends and presence (M3). Only runs while signed in to sync: one background
//! thread keeps a WebSocket to the server, publishes this PC's status and the
//! game being tracked right now, and receives friends' presence. Signed out,
//! nothing here touches the network and the Friends tab shows sample data.
//!
//! Privacy: "Invisible" is sent as invisible and the server shows it to friends
//! as offline. Excluded games are never tracked, so they never appear here.

use crate::{now, sync, AppState, ToastPayload};
use fraglink_core::engine::keys as core_keys;
use fraglink_sync::presence::{Event, FriendPresence, GameRef, MyPresence, PresenceSocket};
use fraglink_sync::{Client, SyncError};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const FRIENDS_EVENT: &str = "fraglink://friends";
/// Keep-alive. The server drops a socket after 150 s of silence, but NAT routers and
/// proxies on the way can cut idle TCP connections after 60 s (seen in testing), so
/// ping well inside that. Discord heartbeats about every 41 s.
const PING_EVERY: Duration = Duration::from_secs(30);
/// At most one "started playing" toast per friend in this window.
const TOAST_GAP_SECS: i64 = 30 * 60;

pub mod keys {
    /// "0" turns friend toasts off (the global `toasts` switch also applies).
    pub const FRIEND_TOASTS: &str = "friend_toasts";
    /// "1" (default) = no friend toasts while I'm in a game.
    pub const FRIEND_TOASTS_DND: &str = "friend_toasts_dnd";
    /// Comma-separated lower-case handles whose toasts are muted.
    pub const MUTED: &str = "muted_friends";
}

#[derive(Default)]
pub struct FriendsShared {
    running: AtomicBool,
    connected: AtomicBool,
    /// Latest `FriendsList` JSON from the server (hello or REST refresh).
    pub(crate) list: Mutex<Option<Value>>,
    last_toast: Mutex<HashMap<String, i64>>,
    /// Set by the UI to re-fetch the friends list.
    refresh: AtomicBool,
    /// Set when status changes so the thread republishes right away.
    dirty: AtomicBool,
}

fn setting(app: &AppHandle, k: &str) -> Option<String> {
    let st = app.state::<AppState>();
    let e = st.engine.lock().unwrap();
    e.tracker.store.get_setting(k).ok().flatten()
}

pub fn emit(app: &AppHandle) {
    let _ = app.emit(FRIENDS_EVENT, ());
}

/// What I publish right now: status setting + the game being tracked (if any).
pub fn my_presence(app: &AppHandle) -> MyPresence {
    let st = app.state::<AppState>();
    let e = st.engine.lock().unwrap();
    let mut status = e
        .tracker
        .store
        .get_setting(core_keys::STATUS)
        .ok()
        .flatten()
        .filter(|s| ["online", "away", "invisible"].contains(&s.as_str()))
        .unwrap_or_else(|| "online".into());
    // automatic Away (10 min without input) only replaces Online
    if status == "online" && st.auto_away.load(std::sync::atomic::Ordering::SeqCst) {
        status = "away".into();
    }
    // Options > Privacy: "Show my game to my friends"
    let share = e
        .tracker
        .store
        .get_setting("share_game")
        .ok()
        .flatten()
        .as_deref()
        != Some("0");
    let game = e.now_playing().filter(|_| share).map(|n| GameRef {
        id: n.game_id,
        name: n.name,
        started_at: Some(n.started_at),
        detail: n.detail,
    });
    MyPresence { status, game }
}

/// Toast decision, kept pure for tests.
pub struct ToastPrefs {
    pub toasts: bool,
    pub friend_toasts: bool,
    pub dnd: bool,
    pub muted: Vec<String>,
}

pub fn should_toast(
    p: &FriendPresence,
    prefs: &ToastPrefs,
    i_am_playing: bool,
    last: Option<i64>,
    now: i64,
) -> bool {
    p.started
        && p.game_name.is_some()
        && prefs.toasts
        && prefs.friend_toasts
        && !(prefs.dnd && i_am_playing)
        && !prefs.muted.contains(&p.handle.to_lowercase())
        && last.is_none_or(|t| now - t >= TOAST_GAP_SECS)
}

/// "<name> logged on": they were offline in our list and are now online/away.
pub fn signed_on(old: Option<&str>, new: &str) -> bool {
    old == Some("offline") && matches!(new, "online" | "away")
}

pub fn should_toast_logon(
    handle: &str,
    prefs: &ToastPrefs,
    i_am_playing: bool,
    last: Option<i64>,
    now: i64,
) -> bool {
    prefs.toasts
        && prefs.friend_toasts
        && !(prefs.dnd && i_am_playing)
        && !prefs.muted.contains(&handle.to_lowercase())
        && last.is_none_or(|t| now - t >= TOAST_GAP_SECS)
}

fn old_status(list: &Option<Value>, handle: &str) -> Option<String> {
    list.as_ref()?
        .get("friends")?
        .as_array()?
        .iter()
        .find(|f| f.get("handle").and_then(|h| h.as_str()) == Some(handle))?
        .get("presence")?
        .get("status")?
        .as_str()
        .map(String::from)
}

fn prefs(app: &AppHandle) -> ToastPrefs {
    ToastPrefs {
        toasts: setting(app, core_keys::TOASTS).as_deref() != Some("0"),
        friend_toasts: setting(app, keys::FRIEND_TOASTS).as_deref() != Some("0"),
        dnd: setting(app, keys::FRIEND_TOASTS_DND).as_deref() != Some("0"),
        muted: muted(app),
    }
}

pub fn muted(app: &AppHandle) -> Vec<String> {
    setting(app, keys::MUTED)
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn set_muted(app: &AppHandle, handle: &str, mute: bool) {
    let h = handle.trim().trim_start_matches('@').to_lowercase();
    let mut m = muted(app);
    m.retain(|x| x != &h);
    if mute && !h.is_empty() {
        m.push(h);
    }
    m.truncate(500);
    let st = app.state::<AppState>();
    let e = st.engine.lock().unwrap();
    let _ = e.tracker.store.set_setting(keys::MUTED, &m.join(","));
}

/// Friends state for the UI. `list` is None while signed out or before the
/// first successful fetch; the UI then shows sample data or "connecting".
pub fn view(app: &AppHandle) -> Value {
    let st = app.state::<AppState>();
    let signed_in = sync::signed_in(app);
    serde_json::json!({
        "signed_in": signed_in,
        "connected": signed_in && st.friends.connected.load(Ordering::SeqCst),
        "me": setting(app, sync::keys::USER),
        "list": if signed_in { st.friends.list.lock().unwrap().clone() } else { None },
        "muted": muted(app),
    })
}

/// Start the presence thread (no-op when already running or signed out).
pub fn start(app: &AppHandle) {
    let st = app.state::<AppState>();
    if !sync::signed_in(app) || st.friends.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let app2 = app.clone();
    let spawned = std::thread::Builder::new()
        .name("fraglink-presence".into())
        .spawn(move || {
            run(&app2);
            let st = app2.state::<AppState>();
            st.friends.connected.store(false, Ordering::SeqCst);
            *st.friends.list.lock().unwrap() = None;
            st.friends.running.store(false, Ordering::SeqCst);
            emit(&app2);
            log::info!("presence: stopped");
        });
    if spawned.is_err() {
        st.friends.running.store(false, Ordering::SeqCst);
    }
}

/// Republish soon (status changed, or a game started/stopped).
pub fn nudge(app: &AppHandle) {
    app.state::<AppState>()
        .friends
        .dirty
        .store(true, Ordering::SeqCst);
}

pub fn request_refresh(app: &AppHandle) {
    app.state::<AppState>()
        .friends
        .refresh
        .store(true, Ordering::SeqCst);
}

fn sleep_while_signed_in(app: &AppHandle, secs: u64) -> bool {
    for _ in 0..secs {
        if !sync::signed_in(app) {
            return false;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    sync::signed_in(app)
}

fn run(app: &AppHandle) {
    let st = app.state::<AppState>();
    let mut backoff = 2u64;
    log::info!("presence: started");
    while let Some(client) = sync::client(app) {
        let mut sock = match client.presence() {
            Ok(s) => s,
            Err(SyncError::Unauthorized) => {
                log::warn!("presence: token refused; the sync worker will sign out");
                sync::nudge(app);
                return;
            }
            Err(e) => {
                log::info!("presence: can't connect ({e}); retry in {backoff}s");
                if !sleep_while_signed_in(app, backoff) {
                    return;
                }
                backoff = (backoff * 2).min(60);
                continue;
            }
        };
        backoff = 2;
        log::info!("presence: connected");
        st.friends.connected.store(true, Ordering::SeqCst);
        emit(app);
        let reason = session(app, &client, &mut sock);
        st.friends.connected.store(false, Ordering::SeqCst);
        emit(app);
        match reason {
            Ok(()) => {
                sock.close();
                return; // signed out
            }
            Err(e) => {
                log::info!("presence: connection lost ({e}); reconnecting");
                if !sleep_while_signed_in(app, 2) {
                    return;
                }
            }
        }
    }
}

/// One connected session. Ok(()) = signed out; Err = reconnect.
fn session(app: &AppHandle, client: &Client, sock: &mut PresenceSocket) -> Result<(), SyncError> {
    let st = app.state::<AppState>();
    let mut sent = my_presence(app);
    sock.send_presence(&sent)?;
    let mut last_ping = Instant::now();
    let mut last_check = Instant::now();
    loop {
        if !sync::signed_in(app) {
            return Ok(());
        }
        // poll() waits up to the socket read timeout (500 ms).
        if let Some(ev) = sock.poll()? {
            handle(app, client, ev);
        }
        if st.friends.refresh.swap(false, Ordering::SeqCst) {
            refresh(app, client);
        }
        if st.friends.dirty.swap(false, Ordering::SeqCst)
            || last_check.elapsed() >= Duration::from_secs(3)
        {
            last_check = Instant::now();
            let cur = my_presence(app);
            if cur != sent {
                sock.send_presence(&cur)?;
                sent = cur;
            }
        }
        if last_ping.elapsed() >= PING_EVERY {
            sock.ping()?;
            last_ping = Instant::now();
        }
    }
}

fn refresh(app: &AppHandle, client: &Client) {
    match client.friends() {
        Ok(v) => {
            *app.state::<AppState>().friends.list.lock().unwrap() = Some(v);
            emit(app);
        }
        Err(e) => log::info!("presence: friends refresh failed: {e}"),
    }
}

/// Apply a presence update to the cached list.
pub fn apply_presence(list: &mut Value, p: &FriendPresence) {
    if let Some(arr) = list.get_mut("friends").and_then(|f| f.as_array_mut()) {
        for f in arr {
            if f.get("handle").and_then(|h| h.as_str()) == Some(p.handle.as_str()) {
                f["presence"] = serde_json::json!({
                    "status": p.status,
                    "game_id": p.game_id,
                    "game_name": p.game_name,
                    "game_detail": p.game_detail,
                    "session_started_at": p.session_started_at,
                });
            }
        }
    }
}

fn display_name(list: &Option<Value>, handle: &str) -> String {
    list.as_ref()
        .and_then(|l| l.get("friends")?.as_array().cloned())
        .and_then(|a| {
            a.into_iter()
                .find(|f| f.get("handle").and_then(|h| h.as_str()) == Some(handle))
        })
        .and_then(|f| f.get("display_name")?.as_str().map(String::from))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| handle.to_string())
}

fn handle(app: &AppHandle, client: &Client, ev: Event) {
    let st = app.state::<AppState>();
    match ev {
        Event::Hello { friends } => {
            *st.friends.list.lock().unwrap() = Some(friends);
            emit(app);
        }
        Event::Presence(p) => {
            let (name, was) = {
                let mut l = st.friends.list.lock().unwrap();
                let was = old_status(&l, &p.handle);
                if let Some(v) = l.as_mut() {
                    apply_presence(v, &p);
                }
                (display_name(&l, &p.handle), was)
            };
            emit(app);
            let playing = st.engine.lock().unwrap().tracker.now_playing().is_some();
            let key = p.handle.to_lowercase();
            let last = st.friends.last_toast.lock().unwrap().get(&key).copied();
            if should_toast(&p, &prefs(app), playing, last, now()) {
                st.friends.last_toast.lock().unwrap().insert(key, now());
                crate::windows::show_toast(
                    app,
                    ToastPayload {
                        kind: format!("friend:{}", p.handle),
                        title: name,
                        line: "started playing".into(),
                        game_id: p.game_id.clone().unwrap_or_default(),
                        game: p.game_name.clone().unwrap_or_default(),
                        icon: crate::icons::one(app, p.game_id.as_deref().unwrap_or(""), true),
                    },
                );
            } else if signed_on(was.as_deref(), &p.status)
                && setting(app, "friend_toasts_logon").as_deref() != Some("0")
                && should_toast_logon(&p.handle, &prefs(app), playing, last, now())
            {
                st.friends.last_toast.lock().unwrap().insert(key, now());
                crate::windows::show_toast(
                    app,
                    ToastPayload {
                        kind: format!("friend:{}", p.handle),
                        title: name,
                        line: "logged on".into(),
                        game_id: String::new(),
                        game: String::new(),
                        icon: None,
                    },
                );
            }
        }
        Event::FriendRequest {
            handle,
            display_name,
        } => {
            refresh(app, client);
            let p = prefs(app);
            if p.toasts && p.friend_toasts && !p.muted.contains(&handle.to_lowercase()) {
                crate::windows::show_toast(
                    app,
                    ToastPayload {
                        kind: format!("request:{handle}"),
                        title: if display_name.is_empty() {
                            handle.clone()
                        } else {
                            display_name
                        },
                        line: "wants to be your friend".into(),
                        game_id: String::new(),
                        game: format!("@{handle}"),
                        icon: None,
                    },
                );
            }
        }
        Event::FriendAccepted { .. } | Event::FriendRemoved { .. } => refresh(app, client),
        Event::Pong => {}
        Event::Error { message } => log::warn!("presence: server said: {message}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(started: bool) -> FriendPresence {
        FriendPresence {
            handle: "Bob".into(),
            started,
            status: "online".into(),
            game_id: Some("steam:570".into()),
            game_name: Some("Dota 2".into()),
            game_detail: None,
            session_started_at: None,
        }
    }
    fn prefs() -> ToastPrefs {
        ToastPrefs {
            toasts: true,
            friend_toasts: true,
            dnd: true,
            muted: vec![],
        }
    }

    #[test]
    fn logon_rules() {
        assert!(signed_on(Some("offline"), "online"));
        assert!(signed_on(Some("offline"), "away"));
        assert!(!signed_on(None, "online")); // first snapshot: no toast
        assert!(!signed_on(Some("online"), "online"));
        assert!(!signed_on(Some("offline"), "offline"));
        assert!(should_toast_logon("a", &prefs(), false, None, 1000));
        assert!(!should_toast_logon("a", &prefs(), true, None, 1000));
    }

    #[test]
    fn toast_rules() {
        assert!(should_toast(&fp(true), &prefs(), false, None, 1000));
        assert!(
            !should_toast(&fp(false), &prefs(), false, None, 1000),
            "only on start"
        );
        assert!(
            !should_toast(&fp(true), &prefs(), true, None, 1000),
            "DND while I play"
        );
        let no_dnd = ToastPrefs {
            dnd: false,
            ..prefs()
        };
        assert!(should_toast(&fp(true), &no_dnd, true, None, 1000));
        assert!(
            !should_toast(&fp(true), &prefs(), false, Some(1000), 1000 + 60),
            "30 min gap"
        );
        assert!(should_toast(
            &fp(true),
            &prefs(),
            false,
            Some(0),
            TOAST_GAP_SECS
        ));
        let muted = ToastPrefs {
            muted: vec!["bob".into()],
            ..prefs()
        };
        assert!(!should_toast(&fp(true), &muted, false, None, 1000));
        let off = ToastPrefs {
            toasts: false,
            ..prefs()
        };
        assert!(!should_toast(&fp(true), &off, false, None, 1000));
        let off = ToastPrefs {
            friend_toasts: false,
            ..prefs()
        };
        assert!(!should_toast(&fp(true), &off, false, None, 1000));
    }

    #[test]
    fn presence_applied_to_list() {
        let mut l = serde_json::json!({"friends":[{"handle":"Bob","display_name":"B","presence":{"status":"offline"}}],"incoming":[]});
        apply_presence(&mut l, &fp(true));
        assert_eq!(l["friends"][0]["presence"]["game_name"], "Dota 2");
        assert_eq!(l["friends"][0]["presence"]["status"], "online");
        assert_eq!(display_name(&Some(l), "Bob"), "B");
    }
}
