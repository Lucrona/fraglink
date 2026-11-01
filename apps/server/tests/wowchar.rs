mod common;
use axum::http::StatusCode;
use common::*;
use fraglink_core::wow_played::{sign_with, CharEntry, SIG_KEYS};
use serde_json::{json, Value};
use sqlx::PgPool;

fn entry(name: &str, played: i64, at: i64) -> CharEntry {
    let mut e = CharEntry {
        region: "us".into(),
        realm: "Area 52".into(),
        name: name.into(),
        class: "SHAMAN".into(),
        level: 80,
        played,
        at,
        sv: SIG_KEYS[0].0,
        battletag: "Thrall#1234".into(),
        sig: String::new(),
    };
    e.sig = sign_with(SIG_KEYS[0].1, &e);
    e
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn popup_is_gated_and_guild_is_opt_in(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "c@example.com", "popper").await;
    let now = chrono::Utc::now().timestamp() - 60;
    let r = api(&app, "POST", "/api/v1/hours/wow_played", Some(&t),
        Some(json!({ "game_id": "wow", "characters": [entry("Thrall", 360_000, now - 7200), entry("Alt", 3_600, now - 7200)] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = 'popper'")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                 SELECT id, 'wow', 'us', 'area-52', 'Thrall' FROM users WHERE handle = 'popper'",
    )
    .execute(&pool)
    .await
    .unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM wow_characters WHERE name = 'Thrall'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let alt: uuid::Uuid = sqlx::query_scalar("SELECT id FROM wow_characters WHERE name = 'Alt'")
        .fetch_one(&pool)
        .await
        .unwrap();
    // a fresh cached summary, so no Blizzard call is made
    sqlx::query("INSERT INTO wow_char_details(char_id, status, data, fetched_at) VALUES($1, 'ok', $2, now())")
        .bind(id)
        .bind(json!({ "name": "Thrall", "level": 80, "guild": { "name": "Horde Elite", "rank": 0 }, "gear": [] }))
        .execute(&pool).await.unwrap();
    let url = format!("/api/v1/users/popper/wow/{id}");

    // feature off: nothing
    assert_eq!(
        api(&app, "GET", &url, Some(&t), None).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES('wow_characters', true)")
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::features::forget();

    // on: served, guild withheld until opted in
    let r = api(&app, "GET", &url, Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v: Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(v["status"], "ok");
    assert!(v["guild"].is_null(), "{v}");
    let p: Value = serde_json::from_str(
        &api(&app, "GET", "/api/v1/users/popper/profile", Some(&t), None)
            .await
            .text,
    )
    .unwrap();
    assert!(p["wow_characters"][0]["guild"].is_null(), "{p}");

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/me/wow/{id}/guild"),
        Some(&t),
        Some(json!({ "show_guild": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v: Value =
        serde_json::from_str(&api(&app, "GET", &url, Some(&t), None).await.text).unwrap();
    assert_eq!(v["guild"]["name"], "Horde Elite", "{v}");
    let p: Value = serde_json::from_str(
        &api(&app, "GET", "/api/v1/users/popper/profile", Some(&t), None)
            .await
            .text,
    )
    .unwrap();
    assert_eq!(p["wow_characters"][0]["guild"], "Horde Elite", "{p}");

    // unverified and unknown characters are not served
    assert_eq!(
        api(
            &app,
            "GET",
            &format!("/api/v1/users/popper/wow/{alt}"),
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(
            &app,
            "GET",
            &format!("/api/v1/users/popper/wow/{}", uuid::Uuid::new_v4()),
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // someone else can't toggle it
    let t2 = signup(&app, "d@example.com", "other").await;
    assert_ne!(
        api(
            &app,
            "POST",
            &format!("/api/v1/me/wow/{id}/guild"),
            Some(&t2),
            Some(json!({ "show_guild": false }))
        )
        .await
        .status,
        StatusCode::OK
    );

    // hidden: gone
    sqlx::query("UPDATE wow_characters SET hidden = true WHERE id = $1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &url, Some(&t), None).await.status,
        StatusCode::NOT_FOUND
    );
}
