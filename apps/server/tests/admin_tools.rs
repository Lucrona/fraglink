mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

async fn uid(pool: &PgPool, handle: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn role(pool: &PgPool, handle: &str, role: &str) {
    sqlx::query("UPDATE users SET role = $2 WHERE handle = $1")
        .bind(handle)
        .bind(role)
        .execute(pool)
        .await
        .unwrap();
}

async fn audits(pool: &PgPool, action: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = $1")
        .bind(action)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn changelog_drafts_publish_and_public(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "clb@example.com", "clboss").await;
    let modr = signup(&app, "clm@example.com", "clmod").await;
    role(&pool, "clboss", "admin").await;
    role(&pool, "clmod", "moderator").await;

    let entry = json!({ "area": "Website", "date": "2026-11-05", "new": ["- Admin console", ""], "changes": [], "fixes": ["Counts fixed"] });
    // moderators can't write the changelog
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/changelog",
        Some(&modr),
        Some(entry.clone()),
    )
    .await;
    assert_ne!(r.status, StatusCode::OK);
    // bad input
    for bad in [
        json!({ "area": "Blog", "date": "2026-11-05", "new": ["x"] }),
        json!({ "area": "App", "date": "5 Nov", "new": ["x"] }),
        json!({ "area": "App", "date": "2026-11-05", "new": [" ", ""] }),
        json!({ "area": "App", "date": "2026-11-05", "new": ["Works like Xfire"] }),
    ] {
        let r = api(
            &app,
            "POST",
            "/api/v1/admin/changelog",
            Some(&boss),
            Some(bad.clone()),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/changelog",
        Some(&boss),
        Some(entry),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json());
    let e = r.json()["entry"].clone();
    let id = e["id"].as_i64().unwrap();
    assert_eq!(e["new"], json!(["Admin console"]));
    assert!(e["published_at"].is_null());

    // drafts don't show publicly
    let p = api(&app, "GET", "/api/v1/changelog", None, None)
        .await
        .json();
    assert_eq!(p["entries"].as_array().unwrap().len(), 0);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/changelog/{id}/publish"),
        Some(&boss),
        Some(json!({ "publish": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let p = api(&app, "GET", "/api/v1/changelog", None, None)
        .await
        .json();
    assert_eq!(p["entries"][0]["label"], "5 November 2026");
    assert_eq!(p["entries"][0]["fixes"], json!(["Counts fixed"]));

    // edit keeps it published
    let r = api(&app, "POST", "/api/v1/admin/changelog", Some(&boss), Some(json!({ "id": id, "area": "App", "date": "2026-11-05", "version": "0.9.1", "new": ["One"] }))).await;
    assert_eq!(r.status, StatusCode::OK);
    let l = api(&app, "GET", "/api/v1/admin/changelog", Some(&boss), None)
        .await
        .json();
    assert_eq!(l["total"], 1);
    assert_eq!(l["rows"][0]["version"], "0.9.1");
    assert!(!l["rows"][0]["published_at"].is_null());

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/changelog/{id}/delete"),
        Some(&boss),
        Some(json!({ "confirm": "no" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/changelog/{id}/delete"),
        Some(&boss),
        Some(json!({ "confirm": "delete" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    for a in [
        "changelog.create",
        "changelog.edit",
        "changelog.publish",
        "changelog.delete",
    ] {
        assert_eq!(audits(&pool, a).await, 1, "{a}");
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn download_links_are_checked_and_public(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "lnk@example.com", "lnkboss").await;
    role(&pool, "lnkboss", "admin").await;
    for v in ["http://x.example", "javascript:alert(1)", "https://a b"] {
        let r = api(
            &app,
            "POST",
            "/api/v1/admin/settings",
            Some(&boss),
            Some(json!({ "key": "link_discord", "value": v })),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{v}");
    }
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/settings",
        Some(&boss),
        Some(json!({ "key": "link_discord", "value": "https://chat.example/invite" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.json()["links"].to_string().contains("chat.example"));
    let f = api(&app, "GET", "/api/v1/features", None, None)
        .await
        .json();
    assert_eq!(f["links"]["link_discord"], "https://chat.example/invite");
    assert!(f["links"].get("link_download").is_none());
    // empty puts the default back
    api(
        &app,
        "POST",
        "/api/v1/admin/settings",
        Some(&boss),
        Some(json!({ "key": "link_discord", "value": "" })),
    )
    .await;
    let f = api(&app, "GET", "/api/v1/features", None, None)
        .await
        .json();
    assert!(f["links"].get("link_discord").is_none());
    assert_eq!(audits(&pool, "site_setting").await, 2);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn user_delete_needs_typed_handle_and_spares_staff(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "udb@example.com", "udboss").await;
    let modr = signup(&app, "udm@example.com", "udmod").await;
    signup(&app, "udv@example.com", "udvictim").await;
    role(&pool, "udboss", "admin").await;
    role(&pool, "udmod", "moderator").await;
    let victim = uid(&pool, "udvictim").await;
    let path = format!("/api/v1/admin/users/{victim}/delete");

    let r = api(
        &app,
        "POST",
        &path,
        Some(&modr),
        Some(json!({ "confirm": "udvictim" })),
    )
    .await;
    assert_ne!(r.status, StatusCode::OK, "moderators can't delete");
    let r = api(
        &app,
        "POST",
        &path,
        Some(&boss),
        Some(json!({ "confirm": "udvictimx" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let me = uid(&pool, "udboss").await;
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{me}/delete"),
        Some(&boss),
        Some(json!({ "confirm": "udboss" })),
    )
    .await;
    assert_ne!(r.status, StatusCode::OK, "not yourself");
    let m = uid(&pool, "udmod").await;
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{m}/delete"),
        Some(&boss),
        Some(json!({ "confirm": "udmod" })),
    )
    .await;
    assert_ne!(r.status, StatusCode::OK, "not staff");

    let r = api(
        &app,
        "POST",
        &path,
        Some(&boss),
        Some(json!({ "confirm": "UDVICTIM" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE id = $1")
        .bind(victim)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(left, 0);
    assert_eq!(audits(&pool, "user.delete").await, 1);

    // the user list pages
    let l = api(
        &app,
        "GET",
        "/api/v1/admin/users?q=&offset=0",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(l["total"], 2);
    assert_eq!(l["page_size"], 50);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn hours_check_compares_and_closes_stuck_sessions(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "hcb@example.com", "hcboss").await;
    signup(&app, "hcp@example.com", "hcplayer").await;
    let plain = signup(&app, "hcx@example.com", "hcplain").await;
    role(&pool, "hcboss", "moderator").await;
    let p = uid(&pool, "hcplayer").await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();
    let sid = uuid::Uuid::new_v4();
    sqlx::query(
        "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
         VALUES ($1, $2, 'wow', now() - interval '3 hours', now() - interval '1 hour', 7200, 'tracked'),
                ($3, $2, 'wow', now() - interval '90 minutes', now() - interval '30 minutes', 3600, 'tracked')",
    )
    .bind(sid)
    .bind(p)
    .bind(uuid::Uuid::new_v4())
    .execute(&pool)
    .await
    .unwrap();
    // an open session whose app went quiet 10 minutes ago
    sqlx::query(
        "INSERT INTO presence(user_id, status, game_id, session_started_at, heartbeat_at)
         VALUES ($1, 'online', 'wow', now() - interval '40 minutes', now() - interval '10 minutes')",
    )
    .bind(p)
    .execute(&pool)
    .await
    .unwrap();

    let path = format!("/api/v1/admin/users/{p}/hours_check");
    let r = api(&app, "GET", &path, Some(&plain), None).await;
    assert_ne!(r.status, StatusCode::OK);
    let d = api(&app, "GET", &path, Some(&boss), None).await.json();
    assert_eq!(d["mismatches"], 0, "{d}");
    assert!(d["open_session"]["stuck"].as_bool().unwrap(), "{d}");
    let flags = d["sessions"].to_string();
    assert!(flags.contains("overlap"), "{flags}");

    // reasons are required
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{p}/open_session/close"),
        Some(&boss),
        Some(json!({ "reason": "", "keep": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{p}/open_session/close"),
        Some(&boss),
        Some(json!({ "reason": "App crashed", "keep": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.json());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = $1")
        .bind(p)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 3, "kept up to the last heartbeat");
    let d = api(&app, "GET", &path, Some(&boss), None).await.json();
    assert!(d["open_session"].is_null());

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{p}/sessions/{sid}/remove"),
        Some(&boss),
        Some(json!({ "reason": "Duplicate upload" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let why: serde_json::Value =
        sqlx::query_scalar("SELECT after FROM admin_audit WHERE action = 'hours.delete_session'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(why["reason"], "Duplicate upload");
    assert_eq!(audits(&pool, "hours.close_session").await, 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn health_is_admin_only(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "hb@example.com", "hlboss").await;
    let modr = signup(&app, "hm@example.com", "hlmod").await;
    role(&pool, "hlboss", "admin").await;
    role(&pool, "hlmod", "moderator").await;
    let r = api(&app, "GET", "/api/v1/admin/health", Some(&modr), None).await;
    assert_ne!(r.status, StatusCode::OK);
    let d = api(&app, "GET", "/api/v1/admin/health", Some(&boss), None)
        .await
        .json();
    assert!(d["version"].is_string(), "{d}");
    assert!(d["migration"]["version"].as_i64().unwrap() >= 54, "{d}");
    assert!(
        d["backups"].is_object() && d["email"]["live"].is_boolean(),
        "{d}"
    );
    // no job rows: the page must not be handed a made-up healthy job
    assert!(d["jobs"].as_array().unwrap().is_empty(), "{d}");
    assert!(
        d["errors"].is_null() || d["errors"].is_array(),
        "errors are missing or a real list, never a stand-in: {d}"
    );

    sqlx::query(
        "INSERT INTO job_runs(job, last_run_at, last_ok_at, detail) VALUES
         ('steam_sync', timestamptz '2026-10-01', timestamptz '2026-10-01', 'old import'),
         ('games_list', timestamptz '2026-10-01', timestamptz '2026-10-01', 'old list')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let d = api(&app, "GET", "/api/v1/admin/health", Some(&boss), None)
        .await
        .json();
    let jobs = d["jobs"].as_array().unwrap();
    let steam = jobs.iter().find(|j| j["job"] == "steam_sync").unwrap();
    let list = jobs.iter().find(|j| j["job"] == "games_list").unwrap();
    assert_eq!(steam["scheduled"], false, "{steam}");
    assert_eq!(
        steam["stale"], false,
        "an old import is not a missed schedule: {steam}"
    );
    assert_eq!(steam["failing"], false, "{steam}");
    assert_eq!(list["scheduled"], true, "{list}");
    assert_eq!(list["stale"], true, "{list}");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn bulk_preview_reads_and_does_not_write(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "bpb@example.com", "bpboss").await;
    let modr = signup(&app, "bpm@example.com", "bpmod").await;
    role(&pool, "bpboss", "admin").await;
    role(&pool, "bpmod", "moderator").await;
    sqlx::query(
        "INSERT INTO games(id, name, verified) VALUES ('preview_game', 'Preview Game', true)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO game_status(game_id, lifecycle, note) VALUES ('preview_game', 'beta', 'still in beta')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let body = json!({
        "ids": ["preview_game", "missing_game", "preview_game"],
        "lifecycle": "ended",
        "note": "season over",
        "checked": true
    });
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/bulk/preview",
        Some(&modr),
        Some(body.clone()),
    )
    .await;
    assert_ne!(
        r.status,
        StatusCode::OK,
        "moderators can't preview bulk changes"
    );
    let d = api(
        &app,
        "POST",
        "/api/v1/admin/bulk/preview",
        Some(&boss),
        Some(body),
    )
    .await
    .json();
    assert_eq!(d["lifecycle"], "ended");
    assert_eq!(
        d["rows"].as_array().unwrap().len(),
        2,
        "duplicates collapse: {d}"
    );
    assert_eq!(d["rows"][0]["found"], true);
    assert_eq!(d["rows"][0]["name"], "Preview Game");
    assert_eq!(d["rows"][0]["before"]["lifecycle"], "beta");
    assert_eq!(d["rows"][1]["id"], "missing_game");
    assert_eq!(d["rows"][1]["found"], false);
    assert_eq!(d["missing"], 1);
    let life: String =
        sqlx::query_scalar("SELECT lifecycle FROM game_status WHERE game_id = 'preview_game'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(life, "beta", "preview must not write");
    let audits_n: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(audits_n, 0);
}
