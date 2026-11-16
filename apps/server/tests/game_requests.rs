//! Public "missing a game?" requests and the admin queue.
mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

async fn set_admin(pool: &PgPool, handle: &str) {
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = $1")
        .bind(handle)
        .execute(pool)
        .await
        .unwrap();
}

async fn submit(app: &axum::Router, token: &str, name: &str, exe: &str, note: &str) -> Resp {
    api(
        app,
        "POST",
        "/api/v1/games/requests",
        Some(token),
        Some(json!({ "name": name, "exe_or_link": exe, "note": note })),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn unsigned_users_cannot_request(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let r = api(
        &app,
        "POST",
        "/api/v1/games/requests",
        None,
        Some(json!({ "name": "Anything" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{}", r.text);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn already_listed_does_not_insert(pool: PgPool) {
    sqlx::query(
        "INSERT INTO games (id, name, verified) VALUES ('listed_game', 'Listed Game', true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let app = fraglink_server::router(state(pool.clone()));
    let tok = signup(&app, "req-listed@example.com", "reqlisted").await;
    let r = submit(&app, &tok, "listed game", "", "").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let j = r.json();
    assert_eq!(j["already_listed"], true);
    assert_eq!(j["game"]["id"], "listed_game");
    assert_eq!(j["game"]["name"], "Listed Game");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM game_requests")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn submit_dedupes_and_rate_limits(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let tok = signup(&app, "req-rate@example.com", "reqrate").await;
    let first = submit(&app, &tok, "Brand New Title", "BrandNew.exe", "please").await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);
    let id = first.json()["id"].as_i64().unwrap();
    assert_eq!(first.json()["already_requested"], Value::Null);

    let again = submit(&app, &tok, "brand-new title", "", "").await;
    assert_eq!(again.status, StatusCode::OK, "{}", again.text);
    assert_eq!(again.json()["already_requested"], true);
    assert_eq!(again.json()["id"], id);
    let reports: i32 = sqlx::query_scalar("SELECT reports FROM game_requests WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(reports, 2);

    for i in 2..=5 {
        let r = submit(&app, &tok, &format!("Unique Game {i}"), "", "").await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    let sixth = submit(&app, &tok, "One Too Many", "", "").await;
    assert_eq!(
        sixth.status,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        sixth.text
    );
    assert!(
        sixth.text.contains("5 games a day") || sixth.text.contains("rate"),
        "{}",
        sixth.text
    );

    // A duplicate of an existing open request still does not consume a new slot.
    let bump = submit(&app, &tok, "Brand New Title", "", "").await;
    assert_eq!(bump.status, StatusCode::OK, "{}", bump.text);
    assert_eq!(bump.json()["already_requested"], true);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn admin_queue_approve_writes_history_and_can_dismiss(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let tok = signup(&app, "req-user@example.com", "requser").await;
    let admin = signup(&app, "req-admin@example.com", "reqadmin").await;
    set_admin(&pool, "reqadmin").await;

    let r = submit(&app, &tok, "New Face Game", "NewFace.exe", "store later").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let id = r.json()["id"].as_i64().unwrap();

    let denied = api(&app, "GET", "/api/v1/admin/game-requests", Some(&tok), None).await;
    // Non-staff get 404 so the admin API is not advertised.
    assert_eq!(denied.status, StatusCode::NOT_FOUND, "{}", denied.text);

    let list = api(
        &app,
        "GET",
        "/api/v1/admin/game-requests?status=open",
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text);
    let j = list.json();
    assert_eq!(j["open"], 1);
    assert_eq!(j["total"], 1);
    assert_eq!(j["requests"][0]["name"], "New Face Game");
    assert_eq!(j["requests"][0]["exe_or_link"], "NewFace.exe");
    assert_eq!(j["requests"][0]["by"], "requser");
    assert!(j["requests"][0].get("email").is_none());

    // A name-only request cannot be approved without an exe, Steam link, or existing id.
    let bare = submit(&app, &tok, "Name Only Game", "", "").await;
    let bare_id = bare.json()["id"].as_i64().unwrap();
    let bad = api(
        &app,
        "POST",
        &format!("/api/v1/admin/game-requests/{bare_id}/approve"),
        Some(&admin),
        Some(json!({ "name": "Name Only Game" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.text);
    let still_open: String = sqlx::query_scalar("SELECT status FROM game_requests WHERE id = $1")
        .bind(bare_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still_open, "open");

    let ok = api(
        &app,
        "POST",
        &format!("/api/v1/admin/game-requests/{id}/approve"),
        Some(&admin),
        Some(json!({ "name": "New Face Game", "exe": "NewFace.exe" })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let game_id = ok.json()["game_id"].as_str().unwrap().to_string();
    assert!(game_id.starts_with("exe:"), "{game_id}");

    let hist = sqlx::query(
        "SELECT action, game_name, public, diff FROM game_catalogue_history WHERE game_id = $1",
    )
    .bind(&game_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].get::<String, _>("action"), "added");
    assert_eq!(hist[0].get::<String, _>("game_name"), "New Face Game");
    assert!(hist[0].get::<bool, _>("public"));

    let after = api(
        &app,
        "GET",
        "/api/v1/admin/game-requests?status=approved",
        Some(&admin),
        None,
    )
    .await
    .json();
    assert_eq!(after["requests"][0]["status"], "approved");
    assert_eq!(after["requests"][0]["name"], "New Face Game");
    // "Name Only Game" is still open because the failed approve wrote nothing.
    assert_eq!(after["open"], 1);

    let other = submit(&app, &tok, "Skip This One", "", "no thanks").await;
    let oid = other.json()["id"].as_i64().unwrap();
    let dis = api(
        &app,
        "POST",
        &format!("/api/v1/admin/game-requests/{oid}/status"),
        Some(&admin),
        Some(json!({ "status": "dismissed" })),
    )
    .await;
    assert_eq!(dis.status, StatusCode::OK, "{}", dis.text);
    let st: String = sqlx::query_scalar("SELECT status FROM game_requests WHERE id = $1")
        .bind(oid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "dismissed");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn approve_steam_link_and_existing_id(pool: PgPool) {
    sqlx::query(
        "INSERT INTO games (id, name, verified) VALUES ('already_there', 'Already There', true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let app = fraglink_server::router(state(pool.clone()));
    let tok = signup(&app, "req-steam@example.com", "reqsteam").await;
    let admin = signup(&app, "req-sadmin@example.com", "reqsadmin").await;
    set_admin(&pool, "reqsadmin").await;

    let steam = submit(
        &app,
        &tok,
        "Steam Only",
        "https://store.steampowered.com/app/424242/Steam_Only/",
        "",
    )
    .await;
    let sid = steam.json()["id"].as_i64().unwrap();
    let ok = api(
        &app,
        "POST",
        &format!("/api/v1/admin/game-requests/{sid}/approve"),
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(ok.json()["game_id"], "steam:424242");
    let name: String = sqlx::query_scalar("SELECT name FROM games WHERE id = 'steam:424242'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Steam Only");

    let link = submit(&app, &tok, "Link Existing", "", "").await;
    let lid = link.json()["id"].as_i64().unwrap();
    let ok2 = api(
        &app,
        "POST",
        &format!("/api/v1/admin/game-requests/{lid}/approve"),
        Some(&admin),
        Some(json!({ "game_id": "already_there" })),
    )
    .await;
    assert_eq!(ok2.status, StatusCode::OK, "{}", ok2.text);
    assert_eq!(ok2.json()["game_id"], "already_there");
    let gid: String = sqlx::query_scalar("SELECT game_id FROM game_requests WHERE id = $1")
        .bind(lid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(gid, "already_there");
}
