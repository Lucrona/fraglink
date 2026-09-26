//! Simulation mode: a fake [`Environment`] that plays a scripted sequence of
//! games so the whole app can be exercised on Linux/CI without real games.
//!
//! Script syntax (comma-separated segments, looped forever):
//! `<what>:<seconds>[:idle]`
//! * `-`            nothing running
//! * `cs2.exe`      a process with that exe name
//! * `C:/Games/X/X.exe`  a process with a full path (name = file name)
//! * `steam:570`    Steam running with RunningAppID = 570
//! * `:idle`        no user input during the segment (idle grows from segment start)

use crate::env::{Environment, ProcInfo};
use anyhow::{bail, Result};
use std::sync::Arc;

pub const DEFAULT_SCRIPT: &str =
    "-:10,cs2.exe:150,-:20,Wow.exe:200,-:20,steam:570:150,-:20,C:/Games/MyIndie/MyIndie.exe:120,-:20,Wow.exe:180:idle,-:30";

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub procs: Vec<ProcInfo>,
    pub secs: u64,
    pub idle: bool,
    pub steam_appid: Option<u32>,
}

pub fn parse_script(script: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    for (i, raw) in script
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
    {
        // split from the right so Windows drive letters ("C:/...") survive
        let (body, idle) = match raw.strip_suffix(":idle") {
            Some(b) => (b, true),
            None => (raw, false),
        };
        let Some((what, secs)) = body.rsplit_once(':') else {
            bail!("segment {i} '{raw}': expected <what>:<seconds>")
        };
        let secs: u64 = secs
            .parse()
            .map_err(|_| anyhow::anyhow!("segment {i} '{raw}': bad seconds"))?;
        if secs == 0 {
            bail!("segment {i} '{raw}': seconds must be > 0");
        }
        let pid = 50_000 + i as u32;
        let mut seg = Segment {
            procs: vec![],
            secs,
            idle,
            steam_appid: None,
        };
        if let Some(appid) = what.strip_prefix("steam:") {
            seg.steam_appid = Some(
                appid
                    .parse()
                    .map_err(|_| anyhow::anyhow!("segment {i}: bad app id"))?,
            );
            seg.procs.push(ProcInfo {
                pid,
                name: "steam.exe".into(),
                path: Some(r"C:\Program Files (x86)\Steam\steam.exe".into()),
            });
        } else if what != "-" {
            let norm = what.replace('\\', "/");
            let (name, path) = match norm.rsplit_once('/') {
                Some((_, n)) => (n.to_string(), Some(what.to_string())),
                None => (what.to_string(), Some(format!(r"C:\Games\Sim\{what}"))),
            };
            seg.procs.push(ProcInfo { pid, name, path });
        }
        out.push(seg);
    }
    if out.is_empty() {
        bail!("empty simulation script");
    }
    Ok(out)
}

/// Seconds since the simulation started (real or virtual time).
pub type SimClock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct SimEnv {
    segs: Vec<Segment>,
    cycle: u64,
    clock: SimClock,
    background: Vec<ProcInfo>,
}

impl SimEnv {
    pub fn new(script: &str, clock: SimClock) -> Result<Self> {
        let segs = parse_script(script)?;
        let cycle = segs.iter().map(|s| s.secs).sum();
        let background = vec![
            ProcInfo {
                pid: 4,
                name: "explorer.exe".into(),
                path: Some(r"C:\Windows\explorer.exe".into()),
            },
            ProcInfo {
                pid: 5,
                name: "chrome.exe".into(),
                path: Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe".into()),
            },
            ProcInfo {
                pid: 6,
                name: "Discord.exe".into(),
                path: Some(r"C:\Users\me\AppData\Local\Discord\app-1.0\Discord.exe".into()),
            },
        ];
        Ok(SimEnv {
            segs,
            cycle,
            clock,
            background,
        })
    }

    /// Real-time clock starting now.
    pub fn realtime(script: &str) -> Result<Self> {
        let t0 = std::time::Instant::now();
        Self::new(script, Arc::new(move || t0.elapsed().as_secs()))
    }

    /// (segment, seconds into segment) at elapsed time `t`.
    fn at(&self, t: u64) -> (&Segment, u64) {
        let mut x = t % self.cycle;
        for s in &self.segs {
            if x < s.secs {
                return (s, x);
            }
            x -= s.secs;
        }
        (self.segs.last().expect("non-empty"), 0)
    }
}

impl Environment for SimEnv {
    fn processes(&mut self) -> Vec<ProcInfo> {
        let (seg, _) = self.at((self.clock)());
        let mut v = self.background.clone();
        v.extend(seg.procs.iter().cloned());
        v
    }

    fn idle_seconds(&mut self) -> u64 {
        let (seg, into) = self.at((self.clock)());
        if seg.idle {
            into
        } else {
            0
        }
    }

    fn steam_running_appid(&mut self) -> Option<u32> {
        self.at((self.clock)()).0.steam_appid
    }

    fn label(&self) -> &'static str {
        "simulation"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn parses_all_segment_kinds() {
        let segs =
            parse_script("-:5, cs2.exe:10, C:/Games/X/X.exe:20, steam:570:30, Wow.exe:40:idle")
                .unwrap();
        assert_eq!(segs.len(), 5);
        assert!(segs[0].procs.is_empty());
        assert_eq!(segs[1].procs[0].name, "cs2.exe");
        assert_eq!(segs[2].procs[0].name, "X.exe");
        assert_eq!(segs[2].procs[0].path.as_deref(), Some("C:/Games/X/X.exe"));
        assert_eq!(segs[3].steam_appid, Some(570));
        assert!(segs[4].idle);
        assert!(parse_script("cs2.exe").is_err());
        assert!(parse_script("cs2.exe:abc").is_err());
        assert!(parse_script("").is_err());
    }

    #[test]
    fn clock_drives_segments_and_loops() {
        let t = Arc::new(AtomicU64::new(0));
        let tc = t.clone();
        let mut env = SimEnv::new(
            "-:10,Wow.exe:20:idle",
            Arc::new(move || tc.load(Ordering::SeqCst)),
        )
        .unwrap();
        assert!(!env.processes().iter().any(|p| p.name == "Wow.exe"));
        t.store(15, Ordering::SeqCst);
        assert!(env.processes().iter().any(|p| p.name == "Wow.exe"));
        assert_eq!(env.idle_seconds(), 5);
        t.store(35, Ordering::SeqCst); // looped back into the first segment
        assert!(!env.processes().iter().any(|p| p.name == "Wow.exe"));
    }
}
