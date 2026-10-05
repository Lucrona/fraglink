//! "Scan for games": a light walk of the folders games usually live in, with every
//! `.exe` found checked against the same lists the running detector uses (Fraglink's
//! overrides and approved games, the signature list, the full detectable list).
//!
//! Local only: the results are cached in the local settings (`folder_scan`) and
//! feed Options > Games; nothing is uploaded. The walk is bounded (depth, folder
//! count, time) and throttled so it can run in the background on first start.

use crate::detector::{DetectSource, Detector};
use crate::libraries::{make_prefix, prefix_is_safe, InstalledGame, Launcher, LibraryIndex};
use crate::steam::SteamIndex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Setting holding the cached results (JSON [`ScanCache`]).
pub const CACHE_KEY: &str = "folder_scan";

#[derive(Debug, Clone)]
pub struct ScanOpts {
    /// Folder levels below each root (an exe in `<root>\a\b\c\d\` is depth 4).
    pub max_depth: usize,
    pub max_dirs: usize,
    pub time_limit: Duration,
    /// Pause `throttle` after every `throttle_every` folders.
    pub throttle_every: usize,
    pub throttle: Duration,
}

impl Default for ScanOpts {
    fn default() -> Self {
        ScanOpts {
            max_depth: 5,
            max_dirs: 60_000,
            time_limit: Duration::from_secs(60),
            throttle_every: 64,
            throttle: Duration::from_millis(2),
        }
    }
}

/// One game found on disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Found {
    /// Fraglink game id (same id the running detector would use).
    pub id: String,
    pub name: String,
    /// Full path of the exe that matched.
    pub exe: String,
    /// The game's folder (the exe's folder, minus `Bin64` / `Binaries\Win64`-style parts).
    pub dir: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanCache {
    /// Unix seconds of the scan.
    pub at: i64,
    pub dirs: usize,
    pub found: Vec<Found>,
}

/// Folder names never worth entering.
const SKIP_DIRS: &[&str] = &[
    "windows",
    "winnt",
    "system32",
    "syswow64",
    "$recycle.bin",
    "$windows.~bt",
    "$windows.~ws",
    "system volume information",
    "recovery",
    "perflogs",
    "programdata",
    "users",
    "appdata",
    "node_modules",
    ".git",
    ".svn",
    "__pycache__",
    "windowsapps",
    "common files",
    "windows defender",
    "windows nt",
    "windows mail",
    "windows media player",
    "windows photo viewer",
    "windows sidebar",
    "windowspowershell",
    "internet explorer",
    "microsoft office",
    "microsoft",
    "msbuild",
    "reference assemblies",
    "dotnet",
    "nvidia corporation",
    "amd",
    "intel",
    "drivers",
    "redist",
    "_commonredist",
    "commonredist",
    "redistributables",
    "directx",
    "vcredist",
    "prereqs",
    "installers",
    "shadercache",
    "downloading",
    "temp",
    "tmp",
    "workshop",
    "compatdata",
    "logs",
    "crashes",
    "crashreports",
    "screenshots",
    "steamworks shared",
    "engine",
    "mono",
    "monobleedingedge",
    "jre",
    "jdk",
    "python",
    "cef",
    "webhelper",
    "battleye",
    "easyanticheat",
    "easyanticheat_eos",
    "_data",
];

/// Exe names that are never the game itself (launchers, installers, crash tools).
fn is_helper_exe(exe_l: &str) -> bool {
    const BAD: &[&str] = &[
        "launcher",
        "unins",
        "setup",
        "install",
        "crash",
        "redist",
        "updater",
        "update",
        "patcher",
        "report",
        "helper",
        "prereq",
        "dxsetup",
        "vc_redist",
        "cefprocess",
        "bootstrap",
        "anticheat",
        "battleye",
        "be_service",
        "eac",
        "uploader",
        "benchmark",
        "server",
        "editor",
        "config",
        "settings",
    ];
    BAD.iter().any(|b| exe_l.contains(b))
}

/// Public test / PTR builds are not counted as installed games.
fn is_test_path(path_n: &str, name: &str) -> bool {
    crate::detectable::is_test_build(name)
        || path_n.contains("public test")
        || path_n.contains("/_ptr_/")
        || path_n.contains("/ptr/")
        || path_n.contains("_ptr/")
}

