//! Optional fallback game icons from the classic Xfire client (private testing).
//! Off unless `FRAGLINK_XFIRE_ICONS=1`. The PNGs and `manifest.json` (made by
//! `scripts/xfire_icons.py`) live in `FRAGLINK_XFIRE_ICONS_DIR` (default
//! `/var/lib/fraglink/icons/xfire`) and Caddy serves them at `/icons/xfire/`.
//! To remove: unset the flag (or delete the directory); nothing else depends on it.

use fraglink_core::signatures::SignatureDb;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Deserialize)]
struct Manifest {
    icons: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    file32: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    norm: Option<String>,
    #[serde(default)]
    exes: Vec<String>,
    #[serde(default)]
    xfire_ids: Vec<u32>,
}

#[derive(Default)]
pub struct XfireIcons {
    by_exe: HashMap<String, String>,
    by_name: HashMap<String, String>,
}

/// Same normalisation as the extraction script: lower case, punctuation to spaces.
pub fn norm(s: &str) -> String {
    let s = s.to_lowercase().replace('&', " and ");
    let s: String = s
        .chars()
        .map(|c| {
            if "™®©:'’.,!?()[]-_/".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl XfireIcons {
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        let m: Manifest = serde_json::from_slice(&std::fs::read(dir.join("manifest.json"))?)?;
        Ok(Self::from_entries(m.icons))
    }

    fn from_entries(icons: Vec<Entry>) -> Self {
        let icons: Vec<Entry> = icons
            .into_iter()
            .filter(|e| !e.file32.contains('/') && !e.file32.contains(".."))
            .collect();
        // Exe names shared by several games (steam.exe, launcher.exe, game.exe, hl.exe...)
        // say nothing about the game, so only exes unique to one icon are used.
        let mut exe_count: HashMap<String, usize> = HashMap::new();
        for e in &icons {
            let mut ex: Vec<String> = e.exes.iter().map(|x| x.to_lowercase()).collect();
            ex.sort();
            ex.dedup();
            for x in ex {
                *exe_count.entry(x).or_default() += 1;
            }
        }
        // "Halo" -> "Halo: Combat Evolved": a bare series name falls back to the
        // oldest Xfire entry (lowest id) whose title starts with it.
        let mut base: HashMap<String, (u32, String)> = HashMap::new();
        let mut me = XfireIcons::default();
        for e in &icons {
            for x in &e.exes {
                let x = x.to_lowercase();
                if exe_count.get(&x) == Some(&1) {
                    me.by_exe.insert(x, e.file32.clone());
                }
            }
            if let Some(n) = &e.norm {
                me.by_name
                    .entry(n.clone())
                    .or_insert_with(|| e.file32.clone());
            }
            if let Some(b) = e
                .name
                .as_deref()
                .and_then(|n| n.split_once(':'))
                .map(|(b, _)| norm(b))
            {
                let id = e.xfire_ids.iter().copied().min().unwrap_or(u32::MAX);
                let cur = base.entry(b).or_insert((id, e.file32.clone()));
                if id < cur.0 {
                    *cur = (id, e.file32.clone());
                }
            }
        }
        for (b, (_, f)) in base {
            me.by_name.entry(b).or_insert(f);
        }
        me
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// File name for a Fraglink game: its signature's exe names first, then the name.
    pub fn lookup(&self, sigs: &SignatureDb, game_id: &str, name: Option<&str>) -> Option<&str> {
        let sig = sigs.get(game_id);
        if let Some(s) = sig {
            for exe in s.exes.iter().chain(s.path_exes.iter()) {
                if let Some(f) = self.by_exe.get(&exe.to_lowercase()) {
                    return Some(f);
                }
            }
        }
        [name, sig.map(|s| s.name.as_str())]
            .into_iter()
            .flatten()
            .find_map(|n| self.by_name.get(&norm(n)))
            .map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exe_then_name() {
        let mut x = XfireIcons::default();
        x.by_exe
            .insert("wow.exe".into(), "world-of-warcraft--wow-32.png".into());
        x.by_name.insert(
            "counter strike 1 6".into(),
            "counter-strike-1-6--cs-32.png".into(),
        );
        let sigs = SignatureDb::builtin();
        assert_eq!(
            x.lookup(&sigs, "wow", None),
            Some("world-of-warcraft--wow-32.png")
        );
        assert_eq!(
            x.lookup(&sigs, "manual:x", Some("Counter-Strike 1.6")),
            Some("counter-strike-1-6--cs-32.png")
        );
        assert_eq!(x.lookup(&sigs, "manual:y", Some("Nope")), None);
        assert_eq!(
            norm("Medal of Honor: Allied Assault"),
            "medal of honor allied assault"
        );
    }

    fn ent(id: u32, file: &str, name: &str, exes: &[&str]) -> Entry {
        Entry {
            file32: file.into(),
            name: Some(name.into()),
            norm: Some(norm(name)),
            exes: exes.iter().map(|s| s.to_string()).collect(),
            xfire_ids: vec![id],
        }
    }

    #[test]
    fn shared_exes_and_base_names() {
        let x = XfireIcons::from_entries(vec![
            ent(8509, "mcc.png", "Halo: The Master Chief Collection", &[]),
            ent(4107, "halo.png", "Halo: Combat Evolved", &["halo.exe"]),
            ent(
                1,
                "a.png",
                "Medal of Honor: Allied Assault",
                &["mohaa.exe", "steam.exe"],
            ),
            ent(
                2,
                "b.png",
                "Medal of Honor: Airborne",
                &["moha.exe", "steam.exe"],
            ),
        ]);
        assert!(!x.by_exe.contains_key("steam.exe"));
        assert_eq!(x.by_exe.get("mohaa.exe").map(|s| s.as_str()), Some("a.png"));
        assert_eq!(x.by_name.get("halo").map(|s| s.as_str()), Some("halo.png"));
        assert_eq!(
            x.by_name.get("medal of honor").map(|s| s.as_str()),
            Some("a.png")
        );
    }
}
