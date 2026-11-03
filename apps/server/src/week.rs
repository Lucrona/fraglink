//! "This week" for the home page: one definition shared by the headline hours, the daily
//! bars, the per-game weekly totals and the friends "This week" leaderboard, so they always
//! add up to each other.
//!
//! **Week** = the 7 local calendar days ending today (today and the 6 days before it), in
//! the viewer's time zone: from local midnight 6 days ago up to now. Days are split at local
//! midnight, so a DST change gives a 23 or 25 hour day.
//!
//! **Time zone** = the viewer's browser zone (`?tz=`, an IANA name checked against
//! Postgres), else [`crate::activity::DEFAULT_TZ`] (Australia/Sydney). Friends on the
//! leaderboard are measured over the same window in the viewer's zone, so the rows compare.
//!
//! **Hours** = tracked play time (`play_sessions`: stored sessions plus the game running
//! now). A session counts as active for its last `duration_s` seconds before `ended_at`
//! (idle time trimmed off the start, as the friend summary does), clipped to the window and
//! split at local midnights. Everything is whole seconds; nothing is rounded until display.
//! Imported counters (Steam totals, manual imports, WoW /played growth) have no time of day,
//! so they can't go into daily bars and are not part of this week (the leaderboards page
//! still uses them).
//!
//! **Display**: hours with one decimal. The day and game tenths are apportioned from the
//! total's tenths (largest remainder), so the numbers on screen add up exactly too.

use crate::error::ApiResult;
use crate::AppState;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use std::collections::HashMap;
use uuid::Uuid;

pub const DAYS: usize = 7;

/// The window: 7 local days, `bounds[i]..bounds[i + 1]` is day `i` (`bounds[7]` = now).
#[derive(Debug, Clone)]
pub struct Window {
    pub tz: Tz,
    pub dates: [NaiveDate; DAYS],
    /// Unix seconds: start of each day, then the end of the window (now).
    pub bounds: [i64; DAYS + 1],
}

/// UTC instant of local midnight on `d` (the first valid local time that day when a DST
/// gap swallows midnight).
pub fn local_midnight(tz: Tz, d: NaiveDate) -> DateTime<Utc> {
    let mut t = d.and_hms_opt(0, 0, 0).expect("midnight");
    for _ in 0..24 * 4 {
        if let Some(x) = tz.from_local_datetime(&t).earliest() {
            return x.with_timezone(&Utc);
        }
        t += Duration::minutes(15);
    }
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).expect("midnight"))
}

pub fn window(tz: Tz, now: DateTime<Utc>) -> Window {
    let today = now.with_timezone(&tz).date_naive();
    let dates: [NaiveDate; DAYS] =
        std::array::from_fn(|i| today - Duration::days((DAYS - 1 - i) as i64));
    let mut bounds = [0i64; DAYS + 1];
    for (i, d) in dates.iter().enumerate() {
        bounds[i] = local_midnight(tz, *d).timestamp();
    }
    bounds[DAYS] = now.timestamp().max(bounds[DAYS - 1]);
    Window { tz, dates, bounds }
}

/// Parse a zone name; unknown names fall back to the site default.
pub fn parse_tz(name: &str) -> Tz {
    name.parse::<Tz>().unwrap_or_else(|_| {
        crate::activity::DEFAULT_TZ
            .parse()
            .unwrap_or(chrono_tz::Australia::Sydney)
    })
}

/// Seconds of one session inside each day of the window. The session is active for the
/// `duration_s` seconds before `ended_at` (never earlier than `started_at`).
pub fn split(
    w: &Window,
    started: DateTime<Utc>,
    ended: DateTime<Utc>,
    duration_s: i64,
) -> [i64; DAYS] {
    let end = ended.timestamp();
    let start = (end - duration_s.max(0)).max(started.timestamp().min(end));
    let mut out = [0i64; DAYS];
    for (i, o) in out.iter_mut().enumerate() {
        let (a, b) = (w.bounds[i], w.bounds[i + 1]);
        *o = (end.min(b) - start.max(a)).max(0);
    }
    out
}

