//! WoW /played from the bundled `FraglinkPlayed` addon.
//!
//! The addon keeps one entry per character in its SavedVariables file
//! (`<flavour>\WTF\Account\<ACCOUNT>\SavedVariables\FraglinkPlayed.lua`). This module
//! reads that file with a small parser for Lua table literals (nothing is run),
//! computes the same checksum as the addon, finds the WoW flavour folders and copies
//! the addon into them.
//!
//! The checksum is a deterrent against hand-edited totals, not security: the key ships
//! inside the addon. Leaderboards additionally need the character verified through
//! Battle.net (server side).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Addon folder name (also the SavedVariables file stem).
pub const ADDON: &str = "FraglinkPlayed";
/// Version in the bundled TOC.
pub const ADDON_VERSION: &str = "1.0.0";

/// Checksum keys by signature version. Every addon release that bumps
/// `SIG_VERSION` adds its key here; the server accepts only listed versions, so an
/// old key is retired by deleting its row.
pub const SIG_KEYS: &[(i64, &str)] = &[(1, "OlApFDIsED1aJE04makEnBj7W6DZwvYG")];

/// The bundled addon files (name, contents).
pub const ADDON_FILES: &[(&str, &str)] = &[
    (
        "FraglinkPlayed.toc",
        include_str!("../../../apps/desktop/addon/FraglinkPlayed/FraglinkPlayed.toc"),
    ),
    (
        "FraglinkPlayed_Camelot.toc",
        include_str!("../../../apps/desktop/addon/FraglinkPlayed/FraglinkPlayed_Camelot.toc"),
    ),
    (
        "FraglinkPlayed.lua",
        include_str!("../../../apps/desktop/addon/FraglinkPlayed/FraglinkPlayed.lua"),
    ),
];

/// WoW flavour folder -> Fraglink game id. Only these two count.
pub const FLAVOURS: &[(&str, &str)] = &[("_retail_", "wow"), ("_forever_", "wow_forever")];

/// One character's entry as the addon saved it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CharEntry {
    pub region: String,
    pub realm: String,
    pub name: String,
    pub class: String,
    pub level: i64,
    /// Total /played in seconds.
    pub played: i64,
    /// Unix time the game answered.
    pub at: i64,
    /// Signature version (which key).
    pub sv: i64,
    /// The player's BattleTag (BNGetInfo), checked against the linked Battle.net account.
    #[serde(default)]
    pub battletag: String,
    pub sig: String,
}

fn hash(s: &[u8]) -> String {
    let (mut a, mut b): (u64, u64) = (7, 11);
    for &c in s {
        let c = c as u64;
        a = (a * 131 + c) % 2_147_483_629;
        b = (b * 137 + c + a % 251) % 2_147_483_587;
    }
    format!("{a:08x}{b:08x}")
}

/// The addon's checksum of an entry with `key`.
pub fn sign_with(key: &str, e: &CharEntry) -> String {
    let fields = [
        e.region.clone(),
        e.realm.clone(),
        e.name.clone(),
        e.class.clone(),
        e.level.to_string(),
        e.played.to_string(),
        e.at.to_string(),
        e.sv.to_string(),
        e.battletag.clone(),
    ]
    .join("|");
    let inner = hash(format!("{key}|{fields}").as_bytes());
    hash(format!("{key}{inner}").as_bytes())
}

/// Checksum result for an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigCheck {
    Ok,
    /// The checksum doesn't match: the entry was edited (or corrupted).
    Bad,
    /// Signature version not (or no longer) accepted.
    UnknownVersion,
}

pub fn verify(e: &CharEntry) -> SigCheck {
    match SIG_KEYS.iter().find(|(v, _)| *v == e.sv) {
        None => SigCheck::UnknownVersion,
        Some((_, k)) if sign_with(k, e).eq_ignore_ascii_case(e.sig.trim()) => SigCheck::Ok,
        Some(_) => SigCheck::Bad,
    }
}

// ---- Lua table literal parser ------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Lua {
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Table(BTreeMap<String, Lua>),
}

