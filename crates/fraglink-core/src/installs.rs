//! Installed games from launchers other than Steam, Epic and GOG (read-only, offline).
//!
//! * **Battle.net:** Windows "Uninstall" entries written by the Blizzard
//!   uninstaller (`--uid=<product>` in `UninstallString`) and
//!   `%ProgramData%\Battle.net\Agent\product.db` (protobuf: product uid, code and
//!   install path). Products map to signatures and launch codes via [`bnet_product`].
//! * **EA app:** Uninstall entries published by Electronic Arts / EAInstaller.
//! * **Ubisoft Connect:** `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>`
//!   (`InstallDir`) and `Uplay Install <id>` Uninstall entries.
//! * **Xbox / Microsoft Store:** folders under `<drive>:\XboxGames\<game>\Content`.
//! * **Anything else:** an Uninstall entry is kept only when [`resolve`] can tie
//!   it to a signature (same name, or one of its exes in the install folder).
//!
//! `FRAGLINK_UNINSTALL_JSON` (a JSON array of entries), `FRAGLINK_BNET_PRODUCT_DB`
//! and `FRAGLINK_XBOX_DIR` stand in for the registry and fixed paths in tests.

use crate::libraries::{
    file_name, make_prefix, prefix_is_safe, InstalledGame, Launcher, LibraryIndex,
};
use crate::signatures::SignatureDb;
use serde::Deserialize;
use std::path::Path;

/// One `...\CurrentVersion\Uninstall\<key>` registry key.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UninstallEntry {
    pub key: String,
    pub display_name: String,
    pub install_location: String,
    pub publisher: String,
    pub uninstall_string: String,
}

/// Battle.net product (uid without locale, or product code) → (signature id, launch code).
pub fn bnet_product(code: &str) -> (Option<&'static str>, Option<&'static str>) {
    let c = code.to_ascii_lowercase();
    let c = strip_locale(&c);
    match c {
        "wow" => (Some("wow"), Some("WoW")),
        "wow_classic" | "wowc" => (Some("wow_classic"), Some("WoWC")),
        "wow_classic_era" => (Some("wow_classic_era"), Some("WoWC")),
        "diablo3" | "d3" => (Some("diablo3"), Some("D3")),
        "fenris" | "fen" => (Some("diablo4"), Some("Fen")),
        "osi" => (Some("d2r"), Some("OSI")),
        "prometheus" | "pro" => (Some("overwatch2"), Some("Pro")),
        "hs_beta" | "wtcg" | "hs" => (Some("hearthstone"), Some("WTCG")),
        "s2" | "sc2" => (Some("sc2"), Some("S2")),
        "s1" => (Some("starcraft"), Some("S1")),
        "w3" => (Some("wc3"), Some("W3")),
        "heroes" | "hero" => (Some("hots"), Some("Hero")),
        "odin" => (Some("cod_mw2019"), Some("ODIN")),
        "zeus" => (Some("cod_bocw"), Some("ZEUS")),
        "auks" => (Some("cod"), Some("AUKS")),
        "fore" => (Some("cod"), Some("FORE")),
        "lazr" | "lazarus" => (Some("cod"), Some("LAZR")),
        "viper" | "vipr" => (None, Some("VIPR")),
        _ => (None, None),
    }
}

/// `wow_enus` → `wow`, `diablo3_dede` → `diablo3` (Blizzard uids carry a locale).
fn strip_locale(c: &str) -> &str {
    if let Some((head, tail)) = c.rsplit_once('_') {
        if tail.len() == 4 && tail.chars().all(|x| x.is_ascii_lowercase()) && !head.is_empty() {
            let known = [
                "enus", "engb", "dede", "frfr", "eses", "esmx", "itit", "kokr", "ptbr", "ruru",
                "zhcn", "zhtw", "plpl", "jajp",
            ];
            if known.contains(&tail) {
                return head;
            }
        }
    }
    c
}

fn uid_from_uninstall(s: &str) -> Option<String> {
    let i = s.find("--uid=")? + 6;
    let rest = s[i..].trim_start_matches('"');
    let uid: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '"')
        .collect();
    (!uid.is_empty()).then_some(uid)
}

