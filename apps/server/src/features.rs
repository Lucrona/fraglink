//! Site settings: feature switches staff flip in Admin > Site settings.
//!
//! Stored in `site_settings` (only changed keys), cached in memory for 10 s and dropped on
//! every change. `GET /api/v1/features` (public, read-only) gives the website and the desktop
//! app the same view; the server enforces every switch itself (hiding UI is only cosmetic).
//!
//! | key               | default | off means                                                   |
//! |-------------------|---------|-------------------------------------------------------------|
//! | `signups`         | on      | new accounts refused (website, app, legacy form)            |
//! | `announcement`    | on      | the staff banner isn't served (site + app)                  |
//! | `connect_steam`   | on      | linking Steam (and hours import) refused                    |
//! | `connect_bnet`    | on      | linking Battle.net refused                                  |
//! | `connect_discord` | on      | linking Discord refused                                     |
//! | `friend_requests` | on      | new friend requests refused (answering old ones still works)|
//! | `public_profiles` | on      | profiles only for yourself/friends; player search off        |
//! | `maintenance`     | off     | everything but staff gets 503 maintenance (see `gate`)      |

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// (key, default, label, help) in display order.
pub const SWITCHES: [(&str, bool, &str, &str); 8] = [
    ("signups", true, "Sign-ups open", "New accounts can be created on the website and in the app."),
    ("announcement", true, "Announcement banner", "Show the staff announcement on the website and in the app."),
    ("connect_steam", true, "Steam connect", "Players can link Steam and import their Steam hours."),
    ("connect_bnet", true, "Battle.net connect", "Players can link their Battle.net account."),
    ("connect_discord", true, "Discord connect", "Players can link their Discord account."),
    ("friend_requests", true, "Friend requests", "Players can send new friend requests. Answering ones already sent still works."),
    ("public_profiles", true, "Public profiles and search", "Profiles can be seen by anyone allowed by the player's privacy settings, and players can be searched. Off: only yourself and friends see a profile, and search is off."),
    ("maintenance", false, "Maintenance mode", "Everyone except staff sees a maintenance page and the app can't sync. Staff can still sign in and use everything."),
];

const TTL: Duration = Duration::from_secs(10);

fn cache() -> &'static Mutex<Option<(Instant, BTreeMap<String, bool>)>> {
    static C: std::sync::OnceLock<Mutex<Option<(Instant, BTreeMap<String, bool>)>>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(None))
}

pub fn forget() {
    *cache().lock().unwrap() = None;
}

pub fn is_key(k: &str) -> bool {
    SWITCHES.iter().any(|s| s.0 == k)
}

/// Every switch with its current value.
pub async fn all(st: &AppState) -> ApiResult<BTreeMap<String, bool>> {
    if let Some((at, m)) = cache().lock().unwrap().as_ref() {
        if at.elapsed() < TTL {
            return Ok(m.clone());
        }
    }
    let rows: Vec<(String, bool)> = sqlx::query_as("SELECT key, enabled FROM site_settings")
        .fetch_all(&st.db)
        .await?;
    let mut m: BTreeMap<String, bool> = SWITCHES.iter().map(|s| (s.0.to_string(), s.1)).collect();
    for (k, v) in rows {
        if m.contains_key(&k) {
            m.insert(k, v);
        }
    }
    *cache().lock().unwrap() = Some((Instant::now(), m.clone()));
    Ok(m)
}

/// One switch (unknown keys count as on). A database error keeps the default.
pub async fn on(st: &AppState, key: &str) -> bool {
    let default = SWITCHES
        .iter()
        .find(|s| s.0 == key)
        .map(|s| s.1)
        .unwrap_or(true);
    match all(st).await {
        Ok(m) => m.get(key).copied().unwrap_or(default),
        Err(_) => default,
    }
}

/// Err(403 feature_off) when `key` is off.
pub async fn require(st: &AppState, key: &str, message: &'static str) -> ApiResult<()> {
    if on(st, key).await {
        Ok(())
    } else {
        Err(AppError::new(StatusCode::FORBIDDEN, "feature_off", message))
    }
}

