//! Turns a process snapshot into "which games are running".
//!
//! Priority (0.2.0-dev.12): manual tags > Fraglink overrides (WoW flavours, Star
//! Citizen channels) > signature DB > Discord's detectable list > Steam library
//! path > Steam RunningAppID.
//!
//! Other launchers' install records (Epic, GOG, Battle.net, EA, Ubisoft, Xbox)
//! no longer decide *whether* something is a game: they only feed the Installed
//! Games list and Launch buttons, and keep the old `epic:`/`gog:` id of a game
//! the detectable list recognises inside such an install (so its hours stay put).

use crate::detectable::DetectableDb;
use crate::env::ProcInfo;
use crate::libraries::{InstalledGame, Launcher, LibraryIndex};
use crate::overrides::Overrides;
use crate::signatures::{normalize_path, SignatureDb};
use crate::steam::SteamIndex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectSource {
    Manual,
    /// Fraglink's own folder rules (`overrides.json`).
    Override,
    Signature,
    /// Discord's public detectable list.
    Detectable,
    SteamLibrary,
    SteamRunning,
    EpicLibrary,
    GogLibrary,
    BattlenetLibrary,
    EaLibrary,
    UbisoftLibrary,
    XboxLibrary,
    /// A folder from the Windows Uninstall list matched to a signature.
    InstalledLibrary,
}

impl DetectSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectSource::Manual => "manual",
            DetectSource::Override => "override",
            DetectSource::Signature => "signature",
            DetectSource::Detectable => "detectable",
            DetectSource::SteamLibrary => "steam_library",
            DetectSource::SteamRunning => "steam_running",
            DetectSource::EpicLibrary => "epic_library",
            DetectSource::GogLibrary => "gog_library",
            DetectSource::BattlenetLibrary => "battlenet_library",
            DetectSource::EaLibrary => "ea_library",
            DetectSource::UbisoftLibrary => "ubisoft_library",
            DetectSource::XboxLibrary => "xbox_library",
            DetectSource::InstalledLibrary => "installed_library",
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
    /// Folder rules that win over every list (server-updatable).
    pub overrides: Overrides,
    /// Discord's detectable list (latest download, else the bundled snapshot).
    pub detectable: Arc<DetectableDb>,
}

pub fn steam_game_id(sigs: &SignatureDb, appid: u32) -> String {
    sigs.by_steam_appid(appid)
        .map(|g| g.id.clone())
        .unwrap_or_else(|| format!("steam:{appid}"))
}

