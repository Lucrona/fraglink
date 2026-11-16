//! `/api/v1/art/<appid>.jpg`: only catalogue/games-table app ids are fetched or
//! cached; cache size is capped with LRU-by-mtime eviction; failure markers expire;
//! cache misses are rate-limited per IP. Cached paths stay `{data_dir}/art/{appid}.jpg`.
mod common;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::routing::get;
use common::*;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const UNKNOWN: u32 = 9_000_001;
const TABLE: u32 = 9_000_002;
const PREFIX: u32 = 9_000_003;
const HIDDEN: u32 = 9_000_004;
const EVICT_A: u32 = 9_000_011;
const EVICT_B: u32 = 9_000_012;
const EVICT_C: u32 = 9_000_013;
const MISS_A: u32 = 9_000_021;
const MISS_B: u32 = 9_000_022;
const MISS_C: u32 = 9_000_023;
const BIG: u32 = 9_000_031;
const LATE: u32 = 9_000_041;

fn jpeg(len: usize) -> Vec<u8> {
    let mut b = vec![0u8; len.max(2)];
    b[0] = 0xff;
    b[1] = 0xd8;
    b
}

struct Cdn {
    hits: Arc<AtomicUsize>,
    bodies: Arc<Mutex<HashMap<u32, Vec<u8>>>>,
    base: String,
}

