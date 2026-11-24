//! Game art (banner pictures) for the website, cached and served by this server.
//!
//! `GET /api/v1/game-art/<percent-encoded game id>.jpg` -> 460x215 JPEG.
//!
//! Sources, first hit wins:
//! 1. an admin override (Games admin: upload or URL), kept until removed,
//! 2. Steam's store header for the game's Steam app id (our `games` table, the
//!    detectable list's Steam SKUs, or `steam:<appid>` ids). Newer Steam apps have no
//!    `<appid>/header.jpg`, so the `header_image` URL the game page warmer stored for
//!    that app is tried next (Steam image hosts only, see [`stored_header_ok`]),
//! 3. the game's cover image (or splash) from its entry in the Discord application
//!    directory (covers Battle.net, Star Citizen, Riot, Epic-only games...),
//!
//! otherwise 404 and the site draws a gradient with the game's icon.
//! Fetched art is cached in `<data dir>/art/g/` for 30 days; misses are remembered a day.

use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::Duration;

pub const W: u32 = 460;
pub const H: u32 = 215;
const MAX_FETCH: usize = 6 * 1024 * 1024;

fn key(id: &str) -> String {
    Sha256::digest(id.as_bytes())[..12]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn dir(st: &AppState) -> PathBuf {
    st.cfg.data_dir.join("art").join("g")
}
pub fn override_path(st: &AppState, id: &str) -> PathBuf {
    st.cfg
        .data_dir
        .join("art")
        .join("override")
        .join(format!("{}.jpg", key(id)))
}

fn fresh(p: &std::path::Path, secs: u64) -> bool {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|e| e < Duration::from_secs(secs))
}

/// Art response. `no-cache` + ETag: browsers keep the picture but re-check it on every
/// view (a cheap 304 when unchanged), so art an admin saves shows up straight away.
fn send(b: Vec<u8>, req: &HeaderMap) -> Response {
    let h = Sha256::digest(&b);
    let etag = format!(
        "\"{}\"",
        h[..10]
            .iter()
            .map(|x| format!("{x:02x}"))
            .collect::<String>()
    );
    let cc = (header::CACHE_CONTROL, "public, no-cache".to_string());
    let matches = req
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag));
    if matches {
        return (StatusCode::NOT_MODIFIED, [cc, (header::ETAG, etag)]).into_response();
    }
    (
        [
            (header::CONTENT_TYPE, "image/jpeg".to_string()),
            cc,
            (header::ETAG, etag),
        ],
        Body::from(b),
    )
        .into_response()
}

/// Decode any common image, centre-crop to 460:215 and encode JPEG.
pub fn normalise(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w < 64 || h < 32 {
        return None;
    }
    let target = W as f64 / H as f64;
    let (cw, ch) = if (w as f64 / h as f64) > target {
        (((h as f64) * target) as u32, h)
    } else {
        (w, ((w as f64) / target) as u32)
    };
    let (x, y) = ((w - cw) / 2, (h - ch) / 2);
    let out = img
        .crop_imm(x, y, cw, ch)
        .resize_exact(W, H, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let mut buf = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 85)
        .encode_image(&out)
        .ok()?;
    Some(buf.into_inner())
}

pub async fn fetch(url: &str) -> Option<Vec<u8>> {
    let r = reqwest::Client::new()
        .get(url)
        .header(
            "User-Agent",
            "FraglinkServer/1.0 (+https://playfraglink.com)",
        )
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .ok()?;
    if !r.status().is_success() {
        return None;
    }
    let b = r.bytes().await.ok()?;
    (b.len() <= MAX_FETCH).then(|| b.to_vec())
}

/// Hosts Steam serves store art from.
const STEAM_IMAGE_HOSTS: [&str; 7] = [
    "shared.akamai.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "cdn.fastly.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "steamcdn-a.akamaihd.net",
];

