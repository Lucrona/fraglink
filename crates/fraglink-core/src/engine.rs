//! Glue used by the desktop app and the headless `fraglink-sim` binary:
//! environment + detector + tracker + settings.

use crate::detector::{Candidate, Detector};
use crate::env::{Environment, ProcInfo};
use crate::signatures::SignatureDb;
use crate::stats::{self, Glance};
use crate::steam::{self, SteamIndex};
use crate::store::Store;
use crate::tracker::{DayFn, Tracker, TrackerConfig, TrackerEvent};
use anyhow::{bail, Result};
use serde_json::json;

pub const DEFAULT_POLL_SECS: u64 = 5;
const STEAM_REFRESH_SECS: i64 = 600;

/// Settings keys stored in the `settings` table.
pub mod keys {
    pub const IDLE_THRESHOLD_MIN: &str = "idle_threshold_min";
    pub const MIN_SESSION_MIN: &str = "min_session_min";
    pub const PAUSED: &str = "paused";
    pub const TOASTS: &str = "toasts";
    pub const SKIN: &str = "skin";
    pub const ZOOM: &str = "zoom";
    pub const STATUS: &str = "status";
}

pub struct Engine {
    pub tracker: Tracker,
    pub detector: Detector,
    env: Box<dyn Environment>,
    last_procs: Vec<ProcInfo>,
    steam_loaded_at: Option<i64>,
    /// Exe path and Steam app id of games detected since start (for icons).
    pub seen: std::collections::HashMap<String, SeenGame>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeenGame {
    pub exe_path: Option<String>,
    pub steam_appid: Option<u32>,
}

pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    s.trim_end_matches('-').chars().take(48).collect()
}

impl Engine {
    pub fn new(store: Store, env: Box<dyn Environment>, day: DayFn) -> Result<Self> {
        let detector = Detector::new(SignatureDb::builtin(), SteamIndex::default());
        let mut e = Engine {
            tracker: Tracker::new(store, TrackerConfig::default(), day),
            detector,
            env,
            last_procs: vec![],
            steam_loaded_at: None,
            seen: Default::default(),
        };
        e.apply_settings()?;
        e.reload_rules()?;
        Ok(e)
    }

