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

/// Addon folder name (also the SavedVariables file stem).
pub const ADDON: &str = "FraglinkPlayed";
/// Current addon release (CurseForge project 1766775). The app no longer installs it.
pub const ADDON_VERSION: &str = "1.0.1";

/// Checksum keys by signature version. Every addon release that bumps
/// `SIG_VERSION` adds its key here; the server accepts only listed versions, so an
/// old key is retired by deleting its row.
pub const SIG_KEYS: &[(i64, &str)] = &[(1, "OlApFDIsED1aJE04makEnBj7W6DZwvYG")];

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

    const ADDON_LUA: &str =
        include_str!("../../../apps/desktop/addon/FraglinkPlayed/FraglinkPlayed.lua");
    const ADDON_TOC: &str =
        include_str!("../../../apps/desktop/addon/FraglinkPlayed/FraglinkPlayed.toc");

    #[test]
    fn key_matches_the_addon() {
        let lua = ADDON_LUA;
        let (v, k) = SIG_KEYS.last().unwrap();
        assert!(
            lua.contains(&format!("local SIG_VERSION = {v} ")),
            "addon SIG_VERSION"
        );
        assert!(lua.contains(&format!("local KEY = \"{k}\"")), "addon KEY");
        assert!(ADDON_TOC.contains(&format!("## Version: {ADDON_VERSION}")));
        assert!(ADDON_TOC.contains("## X-Curse-Project-ID: 1766775"));
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
}
