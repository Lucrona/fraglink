mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn role(pool: &PgPool, handle: &str, role: &str) {
    sqlx::query("UPDATE users SET role = $2, is_test = true WHERE handle = $1")
        .bind(handle)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn hours_review_hides_without_changing_sessions(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "hrb@example.com", "hrboss").await;
    let plain = signup(&app, "hrp@example.com", "hrplain").await;
    role(&pool, "hrboss", "moderator").await;
    role(&pool, "hrplain", "moderator").await;
    sqlx::query("UPDATE users SET role = NULL WHERE handle = 'hrplain'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games(id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'hrboss'")
        .fetch_one(&pool)
        .await
        .unwrap();
    // 22.1 h on the first of ten sessions, plus nine short ones, all on 1 Nov UTC.
    for i in 0..10 {
        let secs = if i == 0 { 22 * 3600 + 360 } else { 360 };
        sqlx::query(
            "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
             VALUES ($1, $2, 'wow',
                     timestamptz '2026-11-01 00:10:00+00' + $3 * interval '1 minute',
                     timestamptz '2026-11-01 00:10:00+00' + $3 * interval '1 minute' + $4 * interval '1 second',
                     $4, 'tracked')",
        )
        .bind(uuid::Uuid::new_v4())
        .bind(uid)
        .bind(i * 30)
        .bind(secs)
        .execute(&pool)
        .await
        .unwrap();
    }
    let before: i64 =
        sqlx::query_scalar("SELECT coalesce(sum(duration_s),0) FROM sessions WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    let list = api(
        &app,
        "GET",
        "/api/v1/admin/hours/flags?days=365",
        Some(&boss),
        None,
    )
    .await
    .json();
    let flag = list["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["kind"] == "day_over_20h")
        .unwrap();
    let key = flag["flag_key"].as_str().unwrap();
    assert!(flag["detail"].as_str().unwrap().contains("10 sessions"));
    let denied = api(
        &app,
        "POST",
        "/api/v1/admin/hours/flags/review",
        Some(&plain),
        Some(json!({ "flag_key": key, "days": 365 })),
    )
    .await;
    assert_ne!(denied.status, StatusCode::OK);
    let ok = api(
        &app,
        "POST",
        "/api/v1/admin/hours/flags/review",
        Some(&boss),
        Some(json!({ "flag_key": key, "days": 365, "reason": "checked the ten sessions" })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    // repeating the review still does not change hours
    let again = api(
        &app,
        "POST",
        "/api/v1/admin/hours/flags/review",
        Some(&boss),
        Some(json!({ "flag_key": key, "days": 365 })),
    )
    .await;
    assert_eq!(again.status, StatusCode::OK);
    let seen = api(
        &app,
        "GET",
        "/api/v1/admin/hours/flags?days=365&show=reviewed",
        Some(&boss),
        None,
    )
    .await
    .json();
    let row = seen["flags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["flag_key"] == key)
        .unwrap();
    assert_eq!(row["review"]["by"], "hrboss");
    let noted: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit WHERE action = 'hours.review' AND after->>'reason' = 'checked the ten sessions'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(noted, 1);
    let after: i64 =
        sqlx::query_scalar("SELECT coalesce(sum(duration_s),0) FROM sessions WHERE user_id = $1")
            .bind(uid)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(before, after);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 10);
    let hidden = api(
        &app,
        "GET",
        "/api/v1/admin/hours/flags?days=365",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(hidden["flags"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["flag_key"] != key));
    // a changed total is new evidence and must show again
    sqlx::query("UPDATE sessions SET duration_s = duration_s + 60 WHERE user_id = $1")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();
    let back = api(
        &app,
        "GET",
        "/api/v1/admin/hours/flags?days=365",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(back["flags"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["flag_key"] == key));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn suggested_merge_needs_a_launcher_id_and_does_not_merge(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "smb@example.com", "smboss").await;
    let modr = signup(&app, "smm@example.com", "smmod").await;
    role(&pool, "smboss", "admin").await;
    role(&pool, "smmod", "moderator").await;
    sqlx::query(
        "INSERT INTO games(id, name, steam_appid, verified) VALUES
         ('quest', 'Quest', 424242, true),
         ('quest_2', 'Quest 2', 424242, true),
         ('steam:515151', 'Other', 515151, true),
         ('listed_other', 'Other Name', 515151, true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/suggested-merge",
        Some(&boss),
        None,
    )
    .await
    .json();
    let rows = d["rows"].as_array().unwrap();
    let quest = rows.iter().find(|r| r["product"] == "424242").unwrap();
    assert_eq!(
        quest["op"], "look",
        "edition names are not a merge: {quest}"
    );
    let other = rows.iter().find(|r| r["product"] == "515151").unwrap();
    assert_eq!(other["op"], "merge");
    assert!(d["coverage"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["id"] == "epic" && s["available"] == false));
    // name-only neighbours are absent
    assert!(rows
        .iter()
        .all(|r| r["source"] != "quest" || r["launcher"] == "steam"));
    let denied = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:515151/merge",
        Some(&modr),
        Some(json!({ "into": "listed_other", "confirm": "steam:515151" })),
    )
    .await;
    assert_ne!(denied.status, StatusCode::OK);
    let wrong = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:515151/merge",
        Some(&boss),
        Some(json!({ "into": "listed_other", "confirm": "nope" })),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::BAD_REQUEST);
    let still: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still, 0);
    let key = other["key"].as_str().unwrap();
    let dsm = api(
        &app,
        "POST",
        "/api/v1/admin/suggested-merge/dismiss",
        Some(&modr),
        Some(json!({ "key": key })),
    )
    .await;
    assert_ne!(dsm.status, StatusCode::OK);
    let dsm = api(
        &app,
        "POST",
        "/api/v1/admin/suggested-merge/dismiss",
        Some(&boss),
        Some(json!({ "key": key, "reason": "not the same build" })),
    )
    .await;
    assert_eq!(dsm.status, StatusCode::OK, "{}", dsm.text);
    let d2 = api(
        &app,
        "GET",
        "/api/v1/admin/suggested-merge",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(d2["rows"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["key"] != key));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn detect_test_is_read_only_and_names_what_it_cannot_see(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "dtb@example.com", "dtboss").await;
    role(&pool, "dtboss", "admin").await;
    let d = api(
        &app,
        "POST",
        "/api/v1/admin/detect-test",
        Some(&boss),
        Some(json!({ "exe": "Wow.exe", "folder": "D:\\World of Warcraft\\_retail_" })),
    )
    .await;
    assert_eq!(d.status, StatusCode::OK, "{}", d.text);
    let j = d.json();
    assert_eq!(j["matched"]["game_id"], "wow");
    assert!(j["not_reproduced"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["id"] == "steam_library"));
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audits, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn undo_restores_status_and_refuses_a_stale_edit(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "unb@example.com", "unboss").await;
    role(&pool, "unboss", "admin").await;
    sqlx::query("INSERT INTO games(id, name, verified) VALUES ('undo_game', 'Undo Game', true)")
        .execute(&pool)
        .await
        .unwrap();
    let set = api(
        &app,
        "POST",
        "/api/v1/admin/games/undo_game/status",
        Some(&boss),
        Some(json!({ "lifecycle": "beta", "date": "", "note": "first", "checked": false })),
    )
    .await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.text);
    let id: i64 = sqlx::query_scalar("SELECT id FROM admin_audit WHERE action = 'game.status'")
        .fetch_one(&pool)
        .await
        .unwrap();
    api(
        &app,
        "POST",
        "/api/v1/admin/games/undo_game/status",
        Some(&boss),
        Some(json!({ "lifecycle": "ended", "date": "", "note": "later", "checked": false })),
    )
    .await;
    let stale = api(
        &app,
        "POST",
        "/api/v1/admin/undo",
        Some(&boss),
        Some(json!({ "id": id, "confirm": "UNDO" })),
    )
    .await;
    assert_eq!(stale.status, StatusCode::BAD_REQUEST, "{}", stale.text);
    let life: String =
        sqlx::query_scalar("SELECT lifecycle FROM game_status WHERE game_id = 'undo_game'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(life, "ended");
    let latest: i64 = sqlx::query_scalar(
        "SELECT id FROM admin_audit WHERE action = 'game.status' ORDER BY id DESC LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let bad = api(
        &app,
        "POST",
        "/api/v1/admin/undo",
        Some(&boss),
        Some(json!({ "id": latest, "confirm": "no" })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let ok = api(
        &app,
        "POST",
        "/api/v1/admin/undo",
        Some(&boss),
        Some(json!({ "id": latest, "confirm": "UNDO" })),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let life: String =
        sqlx::query_scalar("SELECT lifecycle FROM game_status WHERE game_id = 'undo_game'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(life, "beta");
    let hist = api(
        &app,
        "GET",
        "/api/v1/admin/review-history?group=corrected",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(hist["rows"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["action"] == "game.status" && r["by"] == "unboss"));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn data_checks_do_not_write(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "dcb@example.com", "dcboss").await;
    role(&pool, "dcboss", "admin").await;
    sqlx::query(
        "INSERT INTO games(id, name, steam_appid, verified) VALUES
         ('a_game', 'A', 77, true), ('b_game', 'B', 77, true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO detection_aliases(kind, pattern, game_id) VALUES ('steam', '2357570', 'not_overwatch')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    let d = api(&app, "GET", "/api/v1/admin/data-checks", Some(&boss), None)
        .await
        .json();
    assert!(d["duplicate_launchers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["product"] == "77"));
    assert!(d["conflicting_rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["pattern"] == "2357570"));
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after);
}
