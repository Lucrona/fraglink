//! World of Warcraft realm, shown as the game detail ("World of Warcraft · Frostmourne").
//!
//! Every WoW flavour keeps its settings next to the exe, in
//! `<flavour folder>\WTF\Config.wtf`, with a line `SET realmName "Frostmourne"`.
//! The file is read when that WoW starts and again whenever its modification
//! time or size changes (checked on the normal 5 s poll). No game hooks and no
//! memory reads: just a small text file the game itself writes.
//!
//! Some installs (e.g. the Classic beta) never write `realmName`. Then the realm is
//! taken from the folders WoW keeps per character, `WTF\Account\<account>\<realm>\<character>\`:
//! the realm whose character folder changed most recently. That scan runs when the game
//! is detected and then at most every [`SCAN_EVERY`] while playing.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often the account folders are scanned while playing (only when Config.wtf has no realm).
pub const SCAN_EVERY: Duration = Duration::from_secs(180);

fn newest(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (x, y) => x.or(y),
    }
}

/// Most recently played realm under `<WTF>/Account/*/`: a realm folder is one with at least
/// one character subfolder (`SavedVariables` and all-digit names are skipped). Its age is the
/// newest modification time of its character folders and the files directly inside them.
pub fn realm_from_accounts(wtf: &Path) -> Option<String> {
    let mut best: Option<(SystemTime, String)> = None;
    for acc in std::fs::read_dir(wtf.join("Account")).ok()?.flatten() {
        if !acc.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(realms) = std::fs::read_dir(acc.path()) else {
            continue;
        };
        for r in realms.flatten() {
            let name = r.file_name().to_string_lossy().into_owned();
            if !r.file_type().is_ok_and(|t| t.is_dir())
                || name.eq_ignore_ascii_case("SavedVariables")
                || name.chars().all(|c| c.is_ascii_digit())
                || name.starts_with('.')
            {
                continue;
            }
            let Ok(chars) = std::fs::read_dir(r.path()) else {
                continue;
            };
            let mut when: Option<SystemTime> = None;
            let mut has_char = false;
            for c in chars.flatten() {
                if !c.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                has_char = true;
                when = newest(when, c.metadata().ok().and_then(|m| m.modified().ok()));
                if let Ok(files) = std::fs::read_dir(c.path()) {
                    for f in files.flatten() {
                        when = newest(when, f.metadata().ok().and_then(|m| m.modified().ok()));
                    }
                }
            }
            let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
            if !has_char || name.trim().is_empty() {
                continue;
            }
            let when = when.unwrap_or(SystemTime::UNIX_EPOCH);
            if best.as_ref().is_none_or(|(t, _)| when > *t) {
                best = Some((when, name.trim().to_string()));
            }
        }
    }
    best.map(|(_, n)| n)
}

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
    /// Realm from Config.wtf (wins when present).
    config_realm: Option<String>,
    /// Last account-folder scan: when, and what it found.
    scanned: Option<(Instant, Option<String>)>,
}

impl RealmWatch {
    pub fn new(path: PathBuf) -> Self {
        RealmWatch {
            path,
            stamp: None,
            realm: None,
            config_realm: None,
            scanned: None,
        }
    }

