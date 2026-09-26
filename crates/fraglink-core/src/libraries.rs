//! Epic Games Store and GOG Galaxy install-folder detection (read-only, offline).
//!
//! Works like the Steam rule: any process whose image lives inside an installed
//! game's folder counts as that game.
//!
//! * **Epic:** `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` are JSON
//!   files with `DisplayName`, `InstallLocation`, `LaunchExecutable`, `AppName`, …
//!   `FRAGLINK_EPIC_MANIFESTS` overrides the folder (tests / non-Windows).
//! * **GOG:** `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` keys with `gameName`,
//!   `path`, `exe`, `gameID`, `dependsOn`. `FRAGLINK_GOG_DIR` points at a folder of
//!   `*.json` files with the same fields (tests / non-Windows).
//!
//! Missing launchers, unreadable files and odd entries are skipped quietly.

use crate::signatures::normalize_path;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Launcher {
    Epic,
    Gog,
}

impl Launcher {
    pub fn as_str(&self) -> &'static str {
        match self {
            Launcher::Epic => "epic",
            Launcher::Gog => "gog",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledGame {
    pub store: Launcher,
    /// Epic `AppName` or GOG `gameID`.
    pub key: String,
    pub name: String,
    pub install_dir: String,
    /// Main executable file name, if known (e.g. `FortniteLauncher.exe`).
    pub launch_exe: Option<String>,
    /// Normalized install dir with a trailing `/`.
    pub prefix: String,
}

impl InstalledGame {
    /// Stable Fraglink id when no signature matches (`epic:<AppName>`, `gog:<id>`).
    pub fn fallback_id(&self) -> String {
        format!("{}:{}", self.store.as_str(), self.key)
    }
}

/// Install dirs that are too broad to attribute every process under them.
fn prefix_is_safe(prefix: &str) -> bool {
    let trimmed = prefix.trim_end_matches('/');
    let segs: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return false; // "c:" or "/games"
    }
    const BROAD: &[&str] = &[
        "program files",
        "program files (x86)",
        "games",
        "epic games",
        "gog games",
        "gog galaxy",
        "users",
        "windows",
        "steamapps",
        "common",
        "xboxgames",
    ];
    !BROAD.contains(&segs[segs.len() - 1])
}

fn make_prefix(dir: &str) -> String {
    let mut p = normalize_path(dir.trim());
    while p.ends_with('/') {
        p.pop();
    }
    p.push('/');
    p
}

fn file_name(p: &str) -> Option<String> {
    let n = p.replace('\\', "/");
    let last = n.rsplit('/').next()?.trim();
    (!last.is_empty()).then(|| last.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct EpicItem {
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    install_location: String,
    #[serde(default)]
    launch_executable: String,
    #[serde(default)]
    app_name: String,
    #[serde(default)]
    main_game_app_name: String,
    #[serde(default)]
    app_categories: Vec<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    b_is_incomplete_install: bool,
}

/// Parse one Epic `.item` manifest. DLC, engine installs and broken entries give `None`.
pub fn parse_epic_item(text: &str) -> Option<InstalledGame> {
    let it: EpicItem = serde_json::from_str(text).ok()?;
    if it.install_location.trim().is_empty() || it.app_name.is_empty() || it.b_is_incomplete_install
    {
        return None;
    }
    if !it.main_game_app_name.is_empty() && it.main_game_app_name != it.app_name {
        return None; // DLC / add-on sharing the main game's folder
    }
    if !it.app_categories.is_empty() && !it.app_categories.iter().any(|c| c == "games") {
        return None; // Unreal Engine, plugins, …
    }
    let prefix = make_prefix(&it.install_location);
    if !prefix_is_safe(&prefix) {
        return None;
    }
    let name = if it.display_name.trim().is_empty() {
        it.app_name.clone()
    } else {
        it.display_name.trim().to_string()
    };
    Some(InstalledGame {
        store: Launcher::Epic,
        key: it.app_name,
        name,
        install_dir: it.install_location,
        launch_exe: file_name(&it.launch_executable),
        prefix,
    })
}

/// Build a GOG entry from the values of one `GOG.com\Games\<id>` registry key.
pub fn parse_gog_values(key_name: &str, v: &HashMap<String, String>) -> Option<InstalledGame> {
    let get = |k: &str| v.get(k).map(|s| s.trim()).filter(|s| !s.is_empty());
    if get("dependsOn").is_some() {
        return None; // DLC
    }
    let path = get("path")?;
    let prefix = make_prefix(path);
    if !prefix_is_safe(&prefix) {
        return None;
    }
    let key = get("gameID").unwrap_or(key_name).to_string();
    let name = get("gameName")
        .or_else(|| get("startMenu"))
        .map(str::to_string)
        .or_else(|| file_name(path))
        .unwrap_or_else(|| key.clone());
    Some(InstalledGame {
        store: Launcher::Gog,
        key,
        name,
        install_dir: path.to_string(),
        launch_exe: get("exe").and_then(file_name),
        prefix,
    })
}

#[derive(Debug, Clone, Default)]
pub struct LibraryIndex {
    games: Vec<InstalledGame>,
}

impl LibraryIndex {
    pub fn len(&self) -> usize {
        self.games.len()
    }
    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }
    pub fn games(&self) -> &[InstalledGame] {
        &self.games
    }
    pub fn count(&self, store: Launcher) -> usize {
        self.games.iter().filter(|g| g.store == store).count()
    }

    pub fn insert(&mut self, g: InstalledGame) {
        if let Some(i) = self
            .games
            .iter()
            .position(|x| x.store == g.store && x.key == g.key)
        {
            self.games[i] = g;
        } else {
            self.games.push(g);
        }
    }

    /// Which installed Epic/GOG game does this executable path belong to? Longest prefix wins.
    pub fn game_for_path(&self, path: &str) -> Option<&InstalledGame> {
        let p = normalize_path(path);
        self.games
            .iter()
            .filter(|g| p.starts_with(&g.prefix))
            .max_by_key(|g| g.prefix.len())
    }

    /// Read every `*.item` in an Epic manifests folder.
    pub fn add_epic_dir(&mut self, dir: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("item") {
                continue;
            }
            // Manifests are small; cap the read in case of junk.
            if e.metadata().map(|m| m.len() > 1_000_000).unwrap_or(true) {
                continue;
            }
            if let Some(g) = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| parse_epic_item(&t))
            {
                self.insert(g);
            }
        }
    }

