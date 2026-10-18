//! Official game icons: fetched from the upstream CDNs once, cached on disk in 16/32/64 px,
//! served from our own domain with an ETag; nothing for unknown games.
mod common;
use axum::extract::Path;
use axum::http::{header, StatusCode};
use axum::routing::get;
use common::*;
use sqlx::PgPool;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

fn png() -> Vec<u8> {
    let img = fraglink_core::icons::placeholder("Star Citizen");
    let img = image::imageops::resize(&img, 128, 128, image::imageops::FilterType::Nearest);
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
    buf.into_inner()
}

const RAW: &str = r#"[
 {"id":"452295596917784577","name":"Star Citizen","icon_hash":"7a55868f259fca03ba55756e41983e44","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]},
 {"id":"1234821572246896730","name":"Gray Zone Warfare","third_party_skus":[{"distributor":"steam","id":"2479810"}],"executables":[{"name":"gzwclientsteam-win64-shipping.exe","os":"win32","is_launcher":false}]},
 {"id":"999","name":"No Icon Game","icon_hash":"deadbeef","executables":[{"name":"noicon.exe","os":"win32","is_launcher":false}]}
]"#;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn official_icons_are_cached_and_served(pool: PgPool) {
    // upstream mock: application icons, Steam store API and Steam community icons
    let hits = Arc::new(AtomicUsize::new(0));
    let (h1, h2, h3) = (hits.clone(), hits.clone(), hits.clone());
    let up = axum::Router::new()
        .route(
            "/app-icons/{app}/{file}",
            get(move |Path((app, file)): Path<(String, String)>| {
                let h = h1.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    if app == "452295596917784577" && file == "7a55868f259fca03ba55756e41983e44.png" {
                        (StatusCode::OK, png())
                    } else {
                        (StatusCode::NOT_FOUND, vec![])
                    }
                }
            }),
        )
        .route(
            "/IStoreBrowseService/GetItems/v1/",
            get(move || {
                let h = h2.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    axum::Json(serde_json::json!({"response":{"store_items":[{"appid":2479810,"assets":{"community_icon":"ca56095c6c464caf846590898beba429d0f75fe9"}}]}}))
                }
            }),
        )
        .route(
            "/apps/{appid}/{file}",
            get(move |Path((appid, file)): Path<(u32, String)>| {
                let h = h3.clone();
                async move {
                    h.fetch_add(1, Ordering::SeqCst);
                    if appid == 2479810 && file == "ca56095c6c464caf846590898beba429d0f75fe9_full.jpg" {
                        (StatusCode::OK, png())
                    } else {
                        (StatusCode::NOT_FOUND, vec![])
                    }
                }
            }),
        );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, up).await.unwrap() });

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("detectable")).unwrap();
    std::fs::write(dir.path().join("detectable/discord.json"), RAW).unwrap();
    let data = dir.path().to_path_buf();
    let b2 = base.clone();
    let st = state_with(pool, move |c| {
        c.data_dir = data;
        c.app_icon_cdn_url = b2.clone();
        c.steam_api_base = b2.clone();
        c.steam_cdn_url = format!("{b2}/apps");
    });
    let built = fraglink_server::games::build_from_cache(dir.path(), &st.sigs).unwrap();
    st.games.install(built);
    let app = fraglink_server::router(st);

    // the list's own icon, three sizes, cached after the first fetch
    let r = page(&app, "/icons/games/32/star_citizen.png", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.headers[header::CONTENT_TYPE], "image/png");
    // short cache + ETag so admin icon changes show within minutes
    assert!(r.headers[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .contains("max-age=300"));
    let img = image::load_from_memory(&r.bytes).unwrap();
    assert_eq!((img.width(), img.height()), (32, 32));
    let after_first = hits.load(Ordering::SeqCst);
    assert_eq!(after_first, 1);
    for px in [16u32, 64] {
        let r = page(&app, &format!("/icons/games/{px}/star_citizen.png"), None).await;
        assert_eq!(r.status, StatusCode::OK);
        assert_eq!(image::load_from_memory(&r.bytes).unwrap().width(), px);
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        after_first,
        "served from the cache"
    );
    let etag = r.headers[header::ETAG].to_str().unwrap().to_string();
    let r304 = send(
        &app,
        axum::http::Request::get("/icons/games/32/star_citizen.png")
            .header("if-none-match", &etag)
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(r304.status, StatusCode::NOT_MODIFIED);

    // a Steam game with no list icon: Steam's community icon via the store API
    let r = page(&app, "/icons/games/32/steam%3A2479810.png", None).await;
    assert_eq!(r.status, StatusCode::OK, "steam fallback");
    let r = page(&app, "/icons/games/64/gray_zone_warfare.png", None).await;
    assert_eq!(r.status, StatusCode::OK);

    // unknown games, odd sizes and broken upstream icons: 404 (and not retried for a day)
    for p in [
        "/icons/games/32/not_a_game.png",
        "/icons/games/48/star_citizen.png",
        "/icons/games/32/manual%3Ax.png",
        "/icons/games/32/no_icon_game.png",
    ] {
        assert_eq!(
            page(&app, p, None).await.status,
            StatusCode::NOT_FOUND,
            "{p}"
        );
    }
    let n = hits.load(Ordering::SeqCst);
    assert_eq!(
        page(&app, "/icons/games/32/no_icon_game.png", None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(hits.load(Ordering::SeqCst), n, "misses are remembered");

    // the app's lookup: our own paths only, never a third-party URL
    let t = signup(&app, "i@example.com", "icons").await;
    let v = api(
        &app,
        "GET",
        "/api/v1/games/icons?ids=star_citizen,not_a_game,manual:x,steam:2479810",
        Some(&t),
        None,
    )
    .await
    .json();
    assert_eq!(
        v["star_citizen"]["icon_path"],
        "/icons/games/32/star_citizen.png"
    );
    assert!(v["star_citizen"]["icon_url"].is_null());
    assert_eq!(
        v["steam:2479810"]["icon_path"],
        "/icons/games/32/steam%3A2479810.png"
    );
    assert!(v.get("not_a_game").is_none() && v.get("manual:x").is_none());
}
