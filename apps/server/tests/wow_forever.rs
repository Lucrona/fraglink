//! WoW Forever uploads stay off Retail's imported total.
mod common;
use axum::http::StatusCode;
use common::*;
use fraglink_core::wow_played::{sign_with, CharEntry, SIG_KEYS};
use serde_json::json;
use sqlx::PgPool;

fn forever(name: &str, first: &str, surname: &str, played: i64, at: i64, tag: &str) -> CharEntry {
    let mut e = CharEntry {
        region: "eu".into(),
        realm: String::new(),
        name: name.into(),
        class: "WARRIOR".into(),
        level: 60,
        played,
        at,
        sv: 2,
        battletag: tag.into(),
        flavour: "forever".into(),
        project: 18,
        build: "16099".into(),
        first: first.into(),
        surname: surname.into(),
        beta: 0,
        confirmed: 1,
        sig: String::new(),
    };
    e.sig = sign_with(SIG_KEYS[1].1, &e);
    e
}

fn flavoured(mut e: CharEntry, flavour: &str, beta: i64, confirmed: i64) -> CharEntry {
    e.flavour = flavour.into();
    e.beta = beta;
    e.confirmed = confirmed;
    e.sig = sign_with(SIG_KEYS[1].1, &e);
    e
}

async fn post(app: &axum::Router, t: &str, game: &str, e: &CharEntry) -> common::Resp {
    api(
        app,
        "POST",
        "/api/v1/hours/wow_played",
        Some(t),
        Some(json!({ "game_id": game, "characters": [e] })),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_import_does_not_touch_retail_hours(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "ff@example.com", "ffimport").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'ffimport'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO games(id, name) VALUES ('wow', 'World of Warcraft'), ('wow_forever', 'World of Warcraft: Forever') ON CONFLICT DO NOTHING",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO wow_characters(id, user_id, game_id, region, realm, name, played_s, played_at)
         VALUES (gen_random_uuid(), $1, 'wow', 'us', 'Area 52', 'Same Name', 7200, now())",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, captured_at)
         VALUES (gen_random_uuid(), $1, 'wow', 'imported', 'wow_addon', 400, now())",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s)
         VALUES (gen_random_uuid(), $1, 'wow_forever', now() - interval '1 hour', now(), 1800)",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    let flavour: Option<String> =
        sqlx::query_scalar("SELECT flavour FROM wow_characters WHERE game_id = 'wow'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        flavour.is_none(),
        "new columns stay null on existing retail rows"
    );

    let now = chrono::Utc::now().timestamp() - 120;
    let e = forever("Same Name", "Same", "Name", 10_800, now, "For#1");
    let r = post(&app, &t, "wow_forever", &e).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["accepted"], 1, "{}", r.text);
    assert_eq!(r.json()["verified"], 0);

    let again = post(&app, &t, "wow_forever", &e).await;
    assert_eq!(again.json()["accepted"], 0, "{}", again.text);
    assert_eq!(again.json()["unchanged"].as_array().unwrap().len(), 1);

    let snaps: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_snaps s JOIN wow_characters c ON c.id = s.char_id
         WHERE c.game_id = 'wow_forever'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(snaps, 1, "the same snapshot twice adds nothing");

    let down = forever("Same Name", "Same", "Name", 3_600, now + 30, "For#1");
    let r = post(&app, &t, "wow_forever", &down).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "wow_decrease",
        "{}",
        r.text
    );
    let played: i64 =
        sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE game_id = 'wow_forever'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(played, 10_800);

    let retail_min: i32 = sqlx::query_scalar(
        "SELECT minutes FROM hours_entries WHERE game_id = 'wow' AND tier = 'imported'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retail_min, 400);
    let forever_hours: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE game_id = 'wow_forever'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        forever_hours, 0,
        "forever /played is not an imported game total"
    );
    let tracked: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(duration_s),0) FROM sessions WHERE game_id = 'wow_forever'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tracked, 1800);

    let renamed = forever("Other Name", "Other", "Name", 11_000, now + 40, "For#1");
    let r = post(&app, &t, "wow_forever", &renamed).await;
    assert_eq!(r.json()["accepted"], 1, "{}", r.text);
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM wow_characters WHERE game_id = 'wow_forever' ORDER BY name",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(names, ["Other Name", "Same Name"]);

    let beta = flavoured(
        forever("Beta Kin", "Beta", "Kin", 5_000, now, "For#1"),
        "forever_beta",
        1,
        1,
    );
    let r = post(&app, &t, "wow_forever", &beta).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "forever_beta",
        "{}",
        r.text
    );
    let unknown = flavoured(
        forever("Quiet Kin", "Quiet", "Kin", 5_000, now, "For#1"),
        "forever_unconfirmed",
        0,
        0,
    );
    let r = post(&app, &t, "wow_forever", &unknown).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "forever_unconfirmed",
        "{}",
        r.text
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_characters WHERE name IN ('Beta Kin', 'Quiet Kin')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stored, 0);

    let id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM wow_characters WHERE game_id = 'wow_forever' AND name = 'Same Name'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    for hidden in [true, false] {
        let r = api(
            &app,
            "POST",
            &format!("/api/v1/me/wow/{id}"),
            Some(&t),
            Some(json!({ "hidden": hidden })),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    let still: i64 = sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 10_800);
    let verified: bool = sqlx::query_scalar("SELECT verified FROM wow_characters WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        !verified,
        "Forever stays unverified until the account list is tested"
    );
    let realm: String = sqlx::query_scalar("SELECT realm FROM wow_characters WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(realm.is_empty());
}
