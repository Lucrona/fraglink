mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

async fn names(v: &Value) -> Vec<String> {
    v["wow_characters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn profile_show_filters_the_page_and_keeps_every_row(pool: PgPool) {
    let sql = include_str!("../migrations/0058_wow_default_flavour.sql").to_ascii_lowercase()
        + include_str!("../migrations/0059_wow_profile_none.sql");
    for line in sql.lines() {
        let t = line.trim();
        if t.starts_with("--") {
            continue;
        }
        assert!(
            !t.contains("update ") && !t.contains("delete ") && !t.contains("insert "),
            "{line}"
        );
    }
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "cards@example.com", "cardfan").await;
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES('wow_characters', true)")
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::features::forget();
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'cardfan'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for (name, game, verified, hidden, played) in [
        ("Mockrend", "wow", true, false, 7200_i64),
        ("Samplekin", "wow", false, false, 3600_i64),
        ("Hideme", "wow", true, true, 1800_i64),
        ("Vale Mocken", "wow_forever", false, false, 5000_i64),
    ] {
        sqlx::query(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, class, played_s, played_at, verified, hidden, show_guild)
             VALUES($1, $2, $3, 'us', 'Area 52', 'area-52', $4, 'MAGE', $5, now(), $6, $7, true)",
        )
        .bind(Uuid::new_v4())
        .bind(uid)
        .bind(game)
        .bind(name)
        .bind(played)
        .bind(verified)
        .bind(hidden)
        .execute(&pool)
        .await
        .unwrap();
    }
    let profile = || async {
        let r = api(&app, "GET", "/api/v1/users/cardfan/profile", Some(&t), None).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        r.json()
    };
    let p = profile().await;
    assert_eq!(p["wow_show"], "both");
    assert_eq!(p["wow_default"], "wow");
    let mut got = names(&p).await;
    got.sort();
    assert_eq!(got, vec!["Mockrend".to_string(), "Vale Mocken".to_string()]);
    assert!(p["wow_characters"][0].get("played_min").is_none());
    assert!(p["wow_characters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "Mockrend" && c["verified"] == true));
    assert!(p["wow_characters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "Vale Mocken" && c["verified"] == false));

    let me = api(&app, "GET", "/api/v1/me/bnet", Some(&t), None).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.text);
    assert_eq!(me.json()["profile_show"], "both");
    assert_eq!(me.json()["default_flavour"], "wow");

    let bad = api(
        &app,
        "POST",
        "/api/v1/me/wow/profile_show",
        Some(&t),
        Some(serde_json::json!({ "show": "classic" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST, "{}", bad.text);

    let set = |show: &str| {
        let app = &app;
        let t = &t;
        let show = show.to_string();
        async move {
            let r = api(
                app,
                "POST",
                "/api/v1/me/wow/profile_show",
                Some(t),
                Some(serde_json::json!({ "show": show })),
            )
            .await;
            assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        }
    };
    set("wow").await;
    let mut only = names(&profile().await).await;
    only.sort();
    assert_eq!(only, vec!["Mockrend".to_string()]);
    set("wow_forever").await;
    assert_eq!(
        names(&profile().await).await,
        vec!["Vale Mocken".to_string()]
    );
    set("both").await;
    let both = profile().await;
    assert_eq!(both["wow_toggle"], true);
    assert_eq!(both["wow_open"], "wow");
    sqlx::query(
        "UPDATE wow_characters SET hidden = true WHERE user_id = $1 AND game_id = 'wow_forever'",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    let one = profile().await;
    assert_eq!(one["wow_show"], "both");
    assert_eq!(one["wow_toggle"], false);
    assert_eq!(one["wow_open"], "wow");
    assert!(one["wow_characters"]
        .as_array()
        .unwrap()
        .iter()
        .all(|c| c["game_id"] == "wow"));
    sqlx::query(
        "UPDATE wow_characters SET hidden = false WHERE user_id = $1 AND game_id = 'wow_forever'",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    set("none").await;
    let hidden_page = profile().await;
    assert_eq!(hidden_page["wow_show"], "none");
    assert_eq!(hidden_page["wow_toggle"], false);
    assert_eq!(hidden_page["wow_characters"].as_array().unwrap().len(), 0);
    let me_none = api(&app, "GET", "/api/v1/me/bnet", Some(&t), None).await;
    assert_eq!(me_none.json()["profile_show"], "none");
    let me_user = api(&app, "GET", "/api/v1/me", Some(&t), None).await;
    assert_eq!(me_user.json()["user"]["has_wow"], true);
    set("both").await;
    let r = api(
        &app,
        "POST",
        "/api/v1/me/wow/default_flavour",
        Some(&t),
        Some(serde_json::json!({ "flavour": "wow_forever" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = profile().await;
    assert_eq!(p["wow_show"], "both");
    assert_eq!(p["wow_default"], "wow_forever");
    assert_eq!(p["wow_open"], "wow_forever");
    assert_eq!(p["wow_toggle"], true);
    assert_eq!(names(&p).await.len(), 2);
    let t2 = signup(&app, "nowow@example.com", "nowow").await;
    let bare = api(&app, "GET", "/api/v1/me", Some(&t2), None).await;
    assert_eq!(bare.json()["user"]["has_wow"], false);

    let stored: Vec<(String, i64, bool)> = sqlx::query_as(
        "SELECT name, played_s, hidden FROM wow_characters WHERE user_id = $1 ORDER BY name",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        stored,
        vec![
            ("Hideme".into(), 1800, true),
            ("Mockrend".into(), 7200, false),
            ("Samplekin".into(), 3600, false),
            ("Vale Mocken".into(), 5000, false),
        ]
    );
    let hours: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hours, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn armory_add_verifies_only_from_the_account_list(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let _t = signup(&app, "arm@example.com", "armoryer").await;
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'armoryer'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified, scopes)
         VALUES($1, 'bnet', 'arm-1', 'Armoryer#1', true, 'openid wow.profile')",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, realm_name, name, bnet_id)
         VALUES($1, 'wow', 'us', 'area-52', 'Area 52', 'Mockrend', 4242)",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    let st = state(pool.clone());
    let profile = serde_json::json!({
        "id": 4242,
        "name": "Mockrend",
        "level": 80,
        "realm": {"name": "Area 52", "slug": "area-52"},
        "playable_class": {"name": "Mage"},
        "race": {"name": "Human"},
        "gender": {"type": "MALE"},
        "active_spec": {"name": "Frost"}
    });
    let added = fraglink_server::wow::add_armory_character(
        &st, uid, "us", "Area 52", "Mockrend", &profile, "wow",
    )
    .await
    .unwrap();
    assert_eq!(added["verified"], true);
    assert_eq!(added["created"], true);
    let row: (i64, i32, String, bool) = sqlx::query_as(
        "SELECT played_s, level, class, verified FROM wow_characters WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, 0);
    assert_eq!((row.1, row.2.as_str(), row.3), (80, "MAGE", true));
    sqlx::query("UPDATE wow_characters SET played_s = 5000 WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let again = fraglink_server::wow::add_armory_character(
        &st, uid, "us", "area 52", "mockrend", &profile, "wow",
    )
    .await
    .unwrap();
    assert_eq!(again["created"], false);
    let played: i64 = sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(played, 5000, "an Armory add does not rewrite /played");
    let hours: i64 = sqlx::query_scalar("SELECT count(*) FROM hours_entries WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(hours, 0);

    let other = signup(&app, "arm2@example.com", "armorytwo").await;
    let uid2: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'armorytwo'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let _ = other;
    let stray = fraglink_server::wow::add_armory_character(
        &state(pool.clone()),
        uid2,
        "us",
        "Area 52",
        "Samplekin",
        &serde_json::json!({"id": 99, "name": "Samplekin", "level": 0, "realm": {"name": "Area 52"}, "playable_class": {"name": "Hunter"}}),
        "wow",
    )
    .await
    .unwrap();
    assert_eq!(
        stray["verified"], false,
        "not on the account list, so no tick"
    );
    let bad = fraglink_server::wow::add_armory_character(
        &state(pool.clone()),
        uid2,
        "us",
        "x",
        "a",
        &serde_json::json!({"name": "a"}),
        "wow",
    )
    .await;
    assert!(bad.is_err());
}
