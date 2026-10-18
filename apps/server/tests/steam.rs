//! Steam import against a local mock of the Steam Web API + OpenID endpoint.
//! No real Steam key is needed or used.
mod common;
use axum::extract::{Form, Query};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;

const GABEN: &str = "76561197960287930";
const PRIVATE: &str = "76561190000000002";
const KEY: &str = "test-key-not-real";

async fn mock_steam() -> String {
    async fn vanity(Query(q): Query<HashMap<String, String>>) -> Result<Json<Value>, StatusCode> {
        if q.get("key").map(String::as_str) != Some(KEY) {
            return Err(StatusCode::FORBIDDEN);
        }
        Ok(Json(match q.get("vanityurl").map(String::as_str) {
            Some("gabelogannewell") => json!({ "response": { "steamid": GABEN, "success": 1 } }),
            _ => json!({ "response": { "success": 42, "message": "No match" } }),
        }))
    }
    async fn owned(Query(q): Query<HashMap<String, String>>) -> Result<String, StatusCode> {
        if q.get("key").map(String::as_str) != Some(KEY) {
            return Err(StatusCode::FORBIDDEN);
        }
        assert_eq!(q.get("include_appinfo").map(String::as_str), Some("1"));
        match q.get("steamid").map(String::as_str) {
            Some(GABEN) => Ok(include_str!("fixtures/steam_owned_games.json").to_string()),
            Some(PRIVATE) => {
                Ok(include_str!("fixtures/steam_owned_games_private.json").to_string())
            }
            _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
        }
    }
    async fn openid(Form(f): Form<HashMap<String, String>>) -> String {
        let ok = f.get("openid.mode").map(String::as_str) == Some("check_authentication")
            && f.get("openid.sig").map(String::as_str) == Some("good-signature");
        format!("ns:http://specs.openid.net/auth/2.0\nis_valid:{ok}\n")
    }
    let app = Router::new()
        .route("/ISteamUser/ResolveVanityURL/v1/", get(vanity))
        .route("/IPlayerService/GetOwnedGames/v1/", get(owned))
        .route("/openid/login", post(openid));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

async fn app_with_mock(pool: PgPool, key: Option<&str>) -> (Router, String) {
    let base = mock_steam().await;
    let b = base.clone();
    let key = key.map(str::to_string);
    let st = state_with(pool, move |c| {
        c.steam_api_key = key;
        c.steam_api_base = b.clone();
        c.steam_openid_url = format!("{b}/openid/login");
    });
    (fraglink_server::router(st), base)
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn link_by_vanity_and_import(pool: PgPool) {
    let (app, _) = app_with_mock(pool.clone(), Some(KEY)).await;
    let t = signup(&app, "g@example.com", "gamer").await;

    assert_eq!(
        api(&app, "POST", "/api/v1/steam/sync", Some(&t), None)
            .await
            .status,
        StatusCode::BAD_REQUEST,
        "not linked yet"
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": "https://steamcommunity.com/id/nobody-here/" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "steam_profile_not_found");
    let r = api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": "https://steamcommunity.com/id/gabelogannewell/" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["steamid"], GABEN);
    assert_eq!(r.json()["verified"], false);

    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert_eq!(v["games"], 3, "games with 0 minutes are skipped");
    // CS2 maps onto the signature id so Steam minutes and tracked sessions merge
    let rows: Vec<(String, i32)> =
        sqlx::query_as("SELECT game_id, minutes_forever FROM steam_playtime ORDER BY steam_appid")
            .fetch_all(&pool)
            .await
            .unwrap();
    let cs2_min = rows
        .iter()
        .find(|(g, _)| g == "cs2")
        .expect("appid 730 maps to cs2")
        .1;
    assert!(rows.iter().any(|(g, _)| g == "dota2"));
    assert!(rows.iter().all(|(_, m)| *m > 0));

    let s = api(&app, "GET", "/api/v1/stats", Some(&t), None)
        .await
        .json();
    let cs2 = s["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["game_id"] == "cs2")
        .unwrap()
        .clone();
    assert_eq!(cs2["display_tier"], "tracked");
    assert_eq!(cs2["tracked_min"], cs2_min);

    // icon lookup for friends' trays: our own icon paths only, never a third-party URL
    let ic = api(
        &app,
        "GET",
        "/api/v1/games/icons?ids=cs2,dota2,steam:999,wow-unknown",
        Some(&t),
        None,
    )
    .await
    .json();
    assert_eq!(ic["cs2"]["steam_appid"], 730);
    assert_eq!(ic["cs2"]["icon_path"], "/icons/games/32/cs2.png");
    assert!(ic["cs2"]["icon_url"].is_null());
    assert!(ic["dota2"]["icon_url"].is_null());
    assert_eq!(ic["steam:999"]["steam_appid"], 999);
    assert!(ic.get("wow-unknown").is_none());

    // cooldown
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.json()["error"], "steam_sync_cooldown");
    sqlx::query("UPDATE linked_accounts SET last_sync_at = now() - interval '11 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "POST", "/api/v1/steam/sync", Some(&t), None)
            .await
            .status,
        StatusCode::OK
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM steam_playtime")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 3, "re-import updates in place");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn private_profile_explains_setting(pool: PgPool) {
    let (app, _) = app_with_mock(pool.clone(), Some(KEY)).await;
    let t = signup(&app, "p@example.com", "private").await;
    api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": PRIVATE })),
    )
    .await;
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.json()["error"], "steam_private");
    assert!(
        r.json()["message"]
            .as_str()
            .unwrap()
            .contains("Game details"),
        "{}",
        r.text
    );
    let st: String = sqlx::query_scalar("SELECT sync_status FROM linked_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(st, "private");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn missing_key_and_upstream_errors(pool: PgPool) {
    let (app, _) = app_with_mock(pool.clone(), None).await;
    let t = signup(&app, "k@example.com", "nokey").await;
    // linking by SteamID64 needs no key
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/steam/link",
            Some(&t),
            Some(json!({ "profile": GABEN }))
        )
        .await
        .status,
        StatusCode::OK
    );
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(r.json()["error"], "steam_not_configured");

    // upstream failure: the key must never appear in errors
    let (app, _) = app_with_mock(pool, Some(KEY)).await;
    let t = signup(&app, "u@example.com", "upstream").await;
    api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": "76561190000000099" })),
    )
    .await;
    let r = api(&app, "POST", "/api/v1/steam/sync", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY);
    assert!(!r.text.contains(KEY), "{}", r.text);
}

