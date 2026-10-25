//! The big game list: Discord's public "detectable applications" list, trimmed
//! to Windows game executables (launchers and other OSes dropped).
//!
//! The Fraglink server downloads Discord's list once a day and serves the
//! compact form built here (`GET /api/v1/games/detectable`). The desktop app
//! caches the latest copy and falls back to the snapshot bundled at build time
//! (`packages/signatures/detectable.json.gz`) plus the hand-written signatures.
//!
//! Matching: the process exe name and full path are lower-cased (path with
//! forward slashes). A pattern without `/` matches the exe name; a pattern with
//! `/` (e.g. `_retail_/wow.exe`) matches when the path ends with it. The longest
//! (most specific) matching pattern wins.

use crate::signatures::{normalize_path, SignatureDb};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;

/// Compressed snapshot bundled into the app (refresh with `scripts/update-detectable.sh`).
pub const BUNDLED_GZ: &[u8] = include_bytes!("../../../packages/signatures/detectable.json.gz");

pub const SCHEMA: u32 = 1;

/// One game of the compact list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DGame {
    /// Discord application id.
    #[serde(rename = "i")]
    pub id: String,
    /// Stable Fraglink game id: a signature id when the game is one of ours
    /// (`wow`, `cs2`), else a slug of the name frozen on first sight (`star_citizen`).
    #[serde(rename = "s")]
    pub slug: String,
    #[serde(rename = "n")]
    pub name: String,
    /// Lower-case exe names / path suffixes (Windows, non-launcher).
    #[serde(rename = "e")]
    pub exes: Vec<String>,
    /// Steam app ids Discord lists for this game (`third_party_skus`), so a Steam
    /// install of it keeps this id instead of becoming `steam:<appid>`.
    #[serde(rename = "st", default, skip_serializing_if = "Vec::is_empty")]
    pub steam: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DetectableFile {
    pub v: u32,
    /// When the server built it (RFC 3339), informational.
    #[serde(default)]
    pub updated: String,
    pub games: Vec<DGame>,
}

/// Exe names too generic to identify a game on their own: only a pattern with a
/// folder part (`garrysmod/hl2.exe`) may match them.
const GENERIC_EXES: &[&str] = &[
    "game.exe",
    "launcher.exe",
    "client.exe",
    "start.exe",
    "main.exe",
    "app.exe",
    "play.exe",
    "run.exe",
    "javaw.exe",
    "java.exe",
    "python.exe",
    "pythonw.exe",
    "nw.exe",
    "electron.exe",
    "hl2.exe",
    "unity.exe",
    "godot.exe",
    "love.exe",
    "dosbox.exe",
    "win64.exe",
    "shipping.exe",
    "update.exe",
    "setup.exe",
];

#[derive(Debug, Clone, Default)]
pub struct DetectableDb {
    pub updated: String,
    games: Vec<DGame>,
    /// exe file name -> (game index, pattern)
    by_exe: HashMap<String, Vec<(u32, String)>>,
    /// Steam app id -> game index
    by_steam: HashMap<u32, u32>,
}

/// The match for one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DMatch<'a> {
    pub game: &'a DGame,
    /// The pattern that matched (longer = more specific).
    pub pattern: &'a str,
}

/// Public test / PTR builds on Discord's list ("World of Warcraft Public Test") are
/// not tracked: their entries are dropped when the list is built and when it is loaded.
pub fn is_test_build(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("public test")
        || n.split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| w == "ptr")
}

impl DetectableDb {
    pub fn from_file(mut f: DetectableFile) -> Self {
        f.games.retain(|g| !is_test_build(&g.name));
        let mut by_exe: HashMap<String, Vec<(u32, String)>> = HashMap::new();
        let mut by_steam: HashMap<u32, u32> = HashMap::new();
        for (i, g) in f.games.iter().enumerate() {
            for a in &g.steam {
                by_steam.entry(*a).or_insert(i as u32);
            }
            for e in &g.exes {
                let pat = e.trim().to_lowercase().replace('\\', "/");
                let pat = pat.trim_start_matches('/').to_string();
                let base = pat.rsplit('/').next().unwrap_or("").to_string();
                if base.is_empty() {
                    continue;
                }
                by_exe.entry(base).or_default().push((i as u32, pat));
            }
        }
        DetectableDb {
            updated: f.updated,
            games: f.games,
            by_exe,
            by_steam,
        }
    }

