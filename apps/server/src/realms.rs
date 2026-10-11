//! WoW realm names (dev.18): `GET /api/v1/games/wow-realms`.
//!
//! The desktop app reads the realm from the game's own files; for installs that only
//! keep per-character folders it checks those folder names against this list. Built
//! once a day from Blizzard's Game Data API (realm index of the retail, Classic and
//! Classic Era namespaces in us/eu/kr/tw) when `BNET_CLIENT_ID` / `BNET_CLIENT_SECRET`
//! are set, else from a public realm table on GitHub (TheGeneticsGuy/WarcraftRealms,
//! itself generated from that API). Cached in `<data dir>/wow-realms.json`, served
//! gzip-able with an ETag.
//!
//! Format: `{"v":1,"updated":"...","source":"...","flavours":{"retail":{"us":[..]},..}}`.

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

pub const PUBLIC_SOURCE: &str =
    "https://raw.githubusercontent.com/TheGeneticsGuy/WarcraftRealms/main/GRM_Realms.lua";
const REFRESH_EVERY: Duration = Duration::from_secs(24 * 3600);
/// Anything smaller is an error page, not a realm list.
const MIN_REALMS: usize = 200;
pub const REGIONS: [&str; 4] = ["us", "eu", "kr", "tw"];
/// (flavour, Blizzard namespace prefix)
pub const FLAVOURS: [(&str, &str); 3] = [
    ("retail", "dynamic"),
    ("classic", "dynamic-classic"),
    ("classic_era", "dynamic-classic1x"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealmFile {
    pub v: u32,
    pub updated: String,
    pub source: String,
    /// flavour -> region -> sorted, de-duplicated names
    pub flavours: BTreeMap<String, BTreeMap<String, Vec<String>>>,
}

impl RealmFile {
    pub fn count(&self) -> usize {
        self.flavours
            .values()
            .flat_map(|r| r.values())
            .map(Vec::len)
            .sum()
    }
}

#[derive(Clone)]
pub struct Built {
    pub etag: String,
    pub json: Arc<Vec<u8>>,
    pub gz: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct RealmCache {
    built: RwLock<Option<Built>>,
}

fn cache_file(data: &Path) -> PathBuf {
    data.join("wow-realms.json")
}

fn build(json: Vec<u8>) -> anyhow::Result<Built> {
    let f: RealmFile = serde_json::from_slice(&json)?;
    anyhow::ensure!(f.count() > 0, "empty realm list");
    Ok(Built {
        etag: crate::games::etag_of(&json),
        gz: Arc::new(crate::games::gzip(&json)?),
        json: Arc::new(json),
    })
}

fn clean_name(s: &str) -> Option<String> {
    let n: String = s.chars().filter(|c| !c.is_control()).take(64).collect();
    let n = n.trim();
    (!n.is_empty()).then(|| n.to_string())
}

/// The public table is Lua: sections `if/elseif GRM.GameVersion() == "RETAIL" | "MOP" |
/// "CLASSICERA"`, each with `Realms.<region>.<locale> = {` followed by one line of quoted
/// names. Every locale's names are kept (WoW names the folders in the client's language).
pub fn parse_grm_lua(text: &str) -> RealmFile {
    let mut flavours: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    let mut flavour: Option<&str> = None;
    let mut region: Option<String> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.contains("GameVersion()") && t.contains("==") {
            flavour = if t.contains("\"RETAIL\"") {
                Some("retail")
            } else if t.contains("\"CLASSICERA\"") {
                Some("classic_era")
            } else if t.contains('"') {
                Some("classic")
            } else {
                None
            };
            region = None;
            continue;
        }
        if let Some(rest) = t.strip_prefix("Realms.") {
            let mut parts = rest.split(|c: char| c == '.' || c.is_whitespace());
            let r = parts.next().unwrap_or("");
            region = (rest.contains('.') && rest.trim_end().ends_with('{') && REGIONS.contains(&r))
                .then(|| r.to_string());
            continue;
        }
        let (Some(f), Some(r)) = (flavour, region.as_ref()) else {
            continue;
        };
        if !t.starts_with('"') {
            continue;
        }
        let names = flavours
            .entry(f.to_string())
            .or_default()
            .entry(r.clone())
            .or_default();
        for (i, part) in t.split('"').enumerate() {
            if i % 2 == 1 {
                if let Some(n) = clean_name(part) {
                    names.push(n);
                }
            }
        }
        region = None;
    }
    for regions in flavours.values_mut() {
        for v in regions.values_mut() {
            v.sort();
            v.dedup();
        }
    }
    RealmFile {
        v: 1,
        updated: chrono::Utc::now().to_rfc3339(),
        source: "github:TheGeneticsGuy/WarcraftRealms".into(),
        flavours,
    }
}

fn http() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("fraglink-server")
        .build()?)
}

async fn from_public(url: &str) -> anyhow::Result<RealmFile> {
    let text = http()?
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    Ok(parse_grm_lua(&text))
}

