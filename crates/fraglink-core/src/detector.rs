//! Turns a process snapshot into "which games are running".
//! Priority: manual tags > signature DB > Steam library path > Epic/GOG install
//! folder > Steam RunningAppID.

use crate::env::ProcInfo;
use crate::libraries::{InstalledGame, Launcher, LibraryIndex};
use crate::signatures::{normalize_path, SignatureDb};
use crate::steam::SteamIndex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectSource {
    Manual,
    Signature,
    SteamLibrary,
    SteamRunning,
    EpicLibrary,
    GogLibrary,
}

impl DetectSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectSource::Manual => "manual",
            DetectSource::Signature => "signature",
            DetectSource::SteamLibrary => "steam_library",
            DetectSource::SteamRunning => "steam_running",
            DetectSource::EpicLibrary => "epic_library",
            DetectSource::GogLibrary => "gog_library",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    pub game_id: String,
    pub name: String,
    pub source: DetectSource,
    pub exe: Option<String>,
    pub steam_appid: Option<u32>,
}

/// A running program that is not a known game (offered for manual tagging).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub exe: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct Detector {
    pub sigs: SignatureDb,
    pub steam: SteamIndex,
    /// Epic Games Store + GOG Galaxy installs.
    pub libs: LibraryIndex,
    /// lower-case exe -> (game id, display name)
    pub tags: HashMap<String, (String, String)>,
    pub excluded_games: HashSet<String>,
    pub excluded_exes: HashSet<String>,
}

pub fn steam_game_id(sigs: &SignatureDb, appid: u32) -> String {
    sigs.by_steam_appid(appid)
        .map(|g| g.id.clone())
        .unwrap_or_else(|| format!("steam:{appid}"))
}

fn looks_like_helper(exe_l: &str, path_n: &str) -> bool {
    const BAD: &[&str] = &[
        "crash",
        "redist",
        "setup",
        "unins",
        "report",
        "updater",
        "prereq",
        "installer",
        "dxsetup",
        "cefprocess",
        "webhelper",
    ];
    BAD.iter().any(|b| exe_l.contains(b))
        || path_n.contains("/_commonredist/")
        || path_n.contains("/redist/")
}

impl Detector {
    pub fn new(sigs: SignatureDb, steam: SteamIndex) -> Self {
        Detector {
            sigs,
            steam,
            libs: LibraryIndex::default(),
            tags: HashMap::new(),
            excluded_games: HashSet::new(),
            excluded_exes: HashSet::new(),
        }
    }

