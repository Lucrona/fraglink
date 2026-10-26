mod common;
use axum::http::StatusCode;
use common::*;
use fraglink_core::wow_played::{sign_with, CharEntry, SIG_KEYS};
use serde_json::{json, Value};
use sqlx::PgPool;

fn entry(name: &str, played: i64, at: i64, tag: &str) -> CharEntry {
    let mut e = CharEntry {
        region: "us".into(),
        realm: "Area 52".into(),
        name: name.into(),
        class: "SHAMAN".into(),
        level: 80,
        played,
        at,
        sv: SIG_KEYS[0].0,
        battletag: tag.into(),
        sig: String::new(),
    };
    e.sig = sign_with(SIG_KEYS[0].1, &e);
    e
}

async fn up(app: &axum::Router, t: &str, es: &[CharEntry]) -> Value {
    let r = api(
        app,
        "POST",
        "/api/v1/hours/wow_played",
        Some(t),
        Some(json!({ "game_id": "wow", "characters": es })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    serde_json::from_str(&r.text).unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn signed_uploads_verify_and_scams_are_refused(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "w@example.com", "wowtester").await;
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = 'wowtester'")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                 SELECT id, 'wow', 'us', 'area-52', 'Thrall' FROM users WHERE handle = 'wowtester'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let now = chrono::Utc::now().timestamp() - 60;

    // a genuine entry for a Battle.net character verifies; one not on the account is self-reported
    let r = up(
        &app,
        &t,
        &[
            entry("Thrall", 360_000, now - 7200, "Thrall#1234"),
            entry("Alt", 3_600, now - 7200, "Thrall#1234"),
        ],
    )
    .await;
    assert_eq!(r["accepted"], 2, "{r}");
    let me: Value = serde_json::from_str(
        &api(&app, "GET", "/api/v1/me/wow", Some(&t), None)
            .await
            .text,
    )
    .unwrap();
    let v: Vec<(String, bool)> = me["characters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_string(),
                c["verified"].as_bool().unwrap(),
            )
        })
        .collect();
    assert!(
        v.contains(&("Thrall".into(), true)) && v.contains(&("Alt".into(), false)),
        "{v:?}"
    );

    // edited after the game wrote it: refused and flagged
    let mut forged = entry("Thrall", 360_000, now - 3600, "Thrall#1234");
    forged.played = 9_000_000;
    let r = up(&app, &t, &[forged]).await;
    assert_eq!(r["rejected"][0]["reason"], "wow_checksum", "{r}");
    // another account's file (validly signed, different BattleTag)
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 370_000, now - 3600, "Someone#9999")],
    )
    .await;
    assert_eq!(r["rejected"][0]["reason"], "wow_battletag", "{r}");
    // /played can't go down
    let r = up(&app, &t, &[entry("Thrall", 100, now - 3000, "Thrall#1234")]).await;
    assert_eq!(r["rejected"][0]["reason"], "wow_decrease", "{r}");
    // more /played than wall-clock time since the last answer
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 360_000 + 90_000, now - 1800, "Thrall#1234")],
    )
    .await;
    assert_eq!(r["rejected"][0]["reason"], "wow_impossible", "{r}");

    let kinds: Vec<String> = sqlx::query_scalar("SELECT DISTINCT kind FROM wow_flags ORDER BY 1")
        .fetch_all(&pool)
        .await
        .unwrap();
    for k in [
        "wow_battletag",
        "wow_checksum",
        "wow_decrease",
        "wow_impossible",
    ] {
        assert!(kinds.iter().any(|x| x == k), "{k} flagged: {kinds:?}");
    }
    // stored value is untouched by every refused upload
    let s: i64 = sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE name = 'Thrall'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(s, 360_000);
}