impl Lua {
    fn get(&self, k: &str) -> Option<&Lua> {
        match self {
            Lua::Table(t) => t.get(k),
            _ => None,
        }
    }
    fn str(&self, k: &str) -> Option<String> {
        match self.get(k)? {
            Lua::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
    fn int(&self, k: &str) -> Option<i64> {
        match self.get(k)? {
            Lua::Num(n) if n.fract() == 0.0 && n.abs() < 9e15 => Some(*n as i64),
            _ => None,
        }
    }
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                return;
            }
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }
    fn eat(&mut self, c: u8) -> bool {
        if self.peek() == Some(c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn ident(&mut self) -> Option<String> {
        self.ws();
        let st = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || self.s[self.i] == b'_')
        {
            self.i += 1;
        }
        (self.i > st && !self.s[st].is_ascii_digit())
            .then(|| String::from_utf8_lossy(&self.s[st..self.i]).into_owned())
    }
    fn string(&mut self) -> Option<String> {
        let q = self.peek()?;
        if q != b'"' && q != b'\'' {
            return None;
        }
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let c = *self.s.get(self.i)?;
            self.i += 1;
            if c == q {
                break;
            }
            if c == b'\n' {
                return None;
            }
            if c != b'\\' {
                out.push(c);
                continue;
            }
            let e = *self.s.get(self.i)?;
            self.i += 1;
            match e {
                b'n' | b'\n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(7),
                b'b' => out.push(8),
                b'f' => out.push(12),
                b'v' => out.push(11),
                b'0'..=b'9' => {
                    let mut n = (e - b'0') as u32;
                    for _ in 0..2 {
                        match self.s.get(self.i) {
                            Some(d @ b'0'..=b'9') => {
                                n = n * 10 + (d - b'0') as u32;
                                self.i += 1;
                            }
                            _ => break,
                        }
                    }
                    out.push(u8::try_from(n).ok()?);
                }
                other => out.push(other),
            }
        }
        Some(String::from_utf8_lossy(&out).into_owned())
    }
    fn number(&mut self) -> Option<f64> {
        self.ws();
        let st = self.i;
        while self.i < self.s.len()
            && (self.s[self.i].is_ascii_alphanumeric() || b"+-.".contains(&self.s[self.i]))
        {
            self.i += 1;
        }
        let t = std::str::from_utf8(&self.s[st..self.i]).ok()?;
        if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
            return i64::from_str_radix(h, 16).ok().map(|n| n as f64);
        }
        t.parse::<f64>().ok().filter(|n| n.is_finite())
    }
    fn value(&mut self) -> Option<Lua> {
        match self.peek()? {
            b'{' => self.table(),
            b'"' | b'\'' => self.string().map(Lua::Str),
            b'-' | b'0'..=b'9' | b'.' => self.number().map(Lua::Num),
            _ => match self.ident()?.as_str() {
                "nil" => Some(Lua::Nil),
                "true" => Some(Lua::Bool(true)),
                "false" => Some(Lua::Bool(false)),
                _ => None,
            },
        }
    }
    fn table(&mut self) -> Option<Lua> {
        if !self.eat(b'{') {
            return None;
        }
        self.depth += 1;
        if self.depth > 16 {
            return None;
        }
        let mut t = BTreeMap::new();
        let mut n = 0;
        loop {
            if self.eat(b'}') {
                break;
            }
            let key = if self.eat(b'[') {
                let k = match self.value()? {
                    Lua::Str(s) => s,
                    Lua::Num(x) => format!("{x}"),
                    _ => return None,
                };
                if !self.eat(b']') || !self.eat(b'=') {
                    return None;
                }
                k
            } else {
                let save = self.i;
                match self.ident() {
                    Some(id)
                        if !["nil", "true", "false"].contains(&id.as_str()) && self.eat(b'=') =>
                    {
                        id
                    }
                    _ => {
                        self.i = save;
                        n += 1;
                        format!("{n}")
                    }
                }
            };
            let v = self.value()?;
            t.insert(key, v);
            if t.len() > 100_000 {
                return None;
            }
            if !self.eat(b',') && !self.eat(b';') {
                if !self.eat(b'}') {
                    return None;
                }
                break;
            }
        }
        self.depth -= 1;
        Some(Lua::Table(t))
    }
}

/// Parse a SavedVariables file: `Name = value` assignments, nothing else.
pub fn parse_saved_variables(text: &str) -> Option<BTreeMap<String, Lua>> {
    let mut p = P {
        s: text.as_bytes(),
        i: 0,
        depth: 0,
    };
    let mut out = BTreeMap::new();
    while p.peek().is_some() {
        let name = p.ident()?;
        if !p.eat(b'=') {
            return None;
        }
        out.insert(name, p.value()?);
    }
    Some(out)
}

/// The character entries in a `FraglinkPlayed.lua` SavedVariables file. Malformed
/// entries are skipped; an unreadable file gives none.
pub fn entries_from_saved(text: &str) -> Vec<CharEntry> {
    let Some(vars) = parse_saved_variables(text) else {
        return Vec::new();
    };
    let Some(Lua::Table(chars)) = vars.get("FraglinkPlayedDB").and_then(|d| d.get("chars")) else {
        return Vec::new();
    };
    chars
        .values()
        .filter_map(|c| {
            Some(CharEntry {
                region: c.str("region")?,
                realm: c.str("realm")?,
                name: c.str("name")?,
                class: c.str("class").unwrap_or_default(),
                level: c.int("level").unwrap_or(0),
                played: c.int("played")?,
                at: c.int("at")?,
                sv: c.int("sv")?,
                battletag: c.str("battletag").unwrap_or_default(),
                sig: c.str("sig")?,
            })
        })
        .filter(|e| e.played > 0 && !e.realm.is_empty() && !e.name.is_empty())
        .collect()
}