/// The id of a Steam app: a signature id, else `steam:<appid>`, then any Admin
/// alias / merge (`steam:793460` -> `112_operator`). The server's Steam sync uses
/// the same order, so app sessions and Steam playtime land on one game.
pub fn steam_identity(sigs: &SignatureDb, overrides: &Overrides, appid: u32) -> String {
    overrides.resolve(&steam_game_id(sigs, appid)).to_string()
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
            overrides: Overrides::builtin(),
            detectable: DetectableDb::bundled_shared(),
        }
    }

    fn steam_id(&self, appid: u32) -> String {
        steam_identity(&self.sigs, &self.overrides, appid)
    }

    fn steam_name(&self, appid: u32) -> String {
        if let Some(g) = self.sigs.by_steam_appid(appid) {
            return g.name.clone();
        }
        if let Some(g) = self.detectable.by_steam_appid(appid) {
            return g.name.clone();
        }
        self.steam
            .get(appid)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| format!("Steam app {appid}"))
    }

    /// Signature id/name for an Epic/GOG install (via its launch exe), else `epic:<key>` / `gog:<key>`.
    fn library_identity(&self, g: &InstalledGame) -> (String, String) {
        if let Some(sig) = g.sig_id.as_deref().and_then(|id| self.sigs.get(id)) {
            return (sig.id.clone(), sig.name.clone());
        }
        if let Some(exe) = &g.launch_exe {
            let full = format!("{}{}", g.prefix, exe.to_lowercase());
            if let Some(sig) = self.sigs.match_process(exe, Some(&full)) {
                return (sig.id.clone(), sig.name.clone());
            }
        }
        (g.fallback_id(), g.name.clone())
    }

    /// How an exe on disk would be classified if it were running (folder scan).
    pub fn classify_file(&self, path: &str) -> Option<Detection> {
        let name = std::path::Path::new(path)
            .file_name()?
            .to_string_lossy()
            .into_owned();
        self.classify(&ProcInfo {
            pid: 0,
            name,
            path: Some(path.to_string()),
        })
    }

    /// [`classify_raw`](Self::classify_raw) with Admin aliases applied.
    fn classify(&self, p: &ProcInfo) -> Option<Detection> {
        let mut d = self.classify_raw(p)?;
        if d.source != DetectSource::Manual {
            let to = self.overrides.resolve(&d.game_id);
            if to != d.game_id {
                if let Some(n) = self
                    .sigs
                    .get(to)
                    .map(|g| g.name.clone())
                    .or_else(|| self.detectable.by_slug(to).map(|g| g.name.clone()))
                {
                    d.name = n;
                }
                d.game_id = to.to_string();
            }
        }
        Some(d)
    }

    fn classify_raw(&self, p: &ProcInfo) -> Option<Detection> {
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
        if let Some(r) = self.overrides.match_process(&p.name, p.path.as_deref()) {
            return Some(Detection {
                game_id: r.id.clone(),
                name: r.name.clone(),
                source: DetectSource::Override,
                exe: Some(p.name.clone()),
                steam_appid: self
                    .sigs
                    .get(&r.id)
                    .and_then(|g| g.steam_appids.first().copied()),
            });
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
        let path_n = p.path.as_deref().map(normalize_path);
        if !self.sigs.is_known_non_game(&p.name) {
            if let Some(m) = self.detectable.match_process(&p.name, p.path.as_deref()) {
                // Same game, old id: inside a Steam / launcher install keep that id.
                if let Some(path) = &p.path {
                    if let Some(app) = self.steam.app_for_path(path) {
                        // (the list's own id when Discord links it to this app id)
                        return Some(Detection {
                            game_id: self.steam_id(app.appid),
                            name: self.steam_name(app.appid),
                            source: DetectSource::Detectable,
                            exe: Some(p.name.clone()),
                            steam_appid: Some(app.appid),
                        });
                    }
                    if let Some(g) = self.libs.game_for_path(path) {
                        if g.store != Launcher::Windows {
                            let (game_id, name) = self.library_identity(g);
                            return Some(Detection {
                                game_id,
                                name,
                                source: DetectSource::Detectable,
                                exe: Some(p.name.clone()),
                                steam_appid: None,
                            });
                        }
                    }
                }
                // Discord links the game to a Steam app installed here: it is that
                // Steam game, whatever folder the Steam index knows it by.
                if let Some(appid) = m
                    .game
                    .steam
                    .iter()
                    .copied()
                    .find(|a| self.steam.get(*a).is_some())
                {
                    return Some(Detection {
                        game_id: self.steam_id(appid),
                        name: self.steam_name(appid),
                        source: DetectSource::Detectable,
                        exe: Some(p.name.clone()),
                        steam_appid: Some(appid),
                    });
                }
                let sig = self.sigs.get(&m.game.slug);
                return Some(Detection {
                    game_id: m.game.slug.clone(),
                    name: sig
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| m.game.name.clone()),
                    source: DetectSource::Detectable,
                    exe: Some(p.name.clone()),
                    steam_appid: sig.and_then(|g| g.steam_appids.first().copied()),
                });
            }
        }
        if let (Some(path), Some(path_n)) = (&p.path, &path_n) {
            if let Some(app) = self.steam.app_for_path(path) {
                if !looks_like_helper(&exe_l, path_n) {
                    return Some(Detection {
                        game_id: self.steam_id(app.appid),
                        name: self.steam_name(app.appid),
                        source: DetectSource::SteamLibrary,
                        exe: Some(p.name.clone()),
                        steam_appid: Some(app.appid),
                    });
                }
            }
        }
        None
    }

    /// All games currently running, de-duplicated by game id and by exe path (one
    /// program is never two games), in process order.
    pub fn detect(&self, procs: &[ProcInfo], steam_running: Option<u32>) -> Vec<Detection> {
        let mut out: Vec<Detection> = Vec::new();
        let mut paths: HashSet<String> = HashSet::new();
        for p in procs {
            let key = p.path.as_deref().map(normalize_path);
            if key.as_ref().is_some_and(|k| paths.contains(k)) {
                continue;
            }
            if let Some(d) = self.classify(p) {
                if let Some(k) = key {
                    paths.insert(k);
                }
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
                let id = self.steam_id(appid);
                // the same Steam app already seen through its exe counts once
                if !out
                    .iter()
                    .any(|x| x.game_id == id || x.steam_appid == Some(appid))
                {
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

    fn dgame(id: &str, slug: &str, name: &str, exes: &[&str]) -> crate::detectable::DGame {
        crate::detectable::DGame {
            id: id.into(),
            slug: slug.into(),
            name: name.into(),
            exes: exes.iter().map(|e| e.to_string()).collect(),
            steam: vec![],
        }
    }

    #[test]
    fn steam_linked_game_has_one_id() {
        // 24 Oct: 112 Operator was first seen by the detectable list, then a Steam
        // rescan made the same process steam:793460. Every route now gives one id.
        let mut steam = SteamIndex::default();
        steam.insert(r"D:\SteamLibrary", 793460, "112 Operator", "112 Operator");
        let mut g = dgame(
            "1",
            "112_operator",
            "112 Operator",
            &["112 operator/operator 112.exe"],
        );
        g.steam = vec![793460];
        let d = with_list(Detector::new(SignatureDb::builtin(), steam), vec![g]);
        let steam_proc = p(2, "steam.exe", Some(r"C:\Steam\steam.exe"));
        let in_steam = p(
            1,
            "Operator 112.exe",
            Some(r"D:\SteamLibrary\steamapps\common\112 Operator\Operator 112.exe"),
        );
        // a path the Steam index does not cover (moved library, symlink)
        let elsewhere = p(
            1,
            "Operator 112.exe",
            Some(r"E:\Games\112 Operator\Operator 112.exe"),
        );
        for procs in [
            vec![in_steam.clone(), steam_proc.clone()],
            vec![elsewhere.clone(), steam_proc.clone()],
        ] {
            let found = d.detect(&procs, Some(793460));
            assert_eq!(found.len(), 1, "{found:?}");
            assert_eq!(found[0].game_id, "steam:793460");
            assert_eq!(found[0].name, "112 Operator");
        }
        assert_eq!(
            d.detect(&[steam_proc], Some(793460))[0].game_id,
            "steam:793460"
        );
        // not installed through Steam: the list's own id
        let mut d2 = d.clone();
        d2.steam = SteamIndex::default();
        assert_eq!(d2.detect(&[elsewhere], None)[0].game_id, "112_operator");
    }

    #[test]
    fn admin_alias_maps_steam_id() {
        let mut steam = SteamIndex::default();
        steam.insert(r"D:\SteamLibrary", 793460, "112 Operator", "112 Operator");
        let mut d = Detector::new(SignatureDb::builtin(), steam);
        d.overrides = Overrides::from_json(
            r#"{"schema":1,"version":"t","rules":[],"aliases":[{"from":"steam:793460","to":"112_operator"}]}"#,
        )
        .unwrap();
        let procs = vec![p(
            1,
            "Operator 112.exe",
            Some(r"D:\SteamLibrary\steamapps\common\112 Operator\Operator 112.exe"),
        )];
        assert_eq!(d.detect(&procs, None)[0].game_id, "112_operator");
        assert_eq!(d.steam_id(793460), "112_operator");
    }

    #[test]
    fn dedup_by_exe_path() {
        let d = det();
        let path = r"D:\SteamLibrary\steamapps\common\TinyIndie\TinyIndie.exe";
        let procs = vec![
            p(3, "TinyIndie.exe", Some(path)),
            p(4, "TinyIndie.exe", Some(path)),
            p(9, "steam.exe", Some(r"C:\Steam\steam.exe")),
        ];
        let found = d.detect(&procs, Some(999999));
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].game_id, "steam:999999");
    }

    fn with_list(mut d: Detector, games: Vec<crate::detectable::DGame>) -> Detector {
        d.detectable = Arc::new(DetectableDb::from_file(crate::detectable::DetectableFile {
            v: 1,
            updated: String::new(),
            games,
        }));
        d
    }

    #[test]
    fn launcher_installs_no_longer_decide_what_is_a_game() {
        use crate::libraries::{parse_epic_item, parse_gog_values};
        let mut d = with_list(
            det(),
            vec![dgame(
                "77",
                "known_indie",
                "Known Indie",
                &["knownindie.exe"],
            )],
        );
        d.libs.insert(
            parse_epic_item(
                r#"{"AppName":"Obscure","DisplayName":"Obscure Indie","InstallLocation":"D:\\Epic Games\\ObscureIndie","LaunchExecutable":"Obscure.exe","AppCategories":["games"]}"#,
            )
            .unwrap(),
        );
        d.libs.insert(
            parse_epic_item(
                r#"{"AppName":"Known","DisplayName":"Known Indie (Epic)","InstallLocation":"D:\\Epic Games\\KnownIndie","LaunchExecutable":"KnownIndie.exe","AppCategories":["games"]}"#,
            )
            .unwrap(),
        );
        let mut v = std::collections::HashMap::new();
        v.insert("gameID".to_string(), "1207658924".to_string());
        v.insert("gameName".to_string(), "Unreal Tournament 2004".to_string());
        v.insert("path".to_string(), r"C:\GOG Games\UT2004".to_string());
        d.libs.insert(parse_gog_values("1207658924", &v).unwrap());

        let procs = vec![
            // in an Epic install, but on no list: not detected (the full-screen
            // prompt offers it instead)
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
            // on the detectable list and inside an Epic install: keeps the old epic: id
            p(
                3,
                "KnownIndie.exe",
                Some(r"D:\Epic Games\KnownIndie\KnownIndie.exe"),
            ),
        ];
        let found = d.detect(&procs, None);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].game_id, "ut2004");
        assert_eq!(found[0].source, DetectSource::Signature);
        assert_eq!(found[1].game_id, "epic:Known");
        assert_eq!(found[1].source, DetectSource::Detectable);
        // ...and outside any install it gets the list's slug
        let loose = vec![p(4, "KnownIndie.exe", Some(r"C:\Games\KnownIndie.exe"))];
        let f = d.detect(&loose, None);
        assert_eq!(
            (f[0].game_id.as_str(), f[0].name.as_str()),
            ("known_indie", "Known Indie")
        );
        assert!(d.candidates(&procs).iter().any(|c| c.exe == "Obscure.exe"));

        d.excluded_games.insert("ut2004".into());
        assert_eq!(d.detect(&procs, None).len(), 1);
    }

    /// WoW follows Discord's list as-is (0.2.0-dev.14), against the real bundled lists.
    #[test]
    fn wow_flavours_and_star_citizen_real_paths() {
        let d = det();
        let one = |name: &str, path: &str| {
            let f = d.detect(&[p(1, name, Some(path))], None);
            assert_eq!(f.len(), 1, "{path}: {f:?}");
            (f[0].game_id.clone(), f[0].name.clone())
        };
        let wow = r"E:\Battle.net Games\World of Warcraft";
        let cases = [
            ("_retail_", "Wow.exe", "wow", "World of Warcraft"),
            ("_ptr_", "Wow.exe", "wow", "World of Warcraft"),
            ("_beta_", "Wow-64.exe", "wow", "World of Warcraft"),
            (
                "_classic_",
                "WowClassic.exe",
                "wow_classic",
                "World of Warcraft Classic",
            ),
            (
                "_classic_era_",
                "WowClassic.exe",
                "wow_classic",
                "World of Warcraft Classic",
            ),
            (
                "_anniversary_",
                "WowClassic.exe",
                "wow_classic",
                "World of Warcraft Classic",
            ),
            // the Forever beta's WowB.exe is never Forever (retired; Blizzard may reuse it)
            (
                "_classic_beta_",
                "WowB.exe",
                "exe:wowb.exe",
                "Unlisted game",
            ),
            ("_forever_", "WowB.exe", "exe:wowb.exe", "Unlisted game"),
            // Forever's real client: retail's exe name in its own folder
            (
                "_forever_",
                "Wow.exe",
                "wow_forever",
                "World of Warcraft: Forever",
            ),
            (
                "_forever_",
                "Wow-64.exe",
                "wow_forever",
                "World of Warcraft: Forever",
            ),
        ];
        for (dir, exe, id, name) in cases {
            assert_eq!(
                one(exe, &format!(r"{wow}\{dir}\{exe}")),
                (id.to_string(), name.to_string()),
                "{dir}"
            );
        }
        // World of Warcraft Public Test (WowT.exe) is not tracked
        assert!(d
            .detect(
                &[p(1, "WowT.exe", Some(&format!(r"{wow}\_ptr_\WowT.exe")))],
                None
            )
            .is_empty());
        assert_eq!(
            one(
                "StarCitizen.exe",
                r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Bin64\StarCitizen.exe"
            ),
            ("star_citizen".into(), "Star Citizen".into())
        );
        assert_eq!(
            one(
                "StarCitizen.exe",
                r"D:\RSI\StarCitizen\PTU\Bin64\StarCitizen.exe"
            )
            .0,
            "star_citizen"
        );
        // two flavours at once are two games
        let both = d.detect(
            &[
                p(
                    1,
                    "Wow.exe",
                    Some(r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe"),
                ),
                p(
                    2,
                    "Wow.exe",
                    Some(r"E:\Battle.net Games\World of Warcraft\_forever_\Wow.exe"),
                ),
            ],
            None,
        );
        assert_eq!(
            both.iter().map(|d| d.game_id.as_str()).collect::<Vec<_>>(),
            vec!["wow", "wow_forever"]
        );
        // retail and Forever both run as Wow.exe: still two games
        let same_exe = d.detect(
            &[
                p(1, "Wow.exe", Some(&format!(r"{wow}\_retail_\Wow.exe"))),
                p(2, "Wow.exe", Some(&format!(r"{wow}\_forever_\Wow.exe"))),
            ],
            None,
        );
        assert_eq!(
            same_exe
                .iter()
                .map(|d| d.game_id.as_str())
                .collect::<Vec<_>>(),
            vec!["wow", "wow_forever"]
        );
    }

    #[test]
    fn detectable_list_fills_the_long_tail() {
        let d = det();
        // not one of our signatures, but on Discord's list
        let f = d.detect(
            &[p(
                1,
                "Brawlhalla.exe",
                Some(r"C:\Games\Brawlhalla\Brawlhalla.exe"),
            )],
            None,
        );
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(f[0].source, DetectSource::Detectable);
        assert_eq!(f[0].name, "Brawlhalla");
        // our signature still wins (same id as before)
        let cs = d.detect(&[p(2, "cs2.exe", Some(r"C:\S\cs2.exe"))], None);
        assert_eq!(
            (cs[0].game_id.as_str(), cs[0].source),
            ("cs2", DetectSource::Signature)
        );
        // known desktop apps are never games, whatever a list says
        let mut d2 = with_list(det(), vec![dgame("9", "vlc_game", "VLC", &["vlc.exe"])]);
        assert!(d2
            .detect(&[p(3, "vlc.exe", Some(r"C:\VLC\vlc.exe"))], None)
            .is_empty());
        // inside a Steam library the Steam id is kept
        d2.detectable = Arc::new(DetectableDb::from_file(crate::detectable::DetectableFile {
            v: 1,
            updated: String::new(),
            games: vec![dgame(
                "10",
                "tiny_indie",
                "Tiny Indie (Discord)",
                &["tinyindie.exe"],
            )],
        }));
        let t = d2.detect(
            &[p(
                4,
                "TinyIndie.exe",
                Some(r"D:\SteamLibrary\steamapps\common\TinyIndie\TinyIndie.exe"),
            )],
            None,
        );
        assert_eq!(t[0].game_id, "steam:999999");
        assert_eq!(t[0].name, "Tiny Indie Game");
    }

    /// The server's detectable list (dev.15 shape: `starcitizen.exe` only) matches the
    /// RSI Launcher's install path, with and without the RSI install in the library list.
    #[test]
    fn star_citizen_from_server_list_and_rsi_install() {
        let json = br#"{"v":1,"updated":"2026-10-04T17:35:24Z","games":[{"i":"452295596917784577","s":"star_citizen","n":"Star Citizen","e":["starcitizen.exe"]}]}"#;
        let mut d = Detector::new(SignatureDb::builtin(), SteamIndex::default());
        d.overrides = crate::overrides::Overrides::default();
        d.detectable =
            std::sync::Arc::new(crate::detectable::DetectableDb::from_json(json).unwrap());
        let path =
            r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE\Bin64\StarCitizen.exe";
        let m = d
            .detectable
            .match_process("StarCitizen.exe", Some(path))
            .unwrap();
        assert_eq!(m.game.slug, "star_citizen");
        let ids = |d: &Detector| {
            d.detect(
                &[ProcInfo {
                    pid: 1,
                    name: "StarCitizen.exe".into(),
                    path: Some(path.into()),
                }],
                None,
            )
            .into_iter()
            .map(|x| x.game_id)
            .collect::<Vec<_>>()
        };
        assert_eq!(ids(&d), vec!["star_citizen".to_string()]);
        d.libs.insert(InstalledGame {
            store: Launcher::Rsi,
            key: "StarCitizen".into(),
            name: "Star Citizen".into(),
            install_dir: r"C:\Program Files\Roberts Space Industries\StarCitizen\LIVE".into(),
            launch_exe: Some(r"Bin64\StarCitizen.exe".into()),
            prefix: crate::libraries::make_prefix(
                r"C:\Program Files\Roberts Space Industries\StarCitizen",
            ),
            sig_id: Some("star_citizen".into()),
            launch_code: None,
        });
        assert_eq!(ids(&d), vec!["star_citizen".to_string()]);
    }
}
