//! Steam library mapping without per-exe entries.
//!
//! * `libraryfolders.vdf` lists every Steam library folder.
//! * each `steamapps/appmanifest_<appid>.acf` gives `appid`, `name`, `installdir`.
//! * any process whose image lives under `<library>/steamapps/common/<installdir>/`
//!   is attributed to that app.
//! * On Windows, `HKCU\Software\Valve\Steam\RunningAppID` names the running game.
//!
//! Everything here is read-only file/registry access.

use crate::signatures::normalize_path;
use crate::vdf;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamApp {
    pub appid: u32,
    pub name: String,
    pub installdir: String,
    /// Normalized `<library>/steamapps/common/<installdir>/` prefix.
    pub prefix: String,
}

#[derive(Debug, Clone, Default)]
pub struct SteamIndex {
    apps: Vec<SteamApp>,
    by_appid: HashMap<u32, usize>,
    pub libraries: Vec<PathBuf>,
}

/// Steam tools / runtimes that are never games.
pub fn is_tool_app(appid: u32, name: &str) -> bool {
    const TOOLS: &[u32] = &[
        228980,  // Steamworks Common Redistributables
        1070560, // Steam Linux Runtime
        1391110, // Steam Linux Runtime - Soldier
        1628350, // Steam Linux Runtime - Sniper
        250820,  // SteamVR
        431960,  // Wallpaper Engine
        1493710, // Proton Experimental
    ];
    let n = name.to_lowercase();
    TOOLS.contains(&appid)
        || n.starts_with("proton ")
        || n.contains("redistributable")
        || n.starts_with("steam linux runtime")
        || n.contains("dedicated server")
        || n.ends_with(" sdk")
}

/// Parse `libraryfolders.vdf` (both the modern and the pre-2021 format).
pub fn parse_library_folders(text: &str) -> Result<Vec<PathBuf>, vdf::ParseError> {
    let doc = vdf::parse(text)?;
    let Some(root) = doc.get("libraryfolders") else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (k, v) in root.entries() {
        if !k.chars().all(|c| c.is_ascii_digit()) {
            continue; // e.g. "TimeNextStatsReport", "contentstatsid"
        }
        let path = match v {
            vdf::Vdf::Str(s) => Some(s.as_str()), // old format: "1" "D:\\SteamLibrary"
            vdf::Vdf::Obj(_) => v.get_str("path"), // new format: "0" { "path" "..." }
        };
        if let Some(p) = path {
            if !p.is_empty() {
                out.push(PathBuf::from(p));
            }
        }
    }
    Ok(out)
}

/// Parse an `appmanifest_*.acf`, returning `(appid, name, installdir)`.
pub fn parse_app_manifest(text: &str) -> Option<(u32, String, String)> {
    let doc = vdf::parse(text).ok()?;
    let st = doc.get("AppState")?;
    let appid = st.get_str("appid")?.trim().parse().ok()?;
    let name = st.get_str("name").unwrap_or("").to_string();
    let installdir = st.get_str("installdir")?.to_string();
    if installdir.is_empty() {
        return None;
    }
    let name = if name.is_empty() {
        installdir.clone()
    } else {
        name
    };
    Some((appid, name, installdir))
}

fn prefix_for(library: &str, installdir: &str) -> String {
    let mut lib = normalize_path(library);
    while lib.ends_with('/') {
        lib.pop();
    }
    format!("{lib}/steamapps/common/{}/", installdir.to_lowercase())
}

impl SteamIndex {
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    pub fn apps(&self) -> &[SteamApp] {
        &self.apps
    }

    pub fn get(&self, appid: u32) -> Option<&SteamApp> {
        self.by_appid.get(&appid).map(|&i| &self.apps[i])
    }

    /// Add one app (used by `load` and by tests with Windows-style paths).
    pub fn insert(&mut self, library: &str, appid: u32, name: &str, installdir: &str) {
        if is_tool_app(appid, name) {
            return;
        }
        let app = SteamApp {
            appid,
            name: name.to_string(),
            installdir: installdir.to_string(),
            prefix: prefix_for(library, installdir),
        };
        if let Some(&i) = self.by_appid.get(&appid) {
            self.apps[i] = app;
        } else {
            self.by_appid.insert(appid, self.apps.len());
            self.apps.push(app);
        }
    }

    /// Scan the given library folders for app manifests.
    pub fn from_libraries(libs: &[PathBuf]) -> Self {
        let mut idx = SteamIndex {
            libraries: libs.to_vec(),
            ..Default::default()
        };
        for lib in libs {
            let steamapps = lib.join("steamapps");
            let Ok(rd) = std::fs::read_dir(&steamapps) else {
                continue;
            };
            for e in rd.flatten() {
                let fname = e.file_name().to_string_lossy().to_string();
                if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(e.path()) else {
                    continue;
                };
                if let Some((appid, name, dir)) = parse_app_manifest(&text) {
                    idx.insert(&lib.to_string_lossy(), appid, &name, &dir);
                }
            }
        }
        idx
    }

    /// Load everything starting from a Steam install root.
    pub fn load(steam_root: &Path) -> Self {
        let mut libs = vec![steam_root.to_path_buf()];
        let lf = steam_root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = std::fs::read_to_string(&lf) {
            match parse_library_folders(&text) {
                Ok(found) => {
                    for l in found {
                        if !libs.iter().any(|x| {
                            normalize_path(&x.to_string_lossy())
                                == normalize_path(&l.to_string_lossy())
                        }) {
                            libs.push(l);
                        }
                    }
                }
                Err(e) => log::warn!("libraryfolders.vdf: {e}"),
            }
        }
        Self::from_libraries(&libs)
    }

