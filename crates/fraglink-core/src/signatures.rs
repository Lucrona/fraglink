//! Game signature database (exe name -> game), shipped as JSON in
//! `packages/signatures/games.json`. Only executable names, optional
//! install-path fragments and Steam app ids: no game assets.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The signature file bundled into the binary at compile time.
pub const BUILTIN_JSON: &str = include_str!("../../../packages/signatures/games.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GameSig {
    /// Stable Fraglink game id (e.g. `wow`, `cs2`).
    pub id: String,
    pub name: String,
    /// Executable names that identify the game on their own (case-insensitive).
    #[serde(default)]
    pub exes: Vec<String>,
    /// Generic executable names (e.g. `hl2.exe`, `javaw.exe`) that only count
    /// when the full path contains one of `path_hints`.
    #[serde(default)]
    pub path_exes: Vec<String>,
    /// Lower-case, forward-slash path fragments used with `path_exes` and to
    /// break ties between games sharing an exe name.
    #[serde(default)]
    pub path_hints: Vec<String>,
    #[serde(default)]
    pub steam_appids: Vec<u32>,
    #[serde(default)]
    pub platform: String,
    /// True when the exe names / app ids have not been verified.
    #[serde(default)]
    pub uncertain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureFile {
    pub schema: u32,
    pub version: String,
    #[serde(default)]
    pub ignore_exes: Vec<String>,
    #[serde(default)]
    pub non_game_exes: Vec<String>,
    pub games: Vec<GameSig>,
}

#[derive(Debug, Clone)]
pub struct SignatureDb {
    pub version: String,
    games: Vec<GameSig>,
    by_exe: HashMap<String, Vec<(usize, bool)>>, // exe -> (game index, needs path hint)
    by_appid: HashMap<u32, usize>,
    by_id: HashMap<String, usize>,
    ignore: HashSet<String>,
    non_game: HashSet<String>,
}

/// Lower-case a path and use forward slashes so hints work on any OS.
pub fn normalize_path(p: &str) -> String {
    p.replace('\\', "/").to_lowercase()
}

impl SignatureDb {
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_JSON).expect("bundled games.json must parse")
    }

    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let file: SignatureFile = serde_json::from_str(json)?;
        Ok(Self::from_file(file))
    }

    pub fn from_file(file: SignatureFile) -> Self {
        let mut by_exe: HashMap<String, Vec<(usize, bool)>> = HashMap::new();
        let mut by_appid = HashMap::new();
        let mut by_id = HashMap::new();
        for (i, g) in file.games.iter().enumerate() {
            by_id.insert(g.id.clone(), i);
            for e in &g.exes {
                by_exe.entry(e.to_lowercase()).or_default().push((i, false));
            }
            for e in &g.path_exes {
                by_exe.entry(e.to_lowercase()).or_default().push((i, true));
            }
            for a in &g.steam_appids {
                by_appid.entry(*a).or_insert(i);
            }
        }
        SignatureDb {
            version: file.version,
            games: file.games,
            by_exe,
            by_appid,
            by_id,
            ignore: file.ignore_exes.iter().map(|s| s.to_lowercase()).collect(),
            non_game: file
                .non_game_exes
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        }
    }

    pub fn games(&self) -> &[GameSig] {
        &self.games
    }

    pub fn len(&self) -> usize {
        self.games.len()
    }

    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&GameSig> {
        self.by_id.get(id).map(|&i| &self.games[i])
    }

    pub fn by_steam_appid(&self, appid: u32) -> Option<&GameSig> {
        self.by_appid.get(&appid).map(|&i| &self.games[i])
    }

    /// Launchers, anti-cheat services, crash handlers, redistributables.
    pub fn is_ignored(&self, exe: &str) -> bool {
        self.ignore.contains(&exe.to_lowercase())
    }

    /// Common desktop apps that should not be offered as "is this a game?".
    pub fn is_known_non_game(&self, exe: &str) -> bool {
        self.non_game.contains(&exe.to_lowercase())
    }

    /// Match a running process by exe name and (optionally) full path.
    ///
    /// Rules: entries whose path hint matches win; generic `path_exes` only
    /// match with a hint; otherwise the first plain `exes` entry wins.
    pub fn match_process(&self, exe: &str, path: Option<&str>) -> Option<&GameSig> {
        let exe_l = exe.to_lowercase();
        if self.ignore.contains(&exe_l) {
            return None;
        }
        let cands = self.by_exe.get(&exe_l)?;
        let path_n = path.map(normalize_path);
        let hint_hit = |g: &GameSig| match &path_n {
            Some(p) => g.path_hints.iter().any(|h| p.contains(h.as_str())),
            None => false,
        };
        // 1. any candidate whose hint matches the path
        if let Some(&(i, _)) = cands.iter().find(|(i, _)| hint_hit(&self.games[*i])) {
            return Some(&self.games[i]);
        }
        // 2. first unconditional candidate
        cands
            .iter()
            .find(|(_, needs_hint)| !needs_hint)
            .map(|&(i, _)| &self.games[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> SignatureDb {
        SignatureDb::builtin()
    }

    #[test]
    fn builtin_parses_and_is_sane() {
        let d = db();
        assert!(d.len() >= 100, "expected ~100 seed games, got {}", d.len());
        let mut ids = HashSet::new();
        for g in d.games() {
            assert!(ids.insert(g.id.clone()), "duplicate id {}", g.id);
            assert!(!g.name.is_empty());
            assert!(
                !g.exes.is_empty() || !g.path_exes.is_empty() || !g.steam_appids.is_empty(),
                "{} has no way to be detected",
                g.id
            );
            if !g.path_exes.is_empty() {
                assert!(
                    !g.path_hints.is_empty(),
                    "{} has path_exes but no hints",
                    g.id
                );
            }
            for h in &g.path_hints {
                assert_eq!(
                    h,
                    &normalize_path(h),
                    "hint for {} must be normalized",
                    g.id
                );
            }
        }
    }

    #[test]
    fn matches_plain_exe_case_insensitively() {
        let d = db();
        assert_eq!(d.match_process("Wow.exe", None).unwrap().id, "wow");
        assert_eq!(d.match_process("WOW.EXE", None).unwrap().id, "wow");
        assert_eq!(d.match_process("cs2.exe", Some(r"D:\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe")).unwrap().id, "cs2");
        assert!(d.match_process("notepad.exe", None).is_none());
    }

    #[test]
    fn path_hints_disambiguate_shared_exes() {
        let d = db();
        let era = d
            .match_process(
                "WowClassic.exe",
                Some(r"C:\Program Files (x86)\World of Warcraft\_classic_era_\WowClassic.exe"),
            )
            .unwrap();
        assert_eq!(era.id, "wow_classic_era");
        let prog = d
            .match_process(
                "WowClassic.exe",
                Some(r"C:\Program Files (x86)\World of Warcraft\_classic_\WowClassic.exe"),
            )
            .unwrap();
        assert_eq!(prog.id, "wow_classic");
        // hl2.exe is shared by many Source games: needs a path
        assert!(d.match_process("hl2.exe", None).is_none());
        assert_eq!(
            d.match_process(
                "hl2.exe",
                Some(r"C:\Steam\steamapps\common\Half-Life 2\hl2.exe")
            )
            .unwrap()
            .id,
            "hl2"
        );
        assert_eq!(
            d.match_process(
                "hl2.exe",
                Some(r"C:\Steam\steamapps\common\Team Fortress 2\hl2.exe")
            )
            .unwrap()
            .id,
            "tf2"
        );
        // generic java only counts for Minecraft paths
        assert!(d
            .match_process("javaw.exe", Some(r"C:\Program Files\Java\bin\javaw.exe"))
            .is_none());
        assert_eq!(
            d.match_process(
                "javaw.exe",
                Some(r"C:\Users\me\AppData\Roaming\.minecraft\runtime\bin\javaw.exe")
            )
            .unwrap()
            .id,
            "minecraft_java"
        );
    }

    #[test]
    fn ignores_launchers_and_anticheat() {
        let d = db();
        for exe in [
            "steam.exe",
            "Battle.net.exe",
            "EasyAntiCheat.exe",
            "UnityCrashHandler64.exe",
            "LeagueClientUx.exe",
        ] {
            assert!(d.is_ignored(exe), "{exe} should be ignored");
            assert!(d.match_process(exe, None).is_none());
        }
    }

    #[test]
    fn steam_appid_lookup() {
        let d = db();
        assert_eq!(d.by_steam_appid(570).unwrap().id, "dota2");
        assert_eq!(d.by_steam_appid(730).unwrap().id, "cs2");
        assert!(d.by_steam_appid(1).is_none());
    }
}