/// A stored header URL we're willing to fetch for `appid`: https on a Steam image host
/// (or the configured art CDN's own origin, for mirrors and tests), no credentials or
/// odd ports, and a `.jpg` under that same app's folder.
pub fn stored_header_ok(url: &str, appid: u32, art_cdn: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else {
        return false;
    };
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    let host = u.host_str().unwrap_or("");
    let steam = u.scheme() == "https" && u.port().is_none() && STEAM_IMAGE_HOSTS.contains(&host);
    let mirror = url::Url::parse(art_cdn).ok().is_some_and(|c| {
        c.scheme() == u.scheme()
            && c.host_str() == u.host_str()
            && c.port_or_known_default() == u.port_or_known_default()
    });
    if !(steam || mirror) {
        return false;
    }
    let p = u.path();
    let legacy = format!("/steam/apps/{appid}/");
    let assets = format!("/store_item_assets/steam/apps/{appid}/");
    (p.starts_with(&legacy) || p.starts_with(&assets)) && p.ends_with(".jpg") && !p.contains("..")
}

/// The store header URL the game page warmer saved for this Steam app, if it passes
/// [`stored_header_ok`]. Read-only; no Steam call.
pub async fn stored_header_url(st: &AppState, appid: u32) -> Option<String> {
    let url: Option<String> = sqlx::query_scalar(
        "SELECT details_json->>'header_image' FROM game_feed_cache
         WHERE steam_appid = $1 AND details_ok IS TRUE
           AND details_json->>'header_image' IS NOT NULL
         ORDER BY details_fetched_at DESC NULLS LAST
         LIMIT 1",
    )
    .bind(appid as i32)
    .fetch_optional(&st.db)
    .await
    .ok()
    .flatten();
    url.filter(|u| stored_header_ok(u, appid, &st.cfg.steam_art_cdn))
}

/// Cover/splash image URL for a Discord application id (public, no auth).
pub async fn discord_cover(app_id: &str) -> Option<String> {
    let base = std::env::var("FRAGLINK_DISCORD_API")
        .unwrap_or_else(|_| "https://discord.com/api/v9".into());
    let cdn = std::env::var("FRAGLINK_DISCORD_CDN")
        .unwrap_or_else(|_| "https://cdn.discordapp.com".into());
    if !app_id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let b = fetch(&format!("{base}/applications/{app_id}/rpc")).await?;
    let v: serde_json::Value = serde_json::from_slice(&b).ok()?;
    let h = v["cover_image"].as_str().or(v["splash"].as_str())?;
    if !h.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(format!("{cdn}/app-icons/{app_id}/{h}.png?size=1024"))
}

/// Which source would be used for `id` (for the Games admin). Network-free.
pub async fn source_of(st: &AppState, id: &str) -> &'static str {
    let pref = pref(st, id).await;
    if use_override(pref.as_deref()) && override_path(st, id).exists() {
        return "override";
    }
    let s = crate::icons::only(crate::icons::sources(st, id).await, pref.as_deref());
    if s.steam_appid.is_some() {
        "steam"
    } else if s.app_icon.is_some() {
        "discord"
    } else {
        "none"
    }
}

async fn pref(st: &AppState, id: &str) -> Option<String> {
    crate::game_overrides::media(st, id).await.1
}
fn use_override(pref: Option<&str>) -> bool {
    matches!(pref, None | Some("custom"))
}

/// Forget cached art (after a source change).
pub fn purge(st: &AppState, id: &str) {
    let (d, k) = (dir(st), key(id));
    let _ = std::fs::remove_file(d.join(format!("{k}.jpg")));
    let _ = std::fs::remove_file(d.join(format!("{k}.miss")));
}

/// A usable banner is already on disk (admin override or a still-fresh cache hit).
pub fn has_banner(st: &AppState, id: &str) -> bool {
    override_path(st, id).is_file() || fresh(&dir(st).join(format!("{}.jpg", key(id))), 30 * 86400)
}

/// A previous look recorded no art. [`retry`] clears this so the same sources as
/// `/api/v1/game-art` can be tried again.
pub fn has_miss(st: &AppState, id: &str) -> bool {
    dir(st).join(format!("{}.miss", key(id))).is_file()
}

