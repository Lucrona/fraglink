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
    /// Broad genre, used for the fallback icon when there is no real one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    /// beta.6: Fraglink staff checked that detection works (tick in Options > Games).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub verified: bool,
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
    /// Rules (as Discord does): a game with path hints only matches when the path contains
    /// one of them, for its plain `exes` too, so e.g. Wow.exe in an unknown folder is no
    /// game at all (and offered as "Add it"). Entries whose hint matches win; otherwise the
    /// first plain `exes` entry of a game without hints. With no path known, a plain exe
    /// still matches (nothing to check it against); `path_exes` never do.
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
        // 2. first unconditional candidate: hints are required when the path is known
        cands
            .iter()
            .find(|(i, needs_hint)| {
                !needs_hint && (path_n.is_none() || self.games[*i].path_hints.is_empty())
            })
            .map(|&(i, _)| &self.games[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> SignatureDb {
        SignatureDb::builtin()
    }

    /// Like Discord: a game with path hints only counts inside them, plain exes too.
    #[test]
    fn path_hints_are_required() {
        let d = db();
        let m = |exe: &str, p: Option<&str>| d.match_process(exe, p).map(|g| g.id.clone());
        for (exe, p, want) in [
            ("Wow.exe", r"C:\WoW\_retail_\Wow.exe", Some("wow")),
            ("Wow.exe", r"C:\WoW\_forever_\Wow.exe", Some("wow_forever")),
            ("Wow.exe", r"C:\WoW\_someday_\Wow.exe", None),
            ("Wow.exe", r"D:\Wow.exe", None),
            (
                "Diablo II.exe",
                r"C:\Games\Diablo II\Diablo II.exe",
                Some("diablo2"),
            ),
            ("Diablo II.exe", r"C:\Games\D2\Diablo II.exe", None),
            (
                "tf_win64.exe",
                r"C:\Steam\steamapps\common\Team Fortress 2\tf_win64.exe",
                Some("tf2"),
            ),
            ("hl2.exe", r"C:\Elsewhere\hl2.exe", None),
            // no hints: any folder
            (
                "Hearthstone.exe",
                r"X:\Any\Hearthstone.exe",
                Some("hearthstone"),
            ),
        ] {
            assert_eq!(m(exe, Some(p)).as_deref(), want, "{p}");
        }
        // no path known: a plain exe still matches, a path_exe never does
        assert_eq!(m("Wow.exe", None).as_deref(), Some("wow"));
        assert_eq!(m("hl2.exe", None), None);
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
        // WoW Classic follows Discord's list: every WowClassic.exe is one game
        let era = d
            .match_process(
                "WowClassic.exe",
                Some(r"C:\Program Files (x86)\World of Warcraft\_classic_era_\WowClassic.exe"),
            )
            .unwrap();
        assert_eq!(era.id, "wow_classic");
        // public test builds aren't tracked
        assert!(d
            .match_process("WowT.exe", Some(r"C:\WoW\_ptr_\WowT.exe"))
            .is_none());
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

    #[test]
    fn verified_0_1_1_fixes() {
        let d = db();
        assert!(
            d.games().iter().all(|g| !g.uncertain),
            "no uncertain rows left"
        );
        // Civ VII: real Windows exe (was the macOS name)
        assert_eq!(
            d.match_process("Civ7_Win64_DX12_FinalRelease.exe", None)
                .unwrap()
                .id,
            "civ7"
        );
        assert!(d.match_process("CivilizationVII.exe", None).is_none());
        // Minecraft via the Microsoft Store launcher's bundled runtime
        let ms = r"C:\Users\me\AppData\Local\Packages\Microsoft.4297127D64EC6_8wekyb3d8bbwe\LocalCache\Local\runtime\java-runtime-delta\windows-x64\java-runtime-delta\bin\javaw.exe";
        assert_eq!(
            d.match_process("javaw.exe", Some(ms)).unwrap().id,
            "minecraft_java"
        );
        // THE FINALS: generic Discovery.exe only counts in its own folder
        assert!(d.match_process("Discovery.exe", None).is_none());
        assert_eq!(
            d.match_process(
                "Discovery.exe",
                Some(r"D:\SteamLibrary\steamapps\common\The Finals\Discovery.exe")
            )
            .unwrap()
            .id,
            "thefinals"
        );
        assert_eq!(
            d.match_process("deadlock.exe", None).unwrap().id,
            "deadlock"
        );
        assert_eq!(
            d.match_process("Marvel-Win64-Shipping.exe", None)
                .unwrap()
                .id,
            "marvel_rivals"
        );
    }
}
