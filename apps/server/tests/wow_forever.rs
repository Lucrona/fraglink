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
        guid: String::new(),
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
    assert_eq!(r.json()["total_min"], 180, "{}", r.text);

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
        forever_hours, 1,
        "a duplicate upload does not add a second total"
    );
    let forever_min: i32 = sqlx::query_scalar(
        "SELECT minutes FROM hours_entries WHERE game_id = 'wow_forever' AND tier = 'imported'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(forever_min, 180);
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

/// Shown hours for Forever use Retail's rule: the imported /played total and the
/// app's tracked time are not added together. Play before the snapshot is already
/// inside /played. Only tracked play after that snapshot is added, and only on the
/// imported side; display then keeps the higher of that and the full tracked time.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_display_follows_the_retail_rule(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "fd@example.com", "fdisplay").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'fdisplay'")
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
        "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, captured_at)
         VALUES (gen_random_uuid(), $1, 'wow', 'imported', 'wow_addon', 400, timestamptz '2026-01-01')",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    // 30 minutes tracked, entirely before the /played snapshot.
    sqlx::query(
        "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s)
         VALUES (gen_random_uuid(), $1, 'wow_forever',
                 timestamptz '2026-02-01 00:00:00+00', timestamptz '2026-02-01 00:30:00+00', 1800)",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    let at = chrono::DateTime::parse_from_rfc3339("2026-03-01T00:00:00Z")
        .unwrap()
        .timestamp();
    let e = forever("Vale Mocken", "Vale", "Mocken", 7_200, at, "For#1");
    let r = post(&app, &t, "wow_forever", &e).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["total_min"], 120, "{}", r.text);

    let (tracked, imported, display): (i64, i64, i64) = sqlx::query_as(
        "SELECT tracked_min, imported_min, display_min FROM user_game_stats
         WHERE user_id = $1 AND game_id = 'wow_forever'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tracked, 30);
    assert_eq!(imported, 120);
    assert_eq!(display, 120, "display is not tracked 30 plus imported 120");
    let retail: i32 = sqlx::query_scalar("SELECT minutes FROM hours_entries WHERE game_id = 'wow'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(retail, 400);

    let again = post(&app, &t, "wow_forever", &e).await;
    assert_eq!(again.json()["unchanged"].as_array().unwrap().len(), 1);
    let rows: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE game_id = 'wow_forever'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(rows, 1);
    let display_again: i64 = sqlx::query_scalar(
        "SELECT display_min FROM user_game_stats WHERE user_id = $1 AND game_id = 'wow_forever'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(display_again, 120);

    // A Retail upload must not move the Forever total. This character is on the
    // linked account, so Retail's own total can update; Forever's row must not.
    sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
         SELECT id, 'bnet', 'fd', 'For#1', true, 'openid wow.profile' FROM users WHERE handle = 'fdisplay'",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
         SELECT id, 'wow', 'us', 'area-52', 'Retail Kin' FROM users WHERE handle = 'fdisplay'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let mut retail_char = forever("Retail Kin", "Retail", "Kin", 3_600, at, "For#1");
    retail_char.region = "us".into();
    retail_char.realm = "Area 52".into();
    retail_char.name = "Retail Kin".into();
    retail_char.flavour.clear();
    retail_char.project = 0;
    retail_char.sv = 1;
    retail_char.first.clear();
    retail_char.surname.clear();
    retail_char.confirmed = 0;
    retail_char.beta = 0;
    retail_char.sig = sign_with(SIG_KEYS[0].1, &retail_char);
    let rr = post(&app, &t, "wow", &retail_char).await;
    assert_eq!(rr.status, StatusCode::OK, "{}", rr.text);
    let forever_after: i32 = sqlx::query_scalar(
        "SELECT minutes FROM hours_entries WHERE game_id = 'wow_forever' AND tier = 'imported'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(forever_after, 120);
    let retail_kept: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hours_entries WHERE game_id = 'wow' AND minutes = 400",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        retail_kept, 1,
        "the existing Retail import row is not rewritten"
    );
}

async fn retail_snapshot(pool: &PgPool, uid: uuid::Uuid) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
          'characters', (SELECT jsonb_agg(to_jsonb(c) ORDER BY c.id) FROM wow_characters c WHERE user_id=$1 AND game_id='wow'),
          'hours', (SELECT jsonb_agg(to_jsonb(h) ORDER BY h.id) FROM hours_entries h WHERE user_id=$1 AND game_id='wow'),
          'sessions', (SELECT jsonb_agg(to_jsonb(s) ORDER BY s.id) FROM sessions s WHERE user_id=$1 AND game_id='wow'))",
    ).bind(uid).fetch_one(pool).await.unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_optional_surnames_keep_exact_identities_and_retail_intact(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "nosurname@example.com", "nosurname").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle='nosurname'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games(id,name) VALUES('wow','World of Warcraft'),('wow_forever','World of Warcraft: Forever') ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO linked_accounts(user_id,provider,provider_uid,display,verified,scopes) VALUES($1,'bnet','optional-surname','For#1',true,'openid wow.profile')")
        .bind(uid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO wow_characters(id,user_id,game_id,region,realm,name,played_s,played_at) VALUES(gen_random_uuid(),$1,'wow','us','Area 52','Vale',7200,now())")
        .bind(uid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO hours_entries(id,user_id,game_id,tier,source,minutes,captured_at) VALUES(gen_random_uuid(),$1,'wow','imported','wow_addon',400,now())")
        .bind(uid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sessions(id,user_id,game_id,started_at,ended_at,duration_s) VALUES(gen_random_uuid(),$1,'wow',now()-interval '1 hour',now(),900)")
        .bind(uid).execute(&pool).await.unwrap();
    let retail_before = retail_snapshot(&pool, uid).await;
    let at = chrono::Utc::now().timestamp() - 120;
    let e = forever("Vale", "Vale", "", 10_800, at, "For#1");
    let first = post(&app, &t, "wow_forever", &e).await;
    assert_eq!(first.status, StatusCode::OK, "{}", first.text);
    assert_eq!(first.json()["accepted"], 1, "{}", first.text);
    assert_eq!(first.json()["verified"], 0);
    assert_eq!(first.json()["total_min"], 180);
    // Missing surname deserializes as empty and signs identically; it is the same character.
    let mut missing = json!(e);
    missing.as_object_mut().unwrap().remove("surname");
    let again = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played",
        Some(&t),
        Some(json!({"game_id":"wow_forever","characters":[missing]})),
    )
    .await;
    assert_eq!(again.json()["accepted"], 0, "{}", again.text);
    assert_eq!(again.json()["unchanged"].as_array().unwrap().len(), 1);
    // A new character with an omitted field is accepted too.
    let mut rina = json!(forever("Rina", "Rina", "", 7200, at, "For#1"));
    rina.as_object_mut().unwrap().remove("surname");
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played",
        Some(&t),
        Some(json!({"game_id":"wow_forever","characters":[rina]})),
    )
    .await;
    assert_eq!(r.json()["accepted"], 1, "{}", r.text);
    let imports_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hours_entries WHERE user_id=$1 AND game_id='wow_forever'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    let legacy_id: uuid::Uuid = sqlx::query_scalar(
        "SELECT id FROM wow_characters WHERE user_id=$1 AND game_id='wow_forever' AND name='Vale'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    // 1.1.1 learns the real surname and signs its GUID. Enrich the same snapshot,
    // same row and same hours, even though the display name has changed.
    let mut full = forever("Vale Mocken", "Vale", "Mocken", 10_800, at, "For#1");
    full.sv = 3;
    full.guid = "Player-70-00000001".into();
    full.sig = sign_with(SIG_KEYS[2].1, &full);
    let r = post(&app, &t, "wow_forever", &full).await;
    assert_eq!(r.json()["accepted"], 0, "{}", r.text);
    assert_eq!(r.json()["unchanged"].as_array().unwrap().len(), 1);
    assert_eq!(r.json()["total_min"], 300, "{}", r.text);
    let new_id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM wow_characters WHERE user_id=$1 AND game_id='wow_forever' AND name='Vale Mocken'")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(
        legacy_id, new_id,
        "surname arrival must not create a second character"
    );
    let imports_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM hours_entries WHERE user_id=$1 AND game_id='wow_forever'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        imports_before, imports_after,
        "surname metadata must not add an import total"
    );
    let trailing = forever("Vale ", "Vale", "", 10_800, at, "For#1");
    let r = post(&app, &t, "wow_forever", &trailing).await;
    assert_eq!(r.json()["accepted"], 0, "{}", r.text);
    assert_eq!(r.json()["unchanged"].as_array().unwrap().len(), 1);
    // Different GUIDs prove these are separate characters despite sharing a first name.
    let mut other = forever("Vale Other", "Vale", "Other", 3600, at, "For#1");
    other.sv = 3;
    other.guid = "Player-70-00000002".into();
    other.sig = sign_with(SIG_KEYS[2].1, &other);
    let r = post(&app, &t, "wow_forever", &other).await;
    assert_eq!(r.json()["accepted"], 1, "{}", r.text);
    assert_eq!(r.json()["total_min"], 360, "{}", r.text);
    let mut lower = full.clone();
    lower.played = 3600;
    lower.at += 30;
    lower.sig = sign_with(SIG_KEYS[2].1, &lower);
    let r = post(&app, &t, "wow_forever", &lower).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "wow_decrease",
        "{}",
        r.text
    );
    // A legacy first-name-only file is now ambiguous. Do not guess or merge.
    let mut unsigned_guid = trailing.clone();
    unsigned_guid.guid = full.guid.clone();
    let r = post(&app, &t, "wow_forever", &unsigned_guid).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "ambiguous_identity",
        "{}",
        r.text
    );
    // GUID tampering is covered by the new checksum, not merely trusted metadata.
    let mut tampered = full.clone();
    tampered.guid = "Player-70-00000009".into();
    let r = post(&app, &t, "wow_forever", &tampered).await;
    assert_eq!(
        r.json()["rejected"][0]["reason"],
        "wow_checksum",
        "{}",
        r.text
    );
    let rows: Vec<(String,String,String,bool,i64)> = sqlx::query_as("SELECT name,first_name,surname,verified,played_s FROM wow_characters WHERE user_id=$1 AND game_id='wow_forever' ORDER BY name")
        .bind(uid).fetch_all(&pool).await.unwrap();
    assert_eq!(
        rows,
        vec![
            ("Rina".into(), "Rina".into(), "".into(), false, 7200),
            (
                "Vale Mocken".into(),
                "Vale".into(),
                "Mocken".into(),
                false,
                10_800
            ),
            (
                "Vale Other".into(),
                "Vale".into(),
                "Other".into(),
                false,
                3600
            )
        ]
    );
    let minutes: i32 = sqlx::query_scalar("SELECT minutes FROM hours_entries WHERE user_id=$1 AND game_id='wow_forever' ORDER BY captured_at DESC,created_at DESC,id DESC LIMIT 1")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(minutes, 360);
    let snaps: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_played_snaps s JOIN wow_characters c ON c.id=s.char_id WHERE c.user_id=$1 AND c.game_id='wow_forever'")
        .bind(uid).fetch_one(&pool).await.unwrap();
    assert_eq!(
        snaps, 3,
        "metadata enrichment and replay must not add a snapshot"
    );
    assert_eq!(retail_before, retail_snapshot(&pool, uid).await);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn optional_surname_does_not_bypass_forever_validation(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "guardforever@example.com", "guardforever").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle='guardforever'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO linked_accounts(user_id,provider,provider_uid,display,verified,scopes) VALUES($1,'bnet','surname-guards','For#1',true,'openid wow.profile')")
        .bind(uid).execute(&pool).await.unwrap();
    let at = chrono::Utc::now().timestamp() - 120;
    let base = forever("Guard", "Guard", "", 7200, at, "For#1");
    let mut bad_sig = base.clone();
    bad_sig.sig = "bad".into();
    let bad_tag = forever("Wrongtag", "Wrongtag", "", 7200, at, "Other#9");
    let beta = flavoured(base.clone(), "forever", 1, 1);
    let unconfirmed = flavoured(base.clone(), "forever", 0, 0);
    let mut wrong_project = base.clone();
    wrong_project.project = 1;
    wrong_project.sig = sign_with(SIG_KEYS[1].1, &wrong_project);
    let missing_first = forever("Missing", "", "", 7200, at, "For#1");
    for (entry, reason) in [
        (bad_sig, "wow_checksum"),
        (bad_tag, "wow_battletag"),
        (beta, "forever_beta"),
        (unconfirmed, "forever_unconfirmed"),
        (wrong_project, "incomplete_identity"),
        (missing_first, "incomplete_identity"),
    ] {
        let r = post(&app, &t, "wow_forever", &entry).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        assert_eq!(r.json()["accepted"], 0, "{}", r.text);
        assert_eq!(r.json()["rejected"][0]["reason"], reason, "{}", r.text);
    }
    let chars: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE user_id=$1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(chars, 0);
    let minutes: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(minutes),0)::bigint FROM hours_entries WHERE user_id=$1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(minutes, 0);
}