    fn steam_name(&self, appid: u32) -> String {
        if let Some(g) = self.sigs.by_steam_appid(appid) {
            return g.name.clone();
        }
        self.steam
            .get(appid)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("Steam app {appid}"))
    }

    /// Signature id/name for an Epic/GOG install (via its launch exe), else `epic:<key>` / `gog:<key>`.
    fn library_identity(&self, g: &InstalledGame) -> (String, String) {
        if let Some(exe) = &g.launch_exe {
            let full = format!("{}{}", g.prefix, exe.to_lowercase());
            if let Some(sig) = self.sigs.match_process(exe, Some(&full)) {
                return (sig.id.clone(), sig.name.clone());
            }
        }
        (g.fallback_id(), g.name.clone())
    }

    fn classify(&self, p: &ProcInfo) -> Option<Detection> {
        let exe_l = p.name.to_lowercase();
        if self.excluded_exes.contains(&exe_l) {
            return None;
        }
        if let Some((id, name)) = self.tags.get(&exe_l) {
            return Some(Detection {
                game_id: id.clone(),
                name: name.clone(),
                source: DetectSource::Manual,
                exe: Some(p.name.clone()),
                steam_appid: None,
            });
        }
        if self.sigs.is_ignored(&p.name) {
            return None;
        }
        if let Some(g) = self.sigs.match_process(&p.name, p.path.as_deref()) {
            return Some(Detection {
                game_id: g.id.clone(),
                name: g.name.clone(),
                source: DetectSource::Signature,
                exe: Some(p.name.clone()),
                steam_appid: g.steam_appids.first().copied(),
            });
        }
        if let Some(path) = &p.path {
            if let Some(app) = self.steam.app_for_path(path) {
                if !looks_like_helper(&exe_l, &normalize_path(path)) {
                    return Some(Detection {
                        game_id: steam_game_id(&self.sigs, app.appid),
                        name: self.steam_name(app.appid),
                        source: DetectSource::SteamLibrary,
                        exe: Some(p.name.clone()),
                        steam_appid: Some(app.appid),
                    });
                }
            }
            if let Some(g) = self.libs.game_for_path(path) {
                if !looks_like_helper(&exe_l, &normalize_path(path)) {
                    let (game_id, name) = self.library_identity(g);
                    return Some(Detection {
                        game_id,
                        name,
                        source: match g.store {
                            Launcher::Epic => DetectSource::EpicLibrary,
                            Launcher::Gog => DetectSource::GogLibrary,
                        },
                        exe: Some(p.name.clone()),
                        steam_appid: None,
                    });
                }
            }
        }
        None
    }

    /// All games currently running, de-duplicated by game id, in process order.
    pub fn detect(&self, procs: &[ProcInfo], steam_running: Option<u32>) -> Vec<Detection> {
        let mut out: Vec<Detection> = Vec::new();
        for p in procs {
            if let Some(d) = self.classify(p) {
                if !out.iter().any(|x| x.game_id == d.game_id) {
                    out.push(d);
                }
            }
        }
        if let Some(appid) = steam_running {
            // Only trust RunningAppID while Steam itself is running (it can go stale after a crash).
            let steam_up = procs.iter().any(|p| {
                let n = p.name.to_lowercase();
                n == "steam.exe" || n == "steam"
            });
            let name = self.steam_name(appid);
            if steam_up && !crate::steam::is_tool_app(appid, &name) {
                let id = steam_game_id(&self.sigs, appid);
                if !out.iter().any(|x| x.game_id == id) {
                    out.push(Detection {
                        game_id: id,
                        name,
                        source: DetectSource::SteamRunning,
                        exe: None,
                        steam_appid: Some(appid),
                    });
                }
            }
        }
        out.retain(|d| !self.excluded_games.contains(&d.game_id));
        out
    }

    /// Running programs with a readable path that are not games, launchers,
    /// system processes or common desktop apps. Exe names only leave this
    /// function; nothing is sent anywhere.
    pub fn candidates(&self, procs: &[ProcInfo]) -> Vec<Candidate> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for p in procs {
            let Some(path) = &p.path else { continue };
            let exe_l = p.name.to_lowercase();
            let path_n = normalize_path(path);
            if self.sigs.is_ignored(&p.name)
                || self.sigs.is_known_non_game(&p.name)
                || self.tags.contains_key(&exe_l)
                || path_n.starts_with("c:/windows/")
                || path_n.contains("/windowsapps/microsoft.")
                || path_n.starts_with("/usr/")
                || path_n.starts_with("/bin/")
                || path_n.starts_with("/sbin/")
                || path_n.starts_with("/lib")
                || looks_like_helper(&exe_l, &path_n)
                || self.classify(p).is_some()
                || !seen.insert(exe_l)
            {
                continue;
            }
            out.push(Candidate {
                exe: p.name.clone(),
                path: path.clone(),
            });
        }
        out.sort_by_key(|c| c.exe.to_lowercase());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pid: u32, name: &str, path: Option<&str>) -> ProcInfo {
        ProcInfo {
            pid,
            name: name.into(),
            path: path.map(Into::into),
        }
    }

    fn det() -> Detector {
        let mut steam = SteamIndex::default();
        steam.insert(r"D:\SteamLibrary", 1145350, "Hades II", "Hades II");
        steam.insert(r"D:\SteamLibrary", 999999, "Tiny Indie Game", "TinyIndie");
        Detector::new(SignatureDb::builtin(), steam)
    }

    #[test]
    fn detects_signature_and_steam_library_games() {
        let d = det();
        let procs = vec![
            p(1, "explorer.exe", Some(r"C:\Windows\explorer.exe")),
            p(
                2,
                "Wow.exe",
                Some(r"C:\Program Files (x86)\World of Warcraft\_retail_\Wow.exe"),
            ),
            p(
                3,
                "TinyIndie.exe",
                Some(r"D:\SteamLibrary\steamapps\common\TinyIndie\TinyIndie.exe"),
            ),
            p(
                4,
                "UnityCrashHandler64.exe",
                Some(r"D:\SteamLibrary\steamapps\common\TinyIndie\UnityCrashHandler64.exe"),
            ),
        ];
        let found = d.detect(&procs, None);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].game_id, "wow");
        assert_eq!(found[0].source, DetectSource::Signature);
        assert_eq!(found[1].game_id, "steam:999999");
        assert_eq!(found[1].name, "Tiny Indie Game");
        assert_eq!(found[1].source, DetectSource::SteamLibrary);
    }

    #[test]
    fn steam_library_uses_signature_id_when_known() {
        let d = det();
        let procs = vec![p(
            3,
            "Hades2.exe",
            Some(r"D:\SteamLibrary\steamapps\common\Hades II\Ship\Hades2.exe"),
        )];
        let found = d.detect(&procs, None);
        assert_eq!(found[0].game_id, "hades2"); // signature wins, same id either way
    }

    #[test]
    fn running_appid_needs_steam_process() {
        let d = det();
        assert!(
            d.detect(&[], Some(570)).is_empty(),
            "stale RunningAppID without Steam is ignored"
        );
        let procs = vec![p(
            9,
            "steam.exe",
            Some(r"C:\Program Files (x86)\Steam\steam.exe"),
        )];
        let found = d.detect(&procs, Some(570));
        assert_eq!(found[0].game_id, "dota2");
        assert_eq!(found[0].source, DetectSource::SteamRunning);
        let unknown = d.detect(&procs, Some(4242));
        assert_eq!(unknown[0].game_id, "steam:4242");
        assert!(
            d.detect(&procs, Some(228980)).is_empty(),
            "tools never count"
        );
    }

    #[test]
    fn manual_tags_and_exclusions() {
        let mut d = det();
        let procs = vec![
            p(1, "MyGame.exe", Some(r"C:\Games\MyGame\MyGame.exe")),
            p(2, "cs2.exe", Some(r"C:\Steam\steamapps\common\cs2\cs2.exe")),
        ];
        assert_eq!(d.detect(&procs, None).len(), 1);
        let cands = d.candidates(&procs);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].exe, "MyGame.exe");

        d.tags.insert(
            "mygame.exe".into(),
            ("manual:mygame".into(), "My Game".into()),
        );
        let found = d.detect(&procs, None);
        assert_eq!(found[0].game_id, "manual:mygame");
        assert_eq!(found[0].source, DetectSource::Manual);
        assert!(d.candidates(&procs).is_empty());

        d.excluded_games.insert("cs2".into());
        let found = d.detect(&procs, None);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].game_id, "manual:mygame");

        d.excluded_exes.insert("mygame.exe".into());
        assert!(d.detect(&procs, None).is_empty());
    }

    #[test]
    fn candidates_skip_system_and_known_apps() {
        let d = det();
        let procs = vec![
            p(1, "svchost.exe", Some(r"C:\Windows\System32\svchost.exe")),
            p(
                2,
                "chrome.exe",
                Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
            ),
            p(
                3,
                "steam.exe",
                Some(r"C:\Program Files (x86)\Steam\steam.exe"),
            ),
            p(4, "Indie.exe", Some(r"C:\Games\Indie\Indie.exe")),
            p(5, "Indie.exe", Some(r"C:\Games\Indie\Indie.exe")),
            p(6, "nopath.exe", None),
        ];
        let c = d.candidates(&procs);
        assert_eq!(
            c.iter().map(|c| c.exe.as_str()).collect::<Vec<_>>(),
            vec!["Indie.exe"]
        );
    }

    #[test]
    fn detects_epic_and_gog_installs_by_folder() {
        use crate::libraries::{parse_epic_item, parse_gog_values};
        let mut d = det();
        d.libs.insert(
            parse_epic_item(
                r#"{"AppName":"Obscure","DisplayName":"Obscure Indie","InstallLocation":"D:\\Epic Games\\ObscureIndie","LaunchExecutable":"Obscure.exe","AppCategories":["games"]}"#,
            )
            .unwrap(),
        );
        d.libs.insert(
            parse_epic_item(
                r#"{"AppName":"Fortnite","DisplayName":"Fortnite","InstallLocation":"C:\\Program Files\\Epic Games\\Fortnite","LaunchExecutable":"FortniteGame/Binaries/Win64/FortniteLauncher.exe"}"#,
            )
            .unwrap(),
        );
        let mut v = std::collections::HashMap::new();
        v.insert("gameID".to_string(), "1207658924".to_string());
        v.insert("gameName".to_string(), "Unreal Tournament 2004".to_string());
        v.insert("path".to_string(), r"C:\GOG Games\UT2004".to_string());
        d.libs.insert(parse_gog_values("1207658924", &v).unwrap());

        let procs = vec![
            p(
                1,
                "Obscure.exe",
                Some(r"D:\Epic Games\ObscureIndie\Obscure.exe"),
            ),
            p(
                2,
                "UT2004.exe",
                Some(r"C:\GOG Games\UT2004\System\UT2004.exe"),
            ),
            p(
                3,
                "CrashReportClient.exe",
                Some(r"D:\Epic Games\ObscureIndie\CrashReportClient.exe"),
            ),
        ];
        let found = d.detect(&procs, None);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].game_id, "epic:Obscure");
        assert_eq!(found[0].name, "Obscure Indie");
        assert_eq!(found[0].source, DetectSource::EpicLibrary);
        // UT2004 is in the signature DB, so the signature wins with its own id
        assert_eq!(found[1].source, DetectSource::Signature);

        // A helper process in the Fortnite folder maps to the signature id via the launch exe
        let helper = vec![p(
            4,
            "FortniteClient-Win64-Shipping_EAC_EOS.exe",
            Some(
                r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping_EAC_EOS.exe",
            ),
        )];
        let f = d.detect(&helper, None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].source, DetectSource::EpicLibrary);
        assert_eq!(
            f[0].game_id,
            d.sigs
                .match_process("FortniteLauncher.exe", None)
                .map(|g| g.id.clone())
                .unwrap_or("epic:Fortnite".into())
        );

        // exclusions apply to library games too
        d.excluded_games.insert("epic:Obscure".into());
        assert_eq!(d.detect(&procs, None).len(), 1);
    }
}