fn game(store: Launcher, key: String, name: &str, dir: &str) -> Option<InstalledGame> {
    let prefix = make_prefix(dir);
    if !prefix_is_safe(&prefix) {
        return None;
    }
    let name = if name.trim().is_empty() {
        file_name(dir).unwrap_or_else(|| key.clone())
    } else {
        name.trim().to_string()
    };
    Some(InstalledGame {
        store,
        key,
        name,
        install_dir: dir.trim().to_string(),
        launch_exe: None,
        prefix,
        sig_id: None,
        launch_code: None,
    })
}

/// A Battle.net product from its uid / product code and install path.
pub fn battlenet_game(uid: &str, name: &str, dir: &str) -> Option<InstalledGame> {
    let base = strip_locale(&uid.to_ascii_lowercase()).to_string();
    if matches!(
        base.as_str(),
        "agent" | "bna" | "battle.net" | "battle_net" | ""
    ) {
        return None;
    }
    let (sig, code) = bnet_product(&base);
    let mut g = game(Launcher::Battlenet, base.clone(), name, dir)?;
    g.sig_id = sig.map(str::to_string);
    g.launch_code = Some(code.map(str::to_string).unwrap_or(base));
    Some(g)
}

/// Classify one Uninstall entry. `Launcher::Windows` entries are candidates that
/// [`resolve`] keeps only if they match a signature.
pub fn classify_uninstall(e: &UninstallEntry) -> Option<InstalledGame> {
    let dir = e.install_location.trim().trim_matches('"');
    let name = e.display_name.trim();
    if dir.is_empty() || name.is_empty() || e.key.starts_with("Steam App ") {
        return None;
    }
    let publ = e.publisher.to_ascii_lowercase();
    let un = e.uninstall_string.to_ascii_lowercase();
    if let Some(uid) = uid_from_uninstall(&e.uninstall_string)
        .filter(|_| un.contains("blizzard") || publ.contains("blizzard"))
    {
        return battlenet_game(&uid, name, dir);
    }
    if publ.contains("blizzard") {
        if name.eq_ignore_ascii_case("battle.net") {
            return None;
        }
        return battlenet_game(&e.key, name, dir);
    }
    if let Some(id) = e.key.strip_prefix("Uplay Install ") {
        let mut g = game(Launcher::Ubisoft, id.trim().to_string(), name, dir)?;
        g.launch_code = Some(id.trim().to_string());
        return Some(g);
    }
    if (publ.contains("electronic arts") || un.contains("eainstaller"))
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "ea app" | "origin" | "ea desktop"
        )
    {
        return game(Launcher::Ea, e.key.clone(), name, dir);
    }
    if publ.contains("gog.com") || publ.contains("epic games") {
        return None; // read from their own launcher data
    }
    game(Launcher::Windows, e.key.clone(), name, dir)
}

/* ---- tiny protobuf reader for Battle.net's product.db ---- */

fn varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let x = *b.get(*i)?;
        *i += 1;
        v |= u64::from(x & 0x7f) << shift;
        if x & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Length-delimited fields of one message as (field number, bytes). Other wire
/// types are skipped; malformed data ends the walk.
fn fields(b: &[u8]) -> Vec<(u64, &[u8])> {
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let Some(tag) = varint(b, &mut i) else { break };
        match tag & 7 {
            0 => {
                if varint(b, &mut i).is_none() {
                    break;
                }
            }
            1 => i += 8,
            5 => i += 4,
            2 => {
                let Some(n) = varint(b, &mut i) else { break };
                let Some(end) = i.checked_add(n as usize).filter(|e| *e <= b.len()) else {
                    break;
                };
                out.push((tag >> 3, &b[i..end]));
                i = end;
            }
            _ => break,
        }
    }
    out
}

/// `product.db`: `Database { repeated ProductInstall = 1 }`,
/// `ProductInstall { uid = 1; product_code = 2; UserSettings settings = 3 }`,
/// `UserSettings { install_path = 1 }`. Returns (uid, product code, install path).
pub fn parse_product_db(b: &[u8]) -> Vec<(String, String, String)> {
    let s = |x: &[u8]| String::from_utf8_lossy(x).into_owned();
    let mut out = vec![];
    for (f, pi) in fields(b) {
        if f != 1 {
            continue;
        }
        let (mut uid, mut code, mut path) = (String::new(), String::new(), String::new());
        for (g, v) in fields(pi) {
            match g {
                1 => uid = s(v),
                2 => code = s(v),
                3 => {
                    if let Some((_, p)) = fields(v).into_iter().find(|(h, _)| *h == 1) {
                        path = s(p);
                    }
                }
                _ => {}
            }
        }
        if !uid.is_empty() && !path.is_empty() {
            out.push((uid, code, path));
        }
    }
    out
}