async fn start_cdn() -> Cdn {
    let hits = Arc::new(AtomicUsize::new(0));
    let bodies: Arc<Mutex<HashMap<u32, Vec<u8>>>> = Arc::new(Mutex::new(HashMap::new()));
    let (h, b) = (hits.clone(), bodies.clone());
    let up = axum::Router::new().route(
        "/apps/{appid}/header.jpg",
        get(
            move |axum::extract::Path(appid): axum::extract::Path<u32>| {
                let h = h.clone();
                let b = b.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
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
    Cdn { hits, bodies, base }
}

impl Cdn {
    fn put(&self, appid: u32, body: Vec<u8>) {
        self.bodies.lock().unwrap().insert(appid, body);
    }
    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
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

async fn art_get(app: &axum::Router, appid: u32, ip: Option<&str>) -> Resp {
    let mut b = Request::builder().uri(format!("/api/v1/art/{appid}.jpg"));
    if let Some(ip) = ip {
        b = b.header("x-forwarded-for", ip);
    }
    send(app, b.body(Body::empty()).unwrap()).await
}

fn art_state(
    pool: PgPool,
    data: std::path::PathBuf,
    cdn: &str,
    f: impl FnOnce(&mut fraglink_server::Config),
) -> fraglink_server::AppState {
    let cdn = cdn.to_string();
    state_with(pool, move |c| {
        c.data_dir = data;
        c.steam_art_cdn = format!("{cdn}/apps");
        f(c);
    })
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn unknown_appid_is_404_without_touching_steam_or_disk(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(UNKNOWN, jpeg(80));
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("art");
    std::fs::create_dir_all(&art).unwrap();
    let leftover = art.join(format!("{UNKNOWN}.jpg"));
    std::fs::write(&leftover, jpeg(80)).unwrap();
    let before = std::fs::metadata(&leftover).unwrap().modified().unwrap();
    add_game(&pool, "ok_game", "Ok Game", TABLE as i32).await;
    let st = art_state(pool, dir.path().to_path_buf(), &cdn.base, |_| {});
    let app = fraglink_server::router(st);
    let r = art_get(&app, UNKNOWN, Some("203.0.113.10")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);
    assert_eq!(cdn.hits(), 0, "must not fetch Steam for an unknown app id");
    assert!(leftover.exists(), "must not touch leftover cache files");
    assert!(!art.join(format!("{UNKNOWN}.miss")).exists());
    let after = std::fs::metadata(&leftover).unwrap().modified().unwrap();
    assert_eq!(after, before);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn allowed_ids_are_fetched_and_cached_at_the_existing_path(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(TABLE, jpeg(80));
    cdn.put(PREFIX, jpeg(90));
    cdn.put(HIDDEN, jpeg(70));
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "table_game", "Table Game", TABLE as i32).await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ($1, $2, true)")
        .bind(format!("steam:{PREFIX}"))
        .bind("Prefix Game")
        .execute(&pool)
        .await
        .unwrap();
    add_game(&pool, "hidden_game", "Hidden Game", HIDDEN as i32).await;
    sqlx::query("INSERT INTO game_overrides (game_id, hidden) VALUES ('hidden_game', true)")
        .execute(&pool)
        .await
        .unwrap();
    let st = art_state(pool.clone(), dir.path().to_path_buf(), &cdn.base, |_| {});
    let app = fraglink_server::router(st);

    for (id, len) in [(TABLE, 80usize), (PREFIX, 90), (HIDDEN, 70)] {
        let r = art_get(&app, id, Some("203.0.113.11")).await;
        assert_eq!(r.status, StatusCode::OK, "{id}: {}", r.text);
        assert_eq!(r.headers[header::CONTENT_TYPE], "image/jpeg");
        assert_eq!(r.bytes.len(), len);
        let cached = dir.path().join("art").join(format!("{id}.jpg"));
        assert_eq!(
            std::fs::read(&cached).unwrap(),
            r.bytes,
            "path must stay art/{{appid}}.jpg"
        );
    }
    let after_fetch = cdn.hits();
    assert_eq!(after_fetch, 3);
    let r = art_get(&app, TABLE, Some("203.0.113.11")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(cdn.hits(), after_fetch, "cache hit must not hit Steam");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn allow_set_is_refreshed_when_the_catalogue_is_rebuilt(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(TABLE, jpeg(80));
    cdn.put(LATE, jpeg(80));
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "early", "Early", TABLE as i32).await;
    let st = art_state(pool.clone(), dir.path().to_path_buf(), &cdn.base, |_| {});
    let app = fraglink_server::router(st);
    assert_eq!(
        art_get(&app, TABLE, Some("203.0.113.12")).await.status,
        StatusCode::OK
    );
    add_game(&pool, "late", "Late", LATE as i32).await;
    assert_eq!(
        art_get(&app, LATE, Some("203.0.113.12")).await.status,
        StatusCode::NOT_FOUND,
        "stale allow set should not see a row added after the last catalogue build"
    );
    assert_eq!(cdn.hits(), 1);
    let cat = api(&app, "GET", "/api/v1/games/catalogue", None, None).await;
    assert_eq!(cat.status, StatusCode::OK, "{}", cat.text);
    let r = art_get(&app, LATE, Some("203.0.113.12")).await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "catalogue rebuild refreshes allow set"
    );
    assert_eq!(cdn.hits(), 2);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn fresh_miss_is_honoured_stale_miss_is_expired_and_retried(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(TABLE, jpeg(80));
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("art");
    std::fs::create_dir_all(&art).unwrap();
    add_game(&pool, "table_game", "Table Game", TABLE as i32).await;
    let miss = art.join(format!("{TABLE}.miss"));
    std::fs::write(&miss, b"").unwrap();
    let st = art_state(pool, dir.path().to_path_buf(), &cdn.base, |_| {});
    let app = fraglink_server::router(st);

    let r = art_get(&app, TABLE, Some("203.0.113.13")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(cdn.hits(), 0, "fresh miss must not hit Steam");
    assert!(miss.exists());

    let f = std::fs::File::options().write(true).open(&miss).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(86_400 * 2))
        .unwrap();
    drop(f);
    let r = art_get(&app, TABLE, Some("203.0.113.13")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(cdn.hits(), 1);
    assert!(!miss.exists(), "expired miss marker must be deleted");
    assert!(art.join(format!("{TABLE}.jpg")).exists());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn oversized_body_is_rejected(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(BIG, jpeg(2_000));
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "big", "Big", BIG as i32).await;
    let st = art_state(pool, dir.path().to_path_buf(), &cdn.base, |c| {
        c.art_max_bytes = 1_000;
    });
    let app = fraglink_server::router(st);
    let r = art_get(&app, BIG, Some("203.0.113.14")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(cdn.hits(), 1);
    let art = dir.path().join("art");
    assert!(!art.join(format!("{BIG}.jpg")).exists());
    assert!(art.join(format!("{BIG}.miss")).exists());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn cache_evicts_oldest_jpg_by_mtime_when_over_cap(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(EVICT_C, jpeg(80));
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("art");
    std::fs::create_dir_all(art.join("g")).unwrap();
    std::fs::create_dir_all(art.join("override")).unwrap();
    std::fs::write(art.join("g/keep.jpg"), jpeg(200)).unwrap();
    std::fs::write(art.join("override/1.jpg"), jpeg(200)).unwrap();
    for (id, age) in [(EVICT_A, 30u64), (EVICT_B, 20)] {
        let p = art.join(format!("{id}.jpg"));
        std::fs::write(&p, jpeg(80)).unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age))
            .unwrap();
    }
    add_game(&pool, "a", "A", EVICT_A as i32).await;
    add_game(&pool, "b", "B", EVICT_B as i32).await;
    add_game(&pool, "c", "C", EVICT_C as i32).await;
    let st = art_state(pool, dir.path().to_path_buf(), &cdn.base, |c| {
        c.art_cache_bytes = 160;
    });
    let app = fraglink_server::router(st);
    let r = art_get(&app, EVICT_C, Some("203.0.113.15")).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        !art.join(format!("{EVICT_A}.jpg")).exists(),
        "oldest evicted"
    );
    assert!(art.join(format!("{EVICT_B}.jpg")).exists());
    assert!(art.join(format!("{EVICT_C}.jpg")).exists());
    assert!(art.join("g/keep.jpg").exists(), "art/g must be left alone");
    assert!(
        art.join("override/1.jpg").exists(),
        "art/override must be left alone"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn cache_misses_are_rate_limited_per_ip_hits_are_not(pool: PgPool) {
    let cdn = start_cdn().await;
    cdn.put(TABLE, jpeg(80));
    let dir = tempfile::tempdir().unwrap();
    let art = dir.path().join("art");
    std::fs::create_dir_all(&art).unwrap();
    std::fs::write(art.join(format!("{TABLE}.jpg")), jpeg(80)).unwrap();
    add_game(&pool, "ok", "Ok", TABLE as i32).await;
    add_game(&pool, "ma", "Miss A", MISS_A as i32).await;
    add_game(&pool, "mb", "Miss B", MISS_B as i32).await;
    add_game(&pool, "mc", "Miss C", MISS_C as i32).await;
    let st = art_state(pool, dir.path().to_path_buf(), &cdn.base, |c| {
        c.art_miss_per_min = 2;
    });
    let app = fraglink_server::router(st);

    let hit = art_get(&app, TABLE, Some("203.0.113.16")).await;
    assert_eq!(hit.status, StatusCode::OK);
    assert_eq!(cdn.hits(), 0, "pre-seeded cache must not hit Steam");
    for _ in 0..5 {
        assert_eq!(
            art_get(&app, TABLE, Some("203.0.113.16")).await.status,
            StatusCode::OK,
            "cache hits must not count toward the miss limit"
        );
    }

    assert_eq!(
        art_get(&app, MISS_A, Some("203.0.113.16")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        art_get(&app, MISS_B, Some("203.0.113.16")).await.status,
        StatusCode::NOT_FOUND
    );
    let limited = art_get(&app, MISS_C, Some("203.0.113.16")).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        !dir.path()
            .join("art")
            .join(format!("{MISS_C}.miss"))
            .exists(),
        "a rate-limited miss must not write a failure marker"
    );
    let other = art_get(&app, MISS_C, Some("203.0.113.17")).await;
    assert_eq!(
        other.status,
        StatusCode::NOT_FOUND,
        "a different IP still has budget"
    );
    assert_eq!(
        cdn.hits(),
        3,
        "two misses from the limited IP, plus one from the other IP"
    );
}