    /// Read `*.json` files shaped like GOG registry keys (test / non-Windows fallback).
    pub fn add_gog_json_dir(&mut self, dir: &Path) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Some(map) = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<HashMap<String, String>>(&t).ok())
            else {
                continue;
            };
            let key = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            if let Some(g) = parse_gog_values(&key, &map) {
                self.insert(g);
            }
        }
    }

    #[cfg(windows)]
    fn add_gog_registry(&mut self) {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
        use winreg::RegKey;
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        for root in [
            r"SOFTWARE\WOW6432Node\GOG.com\Games",
            r"SOFTWARE\GOG.com\Games",
        ] {
            let Ok(games) = hklm.open_subkey_with_flags(root, KEY_READ) else {
                continue;
            };
            for name in games.enum_keys().flatten() {
                let Ok(k) = games.open_subkey_with_flags(&name, KEY_READ) else {
                    continue;
                };
                let mut map = HashMap::new();
                for (vname, _) in k.enum_values().flatten() {
                    if let Ok(s) = k.get_value::<String, _>(&vname) {
                        map.insert(vname, s);
                    }
                }
                if let Some(g) = parse_gog_values(&name, &map) {
                    self.insert(g);
                }
            }
        }
    }

    /// Discover Epic + GOG installs on this machine.
    pub fn load() -> Self {
        let mut idx = LibraryIndex::default();
        if let Some(dir) = epic_manifest_dir() {
            idx.add_epic_dir(&dir);
        }
        if let Ok(d) = std::env::var("FRAGLINK_GOG_DIR") {
            idx.add_gog_json_dir(Path::new(&d));
        } else {
            #[cfg(windows)]
            idx.add_gog_registry();
        }
        idx
    }
}

/// `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests`, or `FRAGLINK_EPIC_MANIFESTS`.
pub fn epic_manifest_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FRAGLINK_EPIC_MANIFESTS") {
        return Some(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        let d = base.join(r"Epic\EpicGamesLauncher\Data\Manifests");
        d.exists().then_some(d)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn epic_manifests_parse_and_skip_non_games() {
        let mut idx = LibraryIndex::default();
        idx.add_epic_dir(&fixtures().join("epic"));
        let mut names: Vec<_> = idx.games().iter().map(|g| g.name.as_str()).collect();
        names.sort();
        // DLC, Unreal Engine, incomplete install and junk file are skipped
        assert_eq!(names, vec!["Fortnite", "Hades", "Rocket League®"]);
        let hades = idx.games().iter().find(|g| g.key == "Min").unwrap();
        assert_eq!(hades.prefix, "d:/epic games/hades/");
        assert_eq!(hades.launch_exe.as_deref(), Some("Hades.exe"));
        assert_eq!(hades.fallback_id(), "epic:Min");
    }

    #[test]
    fn gog_values_parse_and_skip_dlc() {
        let mut idx = LibraryIndex::default();
        idx.add_gog_json_dir(&fixtures().join("gog"));
        assert_eq!(idx.count(Launcher::Gog), 2);
        let w3 = idx.games().iter().find(|g| g.key == "1207664663").unwrap();
        assert_eq!(
            w3.name,
            "The Witcher 3: Wild Hunt - Game of the Year Edition"
        );
        assert_eq!(w3.launch_exe.as_deref(), Some("witcher3.exe"));
        assert_eq!(w3.fallback_id(), "gog:1207664663");
    }

    #[test]
    fn path_lookup_uses_longest_prefix_and_is_case_insensitive() {
        let mut idx = LibraryIndex::default();
        idx.add_epic_dir(&fixtures().join("epic"));
        idx.add_gog_json_dir(&fixtures().join("gog"));
        let g = idx
            .game_for_path(r"D:\EPIC GAMES\Hades\x64\Hades.exe")
            .unwrap();
        assert_eq!(g.key, "Min");
        let w = idx
            .game_for_path(r"C:\GOG Games\The Witcher 3 Wild Hunt GOTY\bin\x64\witcher3.exe")
            .unwrap();
        assert_eq!(w.store, Launcher::Gog);
        assert!(idx
            .game_for_path(r"D:\Epic Games\HadesII\Hades2.exe")
            .is_none());
        assert!(idx.game_for_path(r"C:\Windows\notepad.exe").is_none());
    }

    #[test]
    fn broad_install_dirs_are_rejected() {
        let mut v = HashMap::new();
        v.insert("path".to_string(), r"C:\GOG Games".to_string());
        v.insert("gameName".to_string(), "Bad".to_string());
        assert!(parse_gog_values("1", &v).is_none());
        v.insert("path".to_string(), r"D:\".to_string());
        assert!(parse_gog_values("1", &v).is_none());
        let epic = r#"{"DisplayName":"X","InstallLocation":"C:\\Program Files","AppName":"x","LaunchExecutable":"x.exe"}"#;
        assert!(parse_epic_item(epic).is_none());
        assert!(parse_epic_item("not json").is_none());
    }
}
