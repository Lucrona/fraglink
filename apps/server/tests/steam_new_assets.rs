//! Newer Steam apps have no `<appid>/header.jpg` or legacy community icon path: banners
//! fall back to the `header_image` the game page warmer stored (Steam image hosts only),
//! icons to the community_assets host, staff uploads still win, and history entries
//! recorded without a game name show the name instead of the raw id.
mod common;
use axum::body::Body;
use axum::extract::Path;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

const NUMERIC: u32 = 9_100_001;
const UNTRUSTED: u32 = 9_100_002;
const GAME_ART: u32 = 9_100_003;
const OVERRIDDEN: u32 = 9_100_004;
const ICON: u32 = 9_100_005;
const CUSTOM_ICON: u32 = 9_100_006;
const HASH: &str = "ca56095c6c464caf846590898beba429d0f75fe9";

fn header_jpeg() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(460, 215, image::Rgb([200, 40, 40]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
    buf.into_inner()
}

fn png(px: u32, shade: u8) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(px, px, image::Rgba([shade, 30, 40, 255]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

#[derive(Clone, Default)]
struct Hits {
    legacy: Arc<AtomicUsize>,
    stored: Arc<AtomicUsize>,
    community: Arc<AtomicUsize>,
}
impl Hits {
    fn get(c: &Arc<AtomicUsize>) -> usize {
        c.load(Ordering::SeqCst)
    }
}

/// Steam-like upstream: legacy paths 404, hashed store assets and community_assets work.
async fn start_steam() -> (String, Hits) {
    let hits = Hits::default();
    let (h1, h2, h3) = (hits.clone(), hits.clone(), hits.clone());
    let up = axum::Router::new()
        .route(
            "/apps/{appid}/{file}",
            get(move |Path((_a, _f)): Path<(u32, String)>| {
                let h = h1.clone();
                async move {
                    h.legacy.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::NOT_FOUND, Vec::<u8>::new())
                }
            }),
        )
        .route(
            "/store_item_assets/steam/apps/{appid}/{hash}/header.jpg",
            get(move |Path((_a, _h)): Path<(u32, String)>| {
                let h = h2.clone();
                async move {
                    h.stored.fetch_add(1, Ordering::SeqCst);
                    (StatusCode::OK, header_jpeg())
                }
            }),
        )
        .route(
            "/community_assets/{appid}/{file}",
            get(move |Path((_a, file)): Path<(u32, String)>| {
                let h = h3.clone();
                async move {
                    h.community.fetch_add(1, Ordering::SeqCst);
                    if file == format!("{HASH}.jpg") {
                        (StatusCode::OK, png(128, 90))
                    } else {
                        (StatusCode::NOT_FOUND, vec![])
                    }
                }
            }),
        )
        .route(
            "/IStoreBrowseService/GetItems/v1/",
            get(|| async {
                axum::Json(
                    json!({"response":{"store_items":[{"assets":{"community_icon": HASH}}]}}),
                )
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, up).await.unwrap() });
    (base, hits)
}

fn steam_state(pool: PgPool, data: std::path::PathBuf, base: &str) -> fraglink_server::AppState {
    let base = base.to_string();
    state_with(pool, move |c| {
        c.data_dir = data;
        c.steam_art_cdn = format!("{base}/apps");
        c.steam_cdn_url = format!("{base}/apps");
        c.steam_community_assets_url = format!("{base}/community_assets");
        c.steam_api_base = base.clone();
        c.app_icon_cdn_url = base.clone();
    })
}

async fn add_game(pool: &PgPool, id: &str, name: &str, appid: u32) {
    sqlx::query(
        "INSERT INTO games (id, name, steam_appid, steam_icon_hash, verified)
         VALUES ($1, $2, $3, $4, true)",
    )
    .bind(id)
    .bind(name)
    .bind(appid as i32)
    .bind(HASH)
    .execute(pool)
    .await
    .unwrap();
}

async fn store_header(pool: &PgPool, id: &str, appid: u32, url: &str) {
    sqlx::query(
        "INSERT INTO game_feed_cache (game_id, steam_appid, details_json, details_fetched_at, details_ok)
         VALUES ($1, $2, $3, now(), true)",
    )
    .bind(id)
    .bind(appid as i32)
    .bind(json!({ "name": id, "header_image": url }))
    .execute(pool)
    .await
    .unwrap();
}

async fn get_path(app: &axum::Router, path: &str) -> Resp {
    send(
        app,
        Request::builder()
            .uri(path)
            .header("x-forwarded-for", "203.0.113.50")
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn numeric_art_falls_back_to_the_stored_header_on_steam_hosts_only(pool: PgPool) {
    let (base, hits) = start_steam().await;
    // a second server standing in for any non-Steam host
    let (other, other_hits) = start_steam().await;
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "new_game", "New Game", NUMERIC).await;
    add_game(&pool, "odd_game", "Odd Game", UNTRUSTED).await;
    store_header(
        &pool,
        "new_game",
        NUMERIC,
        &format!("{base}/store_item_assets/steam/apps/{NUMERIC}/2a6f/header.jpg?t=1763"),
    )
    .await;
    store_header(
        &pool,
        "odd_game",
        UNTRUSTED,
        &format!("{other}/store_item_assets/steam/apps/{UNTRUSTED}/2a6f/header.jpg"),
    )
    .await;
    let app = fraglink_server::router(steam_state(pool, dir.path().to_path_buf(), &base));

    let r = get_path(&app, &format!("/api/v1/art/{NUMERIC}.jpg")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.bytes.starts_with(&[0xff, 0xd8]));
    assert!(
        Hits::get(&hits.legacy) >= 1,
        "old header.jpg is still tried first"
    );
    assert_eq!(Hits::get(&hits.stored), 1);
    assert!(dir.path().join(format!("art/{NUMERIC}.jpg")).exists());

    // stored URL on a host that isn't Steam's: never fetched, recorded as a miss
    let r = get_path(&app, &format!("/api/v1/art/{UNTRUSTED}.jpg")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        Hits::get(&other_hits.stored),
        0,
        "non-Steam host must not be fetched"
    );
    assert!(dir.path().join(format!("art/{UNTRUSTED}.miss")).exists());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn game_art_uses_the_stored_header_but_a_custom_banner_wins(pool: PgPool) {
    let (base, hits) = start_steam().await;
    let dir = tempfile::tempdir().unwrap();
    for (appid, name) in [(GAME_ART, "Art Game"), (OVERRIDDEN, "Custom Game")] {
        let id = format!("steam:{appid}");
        add_game(&pool, &id, name, appid).await;
        store_header(
            &pool,
            &id,
            appid,
            &format!("{base}/store_item_assets/steam/apps/{appid}/9f/header.jpg"),
        )
        .await;
    }
    let st = steam_state(pool, dir.path().to_path_buf(), &base);
    // staff uploaded a banner for this one
    let custom = {
        let img = image::RgbImage::from_pixel(460, 215, image::Rgb([10, 200, 10]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
        buf.into_inner()
    };
    fraglink_server::art::set_override(&st, &format!("steam:{OVERRIDDEN}"), &custom).unwrap();
    let saved = std::fs::read(fraglink_server::art::override_path(
        &st,
        &format!("steam:{OVERRIDDEN}"),
    ))
    .unwrap();
    let app = fraglink_server::router(st);

    let r = get_path(&app, &format!("/api/v1/game-art/steam%3A{GAME_ART}.jpg")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (460, 215));
    assert_eq!(Hits::get(&hits.stored), 1);

    let r = get_path(&app, &format!("/api/v1/game-art/steam%3A{OVERRIDDEN}.jpg")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.bytes, saved, "custom banner is served");
    assert_eq!(
        Hits::get(&hits.stored),
        1,
        "no Steam fetch when a custom banner exists"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn icons_fall_back_to_community_assets_but_a_custom_icon_wins(pool: PgPool) {
    let (base, hits) = start_steam().await;
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, &format!("steam:{ICON}"), "Icon Game", ICON).await;
    add_game(
        &pool,
        &format!("steam:{CUSTOM_ICON}"),
        "Custom Icon",
        CUSTOM_ICON,
    )
    .await;
    let st = steam_state(pool, dir.path().to_path_buf(), &base);
    let custom_dir = fraglink_server::game_overrides::custom_icon_dir(&st);
    std::fs::create_dir_all(&custom_dir).unwrap();
    let k = fraglink_server::icons::key(&format!("steam:{CUSTOM_ICON}"));
    let custom = png(32, 250);
    std::fs::write(custom_dir.join(format!("{k}-32.png")), &custom).unwrap();
    let app = fraglink_server::router(st);

    let r = get_path(&app, &format!("/icons/games/32/steam%3A{ICON}.png")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (32, 32));
    assert!(
        Hits::get(&hits.legacy) >= 2,
        "legacy full and small tried first"
    );
    assert_eq!(Hits::get(&hits.community), 1);

    let before = Hits::get(&hits.community) + Hits::get(&hits.legacy);
    let r = get_path(&app, &format!("/icons/games/32/steam%3A{CUSTOM_ICON}.png")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.bytes, custom, "custom icon is served");
    assert_eq!(
        Hits::get(&hits.community) + Hits::get(&hits.legacy),
        before,
        "no Steam fetch when a custom icon exists"
    );
}

async fn set_admin(pool: &PgPool, handle: &str) {
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = $1")
        .bind(handle)
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn history_shows_names_for_entries_recorded_without_one(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    add_game(&pool, "steam:4705510", "Happy Wheels", 4705510).await;
    add_game(&pool, "steam:4705511", "Secret Game", 4705511).await;
    sqlx::query("INSERT INTO game_overrides (game_id, hidden) VALUES ('steam:4705511', true)")
        .execute(&pool)
        .await
        .unwrap();
    // rows as older builds stored them: no name, raw id in the summary
    for id in ["steam:4705510", "steam:4705511"] {
        sqlx::query(
            "INSERT INTO game_catalogue_history
               (actor_kind, actor_label, action, game_id, game_name, summary, public)
             VALUES ('admin', 'old', 'icon', $1, NULL, $2, true)",
        )
        .bind(id)
        .bind(format!("Updated icon for {id}"))
        .execute(&pool)
        .await
        .unwrap();
    }
    let app = fraglink_server::router(state_with(pool.clone(), |c| {
        c.data_dir = dir.path().to_path_buf()
    }));

    let ups = api(&app, "GET", "/api/v1/games/updates", None, None)
        .await
        .json();
    let find = |v: &Value, id: &str| -> Value {
        v["updates"]
            .as_array()
            .or(v["entries"].as_array())
            .unwrap()
            .iter()
            .find(|u| u["game_id"] == id)
            .cloned()
            .unwrap_or(Value::Null)
    };
    let hw = find(&ups, "steam:4705510");
    assert_eq!(hw["game_name"], "Happy Wheels");
    assert_eq!(hw["summary"], "Updated icon for Happy Wheels");
    // hidden games keep the id: their name isn't public
    let hidden = find(&ups, "steam:4705511");
    if !hidden.is_null() {
        assert!(hidden["game_name"].is_null());
        assert_eq!(hidden["summary"], "Updated icon for steam:4705511");
    }
    // read-time only: stored rows are untouched
    let raw: Vec<(Option<String>, String)> = sqlx::query_as(
        "SELECT game_name, summary FROM game_catalogue_history
         WHERE game_id = 'steam:4705510' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        raw,
        vec![(None, "Updated icon for steam:4705510".to_string())]
    );

    // admin list gets the same fill
    let admin = signup(&app, "h@example.com", "histadmin").await;
    set_admin(&pool, "histadmin").await;
    let list = api(
        &app,
        "GET",
        "/api/v1/admin/games-history?game=steam%3A4705510",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text);
    let list = list.json();
    let e = find(&list, "steam:4705510");
    assert_eq!(e["game_name"], "Happy Wheels");
    assert_eq!(e["summary"], "Updated icon for Happy Wheels");

    // new entries are recorded with the name
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/admin/games/steam%3A4705510/icon")
        .header("authorization", format!("Bearer {admin}"))
        .header("content-type", "image/png")
        .body(Body::from(png(32, 70)))
        .unwrap();
    let r = send(&app, req).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let (name, summary): (Option<String>, String) = sqlx::query_as(
        "SELECT game_name, summary FROM game_catalogue_history ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(name.as_deref(), Some("Happy Wheels"));
    assert_eq!(summary, "Updated icon for Happy Wheels");
}
