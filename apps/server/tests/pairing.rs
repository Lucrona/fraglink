mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn device_code_flow(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let web = signup(&app, "p@example.com", "pairer").await;

    let s = api(
        &app,
        "POST",
        "/api/v1/device/start",
        None,
        Some(json!({ "device_name": "Gaming PC" })),
    )
    .await;
    assert_eq!(s.status, StatusCode::OK);
    let s = s.json();
    let code = s["user_code"].as_str().unwrap();
    assert_eq!(code.len(), 9);
    assert_eq!(
        s["verification_uri_complete"],
        format!("http://localhost:8787/pair?code={code}")
    );
    // only the hash of the device code is stored
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM device_codes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let poll = || {
        api(
            &app,
            "POST",
            "/api/v1/device/poll",
            None,
            Some(json!({ "device_code": s["device_code"] })),
        )
    };
    assert_eq!(poll().await.json()["status"], "pending");

    // wrong code, then the right one typed in lower case with no dash
    let r = api(
        &app,
        "POST",
        "/api/v1/device/approve",
        Some(&web),
        Some(json!({ "user_code": "AAAA-AAAA" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/device/approve",
        Some(&web),
        Some(json!({ "user_code": code.replace('-', "").to_lowercase() })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["device_name"], "Gaming PC");
    // can't be approved twice
    let r = api(
        &app,
        "POST",
        "/api/v1/device/approve",
        Some(&web),
        Some(json!({ "user_code": code })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let p = poll().await.json();
    assert_eq!(p["status"], "approved");
    assert_eq!(p["user"]["handle"], "pairer");
    let dev = p["token"].as_str().unwrap().to_string();
    // single use
    assert_eq!(poll().await.status, StatusCode::NOT_FOUND);

    let me = api(&app, "GET", "/api/v1/me", Some(&dev), None)
        .await
        .json();
    assert_eq!(me["token_kind"], "device");
    let devices = api(&app, "GET", "/api/v1/devices", Some(&web), None)
        .await
        .json();
    let list = devices["devices"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["device_name"], "Gaming PC");
    // device tokens don't expire
    let exp: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("SELECT expires_at FROM auth_tokens WHERE kind = 'device'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(exp.is_none());

    let id = list[0]["id"].as_str().unwrap();
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/v1/devices/{id}"),
            Some(&web),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&dev), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn expired_and_unknown_codes(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let web = signup(&app, "e@example.com", "expirer").await;
    let s = api(&app, "POST", "/api/v1/device/start", None, Some(json!({})))
        .await
        .json();
    sqlx::query("UPDATE device_codes SET expires_at = now() - interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/device/approve",
        Some(&web),
        Some(json!({ "user_code": s["user_code"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = api(
        &app,
        "POST",
        "/api/v1/device/poll",
        None,
        Some(json!({ "device_code": s["device_code"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::GONE);
    assert_eq!(r.json()["error"], "expired_token");
    let r = api(
        &app,
        "POST",
        "/api/v1/device/poll",
        None,
        Some(json!({ "device_code": "nope" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // default device name
    let n: String = sqlx::query_scalar("SELECT device_name FROM device_codes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, "Fraglink desktop");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn browser_pairing_pages(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let s = api(
        &app,
        "POST",
        "/api/v1/device/start",
        None,
        Some(json!({ "device_name": "Laptop" })),
    )
    .await
    .json();
    let code = s["user_code"].as_str().unwrap();

    // signed out: bounced to login, and back to the pair page afterwards
    let r = page(&app, &format!("/pair?code={code}"), None).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(r.location(), format!("/login?next=%2Fpair%3Fcode%3D{code}"));

    let r = form(&app, "/signup", None, Some(ORIGIN), &format!("email=w%40example.com&handle=webby&password=correct+horse+battery&dob_year=1990&dob_month=01&dob_day=15&next=%2Fpair%3Fcode%3D{code}")).await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.text);
    assert_eq!(r.location(), format!("/pair?code={code}"));
    let set = r
        .headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        set.contains("HttpOnly") && set.contains("SameSite=Lax"),
        "{set}"
    );
    let cookie = r.cookie().unwrap();

    let r = page(&app, &format!("/pair?code={code}"), Some(&cookie)).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains(code));

    // cross-site post is refused
    let r = form(
        &app,
        "/pair",
        Some(&cookie),
        Some("https://evil.example"),
        &format!("code={code}"),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let r = form(
        &app,
        "/pair",
        Some(&cookie),
        Some(ORIGIN),
        &format!("code={code}"),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("Laptop"), "{}", r.text);
    let p = api(
        &app,
        "POST",
        "/api/v1/device/poll",
        None,
        Some(json!({ "device_code": s["device_code"] })),
    )
    .await
    .json();
    assert_eq!(p["status"], "approved");
    assert_eq!(p["user"]["handle"], "webby");
}