/// Games under an `XboxGames` folder (`<dir>\<game>\Content`).
pub fn add_xbox_dir(idx: &mut LibraryIndex, dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let content = p.join("Content");
        if !content.is_dir() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(g) = game(
            Launcher::Xbox,
            name.clone(),
            &name,
            &content.to_string_lossy(),
        ) {
            idx.insert(g);
        }
    }
}

fn add_entry(idx: &mut LibraryIndex, g: InstalledGame) {
    // Epic/GOG already know this folder
    if matches!(g.store, Launcher::Windows | Launcher::Ea) && idx.has_prefix(&g.prefix) {
        return;
    }
    idx.insert(g);
}

pub fn add_uninstall_entries(idx: &mut LibraryIndex, entries: &[UninstallEntry]) {
    for e in entries {
        if e.display_name.trim().eq_ignore_ascii_case("battle.net")
            && !e.install_location.trim().is_empty()
        {
            let exe = Path::new(e.install_location.trim().trim_matches('"')).join("Battle.net.exe");
            idx.battlenet_exe = Some(exe.to_string_lossy().into_owned());
            continue;
        }
        if let Some(g) = classify_uninstall(e) {
            add_entry(idx, g);
        }
    }
}

pub fn add_product_db(idx: &mut LibraryIndex, bytes: &[u8]) {
    for (uid, code, path) in parse_product_db(bytes) {
        let key = if bnet_product(&code).1.is_some() {
            code
        } else {
            uid
        };
        let name = file_name(&path).unwrap_or_default();
        if let Some(mut g) = battlenet_game(&key, &name, &path) {
            // keep a nicer name from the Uninstall list if we have one
            if let Some(old) = idx
                .games()
                .iter()
                .find(|x| x.store == Launcher::Battlenet && x.key == g.key)
            {
                g.name = old.name.clone();
            }
            idx.insert(g);
        }
    }
}

/// Read everything this module knows about into `idx`.
pub fn load_into(idx: &mut LibraryIndex) {
    if let Ok(p) = std::env::var("FRAGLINK_UNINSTALL_JSON") {
        if let Some(v) = std::fs::read_to_string(p)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<UninstallEntry>>(&t).ok())
        {
            add_uninstall_entries(idx, &v);
        }
    } else {
        #[cfg(windows)]
        {
            add_ubisoft_registry(idx);
            add_uninstall_entries(idx, &read_uninstall_registry());
        }
    }
    let pdb = std::env::var_os("FRAGLINK_BNET_PRODUCT_DB")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            cfg!(windows).then(|| {
                std::env::var_os("ProgramData")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|| r"C:\ProgramData".into())
                    .join(r"Battle.net\Agent\product.db")
            })
        });
    if let Some(p) = pdb {
        if let Ok(b) = std::fs::read(&p) {
            if b.len() < 20_000_000 {
                add_product_db(idx, &b);
            }
        }
    }
    if idx.battlenet_exe.is_none() && cfg!(windows) {
        let d = r"C:\Program Files (x86)\Battle.net\Battle.net.exe";
        if Path::new(d).exists() {
            idx.battlenet_exe = Some(d.into());
        }
    }
    if let Ok(d) = std::env::var("FRAGLINK_XBOX_DIR") {
        add_xbox_dir(idx, Path::new(&d));
    } else if cfg!(windows) {
        for l in b'C'..=b'Z' {
            let d = format!("{}:\\XboxGames", l as char);
            if Path::new(&d).is_dir() {
                add_xbox_dir(idx, Path::new(&d));
            }
        }
    }
}

#[cfg(windows)]
fn read_uninstall_registry() -> Vec<UninstallEntry> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    let mut out = vec![];
    for (hive, path) in [
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
        (
            HKEY_CURRENT_USER,
            r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
        ),
    ] {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ) else {
            continue;
        };
        for key in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&key, KEY_READ) else {
                continue;
            };
            let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            out.push(UninstallEntry {
                display_name: get("DisplayName"),
                install_location: get("InstallLocation"),
                publisher: get("Publisher"),
                uninstall_string: get("UninstallString"),
                key,
            });
        }
    }
    out
}

