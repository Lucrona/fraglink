mod common;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use fraglink_core::wow_played::{sign_with, CharEntry, SIG_KEYS};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

fn entry(name: &str, realm: &str, played: i64, at: i64, tag: &str) -> CharEntry {
    let mut e = CharEntry {
        region: "us".into(),
        realm: realm.into(),
        name: name.into(),
        class: "SHAMAN".into(),
        level: 80,
        played,
        at,
        sv: SIG_KEYS[0].0,
        battletag: tag.into(),
        flavour: String::new(),
        project: 0,
        build: String::new(),
        first: String::new(),
        surname: String::new(),
        guid: String::new(),
        beta: 0,
        confirmed: 0,
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

async fn link(pool: &PgPool, handle: &str, tag: &str) -> Uuid {
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
         VALUES($1, 'bnet', $2, $3, true, 'openid wow.profile')",
    )
    .bind(uid)
    .bind(format!("uid-{handle}"))
    .bind(tag)
    .execute(pool)
    .await
    .unwrap();
    uid
}

async fn account(
    pool: &PgPool,
    uid: Uuid,
    game: &str,
    slug: &str,
    realm: &str,
    name: &str,
    bnet_id: Option<i64>,
) {
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, realm_name, name, bnet_id)
         VALUES($1, $2, 'us', $3, $4, $5, $6)",
    )
    .bind(uid)
    .bind(game)
    .bind(slug)
    .bind(realm)
    .bind(name)
    .bind(bnet_id)
    .execute(pool)
    .await
    .unwrap();
}

