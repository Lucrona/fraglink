//! World of Warcraft realm, shown as the game detail ("World of Warcraft · Frostmourne").
//!
//! WoW names the realm in the folders it keeps per character,
//! `<flavour folder>\WTF\Account\<account>\<realm>\<character>\`. That is the
//! primary source for every flavour: the realm whose character folder changed most
//! recently, preferring folder names that are real realms (the server's daily copy of
//! Blizzard's realm lists, see [`set_known_realms`]); folders not on the list (beta or
//! private realms) still count when nothing better is there. The scan runs when the
//! game is detected, whenever `Config.wtf` changes, and at most every [`SCAN_EVERY`]
//! while playing. No game hooks and no memory reads.
//!
//! `SET realmName "..."` in `WTF\Config.wtf` was dropped by WoW in patch 5.4.1, so
//! modern clients never write it; it is only a last resort for old installs without
//! character folders.

/// Realm detection is on the back burner (dev.18): it was not reliable enough, so the
/// app neither reads nor shows realms. Flip to bring it back.
pub const ENABLED: bool = false;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
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
    let known = KNOWN.read().ok()?;
    realm_from_accounts_with(wtf, known.as_ref())
}

/// Real realm names (lower-case), from the server's `/api/v1/games/wow-realms`.
static KNOWN: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// Install the list of real realm names. Folder names found in it win over others
/// (beta / private realms are missing from Blizzard's list and still work as fallback).
pub fn set_known_realms<I: IntoIterator<Item = String>>(names: I) {
    let set: HashSet<String> = names.into_iter().map(|n| n.trim().to_lowercase()).collect();
    if let Ok(mut k) = KNOWN.write() {
        *k = (!set.is_empty()).then_some(set);
    }
}

/// Names from the server's realm list (`{"flavours":{flavour:{region:[names]}}}`,
/// plain or gzip). `None` when unusable or empty.
pub fn known_realms_from_bytes(b: &[u8]) -> Option<Vec<String>> {
    use std::io::Read;
    let mut raw = Vec::new();
    let json = if b.starts_with(&[0x1f, 0x8b]) {
        flate2::read::GzDecoder::new(b).read_to_end(&mut raw).ok()?;
        &raw[..]
    } else {
        b
    };
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let names: Vec<String> = v
        .get("flavours")?
        .as_object()?
        .values()
        .filter_map(|r| r.as_object())
        .flat_map(|r| r.values())
        .filter_map(|l| l.as_array())
        .flatten()
        .filter_map(|n| n.as_str().map(str::to_string))
        .collect();
    (!names.is_empty()).then_some(names)
}

/// [`realm_from_accounts`] with an explicit list: the most recently played realm that is
/// on the list; when none is (or there is no list), the most recently played folder.
pub fn realm_from_accounts_with(wtf: &Path, known: Option<&HashSet<String>>) -> Option<String> {
    let mut best: Option<(SystemTime, String)> = None;
    let mut best_known: Option<(SystemTime, String)> = None;
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
            let name = name.trim().to_string();
            if known.is_some_and(|k| k.contains(&name.to_lowercase()))
                && best_known.as_ref().is_none_or(|(t, _)| when > *t)
            {
                best_known = Some((when, name.clone()));
            }
            if best.as_ref().is_none_or(|(t, _)| when > *t) {
                best = Some((when, name));
            }
        }
    }
    best_known.or(best).map(|(_, n)| n)
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
            self.scanned = None; // WoW rewrites Config.wtf on logout / realm change
            self.config_realm = stamp.and_then(|_| {
                let bytes = std::fs::read(&self.path).ok()?;
                parse_realm(&String::from_utf8_lossy(&bytes))
            });
        }
        let due = self
            .scanned
            .as_ref()
            .is_none_or(|(t, _)| t.elapsed() >= SCAN_EVERY);
        if due {
            let found = self.path.parent().and_then(realm_from_accounts);
            self.scanned = Some((Instant::now(), found));
        }
        let from_folders = self.scanned.as_ref().and_then(|(_, r)| r.clone());
        let realm = from_folders.clone().or_else(|| self.config_realm.clone());
        let changed = realm != self.realm;
        if changed || first {
            log::info!(
                "wow realm from {}{}: {}",
                self.path.display(),
                if from_folders.is_some() {
                    " (account folders)"
                } else {
                    " (Config.wtf realmName)"
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
    /// No character folders (old install): `realmName` is the last resort.
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
        // the folders win even over a stale realmName (dropped by WoW in 5.4.1)
        std::fs::write(wtf.join("Config.wtf"), "SET realmName \"Mankrik\"\n").unwrap();
        assert!(!w.poll());
        assert_eq!(w.realm.as_deref(), Some("Classic Beta PvE"));
        // a newer character on the other realm is picked up when Config.wtf changes
        touch(&acc.join("Classic Beta PvP/Lucro/SavedVariables.lua"), 7200);
        std::fs::write(
            wtf.join("Config.wtf"),
            "SET realmName \"Mankrik\"\nSET x \"1\"\n",
        )
        .unwrap();
        assert!(w.poll());
        assert_eq!(w.realm.as_deref(), Some("Classic Beta PvP"));
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

    #[test]
    fn realm_list_bytes() {
        let j =
            br#"{"v":1,"flavours":{"retail":{"us":["Frostmourne"]},"classic":{"eu":["Firemaw"]}}}"#;
        let mut n = known_realms_from_bytes(j).unwrap();
        n.sort();
        assert_eq!(n, ["Firemaw", "Frostmourne"]);
        assert!(known_realms_from_bytes(b"{}").is_none());
    }

    #[test]
    fn known_realm_names_win_and_unknown_ones_are_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let wtf = dir.path().join("WTF");
        let a = wtf.join("Account").join("ONE");
        std::fs::create_dir_all(a.join("Frostmourne/Aaa")).unwrap();
        std::fs::create_dir_all(a.join("Some Addon Junk/Bbb")).unwrap();
        touch(&a.join("Frostmourne/Aaa/c.lua"), -600);
        touch(&a.join("Some Addon Junk/Bbb/c.lua"), 600);
        let known: HashSet<String> = ["frostmourne".to_string()].into();
        assert_eq!(
            realm_from_accounts_with(&wtf, Some(&known)).as_deref(),
            Some("Frostmourne")
        );
        assert_eq!(
            realm_from_accounts_with(&wtf, None).as_deref(),
            Some("Some Addon Junk")
        );
        // beta realm not on Blizzard's list: heuristic still answers
        let beta = dir.path().join("BETA");
        std::fs::create_dir_all(beta.join("Account/X/Classic Beta PvE/Ccc")).unwrap();
        assert_eq!(
            realm_from_accounts_with(&beta, Some(&known)).as_deref(),
            Some("Classic Beta PvE")
        );
    }
}