pub fn clear_miss(st: &AppState, id: &str) {
    let _ = std::fs::remove_file(dir(st).join(format!("{}.miss", key(id))));
}

/// Status on the Admin missing-artwork list. Games with a banner are omitted.
pub fn list_status(has_banner: bool, has_miss: bool, has_source: bool) -> Option<&'static str> {
    if has_banner {
        return None;
    }
    Some(if has_miss {
        "unavailable"
    } else if has_source {
        "not_cached"
    } else {
        "missing"
    })
}

/// Drop a `.miss` marker and fetch with the same sources as the public game-art
/// proxy (Steam header, stored `header_image`, then Discord cover).
pub async fn retry(st: &AppState, id: &str) -> bool {
    clear_miss(st, id);
    let d = dir(st);
    let k = key(id);
    let p = d.join(format!("{k}.jpg"));
    let _ = std::fs::create_dir_all(&d);
    match resolve(st, id).await {
        Some(b) => {
            let _ = std::fs::write(&p, &b);
            clear_miss(st, id);
            true
        }
        None => {
            if p.is_file() {
                clear_miss(st, id);
                return true;
            }
            let _ = std::fs::write(d.join(format!("{k}.miss")), b"");
            false
        }
    }
}

async fn resolve(st: &AppState, id: &str) -> Option<Vec<u8>> {
    let s = crate::icons::only(
        crate::icons::sources(st, id).await,
        pref(st, id).await.as_deref(),
    );
    if let Some(appid) = s.steam_appid {
        let url = format!("{}/{appid}/header.jpg", st.cfg.steam_art_cdn);
        if let Some(b) = fetch(&url).await {
            if let Some(j) = normalise(&b) {
                return Some(j);
            }
        }
        if let Some(url) = stored_header_url(st, appid).await {
            if let Some(b) = fetch(&url).await {
                if let Some(j) = normalise(&b) {
                    return Some(j);
                }
            }
        }
    }
    if let Some((app_id, _)) = s.app_icon.as_ref() {
        if let Some(url) = discord_cover(app_id).await {
            if let Some(b) = fetch(&url).await {
                return normalise(&b);
            }
        }
    }
    None
}

/// Art bytes for a game (override, cache or fresh fetch), or None.
pub async fn bytes(st: &AppState, id: &str) -> Option<Vec<u8>> {
    if use_override(pref(st, id).await.as_deref()) {
        if let Ok(b) = tokio::fs::read(override_path(st, id)).await {
            return Some(b);
        }
    }
    let d = dir(st);
    let k = key(id);
    let p = d.join(format!("{k}.jpg"));
    if fresh(&p, 30 * 86400) {
        if let Ok(b) = tokio::fs::read(&p).await {
            return Some(b);
        }
    }
    if fresh(&d.join(format!("{k}.miss")), 86400) {
        return None;
    }
    let _ = std::fs::create_dir_all(&d);
    match resolve(st, id).await {
        Some(b) => {
            let _ = std::fs::write(&p, &b);
            Some(b)
        }
        None => {
            let _ = std::fs::write(d.join(format!("{k}.miss")), b"");
            tokio::fs::read(&p).await.ok()
        }
    }
}

