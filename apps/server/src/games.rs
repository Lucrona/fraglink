//! Game detection lists for the desktop app (0.2.0-dev.12).
//!
//! * `GET /api/v1/games/detectable`: Discord's public detectable list, re-downloaded
//!   once a day, trimmed to Windows game exes (see `fraglink_core::detectable`),
//!   served gzip-compressed with an ETag. Until the first download works the
//!   snapshot bundled with this build is served.
//! * `GET /api/v1/games/overrides`: Fraglink's folder rules (WoW flavours, Star
//!   Citizen channels). `<data dir>/detect-overrides.json` replaces the bundled copy
//!   when present and valid, so a renamed beta exe is fixed without an app release.
//! * `POST /api/v1/games/suggest`: "Track it as a game" answers (exe + name), kept
//!   in `game_suggestions` for review. Nothing is published automatically.

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, Response, StatusCode};
use axum::Json;
use fraglink_core::detectable::{self, DetectableDb};
use fraglink_core::overrides::{self, Overrides};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

pub const DISCORD_URL: &str = "https://discord.com/api/v10/applications/detectable";
/// Discord serves this list to browsers; a bare client UA gets refused.
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";
const REFRESH_EVERY: Duration = Duration::from_secs(24 * 3600);
/// A real list has ~24k apps; anything far smaller is an error page or a bad day.
const MIN_APPS: usize = 5000;

/// A built list ready to serve.
#[derive(Clone)]
pub struct Built {
    pub etag: String,
    pub json: Arc<Vec<u8>>,
    pub gz: Arc<Vec<u8>>,
    pub games: usize,
    pub updated: String,
}

#[derive(Default)]
pub struct GameLists {
    built: RwLock<Option<Built>>,
}

fn etag_of(b: &[u8]) -> String {
    let h = Sha256::digest(b);
    let hex: String = h[..12].iter().map(|x| format!("{x:02x}")).collect();
    format!("\"{hex}\"")
}

fn gzip(b: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(b)?;
    gz.finish()
}

fn built_from_json(json: Vec<u8>, games: usize, updated: String) -> std::io::Result<Built> {
    Ok(Built {
        etag: etag_of(&json),
        gz: Arc::new(gzip(&json)?),
        json: Arc::new(json),
        games,
        updated,
    })
}

/// The snapshot compiled into this binary.
fn bundled() -> Option<Built> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(detectable::BUNDLED_GZ)
        .read_to_end(&mut json)
        .ok()?;
    let db = DetectableDb::from_json(&json).ok()?;
    built_from_json(json, db.len(), db.updated.clone()).ok()
}

impl GameLists {
    pub fn current(&self) -> Option<Built> {
        if let Some(b) = self.built.read().unwrap().clone() {
            return Some(b);
        }
        let b = bundled()?;
        let mut w = self.built.write().unwrap();
        Some(w.get_or_insert(b).clone())
    }

    fn set(&self, b: Built) {
        *self.built.write().unwrap() = Some(b);
    }
}

fn list_dir(data: &Path) -> PathBuf {
    data.join("detectable")
}

/// Build from the cached raw Discord file, freezing slugs in `slugs.json`.
pub fn build_from_cache(
    data: &Path,
    sigs: &fraglink_core::signatures::SignatureDb,
) -> anyhow::Result<Built> {
    let dir = list_dir(data);
    let raw = std::fs::read(dir.join("discord.json"))?;
    let slugs_path = dir.join("slugs.json");
    // ids handed out before (this server's file wins over the bundled snapshot's)
    let mut frozen = detectable::bundled_slugs();
    if let Ok(b) = std::fs::read(&slugs_path) {
        frozen.extend(serde_json::from_slice::<BTreeMap<String, String>>(&b)?);
    }
    let mtime = std::fs::metadata(dir.join("discord.json"))?.modified()?;
    let updated = chrono::DateTime::<chrono::Utc>::from(mtime)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let (file, slugs) = detectable::build_from_discord(&raw, sigs, &frozen, &updated)?;
    if slugs != frozen || !slugs_path.exists() {
        write_atomic(&slugs_path, &serde_json::to_vec_pretty(&slugs)?)?;
    }
    let games = file.games.len();
    Ok(built_from_json(serde_json::to_vec(&file)?, games, updated)?)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

/// Download Discord's list into `<data>/detectable/discord.json` (validated first).
pub async fn download(url: &str, data: &Path) -> anyhow::Result<usize> {
    let client = reqwest::Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(90))
        .build()?;
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await?
        .error_for_status()?;
    let body = resp.bytes().await?;
    let apps: Vec<serde_json::Value> = serde_json::from_slice(&body)?;
    if apps.len() < MIN_APPS {
        anyhow::bail!(
            "only {} apps in the download, keeping the old copy",
            apps.len()
        );
    }
    let dir = list_dir(data);
    std::fs::create_dir_all(&dir)?;
    write_atomic(&dir.join("discord.json"), &body)?;
    Ok(apps.len())
}

fn cache_age(data: &Path) -> Option<Duration> {
    let m = std::fs::metadata(list_dir(data).join("discord.json")).ok()?;
    SystemTime::now().duration_since(m.modified().ok()?).ok()
}

