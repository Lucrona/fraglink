//! Official game icons, cached and served by this server (0.2.0-dev.15).
//!
//! `GET /icons/games/<16|32|64>/<percent-encoded game id>.png`
//!
//! Sources, the way Discord and Steam show them:
//! 1. the game's icon from the detectable list (`icon_hash`), fetched from the
//!    application icon CDN,
//! 2. Steam's community icon for the game's app id (184 px "full" version first),
//!    with the hash from our `games` table or Steam's store API.
//!
//! A game with neither gets 404 and the app falls back to the exe icon on the
//! user's PC or a generic glyph. Apps only ever download icons from this server.
//!
//! Fetched icons are kept in `<data dir>/icon-cache/` as 16/32/64 px PNGs, served
//! with an ETag and a week of browser caching, and refreshed after 30 days.
//! A failed lookup is remembered for a day.

use crate::games::{norm_name, IconSrc};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

pub const SIZES: [u32; 3] = [16, 32, 64];
const REFRESH_AFTER: Duration = Duration::from_secs(30 * 24 * 3600);
const MISS_TTL: Duration = Duration::from_secs(24 * 3600);
const MAX_BYTES: usize = 1024 * 1024;

/// Where one game's icon can come from.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Sources {
    /// (application id, icon hash) on the application icon CDN
    pub app_icon: Option<(String, String)>,
    pub steam_appid: Option<u32>,
    /// Steam community icon hash, when we already know it
    pub steam_hash: Option<String>,
}

impl Sources {
    pub fn is_empty(&self) -> bool {
        self.app_icon.is_none() && self.steam_appid.is_none()
    }
}

/// Icon sources for a game id: the list entry for its id, its Steam app id
/// (`steam:<appid>`, signature or `games` row), or the list entry with its name.
pub async fn sources(st: &AppState, id: &str) -> Sources {
    let idx = st.games.icon_index();
    let mut s = Sources::default();
    let take = |src: &IconSrc, s: &mut Sources| {
        if s.app_icon.is_none() {
            s.app_icon = src.icon_hash.clone().map(|h| (src.app_id.clone(), h));
        }
        if s.steam_appid.is_none() {
            s.steam_appid = src.steam_appid;
        }
    };
    if let Some(src) = idx.by_slug.get(id) {
        take(src, &mut s);
    }
    let row: Option<(Option<i32>, Option<String>)> =
        sqlx::query_as("SELECT steam_appid, steam_icon_hash FROM games WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await
            .ok()
            .flatten();
    let appid = fraglink_core::icons::appid_from_game_id(id)
        .or_else(|| {
            st.sigs
                .get(id)
                .and_then(|g| g.steam_appids.first().copied())
        })
        .or_else(|| row.as_ref().and_then(|r| r.0).map(|a| a as u32));
    if let Some(a) = appid {
        if let Some(src) = idx.by_steam.get(&a).and_then(|slug| idx.by_slug.get(slug)) {
            take(src, &mut s);
        }
        s.steam_appid.get_or_insert(a);
    }
    if s.app_icon.is_none() {
        if let Ok(Some(name)) = crate::names::public_name(st, id).await {
            if let Some(src) = idx
                .by_name
                .get(&norm_name(&name))
                .and_then(|slug| idx.by_slug.get(slug))
            {
                take(src, &mut s);
            }
        }
    }
    if s.steam_appid.is_some() && s.steam_appid == row.as_ref().and_then(|r| r.0).map(|a| a as u32)
    {
        s.steam_hash = row.and_then(|r| r.1);
    }
    s
}

/// The path apps fetch for a game's icon.
pub fn icon_path(id: &str, size: u32) -> String {
    format!("/icons/games/{size}/{}.png", encode(id))
}

fn encode(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn cache_dir(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("icon-cache")
}

fn key(id: &str) -> String {
    let h = Sha256::digest(id.as_bytes());
    h[..12].iter().map(|x| format!("{x:02x}")).collect()
}

fn age(p: &std::path::Path) -> Option<Duration> {
    let t = std::fs::metadata(p).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(t).unwrap_or_default())
}

async fn download(url: &str) -> Option<Vec<u8>> {
    let r = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("Fraglink/1.0 (+https://playfraglink.com)")
        .build()
        .ok()?
        .get(url)
        .send()
        .await
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    let b = r.bytes().await.ok()?;
    (b.len() <= MAX_BYTES && image_ok(&b)).then(|| b.to_vec())
}

fn image_ok(b: &[u8]) -> bool {
    fraglink_core::icons::normalize_px(b, 16).is_some()
}

/// Steam's community icon hash for an app, from the store API (no key needed).
async fn steam_icon_hash(st: &AppState, appid: u32) -> Option<String> {
    let input = serde_json::json!({
        "ids": [{ "appid": appid }],
        "context": { "language": "english", "country_code": "US" },
        "data_request": { "include_assets": true }
    });
    let url = format!(
        "{}/IStoreBrowseService/GetItems/v1/",
        st.cfg.steam_api_base.trim_end_matches('/')
    );
    let v: serde_json::Value = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?
        .get(url)
        .query(&[("input_json", input.to_string())])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let h = v["response"]["store_items"][0]["assets"]["community_icon"].as_str()?;
    (h.len() == 40 && h.chars().all(|c| c.is_ascii_hexdigit())).then(|| h.to_lowercase())
}

/// Fetch the best official icon (raw image bytes) for these sources.
async fn fetch(st: &AppState, s: &Sources) -> Option<Vec<u8>> {
    if let Some((app, hash)) = &s.app_icon {
        let url = format!(
            "{}/app-icons/{app}/{hash}.png?size=128",
            st.cfg.app_icon_cdn_url.trim_end_matches('/')
        );
        if let Some(b) = download(&url).await {
            return Some(b);
        }
    }
    let appid = s.steam_appid?;
    let hash = match &s.steam_hash {
        Some(h) => Some(h.clone()),
        None => steam_icon_hash(st, appid).await,
    }?;
    for full in [true, false] {
        if let Some(url) =
            fraglink_core::icons::steam_cdn_icon_url(&st.cfg.steam_cdn_url, appid, &hash, full)
        {
            if let Some(b) = download(&url).await {
                return Some(b);
            }
        }
    }
    None
}

/// One fetch per game at a time, and at most a few at once.
fn in_flight() -> &'static Mutex<std::collections::HashSet<String>> {
    static S: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn semaphore() -> &'static tokio::sync::Semaphore {
    static S: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(4))
}

