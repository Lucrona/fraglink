//! Blocking client for the Fraglink server API (`apps/server`).
//!
//! Nothing here runs unless the user explicitly signs in to sync: the desktop
//! app only constructs a [`Client`] after the user presses "Sign in to sync".

pub mod presence;
pub mod secret;

use fraglink_core::outbox::UploadBatch;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The production server (HTTP on the IP until the domain is set up).
pub const DEFAULT_SERVER: &str = "http://119.42.54.86";
/// The default before 0.2.0-dev.6. A saved copy of it only counts when signed in (a local dev server).
pub const LEGACY_DEFAULT_SERVER: &str = "http://localhost:8787";

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// The server rejected our token (revoked from the website, or account deleted).
    #[error("signed out by the server")]
    Unauthorized,
    #[error("server error {status}: {message}")]
    Server {
        status: u16,
        code: String,
        message: String,
    },
    #[error("can't reach the server: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Decode(String),
}

impl SyncError {
    /// Transient errors are retried later; the local data is untouched either way.
    pub fn is_transient(&self) -> bool {
        match self {
            SyncError::Network(_) => true,
            SyncError::Server { status, .. } => *status >= 500 || *status == 429,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub expires_in: i64,
    pub interval: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RemoteUser {
    pub id: String,
    pub email: String,
    pub handle: String,
    #[serde(default)]
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PollResult {
    Pending,
    Approved {
        token: String,
        user: RemoteUser,
    },
    /// Code expired or already used: start pairing again.
    Expired,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UploadResult {
    pub accepted: Vec<String>,
    pub rejected: Vec<RejectedSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RejectedSession {
    pub id: String,
    pub reason: String,
}

#[derive(Clone)]
pub struct Client {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct ErrBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    message: String,
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("fraglink-desktop/", env!("CARGO_PKG_VERSION")))
            .build();
        Client {
            base: base.trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn req(&self, method: &str, path: &str) -> ureq::Request {
        let r = self
            .agent
            .request(method, &format!("{}{}", self.base, path));
        match &self.token {
            Some(t) => r.set("Authorization", &format!("Bearer {t}")),
            None => r,
        }
    }

    fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, SyncError> {
        let r = self.req(method, path);
        let res = match body {
            Some(b) => r.send_json(b),
            None => r.call(),
        };
        match res {
            Ok(resp) => {
                if resp.status() == 204 {
                    return serde_json::from_value(serde_json::Value::Null)
                        .map_err(|e| SyncError::Decode(e.to_string()));
                }
                resp.into_json::<T>()
                    .map_err(|e| SyncError::Decode(e.to_string()))
            }
            Err(ureq::Error::Status(401, _)) => Err(SyncError::Unauthorized),
            Err(ureq::Error::Status(status, resp)) => {
                let b: ErrBody = resp.into_json().unwrap_or(ErrBody {
                    error: String::new(),
                    message: String::new(),
                });
                Err(SyncError::Server {
                    status,
                    code: b.error,
                    message: b.message,
                })
            }
            Err(ureq::Error::Transport(t)) => Err(SyncError::Network(t.to_string())),
        }
    }

    pub fn health(&self) -> Result<serde_json::Value, SyncError> {
        self.call("GET", "/api/v1/health", None)
    }

    pub fn start_pairing(&self, device_name: &str) -> Result<DeviceStart, SyncError> {
        self.call(
            "POST",
            "/api/v1/device/start",
            Some(serde_json::json!({ "device_name": device_name })),
        )
    }

    pub fn poll(&self, device_code: &str) -> Result<PollResult, SyncError> {
        let r: Result<serde_json::Value, _> = self.call(
            "POST",
            "/api/v1/device/poll",
            Some(serde_json::json!({ "device_code": device_code })),
        );
        match r {
            Ok(v) if v["status"] == "approved" => {
                let token = v["token"]
                    .as_str()
                    .ok_or_else(|| SyncError::Decode("missing token".into()))?
                    .to_string();
                let user = serde_json::from_value(v["user"].clone())
                    .map_err(|e| SyncError::Decode(e.to_string()))?;
                Ok(PollResult::Approved { token, user })
            }
            Ok(_) => Ok(PollResult::Pending),
            Err(SyncError::Server {
                status: 404 | 410, ..
            }) => Ok(PollResult::Expired),
            Err(e) => Err(e),
        }
    }

    /// Username (or email) + password sign-in for this PC. Returns a device token.
    pub fn password_login(
        &self,
        login: &str,
        password: &str,
        device_name: &str,
    ) -> Result<(String, RemoteUser), SyncError> {
        let v: serde_json::Value = self.call(
            "POST",
            "/api/v1/device/login",
            Some(serde_json::json!({ "login": login, "password": password, "device_name": device_name })),
        )?;
        token_and_user(v)
    }

    /// Create an account (email optional) and sign this PC in.
    pub fn password_signup(
        &self,
        handle: &str,
        password: &str,
        email: &str,
        device_name: &str,
    ) -> Result<(String, RemoteUser), SyncError> {
        let v: serde_json::Value = self.call(
            "POST",
            "/api/v1/device/signup",
            Some(serde_json::json!({ "handle": handle, "password": password, "email": email, "device_name": device_name })),
        )?;
        token_and_user(v)
    }

    /// Add Friend search: `{"users": [{"handle", "display_name"}]}`.
    pub fn search_users(&self, q: &str) -> Result<serde_json::Value, SyncError> {
        let path = format!("/api/v1/users/search?q={}", url_encode(q));
        self.call("GET", &path, None)
    }

    pub fn me(&self) -> Result<RemoteUser, SyncError> {
        let v: serde_json::Value = self.call("GET", "/api/v1/me", None)?;
        serde_json::from_value(v["user"].clone()).map_err(|e| SyncError::Decode(e.to_string()))
    }

    /// The raw `GET /api/v1/me` answer (`user` includes `email_verified`).
    pub fn me_raw(&self) -> Result<serde_json::Value, SyncError> {
        self.call("GET", "/api/v1/me", None)
    }

    /// Change the password (needs the current one). The server signs out every other
    /// session of the account; this PC's token stays valid.
    pub fn change_password(
        &self,
        current: &str,
        new: &str,
    ) -> Result<serde_json::Value, SyncError> {
        self.call(
            "POST",
            "/api/v1/me/password",
            Some(serde_json::json!({ "current_password": current, "new_password": new })),
        )
    }

    /// Change the email (needs the current password). Stored unverified for now.
    pub fn change_email(&self, current: &str, email: &str) -> Result<serde_json::Value, SyncError> {
        self.call(
            "POST",
            "/api/v1/me/email",
            Some(serde_json::json!({ "current_password": current, "new_email": email })),
        )
    }

    pub fn upload(&self, batch: &UploadBatch) -> Result<UploadResult, SyncError> {
        let body = serde_json::to_value(batch).map_err(|e| SyncError::Decode(e.to_string()))?;
        self.call("POST", "/api/v1/sessions/upload", Some(body))
    }

    /// Server-side per-game totals by trust tier (`GET /api/v1/stats`).
    pub fn stats(&self) -> Result<serde_json::Value, SyncError> {
        self.call("GET", "/api/v1/stats", None)
    }

    pub fn friends(&self) -> Result<serde_json::Value, SyncError> {
        self.call("GET", "/api/v1/friends", None)
    }

    /// Returns `"pending"` or `"accepted"`.
    pub fn friend_request(&self, handle: &str) -> Result<String, SyncError> {
        let v: serde_json::Value = self.call(
            "POST",
            "/api/v1/friends/request",
            Some(serde_json::json!({ "handle": handle })),
        )?;
        Ok(v["status"].as_str().unwrap_or("pending").to_string())
    }

    pub fn friend_respond(&self, handle: &str, accept: bool) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> = self.call(
            "POST",
            "/api/v1/friends/respond",
            Some(serde_json::json!({ "handle": handle, "accept": accept })),
        )?;
        Ok(())
    }

    pub fn friend_remove(&self, handle: &str) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> =
            self.call("DELETE", &format!("/api/v1/friends/{}", enc(handle)), None)?;
        Ok(())
    }

    pub fn block(&self, handle: &str) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> = self.call(
            "POST",
            "/api/v1/blocks",
            Some(serde_json::json!({ "handle": handle })),
        )?;
        Ok(())
    }

    pub fn unblock(&self, handle: &str) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> =
            self.call("DELETE", &format!("/api/v1/blocks/{}", enc(handle)), None)?;
        Ok(())
    }

    /// Public icon sources for games that aren't installed here
    /// (`GET /api/v1/games/icons`): `{id: {steam_appid, icon_url|null}}`.
    pub fn game_icons(&self, ids: &[String]) -> Result<serde_json::Value, SyncError> {
        let list: Vec<String> = ids.iter().take(50).map(|i| enc(i)).collect();
        self.call(
            "GET",
            &format!("/api/v1/games/icons?ids={}", list.join(",")),
            None,
        )
    }

    /// Download an icon the server hosts itself (`/icons/xfire/...`, max 256 KB).
    pub fn fetch_server_icon(&self, path: &str) -> Result<Vec<u8>, SyncError> {
        if !path.starts_with("/icons/")
            || path.contains("..")
            || path.contains("//")
            || path.contains('?')
        {
            return Err(SyncError::Decode("icon path not allowed".into()));
        }
        let resp = self
            .agent
            .get(&format!("{}{}", self.base, path))
            .call()
            .map_err(|e| SyncError::Network(e.to_string()))?;
        let mut buf = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(256 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| SyncError::Network(e.to_string()))?;
        Ok(buf)
    }

    /// Open the live presence socket.
    pub fn presence(&self) -> Result<presence::PresenceSocket, SyncError> {
        let token = self.token.as_deref().ok_or(SyncError::Unauthorized)?;
        presence::PresenceSocket::connect(&self.base, token)
    }

    /// Revoke this device's token on the server.
    pub fn logout(&self) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> =
            self.call("POST", "/api/v1/auth/logout", Some(serde_json::json!({})))?;
        Ok(())
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Validate a user-entered server URL (http/https only, no path tricks).
fn token_and_user(v: serde_json::Value) -> Result<(String, RemoteUser), SyncError> {
    let token = v["token"]
        .as_str()
        .ok_or_else(|| SyncError::Decode("missing token".into()))?
        .to_string();
    let user =
        serde_json::from_value(v["user"].clone()).map_err(|e| SyncError::Decode(e.to_string()))?;
    Ok((token, user))
}

fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn normalize_server_url(input: &str) -> Option<String> {
    let s = input.trim().trim_end_matches('/');
    let rest = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))?;
    if rest.is_empty()
        || rest.contains(char::is_whitespace)
        || rest.contains('?')
        || rest.contains('#')
        || rest.contains('@')
    {
        return None;
    }
    Some(s.to_string())
}

/// The only host the tray downloads game icons from (Steam's public CDN).
pub const ICON_HOST: &str =
    "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/";

/// Download a Steam community icon (max 256 KB). Other URLs are refused.
pub fn fetch_icon(url: &str) -> Result<Vec<u8>, SyncError> {
    if !url.starts_with(ICON_HOST) || url.contains("..") {
        return Err(SyncError::Decode("icon URL not allowed".into()));
    }
    let resp = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .get(url)
        .call()
        .map_err(|e| SyncError::Network(e.to_string()))?;
    let mut buf = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .take(256 * 1024)
        .read_to_end(&mut buf)
        .map_err(|e| SyncError::Network(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_downloads_only_from_steam_cdn() {
        for u in [
            "http://evil.example/a.jpg",
            "https://cdn.cloudflare.steamstatic.com/steam/apps/570/header.jpg",
            "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/../../x",
        ] {
            assert!(matches!(fetch_icon(u), Err(SyncError::Decode(_))), "{u}");
        }
    }

    #[test]
    fn server_url_normalization() {
        assert_eq!(
            normalize_server_url(" http://localhost:8787/ ").as_deref(),
            Some("http://localhost:8787")
        );
        assert_eq!(
            normalize_server_url("https://fraglink.example").as_deref(),
            Some("https://fraglink.example")
        );
        assert!(normalize_server_url("ftp://x").is_none());
        assert!(normalize_server_url("http://").is_none());
        assert!(normalize_server_url("http://user@evil").is_none());
        assert!(normalize_server_url("localhost").is_none());
    }

    #[test]
    fn unreachable_is_transient() {
        let c = Client::new("http://127.0.0.1:9", None);
        let e = c.health().unwrap_err();
        assert!(e.is_transient(), "{e:?}");
    }

    #[test]
    fn error_classes() {
        assert!(SyncError::Server {
            status: 503,
            code: String::new(),
            message: String::new()
        }
        .is_transient());
        assert!(SyncError::Server {
            status: 429,
            code: String::new(),
            message: String::new()
        }
        .is_transient());
        assert!(!SyncError::Server {
            status: 400,
            code: String::new(),
            message: String::new()
        }
        .is_transient());
        assert!(!SyncError::Unauthorized.is_transient());
    }
}
