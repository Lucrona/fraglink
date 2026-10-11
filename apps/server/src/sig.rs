//! Signature image: a 460 x 100 PNG status card for forums and Discord bios
//! (`/api/v1/sig/<handle>.png`). Only for profiles set to public; shows the avatar,
//! name, what they're playing now (or last played), and hours this week / all time.
//! Rendered on demand and cached for 60 seconds.

use crate::error::ApiResult;
use crate::{avatars, names, presence, AppState};
use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use image::{Rgba, RgbaImage};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const W: u32 = 460;
const H: u32 = 100;
static REG: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");
static BOLD: &[u8] = include_bytes!("../assets/DejaVuSans-Bold.ttf");
/// Default avatar colours (same order as the website's `.av-c0..5`).
pub const COLORS: [[u8; 3]; 6] = [
    [0x2d, 0x4a, 0x66],
    [0x3f, 0x5a, 0x3a],
    [0x5a, 0x3f, 0x3a],
    [0x4a, 0x3d, 0x5e],
    [0x5e, 0x52, 0x34],
    [0x2f, 0x54, 0x58],
];

fn cache() -> &'static Mutex<HashMap<String, (Instant, Vec<u8>)>> {
    static C: std::sync::OnceLock<Mutex<HashMap<String, (Instant, Vec<u8>)>>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn serve(State(st): State<AppState>, Path(file): Path<String>) -> Response {
    let Some(handle) = file.strip_suffix(".png").map(|h| h.to_lowercase()) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if let Some((t, b)) = cache().lock().unwrap().get(&handle) {
        if t.elapsed() < Duration::from_secs(60) {
            return png(b.clone());
        }
    }
    match render_for(&st, &handle).await {
        Ok(Some(b)) => {
            let mut c = cache().lock().unwrap();
            if c.len() > 2000 {
                c.clear();
            }
            c.insert(handle, (Instant::now(), b.clone()));
            png(b)
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => e.into_response(),
    }
}

fn png(b: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=60"),
        ],
        Body::from(b),
    )
        .into_response()
}

pub struct Card {
    pub name: String,
    pub handle: String,
    pub line1: String,
    pub playing: bool,
    pub line2: String,
    pub color: u8,
    pub avatar: Option<Vec<u8>>,
}

async fn render_for(st: &AppState, handle: &str) -> ApiResult<Option<Vec<u8>>> {
    let row: Option<(Uuid, String, String, bool, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, handle, display_name, share_activity, avatar_kind, avatar_hash FROM users
         WHERE lower(handle) = $1 AND public_profile AND deleted_at IS NULL AND banned_at IS NULL AND delete_after IS NULL",
    )
    .bind(handle)
    .fetch_optional(&st.db)
    .await?;
    let Some((uid, handle, dn, share, kind, hash)) = row else {
        return Ok(None);
    };
    let p = presence::visible_presence(st, uid)
        .await?
        .unwrap_or_else(presence::PresenceView::offline);
    let hidden = crate::account::hidden_games(st, uid).await?;
    let mut playing = false;
    let mut line1 = match p.status.as_str() {
        "online" => "Online".to_string(),
        "away" => "Away".to_string(),
        _ => "Offline".to_string(),
    };
    if let Some(g) = p.game_id.as_ref().filter(|g| !hidden.contains(g)) {
        if let Some(n) = names::public_name(st, g).await? {
            line1 = format!("Playing {n}");
            playing = true;
        }
    }
    let mut line2 = String::from("playfraglink.com");
    if share {
        let (w, total): (i64, i64) = sqlx::query_as(
            "SELECT coalesce(sum(minutes_7d), 0)::bigint, coalesce(sum(display_min), 0)::bigint
             FROM user_game_stats WHERE user_id = $1 AND NOT (game_id = ANY($2))",
        )
        .bind(uid)
        .bind(&hidden)
        .fetch_one(&st.db)
        .await?;
        line2 = format!("{} this week  ·  {} total", hrs(w), hrs(total));
        if !playing {
            let last: Option<String> = sqlx::query_scalar(
                "SELECT game_id FROM sessions WHERE user_id = $1 AND NOT (game_id = ANY($2)) ORDER BY ended_at DESC LIMIT 1",
            )
            .bind(uid)
            .bind(&hidden)
            .fetch_optional(&st.db)
            .await?;
            if let Some(g) = last {
                if let Some(n) = names::public_name(st, &g).await? {
                    line1 = format!("{line1}  ·  last played {n}");
                }
            }
        }
    }
    let avatar = match (kind.as_deref(), hash) {
        (Some("upload"), Some(h)) => std::fs::read(
            st.cfg
                .data_dir
                .join("avatars")
                .join(uid.to_string())
                .join(format!("{h}-64.webp")),
        )
        .ok(),
        _ => None,
    };
    let card = Card {
        name: dn,
        handle,
        line1,
        playing,
        line2,
        color: avatars::color_of(uid),
        avatar,
    };
    Ok(Some(
        tokio::task::spawn_blocking(move || render(&card))
            .await
            .map_err(crate::error::AppError::internal)?,
    ))
}

fn hrs(min: i64) -> String {
    if min <= 0 {
        "0 h".into()
    } else if min < 60 {
        "<1 h".into()
    } else {
        format!("{} h", (min + 30) / 60)
    }
}

