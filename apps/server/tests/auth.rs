mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn signup_login_logout(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "Ana@Example.com", "ana_1").await;
    // password is stored as argon2id, never plaintext
    let h: String = sqlx::query_scalar("SELECT password_hash FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(h.starts_with("$argon2id$"), "{h}");
    // tokens are stored hashed
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM auth_tokens WHERE token_hash = convert_to($1, 'UTF8')",
    )
    .bind(&t)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 0);

    let me = api(&app, "GET", "/api/v1/me", Some(&t), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json()["user"]["handle"], "ana_1");
    assert_eq!(me.json()["user"]["email"], "Ana@Example.com"); // stored as typed, unique case-insensitively

    // duplicate email (case-insensitive) and duplicate handle
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/signup",
        None,
        Some(json!({"email":"ANA@example.com","password":"another long pass","handle":"other"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/signup",
        None,
        Some(json!({"email":"b@example.com","password":"another long pass","handle":"ANA_1"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    // validation
    for body in [
        json!({"email":"nope","password":"long enough pass","handle":"abc"}),
        json!({"email":"c@example.com","password":"short","handle":"abc"}),
        json!({"email":"c@example.com","password":"long enough pass","handle":"a b"}),
    ] {
        let r = api(&app, "POST", "/api/v1/auth/signup", None, Some(body)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
    }

    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"ana@example.com","password":"wrong password"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"nobody@example.com","password":"wrong password"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"ANA@example.com","password":"correct horse battery"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let t2 = r.json()["token"].as_str().unwrap().to_string();

    assert_eq!(
        api(&app, "POST", "/api/v1/auth/logout", Some(&t2), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t2), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::OK
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some("garbage"), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", None, None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn login_rate_limit(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    signup(&app, "rl@example.com", "rlx").await;
    for _ in 0..10 {
        let r = api(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({"email":"rl@example.com","password":"bad bad bad"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // even the right password is refused while locked
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email":"rl@example.com","password":"correct horse battery"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{}", r.text);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn expired_web_token_is_rejected(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "exp@example.com", "exp").await;
    sqlx::query("UPDATE auth_tokens SET expires_at = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn delete_account_removes_everything(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "del@example.com", "del").await;
    let dev = pair(&app, &t).await;
    let id = uuid();
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&dev),
        Some(batch(
            json!([{"id":"cs2","name":"Counter-Strike 2","steam_appid":730}]),
            vec![session(&id, "cs2", now() - 7200, 3600)],
        )),
    )
    .await;
    assert_eq!(r.json()["accepted"].as_array().unwrap().len(), 1);
    api(&app, "POST", "/api/v1/hours/estimate", Some(&t), Some(json!({"game_name":"Old Game","eras":[{"from_year":2005,"to_year":2006,"hours_per_week":5}]}))).await;

    let r = api(
        &app,
        "DELETE",
        "/api/v1/me",
        Some(&t),
        Some(json!({"password":"wrong"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = api(
        &app,
        "DELETE",
        "/api/v1/me",
        Some(&t),
        Some(json!({"password":"correct horse battery"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{}", r.text);
    for table in [
        "users",
        "auth_tokens",
        "sessions",
        "hours_entries",
        "linked_accounts",
        "steam_playtime",
    ] {
        let n: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(n, 0, "{table} not empty");
    }
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&dev), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn export_contains_own_data_only(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let a = signup(&app, "a@example.com", "aaa").await;
    let b = signup(&app, "b@example.com", "bbb").await;
    api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&a),
        Some(json!({"game_name":"World of Warcraft","minutes":6000,"source":"wow_manual"})),
    )
    .await;
    api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&b),
        Some(json!({"game_name":"Dota","minutes":60})),
    )
    .await;
    let r = api(&app, "GET", "/api/v1/export", Some(&a), None).await;
    assert_eq!(r.status, StatusCode::OK);
    let v = r.json();
    assert_eq!(v["format"], "fraglink-server-export");
    assert_eq!(v["user"]["handle"], "aaa");
    let h = v["hours_entries"].as_array().unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0]["minutes"], 6000);
    assert!(
        !r.text.contains("password"),
        "export must not include the password hash"
    );
    assert!(!r.text.contains("token_hash"));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn desktop_signup_login_search(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    // no email needed; two accounts without email don't clash
    for h in ["zed_one", "zed_two"] {
        let r = api(
            &app,
            "POST",
            "/api/v1/device/signup",
            None,
            Some(json!({"handle": h, "password": "a long enough pass", "device_name": "PC"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    }
    let kind: String = sqlx::query_scalar("SELECT kind FROM auth_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(kind, "device");
    // login by username, case-insensitive
    let r = api(
        &app,
        "POST",
        "/api/v1/device/login",
        None,
        Some(json!({"login": "ZED_ONE", "password": "a long enough pass"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let tok = r.json()["token"].as_str().unwrap().to_string();
    let r = api(
        &app,
        "POST",
        "/api/v1/device/login",
        None,
        Some(json!({"login": "zed_one", "password": "wrong password!"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // search finds the other user, not yourself
    let r = api(&app, "GET", "/api/v1/users/search?q=zed", Some(&tok), None).await;
    assert_eq!(r.status, StatusCode::OK);
    let body = r.json();
    let users = body["users"].as_array().unwrap();
    assert_eq!(users.len(), 1, "{}", r.text);
    assert_eq!(users[0]["handle"], "zed_two");
}