pub async fn serve(
    State(st): State<AppState>,
    Path(file): Path<String>,
    req: HeaderMap,
) -> Response {
    let Some(id) = file
        .strip_suffix(".jpg")
        .filter(|i| !i.is_empty() && i.len() <= 120)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ov = override_path(&st, id);
    if use_override(pref(&st, id).await.as_deref()) {
        if let Ok(b) = tokio::fs::read(&ov).await {
            return send(b, &req);
        }
    }
    let d = dir(&st);
    let k = key(id);
    let p = d.join(format!("{k}.jpg"));
    let miss = d.join(format!("{k}.miss"));
    if fresh(&p, 30 * 86400) {
        if let Ok(b) = tokio::fs::read(&p).await {
            return send(b, &req);
        }
    }
    if fresh(&miss, 86400) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let _ = std::fs::create_dir_all(&d);
    match resolve(&st, id).await {
        Some(b) => {
            let _ = std::fs::write(&p, &b);
            send(b, &req)
        }
        None => {
            if let Ok(b) = tokio::fs::read(&p).await {
                return send(b, &req);
            }
            let _ = std::fs::write(&miss, b"");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// Admin override from uploaded bytes.
pub fn set_override(st: &AppState, id: &str, bytes: &[u8]) -> Result<(), &'static str> {
    if crate::avatars::sniff(bytes).is_none() {
        return Err("Use a PNG, JPEG, WebP or GIF picture.");
    }
    let j = normalise(bytes).ok_or("That picture couldn't be read (or is too small).")?;
    let p = override_path(st, id);
    std::fs::create_dir_all(p.parent().unwrap()).map_err(|_| "Couldn't save the picture.")?;
    std::fs::write(&p, j).map_err(|_| "Couldn't save the picture.")?;
    Ok(())
}

pub fn clear_override(st: &AppState, id: &str) -> bool {
    let _ = std::fs::remove_file(dir(st).join(format!("{}.miss", key(id))));
    std::fs::remove_file(override_path(st, id)).is_ok()
}

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new().route("/api/v1/game-art/{file}", axum::routing::get(serve))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_header_only_on_steam_hosts_for_the_same_app() {
        let cdn = "https://cdn.cloudflare.steamstatic.com/steam/apps";
        for ok in [
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/4705510/2a6f/header.jpg?t=1763",
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/4705510/header.jpg",
            "https://cdn.akamai.steamstatic.com/steam/apps/4705510/header.jpg",
            "https://steamcdn-a.akamaihd.net/steam/apps/4705510/header.jpg",
        ] {
            assert!(stored_header_ok(ok, 4705510, cdn), "{ok}");
        }
        for bad in [
            // another app, another host, plain http, odd port, credentials, not a jpg
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/570/header.jpg",
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/47055100/header.jpg",
            "https://evil.example/store_item_assets/steam/apps/4705510/header.jpg",
            "https://shared.akamai.steamstatic.com.evil.example/steam/apps/4705510/header.jpg",
            "http://shared.akamai.steamstatic.com/steam/apps/4705510/header.jpg",
            "https://shared.akamai.steamstatic.com:8443/steam/apps/4705510/header.jpg",
            "https://u:p@shared.akamai.steamstatic.com/steam/apps/4705510/header.jpg",
            "https://shared.akamai.steamstatic.com/steam/apps/4705510/header.png",
            "https://shared.akamai.steamstatic.com/steam/apps/4705510/../570/header.jpg",
            "https://127.0.0.1/steam/apps/4705510/header.jpg",
            "not a url",
            "",
        ] {
            assert!(!stored_header_ok(bad, 4705510, cdn), "{bad}");
        }
        // the configured art CDN's own origin (mirrors, tests) is fine, other ports aren't
        let mirror = "http://127.0.0.1:4000/apps";
        assert!(stored_header_ok(
            "http://127.0.0.1:4000/store_item_assets/steam/apps/7/h/header.jpg",
            7,
            mirror
        ));
        assert!(!stored_header_ok(
            "http://127.0.0.1:4001/store_item_assets/steam/apps/7/h/header.jpg",
            7,
            mirror
        ));
    }
    #[test]
    fn crops_to_banner() {
        let img = image::RgbImage::from_pixel(1000, 1000, image::Rgb([10, 20, 30]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let j = normalise(&buf.into_inner()).unwrap();
        let back = image::load_from_memory(&j).unwrap();
        assert_eq!((back.width(), back.height()), (W, H));
    }

    #[test]
    fn list_status_drops_games_once_art_is_on_disk() {
        assert_eq!(list_status(true, true, true), None);
        assert_eq!(list_status(false, true, true), Some("unavailable"));
        assert_eq!(list_status(false, false, true), Some("not_cached"));
        assert_eq!(list_status(false, false, false), Some("missing"));
    }
}
