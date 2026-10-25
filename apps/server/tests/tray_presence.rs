//! The tray's own blocking clients (`fraglink_sync::Client` + `PresenceSocket`)
//! against a real server: friend requests by username, live presence with the
//! "started playing" flag, invisible mode, and friend events. This is exactly
//! the code path the desktop presence thread runs.
mod common;
use common::*;
use fraglink_sync::presence::{Event, GameRef, MyPresence, PresenceSocket};
use fraglink_sync::{Client, SyncError};
use sqlx::PgPool;
use std::time::{Duration, Instant};

fn wait_for(sock: &mut PresenceSocket, what: &str, f: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(ev) = sock.poll().unwrap() {
            if f(&ev) {
                return ev;
            }
        }
    }
    panic!("timed out waiting for {what}");
}

fn quiet(sock: &mut PresenceSocket, ms: u64) {
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        if let Some(ev) = sock.poll().unwrap() {
            if !matches!(ev, Event::Pong) {
                panic!("expected silence, got {ev:?}");
            }
        }
    }
}

fn playing(status: &str, game: Option<(&str, &str)>) -> MyPresence {
    // The second value is what the PC would label the game; it is never sent.
    MyPresence {
        status: status.into(),
        game: game.map(|(id, _label)| GameRef {
            name: None,
            id: id.into(),
            started_at: Some(now() - 60),
            detail: None,
            hidden: false,
        }),
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn tray_clients_share_presence(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let wa = signup(&app, "alice@example.com", "Alice").await;
    let wb = signup(&app, "bob@example.com", "bob_b").await;
    let ta = pair(&app, &wa).await;
    let tb = pair(&app, &wb).await;

    tokio::task::spawn_blocking(move || {
        let alice = Client::new(&base, Some(ta));
        let bob = Client::new(&base, Some(tb));
        let mut sa = alice.presence().unwrap();
        let mut sb = bob.presence().unwrap();
        // hello snapshot: no friends yet
        let hello = wait_for(&mut sa, "hello", |e| matches!(e, Event::Hello { .. }));
        let Event::Hello { friends } = hello else {
            unreachable!()
        };
        assert_eq!(friends["friends"].as_array().unwrap().len(), 0);
        wait_for(&mut sb, "hello", |e| matches!(e, Event::Hello { .. }));

        // request by username (case-insensitive, @ allowed); bob is told live
        assert_eq!(alice.friend_request("@BOB_B").unwrap(), "pending");
        let ev = wait_for(&mut sb, "request", |e| {
            matches!(e, Event::FriendRequest { .. })
        });
        assert!(matches!(ev, Event::FriendRequest { ref handle, .. } if handle == "Alice"));
        assert_eq!(bob.friends().unwrap()["incoming"][0]["handle"], "Alice");
        assert!(matches!(
            alice.friend_request("nobody-here"),
            Err(SyncError::Server { status: 404, .. })
        ));

        bob.friend_respond("alice", true).unwrap();
        wait_for(&mut sa, "accepted", |e| {
            matches!(e, Event::FriendAccepted { .. })
        });

        // bob starts a game: alice sees started=true, then a status change without started
        sb.send_presence(&playing("online", Some(("dota2", "Dota 2"))))
            .unwrap();
        let ev = wait_for(
            &mut sa,
            "presence",
            |e| matches!(e, Event::Presence(p) if p.game_id.is_some()),
        );
        let Event::Presence(p) = ev else {
            unreachable!()
        };
        assert_eq!(
            (p.handle.as_str(), p.started, p.game_name.as_deref()),
            ("bob_b", true, Some("Dota 2"))
        );
        sb.send_presence(&playing("away", Some(("dota2", "Dota 2"))))
            .unwrap();
        let Event::Presence(p) = wait_for(&mut sa, "away", |e| matches!(e, Event::Presence(_)))
        else {
            unreachable!()
        };
        assert_eq!((p.status.as_str(), p.started), ("away", false));
        // resending the same presence is not re-broadcast
        sb.send_presence(&playing("away", Some(("dota2", "Dota 2"))))
            .unwrap();
        quiet(&mut sa, 600);

        // invisible: friends see offline, with no game
        sb.send_presence(&playing("invisible", Some(("dota2", "Dota 2"))))
            .unwrap();
        let Event::Presence(p) =
            wait_for(&mut sa, "invisible", |e| matches!(e, Event::Presence(_)))
        else {
            unreachable!()
        };
        assert_eq!(p.status, "offline");
        assert_eq!(p.game_name, None);
        let l = alice.friends().unwrap();
        assert_eq!(l["friends"][0]["presence"]["status"], "offline");
        assert!(l["friends"][0]["presence"]["game_name"].is_null());

        // ping/pong keeps the socket alive
        sa.ping().unwrap();
        wait_for(&mut sa, "pong", |e| matches!(e, Event::Pong));

        // block: alice is only told she was removed
        bob.block("Alice").unwrap();
        wait_for(&mut sa, "removed", |e| {
            matches!(e, Event::FriendRemoved { .. })
        });
        assert_eq!(
            alice.friends().unwrap()["friends"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        // alice's new request is swallowed silently
        assert_eq!(alice.friend_request("bob_b").unwrap(), "pending");
        assert_eq!(
            bob.friends().unwrap()["incoming"].as_array().unwrap().len(),
            0
        );
        sb.send_presence(&playing("online", Some(("cs2", "Counter-Strike 2"))))
            .unwrap();
        quiet(&mut sa, 600);

        // bob closes the tray: once unblocked and friends again, alice sees him offline
        bob.unblock("alice").unwrap();
        sb.close();
        alice.friend_request("bob_b").unwrap();
        bob.friend_respond("Alice", true).unwrap();
        // the server notices the closed socket asynchronously
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let l = alice.friends().unwrap();
            assert_eq!(l["friends"].as_array().unwrap().len(), 1);
            if l["friends"][0]["presence"]["status"] == "offline" {
                break;
            }
            assert!(Instant::now() < deadline, "bob never went offline: {l}");
            std::thread::sleep(Duration::from_millis(100));
        }
        sa.close();
    })
    .await
    .unwrap();
}