/// One game's week for one person.
#[derive(Debug, Clone, Default)]
pub struct GameWeek {
    pub days: [i64; DAYS],
    pub seconds: i64,
    pub last_played: Option<DateTime<Utc>>,
}

/// One person's week: per game, and the totals derived from it.
#[derive(Debug, Clone, Default)]
pub struct UserWeek {
    pub games: HashMap<String, GameWeek>,
}

impl UserWeek {
    pub fn add(
        &mut self,
        w: &Window,
        game: &str,
        started: DateTime<Utc>,
        ended: DateTime<Utc>,
        dur: i64,
    ) {
        let d = split(w, started, ended, dur);
        let s: i64 = d.iter().sum();
        if s <= 0 {
            return;
        }
        let g = self.games.entry(game.to_string()).or_default();
        for (a, b) in g.days.iter_mut().zip(d) {
            *a += b;
        }
        g.seconds += s;
        let end = ended.min(DateTime::from_timestamp(w.bounds[DAYS], 0).unwrap_or(ended));
        g.last_played = Some(g.last_played.map_or(end, |x| x.max(end)));
    }
    pub fn days(&self) -> [i64; DAYS] {
        let mut out = [0i64; DAYS];
        for g in self.games.values() {
            for (a, b) in out.iter_mut().zip(g.days) {
                *a += b;
            }
        }
        out
    }
    pub fn seconds(&self) -> i64 {
        self.games.values().map(|g| g.seconds).sum()
    }
}

/// Everyone's week over one window.
#[derive(Debug, Clone)]
pub struct Week {
    pub window: Window,
    pub users: HashMap<Uuid, UserWeek>,
}

impl Week {
    pub fn user(&self, u: Uuid) -> UserWeek {
        self.users.get(&u).cloned().unwrap_or_default()
    }
}

/// Load the week for `users` over `window`. `own` keeps its hidden games (they still count
/// in your own totals); everyone else's hidden games are left out.
pub async fn load(
    st: &AppState,
    window: Window,
    users: &[Uuid],
    own: Option<Uuid>,
) -> ApiResult<Week> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, DateTime<Utc>, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT s.user_id, s.game_id, s.started_at, s.ended_at, s.duration_s FROM play_sessions s
         WHERE s.user_id = ANY($1) AND s.ended_at > to_timestamp($2) AND s.started_at < to_timestamp($3)
           AND (s.user_id = $4 OR NOT EXISTS (SELECT 1 FROM hidden_games h WHERE h.user_id = s.user_id AND h.game_id = s.game_id))",
    )
    .bind(users)
    .bind(window.bounds[0] as f64)
    .bind(window.bounds[DAYS] as f64)
    .bind(own)
    .fetch_all(&st.db)
    .await?;
    let mut out: HashMap<Uuid, UserWeek> = HashMap::new();
    for (u, g, a, b, d) in rows {
        out.entry(u).or_default().add(&window, &g, a, b, d as i64);
    }
    Ok(Week { window, users: out })
}

/// Tenths of an hour for `seconds`, rounded half up.
pub fn tenths(seconds: i64) -> i64 {
    (seconds.max(0) * 10 + 1800) / 3600
}