    /// Re-read the file if it changed (or on the first call); without a realm in it, fall back
    /// to the account folders (rescanned every [`SCAN_EVERY`]). True when the realm changed.
    pub fn poll(&mut self) -> bool {
        let stamp: Stamp = std::fs::metadata(&self.path)
            .ok()
            .map(|m| (m.modified().ok(), m.len()));
        let first = self.stamp.is_none();
        if self.stamp != Some(stamp) {
            self.stamp = Some(stamp);
            self.config_realm = stamp.and_then(|_| {
                let bytes = std::fs::read(&self.path).ok()?;
                parse_realm(&String::from_utf8_lossy(&bytes))
            });
        }
        let realm = match &self.config_realm {
            Some(r) => Some(r.clone()),
            None => {
                let due = self
                    .scanned
                    .as_ref()
                    .is_none_or(|(t, _)| t.elapsed() >= SCAN_EVERY);
                if due {
                    let found = self.path.parent().and_then(realm_from_accounts);
                    self.scanned = Some((Instant::now(), found));
                }
                self.scanned.as_ref().and_then(|(_, r)| r.clone())
            }
        };
        let changed = realm != self.realm;
        if changed || first {
            log::info!(
                "wow realm from {}{}: {}",
                self.path.display(),
                if self.config_realm.is_none() {
                    " (account folders)"
                } else {
                    ""
                },
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

    fn touch(p: &Path, secs_from_now: i64) {
        std::fs::write(p, "x").unwrap();
        let t = if secs_from_now >= 0 {
            SystemTime::now() + Duration::from_secs(secs_from_now as u64)
        } else {
            SystemTime::now() - Duration::from_secs((-secs_from_now) as u64)
        };
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    /// `_classic_beta_` layout: no realmName, realms as folders under the account.
    fn beta_install(root: &Path) -> PathBuf {
        let wtf = root.join("_classic_beta_").join("WTF");
        let acc = wtf.join("Account").join("379572#10");
        for d in [
            "Classic Beta PvE/Grumz",
            "Classic Beta PvP/Lucro",
            "SavedVariables/Sub",
            "70/Sub",
        ] {
            std::fs::create_dir_all(acc.join(d)).unwrap();
        }
        std::fs::write(acc.join("bindings-cache.wtf"), "x").unwrap();
        std::fs::write(
            wtf.join("Config.wtf"),
            "SET portal \"test\"\nSET agentUID \"wow_classic_beta\"\n",
        )
        .unwrap();
        wtf
    }

    #[test]
    fn realm_from_account_folders_when_config_has_none() {
        let dir = tempfile::tempdir().unwrap();
        let wtf = beta_install(dir.path());
        let acc = wtf.join("Account").join("379572#10");
        touch(&acc.join("Classic Beta PvE/Grumz/SavedVariables.lua"), 3600);
        touch(
            &acc.join("Classic Beta PvP/Lucro/SavedVariables.lua"),
            -3600,
        );
        // the skipped folders are the newest of all
        touch(&acc.join("SavedVariables/Sub/x.lua"), 7200);
        touch(&acc.join("70/Sub/x.lua"), 7200);
        assert_eq!(
            realm_from_accounts(&wtf).as_deref(),
            Some("Classic Beta PvE")
        );
        let mut w = RealmWatch::new(wtf.join("Config.wtf"));
        assert!(w.poll());
        assert_eq!(w.realm.as_deref(), Some("Classic Beta PvE"));
        // Config.wtf wins once it names a realm
        std::fs::write(wtf.join("Config.wtf"), "SET realmName \"Mankrik\"\n").unwrap();
        assert!(w.poll());
        assert_eq!(w.realm.as_deref(), Some("Mankrik"));
    }

    #[test]
    fn newest_realm_across_accounts_and_no_realm_without_characters() {
        let dir = tempfile::tempdir().unwrap();
        let wtf = dir.path().join("WTF");
        let a = wtf.join("Account").join("ONE");
        let b = wtf.join("Account").join("TWO");
        std::fs::create_dir_all(a.join("Frostmourne/Aaa")).unwrap();
        std::fs::create_dir_all(b.join("Barthilas/Bbb")).unwrap();
        std::fs::create_dir_all(b.join("Empty Realm")).unwrap();
        touch(&a.join("Frostmourne/Aaa/c.lua"), -600);
        touch(&b.join("Barthilas/Bbb/c.lua"), 600);
        touch(&b.join("Empty Realm/file.txt"), 9000);
        assert_eq!(realm_from_accounts(&wtf).as_deref(), Some("Barthilas"));
        assert_eq!(realm_from_accounts(&dir.path().join("nope")), None);
    }
}
