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
use axum::response::Response;
use chrono::{DateTime, TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

pub const IDLE_TIMEOUT_SECS: u64 = 150;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PresenceView {
    /// `online`, `away` or `offline` (invisible users show as offline).
    pub status: String,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
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
    pub name: String,
    /// Unix seconds.
    pub started_at: Option<i64>,
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
    conns: Mutex<HashMap<Uuid, Vec<(u64, mpsc::UnboundedSender<ServerMsg>)>>>,
    next: AtomicU64,
}

impl Hub {
    fn add(&self, user: Uuid, tx: mpsc::UnboundedSender<ServerMsg>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        self.conns
            .lock()
            .unwrap()
            .entry(user)
            .or_default()
            .push((id, tx));
        id
    }
    /// Remove a connection; returns true if it was the user's last one.
    fn remove(&self, user: Uuid, id: u64) -> bool {
        let mut c = self.conns.lock().unwrap();
        if let Some(v) = c.get_mut(&user) {
            v.retain(|(i, _)| *i != id);
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
            for (_, tx) in v {
                let _ = tx.send(msg.clone());
            }
        }
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
    Option<DateTime<Utc>>,
    DateTime<Utc>,
);

fn to_view(r: Row) -> PresenceView {
    let (status, game_id, game_name, started, heartbeat) = r;
    let stale = (Utc::now() - heartbeat).num_seconds() > IDLE_TIMEOUT_SECS as i64;
    if status == "invisible" || status == "offline" || stale {
        return PresenceView::offline();
    }
    PresenceView {
        status,
        game_id,
        game_name,
        session_started_at: started,
    }
}

/// What friends are allowed to see of `user` right now.
pub async fn visible_presence(st: &AppState, user: Uuid) -> ApiResult<Option<PresenceView>> {
    let r: Option<Row> = sqlx::query_as("SELECT status, game_id, game_name, session_started_at, heartbeat_at FROM presence WHERE user_id = $1")
        .bind(user)
        .fetch_optional(&st.db)
        .await?;
    Ok(Some(r.map(to_view).unwrap_or_else(PresenceView::offline)))
}

pub async fn visible_presence_many(
    st: &AppState,
    users: &[Uuid],
) -> ApiResult<HashMap<Uuid, PresenceView>> {
    let rows: Vec<(Uuid, String, Option<String>, Option<String>, Option<DateTime<Utc>>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, status, game_id, game_name, session_started_at, heartbeat_at FROM presence WHERE user_id = ANY($1)",
    )
    .bind(users)
    .fetch_all(&st.db)
    .await?;
    let mut m: HashMap<Uuid, PresenceView> = users
        .iter()
        .map(|u| (*u, PresenceView::offline()))
        .collect();
    for (u, s, gid, gname, started, hb) in rows {
        m.insert(u, to_view((s, gid, gname, started, hb)));
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

/// Store a new presence and push it to online friends if what they see changed.
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
        let id = clean(&g.id, 120);
        let name = clean(&g.name, 120);
        (!id.is_empty()).then_some((
            id,
            if name.is_empty() {
                "a game".to_string()
            } else {
                name
            },
            g.started_at,
        ))
    });
    let before = visible_presence(st, user)
        .await?
        .unwrap_or_else(PresenceView::offline);
    let started_at = game
        .as_ref()
        .and_then(|g| g.2)
        .and_then(|t| Utc.timestamp_opt(t, 0).single())
        .map(|t| t.min(Utc::now()));
    sqlx::query(
        "INSERT INTO presence(user_id, status, game_id, game_name, session_started_at, heartbeat_at, updated_at)
         VALUES($1, $2, $3, $4, $5, now(), now())
         ON CONFLICT (user_id) DO UPDATE SET status = excluded.status, game_id = excluded.game_id,
             game_name = excluded.game_name, session_started_at = excluded.session_started_at,
             heartbeat_at = now(), updated_at = now()",
    )
    .bind(user)
    .bind(status)
    .bind(game.as_ref().map(|g| g.0.clone()))
    .bind(game.as_ref().map(|g| g.1.clone()))
    .bind(started_at)
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

/// At startup nobody is connected yet.
pub async fn reset_all(st: &AppState) -> ApiResult<()> {
    sqlx::query("UPDATE presence SET status = 'offline', game_id = NULL, game_name = NULL, session_started_at = NULL WHERE status <> 'offline'")
        .execute(&st.db)
        .await?;
    Ok(())
}

pub async fn ws_handler(State(st): State<AppState>, a: AuthUser, ws: WebSocketUpgrade) -> Response {
    ws.max_message_size(16 * 1024)
        .on_upgrade(move |socket| connection(st, a, socket))
}

async fn connection(st: AppState, a: AuthUser, socket: WebSocket) {
    let user = a.user.id;
    let handle = a.user.handle.clone();
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<ServerMsg>();
    let conn_id = st.hub.add(user, tx.clone());

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
        let next = tokio::time::timeout(
            Duration::from_secs(st.cfg.presence_idle_secs),
            stream.next(),
        )
        .await;
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
