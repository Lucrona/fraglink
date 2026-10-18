mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

fn years_ago(y: i32, extra_days: i64) -> String {
    use chrono::Datelike;
    let t = chrono::Utc::now().date_naive();
    let d = t
        .with_year(t.year() - y)
        .unwrap_or(t - chrono::Duration::days(365 * y as i64));
    (d + chrono::Duration::days(extra_days))
        .format("%Y-%m-%d")
        .to_string()
}

async fn signup_dob(app: &axum::Router, handle: &str, birth: &str) -> common::Resp {
    api(
        app,
        "POST",
        "/api/v1/auth/signup",
        None,
        Some(json!({
            "email": format!("{handle}@example.com"),
            "password": "correct horse battery",
            "handle": handle,
            "birth_date": birth,
        })),
    )
    .await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn under_18_accounts_are_private_and_unlisted(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    // DOB required
    let r = signup_dob(&app, "nogroup", "").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "dob_required");

    let adult = signup(&app, "a@example.com", "alice").await;
    let r = signup_dob(&app, "teen", &years_ago(15, 0)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    let teen = r.json()["token"].as_str().unwrap().to_string();
    let (minor, public, set): (bool, bool, bool) =
        sqlx::query_as("SELECT is_minor, public_profile, minor_set_at IS NOT NULL FROM users WHERE handle = 'teen'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(minor && !public && set);
    let me = api(&app, "GET", "/api/v1/me", Some(&teen), None).await;
    assert_eq!(me.json()["user"]["is_minor"], true, "{}", me.text);

    // can't go public while under 18
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&teen),
        Some(json!({"public_profile": true})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "minor_private");

    // a stranger: profile is private, not in search, not in newest members
    let p = api(
        &app,
        "GET",
        "/api/v1/users/teen/profile",
        Some(&adult),
        None,
    )
    .await;
    assert_eq!(p.json()["private"], true, "{}", p.text);
    let s = api(
        &app,
        "GET",
        "/api/v1/users/search?q=teen",
        Some(&adult),
        None,
    )
    .await;
    assert_eq!(s.json()["users"].as_array().unwrap().len(), 0, "{}", s.text);
    let home = api(&app, "GET", "/api/v1/home", None, None).await;
    let newest: Vec<String> = home.json()["newest"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["handle"].as_str().unwrap().to_string())
        .collect();
    assert!(
        newest.contains(&"alice".to_string()) && !newest.contains(&"teen".to_string()),
        "{newest:?}"
    );

    // leaderboards and top players leave them out
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool)
        .await
        .unwrap();
    for h in ["alice", "teen"] {
        sqlx::query(
            "INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s, source)
             SELECT gen_random_uuid(), id, 'wow', now() - interval '2 hours', now() - interval '1 hour', 3600, 'tracked' FROM users WHERE handle = $1",
        )
        .bind(h)
        .execute(&pool)
        .await
        .unwrap();
    }
    let lb = api(&app, "GET", "/api/v1/leaderboard/wow", Some(&adult), None).await;
    let ranks: Vec<String> = lb.json()["ranks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["handle"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ranks, ["alice"], "{}", lb.text);
    let home = api(&app, "GET", "/api/v1/home", None, None).await;
    assert!(!home.text.contains("\"teen\""), "{}", home.text);

    // friends still find them
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&teen),
        Some(json!({"handle": "alice"})),
    )
    .await;
    let r = api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&adult),
        Some(json!({"handle": "teen", "accept": true})),
    )
    .await;
    assert!(r.status.is_success(), "{}", r.text);
    let s = api(
        &app,
        "GET",
        "/api/v1/users/search?q=teen",
        Some(&adult),
        None,
    )
    .await;
    assert_eq!(s.json()["users"][0]["handle"], "teen", "{}", s.text);

    // DOB locked after set (user can't change it)
    let r = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&teen),
        Some(json!({"birth_date": years_ago(18, 0)})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    assert_eq!(r.json()["error"], "dob_locked");

    // admin can set an adult DOB; is_minor clears but profile stays private
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'alice'")
        .execute(&pool)
        .await
        .unwrap();
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'teen'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{id}/birth_date"),
        Some(&adult),
        Some(json!({"birth_date": years_ago(18, 0)})),
    )
    .await;
    assert!(r.status.is_success(), "{}", r.text);
    assert_eq!(r.json()["is_minor"], false);
    let (minor, public): (bool, bool) =
        sqlx::query_as("SELECT is_minor, public_profile FROM users WHERE handle = 'teen'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!minor && !public);

    // now they can go public themselves
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&teen),
        Some(json!({"public_profile": true})),
    )
    .await;
    assert_eq!(r.json()["public_profile"], true, "{}", r.text);

    // adult with no DOB yet: adding a 13–17 DOB goes private
    sqlx::query("UPDATE users SET birth_date = NULL, age_confirmed_at = NULL WHERE handle = 'alice'")
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&adult),
        Some(json!({"birth_date": years_ago(15, 0)})),
    )
    .await;
    assert_eq!(r.json()["is_minor"], true, "{}", r.text);
    assert_eq!(r.json()["public_profile"], false);

    // existing accounts count as adults (column default)
    let old: bool = sqlx::query_scalar("SELECT column_default = 'false' FROM information_schema.columns WHERE table_name = 'users' AND column_name = 'is_minor'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(old);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn under_13_signup_refused_and_existing_pauses(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let boss = signup(&app, "o@example.com", "olive").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'olive'")
        .execute(&pool)
        .await
        .unwrap();

    // sign-up under 13: no account
    let r = signup_dob(&app, "baby", &years_ago(10, 0)).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.json()["error"], "too_young");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE handle = 'baby'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);

    let t = signup(&app, "k@example.com", "kiddo").await;
    // existing accounts without a DOB (pre-DOB migration)
    sqlx::query("UPDATE users SET birth_date = NULL, age_confirmed_at = NULL WHERE handle = 'kiddo'")
        .execute(&pool)
        .await
        .unwrap();
    fraglink_server::alerts::check_age_reviews(&st)
        .await
        .unwrap(); // sets the watermark

    // obviously wrong dates: plain error, nothing else happens
    for d in [
        years_ago(-1, 0),
        "1850-01-01".to_string(),
        "31/12/2010".to_string(),
    ] {
        let r = api(
            &app,
            "POST",
            "/api/v1/me/profile",
            Some(&t),
            Some(json!({"birth_date": d})),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{d}: {}", r.text);
        assert_eq!(r.json()["message"], "Please check the date.");
    }
    // the day before the 13th birthday: refused, account paused, birthday not stored
    let r = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&t),
        Some(json!({"birth_date": years_ago(13, 1)})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.text);
    assert_eq!(r.json()["error"], "account_paused");
    assert!(!r.text.contains("13"), "neutral message: {}", r.text);
    let (birth, paused): (Option<chrono::NaiveDate>, bool) = sqlx::query_as(
        "SELECT birth_date, age_review_at IS NOT NULL FROM users WHERE handle = 'kiddo'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(birth.is_none() && paused);
    // signed out everywhere and can't sign in again
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "kiddo", "password": "correct horse battery"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.json()["error"], "account_paused");
    let r = api(
        &app,
        "POST",
        "/api/v1/device/login",
        None,
        Some(json!({"login": "kiddo", "password": "correct horse battery"})),
    )
    .await;
    assert_eq!(r.json()["error"], "account_paused");

    // admin alert queued
    fraglink_server::alerts::check_age_reviews(&st)
        .await
        .unwrap();
    let (kind, subject): (String, String) =
        sqlx::query_as("SELECT kind, subject FROM alert_log ORDER BY sent_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "age_review");
    assert!(subject.contains("paused for an age check"), "{subject}");

    // Admin > Users shows it; clearing lets them sign in
    let u = api(
        &app,
        "GET",
        "/api/v1/admin/users?q=kiddo",
        Some(&boss),
        None,
    )
    .await;
    let row = &u.json()["users"][0];
    assert!(row["age_review_at"].is_string(), "{}", u.text);
    assert_eq!(row["is_minor"], false);
    let id = row["id"].as_str().unwrap().to_string();
    let d = api(
        &app,
        "GET",
        &format!("/api/v1/admin/users/{id}"),
        Some(&boss),
        None,
    )
    .await;
    assert!(d.json()["age_review_at"].is_string());
    assert!(d.json()["birth_date"].is_null());
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{id}/age_review/clear"),
        Some(&boss),
        None,
    )
    .await;
    assert!(r.status.is_success(), "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "kiddo", "password": "correct horse battery"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
}
