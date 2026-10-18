//! Trust tiers (spec §5): Tracked (sessions + Steam playtime) and Imported (game
//! counters, self-reported). The Estimated tier (history calculator) was removed in
//! October 2026; old `hours_entries` rows with tier 'estimated' stay in the table but
//! are excluded from every total and display (migration 0025).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Tracked,
    Imported,
}

/// 12345 -> "12,345"
pub fn group(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

/// Game ids come from the tray (`wow`, `steam:570`, `epic:Min`, `manual:my-game`).
/// Manual tags are per user, so the server namespaces them to avoid clashes.
pub fn server_game_id(user_id: uuid::Uuid, client_id: &str) -> Option<String> {
    let id = client_id.trim();
    if id.is_empty() || id.len() > 120 || id.chars().any(|c| c.is_control()) {
        return None;
    }
    Some(match id.strip_prefix("manual:") {
        Some(slug) => format!("manual:{}:{slug}", user_id.simple()),
        None => id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_ids_are_namespaced() {
        let u = uuid::Uuid::nil();
        assert_eq!(server_game_id(u, "wow").unwrap(), "wow");
        assert_eq!(server_game_id(u, "steam:570").unwrap(), "steam:570");
        assert_eq!(
            server_game_id(u, "manual:my-game").unwrap(),
            "manual:00000000000000000000000000000000:my-game"
        );
        assert!(server_game_id(u, "").is_none());
        assert!(server_game_id(u, "a\nb").is_none());
    }

    #[test]
    fn grouping() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1000), "1,000");
        assert_eq!(group(1234567), "1,234,567");
    }
}
