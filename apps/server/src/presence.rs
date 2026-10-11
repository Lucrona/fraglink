//! Live presence over a WebSocket (M3).
//!
//! Each signed-in client (tray, later the website) keeps one socket open at
//! `GET /api/v1/ws`. The client sends its status and current game; the server
//! stores it and pushes changes to online friends. `invisible` is stored but
//! always shown to others as `offline`. A socket that sends nothing for
//! [`IDLE_TIMEOUT_SECS`] is closed and the user goes offline, so a killed client
//! disappears within ~2.5 min (spec §7: 60 s heartbeat, 150 s sweep).

use crate::auth::AuthUser;
use crate::error::ApiResult;
use crate::friends;
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const IDLE_TIMEOUT_SECS: u64 = 150;

/// Default cap on live presence sockets (about 3,500 fit a 2 GB VPS with Caddy in front).
pub const DEFAULT_MAX_SOCKETS: usize = 3500;

/// `FRAGLINK_MAX_WS`: live socket cap; past it new sockets get 503 + Retry-After so
/// clients back off instead of piling onto an overloaded box.
pub fn max_sockets() -> usize {
    static MAX: OnceLock<usize> = OnceLock::new();
    *MAX.get_or_init(|| {
        std::env::var("FRAGLINK_MAX_WS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_SOCKETS)
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PresenceView {
    /// `online`, `away` or `offline` (invisible users show as offline).
    pub status: String,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    /// Shown after the game name, e.g. the WoW realm (0.2.0-dev.12 clients).
    #[serde(default)]
    pub game_detail: Option<String>,
    pub session_started_at: Option<DateTime<Utc>>,
}

impl PresenceView {
    pub fn offline() -> Self {
        PresenceView {
            status: "offline".into(),
            ..Default::default()
        }
    }
    pub fn with_handle(self, handle: &str, started: bool) -> FriendPresence {
        FriendPresence {
            handle: handle.to_string(),
            started,
            presence: self,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FriendPresence {
    pub handle: String,
    /// True when this update means "just started playing `game_name`" (drives toasts).
    pub started: bool,
    #[serde(flatten)]
    pub presence: PresenceView,
}

/// Server → client.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Hello {
        friends: serde_json::Value,
    },
    Presence(FriendPresence),
    FriendRequest {
        handle: String,
        display_name: String,
    },
    FriendAccepted {
        handle: String,
    },
    FriendRemoved {
        handle: String,
    },
    Pong,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameRef {
    pub id: String,
    /// Ignored since 0.2.0-dev.14 (the server names games); older apps still send it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Unix seconds.
    pub started_at: Option<i64>,
    /// e.g. the WoW realm; optional (older clients don't send it).
    #[serde(default)]
    pub detail: Option<String>,
    /// "Show my game to my friends" is off: record the hours, show nobody (dev.14 apps).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
}

/// Client → server.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Presence {
        status: String,
        game: Option<GameRef>,
    },
    Ping,
}

/// In-process fan-out of messages to connected sockets (one process, see decisions D19).
#[derive(Default)]
pub struct Hub {
    conns: Mutex<
        HashMap<
            Uuid,
            Vec<(
                u64,
                mpsc::UnboundedSender<ServerMsg>,
                Arc<tokio::sync::Notify>,
            )>,
        >,
    >,
    next: AtomicU64,
    /// Sockets accepted or being upgraded (includes ones not yet in `conns`).
    live: AtomicUsize,
}

/// Holds one socket slot; released when dropped (also when an upgrade never happens).
pub struct SocketSlot(Arc<Hub>);

impl Drop for SocketSlot {
    fn drop(&mut self) {
        self.0.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Hub {
    /// Reserve a socket slot, or None when `max` sockets are already live.
    pub fn try_reserve(self: &Arc<Self>, max: usize) -> Option<SocketSlot> {
        if self.live.fetch_add(1, Ordering::SeqCst) >= max {
            self.live.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(SocketSlot(self.clone()))
    }
    pub fn live_sockets(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
    fn add(
        &self,
        user: Uuid,
        tx: mpsc::UnboundedSender<ServerMsg>,
        kill: Arc<tokio::sync::Notify>,
    ) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.conns
            .lock()
            .unwrap()
            .entry(user)
            .or_default()
            .push((id, tx, kill));
        id
    }
    /// Remove a connection; returns true if it was the user's last one.
    fn remove(&self, user: Uuid, id: u64) -> bool {
        let mut c = self.conns.lock().unwrap();
        if let Some(v) = c.get_mut(&user) {
            v.retain(|(i, _, _)| *i != id);
            if v.is_empty() {
                c.remove(&user);
                return true;
            }
            return false;
        }
        true
    }
    pub fn send(&self, user: Uuid, msg: ServerMsg) {
        if let Some(v) = self.conns.lock().unwrap().get(&user) {
            for (_, tx, _) in v {
                let _ = tx.send(msg.clone());
            }
        }
    }
    /// Drop every socket of `user` (banned, password reset): their writers end and
    /// close the sockets.
    pub fn kick(&self, user: Uuid) {
        if let Some(v) = self.conns.lock().unwrap().get(&user) {
            for (_, _, kill) in v {
                kill.notify_one();
            }
        }
    }
    /// Users with at least one live socket.
    pub fn online_users(&self) -> usize {
        self.conns.lock().unwrap().len()
    }
    pub fn is_online(&self, user: Uuid) -> bool {
        self.conns.lock().unwrap().contains_key(&user)
    }
    pub fn connection_count(&self) -> usize {
        self.conns.lock().unwrap().values().map(Vec::len).sum()
    }
}

type Row = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    bool,
);

/// `presence.game_name` holds the server's public name (NULL = unlisted), never a
/// client-sent one, and `game_id` the user's own (private) id.
fn to_view(r: Row) -> PresenceView {
    let (status, game_id, game_name, game_detail, started, heartbeat, hidden) = r;
    let stale = (Utc::now() - heartbeat).num_seconds() > IDLE_TIMEOUT_SECS as i64;
    if status == "invisible" || status == "offline" || stale {
        return PresenceView::offline();
    }
    let (game_id, game_name, game_detail, started) = match (game_id, game_name) {
        (Some(_), _) if hidden => (None, None, None, None),
        (Some(id), Some(name)) => (Some(id), Some(name), game_detail, started),
        (Some(_), None) => (
            Some(crate::names::UNLISTED_ID.to_string()),
            Some(crate::names::UNLISTED_NAME.to_string()),
            None,
            started,
        ),
        (None, _) => (None, None, None, None),
    };
    PresenceView {
        status,
        game_id,
        game_name,
        game_detail,
        session_started_at: started,
    }
}

/// What friends are allowed to see of `user` right now.
pub async fn visible_presence(st: &AppState, user: Uuid) -> ApiResult<Option<PresenceView>> {
    let r: Option<Row> = sqlx::query_as("SELECT status, game_id, game_name, game_detail, session_started_at, heartbeat_at, game_hidden FROM presence WHERE user_id = $1")
        .bind(user)
        .fetch_optional(&st.db)
        .await?;
    Ok(Some(r.map(to_view).unwrap_or_else(PresenceView::offline)))
}

pub async fn visible_presence_many(
    st: &AppState,
    users: &[Uuid],
) -> ApiResult<HashMap<Uuid, PresenceView>> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, Option<String>, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>, bool)> = sqlx::query_as(
        "SELECT user_id, status, game_id, game_name, game_detail, session_started_at, heartbeat_at, game_hidden FROM presence WHERE user_id = ANY($1)",
    )
    .bind(users)
    .fetch_all(&st.db)
    .await?;
    let mut m: HashMap<Uuid, PresenceView> = users
        .iter()
        .map(|u| (*u, PresenceView::offline()))
        .collect();
    for (u, s, gid, gname, gdetail, started, hb, hidden) in rows {
        m.insert(u, to_view((s, gid, gname, gdetail, started, hb, hidden)));
    }
    Ok(m)
}

fn clean(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Live sessions shorter than this aren't kept.
pub const MIN_LIVE_SECS: i64 = 30;
/// A session that lost its connection ends this long after the last heartbeat
/// (clients ping every 30 s).
const HEARTBEAT_GRACE_SECS: i64 = 45;
/// Same cap as uploaded sessions had (the sessions table checks it).
const MAX_SESSION_SECS: i64 = 24 * 3600;

/// When a session that was last confirmed at `heartbeat` ends if it ends `now`.
fn session_end(now: DateTime<Utc>, heartbeat: DateTime<Utc>) -> DateTime<Utc> {
    now.min(heartbeat + chrono::Duration::seconds(HEARTBEAT_GRACE_SECS))
}

/// Keep `start..end` of play the server watched live (split into <= 24 h rows).
pub async fn record_live_session(
    st: &AppState,
    user: Uuid,
    game_id: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> ApiResult<()> {
    if (end - start).num_seconds() < MIN_LIVE_SECS {
        return Ok(());
    }
    crate::names::ensure_game_row(st, game_id).await?;
    let mut from = start;
    while from < end {
        let to = end.min(from + chrono::Duration::seconds(MAX_SESSION_SECS));
        sqlx::query(
            "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
             VALUES($1, $2, $3, $4, $5, $6, 'live')",
        )
        .bind(Uuid::new_v4())
        .bind(user)
        .bind(game_id)
        .bind(from)
        .bind(to)
        .bind((to - from).num_seconds() as i32)
        .execute(&st.db)
        .await?;
        from = to;
    }
    Ok(())
}

/// Store a new presence and push it to online friends if what they see changed.
///
/// Hours (0.2.0-dev.14): the only time recorded is what the server observes here.
/// A game counts from the first presence update that names it until the update
/// that drops or changes it, the user goes offline, or the connection is lost
/// (then until the last heartbeat). The game id is the client's; its name is the
/// server's own ([`crate::names`]).
pub async fn set(
    st: &AppState,
    user: Uuid,
    handle: &str,
    status: &str,
    game: Option<GameRef>,
) -> ApiResult<()> {
    let status = match status {
        "online" | "away" | "invisible" | "offline" => status,
        _ => "online",
    };
    let game = game.filter(|_| status != "offline").and_then(|g| {
        let id = crate::names::canonical_id(user, &clean(&g.id, 120))?;
        let detail = g
            .detail
            .as_deref()
            .map(|d| clean(d, 64))
            .filter(|d| !d.is_empty());
        Some((id, detail, g.hidden))
    });
    // a retired game's program now counts for its successor
    let game = match game {
        Some((id, d, h)) => Some((crate::retire::effective_id(st, &id).await?, d, h)),
        None => None,
    };
    let now = Utc::now();
    let before = visible_presence(st, user)
        .await?
        .unwrap_or_else(PresenceView::offline);
    let prev: Option<(Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT game_id, session_started_at, heartbeat_at FROM presence WHERE user_id = $1",
    )
    .bind(user)
    .fetch_optional(&st.db)
    .await?;
    let (prev_game, prev_start, prev_hb) = match prev {
        Some((Some(g), Some(s), hb)) => (Some(g), Some(s), hb),
        Some((_, _, hb)) => (None, None, hb),
        None => (None, None, now),
    };
    let live = (now - prev_hb).num_seconds() <= IDLE_TIMEOUT_SECS as i64;
    let new_id = game.as_ref().map(|g| g.0.clone());
    let continuing = live && prev_game.is_some() && prev_game == new_id;
    if let (Some(pg), Some(ps), false) = (&prev_game, prev_start, continuing) {
        record_live_session(st, user, pg, ps, session_end(now, prev_hb)).await?;
    }
    let started_at = match (&new_id, continuing) {
        (Some(_), true) => prev_start,
        (Some(_), false) => Some(now),
        (None, _) => None,
    };
    let public = match &new_id {
        Some(id) => {
            let n = crate::names::public_name(st, id).await?;
            if n.is_none() {
                crate::names::lookup_steam_name(st, id);
            }
            n
        }
        None => None,
    };
    sqlx::query(
        "INSERT INTO presence(user_id, status, game_id, game_name, session_started_at, game_detail, game_hidden, heartbeat_at, updated_at)
         VALUES($1, $2, $3, $4, $5, $6, $7, now(), now())
         ON CONFLICT (user_id) DO UPDATE SET status = excluded.status, game_id = excluded.game_id,
             game_name = excluded.game_name, session_started_at = excluded.session_started_at,
             game_detail = excluded.game_detail, game_hidden = excluded.game_hidden,
             heartbeat_at = now(), updated_at = now()",
    )
    .bind(user)
    .bind(status)
    .bind(&new_id)
    .bind(&public)
    .bind(started_at)
    .bind(game.as_ref().and_then(|g| g.1.clone()))
    .bind(game.as_ref().is_some_and(|g| g.2))
    .execute(&st.db)
    .await?;
    let after = visible_presence(st, user)
        .await?
        .unwrap_or_else(PresenceView::offline);
    if after != before {
        let started = after.game_id.is_some() && after.game_id != before.game_id;
        let msg = ServerMsg::Presence(after.with_handle(handle, started));
        for f in friends::friend_ids(st, user).await? {
            st.hub.send(f, msg.clone());
        }
    }
    Ok(())
}

async fn heartbeat(st: &AppState, user: Uuid) -> ApiResult<()> {
    sqlx::query("UPDATE presence SET heartbeat_at = now() WHERE user_id = $1")
        .bind(user)
        .execute(&st.db)
        .await?;
    Ok(())
}

/// At startup nobody is connected yet: keep the sessions that were open (up to
/// their last heartbeat) and mark everyone offline.
pub async fn reset_all(st: &AppState) -> ApiResult<()> {
    let open: Vec<(Uuid, String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, game_id, session_started_at, heartbeat_at FROM presence
         WHERE game_id IS NOT NULL AND session_started_at IS NOT NULL",
    )
    .fetch_all(&st.db)
    .await?;
    let now = Utc::now();
    for (user, game, start, hb) in open {
        if let Err(e) = record_live_session(st, user, &game, start, session_end(now, hb)).await {
            tracing::warn!("keeping open session of {user} failed: {}", e.message);
        }
    }
    sqlx::query("UPDATE presence SET status = 'offline', game_id = NULL, game_name = NULL, game_detail = NULL, session_started_at = NULL, game_hidden = false WHERE status <> 'offline' OR game_id IS NOT NULL")
        .execute(&st.db)
        .await?;
    Ok(())
}

pub async fn ws_handler(State(st): State<AppState>, a: AuthUser, ws: WebSocketUpgrade) -> Response {
    let Some(slot) = st.hub.try_reserve(max_sockets()) else {
        tracing::warn!(
            "presence: {} sockets live, refusing new ones (503)",
            st.hub.live_sockets()
        );
        return crate::error::AppError::overloaded().into_response();
    };
    ws.max_message_size(16 * 1024)
        .on_upgrade(move |socket| async move {
            let _slot = slot;
            connection(st, a, socket).await
        })
}

async fn connection(st: AppState, a: AuthUser, socket: WebSocket) {
    let user = a.user.id;
    let handle = a.user.handle.clone();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let kill = Arc::new(tokio::sync::Notify::new());
    let conn_id = st.hub.add(user, tx.clone(), kill.clone());

    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            let Ok(text) = serde_json::to_string(&m) else {
                continue;
            };
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    if let Ok(l) = friends::list(&st, user).await {
        let _ = tx.send(ServerMsg::Hello {
            friends: serde_json::to_value(l).unwrap_or_default(),
        });
    }

    loop {
        let next = tokio::select! {
            n = tokio::time::timeout(
                Duration::from_secs(st.cfg.presence_idle_secs),
                stream.next(),
            ) => n,
            _ = kill.notified() => break, // signed out by staff
        };
        let msg = match next {
            Ok(Some(Ok(m))) => m,
            _ => break, // timeout, error or closed
        };
        match msg {
            Message::Text(t) => match serde_json::from_str::<ClientMsg>(&t) {
                Ok(ClientMsg::Presence { status, game }) => {
                    if let Err(e) = set(&st, user, &handle, &status, game).await {
                        tracing::warn!("presence set failed: {}", e.message);
                    }
                }
                Ok(ClientMsg::Ping) => {
                    let _ = heartbeat(&st, user).await;
                    let _ = tx.send(ServerMsg::Pong);
                }
                Err(_) => {
                    let _ = tx.send(ServerMsg::Error {
                        message: "unknown message".into(),
                    });
                }
            },
            Message::Ping(_) | Message::Pong(_) => {
                let _ = heartbeat(&st, user).await;
            }
            Message::Close(_) => break,
            Message::Binary(_) => {}
        }
    }

    drop(tx);
    let last = st.hub.remove(user, conn_id);
    writer.abort();
    if last {
        if let Err(e) = set(&st, user, &handle, "offline", None).await {
            tracing::warn!("presence offline failed: {}", e.message);
        }
    }
}
