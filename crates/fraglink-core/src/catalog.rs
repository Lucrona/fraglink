//! The game list behind Options > Games and Tools > Launch: every known game,
//! whether it looks installed on this PC, where, and how to start it.

use crate::libraries::{InstalledGame, Launcher, LibraryIndex};
use crate::signatures::SignatureDb;
use crate::steam::SteamIndex;
use crate::store::GameRow;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// Setting prefixes (per game id).
pub const EXE_KEY: &str = "exe_path:";
pub const PATH_KEY: &str = "game_path:";
pub const ARGS_KEY: &str = "launch_args:";

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub genre: Option<String>,
    pub installed: bool,
    /// steam / epic / gog / seen (ran here) / manual / tracked
    pub source: String,
    pub steam_appid: Option<u32>,
    /// Install folder or the exe the user picked.
    pub game_path: Option<String>,
    /// What detection looks for (exe path seen, else the exe names).
    pub detect_path: String,
    pub launch_args: String,
    pub excluded: bool,
    pub can_launch: bool,
}

/// A folder in normal Windows form for Options > Games: real casing (asked from the
/// file system when the folder exists), backslashes and a trailing `\`, e.g.
/// `E:\Steam\steamapps\common\Satisfactory\`. Non-Windows paths are left alone.
pub fn display_dir(p: &str) -> String {
    let p = p.trim();
    let win = p.contains('\\') || p.as_bytes().get(1) == Some(&b':');
    if !win {
        return p.to_string();
    }
    let mut s = std::fs::canonicalize(p)
        .ok()
        .map(|c| c.to_string_lossy().into_owned())
        .map(|c| {
            if let Some(r) = c.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{r}")
            } else {
                c.strip_prefix(r"\\?\").unwrap_or(&c).to_string()
            }
        })
        .unwrap_or_else(|| p.replace('/', "\\"));
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// The exe detection looks for, as found in the install folder (full path), if any.
fn found_exe(folder: Option<&str>, names: &[String]) -> Option<String> {
    crate::installs::find_exe(folder?, names)
}

/// How to start a game. Only built from catalog data, never from UI input.
#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    Url(String),
    Exe { path: String, args: Vec<String> },
}

pub fn build(
    sigs: &SignatureDb,
    steam: &SteamIndex,
    libs: &LibraryIndex,
    games: &[GameRow],
    settings: &BTreeMap<String, String>,
) -> Vec<Entry> {
    let set = |k: &str, id: &str| {
        settings
            .get(&format!("{k}{id}"))
            .cloned()
            .filter(|v| !v.is_empty())
    };
    let rows: HashMap<&str, &GameRow> = games.iter().map(|g| (g.id.as_str(), g)).collect();
    let mut out: Vec<Entry> = vec![];
    let mut seen_ids = std::collections::HashSet::new();
    for g in sigs.games() {
        let app = g.steam_appids.iter().find_map(|a| steam.get(*a));
        let lib = libs.games().iter().find(|l| {
            l.sig_id.as_deref() == Some(g.id.as_str()) || l.name.eq_ignore_ascii_case(&g.name)
        });
        let exe = set(EXE_KEY, &g.id);
        let (installed, source, path) = if let Some(a) = app {
            (true, "steam", Some(a.dir.clone()))
        } else if let Some(l) = lib {
            (true, l.store.as_str(), Some(l.install_dir.clone()))
        } else if exe.is_some() {
            (true, "seen", exe.clone())
        } else {
            (false, "", None)
        };
        let row = rows.get(g.id.as_str());
        out.push(Entry {
            id: g.id.clone(),
            name: g.name.clone(),
            genre: g.genre.clone(),
            installed,
            source: if installed {
                source.into()
            } else if row.is_some() {
                "tracked".into()
            } else {
                String::new()
            },
            steam_appid: app
                .map(|a| a.appid)
                .or_else(|| row.and_then(|r| r.steam_appid)),
            // dev.10: only the exe actually found (not the list of candidates), and
            // folders in normal Windows form
            detect_path: exe
                .clone()
                .filter(|_| source == "seen")
                .or_else(|| {
                    let names: Vec<String> = g.exes.iter().chain(&g.path_exes).cloned().collect();
                    found_exe(path.as_deref().filter(|_| source != "seen"), &names)
                })
                .or(exe.clone())
                .unwrap_or_default(),
            game_path: set(PATH_KEY, &g.id).or(path.map(|p| {
                if source == "seen" {
                    p
                } else {
                    display_dir(&p)
                }
            })),
            launch_args: set(ARGS_KEY, &g.id).unwrap_or_default(),
            excluded: row.is_some_and(|r| r.excluded),
            can_launch: false,
        });
        seen_ids.insert(g.id.clone());
    }
    // Epic / GOG installs with no signature, then manual tags / other tracked games
    for l in libs.games() {
        let id = l.fallback_id();
        if seen_ids.contains(&id)
            || l.sig_id.is_some()
            || out
                .iter()
                .any(|e| e.installed && e.name.eq_ignore_ascii_case(&l.name))
        {
            continue;
        }
        seen_ids.insert(id.clone());
        out.push(Entry {
            name: l.name.clone(),
            genre: None,
            installed: true,
            source: l.store.as_str().into(),
            steam_appid: None,
            game_path: set(PATH_KEY, &id).or(Some(display_dir(&l.install_dir))),
            detect_path: l
                .launch_exe
                .as_deref()
                .map(|x| crate::installs::join_display(&l.install_dir, x))
                .unwrap_or_default(),
            launch_args: set(ARGS_KEY, &id).unwrap_or_default(),
            excluded: rows.get(id.as_str()).is_some_and(|r| r.excluded),
            can_launch: false,
            id,
        });
    }
    for r in games {
        if seen_ids.contains(&r.id) {
            continue;
        }
        let exe = set(EXE_KEY, &r.id);
        let steam_app = r.steam_appid.and_then(|a| steam.get(a));
        out.push(Entry {
            id: r.id.clone(),
            name: r.name.clone(),
            genre: None,
            installed: steam_app.is_some() || exe.is_some(),
            source: if steam_app.is_some() {
                "steam".into()
            } else if r.source == "manual" {
                "manual".into()
            } else {
                "tracked".into()
            },
            steam_appid: r.steam_appid,
            game_path: set(PATH_KEY, &r.id)
                .or(steam_app.map(|a| display_dir(&a.dir)))
                .or(exe.clone()),
            detect_path: exe.unwrap_or_default(),
            launch_args: set(ARGS_KEY, &r.id).unwrap_or_default(),
            excluded: r.excluded,
            can_launch: false,
        });
    }
    for e in &mut out {
        e.can_launch = e.installed && launch_for(e, libs).is_some();
    }
    out.sort_by(|a, b| {
        b.installed
            .cmp(&a.installed)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn split_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_string).collect()
}

/// The launcher install behind a catalog entry (same launcher first).
fn lib_for<'a>(e: &Entry, libs: &'a LibraryIndex) -> Option<&'a InstalledGame> {
    let hit = |l: &&InstalledGame| {
        l.sig_id.as_deref() == Some(e.id.as_str())
            || l.fallback_id() == e.id
            || l.name.eq_ignore_ascii_case(&e.name)
    };
    let all = libs.games();
    all.iter()
        .filter(hit)
        .find(|l| l.store.as_str() == e.source)
        .or_else(|| all.iter().find(hit))
}

