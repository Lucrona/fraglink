//! Multi-client presence over real WebSockets against a server on a random port.
mod common;
use common::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Client {
    ws: Ws,
}

impl Client {
    async fn connect(base: &str, token: &str) -> Client {
        let mut req = format!("{}/api/v1/ws", base.replace("http://", "ws://"))
            .into_client_request()
            .unwrap();
        req.headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        let (ws, _) = tokio_tungstenite::connect_async(req)
            .await
            .expect("ws connect");
        let mut c = Client { ws };
        let hello = c.next().await.expect("hello");
        assert_eq!(hello["type"], "hello");
        c
    }
    async fn send(&mut self, v: Value) {
        self.ws
            .send(Message::Text(v.to_string().into()))
            .await
            .unwrap();
    }
    async fn presence(&mut self, status: &str, game: Option<(&str, &str)>) {
        let game = game
            .map(|(id, name)| json!({ "id": id, "name": name, "started_at": 1_790_000_000i64 }));
        self.send(json!({ "type": "presence", "status": status, "game": game }))
            .await;
    }
    /// Next JSON message within 3 s (None on timeout).
    async fn next(&mut self) -> Option<Value> {
        loop {
            match tokio::time::timeout(Duration::from_secs(3), self.ws.next()).await {
                Ok(Some(Ok(Message::Text(t)))) => return serde_json::from_str(&t).ok(),
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
    }
    /// Next message of a given type (skipping others) within 3 s.
    async fn expect(&mut self, ty: &str) -> Value {
        loop {
            let m = self
                .next()
                .await
                .unwrap_or_else(|| panic!("no {ty} message"));
            if m["type"] == ty {
                return m;
            }
        }
    }
    /// Assert nothing arrives within `ms`.
    async fn quiet(&mut self, ms: u64) {
        if let Ok(Some(Ok(Message::Text(t)))) =
            tokio::time::timeout(Duration::from_millis(ms), self.ws.next()).await
        {
            panic!("unexpected message: {t}");
        }
    }
}

async fn befriend(app: &axum::Router, a: &str, b: &str, b_handle: &str, a_handle: &str) {
    api(
        app,
        "POST",
        "/api/v1/friends/request",
        Some(a),
        Some(json!({ "handle": b_handle })),
    )
    .await;
    let r = api(
        app,
        "POST",
        "/api/v1/friends/respond",
        Some(b),
        Some(json!({ "handle": a_handle, "accept": true })),
    )
    .await;
    assert_eq!(r.status, axum::http::StatusCode::NO_CONTENT);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn presence_fans_out_to_friends_only(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await; // not a friend
    befriend(&app, &a, &b, "bob", "alice").await;
    let a_dev = pair(&app, &a).await; // the tray uses its device token

    let mut ca = Client::connect(&base, &a_dev).await;
    let mut cb = Client::connect(&base, &b).await;
    let mut cc = Client::connect(&base, &c).await;

    // alice comes online, then starts a game
    let t0 = std::time::Instant::now();
    ca.presence("online", None).await;
    let m = cb.expect("presence").await;
    assert_eq!(
        (
            m["handle"].as_str(),
            m["status"].as_str(),
            m["started"].as_bool()
        ),
        (Some("alice"), Some("online"), Some(false))
    );
    ca.presence("online", Some(("wow", "World of Warcraft")))
        .await;
    let m = cb.expect("presence").await;
    assert_eq!(m["game_name"], "World of Warcraft");
    assert_eq!(m["started"], true);
    assert!(m["session_started_at"].is_string());
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "presence should arrive well under 5 s"
    );
    cc.quiet(500).await; // carol isn't a friend: nothing
                         // the same state again sends nothing
    ca.presence("online", Some(("wow", "World of Warcraft")))
        .await;
    cb.quiet(500).await;

    // the realm (game detail) is dropped for now (dev.18): nothing changes for friends
    ca.send(json!({ "type": "presence", "status": "online", "game": { "id": "wow", "name": "World of Warcraft", "started_at": 1_790_000_000i64, "detail": "Frostmourne\u{7}" } }))
        .await;
    cb.quiet(500).await;
    let l = api(&app, "GET", "/api/v1/friends", Some(&b), None)
        .await
        .json();
    assert!(l["friends"][0]["presence"]["game_detail"].is_null());

    // bob's REST list shows it too
    let l = api(&app, "GET", "/api/v1/friends", Some(&b), None)
        .await
        .json();
    assert_eq!(l["friends"][0]["presence"]["game_id"], "wow");

    // invisible: friends see offline, with no game, and the REST list agrees
    ca.presence("invisible", Some(("wow", "World of Warcraft")))
        .await;
    let m = cb.expect("presence").await;
    assert_eq!(m["status"], "offline");
    assert!(m["game_id"].is_null());
    let l = api(&app, "GET", "/api/v1/friends", Some(&b), None)
        .await
        .json();
    assert_eq!(l["friends"][0]["presence"]["status"], "offline");
    assert!(l["friends"][0]["presence"]["game_id"].is_null());

    // away
    ca.presence("away", None).await;
    assert_eq!(cb.expect("presence").await["status"], "away");

    // ping/pong heartbeat
    ca.send(json!({ "type": "ping" })).await;
    ca.expect("pong").await;

    // a second device for alice: closing one socket keeps her online
    let mut ca2 = Client::connect(&base, &a).await;
    ca2.ws.close(None).await.unwrap();
    cb.quiet(500).await;
    // closing the last socket (killed client) -> offline for friends
    drop(ca);
    let m = cb.expect("presence").await;
    assert_eq!(
        (m["handle"].as_str(), m["status"].as_str()),
        (Some("alice"), Some("offline"))
    );
    cc.quiet(300).await;
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn requests_accepts_and_blocks_are_pushed(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let mut ca = Client::connect(&base, &a).await;
    let mut cb = Client::connect(&base, &b).await;
    ca.presence("online", Some(("cs2", "Counter-Strike 2")))
        .await;
    cb.presence("online", None).await;
    cb.quiet(300).await; // not friends yet: no presence

    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&a),
        Some(json!({ "handle": "bob" })),
    )
    .await;
    let m = cb.expect("friend_request").await;
    assert_eq!(m["handle"], "alice");

    api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&b),
        Some(json!({ "handle": "alice", "accept": true })),
    )
    .await;
    assert_eq!(ca.expect("friend_accepted").await["handle"], "bob");
    // on accept, each side gets the other's current presence
    // (a slow runner may deliver an earlier presence update first; the game must follow)
    let mut p = cb.expect("presence").await;
    for _ in 0..3 {
        if p["game_id"].as_str().is_some() {
            break;
        }
        p = cb.expect("presence").await;
    }
    assert_eq!(
        (p["handle"].as_str(), p["game_id"].as_str()),
        (Some("alice"), Some("cs2"))
    );
    assert_eq!(ca.expect("presence").await["handle"], "bob");

    // hello snapshot on a new connection includes the friend with presence
    let mut req = format!("{}/api/v1/ws", base.replace("http://", "ws://"))
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("authorization", format!("Bearer {b}").parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let hello: Value = match ws.next().await.unwrap().unwrap() {
        Message::Text(t) => serde_json::from_str(&t).unwrap(),
        m => panic!("{m:?}"),
    };
    assert_eq!(hello["friends"]["friends"][0]["handle"], "alice");
    assert_eq!(hello["friends"]["friends"][0]["presence"]["game_id"], "cs2");
    drop(ws);

    // bob blocks alice: alice just sees "removed"; later presence from alice never reaches bob
    api(
        &app,
        "POST",
        "/api/v1/blocks",
        Some(&b),
        Some(json!({ "handle": "alice" })),
    )
    .await;
    assert_eq!(ca.expect("friend_removed").await["handle"], "bob");
    ca.presence("online", Some(("dota2", "Dota 2"))).await;
    loop {
        match cb.next().await {
            None => break,
            Some(m) => assert_ne!(m["type"], "presence", "blocked user's presence leaked: {m}"),
        }
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn websocket_needs_a_valid_token(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut req = format!("{base}/api/v1/ws").into_client_request().unwrap();
    req.headers_mut()
        .insert("authorization", "Bearer nope".parse().unwrap());
    assert!(tokio_tungstenite::connect_async(req).await.is_err());
    assert!(
        tokio_tungstenite::connect_async(format!("{base}/api/v1/ws"))
            .await
            .is_err()
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn silent_client_times_out_to_offline(pool: PgPool) {
    let app = fraglink_server::router(state_with(pool, |c| c.presence_idle_secs = 2));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    befriend(&app, &a, &b, "bob", "alice").await;
    // bob first: sockets have a 2 s idle limit (slow CI runners need the slack), so both speak right away
    let mut cb = Client::connect(&base, &b).await;
    cb.send(json!({ "type": "ping" })).await;
    let mut ca = Client::connect(&base, &a).await;
    ca.presence("online", Some(("wow", "World of Warcraft")))
        .await;
    assert_eq!(cb.expect("presence").await["status"], "online");
    // bob keeps pinging, alice goes silent (a hung or killed client that never closed its socket)
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    let mut went_offline = false;
    while std::time::Instant::now() < deadline {
        cb.send(json!({ "type": "ping" })).await;
        if let Some(m) = cb.next().await {
            if m["type"] == "presence" && m["handle"] == "alice" && m["status"] == "offline" {
                went_offline = true;
                break;
            }
        }
    }
    assert!(
        went_offline,
        "alice should be shown offline after the idle timeout"
    );
    drop(ca);
}

/// dev.14: names come from the server, unknown games are "a game" until an admin
/// approves them, hidden games count but aren't shown, and hours are only what the
/// server saw live.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn server_names_and_live_sessions(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    befriend(&app, &a, &b, "bob", "alice").await;
    let mut ca = Client::connect(&base, &a).await;
    let mut cb = Client::connect(&base, &b).await;
    let backdate = |mins: i64| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "UPDATE presence SET session_started_at = now() - make_interval(mins => $1)
                 WHERE user_id = (SELECT id FROM users WHERE handle = 'alice')",
            )
            .bind(mins as i32)
            .execute(&pool)
            .await
            .unwrap();
        }
    };

    // a client-sent name is ignored: the list name is shown
    ca.presence("online", Some(("wow", "Totally Not WoW")))
        .await;
    let m = cb.expect("presence").await;
    assert_eq!(
        (m["game_id"].as_str(), m["game_name"].as_str()),
        (Some("wow"), Some("World of Warcraft"))
    );
    backdate(10).await;

    // a tagged program nobody approved: "a game", and its label never leaves the PC
    ca.presence("online", Some(("exe:MyThing.exe", "My Secret Label")))
        .await;
    let m = cb.expect("presence").await;
    assert_eq!(
        (m["game_id"].as_str(), m["game_name"].as_str()),
        (Some("unlisted"), Some("a game"))
    );
    assert!(!m.to_string().contains("Secret") && !m.to_string().contains("mything"));
    let l = api(&app, "GET", "/api/v1/friends", Some(&b), None)
        .await
        .json();
    assert_eq!(l["friends"][0]["presence"]["game_name"], "a game");
    // switching games closed the WoW session the server saw (~10 minutes)
    let rows: Vec<(String, i32, String)> =
        sqlx::query_as("SELECT game_id, duration_s, source FROM sessions ORDER BY started_at")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].0.as_str(), rows[0].2.as_str()), ("wow", "live"));
    assert!((595..=605).contains(&rows[0].1), "{rows:?}");
    backdate(20).await;

    // an admin approves it: same id, the hours stay attached, the name shows
    fraglink_server::names::approve(&pool, "mything.exe", "My Thing")
        .await
        .unwrap();
    ca.presence("online", Some(("wow", ""))).await; // closes 20 min of exe:mything.exe
    cb.expect("presence").await;
    ca.presence("online", Some(("exe:mything.exe", "whatever")))
        .await;
    let m = cb.expect("presence").await;
    assert_eq!(
        (m["game_id"].as_str(), m["game_name"].as_str()),
        (Some("exe:mything.exe"), Some("My Thing"))
    );
    let stats = api(&app, "GET", "/api/v1/stats", Some(&a), None)
        .await
        .json();
    let s = stats.to_string();
    assert!(
        s.contains("My Thing") && s.contains("World of Warcraft"),
        "{s}"
    );
    // ...and approved programs are handed to every app as detection rules
    let o = api(&app, "GET", "/api/v1/games/overrides", None, None).await;
    assert!(
        o.text.contains("exe:mything.exe") && o.text.contains("mything.exe"),
        "{}",
        o.text
    );

    // "Share what I'm playing" off: time still counts, friends see no game
    ca.send(
        json!({ "type": "presence", "status": "online", "game": { "id": "wow", "hidden": true } }),
    )
    .await;
    let m = cb.expect("presence").await;
    assert!(m["game_id"].is_null() && m["game_name"].is_null(), "{m}");
    backdate(5).await;
    ca.presence("online", None).await; // stop playing (friends see no change)
    let mut n = 0i64;
    for _ in 0..30 {
        n = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = 'wow'")
            .fetch_one(&pool)
            .await
            .unwrap();
        if n == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(n, 2, "the hidden WoW time was recorded too");

    // the activity feed uses public names only
    let feed = api(&app, "GET", "/api/v1/friends/activity", Some(&b), None).await;
    assert!(!feed.text.contains("Secret"), "{}", feed.text);
}
