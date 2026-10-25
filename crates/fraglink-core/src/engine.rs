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
/// An unknown program full screen in the foreground this long gets "Track it as a game?".
pub const PROMPT_AFTER_SECS: i64 = 5 * 60;
/// Settings key: JSON array of lower-case exe names answered "Not a game".
pub const PROMPT_DISMISSED_KEY: &str = "prompt_dismissed";
/// beta.6: programs the app asked about ("Playing X?") that weren't answered yet, kept for
/// Options > Games > Detected, not added. JSON list of [`Prompted`], newest first.
pub const PROMPTED_KEY: &str = "prompted_unanswered";
/// Most entries kept in [`PROMPTED_KEY`].
pub const PROMPTED_MAX: usize = 200;

/// An unknown program the app asked about.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Prompted {
    pub exe: String,
    pub path: String,
    /// Unix seconds of the last ask.
    pub at: i64,
}

/// Full-screen programs that are never offered as games (on top of the
/// signature file's `non_game_exes`): browsers, the shell, video players,
/// streaming / chat / recording apps, presentations.
const NOT_GAMES: &[&str] = &[
    "explorer.exe",
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "opera_gx.exe",
    "vivaldi.exe",
    "iexplore.exe",
    "arc.exe",
    "zen.exe",
    "floorp.exe",
    "librewolf.exe",
    "waterfox.exe",
    "thorium.exe",
    "yandex.exe",
    "browser.exe",
    "vlc.exe",
    "mpc-hc.exe",
    "mpc-hc64.exe",
    "mpc-be.exe",
    "mpc-be64.exe",
    "potplayer.exe",
    "potplayer64.exe",
    "potplayermini.exe",
    "potplayermini64.exe",
    "mpv.exe",
    "wmplayer.exe",
    "video.ui.exe",
    "microsoft.media.player.exe",
    "kmplayer.exe",
    "kmplayer64x.exe",
    "plex.exe",
    "plex htpc.exe",
    "kodi.exe",
    "stremio.exe",
    "jellyfin media player.exe",
    "netflix.exe",
    "wwahost.exe",
    "applicationframehost.exe",
    "photos.exe",
    "microsoft.photos.exe",
    "discord.exe",
    "discordptb.exe",
    "discordcanary.exe",
    "obs64.exe",
    "obs32.exe",
    "streamlabs obs.exe",
    "streamlabs desktop.exe",
    "xsplit.core.exe",
    "twitch.exe",
    "spotify.exe",
    "zoom.exe",
    "teams.exe",
    "ms-teams.exe",
    "slack.exe",
    "powerpnt.exe",
    "acrord32.exe",
    "acrobat.exe",
    "steam.exe",
    "steamwebhelper.exe",
    "battle.net.exe",
    "epicgameslauncher.exe",
    "galaxyclient.exe",
    "eadesktop.exe",
    "upc.exe",
    "riotclientux.exe",
    "rsi launcher.exe",
    "fraglink.exe",
    "fraglink-desktop.exe",
    "lockapp.exe",
    "logonui.exe",
    "screenclippinghost.exe",
    "snippingtool.exe",
    "taskmgr.exe",
    "mstsc.exe",
    "vmware.exe",
    "vmware-vmx.exe",
    "virtualbox.exe",
    "virtualboxvm.exe",
    "parsecd.exe",
    "moonlight.exe",
    "nvidia overlay.exe",
];

/// Watches for an unknown full-screen program (see [`PROMPT_AFTER_SECS`]).
#[derive(Debug, Default)]
struct PromptWatch {
    /// lower-case exe and when it became the full-screen foreground window
    current: Option<(String, i64)>,
    /// asked already this run
    asked: std::collections::HashSet<String>,
    pending: Option<Candidate>,
}