async fn names(pool: &PgPool, uid: Uuid) -> Vec<(String, String, i64)> {
    sqlx::query_as(
        "SELECT name, realm, played_s FROM wow_characters WHERE user_id = $1 ORDER BY name",
    )
    .bind(uid)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn rename_keeps_hours_snapshots_and_is_idempotent(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "rn@example.com", "renamer").await;
    let uid = link(&pool, "renamer", "Thrall#1234").await;
    account(
        &pool,
        uid,
        "wow",
        "area-52",
        "Area 52",
        "Thrall",
        Some(4242),
    )
    .await;
    let now = Utc::now().timestamp() - 120;
    let r = up(
        &app,
        &t,
        &[entry(
            "Thrall",
            "Area 52",
            360_000,
            now - 7200,
            "Thrall#1234",
        )],
    )
    .await;
    assert_eq!(r["accepted"], 1, "{r}");
    let before: (Uuid, i64, i64) = sqlx::query_as(
        "SELECT c.id, c.played_s, (SELECT count(*) FROM wow_played_snaps s WHERE s.char_id = c.id)
         FROM wow_characters c WHERE c.user_id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    let hours_before: Vec<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, minutes FROM hours_entries WHERE user_id = $1 AND game_id = 'wow' ORDER BY created_at, id",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(!hours_before.is_empty());

    sqlx::query("UPDATE bnet_characters SET name = 'Mockrend' WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let st = state(pool.clone());
    fraglink_server::wow::reverify(&st, uid).await;
    fraglink_server::wow::reverify(&st, uid).await;

    let after: (Uuid, String, String, i64, i64, bool) = sqlx::query_as(
        "SELECT id, name, realm, played_s,
                (SELECT count(*) FROM wow_played_snaps s WHERE s.char_id = c.id),
                hidden
         FROM wow_characters c WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, "Mockrend");
    assert_eq!(after.2, "Area 52");
    assert_eq!(after.3, before.1);
    assert_eq!(after.4, before.2);
    assert!(!after.5);
    let hours_after: Vec<(Uuid, i32)> = sqlx::query_as(
        "SELECT id, minutes FROM hours_entries WHERE user_id = $1 AND game_id = 'wow' ORDER BY created_at, id",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        hours_after, hours_before,
        "a rename does not rewrite imported totals"
    );
    let history: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM wow_character_names WHERE char_id = $1 ORDER BY recorded_at, id",
    )
    .bind(after.0)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(history, vec!["Thrall".to_string()]);
    let list = api(&app, "GET", "/api/v1/me/wow/list", Some(&t), None).await;
    assert_eq!(list.status, StatusCode::OK, "{}", list.text);
    let body: Value = serde_json::from_str(&list.text).unwrap();
    assert_eq!(body["items"][0]["former_name"], "Thrall");
    assert_eq!(body["items"][0]["name"], "Mockrend");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn rename_plus_new_upload_keeps_the_higher_played(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "mg@example.com", "merger").await;
    let uid = link(&pool, "merger", "Thrall#1234").await;
    account(&pool, uid, "wow", "area-52", "Area 52", "Thrall", Some(77)).await;
    let now = Utc::now().timestamp() - 120;
    up(
        &app,
        &t,
        &[entry(
            "Thrall",
            "Area 52",
            7_200,
            now - 100_000,
            "Thrall#1234",
        )],
    )
    .await;
    sqlx::query("UPDATE wow_characters SET hidden = true, show_guild = true, featured = 1 WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let old_hours: i64 =
        sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("UPDATE bnet_characters SET name = 'Mockrend' WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let r = up(
        &app,
        &t,
        &[entry(
            "Mockrend",
            "Area 52",
            14_400,
            now - 50_000,
            "Thrall#1234",
        )],
    )
    .await;
    assert_eq!(r["total_min"], 240, "{r}");
    assert_ne!(
        r["total_min"], 360,
        "the two /played counters must not be added"
    );
    let row: (String, i64, bool, bool, Option<i16>, i64) = sqlx::query_as(
        "SELECT name, played_s, hidden, show_guild, featured,
                (SELECT count(*) FROM wow_played_snaps s WHERE s.char_id = c.id)
         FROM wow_characters c WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "Mockrend");
    assert_eq!(row.1, 14_400);
    assert!(row.2, "the original row's hidden flag stays");
    assert!(row.3);
    assert_eq!(row.4, Some(1));
    assert_eq!(row.5, 2, "snapshots from both rows are kept");
    let chars: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(chars, 1);
    let hours: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        hours > old_hours,
        "the earlier imported total is still stored"
    );
    let minutes: Vec<i32> = sqlx::query_scalar(
        "SELECT minutes FROM hours_entries WHERE user_id = $1 ORDER BY created_at, id",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(minutes.contains(&120), "{minutes:?}");
    assert!(minutes.contains(&240), "{minutes:?}");
    assert!(!minutes.contains(&360), "{minutes:?}");
    let merges: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_character_merges WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(merges, 1);
    let st = state(pool.clone());
    fraglink_server::wow::reverify(&st, uid).await;
    let merges_again: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_character_merges WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(merges_again, 1);
    assert_eq!(
        names(&pool, uid).await,
        vec![("Mockrend".into(), "Area 52".into(), 14_400)]
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn realm_transfer_keeps_the_same_row(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "rt@example.com", "realmer").await;
    let uid = link(&pool, "realmer", "Thrall#1234").await;
    account(&pool, uid, "wow", "area-52", "Area 52", "Thrall", Some(88)).await;
    let now = Utc::now().timestamp() - 120;
    up(
        &app,
        &t,
        &[entry("Thrall", "Area 52", 3_600, now - 7200, "Thrall#1234")],
    )
    .await;
    let id: Uuid = sqlx::query_scalar("SELECT id FROM wow_characters WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE bnet_characters SET realm_slug = 'moon-guard', realm_name = 'Moon Guard' WHERE user_id = $1",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    fraglink_server::wow::reverify(&state(pool.clone()), uid).await;
    let row: (Uuid, String, String, String) =
        sqlx::query_as("SELECT id, name, realm, realm_slug FROM wow_characters WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, id);
    assert_eq!(
        (row.1.as_str(), row.2.as_str(), row.3.as_str()),
        ("Thrall", "Moon Guard", "moon-guard")
    );
    let snaps: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_played_snaps WHERE char_id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(snaps, 1);
    let list = api(&app, "GET", "/api/v1/me/wow/list", Some(&t), None).await;
    let body: Value = serde_json::from_str(&list.text).unwrap();
    assert_eq!(body["items"][0]["former_realm"], "Area 52");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn different_owner_same_id_is_not_merged(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let ta = signup(&app, "oa@example.com", "ownera").await;
    let tb = signup(&app, "ob@example.com", "ownerb").await;
    let a = link(&pool, "ownera", "Alpha#1").await;
    let b = link(&pool, "ownerb", "Bravo#2").await;
    account(&pool, a, "wow", "area-52", "Area 52", "Oldname", Some(10)).await;
    account(&pool, b, "wow", "area-52", "Area 52", "Renamed", Some(10)).await;
    let now = Utc::now().timestamp() - 120;
    up(
        &app,
        &ta,
        &[entry("Oldname", "Area 52", 5_000, now - 8000, "Alpha#1")],
    )
    .await;
    // Bravo's file uses Bravo's tag. Give Bravo the id on a different current name.
    sqlx::query("DELETE FROM bnet_characters WHERE user_id = $2 AND name = 'Renamed'")
        .bind(a)
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    account(&pool, b, "wow", "area-52", "Area 52", "Midname", Some(10)).await;
    up(
        &app,
        &tb,
        &[entry("Midname", "Area 52", 8_000, now - 7000, "Bravo#2")],
    )
    .await;
    account(&pool, b, "wow", "area-52", "Area 52", "Renamed", Some(10)).await;
    sqlx::query("DELETE FROM bnet_characters WHERE user_id = $1 AND name = 'Midname'")
        .bind(b)
        .execute(&pool)
        .await
        .unwrap();
    let a_before = names(&pool, a).await;
    let b_snaps: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_snaps s JOIN wow_characters c ON c.id = s.char_id WHERE c.user_id = $1",
    )
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    fraglink_server::wow::reverify(&state(pool.clone()), b).await;
    assert_eq!(
        names(&pool, a).await,
        a_before,
        "the other owner's row is not renamed or removed"
    );
    assert_eq!(
        names(&pool, b).await,
        vec![("Midname".into(), "Area 52".into(), 8_000)]
    );
    let b_snaps_after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_snaps s JOIN wow_characters c ON c.id = s.char_id WHERE c.user_id = $1",
    )
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(b_snaps_after, b_snaps);
    let held: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_quarantine WHERE user_id = $1 AND reason = 'rename_other_owner'",
    )
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(held, 1);
    fraglink_server::wow::reverify(&state(pool.clone()), b).await;
    let held_again: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_quarantine WHERE user_id = $1 AND reason = 'rename_other_owner'",
    )
    .bind(b)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(held_again, 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn unverified_and_inconsistent_counters_do_not_merge(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "uv@example.com", "unverified").await;
    let uid = link(&pool, "unverified", "Thrall#1234").await;
    account(&pool, uid, "wow", "area-52", "Area 52", "Thrall", Some(55)).await;
    let now = Utc::now().timestamp() - 120;
    up(
        &app,
        &t,
        &[entry(
            "Thrall",
            "Area 52",
            9_000,
            now - 20_000,
            "Thrall#1234",
        )],
    )
    .await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, played_s, played_at, battletag, verified, ever_verified)
         VALUES($1, $2, 'wow', 'us', 'Area 52', 'area-52', 'Mockrend', 12000, $3, '', false, false)",
    )
    .bind(id)
    .bind(uid)
    .bind(Utc::now() - Duration::hours(2))
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE bnet_characters SET name = 'Mockrend' WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::wow::reverify(&state(pool.clone()), uid).await;
    let left = names(&pool, uid).await;
    assert!(left.iter().any(|r| r.0 == "Thrall"), "{left:?}");
    assert!(left.iter().any(|r| r.0 == "Mockrend"), "{left:?}");
    let why: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_played_quarantine WHERE user_id = $1 AND reason = 'rename_unverified'",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(why, 1);

    // A newer counter that is lower than the older one is not combined.
    let t2 = signup(&app, "ct@example.com", "counters").await;
    let u2 = link(&pool, "counters", "Counter#9").await;
    account(&pool, u2, "wow", "area-52", "Area 52", "Newer", Some(91)).await;
    let old_id = Uuid::new_v4();
    let new_id = Uuid::new_v4();
    for (id, name, played, hours_ago) in [
        (old_id, "Older", 20_000_i64, 30_i64),
        (new_id, "Newer", 10_000_i64, 1_i64),
    ] {
        sqlx::query(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, played_s, played_at, battletag, verified, ever_verified, bnet_id)
             VALUES($1, $2, 'wow', 'us', 'Area 52', 'area-52', $3, $4, $5, 'Counter#9', true, true, 91)",
        )
        .bind(id)
        .bind(u2)
        .bind(name)
        .bind(played)
        .bind(Utc::now() - Duration::hours(hours_ago))
        .execute(&pool)
        .await
        .unwrap();
    }
    let _ = t2;
    fraglink_server::wow::reverify(&state(pool.clone()), u2).await;
    assert_eq!(names(&pool, u2).await.len(), 2);
    let held: String =
        sqlx::query_scalar("SELECT reason FROM wow_played_quarantine WHERE user_id = $1")
            .bind(u2)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(held, "rename_counters");

    // No Blizzard id on the account list: nothing is combined.
    let t3 = signup(&app, "id@example.com", "noid").await;
    let u3 = link(&pool, "noid", "Noid#1").await;
    account(&pool, u3, "wow", "area-52", "Area 52", "One", None).await;
    for name in ["One", "Two"] {
        sqlx::query(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, played_s, played_at, battletag, verified, ever_verified)
             VALUES($1, $2, 'wow', 'us', 'Area 52', 'area-52', $3, 1000, now(), 'Noid#1', true, true)",
        )
        .bind(Uuid::new_v4())
        .bind(u3)
        .bind(name)
        .execute(&pool)
        .await
        .unwrap();
    }
    let _ = t3;
    fraglink_server::wow::reverify(&state(pool.clone()), u3).await;
    assert_eq!(names(&pool, u3).await.len(), 2);
    let merges: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_character_merges WHERE user_id = $1")
            .bind(u3)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(merges, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_rename_stays_off_until_the_account_flag(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let _t = signup(&app, "fv@example.com", "foreverer").await;
    let uid = link(&pool, "foreverer", "Vale#1").await;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, played_s, played_at, battletag, verified, ever_verified, bnet_id, first_name, surname)
         VALUES($1, $2, 'wow_forever', 'us', '', '', 'Vale Mocken', 5000, now(), 'Vale#1', true, true, 18, 'Vale', 'Mocken')",
    )
    .bind(id)
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    account(&pool, uid, "wow_forever", "", "", "Vale Renamed", Some(18)).await;
    let st = state(pool.clone());
    fraglink_server::wow::reverify(&st, uid).await;
    let name: String = sqlx::query_scalar("SELECT name FROM wow_characters WHERE id = $1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        name, "Vale Mocken",
        "the account flag is off, so Forever is not renamed"
    );
    fraglink_server::wow::reconcile_linked_characters(&st, uid, true)
        .await
        .unwrap();
    let row: (String, String, String) =
        sqlx::query_as("SELECT name, first_name, surname FROM wow_characters WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row.0, "Vale Renamed");
    assert_eq!(row.1, "Vale");
    assert_eq!(row.2, "Renamed");
    fraglink_server::wow::reconcile_linked_characters(&st, uid, true)
        .await
        .unwrap();
    let history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_character_names WHERE char_id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(history, 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn migration_leaves_existing_characters_alone(pool: PgPool) {
    let sql = include_str!("../migrations/0057_wow_rename_merge.sql").to_ascii_lowercase();
    for line in sql.lines() {
        let t = line.trim();
        if t.starts_with("--") {
            continue;
        }
        // A foreign key's ON DELETE is not a statement that rewrites existing rows.
        let bare = t.replace("on delete cascade", "");
        for word in ["update ", "delete ", "insert ", "truncate ", "drop "] {
            assert!(!bare.contains(word), "{word} in: {line}");
        }
    }
    let app = fraglink_server::router(state(pool.clone()));
    let _t = signup(&app, "mi@example.com", "migrated").await;
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'migrated'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, played_s, played_at, bnet_id)
         VALUES($1, $2, 'wow', 'us', 'Area 52', 'area-52', 'Keepme', 4321, now(), 999)",
    )
    .bind(id)
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    let before: (String, i64, Option<i64>) =
        sqlx::query_as("SELECT name, played_s, bnet_id FROM wow_characters WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::raw_sql(include_str!("../migrations/0057_wow_rename_merge.sql"))
        .execute(&pool)
        .await
        .unwrap();
    let after: (String, i64, Option<i64>) =
        sqlx::query_as("SELECT name, played_s, bnet_id FROM wow_characters WHERE id = $1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    assert_eq!(before.0, "Keepme");
    let empty: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM wow_character_names) + (SELECT count(*) FROM wow_character_merges)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(empty, 0);
}
