//! Game icons, sourced locally the way the old Xfire client effectively did:
//!
//! 1. our own cache (`<data dir>/icons/<game>.png`, 32x32 PNG shown at 16 px),
//! 2. the icon inside the detected game's exe (Windows only, read at runtime),
//! 3. Steam's own library cache for Steam games
//!    (`Steam\appcache\librarycache\<appid>_icon.jpg`, or the per-app folder
//!    `librarycache\<appid>\<sha1>.jpg` used by newer Steam clients),
//! 4. otherwise nothing, and the UI draws a neutral generic game icon.
//!
//! Friends' games that aren't on this PC can use Steam's public community icon
//! (see [`steam_cdn_icon_url`]); the desktop app fetches that and stores it here.
//! No third-party icons are bundled with Fraglink.

use anyhow::{Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Stored size. The UI shows icons at 16x16 CSS px; 32 px keeps them crisp at 150-200 % zoom.
pub const ICON_PX: u32 = 32;
/// How long a "no icon found" result is remembered before trying again.
pub const MISS_TTL: Duration = Duration::from_secs(24 * 3600);

pub struct IconCache {
    dir: PathBuf,
}

/// File-name-safe key for a game id ("steam:570" -> "steam_570-<hash>").
pub fn cache_key(game_id: &str) -> String {
    let safe: String = game_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    // FNV-1a keeps ids that differ only in case/punctuation apart.
    let mut h: u32 = 0x811c9dc5;
    for b in game_id.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x01000193);
    }
    format!("{safe}-{h:08x}")
}

impl IconCache {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        IconCache { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn png_path(&self, game_id: &str) -> PathBuf {
        self.dir.join(format!("{}.png", cache_key(game_id)))
    }

    fn miss_path(&self, game_id: &str) -> PathBuf {
        self.dir.join(format!("{}.none", cache_key(game_id)))
    }

    pub fn get(&self, game_id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.png_path(game_id))
            .ok()
            .filter(|b| !b.is_empty())
    }

    pub fn put(&self, game_id: &str, png: &[u8]) -> Result<()> {
        std::fs::create_dir_all(&self.dir).context("creating icon cache")?;
        let tmp = self.dir.join(format!("{}.tmp", cache_key(game_id)));
        std::fs::write(&tmp, png)?;
        std::fs::rename(&tmp, self.png_path(game_id))?;
        let _ = std::fs::remove_file(self.miss_path(game_id));
        Ok(())
    }

    /// True if we looked recently and found nothing.
    pub fn recent_miss(&self, game_id: &str, now: SystemTime) -> bool {
        std::fs::metadata(self.miss_path(game_id))
            .and_then(|m| m.modified())
            .ok()
            // A marker written a moment after `now` (clock granularity) is fresh.
            .map(|t| now.duration_since(t).unwrap_or(Duration::ZERO))
            .is_some_and(|age| age < MISS_TTL)
    }

    fn stamp_path(&self, game_id: &str) -> PathBuf {
        self.dir.join(format!("{}.exe", cache_key(game_id)))
    }

    /// Remember which exe build (mtime + size) a cached icon came from.
    pub fn put_exe_stamp(&self, game_id: &str, exe: &Path) {
        if let Some(st) = exe_stamp(exe) {
            let _ = std::fs::write(self.stamp_path(game_id), st);
        }
    }

    /// True when the cached icon was extracted from `exe` and the exe has since
    /// changed (a game update can change its icon). Icons from other sources,
    /// or an exe we can't stat, are never stale.
    pub fn exe_changed(&self, game_id: &str, exe: &Path) -> bool {
        match (
            std::fs::read_to_string(self.stamp_path(game_id)),
            exe_stamp(exe),
        ) {
            (Ok(old), Some(now)) => old != now,
            _ => false,
        }
    }

    pub fn put_miss(&self, game_id: &str) {
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let _ = std::fs::write(self.miss_path(game_id), b"");
        }
    }
}

/// "mtime_secs:size" of a file, used to notice a changed exe.
fn exe_stamp(exe: &Path) -> Option<String> {
    let m = std::fs::metadata(exe).ok()?;
    let t = m
        .modified()
        .ok()?
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(format!("{t}:{}", m.len()))
}

/// Decode any supported image (PNG, JPEG, ICO, BMP), scale it to
/// [`ICON_PX`] square and encode as PNG. Small sources are scaled up with
/// nearest-neighbour so pixel art stays sharp; big ones use Lanczos.
pub fn normalize(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    Some(encode_png(&fit(img)))
}

pub fn normalize_rgba(img: RgbaImage) -> Vec<u8> {
    encode_png(&fit(DynamicImage::ImageRgba8(img)))
}

