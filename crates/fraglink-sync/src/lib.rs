//! Blocking client for the Fraglink server API (`apps/server`).
//!
//! Nothing here runs unless the user explicitly signs in to sync: the desktop
//! app only constructs a [`Client`] after the user presses "Sign in to sync".

pub mod presence;
pub mod secret;

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set once a request to [`DEFAULT_SERVER`] failed at the network level (DNS, TLS, connect)
/// and the same request worked on [`FALLBACK_SERVER`]. Lasts until the app exits.
static ON_FALLBACK: AtomicBool = AtomicBool::new(false);

/// True when this session talks to the server by IP because HTTPS/DNS failed.
pub fn on_fallback() -> bool {
    ON_FALLBACK.load(Ordering::Relaxed)
}

/// The production server.
pub const DEFAULT_SERVER: &str = "https://playfraglink.com";
/// The same server by IP over plain HTTP: the default up to dev.10, and the fallback for the
/// rest of the session when HTTPS or DNS for [`DEFAULT_SERVER`] fails.
pub const FALLBACK_SERVER: &str = "http://119.42.54.86";
/// The default before 0.2.0-dev.6. A saved copy of it only counts when signed in (a local dev server).
pub const LEGACY_DEFAULT_SERVER: &str = "http://localhost:8787";

/// `Retry-After` in seconds (the HTTP-date form is ignored), capped at 10 minutes.
pub fn parse_retry_after(v: Option<&str>) -> Option<u64> {
    v.and_then(|s| s.trim().parse::<u64>().ok())
        .map(|n| n.min(600))
}

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
        /// Seconds from a `Retry-After` header (503/429), if the server sent one.
        retry_after: Option<u64>,
    },
    #[error("can't reach the server: {0}")]
    Network(String),
    #[error("unexpected response: {0}")]
    Decode(String),
    /// Password was right but the account has two-step sign-in: finish with
    /// [`Client::mfa_login`] using this ticket and a code (valid 5 minutes).
    #[error("two-step sign-in code needed")]
    MfaRequired(String),
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

    /// The server's `Retry-After`, if it asked us to wait.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            SyncError::Server { retry_after, .. } => *retry_after,
            _ => None,
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

