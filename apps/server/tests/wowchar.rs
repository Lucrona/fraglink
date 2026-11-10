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

#[test]
fn forever_maps_probed_shapes_without_retail_filler() {
    let f: Value = serde_json::from_str(include_str!("fixtures/forever-popup.json")).unwrap();
    let icons = [(1769, "inv_shoulder_06".to_string())].into();
    let d = fraglink_server::wowchar::summarise_forever(
        &f["root"],
        &f["equip"],
        &f["stats"],
        &f["media"],
        &f["specs"],
        &f["profs"],
        &f["pvp"],
        &icons,
    );
    assert_eq!(d["name"], "Sample Character");
    assert_eq!(d["game_id"], "wow_forever");
    assert_eq!(d["spec"], "Protection");
    assert_eq!(d["gear"][0]["quality"], "POOR");
    assert_eq!(d["gear"][0]["icon"], "inv_shoulder_06");
    assert_eq!(d["gear"][0]["ilvl"], 20);
    assert_eq!(d["stats"][0]["name"], "Health");
    assert_eq!(d["stats"][0]["value"], 508.0);
    assert_eq!(d["professions"][0]["name"], "Mining");
    assert!(d["render"].as_bool().unwrap());
    assert_eq!(
        d["armory"],
        "https://worldofwarcraft.blizzard.com/en-us/forever/us/armory/character/sample-character"
    );
    assert!(d["mplus"]["rating"].is_null());
    assert!(d["raids"]["current"].is_null());
    let n = Value::Null;
    let empty = fraglink_server::wowchar::summarise_forever(
        &f["root"],
        &n,
        &n,
        &n,
        &n,
        &n,
        &n,
        &Default::default(),
    );
    assert!(empty["gear"].as_array().unwrap().is_empty());
    assert!(empty["stats"].as_array().unwrap().is_empty());
    assert!(empty["professions"].as_array().unwrap().is_empty());
    assert_eq!(empty["render"], false);
}

