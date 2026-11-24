//! Staff-started catalogue banner backfill: one pick query, no auto-start,
//! stop/resume, miss backoff, off switch, and a disk cap.
mod common;
use axum::http::StatusCode;
use common::*;
use fraglink_server::art_backfill::{
    pause_on_boot, pick_due, pick_sql_count, reset_pick_sql_count, status,
};
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

const FOUND: u32 = 9_200_001;
const MISS: u32 = 9_200_002;

struct Cdn {
    bodies: Arc<Mutex<HashMap<u32, Vec<u8>>>>,
    base: String,
}

async fn start_cdn() -> Cdn {
    let bodies: Arc<Mutex<HashMap<u32, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    let b = bodies.clone();
    let up = axum::Router::new().route(
        "/apps/{appid}/header.jpg",
        axum::routing::get(
            move |axum::extract::Path(appid): axum::extract::Path<u32>| {
                let b = b.clone();
                async move {
                    match b.lock().unwrap().get(&appid).cloned() {
                        Some(body) => (StatusCode::OK, body),
                        None => (StatusCode::NOT_FOUND, vec![]),
                    }
                }
            },
        ),
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, up).await.unwrap() });
    Cdn { bodies, base }
}

impl Cdn {
    fn put(&self, appid: u32, body: Vec<u8>) {
        self.bodies.lock().unwrap().insert(appid, body);
    }
}

fn banner_png() -> Vec<u8> {
    include_bytes!("fixtures/banner.png").to_vec()
}

async fn add_game(pool: &PgPool, id: &str, name: &str, appid: Option<i32>) {
    sqlx::query("INSERT INTO games (id, name, steam_appid, verified) VALUES ($1, $2, $3, true)")
        .bind(id)
        .bind(name)
        .bind(appid)
        .execute(pool)
        .await
        .unwrap();
}

