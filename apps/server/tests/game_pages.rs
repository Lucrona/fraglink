//! Public `/games/<slug>` pages and the Games tab extras: privacy threshold,
//! slugs, admin info/news overrides, and cached store details. Sessions are
//! only inserted (never rewritten) to seed read-only aggregates.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

const PAGE: &str = "<!doctype html><html><head>\n<!--seo--><title>Games · Fraglink</title><!--/seo-->\n</head><body><div data-game-root>x</div></body></html>";

fn app_with_site(pool: PgPool, tag: &str) -> axum::Router {
    let dir = std::env::temp_dir().join(format!("fraglink-gp-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("games-one.html"), PAGE).unwrap();
    fraglink_server::router(state_with(pool, |c| c.site_dir = dir))
}

async fn seed_games(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO games (id, name, verified, steam_appid) VALUES
           ('wow_forever', 'World of Warcraft Forever', true, NULL),
           ('cs2', 'Counter-Strike 2', true, 730),
           ('niche', 'Niche Quest', true, NULL)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, verified = true, steam_appid = excluded.steam_appid",
    )
    .execute(pool)
    .await
    .unwrap();
}

async fn sess(pool: &PgPool, handle: &str, game: &str, hours_ago: i32, secs: i32) {
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, $2,
                now() - ($3 || ' hours')::interval,
                now() - ($3 || ' hours')::interval + ($4 || ' seconds')::interval,
                $4
         FROM users WHERE handle = $1",
    )
    .bind(handle)
    .bind(game)
    .bind(hours_ago)
    .bind(secs)
    .execute(pool)
    .await
    .unwrap();
}

