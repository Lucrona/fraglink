//! Profile pictures. Default: the Fraglink mark on one of six muted colours picked from
//! the user id (drawn by the website/app, nothing stored). Presets: 12 built-in tiles.
//! Uploads: PNG, JPEG, WebP or GIF (first frame), at most 2 MB and 4096 px; checked by
//! magic bytes, decoded and re-encoded (which drops all metadata), centre-cropped square
//! and stored as WebP at 184, 64 and 32 px under `FRAGLINK_DATA_DIR/avatars/<user>/`.
//! Served from `/api/v1/avatars/<user>/<hash>-<px>.webp` with a one-year immutable cache.
//! Rules: 5 changes a day; accounts younger than 24 h get presets/default only; once email
//! is live, uploads need a verified email. Reports go to the admin queue; removing an
//! avatar resets it and blocks that exact file from being uploaded again.

use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::{api, friends, AppState};
use axum::body::{Body, Bytes};
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::imageops::FilterType;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

pub const SIZES: [u32; 3] = [184, 64, 32];
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DIM: u32 = 4096;
pub const PRESETS: i16 = 12;
pub const CHANGES_PER_DAY: i64 = 5;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/me/avatar", post(upload))
        .route("/api/v1/me/avatar/preset", post(set_preset))
        .route("/api/v1/me/avatar/reset", post(reset))
        .route("/api/v1/avatars/{user}/{file}", get(serve))
        .route("/api/v1/users/{handle}/avatar/report", post(report))
}

/// Default colour (0..5) for a user.
pub fn color_of(uid: Uuid) -> u8 {
    (uid.as_u128() % 6) as u8
}

pub fn url(uid: Uuid, hash: &str, px: u32) -> String {
    format!("/api/v1/avatars/{uid}/{hash}-{px}.webp")
}

/// What the website and desktop app need to draw someone's avatar.
pub fn avatar_json(
    uid: Uuid,
    kind: Option<&str>,
    hash: Option<&str>,
    preset: Option<i16>,
) -> Value {
    match (kind, hash, preset) {
        (Some("upload"), Some(h), _) => json!({
            "kind": "upload", "color": color_of(uid),
            "url_184": url(uid, h, 184), "url_64": url(uid, h, 64), "url_32": url(uid, h, 32),
        }),
        (Some("preset"), _, Some(p)) => {
            json!({ "kind": "preset", "preset": p, "color": color_of(uid) })
        }
        _ => json!({ "kind": "default", "color": color_of(uid) }),
    }
}

/// Avatars for many users at once (friends lists, home page).
pub async fn many(
    st: &AppState,
    ids: &[Uuid],
) -> ApiResult<std::collections::HashMap<Uuid, Value>> {
    let rows: Vec<(Uuid, Option<String>, Option<String>, Option<i16>)> = sqlx::query_as(
        "SELECT id, avatar_kind, avatar_hash, avatar_preset FROM users WHERE id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(&st.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, k, h, p)| (id, avatar_json(id, k.as_deref(), h.as_deref(), p)))
        .collect())
}

fn dir(st: &AppState, uid: Uuid) -> PathBuf {
    st.cfg.data_dir.join("avatars").join(uid.to_string())
}

pub fn remove_files(st: &AppState, uid: Uuid) {
    let _ = std::fs::remove_dir_all(dir(st, uid));
}

/// Why this user can't upload right now (None = allowed).
pub async fn upload_block_reason(st: &AppState, uid: Uuid) -> ApiResult<Option<String>> {
    let (young, verified, email_empty): (bool, bool, bool) = sqlx::query_as(
        "SELECT created_at > now() - interval '24 hours', email_verified, email = '' FROM users WHERE id = $1",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    if young {
        return Ok(Some(
            "New accounts can pick a preset or the default for their first 24 hours. Uploads open after that.".into(),
        ));
    }
    if crate::mailer::live(st) && (!verified || email_empty) {
        return Ok(Some(
            "Confirm your email address to upload a picture.".into(),
        ));
    }
    Ok(None)
}

async fn count_change(st: &AppState, uid: Uuid) -> ApiResult<()> {
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM avatar_changes WHERE user_id = $1 AND changed_at > now() - interval '24 hours'",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    if n >= CHANGES_PER_DAY {
        return Err(AppError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "avatar_limit",
            "You can change your picture 5 times a day. Try again tomorrow.",
        ));
    }
    sqlx::query("INSERT INTO avatar_changes(user_id) VALUES($1)")
        .bind(uid)
        .execute(&st.db)
        .await?;
    Ok(())
}

