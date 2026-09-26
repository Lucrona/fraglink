//! End to end: the desktop's local store -> outbox -> fraglink-sync client ->
//! a real server on a random port -> Postgres. Exactly what the tray app does.
mod common;
use common::*;
use fraglink_core::store::Store;
use fraglink_sync::{Client, PollResult, SyncError};
use serde_json::json;
use sqlx::PgPool;
use std::collections::BTreeMap;

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

    // 2) local sessions -> outbox -> upload
    let b = base.clone();
    let tk = token.clone();
    let pending_after = tokio::task::spawn_blocking(move || {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("fraglink.db")).unwrap();
        s.upsert_game("cs2", "Counter-Strike 2", "signature", Some(730), 0)
            .unwrap();
        let t = chrono::Utc::now().timestamp() - 86400;
        let d = BTreeMap::new();
        let u1 = uuid();
        let a = s
            .open_session(&u1, "cs2", t, "signature", Some("cs2.exe"), "0.2.0")
            .unwrap();
        s.close_session(a, "cs2", t + 3600, 3000, 600, &d).unwrap();
        let u2 = uuid();
        let b2 = s
            .open_session(&u2, "cs2", t + 7200, "signature", Some("cs2.exe"), "0.2.0")
            .unwrap();
        s.close_session(b2, "cs2", t + 7200 + 90000, 90000, 0, &d)
            .unwrap(); // > 24 h: server rejects
        assert_eq!(s.pending_upload_count().unwrap(), 2);

        let c = Client::new(&b, Some(tk));
        assert_eq!(c.me().unwrap().handle, "desk");
        let batch = s.pending_upload(100).unwrap();
        let body = serde_json::to_string(&batch).unwrap();
        assert!(!body.contains("cs2.exe"), "exe names never leave the PC");
        let res = c.upload(&batch).unwrap();
        assert_eq!(res.accepted, vec![u1.clone()]);
        assert_eq!(res.rejected.len(), 1);
        s.mark_uploaded(&res.accepted, chrono::Utc::now().timestamp())
            .unwrap();
        for r in &res.rejected {
            s.mark_rejected(&r.id, &r.reason).unwrap();
        }
        // re-running the loop sends nothing
        let again = s.pending_upload(100).unwrap();
        assert!(again.is_empty());
        s.pending_upload_count().unwrap()
    })
    .await
    .unwrap();
    assert_eq!(pending_after, 0);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let dur: i32 = sqlx::query_scalar("SELECT duration_s FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dur, 3000);

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
