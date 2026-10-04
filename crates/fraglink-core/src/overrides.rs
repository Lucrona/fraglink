//! Fraglink's own detection rules, checked before every other list (first match
//! wins). They split or merge games that share an exe by install folder: Star
//! Citizen's LIVE / PTU / EPTU channels are one game. (World of Warcraft follows
//! Discord's list as-is since 0.2.0-dev.14.) The server also appends the games an
//! admin approved from "Track this as a game?" answers (`exe:<name>.exe` ids).
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
    fn wow_is_left_to_the_signatures_and_discord_list() {
        // 0.2.0-dev.14: WoW follows Discord's list as-is, no folder rules of our own.
        let o = Overrides::builtin();
        for (exe, p) in [
            (
                "Wow.exe",
                r"E:\Battle.net Games\World of Warcraft\_retail_\Wow.exe",
            ),
            (
                "WowB.exe",
                r"E:\Battle.net Games\World of Warcraft\_classic_beta_\WowB.exe",
            ),
            ("WowClassic.exe", r"C:\WoW\_classic_era_\WowClassic.exe"),
            ("WowT.exe", r"C:\WoW\_ptr_\WowT.exe"),
        ] {
            assert!(o.match_process(exe, Some(p)).is_none(), "{p}");
        }
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