    pub fn from_json(json: &[u8]) -> anyhow::Result<Self> {
        let f: DetectableFile = serde_json::from_slice(json)?;
        if f.v != SCHEMA {
            anyhow::bail!("detectable list schema {} (want {SCHEMA})", f.v);
        }
        Ok(Self::from_file(f))
    }

    /// JSON, or gzip-compressed JSON (sniffed by the gzip magic bytes).
    pub fn from_bytes(b: &[u8]) -> anyhow::Result<Self> {
        if b.starts_with(&[0x1f, 0x8b]) {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(b).read_to_end(&mut out)?;
            Self::from_json(&out)
        } else {
            Self::from_json(b)
        }
    }

    /// The snapshot bundled into the binary (empty if it is missing or broken).
    pub fn bundled() -> Self {
        Self::from_bytes(BUNDLED_GZ).unwrap_or_else(|e| {
            log::warn!("bundled detectable list unusable: {e:#}");
            Self::default()
        })
    }

    /// [`bundled`](Self::bundled), parsed once per process.
    pub fn bundled_shared() -> std::sync::Arc<Self> {
        static B: std::sync::OnceLock<std::sync::Arc<DetectableDb>> = std::sync::OnceLock::new();
        B.get_or_init(|| std::sync::Arc::new(Self::bundled()))
            .clone()
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    pub fn games(&self) -> &[DGame] {
        &self.games
    }

    pub fn by_slug(&self, slug: &str) -> Option<&DGame> {
        self.games.iter().find(|g| g.slug == slug)
    }

    /// The entry Discord links to this Steam app id, if any.
    pub fn by_steam_appid(&self, appid: u32) -> Option<&DGame> {
        self.by_steam.get(&appid).map(|i| &self.games[*i as usize])
    }

    /// Most specific entry matching this process, if any.
    pub fn match_process(&self, exe: &str, path: Option<&str>) -> Option<DMatch<'_>> {
        let exe_l = exe.to_lowercase();
        let cands = self.by_exe.get(&exe_l)?;
        let path_n = path.map(normalize_path);
        let generic = GENERIC_EXES.contains(&exe_l.as_str());
        let mut best: Option<(u32, &str)> = None;
        for (gi, pat) in cands {
            let ok = if pat.contains('/') {
                match &path_n {
                    Some(p) => p.ends_with(&format!("/{pat}")) || p == pat,
                    None => false,
                }
            } else {
                !generic
            };
            if ok && best.is_none_or(|(_, b)| pat.len() > b.len()) {
                best = Some((*gi, pat.as_str()));
            }
        }
        best.map(|(gi, pat)| DMatch {
            game: &self.games[gi as usize],
            pattern: pat,
        })
    }
}

// ------------------------------------------------------------------ building (server side)

/// Raw Discord entry (only the fields we use).
#[derive(Debug, Deserialize)]
struct RawApp {
    id: String,
    name: String,
    #[serde(default)]
    executables: Option<Vec<RawExe>>,
    #[serde(default)]
    third_party_skus: Option<Vec<RawSku>>,
}

#[derive(Debug, Deserialize)]
struct RawSku {
    #[serde(default)]
    distributor: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RawExe {
    name: String,
    #[serde(default)]
    os: String,
    #[serde(default)]
    is_launcher: bool,
}

/// Discord id -> Fraglink id of the bundled snapshot. Builders start from it so a
/// fresh server (or a snapshot refresh) hands out the same ids as the app has.
pub fn bundled_slugs() -> BTreeMap<String, String> {
    DetectableDb::bundled_shared()
        .games()
        .iter()
        .map(|g| (g.id.clone(), g.slug.clone()))
        .collect()
}

/// `Star Citizen` -> `star_citizen`, `Tom Clancy's Rainbow Six® Siege` -> `tom_clancy_s_rainbow_six_siege`.
pub fn name_slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.is_empty() && !s.ends_with('_') {
            s.push('_');
        }
    }
    let s: String = s.trim_end_matches('_').chars().take(48).collect();
    s.trim_end_matches('_').to_string()
}