pub fn sniff(b: &[u8]) -> Option<image::ImageFormat> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(image::ImageFormat::Png)
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(image::ImageFormat::Jpeg)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(image::ImageFormat::Gif)
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(image::ImageFormat::WebP)
    } else {
        None
    }
}

fn bad_image(msg: &str) -> AppError {
    AppError::bad_request("bad_image", msg)
}

/// Decode, crop square and encode the three WebP sizes. CPU-bound: call from spawn_blocking.
pub fn process(bytes: &[u8]) -> Result<Vec<(u32, Vec<u8>)>, AppError> {
    let fmt = sniff(bytes).ok_or_else(|| bad_image("Use a PNG, JPEG, WebP or GIF image."))?;
    let mut r = image::ImageReader::with_format(std::io::Cursor::new(bytes), fmt);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(128 * 1024 * 1024);
    r.limits(limits);
    let img = r
        .decode()
        .map_err(|_| bad_image("That image couldn't be read, or it's bigger than 4096 x 4096."))?;
    let (w, h) = (img.width(), img.height());
    if w < 16 || h < 16 {
        return Err(bad_image("That image is too small (at least 16 x 16)."));
    }
    let side = w.min(h);
    let sq = img
        .crop_imm((w - side) / 2, (h - side) / 2, side, side)
        .to_rgba8();
    let mut out = Vec::new();
    for px in SIZES {
        let small = image::imageops::resize(&sq, px, px, FilterType::Lanczos3);
        let mut buf = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut buf)
            .encode(small.as_raw(), px, px, image::ExtendedColorType::Rgba8)
            .map_err(|e| AppError::internal(anyhow::anyhow!("webp: {e}")))?;
        out.push((px, buf));
    }
    Ok(out)
}

fn semaphore() -> &'static tokio::sync::Semaphore {
    static S: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    S.get_or_init(|| tokio::sync::Semaphore::new(1)) // one image at a time on a 1-vCPU box
}

async fn upload(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if let Some(why) = upload_block_reason(&st, a.user.id).await? {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "upload_not_allowed",
            &why,
        ));
    }
    if body.len() > MAX_BYTES {
        return Err(bad_image("Images can be up to 2 MB."));
    }
    if sniff(&body).is_none() {
        return Err(bad_image("Use a PNG, JPEG, WebP or GIF image."));
    }
    let src_hash = hex(&Sha256::digest(&body));
    let blocked: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM avatar_blocklist WHERE src_hash = $1)")
            .bind(&src_hash)
            .fetch_one(&st.db)
            .await?;
    if blocked {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "avatar_blocked",
            "That picture was removed by a moderator and can't be used.",
        ));
    }
    count_change(&st, a.user.id).await?;
    let _permit = semaphore().acquire().await.map_err(AppError::internal)?;
    let files = tokio::task::spawn_blocking(move || process(&body))
        .await
        .map_err(AppError::internal)??;
    let hash = hex(&Sha256::digest(&files[0].1))[..16].to_string();
    let d = dir(&st, a.user.id);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).map_err(AppError::internal)?;
    for (px, b) in &files {
        std::fs::write(d.join(format!("{hash}-{px}.webp")), b).map_err(AppError::internal)?;
    }
    sqlx::query(
        "UPDATE users SET avatar_kind = 'upload', avatar_hash = $2, avatar_src_hash = $3, avatar_preset = NULL WHERE id = $1",
    )
    .bind(a.user.id)
    .bind(&hash)
    .bind(&src_hash)
    .execute(&st.db)
    .await?;
    Ok(Json(avatar_json(
        a.user.id,
        Some("upload"),
        Some(&hash),
        None,
    )))
}

#[derive(Deserialize)]
struct PresetReq {
    preset: i16,
}

async fn set_preset(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<PresetReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if !(0..PRESETS).contains(&r.preset) {
        return Err(AppError::bad_request("bad_preset", "Unknown preset."));
    }
    count_change(&st, a.user.id).await?;
    remove_files(&st, a.user.id);
    sqlx::query(
        "UPDATE users SET avatar_kind = 'preset', avatar_preset = $2, avatar_hash = NULL, avatar_src_hash = NULL WHERE id = $1",
    )
    .bind(a.user.id)
    .bind(r.preset)
    .execute(&st.db)
    .await?;
    Ok(Json(avatar_json(
        a.user.id,
        Some("preset"),
        None,
        Some(r.preset),
    )))
}