#[cfg(windows)]
fn add_ubisoft_registry(idx: &mut LibraryIndex) {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;
    for path in [
        r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs",
        r"SOFTWARE\Ubisoft\Launcher\Installs",
    ] {
        let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(path, KEY_READ)
        else {
            continue;
        };
        for id in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey_with_flags(&id, KEY_READ) else {
                continue;
            };
            let dir: String = k.get_value("InstallDir").unwrap_or_default();
            if let Some(mut g) = game(Launcher::Ubisoft, id.clone(), "", &dir) {
                g.launch_code = Some(id);
                idx.insert(g);
            }
        }
    }
}

/* ---- tie installs to signatures ---- */

/// Lowercase letters and digits only (drops ™, ®, punctuation and spaces).
pub fn norm_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// .exe files up to three folders deep: (lowercase file name, relative path).
fn scan_exes(dir: &Path) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut stack = vec![(dir.to_path_buf(), String::new(), 0)];
    let mut budget = 1500usize;
    while let Some((d, rel, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            if budget == 0 {
                return out;
            }
            budget -= 1;
            let n = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() {
                n.clone()
            } else {
                format!("{rel}/{n}")
            };
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 3 {
                    stack.push((e.path(), r, depth + 1));
                }
            } else if n.to_ascii_lowercase().ends_with(".exe") {
                out.push((n.to_ascii_lowercase(), r));
            }
        }
    }
    out
}

