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
    /// Detection fix-ups from Admin: a detected id (`steam:793460`, an old slug) is
    /// tracked as another game (`112_operator`). Older apps ignore this field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<Alias>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Alias {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub version: String,
    rules: Vec<Rule>,
    aliases: std::collections::HashMap<String, String>,
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
            r.exes = r
                .exes
                .iter()
                .map(|e| normalize_path(e.trim()).trim_start_matches('/').to_string())
                .collect();
            r.paths = r.paths.iter().map(|p| normalize_path(p.trim())).collect();
            rules.push(r);
        }
        let aliases = f
            .aliases
            .into_iter()
            .map(|a| (a.from.trim().to_lowercase(), a.to.trim().to_string()))
            .filter(|(f, t)| !f.is_empty() && !t.is_empty() && !f.eq_ignore_ascii_case(t))
            .collect();
        Ok(Overrides {
            version: f.version,
            rules,
            aliases,
        })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// The id a detected game is tracked under (follows Admin aliases, a few hops).
    pub fn resolve<'a>(&'a self, id: &'a str) -> &'a str {
        let mut cur = id;
        for _ in 0..4 {
            match self.aliases.get(&cur.to_lowercase()) {
                Some(t) => cur = t.as_str(),
                None => break,
            }
        }
        cur
    }

    pub fn aliases_len(&self) -> usize {
        self.aliases.len()
    }

    /// First rule matching the process. An exe entry with a folder part
    /// (`world of warcraft/_retail_/wow.exe`, like Discord's list) must match the end
    /// of the process path; a bare name matches the file name.
    pub fn match_process(&self, exe: &str, path: Option<&str>) -> Option<&Rule> {
        let exe_l = exe.to_lowercase();
        let path_n = path.map(normalize_path);
        let exe_ok = |e: &String| {
            if e.contains('/') {
                e.rsplit('/').next() == Some(exe_l.as_str())
                    && path_n
                        .as_deref()
                        .is_some_and(|p| p == e || p.ends_with(&format!("/{e}")))
            } else {
                *e == exe_l
            }
        };
        self.rules.iter().find(|r| {
            r.exes.iter().any(exe_ok)
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
    fn path_fragment_exes_and_aliases() {
        let o = Overrides::from_json(
            r#"{"schema":1,"version":"x","rules":[
              {"id":"wow_forever","name":"World of Warcraft: Forever","exes":["World of Warcraft/_forever_/Wow.exe"]}],
              "aliases":[{"from":"steam:793460","to":"112_operator"},{"from":"a","to":"b"},{"from":"b","to":"c"}]}"#,
        )
        .unwrap();
        let m = |p: &str| o.match_process("Wow.exe", Some(p)).map(|r| r.id.clone());
        assert_eq!(
            m(r"E:\Games\World of Warcraft\_forever_\Wow.exe").as_deref(),
            Some("wow_forever")
        );
        assert_eq!(m(r"E:\Games\World of Warcraft\_retail_\Wow.exe"), None);
        assert_eq!(m(r"E:\Games\Not World of Warcraft\_forever_\Wow.exe"), None);
        assert!(o.match_process("Wow.exe", None).is_none());
        assert_eq!(o.resolve("steam:793460"), "112_operator");
        assert_eq!(o.resolve("STEAM:793460"), "112_operator");
        assert_eq!(o.resolve("a"), "c");
        assert_eq!(o.resolve("wow"), "wow");
        // older files without aliases still parse
        assert_eq!(Overrides::builtin().aliases_len(), 0);
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