/// How to launch an installed entry: Steam and Epic through their launchers,
/// otherwise an .exe we know (picked by the user, seen running, or GOG's).
pub fn launch_for(e: &Entry, libs: &LibraryIndex) -> Option<Launch> {
    if !e.installed {
        return None;
    }
    let exe_like = |p: &str| p.to_ascii_lowercase().ends_with(".exe");
    if let Some(p) = e.game_path.as_deref().filter(|p| exe_like(p)) {
        return Some(Launch::Exe {
            path: p.into(),
            args: split_args(&e.launch_args),
        });
    }
    if let Some(a) = e.steam_appid.filter(|_| e.source == "steam") {
        return Some(Launch::Url(format!("steam://rungameid/{a}")));
    }
    if let Some(l) = lib_for(e, libs) {
        let args = split_args(&e.launch_args);
        let exe_path = |x: &str| {
            format!(
                "{}\\{}",
                l.install_dir.trim_end_matches(['\\', '/']),
                x.replace('/', "\\")
            )
        };
        match l.store {
            Launcher::Epic => {
                return Some(Launch::Url(format!(
                    "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                    l.key
                )))
            }
            Launcher::Battlenet => {
                if let Some(code) = &l.launch_code {
                    return Some(match &libs.battlenet_exe {
                        Some(bnet) => Launch::Exe {
                            path: bnet.clone(),
                            args: vec![format!("--exec=launch {code}")],
                        },
                        None => Launch::Url(format!("battlenet://{code}")),
                    });
                }
            }
            Launcher::Ubisoft => {
                if let Some(id) = &l.launch_code {
                    return Some(Launch::Url(format!("uplay://launch/{id}/0")));
                }
            }
            Launcher::Gog | Launcher::Ea | Launcher::Xbox | Launcher::Windows => {
                if let Some(x) = &l.launch_exe {
                    return Some(Launch::Exe {
                        path: exe_path(x),
                        args,
                    });
                }
            }
        }
    }
    if exe_like(&e.detect_path) && (e.detect_path.contains('\\') || e.detect_path.contains('/')) {
        return Some(Launch::Exe {
            path: e.detect_path.clone(),
            args: split_args(&e.launch_args),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_games_come_first_and_know_how_to_launch() {
        let sigs = SignatureDb::builtin();
        let mut steam = SteamIndex::default();
        steam.insert("C:/Steam", 570, "Dota 2", "dota 2 beta");
        let libs = LibraryIndex::default();
        let mut settings = BTreeMap::new();
        settings.insert(
            format!("{EXE_KEY}wow"),
            "C:\\Games\\WoW\\_retail_\\Wow.exe".to_string(),
        );
        settings.insert(format!("{ARGS_KEY}wow"), "-console -windowed".to_string());
        let games = vec![GameRow {
            id: "manual:myindie".into(),
            name: "My Indie".into(),
            source: "manual".into(),
            steam_appid: None,
            excluded: true,
            total_seconds: 60,
            sessions: 1,
            last_played: None,
        }];
        let c = build(&sigs, &steam, &libs, &games, &settings);
        let dota = c.iter().find(|e| e.id == "dota2").unwrap();
        assert!(dota.installed && dota.can_launch && dota.source == "steam");
        assert_eq!(
            launch_for(dota, &libs),
            Some(Launch::Url("steam://rungameid/570".into()))
        );
        let wow = c.iter().find(|e| e.id == "wow").unwrap();
        assert_eq!(wow.source, "seen");
        assert_eq!(
            launch_for(wow, &libs),
            Some(Launch::Exe {
                path: "C:\\Games\\WoW\\_retail_\\Wow.exe".into(),
                args: vec!["-console".into(), "-windowed".into()]
            })
        );
        let indie = c.iter().find(|e| e.id == "manual:myindie").unwrap();
        assert!(!indie.installed && indie.excluded && !indie.can_launch);
        // installed first
        let first_uninstalled = c.iter().position(|e| !e.installed).unwrap();
        assert!(c[..first_uninstalled].iter().all(|e| e.installed));
        assert!(c[first_uninstalled..].iter().all(|e| !e.installed));
    }

    #[test]
    fn options_show_windows_paths_and_the_exe_found() {
        let sigs = SignatureDb::builtin();
        let mut steam = SteamIndex::default();
        steam.insert("E:/Steam", 526870, "Satisfactory", "Satisfactory");
        let c = build(
            &sigs,
            &steam,
            &LibraryIndex::default(),
            &[],
            &BTreeMap::new(),
        );
        let s = c.iter().find(|e| e.id == "satisfactory").unwrap();
        // real casing, backslashes, trailing backslash (not the lowercase match prefix)
        assert_eq!(
            s.game_path.as_deref(),
            Some(r"E:\Steam\steamapps\common\Satisfactory\")
        );
        // folder doesn't exist here: nothing found, and no list of candidates
        assert_eq!(s.detect_path, "");
        let rust = c.iter().find(|e| e.id == "rust").unwrap();
        assert!(!rust.installed && rust.detect_path.is_empty());

        // a real folder: the exe actually present is shown with its full path
        let tmp = std::env::temp_dir().join(format!("fl-cat-{}", std::process::id()));
        let bin = tmp.join("steamapps/common/Satisfactory/FactoryGame/Binaries/Win64");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("FactoryGameSteam-Win64-Shipping.exe"), b"").unwrap();
        std::fs::write(
            tmp.join("steamapps/common/Satisfactory/FactoryGame.exe"),
            b"",
        )
        .unwrap();
        let mut steam = SteamIndex::default();
        steam.insert(
            &tmp.to_string_lossy(),
            526870,
            "Satisfactory",
            "Satisfactory",
        );
        let dir = steam.get(526870).unwrap().dir.clone();
        crate::installs::clear_exe_cache();
        let found = crate::installs::find_exe(
            &dir,
            &[
                "FactoryGame-Win64-Shipping.exe".into(),
                "FactoryGameSteam-Win64-Shipping.exe".into(),
            ],
        );
        let found = found.unwrap_or_default().replace('\\', "/");
        assert!(
            found.ends_with("FactoryGame/Binaries/Win64/FactoryGameSteam-Win64-Shipping.exe"),
            "{found}"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn battlenet_games_launch_through_the_battlenet_app() {
        let sigs = SignatureDb::builtin();
        let mut libs = LibraryIndex::default();
        libs.insert(
            crate::installs::battlenet_game(
                "wow_enus",
                "World of Warcraft",
                r"C:\Program Files (x86)\World of Warcraft",
            )
            .unwrap(),
        );
        let cat = build(&sigs, &SteamIndex::default(), &libs, &[], &BTreeMap::new());
        let wow = cat.iter().find(|e| e.id == "wow").unwrap();
        assert!(wow.installed && wow.can_launch);
        assert_eq!(wow.source, "battlenet");
        assert_eq!(
            launch_for(wow, &libs),
            Some(Launch::Url("battlenet://WoW".into()))
        );
        libs.battlenet_exe = Some(r"C:\BNet\Battle.net.exe".into());
        assert_eq!(
            launch_for(wow, &libs),
            Some(Launch::Exe {
                path: r"C:\BNet\Battle.net.exe".into(),
                args: vec!["--exec=launch WoW".into()]
            })
        );
    }
}
