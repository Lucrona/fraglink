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
    /// Tracked play time on this PC (seconds) and the last session's end, for sorting.
    pub total_seconds: i64,
    pub last_played: Option<i64>,
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

/// True when `p` is inside one of the game's flavour folders (or it has none).
fn hint_ok(hints: &[String], p: &str) -> bool {
    if hints.is_empty() {
        return true;
    }
    let n = crate::signatures::normalize_path(p);
    hints.iter().any(|h| n.contains(h.as_str()))
}

/// beta.5: games sharing one install root (WoW retail / Forever / PTR all use
/// `Wow.exe` under `World of Warcraft\`) look in their own flavour folder first and
/// never take an exe from another flavour's folder.
fn found_exe_hinted(folder: Option<&str>, names: &[String], hints: &[String]) -> Option<String> {
    let folder = folder?;
    for h in hints {
        let sep = if folder.contains('\\') { "\\" } else { "/" };
        let sub = format!(
            "{}{sep}{}",
            folder.trim_end_matches(['\\', '/']),
            h.trim_matches('/')
        );
        if std::path::Path::new(&sub).is_dir() {
            if let Some(x) = found_exe(Some(&sub), names) {
                return Some(x);
            }
        }
    }
    found_exe(Some(folder), names).filter(|x| hint_ok(hints, x))
}

