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

fn lua(es: &[CharEntry]) -> String {
    let mut s = String::from("FraglinkPlayedDB = {\n\t[\"chars\"] = {\n");
    for e in es {
        s += &format!(
            "\t\t[\"{r}/{m}/{n}\"] = {{ [\"at\"] = {at}, [\"battletag\"] = \"{t}\", [\"class\"] = \"{c}\", [\"level\"] = {l}, [\"name\"] = \"{n}\", [\"played\"] = {p}, [\"realm\"] = \"{m}\", [\"region\"] = \"{r}\", [\"sig\"] = \"{g}\", [\"sv\"] = {sv}, }},\n",
            r = e.region, m = e.realm, n = e.name, at = e.at, t = e.battletag, c = e.class,
            l = e.level, p = e.played, g = e.sig, sv = e.sv
        );
    }
    s + "\t},\n}\n"
}

async fn verified(pool: &PgPool) -> Vec<(String, bool)> {
    sqlx::query_as("SELECT name, verified FROM wow_characters ORDER BY name")
        .fetch_all(pool)
        .await
        .unwrap()
}

/// One PC, several WTF\Account folders: Battle.net A with two WoW licences (WoW1, WoW2:
/// same BattleTag) and a sibling's Battle.net B. Only B's entries are refused; A's from
/// both licences are stored and verified. The same file through the website does the same.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn several_accounts_on_one_pc(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "m@example.com", "multiacct").await;
    let now = chrono::Utc::now().timestamp() - 60;
    let file = [
        entry("Main", 360_000, now - 7200, "Thrall#1234"), // WoW1
        entry("Second", 7_200, now - 7200, "Thrall#1234"), // WoW2, same Battle.net
        entry("Sibling", 99_000, now - 7200, "Jaina#5555"), // another Battle.net
    ];
    // nothing linked yet: all three are kept as self-reported
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played/file",
        Some(&t),
        Some(json!({ "game_id": "wow", "text": lua(&file) })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v: Value = serde_json::from_str(&r.text).unwrap();
    assert_eq!(
        (v["found"].as_i64(), v["accepted"].as_i64()),
        (Some(3), Some(3)),
        "{v}"
    );
    assert!(verified(&pool).await.iter().all(|c| !c.1));

    // link Battle.net A with Main and Second on it (and, as a test, Sibling too):
    // Sibling still doesn't verify, its stored entry carries B's BattleTag
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = 'multiacct'")
        .execute(&pool).await.unwrap();
    for n in ["Main", "Second", "Sibling"] {
        sqlx::query(
            "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                     SELECT id, 'wow', 'us', 'area-52', $1 FROM users WHERE handle = 'multiacct'",
        )
        .bind(n)
        .execute(&pool)
        .await
        .unwrap();
    }
    let o = api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert_eq!(o.status, StatusCode::OK, "{}", o.text);
    assert_eq!(
        verified(&pool).await,
        vec![
            ("Main".into(), true),
            ("Second".into(), true),
            ("Sibling".into(), false)
        ]
    );

    // the next file from that PC: B's newer entry is refused, A's go through
    let file2 = [
        entry("Main", 361_000, now - 3600, "Thrall#1234"),
        entry("Sibling", 100_000, now - 3600, "Jaina#5555"),
    ];
    let v = up(&app, &t, &file2).await;
    assert_eq!(v["accepted"], 1, "{v}");
    assert_eq!(v["rejected"][0]["reason"], "wow_battletag", "{v}");
    // sending it again: Main unchanged, B refused again but flagged once per character
    let v = up(&app, &t, &file2).await;
    assert_eq!(v["unchanged"].as_array().unwrap().len(), 1, "{v}");
    let flags: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_flags WHERE kind = 'wow_battletag'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(flags, 1);
    // leaderboard total counts A only through `verified`; the profile shows all
    let main: i64 = sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE name = 'Main'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(main, 361_000);

    // the link now points at another Battle.net (relink): nothing stays verified
    sqlx::query("UPDATE linked_accounts SET display = 'Other#1', provider_uid = '43'")
        .execute(&pool)
        .await
        .unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert!(verified(&pool).await.iter().all(|c| !c.1));

    // a second Fraglink account can't hold the same verified Battle.net link
    signup(&app, "s@example.com", "second1").await;
    let dup = sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified)
                 SELECT id, 'bnet', '43', 'Other#1', true FROM users WHERE handle = 'second1'",
    )
    .execute(&pool)
    .await;
    assert!(dup.is_err(), "same Battle.net on two Fraglink accounts");

    // junk and oversize files
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played/file",
        Some(&t),
        Some(json!({ "game_id": "wow", "text": "hello" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played/file",
        Some(&t),
        Some(json!({ "game_id": "wow", "text": "x".repeat(1_100_000) })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

async fn wow_total(pool: &PgPool) -> Option<i32> {
    sqlx::query_scalar(
        "SELECT minutes FROM hours_entries WHERE source = 'wow_addon' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .unwrap()
}

/// The site-settings flag cache is process-wide: tests switching flags take turns.
static FLAGS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Only Battle.net-verified characters count toward hours and show on the profile.
/// Unverified ones are stored and start counting once they verify.

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn only_verified_characters_count(pool: PgPool) {
    let _flags = FLAGS.lock().await;
    fraglink_server::features::forget();
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "v@example.com", "onlyverified").await;
    let now = chrono::Utc::now().timestamp() - 60;

    // nothing linked: stored, but no hours at all
    let r = up(
        &app,
        &t,
        &[
            entry("Thrall", 360_000, now - 7200, "Thrall#1234"),
            entry("Alt", 36_000, now - 7200, "Thrall#1234"),
        ],
    )
    .await;
    assert_eq!(r["accepted"], 2, "{r}");
    assert_eq!(r["total_min"], 0, "{r}");
    assert_eq!(wow_total(&pool).await, None);

    // link Battle.net with only Thrall on it: Thrall counts, Alt doesn't
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = 'onlyverified'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                 SELECT id, 'wow', 'us', 'area-52', 'Thrall' FROM users WHERE handle = 'onlyverified'")
        .execute(&pool).await.unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert_eq!(wow_total(&pool).await, Some(6_000));

    // character details are off by default (Admin > Site settings): no cards on the profile
    let get_profile = || async {
        serde_json::from_str::<Value>(
            &api(
                &app,
                "GET",
                "/api/v1/users/onlyverified/profile",
                Some(&t),
                None,
            )
            .await
            .text,
        )
        .unwrap()
    };
    assert!(get_profile().await["wow_characters"].is_null());
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES('wow_characters', true)")
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::features::forget();
    // switched on, the profile lists only the verified character
    let p: Value = serde_json::from_str(
        &api(
            &app,
            "GET",
            "/api/v1/users/onlyverified/profile",
            Some(&t),
            None,
        )
        .await
        .text,
    )
    .unwrap();
    let names: Vec<&str> = p["wow_characters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["Thrall"], "{p}");
    // /connect/wow still lists both, so Alt can be verified
    let me: Value = serde_json::from_str(
        &api(&app, "GET", "/api/v1/me/wow", Some(&t), None)
            .await
            .text,
    )
    .unwrap();
    assert_eq!(me["characters"].as_array().unwrap().len(), 2);

    // Alt verifies later: it counts from then on
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                 SELECT id, 'wow', 'us', 'area-52', 'Alt' FROM users WHERE handle = 'onlyverified'",
    )
    .execute(&pool)
    .await
    .unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert_eq!(wow_total(&pool).await, Some(6_600));

    // a character gone from Battle.net (deleted or moved) stops verifying, but the
    // higher total is kept: those hours were played
    sqlx::query("DELETE FROM bnet_characters WHERE name = 'Alt'")
        .execute(&pool)
        .await
        .unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert_eq!(wow_total(&pool).await, Some(6_600));

    // unlinking Battle.net drops the counted hours back to 0
    let r = api(&app, "DELETE", "/api/v1/connect/bnet", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(wow_total(&pool).await, Some(0));
}

async fn shown(pool: &PgPool, handle: &str) -> (i64, i64) {
    sqlx::query_as(
        "SELECT display_min, board_min FROM user_game_stats s JOIN users u ON u.id = s.user_id
         WHERE u.handle = $1 AND s.game_id = 'wow'",
    )
    .bind(handle)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A lower WoW upload never lowers the shown total or the board; staff can recount.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn lower_uploads_keep_the_higher_total(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "k@example.com", "keephigher").await;
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = 'keephigher'")
        .execute(&pool).await.unwrap();
    for n in ["Thrall", "Alt"] {
        sqlx::query(
            "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                     SELECT id, 'wow', 'us', 'area-52', $1 FROM users WHERE handle = 'keephigher'",
        )
        .bind(n)
        .execute(&pool)
        .await
        .unwrap();
    }
    let now = chrono::Utc::now().timestamp() - 60;
    // first upload: 100 h + 10 h
    let r = up(
        &app,
        &t,
        &[
            entry("Thrall", 360_000, now - 4 * 3600, "Thrall#1234"),
            entry("Alt", 36_000, now - 4 * 3600, "Thrall#1234"),
        ],
    )
    .await;
    assert_eq!(r["total_min"], 6_600, "{r}");
    assert_eq!(shown(&pool, "keephigher").await, (6_600, 6_600));
    // backdate the import so tracked play can land after it, then track 2 h
    sqlx::query("UPDATE hours_entries SET captured_at = now() - interval '3 hours'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
                 SELECT gen_random_uuid(), id, 'wow', now() - interval '150 minutes', now() - interval '30 minutes', 7200, 'live'
                 FROM users WHERE handle = 'keephigher'")
        .execute(&pool).await.unwrap();
    assert_eq!(shown(&pool, "keephigher").await, (6_720, 6_720));

    // partial upload (only Thrall, +1 h): Alt keeps its hours. /played rose 1 h against 2 h
    // tracked, so the extra hour (menus etc.) is carried forward: 6 660 + 60
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 363_600, now - 1800, "Thrall#1234")],
    )
    .await;
    assert_eq!(r["total_min"], 6_660, "{r}");
    assert!(r["kept_min"].is_null(), "{r}");
    assert_eq!(shown(&pool, "keephigher").await, (6_720, 6_720));

    // a /played that goes down is refused per character; the total stays
    let r = up(&app, &t, &[entry("Alt", 30_000, now, "Thrall#1234")]).await;
    assert_eq!(r["rejected"][0]["reason"], "wow_decrease", "{r}");
    assert_eq!(shown(&pool, "keephigher").await, (6_720, 6_720));
    // an upload adding nothing is not stored and lowers nothing
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 363_600, now - 1800, "Thrall#1234")],
    )
    .await;
    assert!(r["kept_min"].is_null() || r["kept_min"] == 6_720, "{r}");
    assert_eq!(shown(&pool, "keephigher").await, (6_720, 6_720));

    // a higher upload adds on; the carried hour stays, tracked play isn't counted twice
    let r = up(
        &app,
        &t,
        &[entry("Alt", 36_000 + 3 * 3600, now, "Thrall#1234")],
    )
    .await;
    assert_eq!(r["total_min"], 6_840, "{r}");
    assert_eq!(shown(&pool, "keephigher").await, (6_900, 6_900));

    // a deleted character (on no Battle.net listing any more) keeps its hours, even through
    // a recount, which only drops the carried extra
    sqlx::query("DELETE FROM bnet_characters WHERE name = 'Alt'")
        .execute(&pool)
        .await
        .unwrap();
    api(&app, "POST", "/api/v1/wow/verify/match", Some(&t), None).await;
    assert_eq!(shown(&pool, "keephigher").await, (6_900, 6_900));
    let r = staff_recount(&app, &pool, "keephigher", "wow").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(shown(&pool, "keephigher").await, (6_840, 6_840));
    let r = staff_recount(&app, &pool, "keephigher", "nope").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

/// A user with Thrall and Alt on a linked Battle.net, uploaded 8 hours ago (100 h + 10 h).
async fn two_chars(app: &axum::Router, pool: &PgPool, handle: &str) -> (String, i64) {
    let t = signup(app, &format!("{handle}@example.com"), handle).await;
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', '42', 'Thrall#1234', true, 'openid wow.profile' FROM users WHERE handle = $1")
        .bind(handle).execute(pool).await.unwrap();
    for n in ["Thrall", "Alt"] {
        sqlx::query(
            "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name)
                     SELECT id, 'wow', 'us', 'area-52', $2 FROM users WHERE handle = $1",
        )
        .bind(handle)
        .bind(n)
        .execute(pool)
        .await
        .unwrap();
    }
    let now = chrono::Utc::now().timestamp() - 60;
    let r = up(
        app,
        &t,
        &[
            entry("Thrall", 360_000, now - 8 * 3600, "Thrall#1234"),
            entry("Alt", 36_000, now - 8 * 3600, "Thrall#1234"),
        ],
    )
    .await;
    assert_eq!(r["total_min"], 6_600, "{r}");
    sqlx::query("UPDATE hours_entries SET captured_at = now() - interval '8 hours'")
        .execute(pool)
        .await
        .unwrap();
    (t, now)
}

async fn track(pool: &PgPool, handle: &str, from_h: f64, secs: i64) {
    sqlx::query("INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
                 SELECT gen_random_uuid(), id, 'wow', now() - make_interval(secs => $2),
                        now() - make_interval(secs => $2) + make_interval(secs => $3), $3::int, 'live'
                 FROM users WHERE handle = $1")
        .bind(handle).bind(from_h * 3600.0).bind(secs as f64).execute(pool).await.unwrap();
}

/// /played +5 h with 6 h tracked: the extra hour (menus, loading) is kept, so +6 h.
/// More uploads keep adding their extra; a recount starts again from /played.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn menu_time_is_carried_forward(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let h = "menutime";
    let (t, now) = two_chars(&app, &pool, h).await;
    track(&pool, h, 7.5, 6 * 3600).await;
    assert_eq!(shown(&pool, h).await, (6_960, 6_960));
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 360_000 + 5 * 3600, now, "Thrall#1234")],
    )
    .await;
    assert_eq!(r["total_min"], 6_900, "{r}");
    assert_eq!(shown(&pool, h).await, (6_960, 6_960));

    // second upload: Alt +10 min with 50 min tracked: 40 more carried, 100 in all
    sqlx::query(
        "UPDATE hours_entries SET captured_at = now() - interval '1 hour'
                 WHERE created_at = (SELECT max(created_at) FROM hours_entries)",
    )
    .execute(&pool)
    .await
    .unwrap();
    track(&pool, h, 0.9, 3000).await;
    let r = up(&app, &t, &[entry("Alt", 36_000 + 600, now, "Thrall#1234")]).await;
    assert_eq!(r["total_min"], 6_910, "{r}");
    assert_eq!(shown(&pool, h).await, (7_010, 7_010));

    // a recount drops the carried extra
    let r = staff_recount(&app, &pool, "menutime", "wow").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(shown(&pool, h).await, (6_910, 6_910));
}