/// Background task: load the cache now, re-download when it is a day old.
pub async fn refresh_loop(st: AppState) {
    let data = st.cfg.data_dir.clone();
    let mut loaded = false;
    loop {
        let due = cache_age(&data).is_none_or(|a| a >= REFRESH_EVERY);
        if due {
            match download(&st.cfg.detectable_url, &data).await {
                Ok(n) => {
                    tracing::info!("detectable list: downloaded {n} apps");
                    loaded = false;
                }
                Err(e) => tracing::warn!("detectable list download failed: {e:#}"),
            }
        }
        if !loaded && cache_age(&data).is_some() {
            let sigs = st.sigs.clone();
            let d = data.clone();
            match tokio::task::spawn_blocking(move || build_from_cache(&d, &sigs)).await {
                Ok(Ok(b)) => {
                    tracing::info!(
                        "detectable list: serving {} Windows games ({})",
                        b.games,
                        b.etag
                    );
                    st.games.set(b);
                    loaded = true;
                }
                Ok(Err(e)) => tracing::warn!("detectable list build failed: {e:#}"),
                Err(e) => tracing::warn!("detectable list build panicked: {e}"),
            }
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

fn not_modified(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"))
}

fn respond(headers: &HeaderMap, etag: &str, json: &[u8], gz: Option<&[u8]>) -> Response<Body> {
    let mut b = Response::builder()
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, "public, max-age=3600")
        .header(header::VARY, "Accept-Encoding");
    if not_modified(headers, etag) {
        return b
            .status(StatusCode::NOT_MODIFIED)
            .body(Body::empty())
            .unwrap();
    }
    b = b.header(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    let wants_gzip = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("gzip"));
    match gz {
        Some(gz) if wants_gzip => b
            .header(header::CONTENT_ENCODING, "gzip")
            .body(Body::from(gz.to_vec()))
            .unwrap(),
        _ => b.body(Body::from(json.to_vec())).unwrap(),
    }
}

pub async fn detectable(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response<Body>> {
    let b = st.games.current().ok_or_else(|| {
        AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "Game list not ready yet.",
        )
    })?;
    Ok(respond(&headers, &b.etag, &b.json, Some(&b.gz)))
}

/// The server's override file if valid, else the bundled one.
pub fn overrides_json(data: &Path) -> String {
    let path = data.join("detect-overrides.json");
    if let Ok(s) = std::fs::read_to_string(&path) {
        match Overrides::from_json(&s) {
            Ok(_) => return s,
            Err(e) => tracing::warn!("{} ignored: {e:#}", path.display()),
        }
    }
    overrides::BUILTIN_JSON.to_string()
}

pub async fn overrides(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response<Body>> {
    let s = overrides_json(&st.cfg.data_dir);
    Ok(respond(
        &headers,
        &etag_of(s.as_bytes()),
        s.as_bytes(),
        None,
    ))
}

#[derive(Deserialize)]
pub struct SuggestReq {
    pub exe: String,
    pub name: String,
}

/// Plain `name.exe`: no folders, no control characters.
pub fn valid_exe(exe: &str) -> bool {
    let e = exe.trim();
    !e.is_empty()
        && e.len() <= 120
        && e.to_ascii_lowercase().ends_with(".exe")
        && !e.contains(['/', '\\', ':'])
        && !e.chars().any(char::is_control)
}

pub async fn suggest(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<SuggestReq>,
) -> ApiResult<StatusCode> {
    let ip = crate::client_ip(&headers);
    st.rate_limit(format!("suggest:{ip}"), 30, Duration::from_secs(24 * 3600))?;
    let name = r.name.trim();
    if !valid_exe(&r.exe) {
        return Err(AppError::bad_request(
            "invalid_exe",
            "exe must be a file name like Game.exe",
        ));
    }
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(AppError::bad_request(
            "invalid_name",
            "name must be 1-80 characters",
        ));
    }
    sqlx::query(
        "INSERT INTO game_suggestions(exe, name) VALUES($1, $2)
         ON CONFLICT ((lower(exe)), (lower(name)))
         DO UPDATE SET reports = game_suggestions.reports + 1, last_at = now()",
    )
    .bind(r.exe.trim())
    .bind(name)
    .execute(&st.db)
    .await?;
    tracing::info!(exe = %r.exe.trim(), name, "game suggestion");
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_validation() {
        assert!(valid_exe("OddGame.exe"));
        assert!(valid_exe("My Game.EXE"));
        assert!(!valid_exe("C:\\Games\\x.exe"));
        assert!(!valid_exe("x.bat"));
        assert!(!valid_exe(""));
    }

    #[test]
    fn bundled_list_serves() {
        let b = bundled().expect("bundled snapshot");
        assert!(b.games > 5000);
        assert!(b.gz.len() < b.json.len() / 2);
        assert!(b.etag.starts_with('"'));
    }

    #[test]
    fn cache_build_freezes_slugs() {
        let dir = tempfile::tempdir().unwrap();
        let d = list_dir(dir.path());
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("discord.json"),
            r#"[{"id":"452295596917784577","name":"Star Citizen","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]}]"#,
        )
        .unwrap();
        let sigs = fraglink_core::signatures::SignatureDb::builtin();
        let b = build_from_cache(dir.path(), &sigs).unwrap();
        assert_eq!(b.games, 1);
        let slugs: BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(d.join("slugs.json")).unwrap()).unwrap();
        assert_eq!(slugs["452295596917784577"], "star_citizen");
        // overrides: a broken server file falls back to the bundled rules
        std::fs::write(dir.path().join("detect-overrides.json"), "{oops").unwrap();
        assert_eq!(overrides_json(dir.path()), overrides::BUILTIN_JSON);
    }
}
