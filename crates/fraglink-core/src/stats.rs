//! Quick-glance aggregation for the tray Stats tab (all from local SQLite).

use crate::store::Store;
use crate::tracker::NowPlaying;
use anyhow::Result;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TopGame {
    pub game_id: String,
    pub name: String,
    pub seven_day_seconds: i64,
    pub total_seconds: i64,
    /// Trust tier (spec §5). Everything in M1 is Fraglink-tracked.
    pub tier: &'static str,
    pub last_played: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DayTotal {
    pub day: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Glance {
    pub now_playing: Option<NowPlaying>,
    pub paused: bool,
    pub today: String,
    pub today_seconds: i64,
    pub today_by_game: Vec<(String, String, i64)>,
    /// Monday-start calendar week so far.
    pub week_seconds: i64,
    pub last_week_seconds: i64,
    /// Rolling last 7 days including today.
    pub seven_day_seconds: i64,
    pub last_7_days: Vec<DayTotal>,
    /// Consecutive days (ending today, or yesterday if nothing yet today) with >= 1 min of play.
    pub streak_days: i64,
    pub total_seconds: i64,
    pub top: Vec<TopGame>,
    pub games_tracked: i64,
}

fn d(s: NaiveDate) -> String {
    s.format("%Y-%m-%d").to_string()
}

pub fn glance(
    store: &Store,
    today_s: &str,
    now_playing: Option<NowPlaying>,
    paused: bool,
) -> Result<Glance> {
    let today = NaiveDate::parse_from_str(today_s, "%Y-%m-%d")?;
    let week_start = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    let lw_start = week_start - Duration::days(7);
    let seven_start = today - Duration::days(6);
    let from = lw_start.min(seven_start);
    let rows = store.day_totals(&d(from), &d(today))?;

    let names: HashMap<String, (String, bool, i64, Option<i64>)> = store
        .list_games()?
        .into_iter()
        .map(|g| {
            (
                g.id.clone(),
                (g.name, g.excluded, g.total_seconds, g.last_played),
            )
        })
        .collect();

    let sum = |a: NaiveDate, b: NaiveDate| -> i64 {
        rows.iter()
            .filter(|(day, _, _)| *day >= d(a) && *day <= d(b))
            .map(|r| r.2)
            .sum()
    };

    let mut today_by: HashMap<String, i64> = HashMap::new();
    let mut seven_by: HashMap<String, i64> = HashMap::new();
    for (day, gid, secs) in &rows {
        if *day == d(today) {
            *today_by.entry(gid.clone()).or_default() += secs;
        }
        if *day >= d(seven_start) {
            *seven_by.entry(gid.clone()).or_default() += secs;
        }
    }
    let name_of = |id: &str| {
        names
            .get(id)
            .map(|n| n.0.clone())
            .unwrap_or_else(|| id.to_string())
    };

    let mut today_by_game: Vec<_> = today_by
        .into_iter()
        .map(|(id, s)| (id.clone(), name_of(&id), s))
        .collect();
    today_by_game.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));

    let last_7_days = (0..7)
        .map(|i| {
            let day = seven_start + Duration::days(i);
            DayTotal {
                day: d(day),
                seconds: sum(day, day),
            }
        })
        .collect();

    // top 5: by 7-day time, then fill with all-time favourites
    let mut top: Vec<TopGame> = names
        .iter()
        .filter(|(_, v)| !v.1 && v.2 > 0)
        .map(|(id, v)| TopGame {
            game_id: id.clone(),
            name: v.0.clone(),
            seven_day_seconds: *seven_by.get(id).unwrap_or(&0),
            total_seconds: v.2,
            tier: "tracked",
            last_played: v.3,
        })
        .collect();
    top.sort_by(|a, b| {
        b.seven_day_seconds
            .cmp(&a.seven_day_seconds)
            .then(b.total_seconds.cmp(&a.total_seconds))
            .then(a.name.cmp(&b.name))
    });
    top.truncate(5);

    // streak
    let played = store.played_days_desc(60, 1000)?;
    let mut streak = 0;
    let mut expect = if played.first().map(|s| s.as_str()) == Some(d(today).as_str()) {
        today
    } else {
        today - Duration::days(1)
    };
    for day in &played {
        if *day == d(expect) {
            streak += 1;
            expect -= Duration::days(1);
        } else if *day < d(expect) {
            break;
        }
    }

    let total_seconds = names.values().filter(|v| !v.1).map(|v| v.2).sum();
    let games_tracked = names.values().filter(|v| !v.1 && v.2 > 0).count() as i64;

    Ok(Glance {
        now_playing,
        paused,
        today: d(today),
        today_seconds: sum(today, today),
        today_by_game,
        week_seconds: sum(week_start, today),
        last_week_seconds: sum(lw_start, week_start - Duration::days(1)),
        seven_day_seconds: sum(seven_start, today),
        last_7_days,
        streak_days: streak,
        total_seconds,
        top,
        games_tracked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Insert a closed session with explicit per-day seconds.
    fn add(s: &mut Store, game: &str, days: &[(&str, i64)]) {
        s.upsert_game(game, &game.to_uppercase(), "signature", None, 0)
            .unwrap();
        let id = s
            .open_session(
                &uuid::Uuid::new_v4().to_string(),
                game,
                0,
                "signature",
                None,
                "t",
            )
            .unwrap();
        let map: BTreeMap<String, i64> = days.iter().map(|(d, s)| (d.to_string(), *s)).collect();
        let total = map.values().sum();
        s.close_session(id, game, 10, total, 0, &map).unwrap();
    }

    #[test]
    fn aggregates_today_week_streak_and_top() {
        let mut s = Store::open_in_memory().unwrap();
        // today = Sat 2026-09-26; week starts Mon 2026-09-21
        add(&mut s, "wow", &[("2026-09-26", 3600), ("2026-09-25", 7200)]);
        add(&mut s, "cs2", &[("2026-09-24", 1800), ("2026-09-23", 1800)]);
        add(&mut s, "rust", &[("2026-09-17", 36000)]); // last week (Thu)
        add(&mut s, "old", &[("2026-08-01", 90000)]);
        add(&mut s, "tiny", &[("2026-09-22", 30)]); // under a minute: not a streak day
        let g = glance(&s, "2026-09-26", None, false).unwrap();
        assert_eq!(g.today_seconds, 3600);
        assert_eq!(g.today_by_game, vec![("wow".into(), "WOW".into(), 3600)]);
        assert_eq!(g.week_seconds, 3600 + 7200 + 1800 + 1800 + 30);
        assert_eq!(g.last_week_seconds, 36000);
        assert_eq!(g.seven_day_seconds, 3600 + 7200 + 3600 + 30); // 20th..26th
        assert_eq!(g.streak_days, 4); // 23, 24, 25, 26
        assert_eq!(g.last_7_days.len(), 7);
        assert_eq!(
            g.last_7_days[6],
            DayTotal {
                day: "2026-09-26".into(),
                seconds: 3600
            }
        );
        let ids: Vec<_> = g.top.iter().map(|t| t.game_id.as_str()).collect();
        assert_eq!(ids, vec!["wow", "cs2", "tiny", "old", "rust"]);
        assert!(g.top.iter().all(|t| t.tier == "tracked"));
        assert_eq!(g.total_seconds, 10800 + 3600 + 36000 + 90000 + 30);
    }

    #[test]
    fn streak_counts_from_yesterday_and_excluded_games_are_hidden() {
        let mut s = Store::open_in_memory().unwrap();
        add(&mut s, "wow", &[("2026-09-25", 600), ("2026-09-24", 600)]);
        add(&mut s, "idle_app", &[("2026-09-26", 9999)]);
        s.set_game_excluded("idle_app", true).unwrap();
        let g = glance(&s, "2026-09-26", None, false).unwrap();
        assert_eq!(g.today_seconds, 0);
        assert_eq!(g.streak_days, 2);
        assert!(g.top.iter().all(|t| t.game_id != "idle_app"));
        assert_eq!(g.total_seconds, 1200);
    }

    #[test]
    fn empty_store() {
        let s = Store::open_in_memory().unwrap();
        let g = glance(&s, "2026-09-26", None, true).unwrap();
        assert_eq!(
            (g.today_seconds, g.week_seconds, g.streak_days, g.top.len()),
            (0, 0, 0, 0)
        );
        assert!(g.paused);
    }
}
