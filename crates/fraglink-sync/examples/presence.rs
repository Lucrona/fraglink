//! Dev tool: act as a friend's tray. Connects to the presence WebSocket with an
//! API token, publishes a status (and optionally a game), prints events, and
//! stays online until killed.
//!
//! cargo run -p fraglink-sync --example presence -- http://localhost:8787 TOKEN online steam:570 "Dota 2"
use fraglink_sync::presence::{GameRef, MyPresence};
use fraglink_sync::Client;
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: presence SERVER TOKEN online|away|invisible [GAME_ID [DETAIL]]");
        std::process::exit(2);
    }
    let game = (a.len() >= 4).then(|| GameRef {
        name: None,
        id: a[3].clone(),
        started_at: Some(chrono_now() - 38 * 60),
        detail: a.get(4).cloned(),
        hidden: false,
    });
    let mut sock = Client::new(&a[0], Some(a[1].clone()))
        .presence()
        .expect("connect");
    sock.send_presence(&MyPresence {
        status: a[2].clone(),
        game,
    })
    .expect("send");
    let mut last_ping = Instant::now();
    loop {
        match sock.poll() {
            Ok(Some(ev)) => println!("{ev:?}"),
            Ok(None) => {}
            Err(e) => {
                eprintln!("disconnected: {e}");
                return;
            }
        }
        if last_ping.elapsed() > Duration::from_secs(60) {
            let _ = sock.ping();
            last_ping = Instant::now();
        }
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