/// /played +5 h with only 3 h tracked (played with the app closed): +5 h, nothing carried.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn played_more_than_tracked_carries_nothing(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let h = "offlineplay";
    let (t, now) = two_chars(&app, &pool, h).await;
    track(&pool, h, 7.0, 3 * 3600).await;
    let r = up(
        &app,
        &t,
        &[entry("Thrall", 360_000 + 5 * 3600, now, "Thrall#1234")],
    )
    .await;
    assert_eq!(r["total_min"], 6_900, "{r}");
    assert_eq!(shown(&pool, h).await, (6_900, 6_900));
    let c: i32 =
        sqlx::query_scalar("SELECT carry_min FROM hours_entries ORDER BY created_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(c, 0);
}

#[sqlx::test]
async fn featured_characters_lead_the_profile(pool: PgPool) {
    let _flags = FLAGS.lock().await;
    fraglink_server::features::forget();
    let app = fraglink_server::router(state(pool.clone()));
    let (t, _) = two_chars(&app, &pool, "feat").await;
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES('wow_characters', true)")
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::features::forget();
    let names = || async {
        fraglink_server::features::forget(); // the flag cache is shared with parallel tests
        let p: Value = serde_json::from_str(
            &api(&app, "GET", "/api/v1/users/feat/profile", Some(&t), None)
                .await
                .text,
        )
        .unwrap();
        p["wow_characters"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["hidden"] != true)
            .map(|c| c["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    let id_of = |n: &str| {
        let pool = pool.clone();
        let n = n.to_string();
        async move {
            sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM wow_characters WHERE name = $1")
                .bind(n)
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    let (thrall, alt) = (id_of("Thrall").await, id_of("Alt").await);
    let feature = |ids: Vec<uuid::Uuid>, g: &str| {
        let body = json!({ "game_id": g, "ids": ids });
        let (app, t) = (app.clone(), t.clone());
        async move {
            api(
                &app,
                "POST",
                "/api/v1/me/wow/featured",
                Some(&t),
                Some(body),
            )
            .await
            .status
        }
    };

    // none picked: by hours
    assert_eq!(names().await, ["Thrall", "Alt"]);
    // picked: in the chosen order, ahead of hours
    assert_eq!(feature(vec![alt, thrall], "wow").await, 200);
    assert_eq!(names().await, ["Alt", "Thrall"]);
    let f: Value = serde_json::from_str(
        &api(&app, "GET", "/api/v1/me/wow/featured", Some(&t), None)
            .await
            .text,
    )
    .unwrap();
    assert_eq!(f["wow"][0]["name"], "Alt", "{f}");
    assert_eq!(f["wow_forever"], json!([]), "{f}");
    // bad picks are refused and change nothing
    assert_eq!(feature(vec![alt, alt], "wow").await, 400);
    assert_eq!(feature(vec![alt, thrall, alt, thrall], "wow").await, 400);
    assert_eq!(
        feature(vec![alt], "wow_forever").await,
        400,
        "other flavour"
    );
    assert_eq!(feature(vec![alt], "wow_classic").await, 400);
    assert_eq!(feature(vec![uuid::Uuid::new_v4()], "wow").await, 400);
    assert_eq!(names().await, ["Alt", "Thrall"]);
    // hiding a featured character drops its pick; hidden ones can't be featured
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/me/wow/{alt}"),
        Some(&t),
        Some(json!({ "hidden": true })),
    )
    .await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(names().await, ["Thrall"]);
    assert_eq!(feature(vec![alt], "wow").await, 400);
    let left: Vec<Option<i16>> =
        sqlx::query_scalar("SELECT featured FROM wow_characters ORDER BY name")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(left, [None, Some(2)]);
    // clearing goes back to hours
    assert_eq!(feature(vec![], "wow").await, 200);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE featured IS NOT NULL")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 0);
}

fn entry_on(realm: &str, name: &str, played: i64, at: i64, tag: &str) -> CharEntry {
    let mut e = entry(name, played, at, tag);
    e.realm = realm.into();
    e.sig = sign_with(SIG_KEYS[0].1, &e);
    e
}

/// Link Battle.net for `handle` under `tag` (no listing yet).
async fn link(app: &axum::Router, pool: &PgPool, handle: &str, uid: &str, tag: &str) -> String {
    let t = signup(app, &format!("{handle}@example.com"), handle).await;
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
                 SELECT id, 'bnet', $2, $3, true, 'openid wow.profile' FROM users WHERE handle = $1")
        .bind(handle).bind(uid).bind(tag).execute(pool).await.unwrap();
    t
}

/// Replace `handle`'s Battle.net listing (as a sync does), `ago_h` hours old.
async fn listing(pool: &PgPool, handle: &str, chars: &[(&str, &str, i64)], ago_h: i32) {
    sqlx::query(
        "DELETE FROM bnet_characters WHERE user_id = (SELECT id FROM users WHERE handle = $1)",
    )
    .bind(handle)
    .execute(pool)
    .await
    .unwrap();
    for (slug, name, id) in chars {
        sqlx::query(
            "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, name, bnet_id, fetched_at)
             SELECT id, 'wow', 'us', $2, $3, $4, now() - make_interval(hours => $5) FROM users WHERE handle = $1",
        )
        .bind(handle)
        .bind(slug)
        .bind(name)
        .bind(id)
        .bind(ago_h)
        .execute(pool)
        .await
        .unwrap();
    }
}

async fn total_of(pool: &PgPool, handle: &str) -> i64 {
    shown(pool, handle).await.0
}

/// A rename and a realm transfer keep Battle.net's character id: still one character,
/// counted once, with the higher /played.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn renames_and_realm_transfers_are_one_character(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let h = "renamer";
    let t = link(&app, &pool, h, "50", "Ren#1").await;
    let now = chrono::Utc::now().timestamp() - 60;
    listing(&pool, h, &[("area-52", "Zqxoldname", 9001)], 0).await;
    let r = up(
        &app,
        &t,
        &[entry("Zqxoldname", 360_000, now - 7200, "Ren#1")],
    )
    .await;
    assert_eq!(r["total_min"], 6_000, "{r}");

    // renamed: Battle.net lists the same id under the new name
    listing(&pool, h, &[("area-52", "Zqxnewname", 9001)], 0).await;
    let r = up(
        &app,
        &t,
        &[entry("Zqxnewname", 361_800, now - 3600, "Ren#1")],
    )
    .await;
    assert_eq!(r["total_min"], 6_030, "{r}");
    assert_eq!(total_of(&pool, h).await, 6_030);

    // moved to another realm: same id again
    listing(&pool, h, &[("moon-guard", "Zqxnewname", 9001)], 0).await;
    let r = up(
        &app,
        &t,
        &[entry_on("Moon Guard", "Zqxnewname", 363_600, now, "Ren#1")],
    )
    .await;
    assert_eq!(r["total_min"], 6_060, "{r}");
    assert_eq!(total_of(&pool, h).await, 6_060);
    // the old rows stay in the list but don't count
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE bnet_id = 9001")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 3);
}

/// Sold (or moved to another Battle.net): the user Battle.net lists it under now gets the
/// hours, and it leaves the old owner's total, with no menu-time extra from the gap.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn a_sold_character_moves_to_its_new_owner(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let now = chrono::Utc::now().timestamp() - 60;
    let ts = link(&app, &pool, "seller", "60", "Sel#1").await;
    listing(
        &pool,
        "seller",
        &[("area-52", "Zqxsold", 9002), ("area-52", "Zqxkept", 9003)],
        24,
    )
    .await;
    let r = up(
        &app,
        &ts,
        &[
            entry("Zqxsold", 360_000, now - 7200, "Sel#1"),
            entry("Zqxkept", 36_000, now - 7200, "Sel#1"),
        ],
    )
    .await;
    assert_eq!(r["total_min"], 6_600, "{r}");
    track(&pool, "seller", 1.0, 3600).await;

    let tb = link(&app, &pool, "buyer", "61", "Buy#1").await;
    listing(&pool, "buyer", &[("area-52", "Zqxsold", 9002)], 0).await;
    let r = up(&app, &tb, &[entry("Zqxsold", 363_600, now, "Buy#1")]).await;
    assert_eq!(r["total_min"], 6_060, "{r}");
    assert_eq!(total_of(&pool, "buyer").await, 6_060);
    // the seller keeps only the character they still have; the tracked hour adds nothing
    assert_eq!(total_of(&pool, "seller").await, 600);
    // and their next upload doesn't bring it back
    let r = up(&app, &ts, &[entry("Zqxkept", 36_600, now, "Sel#1")]).await;
    assert_eq!(r["total_min"], 610, "{r}");
}

/// WoW Forever isn't out: its /played uploads are refused (hours are app-tracked).
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_uploads_are_closed(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = link(&app, &pool, "forever", "70", "For#1").await;
    let now = chrono::Utc::now().timestamp() - 60;
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/wow_played",
        Some(&t),
        Some(json!({ "game_id": "wow_forever", "characters": [entry("Zqx", 3600, now, "For#1")] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert!(r.text.contains("forever_not_open"), "{}", r.text);
}

/// Recount is staff-only (audit-logged); the old user endpoint is gone.
async fn staff_recount(
    app: &axum::Router,
    pool: &PgPool,
    handle: &str,
    game: &str,
) -> common::Resp {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let sh = format!("staff{handle}{n}");
    let st = signup(app, &format!("{sh}@example.com"), &sh).await;
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = $1")
        .bind(&sh)
        .execute(pool)
        .await
        .unwrap();
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap();
    let mine = api(
        app,
        "POST",
        "/api/v1/hours/wow_played/recount",
        Some(&st),
        Some(json!({ "game_id": game })),
    )
    .await;
    assert!(mine.status.is_client_error(), "{}", mine.text);
    let r = api(
        app,
        "POST",
        &format!("/api/v1/admin/users/{id}/wow_recount"),
        Some(&st),
        Some(json!({ "game_id": game })),
    )
    .await;
    if r.status == StatusCode::OK {
        let n: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM admin_audit WHERE action = 'user.wow_recount' AND target = $1",
        )
        .bind(format!("user:{handle}"))
        .fetch_one(pool)
        .await
        .unwrap();
        assert!(n >= 1);
    }
    r
}
