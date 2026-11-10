mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn faction_metadata_is_optional_and_does_not_change_hours(pool: PgPool) {
    let migration = include_str!("../migrations/0060_wow_faction.sql");
    assert!(!migration.to_lowercase().contains("update "));
    assert!(!migration.to_lowercase().contains("delete "));
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let token = signup(&app, "factions@example.com", "factiontest").await;
    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'factiontest'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (i, faction) in [Some("HORDE"), Some("ALLIANCE"), None, None]
        .iter()
        .enumerate()
    {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO wow_characters(id,user_id,game_id,region,realm,realm_slug,name,class,played_s,played_at,verified,faction) VALUES($1,$2,'wow','us','Area 52','area-52',$3,'MAGE',7200,to_timestamp(1700000000),true,$4)")
            .bind(id).bind(uid).bind(format!("Sample{i}")).bind(faction).execute(&pool).await.unwrap();
        ids.push(id);
    }
    // Old cached Blizzard summaries can supply a faction without updating the character.
    sqlx::query(
        "INSERT INTO wow_char_details(char_id,status,data,fetched_at) VALUES($1,'ok',$2,now())",
    )
    .bind(ids[3])
    .bind(json!({"faction":"Alliance"}))
    .execute(&pool)
    .await
    .unwrap();
    let before: Vec<(Uuid, i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id,played_s,played_at FROM wow_characters WHERE user_id=$1 ORDER BY id",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    let chars = fraglink_server::wow::characters(&st, uid, true, false)
        .await
        .unwrap();
    for (id, expected) in ids.iter().zip([
        json!("HORDE"),
        json!("ALLIANCE"),
        json!(null),
        json!("ALLIANCE"),
    ]) {
        assert_eq!(
            chars.iter().find(|c| c["id"] == id.to_string()).unwrap()["faction"],
            expected
        );
    }
    let manager = api(&app, "GET", "/api/v1/me/wow/list", Some(&token), None).await;
    assert_eq!(manager.status, StatusCode::OK, "{}", manager.text);
    let rows = manager.json();
    let items = rows["items"].as_array().unwrap();
    assert!(items.iter().any(|c| c["faction"] == "HORDE"));
    assert!(items.iter().any(|c| c["faction"].is_null()));
    // A normal profile lookup fills/changes faction; absent type keeps the last value.
    for profile in [
        json!({"name":"Sample0","level":80,"faction":{"type":"ALLIANCE"}}),
        json!({"name":"Sample0","level":80}),
    ] {
        fraglink_server::wow::add_armory_character(
            &st, uid, "us", "Area 52", "Sample0", &profile, "wow",
        )
        .await
        .unwrap();
    }
    let faction: Option<String> =
        sqlx::query_scalar("SELECT faction FROM wow_characters WHERE id=$1")
            .bind(ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(faction.as_deref(), Some("ALLIANCE"));
    let after: Vec<(Uuid, i64, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id,played_s,played_at FROM wow_characters WHERE user_id=$1 ORDER BY id",
    )
    .bind(uid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(before, after);
}
