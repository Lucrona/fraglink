mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn set_role(pool: &PgPool, handle: &str, role: Option<&str>) {
    sqlx::query("UPDATE users SET role = $2 WHERE handle = $1")
        .bind(handle)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn uid(pool: &PgPool, handle: &str) -> String {
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap();
    id.to_string()
}

async fn login(app: &axum::Router, login: &str, pw: &str) -> common::Resp {
    api(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({ "email": login, "password": pw })),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn roles_and_access(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let owner = signup(&app, "o@example.com", "olive").await;
    let moder = signup(&app, "m@example.com", "moe").await;
    let user = signup(&app, "u@example.com", "ursula").await;
    set_role(&pool, "olive", Some("owner")).await;
    set_role(&pool, "moe", Some("moderator")).await;

    // non-staff and signed-out callers: 404 everywhere
    for t in [None, Some(user.as_str())] {
        for p in [
            "/api/v1/admin/me",
            "/api/v1/admin/users?q=",
            "/api/v1/admin/server",
        ] {
            assert_eq!(
                api(&app, "GET", p, t, None).await.status,
                StatusCode::NOT_FOUND,
                "{p}"
            );
        }
    }
    let r = api(&app, "GET", "/api/v1/admin/me", Some(&moder), None).await;
    assert_eq!(r.json()["role"], "moderator");
    assert_eq!(r.json()["can"]["server"], false);
    // moderator: no server, games, staff, password resets
    for p in [
        "/api/v1/admin/server",
        "/api/v1/admin/games?q=x",
        "/api/v1/admin/staff",
        "/api/v1/admin/audit",
    ] {
        assert_eq!(
            api(&app, "GET", p, Some(&moder), None).await.status,
            StatusCode::FORBIDDEN,
            "{p}"
        );
    }
    let u_id = uid(&pool, "ursula").await;
    let o_id = uid(&pool, "olive").await;
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/reset_password"),
        Some(&moder),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // nobody bans the owner; staff can't ban themselves
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{o_id}/ban"),
        Some(&moder),
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{o_id}/ban"),
        Some(&owner),
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // moderator bans a user: signed out, can't sign in, profile hidden
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/ban"),
        Some(&moder),
        Some(json!({"reason": "spam"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&user), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let r = login(&app, "ursula", "correct horse battery").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.json()["error"], "account_banned");
    let r = api(
        &app,
        "GET",
        "/api/v1/users/ursula/profile",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/unban"),
        Some(&moder),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        login(&app, "ursula", "correct horse battery").await.status,
        StatusCode::OK
    );

    // owner resets a password: temp password works, old one doesn't
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/reset_password"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let temp = r.json()["temp_password"].as_str().unwrap().to_string();
    assert_eq!(
        login(&app, "ursula", "correct horse battery").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(login(&app, "ursula", &temp).await.status, StatusCode::OK);

    // staff management: admins only by the owner
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff",
        Some(&owner),
        Some(json!({"handle": "moe", "role": "admin"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let admin = moder; // moe is an admin now
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff",
        Some(&admin),
        Some(json!({"handle": "ursula", "role": "moderator"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff",
        Some(&admin),
        Some(json!({"handle": "ursula", "role": "admin"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff",
        Some(&admin),
        Some(json!({"handle": "olive", "role": "none"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff",
        Some(&owner),
        Some(json!({"handle": "olive", "role": "admin"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/staff/transfer",
        Some(&admin),
        Some(json!({"handle": "moe", "confirm": "moe"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // admins can't ban other admins or the owner
    let m_id = uid(&pool, "moe").await;
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{o_id}/ban"),
        Some(&admin),
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{m_id}/ban"),
        Some(&owner),
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "owner may ban an admin");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(n >= 6, "audit rows: {n}");
    let row: (String, serde_json::Value) = sqlx::query_as(
        "SELECT action, after FROM admin_audit WHERE action = 'user.ban' ORDER BY id LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.1["reason"], "spam");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn games_approvals_hours_banner(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let owner = signup(&app, "o@example.com", "olive").await;
    let user = signup(&app, "u@example.com", "ursula").await;
    set_role(&pool, "olive", Some("owner")).await;

    // approvals, same effect as the CLI
    api(
        &app,
        "POST",
        "/api/v1/games/suggest",
        None,
        Some(json!({"exe": "OddGame.exe", "name": "Odd Game"})),
    )
    .await;
    let r = api(&app, "GET", "/api/v1/admin/suggestions", Some(&owner), None).await;
    assert_eq!(r.json()["suggestions"][0]["exe"], "oddgame.exe");
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/suggestions/approve",
        Some(&owner),
        Some(json!({"exe": "oddgame.exe", "name": "Odd Game"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v: bool = sqlx::query_scalar("SELECT verified FROM games WHERE id = 'exe:oddgame.exe'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(v);
    let r = api(&app, "GET", "/api/v1/admin/suggestions", Some(&owner), None).await;
    assert_eq!(r.json()["suggestions"].as_array().unwrap().len(), 0);

    // game edits: name override, extra exe (served to apps), hidden
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('steam:570', 'Dota 2', true)")
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:570",
        Some(&owner),
        Some(json!({"name": "DOTA Two", "hidden": false, "extra_exes": ["Dota2Beta.exe"]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["public_name"], "DOTA Two");
    let ov = api(&app, "GET", "/api/v1/games/overrides", None, None).await;
    assert!(ov.text.contains("dota2beta.exe"), "{}", ov.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/steam:570",
        Some(&owner),
        Some(json!({"hidden": true})),
    )
    .await;
    assert_eq!(r.json()["public_name"], serde_json::Value::Null);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/nope:1",
        Some(&owner),
        Some(json!({})),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // icon upload: resized and served
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(100, 100, image::Rgba([200, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let app2 = fraglink_server::router(state_with(pool.clone(), |c| {
        c.data_dir = dir.path().to_path_buf()
    }));
    let req = axum::http::Request::builder()
        .method("POST")
        .uri("/api/v1/admin/games/steam:570/icon")
        .header("authorization", format!("Bearer {owner}"))
        .header("content-type", "image/png")
        .body(axum::body::Body::from(png))
        .unwrap();
    let r = send(&app2, req).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let r = api(&app2, "GET", "/icons/games/16/steam%3A570.png", None, None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(image::load_from_memory(&r.bytes).unwrap().width(), 16);

    // hours: cap and remove
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, 'exe:oddgame.exe', now() - interval '20 hours', now() - interval '1 hour', 19 * 3600
         FROM users WHERE handle = 'ursula'",
    ).execute(&pool).await.unwrap();
    let u_id = uid(&pool, "ursula").await;
    let r = api(&app, "GET", "/api/v1/admin/hours/flags", Some(&owner), None).await;
    assert!(r.text.contains("session_over_18h"), "{}", r.text);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/hours"),
        Some(&owner),
        Some(json!({"game_id": "exe:oddgame.exe", "action": "cap", "hours": 5})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["session_min"], 300);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{u_id}/hours"),
        Some(&owner),
        Some(json!({"game_id": "exe:oddgame.exe", "action": "remove"})),
    )
    .await;
    assert_eq!(r.json()["sessions"], 0);
    let r = api(
        &app,
        "GET",
        &format!("/api/v1/admin/users/{u_id}"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.json()["handle"], "ursula");

    // banner
    let r = api(&app, "GET", "/api/v1/announcement", None, None).await;
    assert_eq!(r.json()["announcement"], serde_json::Value::Null);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/announcements",
        Some(&owner),
        Some(json!({"message": "Maintenance tonight", "style": "warning"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let r = api(&app, "GET", "/api/v1/announcement", Some(&user), None).await;
    assert_eq!(r.json()["announcement"]["message"], "Maintenance tonight");
    api(
        &app,
        "POST",
        "/api/v1/admin/announcements/clear",
        Some(&owner),
        None,
    )
    .await;
    let r = api(&app, "GET", "/api/v1/announcement", None, None).await;
    assert_eq!(r.json()["announcement"], serde_json::Value::Null);

    let r = api(&app, "GET", "/api/v1/admin/server", Some(&owner), None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["accounts"]["total"], 2);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn retire_and_unretire_a_game(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let owner = signup(&app, "o@example.com", "olive").await;
    set_role(&pool, "olive", Some("owner")).await;
    let p = "/api/v1/admin/games/wow_forever";
    let r = api(&app, "POST", &format!("{p}/retire"), Some(&owner), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["retired"]["successor"], "exe:wowb.exe");
    assert_eq!(
        fraglink_server::retire::effective_id(&st, "wow_forever")
            .await
            .unwrap(),
        "exe:wowb.exe"
    );
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_suggestions WHERE exe = 'wowb.exe' AND status = 'new'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
    // twice: refused
    let again = api(&app, "POST", &format!("{p}/retire"), Some(&owner), None).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    // undo while the new entry has no hours
    let u = api(&app, "POST", &format!("{p}/unretire"), Some(&owner), None).await;
    assert_eq!(u.status, StatusCode::OK, "{}", u.text);
    assert!(u.json()["retired"].is_null());
    assert_eq!(
        fraglink_server::retire::effective_id(&st, "wow_forever")
            .await
            .unwrap(),
        "wow_forever"
    );
    let acts: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM admin_audit WHERE target = 'wow_forever' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(acts, ["game.retire", "game.unretire"]);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn retire_some_programs_and_game_status(pool: PgPool) {
    sqlx::query(
        "INSERT INTO game_overrides(game_id, extra_exes) VALUES('wow_forever', '{wowf2.exe}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let owner = signup(&app, "o@example.com", "olive").await;
    set_role(&pool, "olive", Some("owner")).await;
    let p = "/api/v1/admin/games/wow_forever";
    let g = api(&app, "GET", p, Some(&owner), None).await.json();
    let exes: Vec<String> = serde_json::from_value(g["exes"].clone()).unwrap();
    assert!(
        exes.contains(&"wowb.exe".to_string()) && exes.contains(&"wowf2.exe".to_string()),
        "{exes:?}"
    );
    let bad = api(
        &app,
        "POST",
        &format!("{p}/retire"),
        Some(&owner),
        Some(json!({ "exes": ["nope.exe"] })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        &format!("{p}/retire"),
        Some(&owner),
        Some(json!({ "exes": ["wowf2.exe"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["retired"]["partial"], true);
    // the game itself stays live; the program gets a rule of its own
    assert_eq!(
        fraglink_server::retire::effective_id(&st, "wow_forever")
            .await
            .unwrap(),
        "wow_forever"
    );
    let ov = api(&app, "GET", "/api/v1/games/overrides", None, None)
        .await
        .json();
    let rules = ov["rules"].as_array().unwrap();
    assert!(rules
        .iter()
        .any(|r| r["id"] == "exe:wowf2.exe" && r["exes"][0] == "wowf2.exe"));
    assert!(!rules.iter().any(|r| r["id"] == "wow_forever"
        && r["exes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "wowf2.exe")));
    let u = api(&app, "POST", &format!("{p}/unretire"), Some(&owner), None).await;
    assert_eq!(u.status, StatusCode::OK, "{}", u.text);

    // status
    let s = api(&app, "POST", &format!("{p}/status"), Some(&owner), Some(json!({ "lifecycle": "beta", "date": "2026-12-01", "note": "ends with launch", "checked": true }))).await;
    assert_eq!(s.status, StatusCode::OK, "{}", s.text);
    let v = s.json();
    assert_eq!(v["status"]["lifecycle"], "beta");
    assert_eq!(v["status"]["date"], "2026-12-01");
    assert_eq!(v["status"]["checked_by"], "olive");
    let e = api(
        &app,
        "POST",
        &format!("{p}/status"),
        Some(&owner),
        Some(json!({ "lifecycle": "soon" })),
    )
    .await;
    assert_eq!(e.status, StatusCode::BAD_REQUEST);
    // the weekly routine (CLI path): keeps the note, stamps its own name
    let after = fraglink_server::game_status::set(
        &pool,
        "wow_forever",
        fraglink_server::game_status::Update {
            lifecycle: Some("Early Access".into()),
            checked_by: Some("Fraglink check".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(after["lifecycle"], "early_access");
    assert_eq!(after["note"], "ends with launch");
    assert_eq!(after["checked_by"], "Fraglink check");
    let acts: Vec<String> = sqlx::query_scalar(
        "SELECT action FROM admin_audit WHERE target = 'wow_forever' ORDER BY id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(acts, ["game.retire", "game.unretire", "game.status"]);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn email_alert_settings(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let owner = signup(&app, "o@example.com", "olive").await;
    let moder = signup(&app, "m@example.com", "moe").await;
    set_role(&pool, "olive", Some("owner")).await;
    set_role(&pool, "moe", Some("moderator")).await;

    let r = api(&app, "GET", "/api/v1/admin/alerts", Some(&owner), None).await;
    assert_eq!(r.status, StatusCode::OK);
    let v = r.json();
    assert_eq!(v["recipient"], "support@playfraglink.com");
    assert_eq!(v["alerts"].as_array().unwrap().len(), 4);
    assert!(v["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["enabled"] == true));

    // moderators can't see or change alerts
    assert_eq!(
        api(&app, "GET", "/api/v1/admin/alerts", Some(&moder), None)
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let r = api(
        &app,
        "POST",
        "/api/v1/admin/alerts",
        Some(&owner),
        Some(json!({ "key": "alert_errors", "enabled": false })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let off = r.json()["alerts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["key"] == "alert_errors")
        .unwrap()["enabled"]
        .clone();
    assert_eq!(off, false);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/alerts",
        Some(&owner),
        Some(json!({ "recipient": "ops@example.com" })),
    )
    .await;
    assert_eq!(r.json()["recipient"], "ops@example.com");
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/alerts",
        Some(&owner),
        Some(json!({ "recipient": "not an email" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/alerts",
        Some(&owner),
        Some(json!({ "key": "maintenance", "enabled": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // alert switches stay out of the public feature list
    let f = api(&app, "GET", "/api/v1/features", None, None)
        .await
        .json();
    assert!(f["features"].get("alert_errors").is_none());

    // audited
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = 'alert_setting'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 2);

    // test send with mail off: logged as not sent, nothing goes out
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/alerts/test",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["test_sent"], false);
    assert_eq!(r.json()["recent"][0]["kind"], "test");
}