/// A downloaded list: body and its ETag (if the server sent one).
pub type ListBody = (Vec<u8>, Option<String>);

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

    /// Whether this client may fall back to [`FALLBACK_SERVER`] (only for the default server).
    fn primary(&self) -> bool {
        self.base == DEFAULT_SERVER
    }

    /// Where requests go right now: the configured server, or the IP fallback.
    pub fn effective_base(&self) -> &str {
        if self.primary() && on_fallback() {
            FALLBACK_SERVER
        } else {
            &self.base
        }
    }

    fn req_at(&self, base: &str, method: &str, path: &str) -> ureq::Request {
        let r = self.agent.request(method, &format!("{base}{path}"));
        match &self.token {
            Some(t) => r.set("Authorization", &format!("Bearer {t}")),
            None => r,
        }
    }

    #[allow(clippy::result_large_err)] // ureq's own error type, handled right away
    fn send(
        &self,
        base: &str,
        method: &str,
        path: &str,
        body: &Option<serde_json::Value>,
    ) -> Result<ureq::Response, ureq::Error> {
        let r = self.req_at(base, method, path);
        match body {
            Some(b) => r.send_json(b.clone()),
            None => r.call(),
        }
    }

    /// Send to the effective server; when the default server fails at the network level
    /// (DNS, TLS, connect), try the IP once and stay on it for the session if that works.
    #[allow(clippy::result_large_err)] // ureq's own error type, handled right away
    fn send_with_fallback(
        &self,
        method: &str,
        path: &str,
        body: &Option<serde_json::Value>,
    ) -> Result<ureq::Response, ureq::Error> {
        let base = self.effective_base().to_string();
        let res = self.send(&base, method, path, body);
        if let Err(ureq::Error::Transport(_)) = &res {
            if self.primary() && !on_fallback() {
                let again = self.send(FALLBACK_SERVER, method, path, body);
                if !matches!(again, Err(ureq::Error::Transport(_))) {
                    ON_FALLBACK.store(true, Ordering::Relaxed);
                    return again;
                }
            }
        }
        res
    }

    fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, SyncError> {
        let res = self.send_with_fallback(method, path, &body);
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
                let retry_after = parse_retry_after(resp.header("Retry-After"));
                let b: ErrBody = resp.into_json().unwrap_or(ErrBody {
                    error: String::new(),
                    message: String::new(),
                });
                Err(SyncError::Server {
                    status,
                    code: b.error,
                    message: b.message,
                    retry_after,
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

    /// Second step of a two-step sign-in: the ticket from [`SyncError::MfaRequired`] plus
    /// a 6-digit authenticator code or a backup code. Returns a device token.
    pub fn mfa_login(
        &self,
        mfa_token: &str,
        code: &str,
    ) -> Result<(String, RemoteUser), SyncError> {
        let v: serde_json::Value = self.call(
            "POST",
            "/api/v1/auth/mfa",
            Some(serde_json::json!({ "mfa_token": mfa_token, "code": code })),
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

    /// Friends' recent activity for the side panel (`GET /api/v1/friends/activity`): one
    /// line per friend per game per day. `tz` (IANA name, e.g. "Australia/Sydney") splits days.
    pub fn friends_activity(&self, tz: Option<&str>) -> Result<serde_json::Value, SyncError> {
        let tz: String = tz
            .unwrap_or("")
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "/_-+".contains(*c))
            .take(64)
            .collect::<String>()
            .replace('+', "%2B");
        if tz.is_empty() {
            self.call("GET", "/api/v1/friends/activity", None)
        } else {
            self.call("GET", &format!("/api/v1/friends/activity?tz={tz}"), None)
        }
    }

    /// A friend's current game and hours this week (`GET /api/v1/friends/{handle}/summary`).
    /// `handle` must already be validated (letters, digits, `_`, `-`).
    pub fn friend_summary(&self, handle: &str) -> Result<serde_json::Value, SyncError> {
        self.call("GET", &format!("/api/v1/friends/{handle}/summary"), None)
    }

    /// A user's profile (`GET /api/v1/users/{handle}/profile`): total hours, games with
    /// their last-played dates and recent sessions (history only for friends who share it).
    /// The friend side panel uses it for Stats and Recent. `handle` must already be validated.
    pub fn user_profile(&self, handle: &str) -> Result<serde_json::Value, SyncError> {
        self.call("GET", &format!("/api/v1/users/{handle}/profile"), None)
    }

    /// Whether friends may see your recent play history and weekly hours.
    pub fn set_share_activity(&self, on: bool) -> Result<serde_json::Value, SyncError> {
        self.call(
            "POST",
            "/api/v1/me/privacy",
            Some(serde_json::json!({ "share_activity": on })),
        )
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

    /// The current staff announcement (`GET /api/v1/announcement`):
    /// `{announcement: {id, message, style, link, ends_at} | null}`.
    pub fn announcement(&self) -> Result<serde_json::Value, SyncError> {
        self.call("GET", "/api/v1/announcement", None)
    }

    /// Which games the server has an official icon for (`GET /api/v1/games/icons`):
    /// `{id: {icon_path: "/icons/games/32/<id>.png", ...}}`.
    pub fn game_icons(&self, ids: &[String]) -> Result<serde_json::Value, SyncError> {
        let list: Vec<String> = ids.iter().take(50).map(|i| enc(i)).collect();
        self.call(
            "GET",
            &format!("/api/v1/games/icons?ids={}", list.join(",")),
            None,
        )
    }

    /// Download an icon the server hosts itself (`/icons/games/...`, max 256 KB).
    /// Icons only ever come from the Fraglink server.
    pub fn fetch_server_icon(&self, path: &str) -> Result<Vec<u8>, SyncError> {
        if !path.starts_with("/icons/")
            || path.contains("..")
            || path.contains("//")
            || path.contains('?')
        {
            return Err(SyncError::Decode("icon path not allowed".into()));
        }
        let resp = self
            .send_with_fallback("GET", path, &None)
            .map_err(|e| SyncError::Network(e.to_string()))?;
        let mut buf = Vec::new();
        use std::io::Read;
        resp.into_reader()
            .take(256 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| SyncError::Network(e.to_string()))?;
        Ok(buf)
    }

    /// Conditional GET of a public list (`/api/v1/games/detectable`, `/overrides`):
    /// `Ok(None)` on 304 Not Modified, else the body (gzip or plain JSON, max 16 MB) and its ETag.
    pub fn fetch_list(
        &self,
        path: &str,
        etag: Option<&str>,
    ) -> Result<Option<ListBody>, SyncError> {
        #[allow(clippy::result_large_err)] // ureq's own error type, handled right away
        let go = |base: &str| {
            let mut r = self
                .agent
                .get(&format!("{base}{path}"))
                .set("Accept-Encoding", "gzip");
            if let Some(e) = etag {
                r = r.set("If-None-Match", e);
            }
            r.call()
        };
        let mut res = go(self.effective_base());
        if let Err(ureq::Error::Transport(_)) = &res {
            if self.primary() && !on_fallback() {
                let again = go(FALLBACK_SERVER);
                if !matches!(again, Err(ureq::Error::Transport(_))) {
                    ON_FALLBACK.store(true, Ordering::Relaxed);
                    res = again;
                }
            }
        }
        match res {
            Ok(resp) if resp.status() == 304 => Ok(None),
            Ok(resp) => {
                let tag = resp.header("ETag").map(str::to_string);
                let mut buf = Vec::new();
                use std::io::Read;
                resp.into_reader()
                    .take(16 * 1024 * 1024)
                    .read_to_end(&mut buf)
                    .map_err(|e| SyncError::Network(e.to_string()))?;
                Ok(Some((buf, tag)))
            }
            Err(ureq::Error::Status(status, resp)) => Err(SyncError::Server {
                status,
                code: String::new(),
                message: format!("GET {path} failed"),
                retry_after: parse_retry_after(resp.header("Retry-After")),
            }),
            Err(ureq::Error::Transport(t)) => Err(SyncError::Network(t.to_string())),
        }
    }

    /// "Track it as a game": report the exe and the name the user accepted, for review
    /// (`POST /api/v1/games/suggest`; works signed out too).
    pub fn suggest_game(&self, exe: &str, name: &str) -> Result<(), SyncError> {
        let _: Option<serde_json::Value> = self.call(
            "POST",
            "/api/v1/games/suggest",
            Some(serde_json::json!({ "exe": exe, "name": name })),
        )?;
        Ok(())
    }

    /// Open the live presence socket.
    pub fn presence(&self) -> Result<presence::PresenceSocket, SyncError> {
        let token = self.token.as_deref().ok_or(SyncError::Unauthorized)?;
        let res = presence::PresenceSocket::connect(self.effective_base(), token);
        if let Err(SyncError::Network(_)) = &res {
            if self.primary() && !on_fallback() {
                if let Ok(s) = presence::PresenceSocket::connect(FALLBACK_SERVER, token) {
                    ON_FALLBACK.store(true, Ordering::Relaxed);
                    return Ok(s);
                }
            }
        }
        res
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
    if v["mfa_required"] == true {
        let t = v["mfa_token"]
            .as_str()
            .ok_or_else(|| SyncError::Decode("missing mfa_token".into()))?;
        return Err(SyncError::MfaRequired(t.to_string()));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
            message: String::new(),
            retry_after: None
        }
        .is_transient());
        assert!(SyncError::Server {
            status: 429,
            code: String::new(),
            message: String::new(),
            retry_after: None
        }
        .is_transient());
        assert!(!SyncError::Server {
            status: 400,
            code: String::new(),
            message: String::new(),
            retry_after: None
        }
        .is_transient());
        assert!(!SyncError::Unauthorized.is_transient());
    }
}
