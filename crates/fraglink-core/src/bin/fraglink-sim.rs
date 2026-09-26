//! Headless Fraglink tracker, for testing on Linux and measuring footprint.
//!
//! ```text
//! fraglink-sim [--db PATH] [--script SCRIPT] [--fast] [--duration SECS] [--system] [--export DIR]
//! ```
//! * default: simulated environment in real time (5 s polls)
//! * `--fast`: virtual time, no sleeping (runs a whole script in milliseconds)
//! * `--system`: poll the real OS instead of the simulation
//!
//! Prints one JSON line per tracker event, then the stats glance and resource usage.

use fraglink_core::engine::{Engine, DEFAULT_POLL_SECS};
use fraglink_core::env::{Environment, SystemEnv};
use fraglink_core::sim::{SimEnv, DEFAULT_SCRIPT};
use fraglink_core::store::Store;
use fraglink_core::tracker::local_day_fn;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn usage() -> anyhow::Result<()> {
    println!("usage: fraglink-sim [--db PATH] [--script SCRIPT] [--fast] [--duration SECS] [--system] [--export DIR]");
    println!("default script: {DEFAULT_SCRIPT}");
    Ok(())
}

/// (user+system CPU seconds, RSS kB) of this process, Linux only.
fn usage_now() -> Option<(f64, u64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let f: Vec<&str> = rest.split_whitespace().collect();
    let ticks: f64 = f.get(11)?.parse::<f64>().ok()? + f.get(12)?.parse::<f64>().ok()?;
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let rss = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    Some((ticks / 100.0, rss))
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return usage();
    }
    let fast = args.iter().any(|a| a == "--fast");
    let system = args.iter().any(|a| a == "--system");
    let script = arg(&args, "--script")
        .or_else(|| std::env::var("FRAGLINK_SIM_SCRIPT").ok())
        .unwrap_or_else(|| DEFAULT_SCRIPT.into());
    let duration: u64 = arg(&args, "--duration")
        .and_then(|s| s.parse().ok())
        .unwrap_or(if fast { 1200 } else { 120 });
    let store = match arg(&args, "--db") {
        Some(p) => Store::open(std::path::Path::new(&p))?,
        None => Store::open_in_memory()?,
    };

    let vclock = Arc::new(AtomicU64::new(0));
    let env: Box<dyn Environment> = if system {
        Box::new(SystemEnv::new())
    } else if fast {
        let c = vclock.clone();
        Box::new(SimEnv::new(
            &script,
            Arc::new(move || c.load(Ordering::SeqCst)),
        )?)
    } else {
        Box::new(SimEnv::realtime(&script)?)
    };
    let mut engine = Engine::new(store, env, local_day_fn())?;
    for ev in engine.tracker.recover()? {
        println!("{}", serde_json::to_string(&ev)?);
    }
    eprintln!(
        "fraglink-sim: env={} fast={} duration={}s",
        engine.env_label(),
        fast,
        duration
    );

    let start_wall = chrono::Utc::now().timestamp();
    let t_start = Instant::now();
    let mut max_step = Duration::ZERO;
    let mut steps = 0u64;
    let mut t = 0u64;
    while t <= duration {
        let now = if fast {
            start_wall + t as i64
        } else {
            chrono::Utc::now().timestamp()
        };
        vclock.store(t, Ordering::SeqCst);
        let s = Instant::now();
        for ev in engine.step(now)? {
            println!("{}", serde_json::to_string(&ev)?);
        }
        max_step = max_step.max(s.elapsed());
        steps += 1;
        t += DEFAULT_POLL_SECS;
        if !fast {
            std::thread::sleep(Duration::from_secs(DEFAULT_POLL_SECS));
        }
    }
    let now = if fast {
        start_wall + t as i64
    } else {
        chrono::Utc::now().timestamp()
    };
    let glance = engine.glance(now)?;
    for ev in engine.tracker.stop()? {
        println!("{}", serde_json::to_string(&ev)?);
    }
    println!("{}", serde_json::to_string_pretty(&glance)?);
    if let Some(dir) = arg(&args, "--export") {
        let z = fraglink_core::export::write_export(
            &engine.tracker.store,
            std::path::Path::new(&dir),
            now,
        )?;
        eprintln!("exported {}", z.display());
    }
    let wall = t_start.elapsed().as_secs_f64();
    if let Some((cpu, rss)) = usage_now() {
        eprintln!(
            "resources: steps={steps} wall={wall:.1}s cpu={cpu:.3}s ({:.3}% of one core) max_step={:.2}ms rss={}kB",
            100.0 * cpu / wall.max(0.001),
            max_step.as_secs_f64() * 1000.0,
            rss
        );
    }
    Ok(())
}
