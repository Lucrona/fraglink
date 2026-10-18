//! Website v2: two-step sign-in, sessions, avatars, home/profile data, signature image,
//! email verification (logged mailer), deletion grace period, Turnstile.
mod common;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::*;
use serde_json::json;
use sqlx::PgPool;

const PW: &str = "correct horse battery";

fn code_now(secret_b32: &str) -> String {
    let s = base32_decode(&secret_b32.replace(' ', ""));
    let step = fraglink_server::mfa::current_step();
    format!("{:06}", fraglink_server::mfa::totp_at(&s, step))
}

fn base32_decode(s: &str) -> Vec<u8> {
    let a = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut buf, mut bits, mut out) = (0u32, 0u32, vec![]);
    for c in s.bytes() {
        let v = a.iter().position(|x| *x == c).unwrap() as u32;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            out.push((buf >> (bits - 8)) as u8);
            bits -= 8;
        }
    }
    out
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn mfa_flow(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "m@example.com", "mfauser").await;
    let s = api(
        &app,
        "POST",
        "/api/v1/me/mfa/setup",
        Some(&t),
        Some(json!({"password": PW})),
    )
    .await;
    assert_eq!(s.status, StatusCode::OK, "{}", s.text);
    let secret = s.json()["secret"].as_str().unwrap().to_string();
    assert!(s.json()["qr_svg"].as_str().unwrap().contains("<svg"));
    assert!(s.json()["otpauth_uri"]
        .as_str()
        .unwrap()
        .starts_with("otpauth://totp/Fraglink"));
    let bad = api(
        &app,
        "POST",
        "/api/v1/me/mfa/enable",
        Some(&t),
        Some(json!({"code": "000000"})),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    let e = api(
        &app,
        "POST",
        "/api/v1/me/mfa/enable",
        Some(&t),
        Some(json!({"code": code_now(&secret)})),
    )
    .await;
    assert_eq!(e.status, StatusCode::OK, "{}", e.text);
    let codes: Vec<String> = e.json()["backup_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    assert_eq!(codes.len(), 10);

    // web login now needs the second step
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "mfauser", "password": PW})),
    )
    .await;
    assert_eq!(l.status, StatusCode::OK);
    assert_eq!(l.json()["mfa_required"], true);
    assert!(l.json().get("token").is_none());
    let ticket = l.json()["mfa_token"].as_str().unwrap().to_string();
    // same TOTP step as enable() can't be replayed
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/mfa",
        None,
        Some(json!({"mfa_token": ticket, "code": code_now(&secret)})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "replay must fail");
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/mfa",
        None,
        Some(json!({"mfa_token": ticket, "code": codes[0]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.json()["token"].is_string());
    // backup code is single-use
    let l = api(
        &app,
        "POST",
        "/api/v1/device/login",
        None,
        Some(json!({"login": "mfauser", "password": PW, "device_name": "PC"})),
    )
    .await;
    assert_eq!(l.json()["mfa_required"], true);
    let ticket = l.json()["mfa_token"].as_str().unwrap().to_string();
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/mfa",
        None,
        Some(json!({"mfa_token": ticket, "code": codes[0]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/mfa",
        None,
        Some(json!({"mfa_token": ticket, "code": codes[1].to_uppercase()})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let dev = r.json()["token"].as_str().unwrap().to_string();
    let me = api(&app, "GET", "/api/v1/me", Some(&dev), None)
        .await
        .json();
    assert_eq!(me["token_kind"], "device");
    assert_eq!(me["user"]["mfa"]["enabled"], true);
    assert_eq!(me["user"]["mfa"]["backup_codes_left"], 8);
    // too many wrong codes kill the ticket
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "mfauser", "password": PW})),
    )
    .await;
    let ticket = l.json()["mfa_token"].as_str().unwrap().to_string();
    sqlx::query("UPDATE mfa_tickets SET attempts = 5")
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/mfa",
        None,
        Some(json!({"mfa_token": ticket, "code": codes[2]})),
    )
    .await;
    assert_ne!(r.status, StatusCode::OK);
    // disable needs password + code
    let r = api(
        &app,
        "POST",
        "/api/v1/me/mfa/disable",
        Some(&t),
        Some(json!({"password": "nope", "code": codes[3]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/mfa/disable",
        Some(&t),
        Some(json!({"password": PW, "code": codes[3]})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "mfauser", "password": PW})),
    )
    .await;
    assert!(l.json()["token"].is_string());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn admin_resets_mfa_with_audit(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let boss = signup(&app, "b@example.com", "boss").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'boss'")
        .execute(&pool)
        .await
        .unwrap();
    let t = signup(&app, "u@example.com", "locked").await;
    let s = api(
        &app,
        "POST",
        "/api/v1/me/mfa/setup",
        Some(&t),
        Some(json!({"password": PW})),
    )
    .await
    .json();
    api(
        &app,
        "POST",
        "/api/v1/me/mfa/enable",
        Some(&t),
        Some(json!({"code": code_now(s["secret"].as_str().unwrap())})),
    )
    .await;
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'locked'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{id}/mfa/reset"),
        Some(&boss),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["was_enabled"], true);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = 'user.mfa_reset'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(n, 1);
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "locked", "password": PW})),
    )
    .await;
    assert!(l.json()["token"].is_string());
    // non-staff get 404
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{id}/mfa/reset"),
        Some(&t),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn sessions_list_and_revoke(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "s@example.com", "sess").await;
    let ua_login = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/129.0 Safari/537.36",
        )
        .header("x-forwarded-for", "203.0.113.9")
        .body(Body::from(
            json!({"email": "sess", "password": PW}).to_string(),
        ))
        .unwrap();
    let t2 = send(&app, ua_login).await.json()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let _dev = pair(&app, &t).await;
    let l = api(&app, "GET", "/api/v1/me/sessions", Some(&t), None)
        .await
        .json();
    let list = l["sessions"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list.iter().filter(|s| s["current"] == true).count(), 1);
    let win = list
        .iter()
        .find(|s| s["device"] == "Windows · Chrome")
        .expect("ua parsed");
    assert_eq!(win["ip"], "203.0.x.x");
    // new sign-in email logged (mail off)
    let o = fraglink_server::mailer::outbox().lock().unwrap().clone();
    assert!(o
        .iter()
        .any(|m| m.to == "s@example.com" && m.subject == "New sign-in from Windows · Chrome"));
    let id = win["id"].as_str().unwrap();
    assert_eq!(
        api(
            &app,
            "DELETE",
            &format!("/api/v1/me/sessions/{id}"),
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t2), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/me/sessions/revoke_all",
        Some(&t),
        None,
    )
    .await;
    assert_eq!(r.json()["revoked"], 1);
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::OK
    );
}

