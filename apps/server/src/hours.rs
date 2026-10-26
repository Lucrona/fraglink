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
/// Leaderboard week: a CTE `wk(user_id, game_id, m)` with each user's minutes per game over the
/// last 7 days. Like a profile's total, it takes the larger of the app's sessions and the game's
/// own counter (Imported: manual hours, WoW /played), never the sum, so the same play isn't counted
/// twice. Imported counts only what the counter grew by within the 7 days. Its baseline is the
/// newest entry from before the 7 days, else the first entry inside them, so a first import never
/// counts as this week's play. The growth is capped at the 7 days' real time. Steam only reports a
/// two-week figure, so Steam counts in all-time totals (display_min) but not here.
///
/// WoW addon /played (source `wow_addon`) doesn't count through the imported entries: only
/// characters verified on the player's Battle.net account count, each by how much its own
/// /played grew (same baseline rule, per character), summed per game and capped at 7 days.
pub const WEEK_CTE: &str = "wk AS NOT MATERIALIZED (
    SELECT user_id, game_id, max(m)::bigint AS m FROM (
        SELECT s.user_id, s.game_id, sum(s.duration_s) / 60 AS m FROM sessions s
         WHERE s.started_at >= now() - interval '7 days' GROUP BY 1, 2
        UNION ALL
        SELECT e.user_id, e.game_id,
               LEAST(GREATEST((array_agg(e.minutes ORDER BY e.created_at DESC, e.id DESC))[1]
                   - coalesce((SELECT b.minutes FROM hours_entries b
                                WHERE b.user_id = e.user_id AND b.game_id = e.game_id AND b.tier = 'imported'
                                  AND b.created_at < now() - interval '7 days'
                                ORDER BY b.created_at DESC, b.id DESC LIMIT 1),
                              (array_agg(e.minutes ORDER BY e.created_at, e.id))[1]), 0), 7 * 24 * 60)
          FROM hours_entries e WHERE e.tier = 'imported' AND e.source <> 'wow_addon'
           AND e.created_at >= now() - interval '7 days'
         GROUP BY 1, 2
        UNION ALL
        SELECT c.user_id, c.game_id, LEAST(sum(g.m), 7 * 24 * 60)
          FROM wow_characters c
          JOIN LATERAL (
              SELECT GREATEST((array_agg(p.played_s ORDER BY p.at DESC))[1]
                   - coalesce((SELECT b.played_s FROM wow_played_snaps b
                                WHERE b.char_id = c.id AND b.at < now() - interval '7 days'
                                ORDER BY b.at DESC LIMIT 1),
                              (array_agg(p.played_s ORDER BY p.at))[1]), 0) / 60 AS m
                FROM wow_played_snaps p
               WHERE p.char_id = c.id AND p.at >= now() - interval '7 days'
               HAVING count(*) > 0
          ) g ON true
         WHERE c.verified
         GROUP BY 1, 2
    ) x GROUP BY 1, 2 HAVING max(m) > 0)";

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