// ---- files on disk -------------------------------------------------------------

/// The flavour folder (`..\_retail_`) and its game id for a WoW exe path or a
/// flavour / WoW root folder. Roots give every flavour under them that exists.
pub fn flavour_dirs(paths: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut add = |id: &str, d: PathBuf| {
        if !out.iter().any(|(_, x)| x == &d) {
            out.push((id.to_string(), d));
        }
    };
    for p in paths {
        let dir = if p.is_file() || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
            p.parent().map(Path::to_path_buf)
        } else {
            Some(p.clone())
        };
        let Some(dir) = dir else { continue };
        let leaf = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if let Some((_, id)) = FLAVOURS.iter().find(|(f, _)| *f == leaf) {
            add(id, dir);
            continue;
        }
        for (f, id) in FLAVOURS {
            let d = dir.join(f);
            if d.is_dir() {
                add(id, d);
            }
        }
    }
    out
}

/// Every `FraglinkPlayed.lua` under a flavour's `WTF\Account\*\SavedVariables`.
pub fn saved_files(flavour: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(flavour.join("WTF").join("Account")) else {
        return Vec::new();
    };
    rd.flatten()
        .map(|a| a.path().join("SavedVariables").join(format!("{ADDON}.lua")))
        .filter(|p| p.is_file())
        .collect()
}

/// All character entries of one flavour (newest entry wins per character).
pub fn read_flavour(flavour: &Path) -> Vec<CharEntry> {
    let mut by: BTreeMap<String, CharEntry> = BTreeMap::new();
    for f in saved_files(flavour) {
        let Ok(b) = std::fs::read(&f) else { continue };
        if b.len() > 4_000_000 {
            continue;
        }
        for e in entries_from_saved(&String::from_utf8_lossy(&b)) {
            let k = format!(
                "{}/{}/{}",
                e.region,
                e.realm.to_lowercase(),
                e.name.to_lowercase()
            );
            if by.get(&k).is_none_or(|o| o.at < e.at) {
                by.insert(k, e);
            }
        }
    }
    by.into_values().collect()
}