/// Blizzard's Game Data API: realm index per region and namespace (client credentials).
async fn from_blizzard(id: &str, secret: &str) -> anyhow::Result<RealmFile> {
    let c = http()?;
    let tok: serde_json::Value = c
        .post("https://oauth.battle.net/token")
        .basic_auth(id, Some(secret))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let token = tok["access_token"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("no access_token"))?
        .to_string();
    let mut flavours: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for (flavour, ns) in FLAVOURS {
        for region in REGIONS {
            let url = format!(
                "https://{region}.api.blizzard.com/data/wow/realm/index?namespace={ns}-{region}"
            );
            let r = c.get(&url).bearer_auth(&token).send().await?;
            if !r.status().is_success() {
                tracing::warn!("realm index {flavour}/{region}: HTTP {}", r.status());
                continue;
            }
            let v: serde_json::Value = r.json().await?;
            let mut names = vec![];
            for realm in v["realms"].as_array().into_iter().flatten() {
                // without a locale parameter `name` holds every translation
                match &realm["name"] {
                    serde_json::Value::String(s) => names.extend(clean_name(s)),
                    serde_json::Value::Object(m) => {
                        names.extend(m.values().filter_map(|x| x.as_str()).filter_map(clean_name))
                    }
                    _ => {}
                }
            }
            names.sort();
            names.dedup();
            if !names.is_empty() {
                flavours
                    .entry(flavour.to_string())
                    .or_default()
                    .insert(region.to_string(), names);
            }
        }
    }
    Ok(RealmFile {
        v: 1,
        updated: chrono::Utc::now().to_rfc3339(),
        source: "battle.net".into(),
        flavours,
    })
}

async fn download(data: &Path) -> anyhow::Result<usize> {
    let creds = (
        std::env::var("BNET_CLIENT_ID").unwrap_or_default(),
        std::env::var("BNET_CLIENT_SECRET").unwrap_or_default(),
    );
    let f = if !creds.0.is_empty() && !creds.1.is_empty() {
        match from_blizzard(&creds.0, &creds.1).await {
            Ok(f) if f.count() >= MIN_REALMS => f,
            Ok(f) => anyhow::bail!("battle.net: only {} realms", f.count()),
            Err(e) => {
                tracing::warn!("battle.net realm index failed ({e:#}); using the public table");
                from_public(PUBLIC_SOURCE).await?
            }
        }
    } else {
        from_public(PUBLIC_SOURCE).await?
    };
    let n = f.count();
    anyhow::ensure!(n >= MIN_REALMS, "only {n} realms, keeping the old copy");
    std::fs::create_dir_all(data)?;
    let tmp = cache_file(data).with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec(&f)?)?;
    std::fs::rename(&tmp, cache_file(data))?;
    Ok(n)
}

fn cache_age(data: &Path) -> Option<Duration> {
    let m = std::fs::metadata(cache_file(data)).ok()?;
    SystemTime::now().duration_since(m.modified().ok()?).ok()
}

/// Load the cached copy (if any) into memory.
pub fn load(st: &AppState) -> Option<Built> {
    let b = build(std::fs::read(cache_file(&st.cfg.data_dir)).ok()?).ok()?;
    *st.realms.built.write().unwrap() = Some(b.clone());
    Some(b)
}

/// Background task: re-download when the cache is a day old (checked hourly).
pub async fn refresh_loop(st: AppState) {
    let data = st.cfg.data_dir.clone();
    loop {
        if cache_age(&data).is_none_or(|a| a >= REFRESH_EVERY) {
            match download(&data).await {
                Ok(n) => {
                    tracing::info!("wow realms: {n} names");
                    load(&st);
                }
                Err(e) => tracing::warn!("wow realms download failed: {e:#}"),
            }
        }
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}

pub async fn serve(State(st): State<AppState>, headers: HeaderMap) -> ApiResult<Response<Body>> {
    let cur = st.realms.built.read().unwrap().clone();
    let b = cur.or_else(|| load(&st)).ok_or_else(|| {
        AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "Realm list not ready yet.",
        )
    })?;
    Ok(crate::games::respond(
        &headers,
        &b.etag,
        &b.json,
        Some(&b.gz),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_public_lua_table() {
        let lua = r#"
local InitializeRealms = function()
    if GRM.GameVersion() == "RETAIL" then
        Realms.us = {}
        Realms.us.enUS = {
            "Aegwynn", "Barthilas", "Frostmourne", "Aman'Thul"
        }
        Realms.eu.deDE = {
            "Blackhand", "Die Aldor"
        }
    elseif GRM.GameVersion() == "MOP" then
        Realms.us.enUS = {
            "Mankrik", "Pagle"
        }
    elseif GRM.GameVersion() == "CLASSICERA" then
        Realms.eu.enUS = {
            "Firemaw", "Firemaw"
        }
    end
end
GRM.GetRealmNames = function()
    return Realms[region][GetLocale()]
end
"#;
        let f = parse_grm_lua(lua);
        assert_eq!(
            f.flavours["retail"]["us"],
            ["Aegwynn", "Aman'Thul", "Barthilas", "Frostmourne"]
        );
        assert_eq!(f.flavours["retail"]["eu"], ["Blackhand", "Die Aldor"]);
        assert_eq!(f.flavours["classic"]["us"], ["Mankrik", "Pagle"]);
        assert_eq!(f.flavours["classic_era"]["eu"], ["Firemaw"]);
        assert_eq!(f.count(), 9);
    }

    /// `GRM_LUA=/path/GRM_Realms.lua cargo test -p fraglink-server realms -- --ignored`
    #[test]
    #[ignore]
    fn parses_a_real_download() {
        let p = std::env::var("GRM_LUA").unwrap();
        let f = parse_grm_lua(&std::fs::read_to_string(p).unwrap());
        for (fl, r) in &f.flavours {
            for (reg, v) in r {
                println!("{fl}/{reg}: {}", v.len());
            }
        }
        assert!(f.count() > MIN_REALMS);
        assert!(f.flavours["retail"]["us"]
            .iter()
            .any(|n| n == "Frostmourne"));
    }
}