fn fit(img: DynamicImage) -> RgbaImage {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let filter = if w.max(h) < ICON_PX {
        FilterType::Nearest
    } else {
        FilterType::Lanczos3
    };
    // keep aspect ratio, centre on a transparent square
    let scaled = img.resize(ICON_PX, ICON_PX, filter).to_rgba8();
    let mut out = RgbaImage::from_pixel(ICON_PX, ICON_PX, Rgba([0, 0, 0, 0]));
    let x = (ICON_PX - scaled.width()) / 2;
    let y = (ICON_PX - scaled.height()) / 2;
    image::imageops::overlay(&mut out, &scaled, x as i64, y as i64);
    out
}

fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    let _ = img.write_to(&mut buf, ImageFormat::Png);
    buf.into_inner()
}

/// Steam's locally cached icon for `appid`, if the Steam client has one.
pub fn steam_library_icon(steam_root: &Path, appid: u32) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    let flat = cache.join(format!("{appid}_icon.jpg"));
    if flat.is_file() {
        return Some(flat);
    }
    // Newer clients: librarycache/<appid>/<40-hex sha1>.jpg is the small icon
    // (next to header.jpg, library_600x900.jpg, logo.png ...).
    let mut found: Vec<PathBuf> = std::fs::read_dir(cache.join(appid.to_string()))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg"))
                && p.file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

/// Public Steam community icon (the hash comes from Steam's `img_icon_url`).
pub fn steam_cdn_icon_url(appid: u32, icon_hash: &str) -> Option<String> {
    (icon_hash.len() == 40 && icon_hash.chars().all(|c| c.is_ascii_hexdigit())).then(|| {
        format!(
            "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/{appid}/{}.jpg",
            icon_hash.to_ascii_lowercase()
        )
    })
}

/// Steam app id for a game id like `steam:570`.
pub fn appid_from_game_id(game_id: &str) -> Option<u32> {
    game_id.strip_prefix("steam:")?.parse().ok()
}

/// Where to look for one game's icon on this PC.
#[derive(Debug, Default, Clone)]
pub struct LocalSources {
    pub exe_path: Option<PathBuf>,
    pub steam_appid: Option<u32>,
    pub steam_root: Option<PathBuf>,
}

/// Which source produced an icon (for logs and tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    Cache,
    Exe,
    SteamLibrary,
}

/// Cache → exe icon → Steam library cache. Found icons are normalised and cached.
/// `extract` reads the icon from an exe (see [`extract_exe_icon`]; the simulation
/// passes a placeholder generator).
pub fn resolve_local(
    cache: &IconCache,
    game_id: &str,
    src: &LocalSources,
    extract: &dyn Fn(&Path) -> Option<RgbaImage>,
) -> Option<(Vec<u8>, Found)> {
    let exe_changed = src
        .exe_path
        .as_deref()
        .is_some_and(|e| cache.exe_changed(game_id, e));
    if !exe_changed {
        if let Some(b) = cache.get(game_id) {
            return Some((b, Found::Cache));
        }
    }
    if let Some(exe) = &src.exe_path {
        if let Some(img) = extract(exe) {
            let png = normalize_rgba(img);
            let _ = cache.put(game_id, &png);
            cache.put_exe_stamp(game_id, exe);
            return Some((png, Found::Exe));
        }
    }
    let appid = src.steam_appid.or_else(|| appid_from_game_id(game_id));
    if let (Some(root), Some(appid)) = (&src.steam_root, appid) {
        if let Some(p) = steam_library_icon(root, appid) {
            if let Some(png) = std::fs::read(&p).ok().and_then(|b| normalize(&b)) {
                let _ = cache.put(game_id, &png);
                return Some((png, Found::SteamLibrary));
            }
        }
    }
    None
}

/// Simulation stand-in for an exe icon: a coloured square with a glossy top,
/// derived from the name. Only used in `--sim` mode.
pub fn placeholder(name: &str) -> RgbaImage {
    let mut h: u32 = 0;
    for c in name.chars() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    let (r, g, b) = hsv((h % 360) as f32, 0.65, 0.85);
    let (r2, g2, b2) = hsv(((h + 40) % 360) as f32, 0.7, 0.35);
    let n = ICON_PX;
    RgbaImage::from_fn(n, n, |x, y| {
        let edge = x == 0 || y == 0 || x == n - 1 || y == n - 1;
        if edge {
            return Rgba([20, 20, 28, 255]);
        }
        let t = (x + y) as f32 / (2 * n) as f32;
        let mix = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t) as u8;
        let mut px = [mix(r, r2), mix(g, g2), mix(b, b2), 255];
        if y < n * 2 / 5 {
            for c in px.iter_mut().take(3) {
                *c = (*c as u16 + (255 - *c as u16) / 3) as u8;
            }
        }
        Rgba(px)
    })
}

fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

/// Read the large icon embedded in an exe (Windows shell `ExtractIconExW`).
/// Returns None on other platforms or when the exe has no icon.
#[cfg(windows)]
pub fn extract_exe_icon(path: &Path) -> Option<RgbaImage> {
    win::extract(path)
}

#[cfg(not(windows))]
pub fn extract_exe_icon(_path: &Path) -> Option<RgbaImage> {
    None
}

#[cfg(windows)]
mod win {
    use image::RgbaImage;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
    };
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    pub fn extract(path: &Path) -> Option<RgbaImage> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut large: HICON = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated path; asks for one icon at index 0.
        let n = unsafe { ExtractIconExW(wide.as_ptr(), 0, &mut large, std::ptr::null_mut(), 1) };
        if n == 0 || large.is_null() {
            return None;
        }
        let img = unsafe { icon_to_rgba(large) };
        unsafe { DestroyIcon(large) };
        img
    }

    unsafe fn bitmap_bits(
        dc: windows_sys::Win32::Graphics::Gdi::HDC,
        bmp: HBITMAP,
        w: i32,
        h: i32,
    ) -> Option<Vec<u8>> {
        let mut bi: BITMAPINFO = std::mem::zeroed();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let lines = GetDIBits(
            dc,
            bmp,
            0,
            h as u32,
            buf.as_mut_ptr().cast(),
            &mut bi,
            DIB_RGB_COLORS,
        );
        (lines == h).then_some(buf)
    }

    unsafe fn icon_to_rgba(icon: HICON) -> Option<RgbaImage> {
        let mut info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(icon, &mut info) == 0 {
            return None;
        }
        let color = info.hbmColor;
        let mask = info.hbmMask;
        let out = (|| {
            if color.is_null() {
                return None; // monochrome icon: let the UI use the generic icon
            }
            let mut bm: BITMAP = std::mem::zeroed();
            if GetObjectW(
                color,
                std::mem::size_of::<BITMAP>() as i32,
                (&mut bm as *mut BITMAP).cast(),
            ) == 0
            {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
                return None;
            }
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let px = bitmap_bits(dc, color, w, h);
            let mk = if mask.is_null() {
                None
            } else {
                bitmap_bits(dc, mask, w, h)
            };
            DeleteDC(dc);
            let mut px = px?;
            let has_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
            for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                p.swap(0, 2); // BGRA -> RGBA
                if !has_alpha {
                    // old icons: the AND mask says which pixels are transparent
                    let transparent = mk.as_ref().is_some_and(|m| m[i * 4] != 0);
                    p[3] = if transparent { 0 } else { 255 };
                }
            }
            RgbaImage::from_raw(w as u32, h as u32, px)
        })();
        if !color.is_null() {
            DeleteObject(color);
        }
        if !mask.is_null() {
            DeleteObject(mask);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpg(w: u32, h: u32) -> Vec<u8> {
        let img =
            DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30])));
        let mut b = Cursor::new(Vec::new());
        img.write_to(&mut b, ImageFormat::Jpeg).unwrap();
        b.into_inner()
    }

    fn size_of_png(b: &[u8]) -> (u32, u32) {
        let i = image::load_from_memory(b).unwrap();
        (i.width(), i.height())
    }

    #[test]
    fn keys_are_safe_and_distinct() {
        let a = cache_key("steam:570");
        assert!(a.starts_with("steam_570-"), "{a}");
        assert!(!cache_key("../../etc/passwd").contains('/'));
        assert_ne!(cache_key("manual:a.b"), cache_key("manual:a_b"));
    }

    #[test]
    fn normalizes_to_32px_square() {
        assert_eq!(size_of_png(&normalize(&jpg(64, 64)).unwrap()), (32, 32));
        assert_eq!(size_of_png(&normalize(&jpg(16, 16)).unwrap()), (32, 32));
        assert_eq!(size_of_png(&normalize(&jpg(184, 69)).unwrap()), (32, 32));
        assert!(normalize(b"not an image").is_none());
    }

    #[test]
    fn steam_library_cache_both_layouts() {
        let d = tempfile::tempdir().unwrap();
        let lc = d.path().join("appcache/librarycache");
        std::fs::create_dir_all(lc.join("730")).unwrap();
        std::fs::write(lc.join("570_icon.jpg"), jpg(32, 32)).unwrap();
        std::fs::write(lc.join("730/header.jpg"), jpg(460, 215)).unwrap();
        let hash = "0123456789abcdef0123456789abcdef01234567";
        std::fs::write(lc.join(format!("730/{hash}.jpg")), jpg(32, 32)).unwrap();
        assert_eq!(
            steam_library_icon(d.path(), 570).unwrap(),
            lc.join("570_icon.jpg")
        );
        assert_eq!(
            steam_library_icon(d.path(), 730).unwrap(),
            lc.join(format!("730/{hash}.jpg"))
        );
        assert!(steam_library_icon(d.path(), 1).is_none());
    }

    #[test]
    fn resolve_order_and_cache() {
        let d = tempfile::tempdir().unwrap();
        let cache = IconCache::new(d.path().join("icons"));
        let root = d.path().join("steam");
        std::fs::create_dir_all(root.join("appcache/librarycache")).unwrap();
        std::fs::write(root.join("appcache/librarycache/570_icon.jpg"), jpg(32, 32)).unwrap();
        let none = |_: &Path| None;
        let ph = |p: &Path| Some(placeholder(&p.to_string_lossy()));

        // nothing known -> None
        assert!(resolve_local(&cache, "wow", &LocalSources::default(), &none).is_none());
        // exe icon wins over Steam
        let src = LocalSources {
            exe_path: Some("C:/g/dota2.exe".into()),
            steam_appid: Some(570),
            steam_root: Some(root.clone()),
        };
        let (png, f) = resolve_local(&cache, "dota", &src, &ph).unwrap();
        assert_eq!(f, Found::Exe);
        assert_eq!(size_of_png(&png), (32, 32));
        // then served from cache even without sources
        assert_eq!(
            resolve_local(&cache, "dota", &LocalSources::default(), &none)
                .unwrap()
                .1,
            Found::Cache
        );
        // exe without an icon falls back to the Steam library cache (app id from the game id)
        let src = LocalSources {
            exe_path: Some("x.exe".into()),
            steam_appid: None,
            steam_root: Some(root),
        };
        assert_eq!(
            resolve_local(&cache, "steam:570", &src, &none).unwrap().1,
            Found::SteamLibrary
        );
        assert!(cache.png_path("steam:570").is_file());
    }

    #[test]
    fn changed_exe_re_extracts_icon() {
        let d = tempfile::tempdir().unwrap();
        let cache = IconCache::new(d.path().join("icons"));
        let exe = d.path().join("game.exe");
        std::fs::write(&exe, b"v1").unwrap();
        let src = LocalSources {
            exe_path: Some(exe.clone()),
            ..Default::default()
        };
        let calls = std::cell::Cell::new(0);
        let ex = |p: &Path| {
            calls.set(calls.get() + 1);
            Some(placeholder(&p.to_string_lossy()))
        };
        assert_eq!(resolve_local(&cache, "g", &src, &ex).unwrap().1, Found::Exe);
        assert!(!cache.exe_changed("g", &exe));
        assert_eq!(
            resolve_local(&cache, "g", &src, &ex).unwrap().1,
            Found::Cache
        );
        assert_eq!(calls.get(), 1);
        // a game update changes the exe size -> extracted again
        std::fs::write(&exe, b"version two").unwrap();
        assert!(cache.exe_changed("g", &exe));
        assert_eq!(resolve_local(&cache, "g", &src, &ex).unwrap().1, Found::Exe);
        assert_eq!(calls.get(), 2);
        assert!(!cache.exe_changed("g", &exe));
        // icons from other sources have no stamp and are never stale
        assert!(!cache.exe_changed("other", &exe));
    }

    #[test]
    fn misses_expire() {
        let d = tempfile::tempdir().unwrap();
        let cache = IconCache::new(d.path());
        let now = SystemTime::now();
        assert!(!cache.recent_miss("x", now));
        cache.put_miss("x");
        assert!(cache.recent_miss("x", now));
        assert!(!cache.recent_miss("x", now + MISS_TTL + Duration::from_secs(5)));
        cache.put("x", &normalize_rgba(placeholder("x"))).unwrap();
        assert!(!cache.recent_miss("x", now), "a found icon clears the miss");
    }

    #[test]
    fn cdn_url_needs_a_real_hash() {
        assert!(steam_cdn_icon_url(570, "../evil").is_none());
        let u = steam_cdn_icon_url(570, "0BAD0BAD0BAD0BAD0BAD0BAD0BAD0BAD0BAD0BAD").unwrap();
        assert!(u.starts_with(
            "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps/570/0bad"
        ));
        assert_eq!(appid_from_game_id("steam:570"), Some(570));
        assert_eq!(appid_from_game_id("wow"), None);
    }
}