async fn publicize(pool: &PgPool, handles: &[&str]) {
    sqlx::query("UPDATE users SET public_profile = true WHERE handle = ANY($1)")
        .bind(handles)
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn slugs_and_privacy_threshold(pool: PgPool) {
    let app = app_with_site(pool.clone(), "priv");
    seed_games(&pool).await;
    // Warm the catalogue / slug index.
    let cat = api(&app, "GET", "/api/v1/games/catalogue", None, None)
        .await
        .json();
    let cs2 = cat["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g[0] == "cs2")
        .expect("cs2 in catalogue");
    assert_eq!(cs2[3], "counter-strike-2");
    let wow = cat["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g[0] == "wow_forever")
        .expect("wow");
    assert_eq!(wow[3], "world-of-warcraft-forever");

    let missing = api(&app, "GET", "/api/v1/games/page/no-such-game", None, None).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);

    let empty = api(&app, "GET", "/api/v1/games/page/wow-forever", None, None)
        .await
        .json();
    assert_eq!(empty["game_id"], "wow_forever");
    assert_eq!(empty["stats_visible"], false);
    assert!(empty["stats"].is_null());
    assert_eq!(empty["top_players_visible"], false);
    assert!(empty["top_players"].is_null());

    let first_four = ["pika", "nova", "ember", "quill"];
    for (i, h) in first_four.iter().enumerate() {
        signup(&app, &format!("{h}@example.test"), h).await;
        sess(&pool, h, "wow_forever", 2, 3600 + i as i32 * 60).await;
    }
    publicize(&pool, &first_four).await;

    // Four public players: still hidden (default min is 5).
    let four = api(&app, "GET", "/api/v1/games/page/wow-forever", None, None)
        .await
        .json();
    assert_eq!(four["stats_visible"], false, "{four}");

    signup(&app, "ridge@example.test", "ridge").await;
    sess(&pool, "ridge", "wow_forever", 2, 7200).await;
    publicize(&pool, &["ridge"]).await;
    let names = ["pika", "nova", "ember", "quill", "ridge"];
    let five = api(
        &app,
        "GET",
        "/api/v1/games/page/world-of-warcraft-forever",
        None,
        None,
    )
    .await
    .json();
    assert_eq!(five["stats_visible"], true, "{five}");
    assert!(five["stats"]["sessions"].as_i64().unwrap() >= 5);
    assert!(five["stats"]["players_this_week"].as_i64().unwrap() >= 5);
    assert!(five["stats"]["total_seconds"].as_i64().unwrap() >= 5 * 3600);
    assert_eq!(five["top_players_visible"], true);
    let board = five["top_players"].as_array().unwrap();
    assert_eq!(board.len(), 5);
    assert_eq!(board[0]["handle"], "ridge");
    for row in board {
        let h = row["handle"].as_str().unwrap();
        assert!(names.contains(&h), "only public test handles, got {h}");
    }

    // A private profile and a test account never appear on the board; they can
    // still help the anonymous aggregate threshold but not the public list.
    signup(&app, "shade@example.test", "shade").await;
    signup(&app, "botty@example.test", "botty").await;
    sqlx::query("UPDATE users SET public_profile = false WHERE handle = 'shade'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET is_test = true WHERE handle = 'botty'")
        .execute(&pool)
        .await
        .unwrap();
    sess(&pool, "shade", "wow_forever", 4, 10_000).await;
    sess(&pool, "botty", "wow_forever", 4, 9_000).await;
    let again = api(&app, "GET", "/api/v1/games/page/wow-forever", None, None)
        .await
        .json();
    let handles: Vec<&str> = again["top_players"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["handle"].as_str().unwrap())
        .collect();
    assert!(!handles.contains(&"shade"));
    assert!(!handles.contains(&"botty"));
    assert!(handles.contains(&"ridge"));

    let html = send(
        &app,
        axum::http::Request::builder()
            .uri("/games/wow-forever")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(html.status, StatusCode::PERMANENT_REDIRECT, "{}", html.text);
    assert_eq!(html.location(), "/games/world-of-warcraft-forever");

    let ok = send(
        &app,
        axum::http::Request::builder()
            .uri("/games/world-of-warcraft-forever")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert!(ok.text.contains("World of Warcraft Forever"), "{}", ok.text);
    assert!(ok.text.contains("no-cache") || ok.headers.get("cache-control").is_some());

    let gone = send(
        &app,
        axum::http::Request::builder()
            .uri("/games/not-a-real-game")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn tab_most_played_trending_new_and_genres(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    seed_games(&pool).await;
    let names = ["pika", "nova", "ember", "quill", "ridge", "maple"];
    for h in names {
        signup(&app, &format!("{h}@example.test"), h).await;
        // This week: plenty of CS2 hours.
        sess(&pool, h, "cs2", 8, 20_000).await;
        // Last week: a little CS2, so the jump qualifies as trending.
        sess(&pool, h, "cs2", 24 * 8, 1_000).await;
        // Niche stays under the player threshold.
        if h == "pika" || h == "nova" {
            sess(&pool, h, "niche", 2, 8_000).await;
        }
    }
    publicize(&pool, &names).await;

    sqlx::query(
        "INSERT INTO game_catalogue_history
            (actor_kind, actor_label, action, game_id, game_name, summary, public, hidden_from_public)
         VALUES ('system', 'test', 'added', 'cs2', 'Counter-Strike 2', 'Added Counter-Strike 2', true, false)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO game_page_meta (game_id, genres) VALUES ('cs2', 'Action, Free To Play')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // Build slug/catalogue index.
    let _ = api(&app, "GET", "/api/v1/games/catalogue", None, None).await;
    let tab_r = api(&app, "GET", "/api/v1/games/tab", None, None).await;
    assert_eq!(tab_r.status, StatusCode::OK, "{}", tab_r.text);
    let tab = tab_r.json();
    let most = tab["most_played"].as_array().unwrap();
    assert!(
        most.iter()
            .any(|g| g["game_id"] == "cs2" && g["minutes"].as_i64().unwrap() > 0),
        "{tab}"
    );
    assert!(
        most.iter().all(|g| g["game_id"] != "niche"),
        "games under the player threshold must be omitted: {tab}"
    );
    let trending = tab["trending"].as_array().unwrap();
    assert!(
        trending.iter().any(|id| id == "cs2"),
        "cs2 should be trending: {tab}"
    );
    let fresh = tab["new_to_list"].as_array().unwrap();
    assert!(fresh
        .iter()
        .any(|g| g["game_id"] == "cs2" && g["slug"] == "counter-strike-2"));
    let genres = tab["genres"].as_array().unwrap();
    let action = genres
        .iter()
        .find(|g| g["name"] == "Action")
        .expect("Action");
    assert!(action["count"].as_i64().unwrap() >= 1);
    assert!(action["ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == "cs2"));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn admin_can_set_info_and_a_news_feed(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    seed_games(&pool).await;
    let admin = signup(&app, "admin@example.test", "staffer").await;
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = 'staffer'")
        .execute(&pool)
        .await
        .unwrap();
    let _ = api(&app, "GET", "/api/v1/games/catalogue", None, None).await;

    let bad = api(
        &app,
        "POST",
        "/api/v1/admin/games/wow_forever/page",
        Some(&admin),
        Some(json!({ "news_feed_url": "https://127.0.0.1/secret.xml" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.text);

    let saved = api(
        &app,
        "POST",
        "/api/v1/admin/games/wow_forever/page",
        Some(&admin),
        Some(json!({
            "slug": "wow-forever",
            "developer": "Blizzard Entertainment",
            "publisher": "Blizzard Entertainment",
            "release_date": "4 Nov 2026",
            "genres": "MMORPG",
            "platforms": "Windows, macOS",
            "description": "A World of Warcraft private realm with its own news.",
            "news_feed_url": "https://worldofwarcraft.blizzard.com/en-us/news"
        })),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    assert_eq!(saved.json()["developer"], "Blizzard Entertainment");
    assert_eq!(saved.json()["slug"], "wow-forever");

    // Cached news (no outbound fetch in tests): pretends the feed already landed.
    sqlx::query(
        "INSERT INTO game_feed_cache (game_id, news_json, news_fetched_at, news_ok, news_kind)
         VALUES ('wow_forever', $1::jsonb, now(), true, 'rss')
         ON CONFLICT (game_id) DO UPDATE SET news_json = excluded.news_json, news_ok = true, news_kind = 'rss', news_fetched_at = now()",
    )
    .bind(json!([{"title":"Hotfixes","url":"https://example.test/hotfixes","at":"2026-11-13T00:00:00Z"}]))
    .execute(&pool)
    .await
    .unwrap();

    let page = api(&app, "GET", "/api/v1/games/page/wow-forever", None, None)
        .await
        .json();
    assert_eq!(page["slug"], "wow-forever");
    assert_eq!(page["info"]["developer"], "Blizzard Entertainment");
    assert_eq!(page["info"]["genres"], "MMORPG");
    assert_eq!(page["info_source"], "Edited");
    assert_eq!(
        page["description"],
        "A World of Warcraft private realm with its own news."
    );
    assert_eq!(page["description_source"], "Edited");
    let news = page["news"].as_array().unwrap();
    assert_eq!(news[0]["title"], "Hotfixes");
    assert_eq!(page["news_source"], "News feed");

    // Store cache for CS2, no admin override.
    sqlx::query(
        "INSERT INTO game_feed_cache (game_id, steam_appid, details_json, details_fetched_at, details_ok)
         VALUES ('cs2', 730, $1::jsonb, now(), true)
         ON CONFLICT (game_id) DO UPDATE SET details_json = excluded.details_json, details_ok = true, details_fetched_at = now()",
    )
    .bind(json!({
        "developers": ["Valve"],
        "publishers": ["Valve"],
        "release_date": {"date": "27 Sep 2023"},
        "genres": [{"description": "Action"}],
        "platforms": {"windows": true, "mac": false, "linux": true},
        "short_description": "Counter-Strike 2 is the next era of tactical FPS action."
    }))
    .execute(&pool)
    .await
    .unwrap();
    let cs = api(
        &app,
        "GET",
        "/api/v1/games/page/counter-strike-2",
        None,
        None,
    )
    .await
    .json();
    assert_eq!(cs["info"]["developer"], "Valve");
    assert_eq!(cs["info"]["platforms"], "Windows, Linux");
    assert_eq!(cs["info"]["release_date"], "27 Sep 2023");
    assert_eq!(cs["info_source"], "Store listing");
    assert_eq!(
        cs["description"],
        "Counter-Strike 2 is the next era of tactical FPS action."
    );
    assert_eq!(cs["description_source"], "Store listing");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn wikipedia_cache_fills_info_and_blurb(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    seed_games(&pool).await;
    let _ = api(&app, "GET", "/api/v1/games/catalogue", None, None).await;
    sqlx::query(
        "INSERT INTO game_feed_cache (game_id, details_json, details_fetched_at, details_ok, details_kind)
         VALUES ('niche', $1::jsonb, now(), true, 'wiki')
         ON CONFLICT (game_id) DO UPDATE SET details_json = excluded.details_json, details_ok = true, details_kind = 'wiki'",
    )
    .bind(json!({
        "kind": "wiki",
        "title": "Niche Quest",
        "extract": "Niche Quest is a tiny adventure game.",
        "url": "https://en.wikipedia.org/wiki/Niche_Quest",
        "developers": "Quiet Studio",
        "publishers": "Quiet Studio",
        "release_date": "1 Jan 2020",
        "genres": "Adventure",
        "platforms": "Windows",
        "short_description": "Niche Quest is a tiny adventure game."
    }))
    .execute(&pool)
    .await
    .unwrap();
    let page = api(&app, "GET", "/api/v1/games/page/niche-quest", None, None)
        .await
        .json();
    assert_eq!(page["info"]["developer"], "Quiet Studio");
    assert_eq!(page["info"]["genres"], "Adventure");
    assert_eq!(page["info_source"], "Wikipedia");
    assert_eq!(page["description"], "Niche Quest is a tiny adventure game.");
    assert_eq!(page["description_source"], "Wikipedia");
    assert_eq!(
        page["description_url"],
        "https://en.wikipedia.org/wiki/Niche_Quest"
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn feed_gate_caps_outbound_hits(pool: PgPool) {
    let st = state(pool);
    for _ in 0..30 {
        assert!(st
            .feed_gate
            .try_acquire(30, std::time::Duration::from_secs(60)));
    }
    assert!(!st
        .feed_gate
        .try_acquire(30, std::time::Duration::from_secs(60)));
}