    /// Which installed Steam app does this executable path belong to?
    pub fn app_for_path(&self, path: &str) -> Option<&SteamApp> {
        let p = normalize_path(path);
        if !p.contains("/steamapps/common/") {
            return None;
        }
        // longest prefix wins (nested install dirs are rare but possible)
        self.apps
            .iter()
            .filter(|a| p.starts_with(&a.prefix))
            .max_by_key(|a| a.prefix.len())
    }
}

/// Best guess at the Steam install folder. `FRAGLINK_STEAM_DIR` overrides.
pub fn steam_root() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("FRAGLINK_STEAM_DIR") {
        return Some(PathBuf::from(p));
    }
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                return Some(PathBuf::from(p));
            }
        }
        if let Ok(k) =
            RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\Valve\Steam")
        {
            if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                return Some(PathBuf::from(p));
            }
        }
        let def = PathBuf::from(r"C:\Program Files (x86)\Steam");
        if def.exists() {
            return Some(def);
        }
        None
    }
    #[cfg(not(windows))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        [home.join(".steam/steam"), home.join(".local/share/Steam")]
            .into_iter()
            .find(|p| p.join("steamapps").exists())
    }
}

/// `HKCU\Software\Valve\Steam\RunningAppID` (0 / missing = nothing running).
pub fn running_app_id() -> Option<u32> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let k = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Valve\Steam")
            .ok()?;
        let v: u32 = k.get_value("RunningAppID").ok()?;
        (v != 0).then_some(v)
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
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/steam")
    }

    #[test]
    fn parses_modern_libraryfolders() {
        let text = std::fs::read_to_string(fixtures().join("libraryfolders.vdf")).unwrap();
        let libs = parse_library_folders(&text).unwrap();
        assert_eq!(
            libs,
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary")
            ]
        );
    }

    #[test]
    fn parses_legacy_libraryfolders() {
        let text = std::fs::read_to_string(fixtures().join("libraryfolders_legacy.vdf")).unwrap();
        let libs = parse_library_folders(&text).unwrap();
        assert_eq!(libs, vec![PathBuf::from(r"E:\Games\Steam")]);
    }

    #[test]
    fn parses_app_manifest() {
        let text =
            std::fs::read_to_string(fixtures().join("lib1/steamapps/appmanifest_570.acf")).unwrap();
        assert_eq!(
            parse_app_manifest(&text),
            Some((570, "Dota 2".to_string(), "dota 2 beta".to_string()))
        );
        assert_eq!(parse_app_manifest("\"AppState\" { \"appid\" \"x\" }"), None);
    }

    #[test]
    fn indexes_fixture_libraries_and_skips_tools() {
        let f = fixtures();
        let idx = SteamIndex::from_libraries(&[f.join("lib1"), f.join("lib2")]);
        assert!(idx.get(570).is_some());
        assert!(idx.get(1145350).is_some()); // Hades II in lib2
        assert!(idx.get(228980).is_none(), "redistributables are not games");
        let exe = f.join("lib2/steamapps/common/Hades II/Ship/Hades2.exe");
        assert_eq!(
            idx.app_for_path(&exe.to_string_lossy()).unwrap().appid,
            1145350
        );
    }

    #[test]
    fn load_follows_libraryfolders_from_root() {
        // lib1 acts as the Steam root; its libraryfolders.vdf points at lib2 via a relative-free absolute path.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Steam");
        let other = tmp.path().join("Other Library");
        std::fs::create_dir_all(root.join("steamapps")).unwrap();
        std::fs::create_dir_all(other.join("steamapps")).unwrap();
        std::fs::write(
            root.join("steamapps/libraryfolders.vdf"),
            format!(
                "\"libraryfolders\"\n{{\n \"0\" {{ \"path\" \"{}\" }}\n \"1\" {{ \"path\" \"{}\" }}\n}}\n",
                root.to_string_lossy().replace('\\', "\\\\"),
                other.to_string_lossy().replace('\\', "\\\\")
            ),
        )
        .unwrap();
        std::fs::write(
            other.join("steamapps/appmanifest_892970.acf"),
            "\"AppState\" { \"appid\" \"892970\" \"name\" \"Valheim\" \"installdir\" \"Valheim\" }",
        )
        .unwrap();
        let idx = SteamIndex::load(&root);
        assert_eq!(idx.libraries.len(), 2);
        let exe = other.join("steamapps/common/Valheim/valheim.exe");
        assert_eq!(
            idx.app_for_path(&exe.to_string_lossy()).unwrap().name,
            "Valheim"
        );
    }

    #[test]
    fn windows_style_paths_match_case_insensitively() {
        let mut idx = SteamIndex::default();
        idx.insert(
            r"C:\Program Files (x86)\Steam\",
            570,
            "Dota 2",
            "dota 2 beta",
        );
        idx.insert(
            r"D:\SteamLibrary",
            1086940,
            "Baldur's Gate 3",
            "Baldurs Gate 3",
        );
        let dota =
            r"C:\PROGRAM FILES (X86)\Steam\steamapps\common\dota 2 beta\game\bin\win64\dota2.exe";
        assert_eq!(idx.app_for_path(dota).unwrap().appid, 570);
        let bg3 = r"D:\SteamLibrary\steamapps\common\Baldurs Gate 3\bin\bg3_dx11.exe";
        assert_eq!(idx.app_for_path(bg3).unwrap().appid, 1086940);
        assert!(idx
            .app_for_path(r"D:\SteamLibrary\steamapps\common\Other\x.exe")
            .is_none());
        assert!(idx.app_for_path(r"C:\Windows\notepad.exe").is_none());
    }
}
