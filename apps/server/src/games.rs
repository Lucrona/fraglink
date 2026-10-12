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
    /// Fraglink id (slug) -> Discord's name, for server-side display names.
    pub names: Arc<std::collections::HashMap<String, String>>,
    /// Where each game's official icon comes from (built from the raw list).
    pub icons: Arc<IconIndex>,
}

/// Official icon sources per game, from the detectable list's `icon_hash` and its
/// Steam store links (see `crate::icons`).
#[derive(Default, Debug)]
pub struct IconIndex {
    /// slug -> (application id, icon hash, Steam app id)
    pub by_slug: std::collections::HashMap<String, IconSrc>,
    /// Steam app id -> slug
    pub by_steam: std::collections::HashMap<u32, String>,
    /// normalised name -> slug
    pub by_name: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct IconSrc {
    pub app_id: String,
    pub icon_hash: Option<String>,
    pub steam_appid: Option<u32>,
}

/// Lower case, letters and digits only ("World of Warcraft: Forever" -> "worldofwarcraftforever").
pub fn norm_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Deserialize)]
struct RawIconApp {
    id: String,
    #[serde(default)]
    icon_hash: Option<String>,
    #[serde(default)]
    third_party_skus: Option<Vec<RawSku>>,
}

#[derive(Deserialize)]
struct RawSku {
    #[serde(default)]
    distributor: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

fn icon_index(compact: &[u8], raw: Option<&[u8]>) -> IconIndex {
    let mut idx = IconIndex::default();
    let Some(raw) = raw else { return idx };
    let Ok(apps) = serde_json::from_slice::<Vec<RawIconApp>>(raw) else {
        return idx;
    };
    let by_id: std::collections::HashMap<String, RawIconApp> =
        apps.into_iter().map(|a| (a.id.clone(), a)).collect();
    let Ok(db) = DetectableDb::from_json(compact) else {
        return idx;
    };
    for g in db.games() {
        let Some(a) = by_id.get(&g.id) else { continue };
        let hash = a.icon_hash.clone().filter(|h| {
            h.len() <= 64
                && !h.is_empty()
                && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        let steam = a.third_party_skus.iter().flatten().find_map(|k| {
            (k.distributor.as_deref() == Some("steam"))
                .then(|| k.id.as_deref()?.parse::<u32>().ok())
                .flatten()
        });
        if hash.is_none() && steam.is_none() {
            continue;
        }
        if let Some(appid) = steam {
            idx.by_steam.entry(appid).or_insert_with(|| g.slug.clone());
        }
        idx.by_name
            .entry(norm_name(&g.name))
            .or_insert_with(|| g.slug.clone());
        idx.by_slug.insert(
            g.slug.clone(),
            IconSrc {
                app_id: g.id.clone(),
                icon_hash: hash,
                steam_appid: steam,
            },
        );
    }
    idx
}

#[derive(Default)]
pub struct GameLists {
    built: RwLock<Option<Built>>,
    /// Override rule names (id -> name), re-read from disk at most every 5 minutes.
    override_names: RwLock<
        Option<(
            std::time::Instant,
            Arc<std::collections::HashMap<String, String>>,
        )>,
    >,
}

pub(crate) fn etag_of(b: &[u8]) -> String {
    let h = Sha256::digest(b);
    let hex: String = h[..12].iter().map(|x| format!("{x:02x}")).collect();
    format!("\"{hex}\"")
}

pub(crate) fn gzip(b: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(b)?;
    gz.finish()
}

fn built_from_json(
    json: Vec<u8>,
    games: usize,
    updated: String,
    raw: Option<&[u8]>,
) -> std::io::Result<Built> {
    let icons = icon_index(&json, raw);
    // DetectableDb drops public test builds, so they never get a display name either.
    let names = DetectableDb::from_json(&json)
        .map(|db| {
            db.games()
                .iter()
                .map(|g| (g.slug.clone(), g.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    Ok(Built {
        etag: etag_of(&json),
        gz: Arc::new(gzip(&json)?),
        json: Arc::new(json),
        games,
        updated,
        names: Arc::new(names),
        icons: Arc::new(icons),
    })
}

/// The snapshot compiled into this binary.
fn bundled() -> Option<Built> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(detectable::BUNDLED_GZ)
        .read_to_end(&mut json)
        .ok()?;
    let db = DetectableDb::from_json(&json).ok()?;
    built_from_json(json, db.len(), db.updated.clone(), None).ok()
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

    pub fn install(&self, b: Built) {
        *self.built.write().unwrap() = Some(b);
    }

    /// Discord's name for a Fraglink game id, if it is on the detectable list.
    pub fn detectable_name(&self, id: &str) -> Option<String> {
        self.current().and_then(|b| b.names.get(id).cloned())
    }

    /// Official icon index of the current list (empty for the bundled snapshot).
    pub fn icon_index(&self) -> Arc<IconIndex> {
        self.current().map(|b| b.icons).unwrap_or_default()
    }

    /// Name of an override rule (`star_citizen`), from the server's file or the bundled one.
    pub fn override_name(&self, data: &Path, id: &str) -> Option<String> {
        let fresh = |t: &std::time::Instant| t.elapsed() < Duration::from_secs(300);
        if let Some((t, m)) = self.override_names.read().unwrap().as_ref() {
            if fresh(t) {
                return m.get(id).cloned();
            }
        }
        let m: std::collections::HashMap<String, String> =
            Overrides::from_json(&overrides_json(data))
                .map(|o| {
                    o.rules()
                        .iter()
                        .map(|r| (r.id.clone(), r.name.clone()))
                        .collect()
                })
                .unwrap_or_default();
        let m = Arc::new(m);
        *self.override_names.write().unwrap() = Some((std::time::Instant::now(), m.clone()));
        m.get(id).cloned()
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
    Ok(built_from_json(
        serde_json::to_vec(&file)?,
        games,
        updated,
        Some(&raw),
    )?)
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
                    st.games.install(b);
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

pub(crate) fn respond(
    headers: &HeaderMap,
    etag: &str,
    json: &[u8],
    gz: Option<&[u8]>,
) -> Response<Body> {
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

/// The override file plus the games an admin approved from "Track this as a game?"
/// answers (`exe:<name>.exe` -> approved name), so every client detects them.
pub async fn overrides_with_approved(st: &AppState) -> ApiResult<String> {
    let s = overrides_json(&st.cfg.data_dir);
    let approved: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM games WHERE verified AND id LIKE 'exe:%' ORDER BY id")
            .fetch_all(&st.db)
            .await?;
    let ov = crate::game_overrides::current(st).await?;
    let extra: Vec<&crate::game_overrides::GameOverride> =
        ov.values().filter(|o| !o.extra_exes.is_empty()).collect();
    let renamed: Vec<(&String, &String)> = ov
        .values()
        .filter(|o| !o.hidden)
        .filter_map(|o| o.name.as_ref().map(|n| (&o.game_id, n)))
        .collect();
    let detached = crate::retire::detached_exes(st).await?;
    let merged: Vec<(String, String)> = sqlx::query_as(
        "SELECT game_id, successor_id FROM game_retirements WHERE merged ORDER BY game_id",
    )
    .fetch_all(&st.db)
    .await?;
    if approved.is_empty()
        && extra.is_empty()
        && renamed.is_empty()
        && detached.is_empty()
        && merged.is_empty()
    {
        return Ok(s);
    }
    let mut f: overrides::OverrideFile = match serde_json::from_str(&s) {
        Ok(f) => f,
        Err(_) => return Ok(s),
    };
    for (id, name) in approved {
        let exe = id.trim_start_matches("exe:").to_string();
        if !valid_exe(&exe) || f.rules.iter().any(|r| r.id == id) {
            continue;
        }
        f.rules.push(overrides::Rule {
            id,
            name,
            exes: vec![exe],
            paths: vec![],
            channel: None,
            note: Some("approved game".into()),
        });
    }
    // after the approved programs, so a staff rename of an approved `exe:` game wins too
    apply_renames(st, &mut f, renamed);
    // extra exe names added by staff: onto the game's own rule, or a rule of their own
    let mut extra = extra;
    extra.sort_by(|a, b| a.game_id.cmp(&b.game_id));
    for o in extra {
        let exes: Vec<String> = o
            .extra_exes
            .iter()
            .filter(|e| valid_exe(e))
            .cloned()
            .collect();
        if let Some(r) = f.rules.iter_mut().find(|r| r.id == o.game_id) {
            for e in exes {
                if !r.exes.iter().any(|x| x.eq_ignore_ascii_case(&e)) {
                    r.exes.push(e);
                }
            }
            continue;
        }
        let name = o
            .name
            .clone()
            .or_else(|| crate::names::list_name(st, &o.game_id))
            .unwrap_or_else(|| o.game_id.clone());
        f.rules.push(overrides::Rule {
            id: o.game_id.clone(),
            name,
            exes,
            paths: vec![],
            channel: None,
            note: Some("extra exe names".into()),
        });
    }
    // partly retired games: the detached programs get rules of their own (new entries)
    for (game, exe) in &detached {
        for r in f.rules.iter_mut().filter(|r| &r.id == game) {
            r.exes.retain(|x| !x.eq_ignore_ascii_case(exe));
        }
        let id = format!("exe:{exe}");
        if valid_exe(exe) && !f.rules.iter().any(|r| r.id == id) {
            f.rules.insert(
                0,
                overrides::Rule {
                    id,
                    name: crate::names::UNLISTED_TITLE.into(),
                    exes: vec![exe.clone()],
                    paths: vec![],
                    channel: None,
                    note: Some(format!("detached from {game}")),
                },
            );
        }
    }
    // merged games: the old entry's programs are detected as the target game
    for (old, target) in &merged {
        let name = ov
            .get(target)
            .and_then(|o| o.name.clone())
            .or_else(|| crate::names::list_name(st, target))
            .unwrap_or_else(|| target.clone());
        let mut found = false;
        for r in f.rules.iter_mut().filter(|r| &r.id == old) {
            r.id = target.clone();
            r.name = name.clone();
            found = true;
        }
        if found {
            continue;
        }
        for (exes, paths) in rename_patterns(st, old) {
            if exes.is_empty() {
                continue;
            }
            f.rules.push(overrides::Rule {
                id: target.clone(),
                name: name.clone(),
                exes,
                paths,
                channel: None,
                note: Some(format!("merged from {old}")),
            });
        }
    }
    Ok(serde_json::to_string_pretty(&f).unwrap_or(s))
}

/// Staff renames reach every client (dev.18): an existing rule takes the new name, and a game
/// known from the signatures or the detectable list gets a rule of its own with the same exe
/// names, so detection reports the new name. Rules match before the signatures, and the id
/// is unchanged, so this only renames; older apps (dev.12+) pick it up with no update.
fn apply_renames(
    st: &AppState,
    f: &mut overrides::OverrideFile,
    mut renamed: Vec<(&String, &String)>,
) {
    renamed.sort();
    for (id, name) in renamed {
        let mut found = false;
        for r in f.rules.iter_mut().filter(|r| &r.id == id) {
            r.name = name.clone();
            found = true;
        }
        if found {
            continue;
        }
        for (exes, paths) in rename_patterns(st, id) {
            if exes.is_empty() {
                continue;
            }
            f.rules.push(overrides::Rule {
                id: id.clone(),
                name: name.clone(),
                exes,
                paths,
                channel: None,
                note: Some("renamed by staff".into()),
            });
        }
    }
}

/// `(exe names, path fragments)` that identify game `id`, from the signatures, else the
/// detectable list (`folder/game.exe` patterns become exe `game.exe` + path `/folder/`).
pub(crate) fn rename_patterns(st: &AppState, id: &str) -> Vec<(Vec<String>, Vec<String>)> {
    if let Some(g) = st.sigs.get(id) {
        let low = |v: &[String]| v.iter().map(|e| e.to_lowercase()).collect::<Vec<_>>();
        let hints = low(&g.path_hints);
        let mut out = vec![(low(&g.exes), hints.clone())];
        if !g.path_exes.is_empty() && !hints.is_empty() {
            out.push((low(&g.path_exes), hints));
        }
        return out;
    }
    let Some(g) = detectable_game(st, id) else {
        return vec![];
    };
    let mut plain = vec![];
    let mut out = vec![];
    for e in g.exes {
        match e.rsplit_once('/') {
            Some((dir, exe)) if !dir.is_empty() => out.push((
                vec![exe.to_string()],
                vec![format!("/{}/", dir.trim_matches('/'))],
            )),
            _ => plain.push(e),
        }
    }
    out.insert(0, (plain, vec![]));
    out
}

/// A game of the current detectable list by Fraglink id (the parsed list is kept per ETag).
fn detectable_game(st: &AppState, slug: &str) -> Option<detectable::DGame> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Option<(String, Arc<DetectableDb>)>>> = OnceLock::new();
    let b = st.games.current()?;
    let mut c = CACHE.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if c.as_ref().map(|(t, _)| t != &b.etag).unwrap_or(true) {
        *c = Some((
            b.etag.clone(),
            Arc::new(DetectableDb::from_json(&b.json).ok()?),
        ));
    }
    c.as_ref()?.1.by_slug(slug).cloned()
}

pub async fn overrides(
    State(st): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<Response<Body>> {
    let s = overrides_with_approved(&st).await?;
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
            r#"[{"id":"452295596917784577","name":"Star Citizen","icon_hash":"7a55868f259fca03ba55756e41983e44","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]},
               {"id":"1234821572246896730","name":"Gray Zone Warfare","icon_hash":"16c256e96f8565fcc14ba843aa245f2b","third_party_skus":[{"distributor":"steam","id":"2479810"}],"executables":[{"name":"gzwclientsteam-win64-shipping.exe","os":"win32","is_launcher":false}]}]"#,
        )
        .unwrap();
        let sigs = fraglink_core::signatures::SignatureDb::builtin();
        let b = build_from_cache(dir.path(), &sigs).unwrap();
        assert_eq!(b.games, 2);
        // official icon sources come along
        let sc = &b.icons.by_slug["star_citizen"];
        assert_eq!(
            sc.icon_hash.as_deref(),
            Some("7a55868f259fca03ba55756e41983e44")
        );
        assert_eq!(
            b.icons.by_steam.get(&2479810).map(String::as_str),
            Some("gray_zone_warfare")
        );
        assert_eq!(
            b.icons.by_name.get("grayzonewarfare").map(String::as_str),
            Some("gray_zone_warfare")
        );
        let slugs: BTreeMap<String, String> =
            serde_json::from_slice(&std::fs::read(d.join("slugs.json")).unwrap()).unwrap();
        assert_eq!(slugs["452295596917784577"], "star_citizen");
        // overrides: a broken server file falls back to the bundled rules
        std::fs::write(dir.path().join("detect-overrides.json"), "{oops").unwrap();
        assert_eq!(overrides_json(dir.path()), overrides::BUILTIN_JSON);
    }
}
