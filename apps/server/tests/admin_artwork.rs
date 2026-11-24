//! Admin missing-artwork list and fetch: same sources as /games, and a retry
//! clears a stale `.miss` so art can be found.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

const FOUND: u32 = 9_100_001;
const MISS: u32 = 9_100_002;

fn art_key(id: &str) -> String {
    match id {
        "banner_found" => "f6f2c5b185b6a279cf8c9181".into(),
        "banner_none" => "0f83d412eda07e20808d3a55".into(),
        other => panic!("unexpected id {other}"),
    }
}

fn banner_png() -> Vec<u8> {
    include_bytes!("fixtures/banner.png").to_vec()
}

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

async fn add_game(pool: &PgPool, id: &str, name: &str, appid: i32) {
    sqlx::query("INSERT INTO games (id, name, steam_appid, verified) VALUES ($1, $2, $3, true)")
        .bind(id)
        .bind(name)
        .bind(appid)
        .execute(pool)
        .await
        .unwrap();
}

async fn set_role(pool: &PgPool, handle: &str, role: Option<&str>) {
    sqlx::query("UPDATE users SET role = $2 WHERE handle = $1")
        .bind(handle)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

fn miss_path(data: &std::path::Path, id: &str) -> std::path::PathBuf {
    data.join("art")
        .join("g")
        .join(format!("{}.miss", art_key(id)))
}
fn jpg_path(data: &std::path::Path, id: &str) -> std::path::PathBuf {
    data.join("art")
        .join("g")
        .join(format!("{}.jpg", art_key(id)))
}

fn row_for<'a>(rows: &'a [serde_json::Value], id: &str) -> Option<&'a serde_json::Value> {
    rows.iter().find(|r| r["id"] == id)
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn missing_artwork_lists_game_art_and_fetch_retries_past_a_miss(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(FOUND, banner_png());
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "banner_found", "Zebra Banner Found", FOUND as i32).await;
    add_game(&pool, "banner_none", "Zebra Banner None", MISS as i32).await;
    let data = dir.path().to_path_buf();
    let st = state_with(pool.clone(), {
        let data = data.clone();
        let base = cdn.base.clone();
        move |c| {
            c.data_dir = data;
            c.steam_art_cdn = format!("{base}/apps");
        }
    });
    let app = fraglink_server::router(st);
    let owner = signup(&app, "art-admin@example.com", "artadmin").await;
    set_role(&pool, "artadmin", Some("admin")).await;
    let user = signup(&app, "art-user@example.com", "artuser").await;

    let r = api(&app, "GET", "/api/v1/admin/artwork", Some(&user), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);

    let r = api(
        &app,
        "GET",
        "/api/v1/admin/artwork?q=banner_",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let d = r.json();
    let rows = d["rows"].as_array().cloned().unwrap_or_default();
    let found = row_for(&rows, "banner_found").expect("banner_found should be listed");
    let none = row_for(&rows, "banner_none").expect("banner_none should be listed");
    assert_eq!(found["status"], "not_cached");
    assert_eq!(none["status"], "not_cached");
    assert!(
        d["note"].as_str().unwrap_or("").contains("/games"),
        "{}",
        d["note"]
    );

    // A fresh .miss would block /games and used to leave this page stuck.
    std::fs::create_dir_all(data.join("art").join("g")).unwrap();
    std::fs::write(miss_path(&data, "banner_found"), b"").unwrap();
    std::fs::write(miss_path(&data, "banner_none"), b"").unwrap();
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/artwork?q=banner_",
        Some(&owner),
        None,
    )
    .await;
    let rows = r.json()["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        row_for(&rows, "banner_found").unwrap()["status"],
        "unavailable"
    );
    assert_eq!(
        row_for(&rows, "banner_none").unwrap()["status"],
        "unavailable"
    );

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/fetch",
        Some(&owner),
        Some(json!({ "ids": ["banner_found", "banner_none"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let d = r.json();
    assert_eq!(d["fetched"], 1, "{}", r.text);
    assert_eq!(d["still_missing"], 1, "{}", r.text);
    let out = d["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(row_for(&out, "banner_found").unwrap()["found"], true);
    assert_eq!(row_for(&out, "banner_none").unwrap()["found"], false);
    assert!(
        !miss_path(&data, "banner_found").exists(),
        "found art must drop the miss"
    );
    assert!(jpg_path(&data, "banner_found").is_file());
    assert!(
        miss_path(&data, "banner_none").is_file(),
        "a failed retry still records a miss"
    );

    let r = api(
        &app,
        "GET",
        "/api/v1/admin/artwork?q=banner_",
        Some(&owner),
        None,
    )
    .await;
    let rows = r.json()["rows"].as_array().cloned().unwrap_or_default();
    assert!(
        row_for(&rows, "banner_found").is_none(),
        "games with a banner drop off the list: {rows:?}"
    );
    assert_eq!(
        row_for(&rows, "banner_none").unwrap()["status"],
        "unavailable"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn artwork_fetch_is_admin_only_and_rejects_empty_or_huge_batches(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let admin = signup(&app, "a2@example.com", "artlead").await;
    set_role(&pool, "artlead", Some("admin")).await;
    let moder = signup(&app, "m2@example.com", "artmod").await;
    set_role(&pool, "artmod", Some("moderator")).await;

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/fetch",
        Some(&moder),
        Some(json!({ "ids": ["wow"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.text);

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/fetch",
        Some(&admin),
        Some(json!({ "ids": [] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);

    let too_many: Vec<String> = (0..51).map(|i| format!("g{i}")).collect();
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/artwork/fetch",
        Some(&admin),
        Some(json!({ "ids": too_many })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
}