/// beta.5: the exe remembered from a run (`exe_path:` setting) is never forgotten, so
/// an uninstalled game kept showing as installed. Check it is still there.
fn exe_exists(p: &str) -> bool {
    std::path::Path::new(p).is_file()
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
        // a saved exe outside the game's flavour folder (e.g. WoW retail's
        // `_retail_` vs Forever's `_forever_\Wow.exe`) belongs to another entry
        let exe = set(EXE_KEY, &g.id).filter(|p| hint_ok(&g.path_hints, p));
        let (installed, source, path) = if let Some(a) = app {
            (true, "steam", Some(a.dir.clone()))
        } else if let Some(l) = lib {
            (true, l.store.as_str(), Some(l.install_dir.clone()))
        } else if exe.as_deref().is_some_and(exe_exists) {
            // beta.5: a remembered exe only counts while it is still on disk
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
                    found_exe_hinted(
                        path.as_deref().filter(|_| source != "seen"),
                        &names,
                        &g.path_hints,
                    )
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
            total_seconds: row.map_or(0, |r| r.total_seconds),
            last_played: row.and_then(|r| r.last_played),
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
            total_seconds: rows.get(id.as_str()).map_or(0, |r| r.total_seconds),
            last_played: rows.get(id.as_str()).and_then(|r| r.last_played),
            id,
        });
    }
    for r in games {
        // beta.5: a game the user added and then removed
        if seen_ids.contains(&r.id) || r.source == "removed" {
            continue;
        }
        let exe = set(EXE_KEY, &r.id);
        let steam_app = r.steam_appid.and_then(|a| steam.get(a));
        out.push(Entry {
            id: r.id.clone(),
            name: r.name.clone(),
            genre: None,
            installed: steam_app.is_some() || exe.as_deref().is_some_and(exe_exists),
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
            total_seconds: r.total_seconds,
            last_played: r.last_played,
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

/// Folder key for comparing install paths (case, slashes, trailing separator).
fn folder_key(p: &str) -> Option<String> {
    let p = p.trim();
    if p.is_empty() || p.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    Some(p.replace('/', "\\").trim_end_matches('\\').to_lowercase())
}

/// Store-only ids (`steam:793460`, `epic:..`) that no signature claims.
fn is_store_id(id: &str) -> bool {
    id.contains(':')
}

/// Fold `from` into `into`: keep the richer install info, add up play time.
fn absorb(into: &mut Entry, from: Entry) {
    if !into.installed && from.installed {
        into.installed = true;
        into.source = from.source.clone();
    }
    if into.source == "tracked" && !from.source.is_empty() && from.source != "tracked" {
        into.source = from.source.clone();
    }
    into.steam_appid = into.steam_appid.or(from.steam_appid);
    if into.game_path.is_none() {
        into.game_path = from.game_path;
    }
    if into.detect_path.is_empty() {
        into.detect_path = from.detect_path;
    }
    if into.launch_args.is_empty() {
        into.launch_args = from.launch_args;
    }
    if into.genre.is_none() {
        into.genre = from.genre;
    }
    into.excluded |= from.excluded;
    into.total_seconds += from.total_seconds;
    into.last_played = into.last_played.max(from.last_played);
}

/// beta.5: one row per game in Options > Games. Applies the server's detection
/// aliases (e.g. `steam:793460` -> `112_operator`) and folds a store-only entry
/// into another entry installed in the same folder. Per-game settings saved under
/// the old id (game path, launch args) carry over when the kept entry has none.
pub fn merge_dupes(out: Vec<Entry>, ov: &crate::overrides::Overrides) -> Vec<Entry> {
    let mut kept: Vec<Entry> = Vec::with_capacity(out.len());
    let mut pending: Vec<Entry> = vec![];
    for e in out {
        if ov.resolve(&e.id) != e.id {
            pending.push(e);
        } else {
            kept.push(e);
        }
    }
    for mut e in pending {
        let to = ov.resolve(&e.id).to_string();
        if let Some(k) = kept.iter_mut().find(|k| k.id.eq_ignore_ascii_case(&to)) {
            absorb(k, e);
        } else {
            e.id = to;
            kept.push(e);
        }
    }
    // same install folder: a store-only id folds into the signature / other entry
    let mut i = 0;
    while i < kept.len() {
        let key = kept[i]
            .game_path
            .as_deref()
            .filter(|_| kept[i].installed && is_store_id(&kept[i].id))
            .and_then(folder_key);
        let target = key.and_then(|key| {
            kept.iter().enumerate().position(|(j, k)| {
                j != i
                    && k.installed
                    && !(is_store_id(&k.id) && j > i)
                    && k.game_path.as_deref().and_then(folder_key).as_deref() == Some(&key)
            })
        });
        if let Some(j) = target {
            let e = kept.remove(i);
            let j = if j > i { j - 1 } else { j };
            absorb(&mut kept[j], e);
        } else {
            i += 1;
        }
    }
    kept.sort_by(|a, b| {
        b.installed
            .cmp(&a.installed)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    kept
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
            Launcher::Rsi => {
                // through the RSI Launcher (it logs in and patches first), else the game exe
                if let Some(rsi) = &libs.rsi_launcher_exe {
                    return Some(Launch::Exe {
                        path: rsi.clone(),
                        args: vec![],
                    });
                }
                if let Some(x) = &l.launch_exe {
                    return Some(Launch::Exe {
                        path: exe_path(x),
                        args,
                    });
                }
            }
            Launcher::Gog
            | Launcher::Ea
            | Launcher::Xbox
            | Launcher::Folder
            | Launcher::Windows => {
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
    #[test]
    fn wow_retail_detect_path_skips_forever_folder() {
        let root = std::env::temp_dir().join(format!("fl-wowroot-{}", std::process::id()));
        for (d, f) in [
            ("_forever_", "Wow.exe"),
            ("_retail_", "Wow.exe"),
            ("_classic_", "WowClassic.exe"),
        ] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::write(root.join(d).join(f), b"x").unwrap();
        }
        let hints = vec!["/_retail_/".to_string(), "/_ptr_/".to_string()];
        let names = vec!["Wow.exe".to_string()];
        let r = root.to_string_lossy().into_owned();
        let got = found_exe_hinted(Some(&r), &names, &hints).unwrap();
        assert!(got.replace('\\', "/").contains("/_retail_/"), "{got}");
        // only a Forever install: retail finds nothing rather than Forever's exe
        std::fs::remove_dir_all(root.join("_retail_")).unwrap();
        assert_eq!(found_exe_hinted(Some(&r), &names, &hints), None);
        // a saved retail exe pointing into _forever_ is ignored
        let fe = root.join("_forever_").join("Wow.exe");
        assert!(!hint_ok(&hints, &fe.to_string_lossy()));
        // classic (no hints) still finds its own exe
        let c = found_exe_hinted(Some(&r), &["WowClassic.exe".to_string()], &[]).unwrap();
        assert!(c.contains("_classic_"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn uninstalled_seen_exe_is_not_installed() {
        let tmp = std::env::temp_dir().join(format!("fl-seen-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let exe = tmp.join("WowB.exe");
        std::fs::write(&exe, b"x").unwrap();
        let row = GameRow {
            id: "wow_forever".into(),
            name: "World of Warcraft: Forever".into(),
            source: "auto".into(),
            steam_appid: None,
            excluded: false,
            total_seconds: 3600,
            sessions: 1,
            last_played: None,
        };
        let mut set = BTreeMap::new();
        set.insert(
            format!("{EXE_KEY}wow_forever"),
            exe.to_string_lossy().into_owned(),
        );
        let sigs = SignatureDb::builtin();
        let build1 = || {
            build(
                &sigs,
                &SteamIndex::default(),
                &LibraryIndex::default(),
                std::slice::from_ref(&row),
                &set,
            )
        };
        assert!(build1()
            .iter()
            .any(|e| e.id == "wow_forever" && e.installed));
        std::fs::remove_dir_all(&tmp).unwrap();
        let e = build1()
            .into_iter()
            .find(|e| e.id == "wow_forever")
            .unwrap();
        assert!(!e.installed);
        assert_eq!(e.total_seconds, 3600, "hours kept");
    }

    #[test]
    fn merge_dupes_applies_aliases_and_folders() {
        let mk = |id: &str, name: &str, path: Option<&str>, secs: i64| Entry {
            id: id.into(),
            name: name.into(),
            genre: None,
            installed: path.is_some(),
            source: if path.is_some() {
                "steam".into()
            } else {
                "tracked".into()
            },
            steam_appid: None,
            game_path: path.map(Into::into),
            detect_path: String::new(),
            launch_args: String::new(),
            excluded: false,
            can_launch: false,
            total_seconds: secs,
            last_played: Some(secs),
        };
        let dir = r"E:\Steam\steamapps\common\112 Operator\";
        let mut a = mk("steam:793460", "112 Operator", Some(dir), 100);
        a.steam_appid = Some(793460);
        a.launch_args = "-windowed".into();
        let b = mk("112_operator", "112 Operator", None, 50);
        let ov = crate::overrides::Overrides::from_json(
            r#"{"schema":1,"version":"t","rules":[],"aliases":[{"from":"steam:793460","to":"112_operator"}]}"#,
        )
        .unwrap();
        let out = merge_dupes(vec![a.clone(), b.clone()], &ov);
        assert_eq!(out.len(), 1);
        let e = &out[0];
        assert_eq!(e.id, "112_operator");
        assert!(e.installed);
        assert_eq!(e.steam_appid, Some(793460));
        assert_eq!(e.launch_args, "-windowed");
        assert_eq!(e.total_seconds, 150);
        assert_eq!(e.game_path.as_deref(), Some(dir));

        // no alias: same folder still folds the store-only id into the signature
        let mut b2 = b.clone();
        b2.installed = true;
        b2.game_path = Some(r"e:/steam/steamapps/common/112 operator".into());
        let out = merge_dupes(vec![a.clone(), b2], &crate::overrides::Overrides::builtin());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "112_operator");

        // different games in different folders stay apart
        let c = mk("steam:1", "Other", Some(r"E:\x\"), 1);
        let out = merge_dupes(vec![a, c], &crate::overrides::Overrides::builtin());
        assert_eq!(out.len(), 2);
    }

    use super::*;

    #[test]
    fn installed_games_come_first_and_know_how_to_launch() {
        let sigs = SignatureDb::builtin();
        let mut steam = SteamIndex::default();
        steam.insert("C:/Steam", 570, "Dota 2", "dota 2 beta");
        let libs = LibraryIndex::default();
        let mut settings = BTreeMap::new();
        // the remembered exe has to exist on disk (beta.5)
        let wow_dir = std::env::temp_dir().join(format!("fl-wow-{}/_retail_", std::process::id()));
        std::fs::create_dir_all(&wow_dir).unwrap();
        let wow_exe = wow_dir.join("Wow.exe");
        std::fs::write(&wow_exe, b"x").unwrap();
        let wow_exe = wow_exe.to_string_lossy().into_owned();
        settings.insert(format!("{EXE_KEY}wow"), wow_exe.clone());
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
                path: wow_exe.clone(),
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
