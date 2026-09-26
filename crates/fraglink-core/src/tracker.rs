//! Session state machine: open/close sessions, idle trimming, crash-safe
//! checkpoints, merge of quick relaunches, milestones. Pure logic on top of
//! [`Store`]; time and inputs are passed in so tests can drive it.

use crate::detector::{DetectSource, Detection};
use crate::store::Store;
use anyhow::Result;
use chrono::{FixedOffset, Local, TimeZone};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

/// Maps a unix timestamp to a local calendar day `YYYY-MM-DD`.
pub type DayFn = Box<dyn Fn(i64) -> String + Send + Sync>;

pub fn local_day_fn() -> DayFn {
    Box::new(|ts| match Local.timestamp_opt(ts, 0).single() {
        Some(t) => t.format("%Y-%m-%d").to_string(),
        None => "1970-01-01".into(),
    })
}

pub fn fixed_offset_day_fn(offset_secs: i32) -> DayFn {
    let off = FixedOffset::east_opt(offset_secs).expect("valid offset");
    Box::new(move |ts| {
        off.timestamp_opt(ts, 0)
            .single()
            .map(|t| t.format("%Y-%m-%d").to_string())
            .unwrap_or_default()
    })
}

/// Hours at which a "hit N hours in X" activity entry is written.
pub const MILESTONE_HOURS: &[i64] = &[10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000];