/// Newest modification time (unix seconds) of a flavour's addon files, for "changed since".
pub fn saved_stamp(flavour: &Path) -> i64 {
    saved_files(flavour)
        .iter()
        .filter_map(|f| f.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .max()
        .unwrap_or(0)
}

/// The addon version installed in a flavour (from its TOC), if any.
pub fn installed_version(flavour: &Path) -> Option<String> {
    let toc = std::fs::read_to_string(
        flavour
            .join("Interface")
            .join("AddOns")
            .join(ADDON)
            .join(format!("{ADDON}.toc")),
    )
    .ok()?;
    toc.lines()
        .find_map(|l| l.strip_prefix("## Version:"))
        .map(|v| v.trim().to_string())
}

/// Copy the bundled addon into a flavour's `Interface\AddOns` (overwrites ours only).
pub fn install(flavour: &Path) -> std::io::Result<PathBuf> {
    let dir = flavour.join("Interface").join("AddOns").join(ADDON);
    std::fs::create_dir_all(&dir)?;
    for (n, body) in ADDON_FILES {
        std::fs::write(
            dir.join(n),
            body.replace("\r\n", "\n").replace('\n', "\r\n"),
        )?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> CharEntry {
        CharEntry {
            region: "us".into(),
            realm: "Area 52".into(),
            name: "Thrallé".into(),
            class: "SHAMAN".into(),
            level: 80,
            played: 1_234_567,
            at: 1_793_000_000,
            sv: 1,
            battletag: "Thrall#1234".into(),
            sig: String::new(),
        }
    }

    #[test]
    fn hash_is_stable() {
        assert_eq!(hash(b"abc"), hash(b"abc"));
        assert_ne!(hash(b"abc"), hash(b"abd"));
        assert_eq!(hash(b"").len(), 16);
    }

    #[test]
    fn key_matches_the_addon() {
        let lua = ADDON_FILES
            .iter()
            .find(|(n, _)| n.ends_with(".lua"))
            .unwrap()
            .1;
        let (v, k) = SIG_KEYS.last().unwrap();
        assert!(
            lua.contains(&format!("local SIG_VERSION = {v} ")),
            "addon SIG_VERSION"
        );
        assert!(lua.contains(&format!("local KEY = \"{k}\"")), "addon KEY");
        let toc = ADDON_FILES[0].1;
        assert!(toc.contains(&format!("## Version: {ADDON_VERSION}")));
    }

    /// The fixture is what the real addon wrote under Lua 5.1 (see
    /// `apps/desktop/addon/test_addon.lua`; regenerate it when the key rotates).
    #[test]
    fn lua_addon_and_rust_agree() {
        let got = entries_from_saved(include_str!("../tests/fixtures/FraglinkPlayed.lua"));
        assert_eq!(got.len(), 1);
        let mut want = entry();
        want.sig = got[0].sig.clone();
        assert_eq!(got[0], want);
        assert_eq!(verify(&got[0]), SigCheck::Ok);
    }

    #[test]
    fn verify_catches_edits() {
        let mut e = entry();
        e.sig = sign_with(SIG_KEYS[0].1, &e);
        assert_eq!(verify(&e), SigCheck::Ok);
        let mut x = e.clone();
        x.played += 3600;
        assert_eq!(verify(&x), SigCheck::Bad);
        let mut y = e.clone();
        y.sv = 99;
        assert_eq!(verify(&y), SigCheck::UnknownVersion);
        let mut z = e.clone();
        z.battletag = "Other#1".into();
        assert_eq!(verify(&z), SigCheck::Bad);
    }

    #[test]
    fn parses_saved_variables() {
        let mut e = entry();
        e.sig = sign_with(SIG_KEYS[0].1, &e);
        let text = format!(
            "\nFraglinkPlayedDB = {{\n\t[\"chars\"] = {{\n\t\t[\"us/Area 52/Thrallé\"] = {{\n\t\t\t[\"region\"] = \"us\",\n\t\t\t[\"realm\"] = \"Area 52\",\n\t\t\t[\"name\"] = \"Thrallé\",\n\t\t\t[\"class\"] = \"SHAMAN\",\n\t\t\t[\"level\"] = 80,\n\t\t\t[\"played\"] = 1234567,\n\t\t\t[\"at\"] = 1793000000,\n\t\t\t[\"sv\"] = 1,\n\t\t\t[\"battletag\"] = \"Thrall#1234\",\n\t\t\t[\"sig\"] = \"{}\",\n\t\t}},\n\t\t[\"bad\"] = {{ [\"realm\"] = \"x\" }}, -- skipped\n\t}},\n\t[\"version\"] = 1,\n}}\nOther = {{ 1, 2, \"a\\\"b\", true, nil, -3.5e2 }}\n",
            e.sig
        );
        let got = entries_from_saved(&text);
        assert_eq!(got, vec![e.clone()]);
        assert_eq!(verify(&got[0]), SigCheck::Ok);
        assert!(entries_from_saved("FraglinkPlayedDB = { os.exit() }").is_empty());
        assert!(entries_from_saved("garbage").is_empty());
        assert!(entries_from_saved(&"{".repeat(100)).is_empty());
    }

    #[test]
    fn flavours_from_paths() {
        let t = std::env::temp_dir().join(format!("flp-{}", std::process::id()));
        let retail = t.join("World of Warcraft").join("_retail_");
        std::fs::create_dir_all(&retail).unwrap();
        std::fs::create_dir_all(t.join("World of Warcraft").join("_classic_")).unwrap();
        let got = flavour_dirs(&[
            retail.join("Wow.exe"),
            t.join("World of Warcraft"),
            t.join("World of Warcraft")
                .join("_forever_")
                .join("Wow.exe"),
        ]);
        assert_eq!(got[0], ("wow".to_string(), retail.clone()));
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(got[1].0, "wow_forever");
        let dir = install(&retail).unwrap();
        assert!(dir.join("FraglinkPlayed.lua").is_file());
        assert_eq!(installed_version(&retail).as_deref(), Some(ADDON_VERSION));
        let sv = retail.join("WTF/Account/ABC/SavedVariables");
        std::fs::create_dir_all(&sv).unwrap();
        let mut e = entry();
        e.sig = sign_with(SIG_KEYS[0].1, &e);
        std::fs::write(
            sv.join("FraglinkPlayed.lua"),
            format!("FraglinkPlayedDB = {{ chars = {{ x = {{ region = \"us\", realm = \"Area 52\", name = \"Thrallé\", class = \"SHAMAN\", level = 80, played = 1234567, at = 1793000000, sv = 1, battletag = \"Thrall#1234\", sig = \"{}\" }} }} }}", e.sig),
        )
        .unwrap();
        assert_eq!(read_flavour(&retail), vec![e]);
        assert!(saved_stamp(&retail) > 0);
        let _ = std::fs::remove_dir_all(&t);
    }
}