/// Back to the default mark. Not counted against the daily limit.
async fn reset(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    clear(&st, a.user.id).await?;
    Ok(Json(avatar_json(a.user.id, None, None, None)))
}

pub async fn clear(st: &AppState, uid: Uuid) -> ApiResult<()> {
    remove_files(st, uid);
    sqlx::query(
        "UPDATE users SET avatar_kind = NULL, avatar_hash = NULL, avatar_src_hash = NULL, avatar_preset = NULL WHERE id = $1",
    )
    .bind(uid)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn serve(State(st): State<AppState>, Path((user, file)): Path<(Uuid, String)>) -> Response {
    let ok = file.len() <= 40
        && file.ends_with(".webp")
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
    if !ok {
        return StatusCode::NOT_FOUND.into_response();
    }
    match tokio::fs::read(dir(&st, user).join(&file)).await {
        Ok(b) => (
            [
                (header::CONTENT_TYPE, "image/webp"),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            Body::from(b),
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize, Default)]
struct ReportReq {
    #[serde(default)]
    reason: String,
}

async fn report(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(handle): Path<String>,
    body: Option<Json<ReportReq>>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    st.rate_limit(
        format!("avreport:{}", a.user.id),
        20,
        std::time::Duration::from_secs(3600),
    )?;
    let p = friends::find_by_handle(&st, &handle).await?;
    let hash: Option<String> = sqlx::query_scalar(
        "SELECT avatar_hash FROM users WHERE id = $1 AND avatar_kind = 'upload'",
    )
    .bind(p.id)
    .fetch_one(&st.db)
    .await?;
    let Some(hash) = hash else {
        return Err(AppError::bad_request(
            "no_avatar",
            "This player doesn't have an uploaded picture.",
        ));
    };
    let reason: String = body
        .map(|b| b.0.reason)
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect();
    sqlx::query(
        "INSERT INTO avatar_reports(user_id, reporter_id, avatar_hash, reason) VALUES($1, $2, $3, $4)
         ON CONFLICT DO NOTHING",
    )
    .bind(p.id)
    .bind(a.user.id)
    .bind(&hash)
    .bind(if reason.trim().is_empty() { None } else { Some(reason.trim().to_string()) })
    .execute(&st.db)
    .await?;
    Ok(Json(json!({ "reported": true })))
}

/// Staff: remove someone's uploaded picture, block that file, close its open reports.
pub async fn remove_and_block(st: &AppState, uid: Uuid, staff: Uuid) -> ApiResult<Option<String>> {
    let src: Option<String> = sqlx::query_scalar("SELECT avatar_src_hash FROM users WHERE id = $1")
        .bind(uid)
        .fetch_optional(&st.db)
        .await?
        .flatten();
    if let Some(h) = &src {
        sqlx::query("INSERT INTO avatar_blocklist(src_hash, blocked_by) VALUES($1, $2) ON CONFLICT DO NOTHING")
            .bind(h)
            .bind(staff)
            .execute(&st.db)
            .await?;
    }
    clear(st, uid).await?;
    sqlx::query(
        "UPDATE avatar_reports SET resolved_at = now(), resolved_by = $2, resolution = 'removed'
         WHERE user_id = $1 AND resolved_at IS NULL",
    )
    .bind(uid)
    .bind(staff)
    .execute(&st.db)
    .await?;
    Ok(src)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffing() {
        assert_eq!(
            sniff(b"\x89PNG\r\n\x1a\nxxxx"),
            Some(image::ImageFormat::Png)
        );
        assert_eq!(sniff(b"GIF89a...."), Some(image::ImageFormat::Gif));
        assert_eq!(
            sniff(b"RIFF\0\0\0\0WEBPVP8 "),
            Some(image::ImageFormat::WebP)
        );
        assert_eq!(sniff(b"<svg"), None);
    }

    #[test]
    fn crops_and_resizes() {
        let img = image::RgbaImage::from_pixel(300, 200, image::Rgba([200, 30, 30, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let out = process(&png).unwrap();
        assert_eq!(out.len(), 3);
        for (px, b) in out {
            let d = image::load_from_memory(&b).unwrap();
            assert_eq!((d.width(), d.height()), (px, px));
        }
    }

    #[test]
    fn colours_are_stable() {
        let u = Uuid::from_u128(13);
        assert_eq!(color_of(u), 1);
    }
}