fn empty_sigs() -> fraglink_core::signatures::SignatureDb {
    fraglink_core::signatures::SignatureDb::from_json(r#"{"schema":1,"version":"0","games":[]}"#)
        .unwrap()
}

async fn set_role(pool: &PgPool, handle: &str, role: Option<&str>) {
    sqlx::query("UPDATE users SET role = $2 WHERE handle = $1")
        .bind(handle)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn seed_queue(pool: &PgPool, n: i32, due_at: i32) {
    sqlx::query("DELETE FROM art_backfill_queue")
        .execute(pool)
        .await
        .unwrap();
    let mut ids = Vec::with_capacity(n as usize);
    let mut dues = Vec::with_capacity(n as usize);
    for i in 0..n {
        ids.push(format!("g{i}"));
        if i == due_at {
            dues.push("-infinity".to_string());
        } else {
            dues.push("infinity".to_string());
        }
    }
    sqlx::query(
        "INSERT INTO art_backfill_queue (game_id, state, next_due_at)
         SELECT game_id, 'pending', due::timestamptz
         FROM unnest($1::text[], $2::text[]) AS t(game_id, due)",
    )
    .bind(&ids)
    .bind(&dues)
    .execute(pool)
    .await
    .unwrap();
}

async fn wait_status(app: &axum::Router, token: &str, want: &str) -> serde_json::Value {
    let mut last = serde_json::Value::Null;
    for _ in 0..200 {
        let r = api(
            app,
            "GET",
            "/api/v1/admin/artwork/backfill",
            Some(token),
            None,
        )
        .await;
        last = r.json();
        if last["status"].as_str() == Some(want) {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for status {want}: {last}");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn pick_query_count_is_constant_regardless_of_queue_size(pool: PgPool) {
    let st = state(pool.clone());

    seed_queue(&pool, 50, 40).await;
    let _ = pick_due(&st).await.unwrap();

    reset_pick_sql_count();
    seed_queue(&pool, 50, 40).await;
    let before_small = pick_sql_count();
    let small = pick_due(&st).await.unwrap();
    let small_queries = pick_sql_count() - before_small;

    reset_pick_sql_count();
    seed_queue(&pool, 10_000, 9_000).await;
    let before_large = pick_sql_count();
    let large = pick_due(&st).await.unwrap();
    let large_queries = pick_sql_count() - before_large;

    assert_eq!(small.as_deref(), Some("g40"));
    assert_eq!(large.as_deref(), Some("g9000"));
    assert_eq!(small_queries, 1, "small queue should be one pick SQL");
    assert_eq!(
        large_queries, small_queries,
        "a 10,000-row queue must not issue more pick queries than a 50-row queue"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn new_state_does_not_start_a_run_and_boot_marks_it_stopped(pool: PgPool) {
    sqlx::query(
        "INSERT INTO art_backfill_run (status, started_by, remaining)
         VALUES ('running', 'old', 12)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO art_backfill_queue (game_id, state, next_due_at)
         VALUES ('left_over', 'pending', '-infinity')",
    )
    .execute(&pool)
    .await
    .unwrap();

    let st = state(pool.clone());
    tokio::time::sleep(Duration::from_millis(80)).await;
    let mid = status(&st).await.unwrap();
    assert_eq!(
        mid["worker_alive"], false,
        "AppState::new must not spawn a backfill worker: {mid}"
    );
    let before = status(&st).await.unwrap();
    assert_eq!(before["status"], "running");
    assert_eq!(before["can_resume"], false);

    pause_on_boot(&st).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    let after = status(&st).await.unwrap();
    assert_eq!(after["status"], "stopped");
    assert_eq!(after["remaining"], 12);
    assert_eq!(after["can_resume"], true);
    assert_eq!(after["can_start"], true);
    assert_eq!(after["worker_alive"], false);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn staff_start_stops_and_resumes_one_run(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(FOUND, banner_png());
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "bf_found", "Backfill Found", Some(FOUND as i32)).await;
    add_game(&pool, "bf_miss", "Backfill Miss", Some(MISS as i32)).await;
    add_game(&pool, "manual:hidden", "Hidden Manual", Some(9_200_099)).await;

    let mut st = state_with(pool.clone(), {
        let data = dir.path().to_path_buf();
        let base = cdn.base.clone();
        move |c| {
            c.data_dir = data;
            c.steam_art_cdn = format!("{base}/apps");
            c.art_backfill = true;
            c.art_backfill_every_secs = 30;
        }
    });
    st.sigs = std::sync::Arc::new(empty_sigs());
    st.games.install_empty();
    let app = fraglink_server::router(st.clone());
    let admin = signup(&app, "bf-admin@example.com", "bfadmin").await;
    set_role(&pool, "bfadmin", Some("admin")).await;
    let user = signup(&app, "bf-user@example.com", "bfuser").await;
    let moder = signup(&app, "bf-mod@example.com", "bfmod").await;
    set_role(&pool, "bfmod", Some("moderator")).await;

    let r = api(
        &app,
        "GET",
        "/api/v1/admin/artwork/backfill",
        Some(&user),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&moder),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.text);

    let r = api(&app, "GET", "/api/v1/admin/artwork", Some(&admin), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["backfill"]["status"], "idle");
    assert_eq!(r.json()["backfill"]["can_start"], true);

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let started = r.json();
    assert_eq!(started["status"], "running", "{started}");
    assert_eq!(started["remaining"], 2, "{started}");
    // Only catalogue ids: the manual: row is not queued.
    let queued: Vec<String> = sqlx::query_scalar("SELECT game_id FROM art_backfill_queue")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        !queued.iter().any(|id| id.starts_with("manual:")),
        "{queued:?}"
    );

    // A second start while the first is running must not open another run.
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["status"], "running");
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM art_backfill_run")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(runs, 1, "only one run at a time");

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "stop" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let stopped = wait_status(&app, &admin, "stopped").await;
    assert!(stopped["remaining"].as_i64().unwrap() >= 1, "{stopped}");
    assert_eq!(stopped["can_resume"], true);
    assert_eq!(stopped["can_stop"], false);
    assert_eq!(stopped["worker_alive"], false);

    // Resume continues the same queue (does not require a rebuild).
    let pending_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM art_backfill_queue WHERE state = 'pending'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "resume" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["status"], "running", "{}", r.text);
    let pending_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM art_backfill_queue WHERE state = 'pending'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        pending_after, pending_before,
        "resume must not rebuild the queue"
    );

    let _ = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "stop" })),
    )
    .await;
    let _ = wait_status(&app, &admin, "stopped").await;
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn start_fills_missing_catalogue_games_and_skips_fresh_misses(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(FOUND, banner_png());
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "bf2_found", "Found Two", Some(FOUND as i32)).await;
    add_game(&pool, "bf2_miss", "Miss Two", Some(MISS as i32)).await;
    add_game(&pool, "bf2_fresh", "Fresh Miss", Some(9_200_003)).await;

    let data = dir.path().to_path_buf();
    // A fresh .miss is not part of this pass (it would only be due after the ttl).
    std::fs::create_dir_all(data.join("art").join("g")).unwrap();
    let fresh_key = fraglink_server::art::cache_key("bf2_fresh");
    std::fs::write(
        data.join("art").join("g").join(format!("{fresh_key}.miss")),
        b"",
    )
    .unwrap();

    let mut st = state_with(pool.clone(), {
        let data = data.clone();
        let base = cdn.base.clone();
        move |c| {
            c.data_dir = data;
            c.steam_art_cdn = format!("{base}/apps");
            c.art_backfill = true;
            c.art_backfill_every_secs = 0;
        }
    });
    st.sigs = std::sync::Arc::new(empty_sigs());
    st.games.install_empty();
    let app = fraglink_server::router(st);
    let admin = signup(&app, "bf2-admin@example.com", "bf2admin").await;
    set_role(&pool, "bf2admin", Some("admin")).await;

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let done = wait_status(&app, &admin, "finished").await;
    assert_eq!(done["found"], 1, "{done}");
    assert_eq!(done["failed"], 1, "{done}");
    assert_eq!(done["done"], 2, "{done}");
    assert_eq!(done["remaining"], 0, "{done}");

    let queued: Vec<(String, String)> =
        sqlx::query_as("SELECT game_id, state FROM art_backfill_queue ORDER BY game_id")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(
        !queued.iter().any(|(id, _)| id == "bf2_fresh"),
        "fresh misses stay out of this pass: {queued:?}"
    );

    // A second start while the failed game still has a fresh .miss should skip it.
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let again = wait_status(&app, &admin, "finished").await;
    assert_eq!(
        again["done"], 0,
        "a fresh miss must not be retried in a tight loop: {again}"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn off_switch_blocks_start(pool: PgPool) {
    add_game(&pool, "bf_off", "Off Game", Some(FOUND as i32)).await;
    let mut st = state_with(pool.clone(), |c| {
        c.art_backfill = false;
        c.art_backfill_every_secs = 0;
    });
    st.sigs = std::sync::Arc::new(empty_sigs());
    st.games.install_empty();
    let app = fraglink_server::router(st);
    let admin = signup(&app, "bf-off@example.com", "bfoff").await;
    set_role(&pool, "bfoff", Some("admin")).await;

    let r = api(
        &app,
        "GET",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["enabled"], false);
    assert_eq!(r.json()["can_start"], false);

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/backfill",
        Some(&admin),
        Some(json!({ "action": "start" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert_eq!(r.json()["error"], "backfill_off");
    let runs: i64 = sqlx::query_scalar("SELECT count(*) FROM art_backfill_run")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(runs, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn evicts_old_banners_when_over_the_disk_cap(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let d = data.join("art").join("g");
    std::fs::create_dir_all(&d).unwrap();
    let a = d.join("aaaa.jpg");
    let b = d.join("bbbb.jpg");
    std::fs::write(&a, vec![0; 80]).unwrap();
    std::fs::write(&b, vec![0; 80]).unwrap();
    let old = std::time::SystemTime::now() - Duration::from_secs(60);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&a)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let st = state_with(pool, {
        let data = data.clone();
        move |c| {
            c.data_dir = data;
            c.art_cache_bytes = 80;
        }
    });
    fraglink_server::art::evict_banners(&st, Some(&b));
    assert!(!a.exists(), "oldest banner should be evicted");
    assert!(b.exists(), "kept banner must stay under the cap");
}