    pub fn env_label(&self) -> &'static str {
        self.env.label()
    }

    fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.tracker
            .store
            .get_setting(key)
            .ok()
            .flatten()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    }

    /// Re-read tracker-related settings from the store.
    pub fn apply_settings(&mut self) -> Result<()> {
        let idle = self.setting_i64(keys::IDLE_THRESHOLD_MIN, 15).clamp(1, 240);
        let min = self.setting_i64(keys::MIN_SESSION_MIN, 2).clamp(0, 60);
        self.tracker.cfg.idle_threshold_s = idle * 60;
        self.tracker.cfg.min_session_s = min * 60;
        let paused = self.tracker.store.get_setting(keys::PAUSED)?.as_deref() == Some("1");
        if paused != self.tracker.is_paused() {
            self.tracker.set_paused(paused)?;
        }
        Ok(())
    }

    /// Reload manual tags and exclusions into the detector.
    pub fn reload_rules(&mut self) -> Result<()> {
        self.detector.tags = self.tracker.store.manual_tags()?;
        self.detector.excluded_games = self.tracker.store.excluded_game_ids()?;
        self.detector.excluded_exes = self.tracker.store.excluded_exes()?;
        Ok(())
    }

    pub fn refresh_steam(&mut self, now: i64) {
        self.steam_loaded_at = Some(now);
        match steam::steam_root() {
            Some(root) => {
                self.detector.steam = SteamIndex::load(&root);
                log::info!(
                    "steam: {} apps in {} libraries ({})",
                    self.detector.steam.len(),
                    self.detector.steam.libraries.len(),
                    root.display()
                );
            }
            None => log::info!("steam: not installed / not found"),
        }
        self.detector.libs = crate::libraries::LibraryIndex::load();
        log::info!(
            "libraries: {} Epic, {} GOG installs",
            self.detector.libs.count(crate::libraries::Launcher::Epic),
            self.detector.libs.count(crate::libraries::Launcher::Gog)
        );
    }

    /// One poll of the environment. Call every [`DEFAULT_POLL_SECS`].
    pub fn step(&mut self, now: i64) -> Result<Vec<TrackerEvent>> {
        if self
            .steam_loaded_at
            .is_none_or(|t| now - t >= STEAM_REFRESH_SECS)
        {
            self.refresh_steam(now);
        }
        let procs = self.env.processes();
        let idle = self.env.idle_seconds();
        let appid = self.env.steam_running_appid();
        let dets = self.detector.detect(&procs, appid);
        for d in &dets {
            let path = d.exe.as_ref().and_then(|x| {
                procs
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(x))
                    .and_then(|p| p.path.clone())
            });
            let e = self.seen.entry(d.game_id.clone()).or_default();
            if path.is_some() && e.exe_path != path {
                // remembered for Launch and the Options > Games paths
                if let Some(p) = &path {
                    let _ = self
                        .tracker
                        .store
                        .set_setting(&format!("{}{}", crate::catalog::EXE_KEY, d.game_id), p);
                }
                e.exe_path = path;
            }
            if d.steam_appid.is_some() {
                e.steam_appid = d.steam_appid;
            }
        }
        self.last_procs = procs;
        self.tracker.tick(now, &dets, idle)
    }

    pub fn candidates(&self) -> Vec<Candidate> {
        self.detector.candidates(&self.last_procs)
    }

    /// "This is a game": tag a running exe. Returns the new game id.
    pub fn tag_exe(&mut self, exe: &str, name: &str, now: i64) -> Result<String> {
        let name = name.trim();
        if exe.trim().is_empty() || name.is_empty() || name.chars().count() > 80 {
            bail!("need an exe name and a game name (max 80 characters)");
        }
        let s = slug(name);
        let id = format!("manual:{}", if s.is_empty() { slug(exe) } else { s });
        self.tracker.store.add_manual_tag(exe, &id, name, now)?;
        self.tracker.store.add_activity(
            now,
            "tagged",
            Some(&id),
            None,
            &json!({ "exe_name": exe }),
        )?;
        self.reload_rules()?;
        Ok(id)
    }

    pub fn untag_exe(&mut self, exe: &str) -> Result<()> {
        self.tracker.store.remove_manual_tag(exe)?;
        self.reload_rules()
    }

    /// Exclude (or re-include) a game: excluded games are not tracked and hidden from stats.
    pub fn set_game_excluded(
        &mut self,
        game_id: &str,
        excluded: bool,
        now: i64,
    ) -> Result<Vec<TrackerEvent>> {
        let name = self
            .tracker
            .store
            .game_name(game_id)?
            .unwrap_or_else(|| game_id.to_string());
        if !self.tracker.store.set_game_excluded(game_id, excluded)? {
            // not in the games table yet (e.g. from the signature list): add it first
            let sig_name = self
                .detector
                .sigs
                .get(game_id)
                .map(|g| g.name.clone())
                .unwrap_or(name);
            self.tracker
                .store
                .upsert_game(game_id, &sig_name, "signature", None, now)?;
            self.tracker.store.set_game_excluded(game_id, excluded)?;
        }
        if excluded {
            self.tracker
                .store
                .add_activity(now, "excluded", Some(game_id), None, &json!({}))?;
        }
        self.reload_rules()?;
        let mut ev = vec![];
        if excluded
            && self
                .tracker
                .now_playing()
                .is_some_and(|n| n.game_id == game_id)
        {
            ev = self.tracker.stop()?;
        }
        Ok(ev)
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<Vec<TrackerEvent>> {
        self.tracker
            .store
            .set_setting(keys::PAUSED, if paused { "1" } else { "0" })?;
        self.tracker.set_paused(paused)
    }

    /// Stats for the tray (checkpoints the live session first so it is included).
    pub fn glance(&mut self, now: i64) -> Result<Glance> {
        self.tracker.checkpoint(now)?;
        let today = self.tracker.day_of(now);
        stats::glance(
            &self.tracker.store,
            &today,
            self.tracker.now_playing(),
            self.tracker.is_paused(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimEnv;
    use crate::tracker::fixed_offset_day_fn;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[test]
    fn slugs() {
        assert_eq!(slug("My Cool Game: Deluxe!"), "my-cool-game-deluxe");
        assert_eq!(slug("  ---  "), "");
    }

    /// End-to-end on the simulated environment with virtual time.
    #[test]
    fn simulated_run_end_to_end() {
        std::env::set_var("FRAGLINK_STEAM_DIR", "/nonexistent-steam-dir");
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        let env = SimEnv::new(
            "-:10,cs2.exe:300,-:30,steam:570:300,-:30,C:/Games/MyIndie/MyIndie.exe:300,-:30,Wow.exe:1500:idle,-:30",
            Arc::new(move || c.load(Ordering::SeqCst)),
        )
        .unwrap();
        let mut e = Engine::new(
            Store::open_in_memory().unwrap(),
            Box::new(env),
            fixed_offset_day_fn(36000),
        )
        .unwrap();
        let t0 = 1_790_020_000;
        let mut events = vec![];
        let mut tagged = false;
        for t in (0..=2530u64).step_by(5) {
            clock.store(t, Ordering::SeqCst);
            events.extend(e.step(t0 + t as i64).unwrap());
            if !tagged && t == 700 {
                let cands = e.candidates();
                assert_eq!(
                    cands.iter().map(|c| c.exe.as_str()).collect::<Vec<_>>(),
                    vec!["MyIndie.exe"]
                );
                e.tag_exe("MyIndie.exe", "My Indie", t0 + t as i64).unwrap();
                tagged = true;
            }
        }
        let started: Vec<_> = events
            .iter()
            .filter_map(|ev| {
                if let TrackerEvent::Started { game_id, .. } = ev {
                    Some(game_id.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(started, vec!["cs2", "dota2", "manual:my-indie", "wow"]);
        let g = e.glance(t0 + 2530).unwrap();
        // WoW ran 25 min with no input at all: the whole idle stretch is trimmed,
        // leaving 0 s, so the session is dropped.
        assert!(g.top.iter().all(|t| t.game_id != "wow"));
        assert!(events.iter().any(|ev| matches!(ev, TrackerEvent::Stopped { game_id, kept: false, .. } if game_id == "wow")));
        assert!(g
            .top
            .iter()
            .any(|t| t.game_id == "cs2" && t.total_seconds >= 290));

        // exe paths are remembered for icon extraction
        let indie = &e.seen["manual:my-indie"];
        assert_eq!(
            indie.exe_path.as_deref(),
            Some("C:/Games/MyIndie/MyIndie.exe")
        );
        assert!(e.seen["cs2"]
            .exe_path
            .as_deref()
            .is_some_and(|p| p.ends_with("cs2.exe")));

        // exclusion: stop tracking dota2 and hide it
        e.set_game_excluded("dota2", true, t0 + 2600).unwrap();
        let g = e.glance(t0 + 2600).unwrap();
        assert!(g.top.iter().all(|t| t.game_id != "dota2"));
    }
}