/// Settings keys stored in the `settings` table.
pub mod keys {
    pub const IDLE_THRESHOLD_MIN: &str = "idle_threshold_min";
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
    /// "pid|exe path" -> (game id, name) of each running game process, so one
    /// process keeps its first id for its lifetime (a Steam rescan or a list update
    /// can't switch it to another id mid-session).
    pinned: std::collections::HashMap<String, (String, String)>,
    prompt: PromptWatch,
    /// Seconds before the "Track it as a game?" prompt (tests and simulation shorten it).
    pub prompt_after_secs: i64,
    dismissed: std::collections::HashSet<String>,
    /// WoW realms of the running WoW flavours, by game id (see [`crate::realm`]).
    realms: std::collections::HashMap<String, crate::realm::RealmWatch>,
    details_changed: bool,
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
            pinned: Default::default(),
            prompt: PromptWatch::default(),
            prompt_after_secs: PROMPT_AFTER_SECS,
            dismissed: Default::default(),
            realms: Default::default(),
            details_changed: false,
        };
        e.apply_settings()?;
        e.reload_rules()?;
        Ok(e)
    }

    /// Seconds since the last keyboard/mouse input (Windows `GetLastInputInfo`; the
    /// simulation scripts it). Only used for the automatic Away status, never for hours.
    pub fn idle_seconds(&mut self) -> u64 {
        self.env.idle_seconds()
    }

    pub fn env_label(&self) -> &'static str {
        self.env.label()
    }

    /// Re-read tracker-related settings from the store.
    pub fn apply_settings(&mut self) -> Result<()> {
        // Like Steam, every session counts: since 0.2.0-dev.9 there's no idle cutoff
        // and since dev.10 no minimum length and no "pause tracking". The old
        // idle_threshold_min, min_session_min and paused settings are ignored.
        // (The tracker's pause is now only "offline", see `set_live`.)
        self.tracker.cfg.min_session_s = 0;
        Ok(())
    }

    /// Since 0.2.0-dev.14 hours only count while the app is connected live to the
    /// server (which records the time itself). Offline, nothing is tracked locally
    /// either, so the PC never shows hours the server didn't see.
    pub fn set_live(&mut self, live: bool) -> Result<Vec<TrackerEvent>> {
        if self.tracker.is_paused() == !live {
            return Ok(vec![]);
        }
        self.tracker.set_paused(!live)
    }

    /// False while offline (not connected to the server): nothing is tracked.
    pub fn is_live(&self) -> bool {
        !self.tracker.is_paused()
    }

    /// Reload manual tags and exclusions into the detector.
    pub fn reload_rules(&mut self) -> Result<()> {
        self.detector.tags = self.tracker.store.manual_tags()?;
        self.detector.excluded_games = self.tracker.store.excluded_game_ids()?;
        self.detector.excluded_exes = self.tracker.store.excluded_exes()?;
        self.dismissed = self
            .tracker
            .store
            .get_setting(PROMPT_DISMISSED_KEY)?
            .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.to_lowercase())
            .collect();
        Ok(())
    }

    /// Swap in a newer detectable list (downloaded by the app).
    pub fn set_detectable(&mut self, db: crate::detectable::DetectableDb) {
        self.detector.detectable = std::sync::Arc::new(db);
    }

    /// Swap in newer override rules (downloaded by the app).
    /// Rule names win over the built-in lists (staff renames, dev.18), so games already in the
    /// local database take the rule's name too (Activity, Stats, Options > Games).
    pub fn set_overrides(&mut self, o: crate::overrides::Overrides) {
        let mut seen = std::collections::HashSet::new();
        for r in o.rules() {
            if seen.insert(r.id.as_str()) {
                let _ = self.tracker.store.rename_game(&r.id, &r.name);
            }
        }
        self.detector.overrides = o;
    }

    /// Display name from the override rules (staff renames), if any.
    pub fn override_name(&self, id: &str) -> Option<&str> {
        self.detector
            .overrides
            .rules()
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.as_str())
    }

    /// Detail shown after a running game's name (the WoW realm), if known.
    pub fn game_detail(&self, game_id: &str) -> Option<String> {
        self.realms.get(game_id).and_then(|w| w.realm.clone())
    }

    /// The current game with its detail filled in.
    pub fn now_playing(&self) -> Option<crate::tracker::NowPlaying> {
        self.tracker.now_playing().map(|mut n| {
            n.detail = self.game_detail(&n.game_id);
            n
        })
    }

    /// True once after a game detail (realm) appeared, changed or went away.
    pub fn take_details_changed(&mut self) -> bool {
        std::mem::take(&mut self.details_changed)
    }

    /// Start / re-check / drop the Config.wtf watch of each running WoW flavour.
    fn update_details(&mut self, running: &[(String, Option<String>)]) {
        if !crate::realm::ENABLED {
            return;
        }
        let mut changed = false;
        self.realms.retain(|id, w| {
            let keep = running.iter().any(|(g, _)| g == id);
            if !keep && w.realm.is_some() {
                changed = true;
            }
            keep
        });
        for (id, path) in running {
            if !crate::realm::is_wow(id) {
                continue;
            }
            let Some(cfg) = path.as_deref().and_then(crate::realm::config_path) else {
                continue;
            };
            let w = self
                .realms
                .entry(id.clone())
                .or_insert_with(|| crate::realm::RealmWatch::new(cfg.clone()));
            if w.path != cfg {
                let had = w.realm.is_some();
                *w = crate::realm::RealmWatch::new(cfg);
                changed |= had;
            }
            changed |= w.poll();
        }
        self.details_changed |= changed;
    }

    /// The "Track <exe> as a game?" question, once it is due (taken = cleared).
    pub fn take_prompt(&mut self) -> Option<Candidate> {
        self.prompt.pending.take()
    }

    /// "Not a game": never ask about this exe again.
    pub fn dismiss_prompt(&mut self, exe: &str) -> Result<()> {
        let mut list: Vec<String> = self.dismissed.iter().cloned().collect();
        let e = exe.trim().to_lowercase();
        if e.is_empty() {
            return Ok(());
        }
        if !list.contains(&e) {
            list.push(e);
        }
        list.sort();
        self.tracker
            .store
            .set_setting(PROMPT_DISMISSED_KEY, &serde_json::to_string(&list)?)?;
        self.reload_rules()
    }

    /// One poll of the full-screen watch: the foreground program must be full
    /// screen, unknown, not excluded and not dismissed for `prompt_after_secs`
    /// in a row, with no recognised game running.
    fn watch_fullscreen(&mut self, now: i64, procs: &[ProcInfo], games_running: bool) {
        let fg = if games_running {
            None
        } else {
            self.env.foreground().filter(|f| f.fullscreen)
        };
        let proc_ = fg.and_then(|f| procs.iter().find(|p| p.pid == f.pid));
        let cand = proc_.and_then(|p| {
            let exe_l = p.name.to_lowercase();
            if NOT_GAMES.contains(&exe_l.as_str())
                || self.dismissed.contains(&exe_l)
                || self.prompt.asked.contains(&exe_l)
                || self.detector.excluded_exes.contains(&exe_l)
            {
                return None;
            }
            self.detector
                .candidates(std::slice::from_ref(p))
                .into_iter()
                .next()
        });
        let Some(c) = cand else {
            self.prompt.current = None;
            return;
        };
        let exe_l = c.exe.to_lowercase();
        match &self.prompt.current {
            Some((e, since)) if *e == exe_l => {
                if now - since >= self.prompt_after_secs {
                    log::info!(
                        "full-screen unknown program for {}s: {}",
                        now - since,
                        c.exe
                    );
                    self.prompt.asked.insert(exe_l);
                    self.prompt.current = None;
                    self.prompt.pending = Some(c);
                }
            }
            _ => self.prompt.current = Some((exe_l, now)),
        }
    }

    /// True once the installed-games scan (Steam + other launchers) has run at least once.
    pub fn installs_scanned(&self) -> bool {
        self.steam_loaded_at.is_some()
    }

    /// True when the installed-games scan should run again.
    pub fn installs_due(&self, now: i64) -> bool {
        self.steam_loaded_at
            .is_none_or(|t| now - t >= STEAM_REFRESH_SECS)
    }

    pub fn refresh_steam(&mut self, now: i64) {
        let scan = InstallScan::load(&self.detector.sigs);
        self.apply_installs(now, scan);
    }

    /// Swap in a scan made by [`InstallScan::load`] (which reads the disk and the
    /// registry, so the app runs it without holding the engine lock).
    pub fn apply_installs(&mut self, now: i64, scan: InstallScan) {
        self.steam_loaded_at = Some(now);
        match scan.steam {
            Some((root, idx)) => {
                self.detector.steam = idx;
                log::info!(
                    "steam: {} apps in {} libraries ({})",
                    self.detector.steam.len(),
                    self.detector.steam.libraries.len(),
                    root.display()
                );
            }
            None => log::info!("steam: not installed / not found"),
        }
        self.detector.libs = scan.libs;
        self.merge_folder_scan();
        {
            use crate::libraries::Launcher::*;
            let c = |l| self.detector.libs.count(l);
            log::info!(
                "libraries: {} Epic, {} GOG, {} Battle.net, {} EA, {} Ubisoft, {} Xbox, {} RSI, {} other installs, {} from the folder scan",
                c(Epic),
                c(Gog),
                c(Battlenet),
                c(Ea),
                c(Ubisoft),
                c(Xbox),
                c(Rsi),
                c(Windows),
                c(Folder)
            );
        }
    }

    /// The cached "Scan for games" results, if a scan ever ran.
    pub fn folder_scan_cache(&self) -> Option<crate::folderscan::ScanCache> {
        self.tracker
            .store
            .get_setting(crate::folderscan::CACHE_KEY)
            .ok()
            .flatten()
            .and_then(|t| serde_json::from_str(&t).ok())
    }

    fn merge_folder_scan(&mut self) {
        if let Some(c) = self.folder_scan_cache() {
            let d = &mut self.detector;
            crate::folderscan::merge_into(&mut d.libs, &d.steam, &d.sigs, &c.found);
        }
    }

    /// Folders for the scan: the usual game folders plus Steam libraries and the
    /// folders launchers install into.
    pub fn folder_scan_roots(&self) -> Vec<std::path::PathBuf> {
        use std::path::{Path, PathBuf};
        let mut extra: Vec<PathBuf> = vec![];
        for l in &self.detector.steam.libraries {
            extra.push(l.join("steamapps").join("common"));
        }
        for g in self.detector.libs.games() {
            if g.store == crate::libraries::Launcher::Folder {
                continue;
            }
            if let Some(p) = Path::new(&g.install_dir).parent() {
                extra.push(p.to_path_buf());
            }
        }
        extra.extend(self.detector.libs.rsi_roots.iter().map(PathBuf::from));
        crate::folderscan::default_roots(&extra)
    }

    /// Classify the exes a folder walk found, cache the result and add it to the
    /// install list. Returns the number of games found.
    pub fn apply_folder_scan(&mut self, exes: &[String], dirs: usize, now: i64) -> usize {
        let found = crate::folderscan::classify(&self.detector, exes);
        let cache = crate::folderscan::ScanCache {
            at: now,
            dirs,
            found,
        };
        if let Ok(t) = serde_json::to_string(&cache) {
            let _ = self
                .tracker
                .store
                .set_setting(crate::folderscan::CACHE_KEY, &t);
        }
        self.detector
            .libs
            .retain(|g| g.store != crate::libraries::Launcher::Folder);
        self.merge_folder_scan();
        log::info!(
            "folder scan: {} exes in {} folders, {} games ({} new to the install list)",
            exes.len(),
            dirs,
            cache.found.len(),
            self.detector.libs.count(crate::libraries::Launcher::Folder)
        );
        cache.found.len()
    }

    /// One poll of the environment. Call every [`DEFAULT_POLL_SECS`].
    pub fn step(&mut self, now: i64) -> Result<Vec<TrackerEvent>> {
        if self.installs_due(now) {
            self.refresh_steam(now);
        }
        let procs = self.env.processes();
        // Idle time counts the same as active time, so input idleness is never read.
        let idle = 0;
        let appid = self.env.steam_running_appid();
        let mut dets = self.detector.detect(&procs, appid);
        pin_identities(&mut self.pinned, &mut dets, &procs);
        let mut running = Vec::with_capacity(dets.len());
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
            running.push((d.game_id.clone(), e.exe_path.clone()));
        }
        self.update_details(&running);
        self.watch_fullscreen(now, &procs, !dets.is_empty());
        self.last_procs = procs;
        self.tracker.tick(now, &dets, idle)
    }

    fn prompted_raw(&self) -> Vec<Prompted> {
        self.tracker
            .store
            .get_setting(PROMPTED_KEY)
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default()
    }

    /// Remember that the app asked about `c` (newest first, one entry per exe).
    pub fn note_prompted(&mut self, c: &Candidate, now: i64) -> Result<()> {
        let mut list = self.prompted_raw();
        list.retain(|p| !p.exe.eq_ignore_ascii_case(&c.exe));
        list.insert(
            0,
            Prompted {
                exe: c.exe.clone(),
                path: c.path.clone(),
                at: now,
            },
        );
        list.truncate(PROMPTED_MAX);
        self.tracker
            .store
            .set_setting(PROMPTED_KEY, &serde_json::to_string(&list)?)
    }

    /// Asked about but not added and not marked "Not a game".
    pub fn prompted(&self) -> Vec<Prompted> {
        self.prompted_raw()
            .into_iter()
            .filter(|p| {
                let e = p.exe.to_lowercase();
                !self.dismissed.contains(&e) && !self.detector.tags.contains_key(&e)
            })
            .collect()
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

    /// beta.5: remove a game the user added (Options > Games > Remove).
    pub fn remove_manual_game(&mut self, game_id: &str) -> Result<()> {
        if !game_id.starts_with("manual:") {
            bail!("only games you added can be removed");
        }
        self.tracker.store.remove_manual_game(game_id)?;
        self.reload_rules()
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

    /// Stats for the tray (checkpoints the live session first so it is included).
    pub fn glance(&mut self, now: i64) -> Result<Glance> {
        self.tracker.checkpoint(now)?;
        let today = self.tracker.day_of(now);
        stats::glance(
            &self.tracker.store,
            &today,
            self.now_playing(),
            self.tracker.is_paused(),
        )
    }
}

/// Steam libraries plus other launchers' install records, read from disk.
pub struct InstallScan {
    pub steam: Option<(std::path::PathBuf, SteamIndex)>,
    pub libs: crate::libraries::LibraryIndex,
}

impl InstallScan {
    pub fn load(sigs: &SignatureDb) -> Self {
        let steam = steam::steam_root().map(|root| {
            let idx = SteamIndex::load(&root);
            (root, idx)
        });
        let mut libs = crate::libraries::LibraryIndex::load();
        crate::installs::resolve(&mut libs, sigs);
        InstallScan { steam, libs }
    }
}

/// Keep each running game process on the id it was first detected as. Processes
/// that exited are forgotten; two detections that end up on one id count once.
pub fn pin_identities(
    pinned: &mut std::collections::HashMap<String, (String, String)>,
    dets: &mut Vec<crate::detector::Detection>,
    procs: &[ProcInfo],
) {
    let key = |p: &ProcInfo| {
        p.path
            .as_deref()
            .map(|x| format!("{}|{}", p.pid, crate::signatures::normalize_path(x)))
    };
    let live: std::collections::HashSet<String> = procs.iter().filter_map(key).collect();
    pinned.retain(|k, _| live.contains(k));
    for d in dets.iter_mut() {
        let Some(exe) = d.exe.as_deref() else {
            continue;
        };
        let Some(k) = procs
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(exe))
            .and_then(key)
        else {
            continue;
        };
        match pinned.get(&k) {
            Some((id, name)) if *id != d.game_id => {
                log::info!("keeping {exe} as {id} (now detected as {})", d.game_id);
                d.game_id = id.clone();
                d.name = name.clone();
            }
            Some(_) => {}
            None => {
                pinned.insert(k, (d.game_id.clone(), d.name.clone()));
            }
        }
    }
    let exe_apps: std::collections::HashSet<u32> = dets
        .iter()
        .filter(|d| d.exe.is_some())
        .filter_map(|d| d.steam_appid)
        .collect();
    let mut seen = std::collections::HashSet::new();
    dets.retain(|d| {
        !(d.exe.is_none() && d.steam_appid.is_some_and(|a| exe_apps.contains(&a)))
            && seen.insert(d.game_id.clone())
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::SimEnv;
    use crate::tracker::fixed_offset_day_fn;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[test]
    #[ignore = "realm detection disabled (realm::ENABLED)"]
    fn wow_realm_is_the_game_detail() {
        std::env::set_var("FRAGLINK_STEAM_DIR", "/nonexistent-steam-dir");
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("World of Warcraft");
        for f in ["_retail_", "_classic_beta_"] {
            std::fs::create_dir_all(root.join(f).join("WTF")).unwrap();
        }
        std::fs::write(
            root.join("_retail_/WTF/Config.wtf"),
            "SET locale \"enUS\"\nSET realmName \"Frostmourne\"\n",
        )
        .unwrap();
        let retail = root.join("_retail_/Wow.exe");
        let forever = root.join("_classic_beta_/WowB.exe");
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        let env = SimEnv::new(
            &format!("{}:100,{}:100", retail.display(), forever.display()),
            Arc::new(move || c.load(Ordering::SeqCst)),
        )
        .unwrap();
        let mut e = Engine::new(
            Store::open_in_memory().unwrap(),
            Box::new(env),
            fixed_offset_day_fn(0),
        )
        .unwrap();
        let t0 = 1_790_030_000;
        e.step(t0).unwrap();
        let np = e.now_playing().unwrap();
        assert_eq!(
            (np.game_id.as_str(), np.detail.as_deref()),
            ("wow", Some("Frostmourne"))
        );
        assert!(e.take_details_changed());
        assert!(!e.take_details_changed());
        assert_eq!(
            e.glance(t0).unwrap().now_playing.unwrap().detail.as_deref(),
            Some("Frostmourne")
        );
        // realm switched in game: Config.wtf rewritten
        std::fs::write(
            root.join("_retail_/WTF/Config.wtf"),
            "SET locale \"enUS\"\nSET realmName \"Barthilas\"\nSET x \"1\"\n",
        )
        .unwrap();
        clock.store(5, Ordering::SeqCst);
        e.step(t0 + 5).unwrap();
        assert!(e.take_details_changed());
        assert_eq!(e.game_detail("wow").as_deref(), Some("Barthilas"));
        // WoW: Forever without a Config.wtf yet: no detail
        for t in (10..130).step_by(5) {
            clock.store(t, Ordering::SeqCst);
            e.step(t0 + t as i64).unwrap();
        }
        assert_eq!(e.game_detail("wow"), None, "retail closed");
        assert_eq!(e.game_detail("wow_forever"), None);
        assert!(e.take_details_changed());
    }

    #[test]
    fn full_screen_unknown_program_is_offered_once() {
        std::env::set_var("FRAGLINK_STEAM_DIR", "/nonexistent-steam-dir");
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        // windowed unknown app, then the same app full screen, then a browser full screen,
        // then a known game full screen, then the unknown app again
        let env = SimEnv::new(
            "C:/Games/Odd/OddGame.exe:400,C:/Games/Odd/OddGame.exe:400:fs,C:/Program Files/Google/Chrome/Application/chrome.exe:400:fs,cs2.exe:400:fs,C:/Tools/Other/Other.exe:200:fs,C:/Tools/Other/Other.exe:400:fs",
            Arc::new(move || c.load(Ordering::SeqCst)),
        )
        .unwrap();
        let mut e = Engine::new(
            Store::open_in_memory().unwrap(),
            Box::new(env),
            fixed_offset_day_fn(0),
        )
        .unwrap();
        let t0 = 1_790_020_000;
        let mut asked = vec![];
        let run = |e: &mut Engine, from: u64, to: u64, asked: &mut Vec<(u64, String)>| {
            for t in (from..to).step_by(5) {
                clock.store(t, Ordering::SeqCst);
                e.step(t0 + t as i64).unwrap();
                if let Some(c) = e.take_prompt() {
                    asked.push((t, c.exe));
                }
            }
        };
        run(&mut e, 0, 2000, &mut asked);
        // windowed: never; full screen: after 5 minutes; chrome / cs2: never.
        // Other.exe: 200 s then a new segment (same exe, new pid) keeps the timer.
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert_eq!(asked[0].1, "OddGame.exe");
        assert!(
            asked[0].0 >= 400 + 300 && asked[0].0 < 400 + 310,
            "{asked:?}"
        );
        assert_eq!(asked[1].1, "Other.exe");
        // "Not a game" sticks; asked once per run anyway
        e.dismiss_prompt("Other.exe").unwrap();
        assert!(e.dismissed.contains("other.exe"));
        let mut again = vec![];
        run(&mut e, 2000, 4000, &mut again);
        assert!(again.is_empty(), "{again:?}");
        // a fresh engine on the same store keeps the dismissal
        assert!(e
            .tracker
            .store
            .get_setting(PROMPT_DISMISSED_KEY)
            .unwrap()
            .unwrap()
            .contains("other.exe"));
    }

    #[test]
    fn prompted_list_drops_added_and_dismissed() {
        std::env::set_var("FRAGLINK_STEAM_DIR", "/nonexistent-steam-dir");
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        let env = SimEnv::new("cs2.exe:600", Arc::new(move || c.load(Ordering::SeqCst))).unwrap();
        let mut e = Engine::new(
            Store::open_in_memory().unwrap(),
            Box::new(env),
            fixed_offset_day_fn(0),
        )
        .unwrap();
        let cand = |x: &str| Candidate {
            exe: x.into(),
            path: format!("C:\\G\\{x}"),
        };
        e.note_prompted(&cand("A.exe"), 1).unwrap();
        e.note_prompted(&cand("B.exe"), 2).unwrap();
        e.note_prompted(&cand("C.exe"), 3).unwrap();
        e.note_prompted(&cand("a.EXE"), 4).unwrap();
        let ex: Vec<_> = e.prompted().into_iter().map(|p| p.exe).collect();
        assert_eq!(ex, ["a.EXE", "C.exe", "B.exe"]);
        e.dismiss_prompt("B.exe").unwrap();
        e.tag_exe("C.exe", "Cee", 5).unwrap();
        let ex: Vec<_> = e.prompted().into_iter().map(|p| p.exe).collect();
        assert_eq!(ex, ["a.EXE"]);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("My Cool Game: Deluxe!"), "my-cool-game-deluxe");
        assert_eq!(slug("  ---  "), "");
    }

    /// End-to-end on the simulated environment with virtual time.
    #[test]
    fn nothing_is_tracked_offline() {
        std::env::set_var("FRAGLINK_STEAM_DIR", "/nonexistent-steam-dir");
        let clock = Arc::new(AtomicU64::new(0));
        let c = clock.clone();
        let env = SimEnv::new("cs2.exe:600", Arc::new(move || c.load(Ordering::SeqCst))).unwrap();
        let mut e = Engine::new(
            Store::open_in_memory().unwrap(),
            Box::new(env),
            fixed_offset_day_fn(36000),
        )
        .unwrap();
        e.set_live(false).unwrap();
        assert!(!e.is_live());
        let t0 = 1_790_020_000;
        for t in (0..=200u64).step_by(5) {
            clock.store(t, Ordering::SeqCst);
            e.step(t0 + t as i64).unwrap();
        }
        assert!(e.now_playing().is_none(), "offline: no session");
        assert_eq!(e.glance(t0 + 200).unwrap().total_seconds, 0);
        assert!(e.glance(t0 + 200).unwrap().paused);
        // connected: tracking starts; settings changes don't flip it
        e.set_live(true).unwrap();
        e.apply_settings().unwrap();
        for t in (205..=400u64).step_by(5) {
            clock.store(t, Ordering::SeqCst);
            e.step(t0 + t as i64).unwrap();
        }
        assert_eq!(e.now_playing().unwrap().game_id, "cs2");
        // the connection drops: the session ends at once
        e.set_live(false).unwrap();
        e.apply_settings().unwrap();
        assert!(e.now_playing().is_none() && !e.is_live());
        let total = e.glance(t0 + 400).unwrap().total_seconds;
        assert!((150..=200).contains(&total), "{total}");
    }

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
        // WoW ran 25 min with no input at all: idle time counts like Steam, so all of it is kept.
        assert!(g
            .top
            .iter()
            .any(|t| t.game_id == "wow" && t.total_seconds >= 1490));
        assert!(events.iter().any(
            |ev| matches!(ev, TrackerEvent::Stopped { game_id, kept: true, .. } if game_id == "wow")
        ));
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

    #[test]
    fn a_running_process_keeps_its_first_id() {
        use crate::detector::{DetectSource, Detection};
        let det = |id: &str, exe: Option<&str>| Detection {
            game_id: id.into(),
            name: id.into(),
            source: DetectSource::Detectable,
            exe: exe.map(Into::into),
            steam_appid: id.strip_prefix("steam:").and_then(|a| a.parse().ok()),
        };
        let procs = vec![ProcInfo {
            pid: 7,
            name: "Operator 112.exe".into(),
            path: Some(r"D:\Games\112 Operator\Operator 112.exe".into()),
        }];
        let mut pinned = Default::default();
        let mut d = vec![det("112_operator", Some("Operator 112.exe"))];
        pin_identities(&mut pinned, &mut d, &procs);
        assert_eq!(d[0].game_id, "112_operator");
        // the Steam rescan: same process, new id, plus RunningAppID under the new id
        let mut d = vec![
            det("steam:793460", Some("Operator 112.exe")),
            det("steam:793460", None),
        ];
        pin_identities(&mut pinned, &mut d, &procs);
        assert_eq!(d.len(), 1, "{d:?}");
        assert_eq!(d[0].game_id, "112_operator");
        // restarted (new pid): the new id is used
        let procs2 = vec![ProcInfo {
            pid: 8,
            ..procs[0].clone()
        }];
        let mut d = vec![det("steam:793460", Some("Operator 112.exe"))];
        pin_identities(&mut pinned, &mut d, &procs2);
        assert_eq!(d[0].game_id, "steam:793460");
        assert_eq!(pinned.len(), 1);
    }
}
