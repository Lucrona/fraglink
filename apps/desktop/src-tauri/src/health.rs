//! Crash and freeze diagnostics (0.3.0-beta.4): a panic hook that writes the
//! message and a backtrace to the log and to `crash-<time>.txt` next to it, a
//! poison-proof `lock_ok()` for the app's mutexes, and a watchdog that logs
//! "not responding" when the tracker thread or the window thread stops beating
//! for more than [`STALL_SECS`].

use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub const STALL_SECS: i64 = 30;

/// `lock()` that carries on with the data if another thread panicked while
/// holding the lock, instead of panicking too (which took the whole app down).
pub trait LockOk<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T> LockOk<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|p| {
            log::warn!("recovering a poisoned lock");
            p.into_inner()
        })
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

static TRACKER_BEAT: AtomicI64 = AtomicI64::new(0);
static UI_BEAT: AtomicI64 = AtomicI64::new(0);
static TRACKER_PHASE: Mutex<&'static str> = Mutex::new("starting");

/// The tracker thread calls this as it moves through a poll.
pub fn beat_tracker(phase: &'static str) {
    TRACKER_BEAT.store(now(), Ordering::Relaxed);
    *TRACKER_PHASE.lock_ok() = phase;
}

pub fn install_panic_hook(dir: PathBuf) {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed").to_string();
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!(
            "Fraglink {} panicked on thread '{name}': {info}\n\n{bt}\n",
            env!("CARGO_PKG_VERSION")
        );
        log::error!("{text}");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("crash-{}.txt", now())), &text);
        prev(info);
    }));
}

/// Is a stall worth reporting? `beat` 0 means the thread has not started yet.
pub fn stalled(beat: i64, now: i64) -> bool {
    beat > 0 && now - beat > STALL_SECS
}

pub fn start_watchdog(app: AppHandle) {
    UI_BEAT.store(now(), Ordering::Relaxed);
    let _ = std::thread::Builder::new()
        .name("fraglink-watchdog".into())
        .spawn(move || {
            let (mut tracker_told, mut ui_told) = (false, false);
            loop {
                std::thread::sleep(Duration::from_secs(5));
                // the window thread answers this when it is free
                let _ = app.run_on_main_thread(|| UI_BEAT.store(now(), Ordering::Relaxed));
                let t = now();
                let (tb, ub) = (
                    TRACKER_BEAT.load(Ordering::Relaxed),
                    UI_BEAT.load(Ordering::Relaxed),
                );
                let (ts, us) = (stalled(tb, t), stalled(ub, t));
                if (ts && !tracker_told) || (us && !ui_told) {
                    let st = app.state::<crate::AppState>();
                    let engine = if st.engine.try_lock().is_ok() { "free" } else { "held" };
                    log::error!(
                        "not responding: tracker {}s since last beat (in '{}'), window {}s, engine lock {engine}",
                        t - tb,
                        *TRACKER_PHASE.lock_ok(),
                        t - ub,
                    );
                }
                if tracker_told && !ts {
                    log::warn!("tracker responding again");
                }
                if ui_told && !us {
                    log::warn!("window responding again");
                }
                (tracker_told, ui_told) = (ts, us);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisoned_lock_recovers() {
        let m = std::sync::Arc::new(Mutex::new(1));
        let m2 = m.clone();
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("boom");
        })
        .join();
        assert!(m.is_poisoned());
        *m.lock_ok() += 1;
        assert_eq!(*m.lock_ok(), 2);
    }

    #[test]
    fn stall_threshold() {
        assert!(!stalled(0, 100));
        assert!(!stalled(100, 130));
        assert!(stalled(100, 131));
    }
}