/// Fill in `sig_id` / `launch_exe` from the signature list (name, then exes in
/// the install folder), and drop Uninstall-only entries that aren't games.
pub fn resolve(idx: &mut LibraryIndex, sigs: &SignatureDb) {
    let names: Vec<(String, &str)> = sigs
        .games()
        .iter()
        .flat_map(|g| {
            let full = norm_name(&g.name);
            let short = norm_name(g.name.split(" (").next().unwrap_or(&g.name));
            [(full, g.id.as_str()), (short, g.id.as_str())]
        })
        .collect();
    for g in idx.games_mut() {
        if matches!(g.store, Launcher::Epic | Launcher::Gog) {
            continue; // matched by exe when running (detector::library_identity)
        }
        if g.sig_id.is_none() {
            let n = norm_name(&g.name);
            g.sig_id = names
                .iter()
                .find(|(k, _)| !k.is_empty() && *k == n)
                .map(|(_, id)| id.to_string());
        }
        if g.launch_exe.is_some() {
            continue;
        }
        let exes = scan_exes(Path::new(&g.install_dir));
        let want = |id: &str| {
            sigs.get(id)
                .map(|s| {
                    s.exes
                        .iter()
                        .map(|x| x.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        if let Some(id) = g.sig_id.clone() {
            let w = want(&id);
            g.launch_exe = exes
                .iter()
                .find(|(n, _)| w.contains(n))
                .map(|(_, r)| r.clone());
        } else if let Some((sig, rel)) = sigs.games().iter().find_map(|s| {
            let w: Vec<String> = s.exes.iter().map(|x| x.to_ascii_lowercase()).collect();
            exes.iter()
                .find(|(n, _)| w.contains(n))
                .map(|(_, r)| (s.id.clone(), r.clone()))
        }) {
            g.sig_id = Some(sig);
            g.launch_exe = Some(rel);
        }
    }
    idx.retain(|g| g.store != Launcher::Windows || g.sig_id.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }
    fn pb_str(field: u8, s: &[u8]) -> Vec<u8> {
        let mut v = vec![(field << 3) | 2, s.len() as u8];
        v.extend_from_slice(s);
        v
    }

    #[test]
    fn product_db_parses_installs() {
        let mut settings = pb_str(1, b"C:/Program Files (x86)/World of Warcraft");
        settings.extend(pb_str(2, b"US"));
        let mut pi = pb_str(1, b"wow");
        pi.extend(pb_str(2, b"wow"));
        pi.extend(pb_str(3, &settings));
        pi.extend([0x20, 0x01]); // a varint field is skipped
        let agent = [pb_str(1, b"agent"), pb_str(2, b"agent")].concat(); // no install path
        let db = [pb_str(1, &pi), pb_str(1, &agent), vec![0x10, 0x05]].concat();
        let got = parse_product_db(&db);
        assert_eq!(
            got,
            vec![(
                "wow".into(),
                "wow".into(),
                "C:/Program Files (x86)/World of Warcraft".into()
            )]
        );
        // truncated input doesn't panic
        let _ = parse_product_db(&db[..db.len() / 2]);
        let mut idx = LibraryIndex::default();
        add_product_db(&mut idx, &db);
        let g = &idx.games()[0];
        assert_eq!(
            (g.store, g.sig_id.as_deref(), g.launch_code.as_deref()),
            (Launcher::Battlenet, Some("wow"), Some("WoW"))
        );
    }

    #[test]
    fn uninstall_entries_classify_by_launcher() {
        let text = std::fs::read_to_string(fixtures().join("uninstall/entries.json")).unwrap();
        let entries: Vec<UninstallEntry> = serde_json::from_str(&text).unwrap();
        let mut idx = LibraryIndex::default();
        add_uninstall_entries(&mut idx, &entries);
        resolve(&mut idx, &SignatureDb::builtin());
        let find = |k: &str| idx.games().iter().find(|g| g.key == k);
        let d4 = find("fenris").unwrap();
        assert_eq!(
            (
                d4.store,
                d4.sig_id.as_deref(),
                d4.launch_code.as_deref(),
                d4.name.as_str()
            ),
            (
                Launcher::Battlenet,
                Some("diablo4"),
                Some("Fen"),
                "Diablo IV"
            )
        );
        assert_eq!(
            find("prometheus").unwrap().sig_id.as_deref(),
            Some("overwatch2")
        );
        assert_eq!(
            idx.battlenet_exe.as_deref().map(|p| p.replace('\\', "/")),
            Some("C:/Program Files (x86)/Battle.net/Battle.net.exe".into())
        );
        let ubi = find("635").unwrap();
        assert_eq!(
            (ubi.store, ubi.launch_code.as_deref()),
            (Launcher::Ubisoft, Some("635"))
        );
        assert_eq!(find("{EA-APEX}").unwrap().store, Launcher::Ea);
        assert_eq!(find("{EA-APEX}").unwrap().sig_id.as_deref(), Some("apex"));
        // a signature game found by name through the generic Uninstall list
        let rl = find("RocketLeagueKey").unwrap();
        assert_eq!((rl.store, rl.sig_id.is_some()), (Launcher::Windows, true));
        // non-games, Steam's own entries and too-broad folders are dropped
        assert!(find("Discord").is_none());
        assert!(find("Steam App 730").is_none());
        assert!(find("BroadThing").is_none());
    }

    #[test]
    fn exe_scan_ties_unknown_names_to_signatures() {
        let dir = std::env::temp_dir().join(format!("fl-inst-{}", std::process::id()));
        let game = dir.join("Odd Name Folder/bin");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Hades.exe"), b"").unwrap();
        let mut idx = LibraryIndex::default();
        let e = UninstallEntry {
            key: "X".into(),
            display_name: "Some Odd Name".into(),
            install_location: dir.join("Odd Name Folder").to_string_lossy().into(),
            ..Default::default()
        };
        add_uninstall_entries(&mut idx, &[e]);
        resolve(&mut idx, &SignatureDb::builtin());
        let g = &idx.games()[0];
        assert!(g.sig_id.is_some());
        assert_eq!(g.launch_exe.as_deref(), Some("bin/Hades.exe"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn xbox_folders_and_locales() {
        let dir = std::env::temp_dir().join(format!("fl-xbox-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Forza Horizon 5/Content")).unwrap();
        std::fs::create_dir_all(dir.join("NotAGame")).unwrap();
        let mut idx = LibraryIndex::default();
        add_xbox_dir(&mut idx, &dir);
        assert_eq!(idx.games().len(), 1);
        assert_eq!(idx.games()[0].store, Launcher::Xbox);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(strip_locale("wow_enus"), "wow");
        assert_eq!(strip_locale("wow_classic"), "wow_classic");
        assert_eq!(uid_from_uninstall(r#""C:\ProgramData\Battle.net\Agent\Blizzard Uninstaller.exe" --lang=enUS --uid=diablo3_enus --displayname="Diablo III""#).as_deref(), Some("diablo3_enus"));
    }
}
