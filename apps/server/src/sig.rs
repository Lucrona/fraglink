//! Signature image: a 460 x 100 PNG status card for forums and Discord bios
//! (`/api/v1/sig/<handle>.png`). Only for profiles set to public; shows the avatar,
//! name, what they're playing now (or last played), and hours this week / all time.
//! Rendered on demand and cached for 30 seconds.

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
/// The Fraglink "F" mark (default avatar), rendered from site-web's mark.svg.
static MARK: &[u8] = include_bytes!("../assets/mark.png");
const CACHE_SECS: u64 = 30;
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
    if !crate::features::on(&st, "public_profiles").await {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Some((t, b)) = cache().lock().unwrap().get(&handle) {
        if t.elapsed() < Duration::from_secs(CACHE_SECS) {
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

/// Drop a cached card (privacy changed).
pub fn forget(handle: &str) {
    cache().lock().unwrap().remove(&handle.to_lowercase());
}

fn png(b: Vec<u8>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=30, must-revalidate"),
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
    /// Preset glyph index (0..11) when the user picked a preset picture.
    pub preset: Option<i16>,
    /// 460x215 game art (current game, else last played) for the background.
    pub art: Option<Vec<u8>>,
}

async fn render_for(st: &AppState, handle: &str) -> ApiResult<Option<Vec<u8>>> {
    type Row = (
        Uuid,
        String,
        String,
        bool,
        Option<String>,
        Option<String>,
        serde_json::Value,
        Option<i16>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, handle, display_name, share_activity, avatar_kind, avatar_hash, privacy, avatar_preset FROM users
         WHERE (lower(handle) = $1 OR id = (SELECT user_id FROM handle_redirects WHERE old_handle = $1)) AND public_profile AND deleted_at IS NULL AND banned_at IS NULL AND delete_after IS NULL AND NOT is_test",
    )
    .bind(handle)
    .fetch_optional(&st.db)
    .await?;
    let Some((uid, handle, dn, share, kind, hash, privacy, preset)) = row else {
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
    let mut art_game: Option<String> = None;
    let show_game = crate::account::shows(&privacy, "now_playing");
    let show_hours = crate::account::shows(&privacy, "games");
    if let Some(g) = p
        .game_id
        .as_ref()
        .filter(|g| show_game && !hidden.contains(g))
    {
        if let Some(n) = names::public_name(st, g).await? {
            line1 = format!("Playing {n}");
            if let Some(t) = p.session_started_at {
                let m = (chrono::Utc::now() - t).num_minutes().max(0);
                line1 = format!(
                    "{line1}  ·  for {}",
                    if m < 60 {
                        format!("{m} min")
                    } else {
                        format!("{} h {} min", m / 60, m % 60)
                    }
                );
            }
            playing = true;
            art_game = Some(g.clone());
        }
    }
    let mut line2 = String::from("playfraglink.com");
    if show_hours {
        let (w, total): (i64, i64) = sqlx::query_as(
            "SELECT coalesce(sum(minutes_7d), 0)::bigint, coalesce(sum(display_min), 0)::bigint
             FROM user_game_stats WHERE user_id = $1 AND NOT (game_id = ANY($2))",
        )
        .bind(uid)
        .bind(&hidden)
        .fetch_one(&st.db)
        .await?;
        line2 = format!("{} this week  ·  {} total", hrs(w), hrs(total));
        if !playing && share && show_game {
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
                    art_game = Some(g.clone());
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
                .join(format!("{h}-184.webp")),
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
        preset: if kind.as_deref() == Some("preset") {
            preset
        } else {
            None
        },
        art: match art_game {
            Some(g) => crate::art::bytes(st, &g).await,
            None => None,
        },
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

const PRESET_GLYPHS: [&str; 12] = [
    "\u{2694}", "\u{25C9}", "\u{25B2}", "\u{2605}", "\u{25C6}", "\u{2726}", "\u{2663}", "\u{2699}",
    "\u{2620}", "\u{265B}", "\u{26A1}", "\u{271A}",
];

fn lerp(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t) as u8)
}

pub fn render(c: &Card) -> Vec<u8> {
    use image::imageops::{overlay, resize, FilterType};
    let reg = FontRef::try_from_slice(REG).expect("font");
    let bold = FontRef::try_from_slice(BOLD).expect("font");
    let mut img = RgbaImage::new(W, H);
    // base: deep blue gradient (v2 style)
    for y in 0..H {
        let col = lerp([0x1a, 0x2e, 0x48], [0x0b, 0x10, 0x18], y as f32 / H as f32);
        for x in 0..W {
            img.put_pixel(x, y, Rgba([col[0], col[1], col[2], 255]));
        }
    }
    // game art on the right, faded into the background (cover: fill height, keep ratio)
    if let Some(a) = c.art.as_ref().and_then(|b| image::load_from_memory(b).ok()) {
        let aw = (a.width() as f32 * H as f32 / a.height() as f32).round() as u32;
        let a = resize(&a.to_rgba8(), aw.max(1), H, FilterType::Triangle);
        let x0 = W.saturating_sub(aw) as i64;
        overlay(&mut img, &a, x0, 0);
        // fade: fully background at x0, mostly art at the right edge
        for x in x0.max(0) as u32..W {
            let t = ((x as f32 - x0 as f32) / aw as f32).clamp(0.0, 1.0);
            let keep = (t * 1.6).min(0.62); // how much art shows through
            for y in 0..H {
                let p = *img.get_pixel(x, y);
                let bg = lerp([0x14, 0x22, 0x36], [0x0b, 0x10, 0x18], y as f32 / H as f32);
                let mix = |i: usize| (p[i] as f32 * keep + bg[i] as f32 * (1.0 - keep)) as u8;
                img.put_pixel(x, y, Rgba([mix(0), mix(1), mix(2), 255]));
            }
        }
    }
    // glossy top band + frame
    for y in 0..H / 2 {
        let a = 0.07 * (1.0 - y as f32 / (H as f32 / 2.0));
        for x in 0..W {
            blend(&mut img, x as i32, y as i32, [255, 255, 255], a);
        }
    }
    for x in 0..W {
        img.put_pixel(x, 0, Rgba([0x4a, 0x7a, 0xae, 255]));
        img.put_pixel(x, H - 1, Rgba([0x05, 0x06, 0x08, 255]));
    }
    for y in 0..H {
        img.put_pixel(0, y, Rgba([0x2a, 0x3c, 0x52, 255]));
        img.put_pixel(W - 1, y, Rgba([0x2a, 0x3c, 0x52, 255]));
    }
    // avatar 72 x 72 at (14, 14) with a border
    let (ax, ay, asz) = (14u32, 14u32, 72u32);
    for y in ay - 2..ay + asz + 2 {
        for x in ax - 2..ax + asz + 2 {
            img.put_pixel(x, y, Rgba([0x3b, 0x58, 0x78, 255]));
        }
    }
    let pasted = c
        .avatar
        .as_ref()
        .and_then(|b| image::load_from_memory(b).ok())
        .map(|a| {
            // dark backdrop for transparent pictures (same as the site's panels)
            for y in ay..ay + asz {
                for x in ax..ax + asz {
                    img.put_pixel(x, y, Rgba([0x1b, 0x1f, 0x26, 255]));
                }
            }
            let a = resize(&a.to_rgba8(), asz, asz, FilterType::Lanczos3);
            overlay(&mut img, &a, ax as i64, ay as i64);
        });
    if pasted.is_none() {
        let col = COLORS[(c.color % 6) as usize];
        for y in ay..ay + asz {
            let cc = lerp(
                [
                    col[0].saturating_add(18),
                    col[1].saturating_add(18),
                    col[2].saturating_add(18),
                ],
                col,
                (y - ay) as f32 / asz as f32,
            );
            for x in ax..ax + asz {
                img.put_pixel(x, y, Rgba([cc[0], cc[1], cc[2], 255]));
            }
        }
        match c.preset {
            Some(p) => text(
                &mut img,
                &reg,
                38.0,
                ax as f32 + 17.0,
                ay as f32 + 50.0,
                PRESET_GLYPHS[(p.rem_euclid(12)) as usize],
                [0xee, 0xf2, 0xf6],
                60.0,
            ),
            None => {
                if let Ok(m) = image::load_from_memory(MARK) {
                    let m = resize(&m.to_rgba8(), 56, 47, FilterType::Lanczos3);
                    overlay(&mut img, &m, (ax + 8) as i64, (ay + 13) as i64);
                }
            }
        }
    }
    let x0 = (ax + asz + 16) as f32;
    let maxw = W as f32 - x0 - 14.0;
    text(
        &mut img,
        &bold,
        19.0,
        x0,
        33.0,
        &c.name,
        [0xf2, 0xf6, 0xfa],
        maxw,
    );
    if c.playing {
        // green "now playing" dot
        for dy in -4i32..=4 {
            for dx in -4i32..=4 {
                let d = ((dx * dx + dy * dy) as f32).sqrt();
                if d <= 4.2 {
                    blend(
                        &mut img,
                        x0 as i32 + 4 + dx,
                        51 + dy,
                        [0x5c, 0xd6, 0x6a],
                        (4.2 - d).min(1.0),
                    );
                }
            }
        }
        text(
            &mut img,
            &bold,
            13.0,
            x0 + 13.0,
            56.0,
            &c.line1,
            [0x8f, 0xe6, 0x86],
            maxw - 13.0,
        );
    } else {
        text(
            &mut img,
            &reg,
            13.0,
            x0,
            56.0,
            &c.line1,
            [0xb9, 0xc4, 0xd0],
            maxw,
        );
    }
    text(
        &mut img,
        &reg,
        12.0,
        x0,
        77.0,
        &c.line2,
        [0x9a, 0xa8, 0xb8],
        maxw - 80.0,
    );
    text(
        &mut img,
        &bold,
        11.0,
        W as f32 - 76.0,
        90.0,
        "FRAGLINK",
        [0x7f, 0xb4, 0xee],
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
            preset: None,
            art: None,
        });
        let i = image::load_from_memory(&b).unwrap();
        assert_eq!((i.width(), i.height()), (460, 100));
    }

    /// `SIG_PREVIEW_ART=/path/art.jpg cargo test sig::tests::preview` writes /tmp/sig-*.png.
    #[test]
    fn preview() {
        let Ok(p) = std::env::var("SIG_PREVIEW_ART") else {
            return;
        };
        let art = std::fs::read(p).ok();
        for (n, playing, preset) in [("playing", true, None), ("offline", false, Some(3))] {
            let b = super::render(&super::Card {
                name: "fraglinktest".into(),
                handle: "x".into(),
                line1: if playing {
                    "Playing World of Warcraft  ·  for 12 min".into()
                } else {
                    "Online  ·  last played World of Warcraft: Forever - Beta".into()
                },
                playing,
                line2: "13 h this week  ·  15 h total".into(),
                color: 1,
                avatar: None,
                preset,
                art: art.clone(),
            });
            std::fs::write(format!("/tmp/sig-{n}.png"), b).unwrap();
        }
    }
}