fn openid_query(base: &str, steamid: &str, sig: &str) -> String {
    let claimed = format!("https://steamcommunity.com/openid/id/{steamid}");
    let params = [
        ("openid.ns", "http://specs.openid.net/auth/2.0"),
        ("openid.mode", "id_res"),
        ("openid.op_endpoint", &format!("{base}/openid/login")),
        ("openid.claimed_id", &claimed),
        ("openid.identity", &claimed),
        (
            "openid.return_to",
            "http://localhost:8787/auth/steam/callback",
        ),
        ("openid.response_nonce", "2026-09-26T10:00:00Zabc"),
        ("openid.assoc_handle", "1234567890"),
        (
            "openid.signed",
            "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle",
        ),
        ("openid.sig", sig),
    ];
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn sign_in_through_steam_verifies_link(pool: PgPool) {
    let (app, base) = app_with_mock(pool.clone(), Some(KEY)).await;
    let r = form(
        &app,
        "/signup",
        None,
        Some(ORIGIN),
        "email=o%40example.com&handle=openid&password=correct+horse+battery&age_ok=1",
    )
    .await;
    let cookie = r.cookie().unwrap();

    let r = page(&app, "/auth/steam/start", Some(&cookie)).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(r
        .location()
        .starts_with(&format!("{base}/openid/login?openid.ns=")));

    // forged signature is refused
    let r = page(
        &app,
        &format!(
            "/auth/steam/callback?{}",
            openid_query(&base, GABEN, "forged")
        ),
        Some(&cookie),
    )
    .await;
    assert!(r.location().starts_with("/?err="), "{}", r.location());
    // assertion from a different OP endpoint is refused without asking it
    let r = page(
        &app,
        &format!(
            "/auth/steam/callback?{}",
            openid_query("https://evil.example", GABEN, "good-signature")
        ),
        Some(&cookie),
    )
    .await;
    assert!(r.location().starts_with("/?err="));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM linked_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    let r = page(
        &app,
        &format!(
            "/auth/steam/callback?{}",
            openid_query(&base, GABEN, "good-signature")
        ),
        Some(&cookie),
    )
    .await;
    assert!(r.location().starts_with("/?msg="), "{}", r.location());
    let (uid, verified): (String, bool) =
        sqlx::query_as("SELECT provider_uid, verified FROM linked_accounts")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(uid, GABEN);
    assert!(verified);

    // the same Steam account can't be verified by a second Fraglink account
    let r = form(
        &app,
        "/signup",
        None,
        Some(ORIGIN),
        "email=o2%40example.com&handle=openid2&password=correct+horse+battery&age_ok=1",
    )
    .await;
    let cookie2 = r.cookie().unwrap();
    let r = page(
        &app,
        &format!(
            "/auth/steam/callback?{}",
            openid_query(&base, GABEN, "good-signature")
        ),
        Some(&cookie2),
    )
    .await;
    assert!(r.location().contains("already"), "{}", r.location());

    // the dashboard shows the verified link
    let home = page(&app, "/", Some(&cookie)).await;
    assert!(
        home.text.contains(GABEN) && home.text.contains("Verified"),
        "{}",
        home.text
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn daily_sync_refreshes_stale_links(pool: PgPool) {
    let base = mock_steam().await;
    let b = base.clone();
    let st = state_with(pool.clone(), move |c| {
        c.steam_api_key = Some(KEY.into());
        c.steam_api_base = b.clone();
    });
    let app = fraglink_server::router(st.clone());
    let t = signup(&app, "d@example.com", "daily").await;
    api(
        &app,
        "POST",
        "/api/v1/steam/link",
        Some(&t),
        Some(json!({ "profile": GABEN })),
    )
    .await;
    assert_eq!(
        fraglink_server::steam::sync_due(&st, 10).await.unwrap(),
        1,
        "never synced -> due"
    );
    assert_eq!(
        fraglink_server::steam::sync_due(&st, 10).await.unwrap(),
        0,
        "fresh -> not due"
    );
    sqlx::query("UPDATE linked_accounts SET last_sync_at = now() - interval '25 hours'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(fraglink_server::steam::sync_due(&st, 10).await.unwrap(), 1);
}
