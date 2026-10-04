//! World of Warcraft realm, shown as the game detail ("World of Warcraft · Frostmourne").
//!
//! Every WoW flavour keeps its settings next to the exe, in
//! `<flavour folder>\WTF\Config.wtf`, with a line `SET realmName "Frostmourne"`.
//! The file is read when that WoW starts and again whenever its modification
//! time or size changes (checked on the normal 5 s poll). No game hooks and no
//! memory reads: just a small text file the game itself writes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Fraglink game ids of every WoW flavour (`wow`, `wow_classic`, `wow_forever`, ...).
pub fn is_wow(game_id: &str) -> bool {
    game_id == "wow" || game_id.starts_with("wow_")
}

/// `<folder of the exe>/WTF/Config.wtf`.
pub fn config_path(exe_path: &str) -> Option<PathBuf> {
    let p = exe_path.replace('\\', "/");
    let (dir, _) = p.rsplit_once('/')?;
    if dir.is_empty() {
        return None;
    }
    Some(Path::new(dir).join("WTF").join("Config.wtf"))
}

/// The last `SET realmName "..."` line (case-insensitive), trimmed; None if absent or empty.
pub fn parse_realm(text: &str) -> Option<String> {
    let mut found = None;
    for line in text.lines() {
        let mut it = line.trim().splitn(3, char::is_whitespace);
        let (Some(set), Some(key), Some(rest)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if !set.eq_ignore_ascii_case("set") || !key.eq_ignore_ascii_case("realmname") {
            continue;
        }
        let v = rest.trim();
        let v = v.strip_prefix('"').unwrap_or(v);
        let v = match v.find('"') {
            Some(i) => &v[..i],
            None => v,
        };
        let v: String = v.chars().filter(|c| !c.is_control()).take(64).collect();
        let v = v.trim();
        found = (!v.is_empty()).then(|| v.to_string());
    }
    found
}

/// File identity used to spot a change: (modified time, size); None = no file.
type Stamp = Option<(Option<SystemTime>, u64)>;

/// Watches one Config.wtf.
#[derive(Debug, Clone)]
pub struct RealmWatch {
    pub path: PathBuf,
    /// None until the first poll.
    stamp: Option<Stamp>,
    pub realm: Option<String>,
}

impl RealmWatch {
    pub fn new(path: PathBuf) -> Self {
        RealmWatch {
            path,
            stamp: None,
            realm: None,
        }
    }

    /// Re-read the file if it changed (or on the first call). True when the realm changed.
    pub fn poll(&mut self) -> bool {
        let stamp: Stamp = std::fs::metadata(&self.path)
            .ok()
            .map(|m| (m.modified().ok(), m.len()));
        if self.stamp == Some(stamp) {
            return false;
        }
        let first = self.stamp.is_none();
        self.stamp = Some(stamp);
        let realm = stamp.and_then(|_| {
            let bytes = std::fs::read(&self.path).ok()?;
            parse_realm(&String::from_utf8_lossy(&bytes))
        });
        let changed = realm != self.realm;
        if changed || first {
            log::info!(
                "wow realm from {}: {}",
                self.path.display(),
                realm.as_deref().unwrap_or("(none)")
            );
        }
        self.realm = realm;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_realm_lines() {
        let cfg = "SET locale \"enUS\"\r\nSET portal \"US\"\r\nSET realmName \"Frostmourne\"\r\nSET gxWindow \"1\"\r\n";
        assert_eq!(parse_realm(cfg).as_deref(), Some("Frostmourne"));
        assert_eq!(
            parse_realm("set REALMNAME \"Aggra (Português)\"").as_deref(),
            Some("Aggra (Português)")
        );
        assert_eq!(parse_realm("SET realmName \"\"\n"), None);
        assert_eq!(
            parse_realm("SET realmNameX \"Nope\"\nSET lastCharacterIndex \"0\""),
            None
        );
        assert_eq!(parse_realm(""), None);
        // the last line wins
        assert_eq!(
            parse_realm("SET realmName \"A\"\nSET realmName \"B\"").as_deref(),
            Some("B")
        );
    }

    #[test]
    fn config_next_to_the_exe() {
        assert_eq!(
            config_path(r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe").unwrap(),
            Path::new("E:/Battle.net Games/World of Warcraft/_retail_")
                .join("WTF")
                .join("Config.wtf")
        );
        assert!(config_path("Wow.exe").is_none());
        assert!(
            is_wow("wow") && is_wow("wow_forever") && !is_wow("wowowow") && !is_wow("star_citizen")
        );
    }

    #[test]
    fn watch_rereads_on_change() {
        let dir = tempfile::tempdir().unwrap();
        let wtf = dir.path().join("_retail_").join("WTF");
        std::fs::create_dir_all(&wtf).unwrap();
        let exe = dir.path().join("_retail_").join("Wow.exe");
        let mut w = RealmWatch::new(config_path(exe.to_str().unwrap()).unwrap());
        // no file yet: nothing
        assert!(!w.poll());
        assert_eq!(w.realm, None);
        std::fs::write(wtf.join("Config.wtf"), "SET realmName \"Frostmourne\"\n").unwrap();
        assert!(w.poll());
        assert_eq!(w.realm.as_deref(), Some("Frostmourne"));
        assert!(!w.poll(), "unchanged file is not re-read");
        // different size, so the change is seen even within the same mtime tick
        std::fs::write(
            wtf.join("Config.wtf"),
            "SET realmName \"Barthilas\"\nSET x \"1\"\n",
        )
        .unwrap();
        assert!(w.poll());
        assert_eq!(w.realm.as_deref(), Some("Barthilas"));
        std::fs::remove_file(wtf.join("Config.wtf")).unwrap();
        assert!(w.poll());
        assert_eq!(w.realm, None);
    }
}