/// Split `total` whole units over `parts` in proportion (largest remainder, ties to the
/// earlier part), so the result always sums to `total`.
pub fn apportion(total: i64, parts: &[i64]) -> Vec<i64> {
    let sum: i64 = parts.iter().map(|p| (*p).max(0)).sum();
    if sum <= 0 || total <= 0 {
        return vec![0; parts.len()];
    }
    let mut out: Vec<i64> = Vec::with_capacity(parts.len());
    let mut rem: Vec<(i128, usize)> = Vec::with_capacity(parts.len());
    for (i, p) in parts.iter().enumerate() {
        let q = total as i128 * (*p).max(0) as i128;
        out.push((q / sum as i128) as i64);
        rem.push((q % sum as i128, i));
    }
    let left = total - out.iter().sum::<i64>();
    rem.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in rem.into_iter().take(left.max(0) as usize) {
        out[i] += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Australia::Sydney;

    fn at(tz: Tz, y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        tz.with_ymd_and_hms(y, m, d, h, mi, 0)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn dst_start_day_is_23_hours() {
        // Sydney: clocks go forward 2:00 -> 3:00 on Sun 4 Oct 2026
        let w = window(Sydney, at(Sydney, 2026, 10, 6, 12, 0));
        assert_eq!(w.dates[0].to_string(), "2026-09-30");
        let i = w
            .dates
            .iter()
            .position(|d| d.to_string() == "2026-10-04")
            .unwrap();
        assert_eq!(w.bounds[i + 1] - w.bounds[i], 23 * 3600);
        assert_eq!(w.bounds[1] - w.bounds[0], 24 * 3600);
    }

    #[test]
    fn dst_end_day_is_25_hours() {
        // Sydney: clocks go back 3:00 -> 2:00 on Sun 5 Apr 2026
        let w = window(Sydney, at(Sydney, 2026, 4, 7, 9, 0));
        let i = w
            .dates
            .iter()
            .position(|d| d.to_string() == "2026-04-05")
            .unwrap();
        assert_eq!(w.bounds[i + 1] - w.bounds[i], 25 * 3600);
    }

    #[test]
    fn split_at_midnight_and_window_start() {
        let w = window(Sydney, at(Sydney, 2026, 10, 6, 12, 0));
        // 22:00 Tue 29 Sep -> 02:00 Wed 30 Sep: only the 2 h after the window start count
        let d = split(
            &w,
            at(Sydney, 2026, 9, 29, 22, 0),
            at(Sydney, 2026, 9, 30, 2, 0),
            4 * 3600,
        );
        assert_eq!(d[0], 2 * 3600);
        assert_eq!(d.iter().sum::<i64>(), 2 * 3600);
        // 23:30 Thu 1 Oct -> 00:45 Fri 2 Oct: 30 m + 45 m
        let d = split(
            &w,
            at(Sydney, 2026, 10, 1, 23, 30),
            at(Sydney, 2026, 10, 2, 0, 45),
            75 * 60,
        );
        assert_eq!((d[1], d[2]), (30 * 60, 45 * 60));
        // across the DST jump 01:30 -> 04:30 local is 2 real hours
        let d = split(
            &w,
            at(Sydney, 2026, 10, 4, 1, 30),
            at(Sydney, 2026, 10, 4, 4, 30),
            2 * 3600,
        );
        assert_eq!(d[4], 2 * 3600);
        // idle trimmed: 3 h wall clock, 1 h played, counted as the last hour
        let d = split(
            &w,
            at(Sydney, 2026, 10, 2, 22, 0),
            at(Sydney, 2026, 10, 3, 1, 0),
            3600,
        );
        assert_eq!((d[2], d[3]), (0, 3600));
        // after now: clipped
        let d = split(
            &w,
            at(Sydney, 2026, 10, 6, 11, 0),
            at(Sydney, 2026, 10, 6, 13, 0),
            7200,
        );
        assert_eq!(d[6], 3600);
    }

    #[test]
    fn apportion_sums() {
        assert_eq!(apportion(309, &[59400, 48600, 3240]), vec![165, 135, 9]);
        let a = apportion(10, &[1, 1, 1]);
        assert_eq!(a.iter().sum::<i64>(), 10);
        assert_eq!(apportion(0, &[5, 5]), vec![0, 0]);
        assert_eq!(tenths(1800 - 180), 5); // 27 min -> 0.5 h (rounded half up at 27 m)
        assert_eq!(tenths(179), 0);
        assert_eq!(tenths(180), 1);
    }
}