/// Signature id for a Discord entry when one of its exes is one of our games.
/// Several hits (WoW's `_retail_` / `_ptr_` patterns): the one named like the
/// entry wins, else the first.
fn sig_for(sigs: &SignatureDb, name: &str, exes: &[String]) -> Option<String> {
    let mut hits: Vec<&crate::signatures::GameSig> = Vec::new();
    for pat in exes {
        let base = pat.rsplit('/').next().unwrap_or(pat);
        let fake = if pat.contains('/') {
            format!("c:/games/{pat}")
        } else {
            format!("c:/games/x/{pat}")
        };
        if let Some(g) = sigs.match_process(base, Some(&fake)) {
            if !hits.iter().any(|h| h.id == g.id) {
                hits.push(g);
            }
        }
    }
    hits.iter()
        .find(|g| g.name.eq_ignore_ascii_case(name))
        .or(hits.first())
        .map(|g| g.id.clone())
}

/// Build the compact Windows list from Discord's raw JSON.
///
/// `frozen` maps Discord ids to slugs handed out earlier, so a renamed game keeps
/// its id; the returned map is `frozen` plus any new slugs (persist it).
pub fn build_from_discord(
    raw: &[u8],
    sigs: &SignatureDb,
    frozen: &BTreeMap<String, String>,
    updated: &str,
) -> anyhow::Result<(DetectableFile, BTreeMap<String, String>)> {
    let apps: Vec<RawApp> = serde_json::from_slice(raw)?;
    let mut slugs = frozen.clone();
    let mut taken: HashSet<String> = slugs.values().cloned().collect();
    let sig_ids: HashSet<&str> = sigs.games().iter().map(|g| g.id.as_str()).collect();
    let mut games = Vec::new();
    // stable order: Discord ids are snowflakes, older first
    let mut apps = apps;
    apps.sort_by(|a, b| (a.id.len(), a.id.as_str()).cmp(&(b.id.len(), b.id.as_str())));
    for a in apps {
        let mut exes: Vec<String> = Vec::new();
        for x in a.executables.unwrap_or_default() {
            if x.is_launcher || x.os != "win32" {
                continue;
            }
            let n = x.name.trim().to_lowercase().replace('\\', "/");
            // ">name" entries need command-line arguments we never read
            if n.is_empty() || n.starts_with('>') || n.len() > 200 {
                continue;
            }
            if !exes.contains(&n) {
                exes.push(n);
            }
        }
        let name = a.name.trim().to_string();
        if exes.is_empty() || name.is_empty() || a.id.is_empty() || is_test_build(&name) {
            continue;
        }
        let slug = if let Some(s) = slugs.get(&a.id) {
            s.clone()
        } else {
            let s = match sig_for(sigs, &name, &exes) {
                Some(id) => id,
                None => {
                    let base = match name_slug(&name) {
                        s if s.is_empty() => format!("app_{}", a.id),
                        s => s,
                    };
                    let mut s = base.clone();
                    let mut n = 2;
                    while taken.contains(&s) || sig_ids.contains(s.as_str()) {
                        s = format!("{base}_{n}");
                        n += 1;
                    }
                    s
                }
            };
            taken.insert(s.clone());
            slugs.insert(a.id.clone(), s.clone());
            s
        };
        let mut steam: Vec<u32> = Vec::new();
        for k in a.third_party_skus.unwrap_or_default() {
            if k.distributor != "steam" {
                continue;
            }
            let n = match &k.id {
                Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
                Some(serde_json::Value::Number(n)) => {
                    n.as_u64().and_then(|n| u32::try_from(n).ok())
                }
                _ => None,
            };
            if let Some(n) = n.filter(|n| *n > 0) {
                if !steam.contains(&n) {
                    steam.push(n);
                }
            }
        }
        games.push(DGame {
            id: a.id,
            slug,
            name,
            exes,
            steam,
        });
    }
    Ok((
        DetectableFile {
            v: SCHEMA,
            updated: updated.to_string(),
            games,
        },
        slugs,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_test_builds_are_skipped() {
        use super::*;
        assert!(is_test_build("World of Warcraft Public Test"));
        assert!(is_test_build("Some Game (PTR)"));
        assert!(!is_test_build("World of Warcraft"));
        assert!(!is_test_build("Captain Ptron"));
        let db = DetectableDb::bundled();
        assert!(
            db.by_slug("wow_ptr").is_none(),
            "Public Test entry dropped on load"
        );
        assert!(db.by_slug("wow").is_some() && db.by_slug("wow_classic").is_some());
        let raw = br#"[{"id":"1","name":"World of Warcraft Public Test","executables":[{"name":"wowt-64.exe","os":"win32"}]},
                       {"id":"2","name":"Brawl","executables":[{"name":"brawl.exe","os":"win32"}]}]"#;
        let (f, _) =
            build_from_discord(raw, &SignatureDb::builtin(), &BTreeMap::new(), "x").unwrap();
        assert_eq!(f.games.len(), 1);
        assert_eq!(f.games[0].name, "Brawl");
    }

    use super::*;

    const RAW: &str = r#"[
      {"id":"356875762940379136","name":"World of Warcraft","executables":[
        {"name":"_ptr_/wow.exe","os":"win32","is_launcher":false},
        {"name":"_retail_/wow.exe","os":"win32","is_launcher":false},
        {"name":"_beta_/wow.exe","os":"win32","is_launcher":false},
        {"name":"world of warcraft.app","os":"darwin","is_launcher":false}]},
      {"id":"426525131276550144","name":"World of Warcraft Public Test","executables":[{"name":"wowt-64.exe","os":"win32","is_launcher":false}]},
      {"id":"452295596917784577","name":"Star Citizen","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]},
      {"id":"615752773690064908","name":"World of Warcraft Classic","executables":[{"name":"wowclassic.exe","os":"win32","is_launcher":false}]},
      {"id":"1553234742529364018","name":"World of Warcraft: Forever","executables":[{"name":"wowb.exe","os":"win32","is_launcher":false}]},
      {"id":"1","name":"Some Launcher","executables":[{"name":"somelauncher.exe","os":"win32","is_launcher":true}]},
      {"id":"2","name":"Mac Only","executables":[{"name":"maconly.app","os":"darwin","is_launcher":false}]},
      {"id":"3","name":"No Exes","executables":[]},
      {"id":"4","name":"Garry's Mod","executables":[{"name":"garrysmod/hl2.exe","os":"win32","is_launcher":false},{"name":">hl2.exe","os":"win32","is_launcher":false}]},
      {"id":"5","name":"Generic Game","executables":[{"name":"game.exe","os":"win32","is_launcher":false}]},
      {"id":"6","name":"Star Citizen","executables":[{"name":"other.exe","os":"win32","is_launcher":false}]},
      {"id":"7","name":"Deep Folder","executables":[{"name":"deep/win64/thing.exe","os":"win32","is_launcher":false},{"name":"thing.exe","os":"win32","is_launcher":false}]}
    ]"#;

    fn db() -> DetectableDb {
        let (f, _) = build_from_discord(
            RAW.as_bytes(),
            &SignatureDb::builtin(),
            &BTreeMap::new(),
            "t",
        )
        .unwrap();
        DetectableDb::from_file(f)
    }

    #[test]
    fn build_keeps_windows_game_exes_only() {
        let (f, slugs) = build_from_discord(
            RAW.as_bytes(),
            &SignatureDb::builtin(),
            &BTreeMap::new(),
            "t",
        )
        .unwrap();
        let names: Vec<&str> = f.games.iter().map(|g| g.name.as_str()).collect();
        assert!(!names.contains(&"Some Launcher"));
        assert!(!names.contains(&"Mac Only"));
        assert!(!names.contains(&"No Exes"));
        let wow = f
            .games
            .iter()
            .find(|g| g.id == "356875762940379136")
            .unwrap();
        assert_eq!(
            wow.exes,
            vec!["_ptr_/wow.exe", "_retail_/wow.exe", "_beta_/wow.exe"]
        );
        // mapped onto our existing signature id, so old stats stay attached
        assert_eq!(wow.slug, "wow");
        let gm = f.games.iter().find(|g| g.id == "4").unwrap();
        assert_eq!(gm.exes, vec!["garrysmod/hl2.exe"], "'>' entries dropped");
        // slugs are unique and frozen
        let sc = f
            .games
            .iter()
            .find(|g| g.id == "452295596917784577")
            .unwrap();
        assert_eq!(sc.slug, "star_citizen");
        let dup = f.games.iter().find(|g| g.id == "6").unwrap();
        assert_eq!(dup.slug, "star_citizen_2");
        let mut renamed = slugs.clone();
        renamed.insert("452295596917784577".into(), "star_citizen".into());
        let raw2 = RAW.replace(
            "\"name\":\"Star Citizen\",\"executables\":[{\"name\":\"starcitizen.exe\"",
            "\"name\":\"Star Citizen Alpha\",\"executables\":[{\"name\":\"starcitizen.exe\"",
        );
        let (f2, _) =
            build_from_discord(raw2.as_bytes(), &SignatureDb::builtin(), &renamed, "t").unwrap();
        let sc2 = f2
            .games
            .iter()
            .find(|g| g.id == "452295596917784577")
            .unwrap();
        assert_eq!(
            (sc2.name.as_str(), sc2.slug.as_str()),
            ("Star Citizen Alpha", "star_citizen")
        );
    }

    #[test]
    fn matches_name_and_path_suffix_most_specific_first() {
        let d = db();
        let m = |exe: &str, path: &str| {
            d.match_process(exe, Some(path))
                .map(|m| m.game.name.clone())
        };
        assert_eq!(
            m(
                "Wow.exe",
                r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe"
            )
            .as_deref(),
            Some("World of Warcraft")
        );
        assert_eq!(
            m(
                "WowB.exe",
                r"E:\Battle.net Games\World of Warcraft\_classic_beta_\WowB.exe"
            )
            .as_deref(),
            Some("World of Warcraft: Forever")
        );
        assert_eq!(
            m(
                "StarCitizen.exe",
                r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Bin64\StarCitizen.exe"
            )
            .as_deref(),
            Some("Star Citizen")
        );
        // folder pattern needs the folder
        assert_eq!(m("Wow.exe", r"C:\Games\Wow.exe"), None);
        assert!(d.match_process("Wow.exe", None).is_none());
        // generic names never match by name alone
        assert_eq!(m("game.exe", r"C:\Games\Whatever\game.exe"), None);
        assert_eq!(
            m(
                "hl2.exe",
                r"C:\Steam\steamapps\common\GarrysMod\garrysmod\hl2.exe"
            )
            .as_deref(),
            Some("Garry's Mod")
        );
        // the longer pattern is reported
        let hit = d
            .match_process("thing.exe", Some(r"D:\Deep\Win64\thing.exe"))
            .unwrap();
        assert_eq!(hit.pattern, "deep/win64/thing.exe");
        let plain = d
            .match_process("thing.exe", Some(r"D:\Elsewhere\thing.exe"))
            .unwrap();
        assert_eq!(plain.pattern, "thing.exe");
        // launchers are not in the list
        assert_eq!(m("SomeLauncher.exe", r"C:\L\SomeLauncher.exe"), None);
    }

    #[test]
    fn gzip_round_trip_and_bundled_snapshot() {
        use std::io::Write;
        let (f, _) = build_from_discord(
            RAW.as_bytes(),
            &SignatureDb::builtin(),
            &BTreeMap::new(),
            "t",
        )
        .unwrap();
        let json = serde_json::to_vec(&f).unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&json).unwrap();
        let gz = gz.finish().unwrap();
        assert_eq!(DetectableDb::from_bytes(&gz).unwrap().len(), f.games.len());
        assert_eq!(
            DetectableDb::from_bytes(&json).unwrap().len(),
            f.games.len()
        );
        assert!(DetectableDb::from_bytes(b"{\"v\":99,\"games\":[]}").is_err());

        let b = DetectableDb::bundled();
        assert!(b.len() > 5000, "bundled snapshot has {} games", b.len());
        let sc = b
            .match_process(
                "StarCitizen.exe",
                Some(r"C:\RSI\StarCitizen\LIVE\Bin64\StarCitizen.exe"),
            )
            .unwrap();
        assert_eq!(sc.game.slug, "star_citizen");
        let wow = b
            .match_process(
                "Wow.exe",
                Some(r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe"),
            )
            .unwrap();
        assert_eq!(wow.game.slug, "wow");
    }

    #[test]
    fn slugs() {
        assert_eq!(name_slug("Star Citizen"), "star_citizen");
        assert_eq!(
            name_slug("World of Warcraft: Forever"),
            "world_of_warcraft_forever"
        );
        assert_eq!(name_slug("  ®™ "), "");
        assert_eq!(name_slug("流亡黯道"), "");
    }
}