fn png(w: u32, h: u32, seed: u8) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        image::Rgba([(x as u8) ^ seed, y as u8, seed, 255])
    });
    let mut b = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png)
        .unwrap();
    b
}

async fn upload(app: &axum::Router, t: &str, bytes: Vec<u8>) -> Resp {
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/me/avatar")
        .header(header::AUTHORIZATION, format!("Bearer {t}"))
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(bytes))
        .unwrap();
    send(app, req).await
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn avatars_rules_report_and_remove(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    let app = fraglink_server::router(state_with(pool.clone(), |c| c.data_dir = d));
    let t = signup(&app, "a@example.com", "pic").await;
    let me = api(&app, "GET", "/api/v1/me", Some(&t), None).await.json();
    assert_eq!(me["user"]["avatar"]["kind"], "default");
    assert_eq!(me["user"]["avatar_upload_allowed"], false);
    // young account: presets only
    let r = upload(&app, &t, png(300, 200, 1)).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.text);
    let r = api(
        &app,
        "POST",
        "/api/v1/me/avatar/preset",
        Some(&t),
        Some(json!({"preset": 3})),
    )
    .await;
    assert_eq!(r.json()["kind"], "preset");
    sqlx::query("UPDATE users SET created_at = now() - interval '2 days'")
        .execute(&pool)
        .await
        .unwrap();
    // not an image
    assert_eq!(
        upload(&app, &t, b"<svg onload=alert(1)>".to_vec())
            .await
            .status,
        StatusCode::BAD_REQUEST
    );
    let r = upload(&app, &t, png(300, 200, 2)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let url = r.json()["url_64"].as_str().unwrap().to_string();
    let img = api(&app, "GET", &url, None, None).await;
    assert_eq!(img.status, StatusCode::OK);
    assert!(img.headers[header::CACHE_CONTROL]
        .to_str()
        .unwrap()
        .contains("immutable"));
    let dec = image::load_from_memory(&img.bytes).unwrap();
    assert_eq!((dec.width(), dec.height()), (64, 64));
    // friends list carries the avatar for the desktop app
    let f = signup(&app, "f@example.com", "fan").await;
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&f),
        Some(json!({"handle": "pic"})),
    )
    .await;
    api(
        &app,
        "POST",
        "/api/v1/friends/respond",
        Some(&t),
        Some(json!({"handle": "fan", "accept": true})),
    )
    .await;
    let fl = api(&app, "GET", "/api/v1/friends", Some(&f), None)
        .await
        .json();
    assert_eq!(fl["friends"][0]["avatar"]["url_64"], url.as_str());
    // report -> admin queue -> remove + block
    let r = api(
        &app,
        "POST",
        "/api/v1/users/pic/avatar/report",
        Some(&f),
        Some(json!({"reason": "rude"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let boss = signup(&app, "b@example.com", "boss").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'boss'")
        .execute(&pool)
        .await
        .unwrap();
    let q = api(
        &app,
        "GET",
        "/api/v1/admin/avatar_reports",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(q["reports"][0]["handle"], "pic");
    let uid = q["reports"][0]["user_id"].as_str().unwrap().to_string();
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{uid}/avatar/remove"),
        Some(&boss),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(&app, "GET", &url, None, None).await.status,
        StatusCode::NOT_FOUND
    );
    let me = api(&app, "GET", "/api/v1/me", Some(&t), None).await.json();
    assert_eq!(me["user"]["avatar"]["kind"], "default");
    let r = upload(&app, &t, png(300, 200, 2)).await;
    assert_eq!(r.json()["error"], "avatar_blocked");
    // 5 changes a day (preset + upload + blocked attempt doesn't count = 2 so far)
    for i in 0..3 {
        assert_eq!(
            upload(&app, &t, png(40, 40, 10 + i)).await.status,
            StatusCode::OK
        );
    }
    assert_eq!(
        upload(&app, &t, png(40, 40, 50)).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn home_profile_and_signature(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "h1@example.com", "alpha").await;
    let _b = signup(&app, "h2@example.com", "bravo").await;
    // public by default: listed on the home page
    let h = api(&app, "GET", "/api/v1/home", Some(&a), None)
        .await
        .json();
    assert_eq!(h["stats"]["registered"], 2);
    assert_eq!(h["newest"].as_array().unwrap().len(), 2);
    assert!(h["me"]["minutes_7d"].is_number());
    assert_eq!(
        api(&app, "GET", "/api/v1/sig/alpha.png", None, None)
            .await
            .status,
        StatusCode::OK
    );
    // private profile: name + picture only, off the home lists, no signature
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&a),
        Some(json!({"public_profile": false})),
    )
    .await;
    assert_eq!(r.json()["public_profile"], false);
    assert_eq!(
        api(&app, "GET", "/api/v1/users/alpha/profile", None, None)
            .await
            .json()["private"],
        true
    );
    let h = api(&app, "GET", "/api/v1/home", Some(&_b), None)
        .await
        .json();
    assert!(!h.to_string().contains("\"alpha\""), "{h}");
    assert_eq!(
        api(&app, "GET", "/api/v1/sig/alpha.png", None, None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&a),
        Some(json!({"public_profile": true})),
    )
    .await;
    assert_eq!(r.json()["public_profile"], true);
    assert_eq!(r.json()["share_activity"], true);
    let p = api(&app, "GET", "/api/v1/users/alpha/profile", None, None).await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.text);
    assert_eq!(p.json()["relation"], "public");
    let s = api(&app, "GET", "/api/v1/sig/alpha.png", None, None).await;
    assert_eq!(s.status, StatusCode::OK);
    assert_eq!(&s.bytes[..4], b"\x89PNG");
    // profile fields + rig
    let r = api(&app, "POST", "/api/v1/me/profile", Some(&a), Some(json!({"about": "hi", "gaming_style": "Casual", "rig": {"cpu": "Ryzen 7", "bogus": "x"}}))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = api(&app, "GET", "/api/v1/users/alpha/profile", Some(&a), None)
        .await
        .json();
    assert_eq!(p["rig"]["cpu"], "Ryzen 7");
    assert!(p["rig"].get("bogus").is_none());
    assert_eq!(p["gaming_style"], "Casual");
    let r = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&a),
        Some(json!({"gaming_style": "Hacker"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn email_verification_is_logged_not_blocking(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "verify-me@example.com", "verifier").await;
    let mail = fraglink_server::mailer::outbox()
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m.to == "verify-me@example.com" && m.subject.contains("Confirm"))
        .cloned()
        .expect("verification mail logged");
    let token = mail
        .body
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let me = api(&app, "GET", "/api/v1/me", Some(&t), None).await.json();
    assert_eq!(me["user"]["email_verified"], false);
    assert_eq!(me["user"]["mail_live"], false);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/verify",
        None,
        Some(json!({"token": token})),
    )
    .await;
    assert_eq!(r.json()["verified"], true);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/verify",
        None,
        Some(json!({"token": token})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let me = api(&app, "GET", "/api/v1/me", Some(&t), None).await.json();
    assert_eq!(me["user"]["email_verified"], true);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn deletion_grace_cancelled_by_login(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "g@example.com", "grace").await;
    let r = api(
        &app,
        "DELETE",
        "/api/v1/me",
        Some(&t),
        Some(json!({"password": PW})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "grace", "password": PW})),
    )
    .await
    .json();
    assert_eq!(l["deletion_cancelled"], true);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE delete_after IS NOT NULL")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn turnstile_required_when_configured(pool: PgPool) {
    let app = fraglink_server::router(state_with(pool, |c| c.turnstile_secret = Some("x".into())));
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/signup",
        None,
        Some(json!({"email": "", "password": PW, "handle": "bot"})),
    )
    .await;
    assert_eq!(r.json()["error"], "bot_check");
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "bot", "password": PW})),
    )
    .await;
    assert_eq!(r.json()["error"], "bot_check");
    // the desktop app isn't asked
    let r = api(
        &app,
        "POST",
        "/api/v1/device/signup",
        None,
        Some(json!({"handle": "app1", "password": PW, "birth_date": "1990-01-15"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn forgot_and_reset_password(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "forgot-me@example.com", "forgetful").await;
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/forgot",
        None,
        Some(json!({"email": "nobody@example.com"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/forgot",
        None,
        Some(json!({"email": "Forgot-Me@example.com"})),
    )
    .await;
    assert_eq!(r.json()["mail_live"], false);
    let mail = fraglink_server::mailer::outbox()
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m.to == "forgot-me@example.com" && m.subject.contains("Reset"))
        .cloned()
        .expect("reset mail logged");
    let token = mail
        .body
        .split("token=")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/reset",
        None,
        Some(json!({"token": token, "password": "a brand new password"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&t), None).await.status,
        StatusCode::UNAUTHORIZED
    );
    let l = api(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        Some(json!({"email": "forgetful", "password": "a brand new password"})),
    )
    .await;
    assert!(l.json()["token"].is_string());
    let r = api(
        &app,
        "POST",
        "/api/v1/auth/reset",
        None,
        Some(json!({"token": token, "password": "another password!"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn per_feature_privacy_and_grouped_activity(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "pf1@example.com", "pfa").await;
    let b = signup(&app, "pf2@example.com", "pfb").await;
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&a),
        Some(json!({"handle": "pfb"})),
    )
    .await;
    api(
        &app,
        "POST",
        "/api/v1/friends/request",
        Some(&b),
        Some(json!({"handle": "pfa"})),
    )
    .await;
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    // three sessions today, one yesterday -> two grouped rows
    for (ago, secs) in [(1i32, 3600), (3, 1800), (5, 600), (30, 1200)] {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             SELECT gen_random_uuid(), id, 'wow', date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' + interval '20 hours' - make_interval(hours => $1) - make_interval(secs => $2),
                    date_trunc('day', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC' + interval '20 hours' - make_interval(hours => $1), $2
             FROM users WHERE handle = 'pfb'",
        )
        .bind(ago).bind(secs as f64).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE sessions SET started_at = started_at - interval '1 day', ended_at = ended_at - interval '1 day' WHERE ended_at > now()")
        .execute(&pool).await.unwrap();
    let act = api(
        &app,
        "GET",
        "/api/v1/friends/activity?tz=UTC",
        Some(&a),
        None,
    )
    .await
    .json();
    let items = act["items"].as_array().unwrap();
    let played: Vec<_> = items.iter().filter(|i| i["kind"] == "played").collect();
    assert!(played.len() <= 2 && !played.is_empty(), "{act}");
    let total: i64 = played
        .iter()
        .map(|i| i["duration_s"].as_i64().unwrap())
        .sum();
    assert_eq!(total, 3600 + 1800 + 600 + 1200, "{act}");
    assert!(played[0]["day"].is_string() && played[0]["sessions"].as_i64().unwrap() >= 1);
    // bogus tz falls back
    assert_eq!(
        api(
            &app,
            "GET",
            "/api/v1/friends/activity?tz=Mars/Base",
            Some(&a),
            None
        )
        .await
        .status,
        StatusCode::OK
    );

    // per-feature hides
    api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&b),
        Some(json!({"about": "hello", "location": "Perth", "rig": {"cpu": "X"}})),
    )
    .await;
    let r = api(&app, "POST", "/api/v1/me/privacy", Some(&b),
        Some(json!({"privacy": {"games": false, "rig": false, "location": false, "now_playing": false, "leaderboards": false}}))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["privacy"]["games"], false);
    assert_eq!(r.json()["privacy"]["about"], true);
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/me/privacy",
            Some(&b),
            Some(json!({"privacy": {"bogus": false}}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    for viewer in [Some(a.as_str()), None] {
        let p = api(&app, "GET", "/api/v1/users/pfb/profile", viewer, None)
            .await
            .json();
        assert_eq!(p["games"].as_array().unwrap().len(), 0, "{p}");
        assert!(p["rig"].is_null() && p["location"].is_null(), "{p}");
        assert_eq!(p["about"], "hello");
        assert!(p["presence"]["game_id"].is_null());
    }
    let own = api(&app, "GET", "/api/v1/users/pfb/profile", Some(&b), None)
        .await
        .json();
    assert_eq!(own["games"].as_array().unwrap().len(), 1);
    assert_eq!(own["location"], "Perth");
    let h = api(&app, "GET", "/api/v1/home", Some(&a), None)
        .await
        .json();
    assert!(
        h["top_players"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["handle"] != "pfb"),
        "{h}"
    );
    let me = api(&app, "GET", "/api/v1/me", Some(&b), None).await.json();
    assert_eq!(me["user"]["privacy"]["rig"], false);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn connect_steam_start_and_unlink(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "cn@example.com", "conny").await;
    let c = api(&app, "GET", "/api/v1/connect", Some(&t), None).await;
    assert_eq!(c.status, StatusCode::OK, "{}", c.text);
    assert_eq!(c.json()["steam"]["available"], true);
    let r = api(&app, "POST", "/api/v1/connect/steam/start", Some(&t), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let url = r.json()["url"].as_str().unwrap().to_string();
    assert!(
        url.contains("openid.return_to=") && url.contains("state%3D"),
        "{url}"
    );
    assert_eq!(
        api(&app, "POST", "/api/v1/connect/nope/start", Some(&t), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    // unknown state -> back to settings with an error
    let r = api(
        &app,
        "GET",
        "/api/v1/connect/steam/return?state=bogus",
        None,
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    sqlx::query("INSERT INTO linked_accounts(user_id, provider, provider_uid, verified) SELECT id, 'discord', '1', true FROM users WHERE handle = 'conny'")
        .execute(&pool).await.unwrap();
    assert_eq!(
        api(&app, "DELETE", "/api/v1/connect/discord", Some(&t), None)
            .await
            .status,
        StatusCode::OK
    );
    let c = api(&app, "GET", "/api/v1/connect", Some(&t), None)
        .await
        .json();
    assert!(c["discord"]["linked"].is_null());
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn profile_details_events_achievements_report(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "pd1@example.com", "pda").await;
    let b = signup(&app, "pd2@example.com", "pdb").await;
    for (t, h) in [(&a, "pdb"), (&b, "pda")] {
        api(
            &app,
            "POST",
            "/api/v1/friends/request",
            Some(t),
            Some(json!({"handle": h})),
        )
        .await;
    }
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), id, 'wow', now() - interval '13 hours', now() - interval '1 hour', 43200 FROM users WHERE handle = 'pda'",
    )
    .execute(&pool).await.unwrap();
    // clear signup DOB so the first profile save can set it (afterwards it's locked)
    sqlx::query("UPDATE users SET birth_date = NULL, age_confirmed_at = NULL WHERE handle = 'pda'")
        .execute(&pool)
        .await
        .unwrap();
    let r = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&a),
        Some(json!({
            "about": null, "location": "Sydney", "gaming_style": null, "rig": {},
            "country": "au", "real_name": "Al", "gender": "Male", "birth_date": "2000-01-15"
        })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json()["details"]["country"], "AU");
    let bad = api(
        &app,
        "POST",
        "/api/v1/me/profile",
        Some(&a),
        Some(json!({"birth_date": "2099-01-01"})), // locked once set; under-13 pause is in tests/minors.rs
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.json()["error"], "dob_locked");
    let p = api(&app, "GET", "/api/v1/users/pda/profile", Some(&b), None)
        .await
        .json();
    assert_eq!(p["country"], "AU");
    assert_eq!(p["real_name"], "Al");
    assert!(p["age"].as_i64().unwrap() >= 25);
    let kinds: Vec<&str> = p["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"friend") && kinds.contains(&"first_play"),
        "{kinds:?}"
    );
    assert_eq!(p["achievements"]["rows"][0]["tier"], "Bronze"); // 12 hours
    assert_eq!(p["achievements"]["rows"][4]["tier"], "Platinum"); // 12-hour session
                                                                  // hide age + achievements
    api(
        &app,
        "POST",
        "/api/v1/me/privacy",
        Some(&a),
        Some(json!({"privacy": {"age": false, "achievements": false}})),
    )
    .await;
    let p = api(&app, "GET", "/api/v1/users/pda/profile", Some(&b), None)
        .await
        .json();
    assert!(p["age"].is_null() && p["achievements"].is_null());
    // report
    let r = api(
        &app,
        "POST",
        "/api/v1/users/pda/report",
        Some(&b),
        Some(json!({"reason": "rude"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/users/pda/report",
            Some(&a),
            Some(json!({}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM profile_reports")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn paged_lists(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "pg1@example.com", "pga").await;
    let b = signup(&app, "pg2@example.com", "pgb").await;
    for (t, h) in [(&a, "pgb"), (&b, "pga")] {
        api(
            &app,
            "POST",
            "/api/v1/friends/request",
            Some(t),
            Some(json!({"handle": h})),
        )
        .await;
    }
    for i in 0..30 {
        sqlx::query(
            "INSERT INTO games (id, name, verified) VALUES ($1, $2, true) ON CONFLICT DO NOTHING",
        )
        .bind(format!("g{i}"))
        .bind(format!("Game {i}"))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             SELECT gen_random_uuid(), id, $1, now() - make_interval(secs => $2 + 60), now() - interval '1 minute', $2 FROM users WHERE handle = 'pga'",
        )
        .bind(format!("g{i}")).bind(((i + 1) * 600) as f64).execute(&pool).await.unwrap();
    }
    let r = api(
        &app,
        "GET",
        "/api/v1/me/games?limit=10&q=game%202&sort=name",
        Some(&a),
        None,
    )
    .await
    .json();
    assert_eq!(r["total"], 11); // Game 2, 20-29
    assert_eq!(r["games"].as_array().unwrap().len(), 10);
    assert_eq!(r["all"], 30);
    let p = api(
        &app,
        "GET",
        "/api/v1/users/pga/profile?limit=10",
        Some(&b),
        None,
    )
    .await
    .json();
    assert_eq!(p["games"].as_array().unwrap().len(), 10);
    assert_eq!(p["games_total"], 30);
    let g = api(
        &app,
        "GET",
        "/api/v1/users/pga/profile?show=games&limit=5&offset=25",
        Some(&b),
        None,
    )
    .await
    .json();
    assert_eq!(g["games"].as_array().unwrap().len(), 5);
    assert_eq!(g["total"], 30);
    let f = api(
        &app,
        "GET",
        "/api/v1/friends/page?q=PGB&limit=5",
        Some(&a),
        None,
    )
    .await
    .json();
    assert_eq!(f["total"], 1);
    assert_eq!(f["all"], 1);
    let f = api(
        &app,
        "GET",
        "/api/v1/friends/page?show=online",
        Some(&a),
        None,
    )
    .await
    .json();
    assert_eq!(f["total"], 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn admin_game_totals_count_all_tiers(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "ag@example.com", "agboss").await;
    let u = signup(&app, "ag2@example.com", "agplayer").await;
    let _ = u;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'agboss'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('rivals', 'Marvel Rivals', true), ('tiny', 'Tiny', true) ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO steam_playtime (user_id, steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at) SELECT id, 1, 'rivals', 46800, 0, now() FROM users WHERE handle = 'agplayer'")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO hours_entries (id, user_id, game_id, tier, source, minutes) SELECT gen_random_uuid(), id, 'tiny', 'estimated', 'manual', 60 FROM users WHERE handle = 'agplayer'")
        .execute(&pool).await.unwrap();
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/games?sort=hours",
        Some(&t),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let g = r.json();
    assert_eq!(g["games"][0]["id"], "rivals");
    assert_eq!(g["games"][0]["players"], 1);
    assert_eq!(g["games"][0]["total_min"], 46800);
    // an old estimate (tier removed) counts for nothing: tiny has no hours
    assert!(
        g["games"]
            .as_array()
            .unwrap()
            .iter()
            .all(|x| x["id"] != "tiny"),
        "{g}"
    );
    assert!(g["games"][0].get("estimated_min").is_none(), "{g}");
    let s = api(&app, "GET", "/api/v1/admin/games?q=tiny", Some(&t), None)
        .await
        .json();
    let tiny = s["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["id"] == "tiny")
        .cloned()
        .unwrap();
    assert_eq!(tiny["total_min"], 0);
    assert_eq!(tiny["players"], 0);
    let d = api(&app, "GET", "/api/v1/admin/games/rivals", Some(&t), None)
        .await
        .json();
    assert_eq!(d["players"], 1);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn unknown_ids_and_game_delete(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "ug@example.com", "ugboss").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'ugboss'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('weird-id', 'Unlisted game', false), ('bogus', 'Unlisted game', false), ('real', 'Real Game', true) ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO unknown_games (game_id) VALUES ('weird-id')")
        .execute(&pool)
        .await
        .unwrap();
    for g in ["weird-id", "bogus"] {
        sqlx::query("INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s) SELECT gen_random_uuid(), id, $1, now() - interval '1 hour', now(), 3600 FROM users WHERE handle = 'ugboss'")
            .bind(g).execute(&pool).await.unwrap();
    }
    let u = api(&app, "GET", "/api/v1/admin/unknown_games", Some(&t), None)
        .await
        .json();
    assert_eq!(u["unknown"][0]["game_id"], "weird-id");
    assert_eq!(u["unknown"][0]["players"], 1);
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/unknown_games/weird-id/map",
        Some(&t),
        Some(json!({"to": "real"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = 'real'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // delete: needs the id typed; only unlisted entries
    let d = api(&app, "GET", "/api/v1/admin/games/bogus", Some(&t), None)
        .await
        .json();
    assert_eq!(d["deletable"], true);
    assert_eq!(d["sessions"], 1);
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/admin/games/bogus/delete",
            Some(&t),
            Some(json!({"confirm": "nope"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/admin/games/real/delete",
            Some(&t),
            Some(json!({"confirm": "real"}))
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/bogus/delete",
        Some(&t),
        Some(json!({"confirm": "bogus"})),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM games WHERE id = 'bogus'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn merge_game_into_another(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let t = signup(&app, "mg@example.com", "mgboss").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'mgboss'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('oldgame', 'Old Game', true), ('newgame', 'New Game', true)")
        .execute(&pool).await.unwrap();
    sqlx::query(
        "INSERT INTO game_overrides (game_id, extra_exes) VALUES ('oldgame', '{oldgame.exe}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'mgboss'")
        .fetch_one(&pool)
        .await
        .unwrap();
    for _ in 0..2 {
        sqlx::query("INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s) VALUES (gen_random_uuid(), $1, 'oldgame', now() - interval '1 hour', now(), 3600)")
            .bind(uid).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO hidden_games (user_id, game_id) VALUES ($1, 'oldgame')")
        .bind(uid)
        .execute(&pool)
        .await
        .unwrap();

    let p = api(
        &app,
        "GET",
        "/api/v1/admin/games/oldgame/merge?into=newgame",
        Some(&t),
        None,
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.text);
    let p = p.json();
    assert_eq!(p["from"]["totals"]["sessions"], 2);
    assert_eq!(p["from"]["totals"]["players"], 1);
    assert_eq!(p["from"]["totals"]["tracked_min"], 120);
    assert_eq!(p["from"]["totals"]["hidden_by"], 1);
    assert_eq!(
        api(
            &app,
            "GET",
            "/api/v1/admin/games/oldgame/merge?into=oldgame",
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let body = json!({"into": "newgame", "confirm": "nope"});
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/admin/games/oldgame/merge",
            Some(&t),
            Some(body)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    let body = json!({"into": "newgame", "confirm": "oldgame"});
    let r = api(
        &app,
        "POST",
        "/api/v1/admin/games/oldgame/merge",
        Some(&t),
        Some(body),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = 'newgame'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
    let h: i64 = sqlx::query_scalar("SELECT count(*) FROM hidden_games WHERE game_id = 'newgame'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(h, 1);
    let m: bool =
        sqlx::query_scalar("SELECT merged FROM game_retirements WHERE game_id = 'oldgame'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(m);
    // a second merge of the same game is refused; unretire is refused
    let body = json!({"into": "newgame", "confirm": "oldgame"});
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/admin/games/oldgame/merge",
            Some(&t),
            Some(body)
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        api(
            &app,
            "POST",
            "/api/v1/admin/games/oldgame/unretire",
            Some(&t),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );
    // detection rules: the old program is detected as the target game
    let o = api(&app, "GET", "/api/v1/games/overrides", Some(&t), None).await;
    assert!(o.text.contains("oldgame.exe"));
    let v: serde_json::Value = serde_json::from_str(&o.text).unwrap();
    let rule = v["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["exes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e == "oldgame.exe")
        })
        .unwrap();
    assert_eq!(rule["id"], "newgame");
    // the audit log has it
    let a: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_audit WHERE action = 'game.merge'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(a, 1);
}
