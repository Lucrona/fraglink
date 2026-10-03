//! Blocking client for the presence WebSocket (`GET /api/v1/ws`).
//!
//! The tray keeps one socket open while signed in, sends its status and current
//! game when they change plus a ping every 30 s, and receives friends' presence
//! and friend-request events. Reads use a short socket timeout so one thread can
//! both send and receive.

use crate::SyncError;
use serde::{Deserialize, Serialize};
use std::net::TcpStream;
use std::time::Duration;
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameRef {
    pub id: String,
    pub name: String,
    /// Unix seconds.
    pub started_at: Option<i64>,
}

/// What this client publishes. `invisible` is stored but friends see offline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MyPresence {
    pub status: String,
    pub game: Option<GameRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FriendPresence {
    pub handle: String,
    #[serde(default)]
    pub started: bool,
    pub status: String,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub session_started_at: Option<String>,
}

/// Server → client events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
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

pub struct PresenceSocket {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
}

fn set_timeout(ws: &mut WebSocket<MaybeTlsStream<TcpStream>>, d: Option<Duration>) {
    let tcp: &TcpStream = match ws.get_ref() {
        MaybeTlsStream::Plain(s) => s,
        MaybeTlsStream::Rustls(s) => &s.sock,
        _ => return,
    };
    let _ = tcp.set_read_timeout(d);
}

impl PresenceSocket {
    pub fn connect(base: &str, token: &str) -> Result<Self, SyncError> {
        let ws_url = if let Some(r) = base.strip_prefix("https://") {
            format!("wss://{r}/api/v1/ws")
        } else if let Some(r) = base.strip_prefix("http://") {
            format!("ws://{r}/api/v1/ws")
        } else {
            return Err(SyncError::Network("bad server address".into()));
        };
        let mut req = ws_url
            .into_client_request()
            .map_err(|e| SyncError::Network(e.to_string()))?;
        req.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| SyncError::Unauthorized)?,
        );
        req.headers_mut().insert(
            "user-agent",
            concat!("fraglink-desktop/", env!("CARGO_PKG_VERSION"))
                .parse()
                .expect("static header"),
        );
        match tungstenite::connect(req) {
            Ok((mut ws, _)) => {
                set_timeout(&mut ws, Some(Duration::from_millis(500)));
                Ok(PresenceSocket { ws })
            }
            Err(tungstenite::Error::Http(r)) if r.status() == 401 => Err(SyncError::Unauthorized),
            Err(tungstenite::Error::Http(r)) => Err(SyncError::Server {
                status: r.status().as_u16(),
                code: String::new(),
                message: "presence connection refused".into(),
            }),
            Err(e) => Err(SyncError::Network(e.to_string())),
        }
    }

    fn send_json(&mut self, v: serde_json::Value) -> Result<(), SyncError> {
        self.ws
            .send(Message::Text(v.to_string().into()))
            .map_err(|e| SyncError::Network(e.to_string()))
    }

    pub fn send_presence(&mut self, p: &MyPresence) -> Result<(), SyncError> {
        self.send_json(
            serde_json::json!({ "type": "presence", "status": p.status, "game": p.game }),
        )
    }

    pub fn ping(&mut self) -> Result<(), SyncError> {
        self.send_json(serde_json::json!({ "type": "ping" }))
    }

    /// Wait up to the socket timeout (500 ms) for one event. `Ok(None)` = nothing yet.
    pub fn poll(&mut self) -> Result<Option<Event>, SyncError> {
        match self.ws.read() {
            Ok(Message::Text(t)) => Ok(serde_json::from_str::<Event>(&t).ok()),
            Ok(Message::Close(_)) => Err(SyncError::Network("server closed the connection".into())),
            Ok(_) => Ok(None),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                Ok(None)
            }
            Err(e) => Err(SyncError::Network(e.to_string())),
        }
    }

    pub fn close(mut self) {
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_parse() {
        let e: Event = serde_json::from_str(r#"{"type":"presence","handle":"bob","started":true,"status":"online","game_id":"wow","game_name":"World of Warcraft","session_started_at":"2026-09-26T10:00:00Z"}"#).unwrap();
        match e {
            Event::Presence(p) => {
                assert!(p.started);
                assert_eq!(p.game_name.as_deref(), Some("World of Warcraft"));
            }
            other => panic!("{other:?}"),
        }
        let e: Event =
            serde_json::from_str(r#"{"type":"friend_request","handle":"a","display_name":"A"}"#)
                .unwrap();
        assert_eq!(
            e,
            Event::FriendRequest {
                handle: "a".into(),
                display_name: "A".into()
            }
        );
    }

    #[test]
    fn refuses_unreachable() {
        assert!(PresenceSocket::connect("http://127.0.0.1:9", "t")
            .err()
            .unwrap()
            .is_transient());
        assert!(PresenceSocket::connect("ftp://x", "t").is_err());
    }
}
