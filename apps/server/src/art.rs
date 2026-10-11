//! Game art (banner pictures) for the website, cached and served by this server.
//!
//! `GET /api/v1/game-art/<percent-encoded game id>.jpg` -> 460x215 JPEG.
//!
//! Sources, first hit wins:
//! 1. an admin override (Games admin: upload or URL), kept until removed,
//! 2. Steam's store header for the game's Steam app id (our `games` table, the
//!    detectable list's Steam SKUs, or `steam:<appid>` ids),
//! 3. the game's cover image (or splash) from its entry in the Discord application
//!    directory (covers Battle.net, Star Citizen, Riot, Epic-only games...),
//!
//! otherwise 404 and the site draws a gradient with the game's icon.
//! Fetched art is cached in `<data dir>/art/g/` for 30 days; misses are remembered a day.

use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
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

fn send(b: Vec<u8>, max_age: u32) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/jpeg".to_string()),
            (header::CACHE_CONTROL, format!("public, max-age={max_age}")),
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

/// Cover/splash image URL for a Discord application id (public, no auth).
async fn discord_cover(app_id: &str) -> Option<String> {
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
    if override_path(st, id).exists() {
        return "override";
    }
    let s = crate::icons::sources(st, id).await;
    if s.steam_appid.is_some() {
        "steam"
    } else if s.app_icon.is_some() {
        "discord"
    } else {
        "none"
    }
}

async fn resolve(st: &AppState, id: &str) -> Option<Vec<u8>> {
    let s = crate::icons::sources(st, id).await;
    if let Some(appid) = s.steam_appid {
        let url = format!("{}/{appid}/header.jpg", st.cfg.steam_art_cdn);
        if let Some(b) = fetch(&url).await {
            if let Some(j) = normalise(&b) {
                return Some(j);
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
    if let Ok(b) = tokio::fs::read(override_path(st, id)).await {
        return Some(b);
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

pub async fn serve(State(st): State<AppState>, Path(file): Path<String>) -> Response {
    let Some(id) = file
        .strip_suffix(".jpg")
        .filter(|i| !i.is_empty() && i.len() <= 120)
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let ov = override_path(&st, id);
    if let Ok(b) = tokio::fs::read(&ov).await {
        return send(b, 3600);
    }
    let d = dir(&st);
    let k = key(id);
    let p = d.join(format!("{k}.jpg"));
    let miss = d.join(format!("{k}.miss"));
    if fresh(&p, 30 * 86400) {
        if let Ok(b) = tokio::fs::read(&p).await {
            return send(b, 86400);
        }
    }
    if fresh(&miss, 86400) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let _ = std::fs::create_dir_all(&d);
    match resolve(&st, id).await {
        Some(b) => {
            let _ = std::fs::write(&p, &b);
            send(b, 86400)
        }
        None => {
            if let Ok(b) = tokio::fs::read(&p).await {
                return send(b, 3600);
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
    fn crops_to_banner() {
        let img = image::RgbImage::from_pixel(1000, 1000, image::Rgb([10, 20, 30]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let j = normalise(&buf.into_inner()).unwrap();
        let back = image::load_from_memory(&j).unwrap();
        assert_eq!((back.width(), back.height()), (W, H));
    }
}
