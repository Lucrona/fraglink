//! "What's new" after an update: the installed version's section of the repo's
//! CHANGELOG.md, compiled in, so the pop-up always matches the changelog.
//! Only the New / Changes / Fixes headings are shown.

use serde::Serialize;

const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

#[derive(Serialize, Debug, Default, PartialEq)]
pub struct WhatsNew {
    pub version: String,
    /// `YYYY-MM-DD` from the changelog heading.
    pub date: String,
    pub new: Vec<String>,
    pub changes: Vec<String>,
    pub fixes: Vec<String>,
}

/// The section `## [version] - date` of `text`, or None when there isn't one.
pub fn parse(text: &str, version: &str) -> Option<WhatsNew> {
    let head = format!("## [{version}]");
    let mut lines = text.lines();
    let first = lines.find(|l| l.starts_with(&head))?;
    let mut w = WhatsNew {
        version: version.to_string(),
        date: first[head.len()..]
            .trim()
            .trim_start_matches('-')
            .trim()
            .to_string(),
        ..Default::default()
    };
    let mut cur: Option<&mut Vec<String>> = None;
    for l in lines {
        if l.starts_with("## ") {
            break;
        }
        if let Some(h) = l.strip_prefix("### ") {
            cur = match h.trim() {
                "New" => Some(&mut w.new),
                "Changes" => Some(&mut w.changes),
                "Fixes" => Some(&mut w.fixes),
                _ => None,
            };
        } else if let (Some(item), Some(v)) = (l.strip_prefix("- "), cur.as_mut()) {
            v.push(item.trim().to_string());
        }
    }
    Some(w)
}

/// What's new in this build.
pub fn current() -> Option<WhatsNew> {
    parse(CHANGELOG, env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_version_has_changelog_notes() {
        let w = current().expect("CHANGELOG.md needs a section for this version");
        assert_eq!(w.date.len(), 10, "date like 2026-11-02: {:?}", w.date);
        assert!(w.new.len() + w.changes.len() + w.fixes.len() > 0);
    }

    #[test]
    fn parses_only_new_changes_fixes() {
        let t = "# C\n\n## [Unreleased]\n\n## [1.0.1] - 2026-01-02\n\nIntro.\n\n### New\n- A\n- B\n\n### Bug\n- hidden\n\n### Fixes\n- C\n\n## [1.0.0] - 2026-01-01\n\n### New\n- old\n";
        let w = parse(t, "1.0.1").unwrap();
        assert_eq!(w.date, "2026-01-02");
        assert_eq!(w.new, ["A", "B"]);
        assert!(w.changes.is_empty());
        assert_eq!(w.fixes, ["C"]);
        assert!(parse(t, "1.0").is_none());
        assert_eq!(parse(t, "1.0.0").unwrap().new, ["old"]);
    }
}
