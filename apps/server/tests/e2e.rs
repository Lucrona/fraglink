//! End to end: the desktop's fraglink-sync client pairing against a real server
//! on a random port, and old-style session uploads being dropped.
mod common;
use common::*;
use fraglink_sync::{Client, PollResult, SyncError};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn desktop_pairs_and_syncs(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let served = app.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let web = signup(&app, "desk@example.com", "desk").await;

    // 1) pairing via the blocking client (as the desktop app does in a thread)
    let b = base.clone();
    let (client, start) = tokio::task::spawn_blocking(move || {
        let c = Client::new(&b, None);
        let s = c.start_pairing("Desk PC").unwrap();
        assert_eq!(c.poll(&s.device_code).unwrap(), PollResult::Pending);
        (c, s)
    })
    .await
    .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/device/approve",
        Some(&web),
        Some(json!({ "user_code": start.user_code })),
    )
    .await;
    assert_eq!(r.status, axum::http::StatusCode::OK);
    let dc = start.device_code.clone();
    let token = tokio::task::spawn_blocking(move || match client.poll(&dc).unwrap() {
        PollResult::Approved { token, user } => {
            assert_eq!(user.handle, "desk");
            assert_eq!(client.poll(&dc).unwrap(), PollResult::Expired);
            token
        }
        other => panic!("{other:?}"),
    })
    .await
    .unwrap();

    // 2) session uploads were removed (hours only come from live presence): 410
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&token),
        Some(batch(
            json!([{ "id": "cs2", "name": "Counter-Strike 2", "steam_appid": 730 }]),
            vec![session(&uuid(), "cs2", now() - 86400, 3600)],
        )),
    )
    .await;
    assert_eq!(r.status, axum::http::StatusCode::GONE, "{}", r.text);
    let b = base.clone();
    let tk = token.clone();
    tokio::task::spawn_blocking(move || {
        assert_eq!(Client::new(&b, Some(tk)).me().unwrap().handle, "desk");
    })
    .await
    .unwrap();

    // 3) signing out revokes the device token
    let b = base.clone();
    tokio::task::spawn_blocking(move || {
        let c = Client::new(&b, Some(token));
        c.logout().unwrap();
        assert!(matches!(c.me(), Err(SyncError::Unauthorized)));
    })
    .await
    .unwrap();
}
