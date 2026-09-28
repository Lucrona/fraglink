//! The game list behind Options > Games and Tools > Launch: every known game,
//! whether it looks installed on this PC, where, and how to start it.

use crate::libraries::{Launcher, LibraryIndex};
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
        let lib = libs
            .games()
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(&g.name));
        let exe = set(EXE_KEY, &g.id);
        let (installed, source, path) = if let Some(a) = app {
            (true, "steam", Some(a.prefix.clone()))
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
            game_path: set(PATH_KEY, &g.id).or(path),
            detect_path: exe.unwrap_or_else(|| {
                g.exes
                    .iter()
                    .chain(&g.path_exes)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }),
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
            game_path: set(PATH_KEY, &id).or(Some(l.install_dir.clone())),
            detect_path: l.launch_exe.clone().unwrap_or_default(),
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
                .or(steam_app.map(|a| a.prefix.clone()))
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
    if let Some(l) = libs
        .games()
        .iter()
        .find(|l| l.fallback_id() == e.id || l.name.eq_ignore_ascii_case(&e.name))
    {
        match l.store {
            Launcher::Epic => {
                return Some(Launch::Url(format!(
                    "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                    l.key
                )))
            }
            Launcher::Gog => {
                if let Some(x) = &l.launch_exe {
                    let p = format!("{}\\{}", l.install_dir.trim_end_matches(['\\', '/']), x);
                    return Some(Launch::Exe {
                        path: p,
                        args: split_args(&e.launch_args),
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
}
