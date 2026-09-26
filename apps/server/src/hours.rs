//! Trust tiers (spec §5): Tracked (sessions + Steam playtime), Imported (game
//! counters, self-reported), Estimated (history calculator). Estimated hours are
//! shown separately and are never used for ranks.

use crate::error::AppError;
use chrono::Datelike;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Tracked,
    Imported,
    Estimated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Era {
    pub from_year: i32,
    pub to_year: i32,
    pub hours_per_week: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Estimate {
    pub minutes: i64,
    /// e.g. "~9,000h"
    pub display: String,
    pub warnings: Vec<String>,
}

pub const MAX_HOURS_PER_WEEK: f64 = 80.0;
const WARN_HOURS_PER_WEEK: f64 = 40.0;
const WEEKS_PER_YEAR: f64 = 52.0;

/// `sum(weeks_in_era × hours_per_week)` with the spec's caps and warnings.
pub fn estimate(eras: &[Era], current_year: i32) -> Result<Estimate, AppError> {
    if eras.is_empty() || eras.len() > 10 {
        return Err(AppError::bad_request(
            "invalid_eras",
            "Add between 1 and 10 eras.",
        ));
    }
    let mut warnings = Vec::new();
    let mut hours = 0.0;
    let mut sorted: Vec<&Era> = eras.iter().collect();
    sorted.sort_by_key(|e| e.from_year);
    for e in &sorted {
        if e.from_year < 1970 || e.to_year > current_year || e.from_year > e.to_year {
            return Err(AppError::bad_request(
                "invalid_years",
                format!("Years must be between 1970 and {current_year}, oldest first."),
            ));
        }
        if !(e.hours_per_week > 0.0 && e.hours_per_week <= MAX_HOURS_PER_WEEK) {
            return Err(AppError::bad_request(
                "invalid_hours",
                "Hours per week must be more than 0 and at most 80.",
            ));
        }
        let years = (e.to_year - e.from_year + 1) as f64;
        if years > 1.0 && e.hours_per_week > WARN_HOURS_PER_WEEK {
            warnings.push(format!(
                "{}-{}: more than 40 h/week for several years is a lot. Double-check it.",
                e.from_year, e.to_year
            ));
        }
        hours += years * WEEKS_PER_YEAR * e.hours_per_week;
    }
    for w in sorted.windows(2) {
        if w[1].from_year <= w[0].to_year {
            warnings.push(format!(
                "Eras {}-{} and {}-{} overlap; overlapping years are counted twice.",
                w[0].from_year, w[0].to_year, w[1].from_year, w[1].to_year
            ));
        }
    }
    let minutes = (hours * 60.0).round() as i64;
    Ok(Estimate {
        minutes,
        display: format!("~{}h", group(round_estimate_hours(minutes / 60))),
        warnings,
    })
}

pub fn current_year() -> i32 {
    chrono::Utc::now().year()
}

/// Estimated numbers are rounded to the nearest 100 h above 1,000 h (spec §5.1).
pub fn round_estimate_hours(h: i64) -> i64 {
    if h > 1000 {
        ((h + 50) / 100) * 100
    } else {
        h
    }
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

    fn era(a: i32, b: i32, h: f64) -> Era {
        Era {
            from_year: a,
            to_year: b,
            hours_per_week: h,
        }
    }

    #[test]
    fn estimator_math_and_rounding() {
        // 2005-2008 at 20 h/week = 4 * 52 * 20 = 4160 h
        let e = estimate(&[era(2005, 2008, 20.0)], 2026).unwrap();
        assert_eq!(e.minutes, 4160 * 60);
        assert_eq!(e.display, "~4,200h");
        assert!(e.warnings.is_empty());
        let small = estimate(&[era(2020, 2020, 5.0)], 2026).unwrap();
        assert_eq!(small.display, "~260h");
    }

    #[test]
    fn estimator_validation_and_warnings() {
        assert!(estimate(&[], 2026).is_err());
        assert!(estimate(&[era(2010, 2005, 10.0)], 2026).is_err());
        assert!(estimate(&[era(2020, 2030, 10.0)], 2026).is_err());
        assert!(estimate(&[era(2020, 2021, 81.0)], 2026).is_err());
        assert!(estimate(&[era(2020, 2021, 0.0)], 2026).is_err());
        let w = estimate(&[era(2005, 2008, 50.0), era(2008, 2010, 10.0)], 2026).unwrap();
        assert_eq!(w.warnings.len(), 2, "{:?}", w.warnings);
    }

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