fn blend(img: &mut RgbaImage, x: i32, y: i32, c: [u8; 3], a: f32) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 || a <= 0.0 {
        return;
    }
    let p = img.get_pixel_mut(x as u32, y as u32);
    let a = a.min(1.0);
    for (ch, want) in p.0.iter_mut().zip(c) {
        *ch = (*ch as f32 * (1.0 - a) + want as f32 * a) as u8;
    }
}

#[allow(clippy::too_many_arguments)]
fn text(
    img: &mut RgbaImage,
    font: &FontRef,
    px: f32,
    x: f32,
    baseline: f32,
    s: &str,
    c: [u8; 3],
    max_w: f32,
) {
    let sf = font.as_scaled(PxScale::from(px));
    let mut pen = x;
    let mut prev = None;
    let ell = "…";
    let chars: Vec<char> = s.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        let id = sf.glyph_id(*ch);
        if let Some(p) = prev {
            pen += sf.kern(p, id);
        }
        let adv = sf.h_advance(id);
        if pen + adv > x + max_w && i + 1 < chars.len() {
            text(img, font, px, pen, baseline, ell, c, 100.0);
            return;
        }
        let g = id.with_scale_and_position(PxScale::from(px), ab_glyph::point(pen, baseline));
        if let Some(o) = font.outline_glyph(g) {
            let b = o.px_bounds();
            o.draw(|gx, gy, cov| {
                blend(
                    img,
                    b.min.x as i32 + gx as i32,
                    b.min.y as i32 + gy as i32,
                    c,
                    cov,
                )
            });
        }
        pen += adv;
        prev = Some(id);
    }
}

pub fn render(c: &Card) -> Vec<u8> {
    let reg = FontRef::try_from_slice(REG).expect("font");
    let bold = FontRef::try_from_slice(BOLD).expect("font");
    let mut img = RgbaImage::new(W, H);
    // background: dark with a blue glossy top band
    for y in 0..H {
        let t = y as f32 / H as f32;
        let top = [0x1c, 0x2c, 0x40];
        let bot = [0x10, 0x13, 0x18];
        let col = [
            (top[0] as f32 * (1.0 - t) + bot[0] as f32 * t) as u8,
            (top[1] as f32 * (1.0 - t) + bot[1] as f32 * t) as u8,
            (top[2] as f32 * (1.0 - t) + bot[2] as f32 * t) as u8,
        ];
        for x in 0..W {
            img.put_pixel(x, y, Rgba([col[0], col[1], col[2], 255]));
        }
    }
    for x in 0..W {
        img.put_pixel(x, 0, Rgba([0x3a, 0x5a, 0x80, 255]));
        img.put_pixel(x, H - 1, Rgba([0x05, 0x06, 0x08, 255]));
    }
    for y in 0..H {
        img.put_pixel(0, y, Rgba([0x26, 0x2c, 0x35, 255]));
        img.put_pixel(W - 1, y, Rgba([0x26, 0x2c, 0x35, 255]));
    }
    // avatar 72 x 72 at (14, 14)
    let (ax, ay, asz) = (14u32, 14u32, 72u32);
    let pasted = c
        .avatar
        .as_ref()
        .and_then(|b| image::load_from_memory(b).ok())
        .map(|a| {
            let a = image::imageops::resize(
                &a.to_rgba8(),
                asz,
                asz,
                image::imageops::FilterType::Triangle,
            );
            image::imageops::overlay(&mut img, &a, ax as i64, ay as i64);
        });
    if pasted.is_none() {
        let col = COLORS[(c.color % 6) as usize];
        for y in ay..ay + asz {
            for x in ax..ax + asz {
                img.put_pixel(x, y, Rgba([col[0], col[1], col[2], 255]));
            }
        }
        text(
            &mut img,
            &bold,
            44.0,
            ax as f32 + 23.0,
            ay as f32 + 52.0,
            "F",
            [0xee, 0xf2, 0xf6],
            60.0,
        );
    }
    let x0 = (ax + asz + 14) as f32;
    let maxw = W as f32 - x0 - 14.0;
    text(
        &mut img,
        &bold,
        19.0,
        x0,
        34.0,
        &c.name,
        [0xee, 0xf2, 0xf6],
        maxw,
    );
    let lc = if c.playing {
        [0x8f, 0xd6, 0x7a]
    } else {
        [0xb9, 0xbe, 0xc5]
    };
    text(&mut img, &reg, 13.0, x0, 56.0, &c.line1, lc, maxw);
    text(
        &mut img,
        &reg,
        12.0,
        x0,
        77.0,
        &c.line2,
        [0x8a, 0x93, 0x9e],
        maxw - 80.0,
    );
    text(
        &mut img,
        &bold,
        11.0,
        W as f32 - 76.0,
        88.0,
        "FRAGLINK",
        [0x5f, 0x8f, 0xc4],
        70.0,
    );
    let _ = &c.handle;
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .expect("png");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_png() {
        let b = super::render(&super::Card {
            name: "Tester".into(),
            handle: "tester".into(),
            line1: "Playing Dota 2".into(),
            playing: true,
            line2: "3 h this week  ·  120 h total".into(),
            color: 2,
            avatar: None,
        });
        let i = image::load_from_memory(&b).unwrap();
        assert_eq!((i.width(), i.height()), (460, 100));
    }
}