/// Fetch and store all sizes. True when an icon is now cached.
async fn refresh(st: &AppState, id: &str, s: &Sources) -> bool {
    let k = key(id);
    if !in_flight().lock().unwrap().insert(k.clone()) {
        return false;
    }
    let _permit = semaphore().acquire().await;
    let dir = cache_dir(st);
    let ok = match fetch(st, s).await {
        Some(raw) => {
            let _ = std::fs::create_dir_all(&dir);
            let mut ok = true;
            for px in SIZES {
                match fraglink_core::icons::normalize_px(&raw, px) {
                    Some(png) => {
                        let tmp = dir.join(format!("{k}-{px}.tmp"));
                        ok &= std::fs::write(&tmp, &png).is_ok()
                            && std::fs::rename(&tmp, dir.join(format!("{k}-{px}.png"))).is_ok();
                    }
                    None => ok = false,
                }
            }
            let _ = std::fs::remove_file(dir.join(format!("{k}.miss")));
            ok
        }
        None => {
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(format!("{k}.miss")), id.as_bytes());
            false
        }
    };
    in_flight().lock().unwrap().remove(&k);
    if ok {
        tracing::info!("icons: cached {id}");
    } else {
        tracing::info!("icons: no official icon for {id}");
    }
    ok
}

/// The cached PNG for a game at `size`, fetching it first if needed.
pub async fn get(st: &AppState, id: &str, size: u32) -> Option<Vec<u8>> {
    if !SIZES.contains(&size) || id.is_empty() || id.len() > 120 {
        return None;
    }
    let k = key(id);
    let dir = cache_dir(st);
    let file = dir.join(format!("{k}-{size}.png"));
    if let Ok(b) = std::fs::read(&file) {
        if age(&file).is_some_and(|a| a > REFRESH_AFTER) {
            // keep serving the old one; refresh in the background
            let (st2, id2) = (st.clone(), id.to_string());
            tokio::spawn(async move {
                let s = sources(&st2, &id2).await;
                if !s.is_empty() {
                    refresh(&st2, &id2, &s).await;
                }
            });
        }
        return Some(b);
    }
    if age(&dir.join(format!("{k}.miss"))).is_some_and(|a| a < MISS_TTL) {
        return None;
    }
    let s = sources(st, id).await;
    if s.is_empty() || !refresh(st, id, &s).await {
        return None;
    }
    std::fs::read(&file).ok()
}

/// True when this server has (or can look for) an official icon for `id`.
pub async fn available(st: &AppState, id: &str) -> bool {
    let k = key(id);
    let dir = cache_dir(st);
    if dir.join(format!("{k}-32.png")).exists() {
        return true;
    }
    if age(&dir.join(format!("{k}.miss"))).is_some_and(|a| a < MISS_TTL) {
        return false;
    }
    !sources(st, id).await.is_empty()
}

fn etag_of(b: &[u8]) -> String {
    let h = Sha256::digest(b);
    let hex: String = h[..10].iter().map(|x| format!("{x:02x}")).collect();
    format!("\"{hex}\"")
}

/// `GET /icons/games/{size}/{id}.png`
pub async fn serve(
    State(st): State<AppState>,
    Path((size, file)): Path<(u32, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    let id = file.strip_suffix(".png").unwrap_or("");
    let png = match crate::names::canonical_id(uuid::Uuid::nil(), id) {
        Some(id) if !id.starts_with("manual:") => get(&st, &id, size).await,
        _ => None,
    };
    let Some(png) = png else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(Body::empty())
            .unwrap();
    };
    let etag = etag_of(&png);
    let mut r = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
    {
        Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .unwrap()
    } else {
        Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .body(Body::from(png))
            .unwrap()
    };
    let h = r.headers_mut();
    h.insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800"),
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_encode_ids() {
        assert_eq!(icon_path("wow", 32), "/icons/games/32/wow.png");
        assert_eq!(
            icon_path("steam:570", 16),
            "/icons/games/16/steam%3A570.png"
        );
        assert_eq!(
            icon_path("exe:odd game.exe", 64),
            "/icons/games/64/exe%3Aodd%20game.exe.png"
        );
    }
}
