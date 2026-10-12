//! Achievements computed from tracked sessions (no extra storage). Six tracks with
//! Bronze/Silver/Gold/Platinum tiers; cached per user for 10 minutes.

use crate::error::ApiResult;
use crate::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use uuid::Uuid;

const TTL: Duration = Duration::from_secs(600);
pub const TIERS: [&str; 4] = ["Bronze", "Silver", "Gold", "Platinum"];
const POINTS: [i64; 4] = [5, 10, 20, 40];

/// (key, name, what it counts, unit, thresholds)
const TRACKS: [(&str, &str, &str, &str, [i64; 4]); 6] = [
    (
        "clocked_in",
        "Clocked In",
        "hours played",
        "hours",
        [10, 50, 250, 1000],
    ),
    (
        "no_days_off",
        "No Days Off",
        "days in a row",
        "days",
        [3, 7, 14, 30],
    ),
    (
        "game_hopper",
        "Game Hopper",
        "games played",
        "games",
        [3, 10, 25, 50],
    ),
    (
        "regular_circuit",
        "Regular Circuit",
        "active days",
        "active days",
        [7, 30, 100, 365],
    ),
    (
        "one_more_round",
        "One More Round",
        "longest session",
        "hours",
        [2, 4, 6, 10],
    ),
    (
        "night_owl",
        "Night Owl",
        "sessions started after midnight",
        "late sessions",
        [1, 10, 50, 200],
    ),
];

fn cache() -> &'static Mutex<HashMap<Uuid, (Instant, Value)>> {
    static C: std::sync::OnceLock<Mutex<HashMap<Uuid, (Instant, Value)>>> =
        std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn forget(user: Uuid) {
    cache().lock().unwrap().remove(&user);
}

/// Longest run of consecutive days in a sorted, de-duplicated list.
pub fn longest_streak(days: &[chrono::NaiveDate]) -> i64 {
    let (mut best, mut cur) = (0i64, 0i64);
    let mut prev: Option<chrono::NaiveDate> = None;
    for d in days {
        cur = match prev {
            Some(p) if *d - p == chrono::Duration::days(1) => cur + 1,
            Some(p) if *d == p => cur,
            _ => 1,
        };
        best = best.max(cur);
        prev = Some(*d);
    }
    best
}

/// Build the achievements table from raw values (key -> value in the track's unit).
pub fn table(values: &HashMap<&str, f64>) -> Value {
    let mut rows = Vec::new();
    let (mut unlocked, mut points, mut mastered) = (0i64, 0i64, 0i64);
    let mut next: Option<(f64, Value)> = None;
    for (key, name, what, unit, th) in TRACKS {
        let v = values.get(key).copied().unwrap_or(0.0);
        let tier = th.iter().filter(|t| v >= **t as f64).count();
        unlocked += tier as i64;
        points += POINTS[..tier].iter().sum::<i64>();
        if tier == 4 {
            mastered += 1;
        }
        let (target, pct) = if tier < 4 {
            let lo = if tier == 0 { 0.0 } else { th[tier - 1] as f64 };
            let hi = th[tier] as f64;
            (
                Some(th[tier]),
                ((v - lo) / (hi - lo) * 100.0).clamp(0.0, 100.0),
            )
        } else {
            (None, 100.0)
        };
        let shown = if unit == "hours" {
            (v * 10.0).round() / 10.0
        } else {
            v.floor()
        };
        let row = json!({
            "key": key, "name": name, "what": what, "unit": unit, "value": shown,
            "tier": if tier == 0 { Value::Null } else { json!(TIERS[tier - 1]) },
            "next_tier": target.map(|_| TIERS[tier]), "next_at": target,
            "remaining": target.map(|t| ((t as f64 - v).max(0.0) * 10.0).ceil() / 10.0),
            "progress": pct.round(),
            "points": POINTS[..tier].iter().sum::<i64>(),
        });
        if tier < 4 && next.as_ref().is_none_or(|(p, _)| pct > *p) {
            next = Some((pct, row.clone()));
        }
        rows.push(row);
    }
    json!({
        "rows": rows, "unlocked": unlocked, "total_tiers": TRACKS.len() * 4,
        "points": points, "mastered": mastered, "tracks": TRACKS.len(),
        "next": next.map(|n| n.1),
    })
}

pub async fn for_user(st: &AppState, user: Uuid, hidden: &[String]) -> ApiResult<Value> {
    if let Some((at, v)) = cache().lock().unwrap().get(&user) {
        if at.elapsed() < TTL {
            return Ok(v.clone());
        }
    }
    let tz = crate::activity::DEFAULT_TZ;
    let (total_s, games, longest_s, late): (Option<i64>, i64, Option<i32>, i64) = sqlx::query_as(
        "SELECT sum(duration_s)::bigint, count(DISTINCT game_id), max(duration_s),
                count(*) FILTER (WHERE extract(hour FROM started_at AT TIME ZONE $3) < 5 AND duration_s >= 600)
         FROM sessions WHERE user_id = $1 AND NOT (game_id = ANY($2))",
    )
    .bind(user)
    .bind(hidden)
    .bind(tz)
    .fetch_one(&st.db)
    .await?;
    let days: Vec<chrono::NaiveDate> = sqlx::query_scalar(
        "SELECT DISTINCT (started_at AT TIME ZONE $3)::date d FROM sessions
         WHERE user_id = $1 AND NOT (game_id = ANY($2)) ORDER BY d",
    )
    .bind(user)
    .bind(hidden)
    .bind(tz)
    .fetch_all(&st.db)
    .await?;
    let mut vals = HashMap::new();
    vals.insert("clocked_in", total_s.unwrap_or(0) as f64 / 3600.0);
    vals.insert("no_days_off", longest_streak(&days) as f64);
    vals.insert("game_hopper", games as f64);
    vals.insert("regular_circuit", days.len() as f64);
    vals.insert("one_more_round", longest_s.unwrap_or(0) as f64 / 3600.0);
    vals.insert("night_owl", late as f64);
    let v = table(&vals);
    let mut c = cache().lock().unwrap();
    if c.len() > 5000 {
        c.clear();
    }
    c.insert(user, (Instant::now(), v.clone()));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn streaks_and_tiers() {
        let d = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(
            longest_streak(&[
                d("2026-01-01"),
                d("2026-01-02"),
                d("2026-01-04"),
                d("2026-01-05"),
                d("2026-01-06")
            ]),
            3
        );
        let mut v = HashMap::new();
        v.insert("clocked_in", 12.0);
        v.insert("game_hopper", 3.0);
        let t = table(&v);
        assert_eq!(t["unlocked"], 2);
        assert_eq!(t["points"], 10);
        assert_eq!(t["rows"][0]["tier"], "Bronze");
        assert_eq!(t["rows"][0]["next_tier"], "Silver");
    }
}