/// Folders worth entering under `Program Files`: game stores and publishers.
pub fn is_gameish(name: &str) -> bool {
    const WORDS: &[&str] = &[
        "game",
        "steam",
        "epic",
        "gog",
        "electronic arts",
        "ea ",
        "origin",
        "ubisoft",
        "riot",
        "roberts space",
        "rsi",
        "blizzard",
        "battle.net",
        "rockstar",
        "bethesda",
        "xbox",
        "wargaming",
        "bungie",
        "square enix",
        "sega",
        "bandai",
        "2k",
        "cd projekt",
        "valve",
        "activision",
        "minecraft",
        "mojang",
        "nexon",
        "garena",
        "hoyoverse",
        "mihoyo",
        "arena",
        "warframe",
        "digital extremes",
        "grinding gear",
        "path of exile",
        "jagex",
        "runescape",
        "league of legends",
        "valorant",
        "star citizen",
        "world of",
        "call of duty",
        "pubg",
        "krafton",
        "nintendo",
        "emulator",
        "retroarch",
        "itch",
        "playnite",
        "amazon games",
        "legacy games",
        "humble",
    ];
    let n = format!("{} ", name.to_ascii_lowercase());
    WORDS.iter().any(|w| n.contains(w))
}

/// Where to look on this PC: on every fixed drive `X:\Games`, any top-level folder
/// with "games" in its name, game-ish folders in both Program Files, the usual
/// launcher folders, plus `extra` (Steam libraries, launcher library folders).
pub fn default_roots(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![];
    if let Ok(r) = std::env::var("FRAGLINK_SCAN_ROOTS") {
        roots.extend(r.split(';').filter(|s| !s.is_empty()).map(PathBuf::from));
        roots.extend(extra.iter().cloned());
        return dedupe_roots(roots);
    }
    for d in fixed_drives() {
        let drive = PathBuf::from(format!("{d}:\\"));
        if let Ok(rd) = std::fs::read_dir(&drive) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if !e.path().is_dir() {
                    continue;
                }
                let launcherish = [
                    "riot games",
                    "roberts space industries",
                    "rsi",
                    "xboxgames",
                    "steamlibrary",
                    "steam",
                    "epic games",
                    "gog games",
                    "gog galaxy",
                    "ea games",
                    "origin games",
                    "ubisoft",
                    "ubisoft game launcher",
                    "battle.net",
                    "blizzard",
                ];
                if name.contains("games") || name == "games" || launcherish.contains(&name.as_str())
                {
                    roots.push(e.path());
                }
            }
        }
        for pf in ["Program Files", "Program Files (x86)"] {
            let base = drive.join(pf);
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().into_owned();
                    if e.path().is_dir() && is_gameish(&n) {
                        roots.push(e.path());
                    }
                }
            }
        }
    }
    roots.extend(extra.iter().cloned());
    dedupe_roots(roots)
}

fn dedupe_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    roots
        .into_iter()
        .filter(|r| r.is_dir() && seen.insert(r.to_string_lossy().to_ascii_lowercase()))
        .collect()
}

#[cfg(windows)]
fn fixed_drives() -> Vec<char> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_FIXED: u32 = 3;
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i as u8) as char)
        .filter(|c| *c >= 'C')
        .filter(|c| {
            let w: Vec<u16> = format!("{c}:\\").encode_utf16().chain([0]).collect();
            unsafe { GetDriveTypeW(w.as_ptr()) == DRIVE_FIXED }
        })
        .collect()
}

#[cfg(not(windows))]
fn fixed_drives() -> Vec<char> {
    vec![]
}

/// Every `.exe` below the roots (bounded and throttled). Returns (exe paths, folders visited).
pub fn walk_exes(roots: &[PathBuf], opts: &ScanOpts) -> (Vec<String>, usize) {
    let start = Instant::now();
    let mut visited: HashSet<String> = HashSet::new();
    let mut exes = vec![];
    let mut dirs = 0usize;
    // breadth-first per root so the shallow (likely) folders are seen first
    for root in roots {
        let mut queue = std::collections::VecDeque::from([(root.clone(), 0usize)]);
        while let Some((dir, depth)) = queue.pop_front() {
            if dirs >= opts.max_dirs || start.elapsed() >= opts.time_limit {
                log::info!("folder scan: stopped at {dirs} folders");
                return (exes, dirs);
            }
            if !visited.insert(dir.to_string_lossy().to_ascii_lowercase()) {
                continue;
            }
            dirs += 1;
            if opts.throttle_every > 0 && dirs.is_multiple_of(opts.throttle_every) {
                std::thread::sleep(opts.throttle);
            }
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in rd.flatten() {
                let Ok(ft) = e.file_type() else { continue };
                let name = e.file_name().to_string_lossy().to_ascii_lowercase();
                if ft.is_dir() {
                    if depth < opts.max_depth
                        && !name.starts_with('.')
                        && !SKIP_DIRS.contains(&name.as_str())
                    {
                        queue.push_back((e.path(), depth + 1));
                    }
                } else if ft.is_file() && name.ends_with(".exe") && !is_helper_exe(&name) {
                    exes.push(e.path().to_string_lossy().into_owned());
                }
            }
        }
    }
    (exes, dirs)
}

