//! Fraglink's own detection rules, checked before every other list (first match
//! wins). They split games that share an exe by install folder: World of
//! Warcraft's `_retail_` / `_classic_` / `_classic_beta_` (WoW: Forever) / `_ptr_`
//! flavours, and Star Citizen's LIVE / PTU / EPTU channels.
//!
//! Bundled from `packages/signatures/overrides.json`; the server can publish a
//! newer copy (`GET /api/v1/games/overrides`) so a renamed beta exe can be fixed
//! without an app release.

use crate::signatures::normalize_path;
use serde::{Deserialize, Serialize};

pub const BUILTIN_JSON: &str = include_str!("../../../packages/signatures/overrides.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    /// Fraglink game id (stable: stats hang off it).
    pub id: String,
    pub name: String,
    /// Lower-case exe names; the process must be one of them.
    pub exes: Vec<String>,
    /// Lower-case, forward-slash path fragments (any of). Empty = any path.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Release channel inside the same game (`PTU`), informational.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverrideFile {
    pub schema: u32,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub version: String,
    rules: Vec<Rule>,
}

impl Overrides {
    pub fn builtin() -> Self {
        Self::from_json(BUILTIN_JSON).expect("bundled overrides.json must parse")
    }

    /// Parse and validate (every rule needs an id, a name and at least one exe).
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let f: OverrideFile = serde_json::from_str(json)?;
        if f.schema != 1 {
            anyhow::bail!("overrides schema {} (want 1)", f.schema);
        }
        let mut rules = Vec::with_capacity(f.rules.len());
        for mut r in f.rules {
            if r.id.trim().is_empty() || r.name.trim().is_empty() || r.exes.is_empty() {
                anyhow::bail!("override rule {:?} needs an id, a name and exes", r.id);
            }
            r.exes = r.exes.iter().map(|e| e.trim().to_lowercase()).collect();
            r.paths = r.paths.iter().map(|p| normalize_path(p.trim())).collect();
            rules.push(r);
        }
        Ok(Overrides {
            version: f.version,
            rules,
        })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    pub fn match_process(&self, exe: &str, path: Option<&str>) -> Option<&Rule> {
        let exe_l = exe.to_lowercase();
        let path_n = path.map(normalize_path);
        self.rules.iter().find(|r| {
            r.exes.contains(&exe_l)
                && (r.paths.is_empty()
                    || path_n
                        .as_deref()
                        .is_some_and(|p| r.paths.iter().any(|f| p.contains(f.as_str()))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(exe: &str, path: &str) -> Option<(String, Option<String>)> {
        Overrides::builtin()
            .match_process(exe, Some(path))
            .map(|r| (r.id.clone(), r.channel.clone()))
    }

    #[test]
    fn wow_flavours_by_folder() {
        let o = Overrides::builtin();
        let name =
            |exe: &str, path: &str| o.match_process(exe, Some(path)).map(|r| r.name.as_str());
        // the user's real install
        assert_eq!(
            name(
                "WowB.exe",
                r"E:\Battle.net Games\World of Warcraft\_classic_beta_\WowB.exe"
            ),
            Some("World of Warcraft: Forever")
        );
        assert_eq!(
            name(
                "Wow.exe",
                r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe"
            ),
            Some("World of Warcraft")
        );
        let base = r"C:\Program Files (x86)\World of Warcraft";
        let cases = [
            ("_classic_", "WowClassic.exe", "wow_classic"),
            ("_classic_era_", "WowClassic.exe", "wow_classic_era"),
            (
                "_classic_era_ptr_",
                "WowClassicT.exe",
                "wow_classic_era_ptr",
            ),
            ("_classic_ptr_", "WowClassicT.exe", "wow_classic_ptr"),
            ("_anniversary_", "WowClassic.exe", "wow_anniversary"),
            ("_ptr_", "WowT.exe", "wow_ptr"),
            ("_xptr_", "WowT.exe", "wow_ptr"),
            ("_beta_", "WowB.exe", "wow_beta"),
            ("_beta_", "Wow.exe", "wow_beta"),
            ("_retail_", "Wow-64.exe", "wow"),
            ("_classic_beta_", "WowClassicB.exe", "wow_forever"),
        ];
        for (dir, exe, want) in cases {
            let p = format!(r"{base}\{dir}\{exe}");
            assert_eq!(id(exe, &p).map(|x| x.0).as_deref(), Some(want), "{p}");
        }
        // WowB.exe anywhere else is still WoW: Forever
        assert_eq!(
            id("WowB.exe", r"D:\Games\WoWForever\WowB.exe").unwrap().0,
            "wow_forever"
        );
        assert_eq!(o.match_process("WowB.exe", None).unwrap().id, "wow_forever");
        // a plain Wow.exe outside a flavour folder is left to the other lists
        assert!(id("Wow.exe", r"C:\Games\Wow.exe").is_none());
        // other programs in the WoW folders don't match
        assert!(id(
            "WowVoiceProxy.exe",
            &format!(r"{base}\_retail_\WowVoiceProxy.exe")
        )
        .is_none());
        assert!(id("Overwatch.exe", r"C:\Overwatch\_retail_\Overwatch.exe").is_none());
    }

    #[test]
    fn star_citizen_channels_are_one_game() {
        let root = r"C:\Program Files\Roberts Space Industries\StarCitizen";
        for (ch, want) in [("LIVE", "LIVE"), ("PTU", "PTU"), ("EPTU", "EPTU")] {
            let p = format!(r"{root}\{ch}\Bin64\StarCitizen.exe");
            assert_eq!(
                id("StarCitizen.exe", &p),
                Some(("star_citizen".into(), Some(want.into())))
            );
        }
        assert_eq!(
            id("StarCitizen.exe", r"D:\SC\Bin64\StarCitizen.exe"),
            Some(("star_citizen".into(), None))
        );
    }

    #[test]
    fn rejects_bad_files() {
        assert!(Overrides::from_json(r#"{"schema":2,"version":"x","rules":[]}"#).is_err());
        assert!(Overrides::from_json(
            r#"{"schema":1,"version":"x","rules":[{"id":"a","name":"A","exes":[]}]}"#
        )
        .is_err());
        let o = Overrides::from_json(
            r#"{"schema":1,"version":"x","rules":[{"id":"a","name":"A","exes":["NEW.EXE"],"paths":["\\Foo\\"]}]}"#,
        )
        .unwrap();
        assert_eq!(
            o.match_process("new.exe", Some(r"C:\foo\new.exe"))
                .unwrap()
                .id,
            "a"
        );
    }
}
