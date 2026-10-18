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
    api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&t),
        Some(json!({"game_id":"cs2","minutes":300})),
    )
    .await;

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
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.json()["delete_after"].is_string());
    // signed out everywhere straight away; data stays for the 7-day grace period
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE users SET delete_after = now() - interval '1 minute'")
        .execute(&pool)
        .await
        .unwrap();
    let st = fraglink_server::AppState::new(
        pool.clone(),
        fraglink_server::Config::for_tests("postgres://unused"),
    );
    assert_eq!(fraglink_server::account::purge_due(&st).await.unwrap(), 1);
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

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn old_sign_ins_are_purged(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let web = signup(&app, "purge@example.com", "purger").await;
    // eight tokens: web and device, in every state that matters
    for _ in 0..3 {
        pair(&app, &web).await;
    }
    for _ in 0..4 {
        let r = api(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({"email":"purge@example.com","password":"correct horse battery"})),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    let ids: Vec<(uuid::Uuid, String)> =
        sqlx::query_as("SELECT id, kind FROM auth_tokens ORDER BY kind, created_at")
            .fetch_all(&pool)
            .await
            .unwrap();
    let dev: Vec<uuid::Uuid> = ids
        .iter()
        .filter(|x| x.1 == "device")
        .map(|x| x.0)
        .collect();
    let webs: Vec<uuid::Uuid> = ids.iter().filter(|x| x.1 == "web").map(|x| x.0).collect();
    assert_eq!((dev.len(), webs.len()), (3, 5));
    let set = |sql: &'static str, id: uuid::Uuid| {
        let pool = pool.clone();
        async move {
            sqlx::query(sql).bind(id).execute(&pool).await.unwrap();
        }
    };
    // purged: web revoked 31 days ago, web expired 31 days ago, device revoked 31 days
    // ago, device unused for 181 days
    set(
        "UPDATE auth_tokens SET revoked_at = now() - interval '31 days' WHERE id = $1",
        webs[1],
    )
    .await;
    set(
        "UPDATE auth_tokens SET expires_at = now() - interval '31 days' WHERE id = $1",
        webs[2],
    )
    .await;
    set(
        "UPDATE auth_tokens SET revoked_at = now() - interval '31 days' WHERE id = $1",
        dev[0],
    )
    .await;
    set(
        "UPDATE auth_tokens SET last_used_at = now() - interval '181 days' WHERE id = $1",
        dev[1],
    )
    .await;
    // kept: web revoked 29 days ago, web unused for a year but not expired, device used
    // 179 days ago (and the signup token itself)
    set(
        "UPDATE auth_tokens SET revoked_at = now() - interval '29 days' WHERE id = $1",
        webs[3],
    )
    .await;
    set(
        "UPDATE auth_tokens SET last_used_at = now() - interval '365 days' WHERE id = $1",
        webs[4],
    )
    .await;
    set(
        "UPDATE auth_tokens SET last_used_at = now() - interval '179 days' WHERE id = $1",
        dev[2],
    )
    .await;
    let st = fraglink_server::AppState::new(
        pool.clone(),
        fraglink_server::Config::for_tests("postgres://unused"),
    );
    assert_eq!(
        fraglink_server::account::purge_tokens(&st).await.unwrap(),
        4
    );
    let left: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM auth_tokens")
        .fetch_all(&pool)
        .await
        .unwrap();
    let mut want = vec![webs[0], webs[3], webs[4], dev[2]];
    let mut got = left.clone();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    assert_eq!(
        fraglink_server::account::purge_tokens(&st).await.unwrap(),
        0
    );
}