/// The game folder for an exe: its folder, minus `Bin64`, `Binaries\Win64`, … parts.
pub fn game_dir(exe: &Path) -> PathBuf {
    let mut d = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    for _ in 0..3 {
        let n = d
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let binish = matches!(
            n.as_str(),
            "bin"
                | "bin32"
                | "bin64"
                | "binaries"
                | "win32"
                | "win64"
                | "x64"
                | "x86"
                | "retail"
                | "_retail_"
                | "game"
                | "wingdk"
                | "shipping"
        );
        match d.parent() {
            Some(p) if binish && p.parent().is_some() => d = p.to_path_buf(),
            _ => break,
        }
    }
    d
}

/// Check each exe like a running process would be; keep one hit per game.
pub fn classify(det: &Detector, exes: &[String]) -> Vec<Found> {
    // main release channels and shallow folders first (LIVE before PTU, `_retail_` before `_beta_`)
    let rank = |e: &String| {
        let n = crate::signatures::normalize_path(e);
        let side = [
            "/ptu/",
            "/eptu/",
            "/tech-preview/",
            "/hotfix/",
            "_beta_",
            "_ptr_",
            "/beta/",
            "/test/",
        ]
        .iter()
        .any(|w| n.contains(w));
        (side, n.matches('/').count())
    };
    let mut exes: Vec<&String> = exes.iter().collect();
    exes.sort_by_key(|e| rank(e));
    let mut out: Vec<Found> = vec![];
    for exe in exes {
        let Some(d) = det.classify_file(exe) else {
            continue;
        };
        if !matches!(
            d.source,
            DetectSource::Manual
                | DetectSource::Override
                | DetectSource::Signature
                | DetectSource::Detectable
        ) || det.excluded_games.contains(&d.game_id)
        {
            continue;
        }
        let path_n = crate::signatures::normalize_path(exe);
        if is_test_path(&path_n, &d.name) {
            continue;
        }
        if out.iter().any(|f| f.id == d.game_id) {
            continue;
        }
        let dir = game_dir(Path::new(exe));
        out.push(Found {
            id: d.game_id,
            name: d.name,
            exe: exe.clone(),
            dir: dir.to_string_lossy().into_owned(),
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Add cached finds to the install list, skipping games a launcher or Steam already
/// reported and exes that are gone.
pub fn merge_into(
    libs: &mut LibraryIndex,
    steam: &SteamIndex,
    sigs: &crate::signatures::SignatureDb,
    found: &[Found],
) -> usize {
    let mut n = 0;
    for f in found {
        if !Path::new(&f.exe).is_file()
            || steam.app_for_path(&f.exe).is_some()
            || libs
                .game_for_path(&f.exe)
                .is_some_and(|g| g.store != Launcher::Folder)
            || libs.games().iter().any(|g| {
                g.store != Launcher::Folder
                    && (g.sig_id.as_deref() == Some(f.id.as_str()) || g.fallback_id() == f.id)
            })
            || steam_has_sig(steam, sigs, &f.id)
        {
            continue;
        }
        let prefix = make_prefix(&f.dir);
        if !prefix_is_safe(&prefix) {
            continue;
        }
        let rel = Path::new(&f.exe)
            .strip_prefix(&f.dir)
            .map(|r| r.to_string_lossy().into_owned())
            .unwrap_or_else(|_| {
                Path::new(&f.exe)
                    .file_name()
                    .map(|x| x.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        libs.insert(InstalledGame {
            store: Launcher::Folder,
            key: f.id.clone(),
            name: f.name.clone(),
            install_dir: f.dir.clone(),
            launch_exe: Some(rel),
            prefix,
            sig_id: sigs.get(&f.id).map(|g| g.id.clone()),
            launch_code: None,
        });
        n += 1;
    }
    n
}

fn steam_has_sig(steam: &SteamIndex, sigs: &crate::signatures::SignatureDb, id: &str) -> bool {
    sigs.get(id)
        .is_some_and(|g| g.steam_appids.iter().any(|a| steam.get(*a).is_some()))
}

/// Lower this thread's CPU and disk priority (Windows background mode).
pub fn background_priority() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{
            GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN,
        };
        SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signatures::SignatureDb;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"MZ").unwrap();
    }

    #[test]
    fn finds_games_by_exe_and_path_and_skips_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Games");
        // signature game, detectable path suffix, launcher, PTR, skipped folder, too deep
        touch(&root.join("Hades/x64/Hades.exe"));
        touch(&root.join("Roberts Space Industries/StarCitizen/LIVE/Bin64/StarCitizen.exe"));
        touch(&root.join("Roberts Space Industries/StarCitizen/PTU/Bin64/StarCitizen.exe"));
        touch(&root.join("Roberts Space Industries/RSI Launcher/RSI Launcher.exe"));
        touch(&root.join("World of Warcraft/_ptr_/WowT.exe"));
        touch(&root.join("Tool/node_modules/x/Hades.exe"));
        touch(&root.join("Notes/notepad2.exe"));
        let (exes, dirs) = walk_exes(std::slice::from_ref(&root), &ScanOpts::default());
        assert!(dirs > 5);
        assert!(!exes.iter().any(|e| e.contains("RSI Launcher")));
        assert!(!exes.iter().any(|e| e.contains("node_modules")));
        let det = Detector::new(SignatureDb::builtin(), SteamIndex::default());
        let found = classify(&det, &exes);
        let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
        assert!(ids.contains(&"star_citizen"), "{ids:?}");
        assert!(ids.iter().any(|i| i.contains("hades")), "{ids:?}");
        assert!(!found.iter().any(|f| f.exe.to_lowercase().contains("_ptr_")));
        assert_eq!(found.iter().filter(|f| f.id == "star_citizen").count(), 1);
        let sc = found.iter().find(|f| f.id == "star_citizen").unwrap();
        assert!(sc.dir.ends_with("LIVE"), "{}", sc.dir);
        let hades = found.iter().find(|f| f.id.contains("hades")).unwrap();
        assert!(hades.dir.ends_with("Hades"), "{}", hades.dir);

        // merged into the install list; the catalog shows them installed and launchable
        let sigs = SignatureDb::builtin();
        let mut libs = LibraryIndex::default();
        let steam = SteamIndex::default();
        assert_eq!(merge_into(&mut libs, &steam, &sigs, &found), found.len());
        let cat = crate::catalog::build(&sigs, &steam, &libs, &[], &Default::default());
        let e = cat.iter().find(|e| e.id == "star_citizen").unwrap();
        assert!(e.installed && e.can_launch, "{e:?}");
        assert_eq!(e.source, "scan");
        // the cache round-trips
        let c = ScanCache {
            at: 1,
            dirs,
            found: found.clone(),
        };
        let back: ScanCache = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn depth_limit_and_launcher_installs_win() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Games");
        touch(&root.join("a/b/c/d/e/f/Hades.exe"));
        let (exes, _) = walk_exes(std::slice::from_ref(&root), &ScanOpts::default());
        assert!(exes.is_empty());
        // already known from a launcher: not added twice
        let exe = root.join("Hades/Hades.exe");
        touch(&exe);
        let found = vec![Found {
            id: "hades".into(),
            name: "Hades".into(),
            exe: exe.to_string_lossy().into(),
            dir: root.join("Hades").to_string_lossy().into(),
        }];
        let mut libs = LibraryIndex::default();
        libs.insert(InstalledGame {
            store: Launcher::Epic,
            key: "Min".into(),
            name: "Hades".into(),
            install_dir: root.join("Hades").to_string_lossy().into(),
            launch_exe: None,
            prefix: make_prefix(&root.join("Hades").to_string_lossy()),
            sig_id: Some("hades".into()),
            launch_code: None,
        });
        let n = merge_into(
            &mut libs,
            &SteamIndex::default(),
            &SignatureDb::builtin(),
            &found,
        );
        assert_eq!(n, 0);
        assert!(is_gameish("Riot Games") && is_gameish("Electronic Arts") && !is_gameish("7-Zip"));
    }
}