#[derive(Debug, Clone, Serialize)]
pub struct TrackerConfig {
    /// No input for longer than this (seconds) = idle; the idle stretch is trimmed.
    pub idle_threshold_s: i64,
    /// Sessions shorter than this (active seconds) are dropped.
    pub min_session_s: i64,
    /// A relaunch within this many seconds continues the previous session.
    pub merge_gap_s: i64,
    pub checkpoint_every_s: i64,
    /// Consecutive polls without the game before the session ends.
    pub end_grace_polls: u32,
    /// Ticks further apart than this (sleep/hibernate) add no time.
    pub max_tick_gap_s: i64,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        TrackerConfig {
            idle_threshold_s: 15 * 60,
            min_session_s: 120,
            merge_gap_s: 600,
            checkpoint_every_s: 60,
            end_grace_polls: 2,
            max_tick_gap_s: 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TrackerEvent {
    Started {
        game_id: String,
        name: String,
        session_id: i64,
        resumed: bool,
        first_time: bool,
    },
    Stopped {
        game_id: String,
        name: String,
        session_id: i64,
        active_seconds: i64,
        kept: bool,
    },
    Milestone {
        game_id: String,
        name: String,
        hours: i64,
    },
    IdleStarted {
        game_id: String,
    },
    IdleEnded {
        game_id: String,
        trimmed_seconds: i64,
    },
    Recovered {
        game_id: String,
        name: String,
        session_id: i64,
        active_seconds: i64,
        kept: bool,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NowPlaying {
    pub game_id: String,
    pub name: String,
    pub session_id: i64,
    pub started_at: i64,
    pub active_seconds: i64,
    pub idle_trimmed_seconds: i64,
    pub idle: bool,
    pub source: DetectSource,
}

#[derive(Debug)]
struct Open {
    id: i64,
    game_id: String,
    name: String,
    started_at: i64,
    last_seen: i64,
    active: i64,
    trimmed: i64,
    /// Seconds counted as active since the last input (trimmed if idle crosses the threshold).
    pending_idle: i64,
    in_idle: bool,
    days: BTreeMap<String, i64>,
    base_total: i64,
    source: DetectSource,
}

pub struct Tracker {
    pub store: Store,
    pub cfg: TrackerConfig,
    day: DayFn,
    cur: Option<Open>,
    missing: u32,
    last_tick: Option<i64>,
    last_ckpt: i64,
    paused: bool,
    client_version: String,
}

impl Tracker {
    pub fn new(store: Store, cfg: TrackerConfig, day: DayFn) -> Self {
        Tracker {
            store,
            cfg,
            day,
            cur: None,
            missing: 0,
            last_tick: None,
            last_ckpt: 0,
            paused: false,
            client_version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    pub fn day_of(&self, ts: i64) -> String {
        (self.day)(ts)
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn now_playing(&self) -> Option<NowPlaying> {
        self.cur.as_ref().map(|c| NowPlaying {
            game_id: c.game_id.clone(),
            name: c.name.clone(),
            session_id: c.id,
            started_at: c.started_at,
            active_seconds: c.active,
            idle_trimmed_seconds: c.trimmed,
            idle: c.in_idle,
            source: c.source,
        })
    }

    /// Close sessions left open by a crash/power loss, using their last checkpoint.
    pub fn recover(&mut self) -> Result<Vec<TrackerEvent>> {
        let mut ev = Vec::new();
        for s in self.store.open_sessions()? {
            if s.active_seconds < self.cfg.min_session_s {
                self.store.delete_session(s.id)?;
                ev.push(TrackerEvent::Recovered {
                    game_id: s.game_id,
                    name: s.game_name,
                    session_id: s.id,
                    active_seconds: s.active_seconds,
                    kept: false,
                });
                continue;
            }
            let days = self.store.session_days(s.id)?;
            self.store.close_session(
                s.id,
                &s.game_id,
                s.last_checkpoint_at,
                s.active_seconds,
                s.idle_trimmed_seconds,
                &days,
            )?;
            self.store.delete_activity(s.id, "stopped")?;
            self.store.add_activity(
                s.last_checkpoint_at,
                "stopped",
                Some(&s.game_id),
                Some(s.id),
                &json!({ "active_seconds": s.active_seconds, "recovered": true }),
            )?;
            log::info!(
                "recovered open session {} ({}) with {}s",
                s.id,
                s.game_id,
                s.active_seconds
            );
            ev.push(TrackerEvent::Recovered {
                game_id: s.game_id,
                name: s.game_name,
                session_id: s.id,
                active_seconds: s.active_seconds,
                kept: true,
            });
        }
        Ok(ev)
    }

    /// One poll. `dets` = games currently running (first = preferred), `idle_s` = seconds since last input.
    pub fn tick(&mut self, now: i64, dets: &[Detection], idle_s: u64) -> Result<Vec<TrackerEvent>> {
        let mut ev = Vec::new();
        let dt = match self.last_tick {
            Some(t) if now >= t && now - t <= self.cfg.max_tick_gap_s => now - t,
            _ => 0,
        };
        self.last_tick = Some(now);

        if self.paused {
            if self.cur.is_some() {
                ev.extend(self.close_current()?);
            }
            return Ok(ev);
        }

        if let Some(game_id) = self.cur.as_ref().map(|c| c.game_id.clone()) {
            if dets.iter().any(|d| d.game_id == game_id) {
                self.missing = 0;
                ev.extend(self.account(now, dt, idle_s as i64)?);
            } else {
                self.missing += 1;
                if self.missing >= self.cfg.end_grace_polls {
                    ev.extend(self.close_current()?);
                }
            }
        }

        if self.cur.is_none() {
            if let Some(d) = dets.first() {
                ev.extend(self.open(now, d)?);
            }
        }

        if self.cur.is_some() && now - self.last_ckpt >= self.cfg.checkpoint_every_s {
            self.checkpoint(now)?;
        }
        Ok(ev)
    }

    fn account(&mut self, now: i64, dt: i64, idle: i64) -> Result<Vec<TrackerEvent>> {
        let mut ev = Vec::new();
        let thr = self.cfg.idle_threshold_s;
        let today = self.day_of(now);
        let c = self.cur.as_mut().expect("open session");
        let before = c.base_total + c.active;
        c.last_seen = now;
        if idle > thr {
            if !c.in_idle {
                // Crossing the threshold: the whole idle stretch so far is trimmed.
                let mut left = c.pending_idle.min(c.active);
                c.active -= left;
                c.trimmed += left;
                if let Some(v) = c.days.get_mut(&today) {
                    let take = left.min(*v);
                    *v -= take;
                    left -= take;
                }
                for (_, v) in c.days.iter_mut().rev() {
                    if left == 0 {
                        break;
                    }
                    let take = left.min(*v);
                    *v -= take;
                    left -= take;
                }
                c.pending_idle = 0;
                c.in_idle = true;
                ev.push(TrackerEvent::IdleStarted {
                    game_id: c.game_id.clone(),
                });
            }
            c.trimmed += dt;
        } else {
            let counted = if c.in_idle {
                c.in_idle = false;
                let counted = dt.min(idle);
                ev.push(TrackerEvent::IdleEnded {
                    game_id: c.game_id.clone(),
                    trimmed_seconds: c.trimmed + (dt - counted),
                });
                counted
            } else {
                dt
            };
            c.trimmed += dt - counted;
            c.active += counted;
            // The counted seconds are (now - counted, now]. Split them at local
            // midnight so each day gets exactly its share.
            let mut on_today = counted;
            if counted > 0 && (self.day)(now - counted + 1) != today {
                let first_today = (now - counted + 1..=now)
                    .find(|&t| (self.day)(t) == today)
                    .unwrap_or(now);
                on_today = now - first_today + 1;
                let prev = (self.day)(now - counted + 1);
                *c.days.entry(prev).or_insert(0) += counted - on_today;
            }
            *c.days.entry(today).or_insert(0) += on_today;
            c.pending_idle = idle.min(c.pending_idle + counted);
        }
        let after = c.base_total + c.active;
        for &h in MILESTONE_HOURS {
            if before < h * 3600 && after >= h * 3600 {
                let (gid, name) = (c.game_id.clone(), c.name.clone());
                self.store.add_activity(
                    now,
                    "milestone",
                    Some(&gid),
                    Some(c.id),
                    &json!({ "hours": h }),
                )?;
                ev.push(TrackerEvent::Milestone {
                    game_id: gid,
                    name,
                    hours: h,
                });
            }
        }
        Ok(ev)
    }

    fn open(&mut self, now: i64, d: &Detection) -> Result<Vec<TrackerEvent>> {
        let src = match d.source {
            DetectSource::Manual => "manual",
            DetectSource::Signature => "signature",
            DetectSource::SteamLibrary | DetectSource::SteamRunning => "steam",
            DetectSource::EpicLibrary => "epic",
            DetectSource::GogLibrary => "gog",
        };
        self.store
            .upsert_game(&d.game_id, &d.name, src, d.steam_appid, now)?;
        let name = self
            .store
            .game_name(&d.game_id)?
            .unwrap_or_else(|| d.name.clone());
        let mut first_time = false;
        let resumed;
        let open = if let Some(prev) = self
            .store
            .recent_closed_session(&d.game_id, now - self.cfg.merge_gap_s)?
        {
            // Quick relaunch (crash, patch restart): continue the previous session.
            self.store.reopen_session(prev.id)?;
            self.store.delete_activity(prev.id, "stopped")?;
            resumed = true;
            Open {
                id: prev.id,
                game_id: d.game_id.clone(),
                name: name.clone(),
                started_at: prev.started_at,
                last_seen: now,
                active: prev.active_seconds,
                trimmed: prev.idle_trimmed_seconds,
                pending_idle: 0,
                in_idle: false,
                days: self.store.session_days(prev.id)?,
                base_total: self.store.game_total_seconds(&d.game_id, Some(prev.id))?,
                source: d.source,
            }
        } else {
            resumed = false;
            let uuid = uuid::Uuid::new_v4().to_string();
            let id = self.store.open_session(
                &uuid,
                &d.game_id,
                now,
                d.source.as_str(),
                d.exe.as_deref(),
                &self.client_version,
            )?;
            first_time = self.store.game_session_count(&d.game_id)? == 1;
            self.store
                .add_activity(now, "started", Some(&d.game_id), Some(id), &json!({}))?;
            if first_time {
                self.store.add_activity(
                    now,
                    "first_time",
                    Some(&d.game_id),
                    Some(id),
                    &json!({}),
                )?;
            }
            Open {
                id,
                game_id: d.game_id.clone(),
                name: name.clone(),
                started_at: now,
                last_seen: now,
                active: 0,
                trimmed: 0,
                pending_idle: 0,
                in_idle: false,
                days: BTreeMap::new(),
                base_total: self.store.game_total_seconds(&d.game_id, Some(id))?,
                source: d.source,
            }
        };
        let id = open.id;
        self.cur = Some(open);
        self.missing = 0;
        self.last_ckpt = now;
        log::info!(
            "session {} started: {} via {}{}",
            id,
            d.game_id,
            d.source.as_str(),
            if resumed { " (resumed)" } else { "" }
        );
        Ok(vec![TrackerEvent::Started {
            game_id: d.game_id.clone(),
            name,
            session_id: id,
            resumed,
            first_time,
        }])
    }

    fn close_current(&mut self) -> Result<Vec<TrackerEvent>> {
        let Some(c) = self.cur.take() else {
            return Ok(vec![]);
        };
        self.missing = 0;
        let kept = c.active >= self.cfg.min_session_s;
        if kept {
            self.store.close_session(
                c.id,
                &c.game_id,
                c.last_seen,
                c.active,
                c.trimmed,
                &c.days,
            )?;
            self.store.add_activity(
                c.last_seen,
                "stopped",
                Some(&c.game_id),
                Some(c.id),
                &json!({ "active_seconds": c.active, "idle_trimmed_seconds": c.trimmed }),
            )?;
        } else {
            self.store.delete_session(c.id)?;
        }
        log::info!(
            "session {} ended: {} active={}s trimmed={}s kept={}",
            c.id,
            c.game_id,
            c.active,
            c.trimmed,
            kept
        );
        Ok(vec![TrackerEvent::Stopped {
            game_id: c.game_id,
            name: c.name,
            session_id: c.id,
            active_seconds: c.active,
            kept,
        }])
    }

    /// Persist the open session now (also called every `checkpoint_every_s`).
    pub fn checkpoint(&mut self, now: i64) -> Result<()> {
        if let Some(c) = &self.cur {
            self.store
                .checkpoint_session(c.id, &c.game_id, now, c.active, c.trimmed, &c.days)?;
            self.last_ckpt = now;
        }
        Ok(())
    }

    /// End the current session (app quitting).
    pub fn stop(&mut self) -> Result<Vec<TrackerEvent>> {
        self.close_current()
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<Vec<TrackerEvent>> {
        self.paused = paused;
        if paused {
            self.close_current()
        } else {
            Ok(vec![])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_790_000_000; // 2026-09-22 (Tue) ~00:13 AEST

    fn det(id: &str) -> Detection {
        Detection {
            game_id: id.into(),
            name: id.to_uppercase(),
            source: DetectSource::Signature,
            exe: Some(format!("{id}.exe")),
            steam_appid: None,
        }
    }

    fn tracker() -> Tracker {
        Tracker::new(
            Store::open_in_memory().unwrap(),
            TrackerConfig::default(),
            fixed_offset_day_fn(10 * 3600),
        )
    }

    /// Run `secs` seconds of polling every 5 s with the given detections and idle function.
    fn run(
        t: &mut Tracker,
        start: i64,
        secs: i64,
        dets: &[Detection],
        idle: impl Fn(i64) -> u64,
    ) -> (i64, Vec<TrackerEvent>) {
        let mut ev = Vec::new();
        let mut now = start;
        while now < start + secs {
            now += 5;
            ev.extend(t.tick(now, dets, idle(now)).unwrap());
        }
        (now, ev)
    }

    #[test]
    fn basic_session_lifecycle() {
        let mut t = tracker();
        let g = [det("wow")];
        let (now, ev) = run(&mut t, T0, 600, &g, |_| 0);
        assert!(matches!(
            &ev[0],
            TrackerEvent::Started {
                first_time: true,
                resumed: false,
                ..
            }
        ));
        let np = t.now_playing().unwrap();
        assert_eq!(np.game_id, "wow");
        assert_eq!(np.active_seconds, 595); // first tick opens, then 119 * 5 s
                                            // game exits: ends after 2 missed polls, end time = last seen
        let (_, ev) = run(&mut t, now, 10, &[], |_| 0);
        assert!(matches!(
            &ev[0],
            TrackerEvent::Stopped {
                kept: true,
                active_seconds: 595,
                ..
            }
        ));
        let s = &t.store.all_sessions().unwrap()[0];
        assert_eq!(s.status, "closed");
        assert_eq!(s.ended_at, Some(now));
        assert_eq!(s.active_seconds, 595);
        let kinds: Vec<_> = t
            .store
            .all_activity()
            .unwrap()
            .into_iter()
            .map(|a| a.kind)
            .collect();
        assert_eq!(kinds, vec!["started", "first_time", "stopped"]);
    }

    #[test]
    fn short_sessions_are_dropped() {
        let mut t = tracker();
        let (now, _) = run(&mut t, T0, 60, &[det("cs2")], |_| 0);
        let (_, ev) = run(&mut t, now, 10, &[], |_| 0);
        assert!(matches!(&ev[0], TrackerEvent::Stopped { kept: false, .. }));
        assert!(t.store.all_sessions().unwrap().is_empty());
        assert!(
            t.store.all_activity().unwrap().is_empty(),
            "activity of dropped session removed"
        );
    }

    #[test]
    fn idle_stretch_beyond_threshold_is_trimmed() {
        let mut t = tracker();
        t.cfg.idle_threshold_s = 300;
        let g = [det("wow")];
        // 10 min active, then 20 min without input, then 5 min active again
        let (now, _) = run(&mut t, T0, 600, &g, |_| 1);
        let idle_start = now;
        let (now, ev) = run(&mut t, now, 1200, &g, |n| (n - idle_start) as u64);
        assert!(ev
            .iter()
            .any(|e| matches!(e, TrackerEvent::IdleStarted { .. })));
        assert!(t.now_playing().unwrap().idle);
        let (now, ev) = run(&mut t, now, 300, &g, |_| 2);
        assert!(ev
            .iter()
            .any(|e| matches!(e, TrackerEvent::IdleEnded { .. })));
        let np = t.now_playing().unwrap();
        // 595 s before idle + 300 s after (minus the 5 s straddling tick counted as min(dt, idle)=2)
        assert_eq!(np.active_seconds, 595 + 2 + 295);
        assert_eq!(np.idle_trimmed_seconds, 1200 + 3);
        let (_, _) = run(&mut t, now, 10, &[], |_| 0);
        let s = &t.store.all_sessions().unwrap()[0];
        assert_eq!(s.active_seconds, 892);
        assert_eq!(s.idle_trimmed_seconds, 1203);
        let days: i64 = t.store.session_days(s.id).unwrap().values().sum();
        assert_eq!(days, 892, "per-day rows match the trimmed total");
    }

    #[test]
    fn idle_below_threshold_still_counts_and_minimized_counts() {
        let mut t = tracker();
        let g = [det("wow")];
        // 10 minutes without input (under the 15 min default) is still play time
        let (_, _) = run(&mut t, T0, 600, &g, |n| (n - T0) as u64);
        assert_eq!(t.now_playing().unwrap().active_seconds, 595);
        assert_eq!(t.now_playing().unwrap().idle_trimmed_seconds, 0);
    }

    #[test]
    fn sleep_gap_adds_no_time() {
        let mut t = tracker();
        let g = [det("wow")];
        let (now, _) = run(&mut t, T0, 300, &g, |_| 0);
        let before = t.now_playing().unwrap().active_seconds;
        t.tick(now + 3600, &g, 0).unwrap(); // machine slept an hour
        assert_eq!(t.now_playing().unwrap().active_seconds, before);
    }

    #[test]
    fn quick_relaunch_merges_into_one_session() {
        let mut t = tracker();
        let g = [det("rust")];
        let (now, _) = run(&mut t, T0, 600, &g, |_| 0);
        let (now, _) = run(&mut t, now, 120, &[], |_| 0); // crashed, 2 minutes
        let (now, ev) = run(&mut t, now, 600, &g, |_| 0);
        assert!(matches!(
            &ev[0],
            TrackerEvent::Started {
                resumed: true,
                first_time: false,
                ..
            }
        ));
        run(&mut t, now, 10, &[], |_| 0);
        let all = t.store.all_sessions().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].active_seconds, 595 + 595);
        let stops = t
            .store
            .all_activity()
            .unwrap()
            .into_iter()
            .filter(|a| a.kind == "stopped")
            .count();
        assert_eq!(stops, 1);
    }

    #[test]
    fn no_merge_when_another_game_was_played_in_between() {
        let mut t = tracker();
        let (now, _) = run(&mut t, T0, 300, &[det("wow")], |_| 0);
        let (now, _) = run(&mut t, now, 10, &[], |_| 0);
        let (now, _) = run(&mut t, now, 300, &[det("cs2")], |_| 0);
        let (now, _) = run(&mut t, now, 10, &[], |_| 0);
        let (_, ev) = run(&mut t, now, 300, &[det("wow")], |_| 0);
        assert!(matches!(
            &ev[0],
            TrackerEvent::Started { resumed: false, .. }
        ));
        assert_eq!(t.store.all_sessions().unwrap().len(), 3);
    }

    #[test]
    fn checkpoints_and_recovery_after_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let db = tmp.path().join("fraglink.db");
        let g = [det("valheim")];
        {
            let mut t = Tracker::new(
                Store::open(&db).unwrap(),
                TrackerConfig::default(),
                fixed_offset_day_fn(36000),
            );
            run(&mut t, T0, 185, &g, |_| 0);
            // process "crashes" here: no stop(), no close
        }
        let s = Store::open(&db).unwrap();
        let open = s.open_sessions().unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(
            open[0].active_seconds, 180,
            "checkpoints every 60 s: last one at +185 s"
        );
        let mut t = Tracker::new(s, TrackerConfig::default(), fixed_offset_day_fn(36000));
        let ev = t.recover().unwrap();
        assert!(matches!(
            &ev[0],
            TrackerEvent::Recovered {
                kept: true,
                active_seconds: 180,
                ..
            }
        ));
        let row = &t.store.all_sessions().unwrap()[0];
        assert_eq!(row.status, "closed");
        assert_eq!(row.ended_at, Some(T0 + 185));
        assert!(t.store.open_sessions().unwrap().is_empty());
        // recovering twice is a no-op
        assert!(t.recover().unwrap().is_empty());
    }

    #[test]
    fn recovery_drops_too_short_open_sessions() {
        let s = Store::open_in_memory().unwrap();
        s.upsert_game("x", "X", "signature", None, T0).unwrap();
        s.open_session("u1", "x", T0, "signature", None, "t")
            .unwrap();
        let mut t = Tracker::new(s, TrackerConfig::default(), fixed_offset_day_fn(0));
        let ev = t.recover().unwrap();
        assert!(matches!(
            &ev[0],
            TrackerEvent::Recovered { kept: false, .. }
        ));
        assert!(t.store.all_sessions().unwrap().is_empty());
    }

    #[test]
    fn milestones_fire_once() {
        let mut t = tracker();
        // pretend 9h59m50s already logged
        t.store
            .upsert_game("wow", "World of Warcraft", "signature", None, T0)
            .unwrap();
        let id = t
            .store
            .open_session("old", "wow", T0 - 90_000, "signature", None, "t")
            .unwrap();
        let mut days = BTreeMap::new();
        days.insert("2026-09-20".to_string(), 10 * 3600 - 10);
        t.store
            .close_session(id, "wow", T0 - 50_000, 10 * 3600 - 10, 0, &days)
            .unwrap();
        let (_, ev) = run(&mut t, T0, 60, &[det("wow")], |_| 0);
        let ms: Vec<_> = ev
            .iter()
            .filter_map(|e| {
                if let TrackerEvent::Milestone { hours, .. } = e {
                    Some(*hours)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(ms, vec![10]);
        assert_eq!(
            t.store
                .all_activity()
                .unwrap()
                .iter()
                .filter(|a| a.kind == "milestone")
                .count(),
            1
        );
    }

    #[test]
    fn pause_closes_session_and_blocks_new_ones() {
        let mut t = tracker();
        let g = [det("wow")];
        let (now, _) = run(&mut t, T0, 300, &g, |_| 0);
        let ev = t.set_paused(true).unwrap();
        assert!(matches!(&ev[0], TrackerEvent::Stopped { kept: true, .. }));
        let (now, ev) = run(&mut t, now, 300, &g, |_| 0);
        assert!(ev.is_empty() && t.now_playing().is_none());
        t.set_paused(false).unwrap();
        let (_, ev) = run(&mut t, now, 10, &g, |_| 0);
        assert!(matches!(&ev[0], TrackerEvent::Started { .. }));
    }

    #[test]
    fn keeps_current_game_when_second_starts() {
        let mut t = tracker();
        let (now, _) = run(&mut t, T0, 30, &[det("wow")], |_| 0);
        let (_, ev) = run(&mut t, now, 30, &[det("cs2"), det("wow")], |_| 0);
        assert!(ev.is_empty());
        assert_eq!(t.now_playing().unwrap().game_id, "wow");
    }

    #[test]
    fn session_across_midnight_splits_days() {
        let mut t = tracker();
        // 2026-09-22 23:50 AEST = 13:50 UTC
        let start = chrono::DateTime::parse_from_rfc3339("2026-09-22T23:50:00+10:00")
            .unwrap()
            .timestamp();
        run(&mut t, start, 1200, &[det("wow")], |_| 0);
        t.checkpoint(start + 1200).unwrap();
        let days = t
            .store
            .session_days(t.now_playing().unwrap().session_id)
            .unwrap();
        // Split at the exact second: the tick at 00:00:00 covers (23:59:55, 00:00:00],
        // so 4 of its 5 seconds belong to the 22nd and 1 to the 23rd.
        assert_eq!(days.get("2026-09-22"), Some(&594));
        assert_eq!(days.get("2026-09-23"), Some(&601));
    }
}