#[test]
fn stable_character_paths_accept_only_version_and_uuid() {
    let id = uuid::Uuid::new_v4();
    for (version, game) in [("retail", "wow"), ("forever", "wow_forever")] {
        let route = format!("sample/wow/{version}/{id}");
        assert_eq!(
            fraglink_server::seo::character_path(&route),
            Some(("sample", game, id))
        );
    }
    for path in [
        "sample/wow",
        "sample/wow/forever/42",
        "sample/wow/classic/00000000-0000-0000-0000-000000000000",
        "sample/wow/forever/../../private",
    ] {
        assert!(fraglink_server::seo::character_path(path).is_none());
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forever_and_share_links_obey_public_visibility(pool: PgPool) {
    // Only this integration-test process sees the temporary flag. No live flags.
    struct Flag(Option<String>);
    impl Drop for Flag {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("BNET_FOREVER_LOOKUP", v),
                None => std::env::remove_var("BNET_FOREVER_LOOKUP"),
            }
        }
    }
    let _restore = Flag(std::env::var("BNET_FOREVER_LOOKUP").ok());
    std::env::remove_var("BNET_FOREVER_LOOKUP");
    let mut st = state(pool.clone());
    let site = tempfile::tempdir().unwrap();
    std::fs::write(
        site.path().join("u.html"),
        "<html><head><!--seo--><!--/seo--></head><body>Profile</body></html>",
    )
    .unwrap();
    std::sync::Arc::make_mut(&mut st.cfg).site_dir = site.path().into();
    let app = fraglink_server::router(st);
    let token = signup(&app, "share@example.com", "sharetest").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle='sharetest'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE users SET public_profile=true, birth_date='1990-01-01', privacy='{}' WHERE id=$1",
    )
    .bind(uid)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO site_settings(key,enabled) VALUES('wow_characters',true),('public_profiles',true)").execute(&pool).await.unwrap();
    fraglink_server::features::forget();
    let id = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO wow_characters(id,user_id,game_id,region,realm,realm_slug,name,class,level,played_s,played_at,verified) VALUES($1,$2,'wow_forever','us','','','Sample Character','WARRIOR',18,9876,now(),false)")
        .bind(id).bind(uid).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO wow_char_details(char_id,status,data,fetched_at) VALUES($1,'ok',$2,now())")
        .bind(id).bind(json!({"name":"Sample Character","game_id":"wow_forever","level":18,"guild":{"name":"Secret Guild"},"gear":[],"render":false,"avatar_src":"https://render.worldofwarcraft.com/forever-us/character/1/42-avatar.jpg"})).execute(&pool).await.unwrap();
    let detail = format!("/api/v1/users/sharetest/wow/{id}");
    let avatar = format!("{detail}/avatar");
    let before: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM wow_characters c WHERE id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    let share = format!("{detail}/share");
    let route = format!("/u/sharetest/wow/forever/{id}");
    assert_eq!(
        api(&app, "GET", &detail, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    std::env::set_var("BNET_FOREVER_LOOKUP", "on");
    let r = api(&app, "GET", &avatar, None, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        r.json()["avatar_src"],
        "https://render.worldofwarcraft.com/forever-us/character/1/42-avatar.jpg"
    );
    assert_eq!(r.headers["cache-control"], "private, no-store");
    let r = api(&app, "GET", &detail, None, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(!r.text.contains("Secret Guild"));
    let r = api(&app, "GET", &share, None, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(!r.text.contains("Secret Guild"));
    let r = api(&app, "GET", &route, None, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.text.contains("Sample Character"), "{}", r.text);
    assert!(!r.text.contains("Secret Guild"));
    assert!(r.text.contains("og:image"));
    // Wrong-version, missing and hidden ids give the normal generic profile.
    let wrong = api(
        &app,
        "GET",
        &format!("/u/sharetest/wow/retail/{id}"),
        None,
        None,
    )
    .await;
    assert!(!wrong.text.contains("Sample Character"));
    for sql in [
        "UPDATE wow_characters SET hidden=true WHERE id=$1",
        "UPDATE wow_characters SET hidden=false WHERE id=$1",
    ] {
        sqlx::query(sql).bind(id).execute(&pool).await.unwrap();
        if sql.contains("hidden=true") {
            assert_eq!(
                api(&app, "GET", &avatar, None, None).await.status,
                StatusCode::NOT_FOUND
            );
            assert_eq!(
                api(&app, "GET", &avatar, Some(&token), None).await.status,
                StatusCode::OK,
                "owner's manager can show hidden portraits"
            );
            let r = api(&app, "GET", &route, None, None).await;
            assert_eq!(r.status, StatusCode::OK);
            assert!(!r.text.contains("Sample Character"));
            assert_eq!(
                api(&app, "GET", &detail, Some(&token), None).await.status,
                StatusCode::NOT_FOUND
            );
        }
    }
    sqlx::query("INSERT INTO wow_prefs(user_id,profile_show) VALUES($1,'wow')")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &share, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE wow_prefs SET profile_show='both' WHERE user_id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE wow_prefs SET hide_all=true WHERE user_id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &share, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE wow_prefs SET hide_all=false,profile_show='none' WHERE user_id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &share, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE wow_prefs SET profile_show='both' WHERE user_id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET privacy=jsonb_build_object('wow_characters',false) WHERE id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &share, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE users SET privacy='{}' WHERE id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE users SET public_profile=false WHERE id=$1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", &share, Some(&token), None).await.status,
        StatusCode::NOT_FOUND,
        "share is public-only even for the owner"
    );
    let r = api(&app, "GET", &route, None, None).await;
    assert!(!r.text.contains("Sample Character"));
    let seconds: i64 = sqlx::query_scalar("SELECT played_s FROM wow_characters WHERE id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(seconds, 9876);
    assert_eq!(
        api(&app, "GET", &avatar, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api(&app, "GET", &avatar, Some(&token), None).await.status,
        StatusCode::OK
    );
    let after: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM wow_characters c WHERE id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        before, after,
        "avatar requests never modify any character fields or seconds"
    );
}

#[test]
fn forever_avatar_maps_media_and_rejects_wrong_ids_and_urls() {
    use fraglink_server::wowchar::forever_media_avatar;
    let src = "https://render.worldofwarcraft.com/forever-us/character/1/42-avatar.jpg";
    let media = json!({"character":{"id":42},"assets":[{"key":"main-raw","value":"https://render.worldofwarcraft.com/forever-us/character/1/42-main-raw.png"},{"key":"avatar","value":src}]});
    assert_eq!(forever_media_avatar(&media, Some(42)).as_deref(), Some(src));
    assert_eq!(forever_media_avatar(&media, None).as_deref(), Some(src));
    assert!(forever_media_avatar(&media, Some(43)).is_none());
    assert!(forever_media_avatar(&json!({"assets":[]}), None).is_none());
    for url in [
        "https://example.com/avatar.jpg",
        "https://render.worldofwarcraft.com.evil.test/avatar.jpg",
        "javascript:alert(1)",
        "https://render.worldofwarcraft.com/../avatar.jpg",
    ] {
        assert!(
            forever_media_avatar(&json!({"assets":[{"key":"avatar","value":url}]}), None).is_none()
        );
    }
}