/// `GET /api/v1/features`: `{ "features": { key: bool, ... } }`. Public, cached 30 s.
pub async fn public(State(st): State<AppState>) -> ApiResult<impl IntoResponse> {
    let m = all(&st).await?;
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json(json!({ "features": m })),
    ))
}

/// Admin list: every switch with label, help, default and who changed it last.
pub async fn admin_list(st: &AppState) -> ApiResult<Value> {
    let m = all(st).await?;
    let rows: Vec<(String, chrono::DateTime<chrono::Utc>, Option<String>)> = sqlx::query_as(
        "SELECT s.key, s.updated_at, u.handle FROM site_settings s LEFT JOIN users u ON u.id = s.updated_by",
    )
    .fetch_all(&st.db)
    .await?;
    let items: Vec<Value> = SWITCHES
        .iter()
        .map(|(k, d, label, help)| {
            let r = rows.iter().find(|r| r.0 == *k);
            json!({ "key": k, "enabled": m.get(*k).copied().unwrap_or(*d), "default": d,
                    "label": label, "help": help,
                    "updated_at": r.map(|r| r.1), "updated_by": r.and_then(|r| r.2.clone()) })
        })
        .collect();
    Ok(json!({ "settings": items }))
}

pub async fn set(st: &AppState, key: &str, enabled: bool, by: uuid::Uuid) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO site_settings(key, enabled, updated_by) VALUES($1, $2, $3)
         ON CONFLICT (key) DO UPDATE SET enabled = $2, updated_by = $3, updated_at = now()",
    )
    .bind(key)
    .bind(enabled)
    .bind(by)
    .execute(&st.db)
    .await?;
    forget();
    Ok(())
}

/// Paths that keep working in maintenance mode for everyone: health and the switches
/// themselves, signing in (so staff can get in), the banner, admin (staff-gated itself),
/// and static game data the app and site draw with.
fn open_in_maintenance(path: &str) -> bool {
    matches!(
        path,
        "/api/v1/health"
            | "/api/v1/features"
            | "/api/v1/announcement"
            | "/api/v1/auth/login"
            | "/api/v1/auth/mfa"
            | "/api/v1/form-fallback"
    ) || path.starts_with("/api/v1/admin/")
        || path.starts_with("/api/v1/games/")
        || path.starts_with("/api/v1/game-art/")
        || path.starts_with("/api/v1/art/")
        || path.starts_with("/icons/")
        || path.starts_with("/u/")
}

/// Middleware: in maintenance mode, non-staff requests get 503 `maintenance`.
pub async fn gate(State(st): State<AppState>, req: Request, next: Next) -> Response {
    if !on(&st, "maintenance").await || open_in_maintenance(req.uri().path()) {
        return next.run(req).await;
    }
    let (mut parts, body) = req.into_parts();
    let staff =
        <crate::admin::Staff as axum::extract::FromRequestParts<AppState>>::from_request_parts(
            &mut parts, &st,
        )
        .await
        .is_ok();
    if staff {
        return next.run(Request::from_parts(parts, body)).await;
    }
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::RETRY_AFTER, "300"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Json(json!({ "error": "maintenance",
                     "message": "Fraglink is down for maintenance. Back soon." })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    #[test]
    fn maintenance_lets_staff_tools_and_sign_in_through() {
        assert!(super::open_in_maintenance("/api/v1/auth/login"));
        assert!(super::open_in_maintenance("/api/v1/admin/settings"));
        assert!(super::open_in_maintenance("/api/v1/features"));
        assert!(!super::open_in_maintenance("/api/v1/me"));
        assert!(!super::open_in_maintenance("/api/v1/ws"));
        assert!(!super::open_in_maintenance("/api/v1/sessions/upload"));
        assert!(super::is_key("maintenance") && !super::is_key("nope"));
    }
}
