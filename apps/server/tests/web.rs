mod common;
use axum::http::StatusCode;
use common::*;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn web_login_dashboard_and_forms(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let r = page(&app, "/", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("Create an account"));

    signup(&app, "web@example.com", "webuser").await;
    // wrong password -> back to the form with an error, no cookie
    let r = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=web%40example.com&password=nope",
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert!(r.location().starts_with("/login?"));
    assert!(r.cookie().is_none());
    // open redirects are ignored
    let r = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=web%40example.com&password=correct+horse+battery&next=%2F%2Fevil.example",
    )
    .await;
    assert_eq!(r.location(), "/");
    let cookie = r.cookie().unwrap();
    // cross-site login post refused
    let r = form(
        &app,
        "/login",
        None,
        Some("https://evil.example"),
        "email=web%40example.com&password=correct+horse+battery",
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // the old estimate form is gone (410), only game counters can be added
    let r = form(
        &app,
        "/web/estimate",
        Some(&cookie),
        Some(ORIGIN),
        "game_name=Counter-Strike+2&from_year=2005&to_year=2006&hours_per_week=10",
    )
    .await;
    assert_eq!(r.status, StatusCode::GONE);
    // a name that isn't on the server's list is refused (nobody creates or names games)
    let r = form(
        &app,
        "/web/import",
        Some(&cookie),
        Some(ORIGIN),
        "game_name=%3Cscript%3Ealert(1)%3C%2Fscript%3E&hours=10&source=manual",
    )
    .await;
    assert!(r.location().starts_with("/?err="), "{}", r.location());
    let r = form(
        &app,
        "/web/import",
        Some(&cookie),
        Some(ORIGIN),
        "game_name=World+of+Warcraft&hours=250.5&source=wow_manual",
    )
    .await;
    assert!(r.location().starts_with("/?msg="), "{}", r.location());
    let r = form(
        &app,
        "/web/import",
        Some(&cookie),
        Some(ORIGIN),
        "game_name=World+of+Warcraft&hours=-3&source=manual",
    )
    .await;
    assert!(r.location().starts_with("/?err="), "{}", r.location());
    // CSRF: cookie-authenticated form from another origin
    let r = form(
        &app,
        "/web/import",
        Some(&cookie),
        Some("https://evil.example"),
        "game_name=X&hours=1&source=manual",
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // and the JSON API refuses cookie auth from another origin too
    let r = send(
        &app,
        axum::http::Request::post("/api/v1/hours/import")
            .header("cookie", format!("fl_session={cookie}"))
            .header("origin", "https://evil.example")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(r#"{"game_name":"X","minutes":60}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let home = page(&app, "/?msg=%3Cb%3Ehi%3C%2Fb%3E", Some(&cookie)).await;
    assert_eq!(home.status, StatusCode::OK);
    assert!(!home.text.contains("<script>"));
    let e = page(
        &app,
        "/?err=%3Cscript%3Ealert(1)%3C%2Fscript%3E",
        Some(&cookie),
    )
    .await;
    assert!(e.text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!e.text.contains("<script>"));
    assert!(home.text.contains("&lt;b&gt;hi&lt;/b&gt;"));
    // "World of Warcraft" matched the known game, shown as Imported, 250.5 h
    assert!(home.text.contains("◆ 250.5h"), "{}", home.text);
    assert!(!home.text.contains("Estimate"), "{}", home.text);
    let gid: String =
        sqlx::query_scalar("SELECT game_id FROM hours_entries WHERE tier = 'imported'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(gid, "wow");

    // delete an entry and revoke a device through the page forms
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM hours_entries WHERE tier = 'imported'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let r = form(
        &app,
        &format!("/web/hours/{id}/delete"),
        Some(&cookie),
        Some(ORIGIN),
        "",
    )
    .await;
    assert!(r.location().starts_with("/?msg="));
    let web_token = signup(&app, "dev@example.com", "devowner").await;
    let dev = pair(&app, &web_token).await;
    let r = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=dev%40example.com&password=correct+horse+battery",
    )
    .await;
    let c2 = r.cookie().unwrap();
    let home = page(&app, "/", Some(&c2)).await;
    assert!(home.text.contains("Test PC"));
    let did: uuid::Uuid = sqlx::query_scalar("SELECT id FROM auth_tokens WHERE kind = 'device'")
        .fetch_one(&pool)
        .await
        .unwrap();
    // another user can't revoke it
    form(
        &app,
        &format!("/web/devices/{did}/revoke"),
        Some(&cookie),
        Some(ORIGIN),
        "",
    )
    .await;
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&dev), None)
            .await
            .status,
        StatusCode::OK
    );
    form(
        &app,
        &format!("/web/devices/{did}/revoke"),
        Some(&c2),
        Some(ORIGIN),
        "",
    )
    .await;
    assert_eq!(
        api(&app, "GET", "/api/v1/me", Some(&dev), None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );

    // logout clears the cookie and revokes the web session
    let r = form(&app, "/logout", Some(&cookie), Some(ORIGIN), "").await;
    assert!(r
        .headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("Max-Age=0"));
    let home = page(&app, "/", Some(&cookie)).await;
    assert!(home.text.contains("Create an account"));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn secure_cookie_on_https(pool: PgPool) {
    let app = fraglink_server::router(state_with(pool, |c| {
        c.public_url = "https://fraglink.example".into()
    }));
    let r = form(
        &app,
        "/signup",
        None,
        Some("https://fraglink.example"),
        "email=s%40example.com&handle=secure&password=correct+horse+battery&dob_year=1990&dob_month=01&dob_day=15",
    )
    .await;
    let c = r.headers.get("set-cookie").unwrap().to_str().unwrap();
    assert!(c.contains("; Secure"), "{c}");
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn web_friends_forms(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    signup(&app, "f1@example.com", "friendone").await;
    signup(&app, "f2@example.com", "friendtwo").await;
    let c1 = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=f1%40example.com&password=correct+horse+battery",
    )
    .await
    .cookie()
    .unwrap();
    let c2 = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=f2%40example.com&password=correct+horse+battery",
    )
    .await
    .cookie()
    .unwrap();
    let r = form(
        &app,
        "/web/friends/add",
        Some(&c1),
        Some(ORIGIN),
        "handle=friendtwo",
    )
    .await;
    assert!(r.location().starts_with("/?msg="), "{}", r.location());
    let home = page(&app, "/", Some(&c2)).await;
    assert!(
        home.text.contains("<b>friendone</b> wants to be friends"),
        "{}",
        home.text
    );
    assert_eq!(
        form(
            &app,
            "/web/friends/friendone/accept",
            Some(&c2),
            Some("https://evil.example"),
            ""
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    form(
        &app,
        "/web/friends/friendone/accept",
        Some(&c2),
        Some(ORIGIN),
        "",
    )
    .await;
    let home = page(&app, "/", Some(&c1)).await;
    assert!(home.text.contains("<td>friendtwo</td>") && home.text.contains("Offline"));
    form(
        &app,
        "/web/friends/friendtwo/block",
        Some(&c1),
        Some(ORIGIN),
        "",
    )
    .await;
    let home = page(&app, "/", Some(&c1)).await;
    assert!(home.text.contains("Blocked: friendtwo"));
    let r = form(
        &app,
        "/web/friends/friendtwo/bogus",
        Some(&c1),
        Some(ORIGIN),
        "",
    )
    .await;
    assert!(r.location().starts_with("/?err="));
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn web_delete_account(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let tok = signup(&app, "gone@example.com", "goner").await;
    let c = form(
        &app,
        "/login",
        None,
        Some(ORIGIN),
        "email=gone%40example.com&password=correct+horse+battery",
    )
    .await
    .cookie()
    .unwrap();
    assert!(page(&app, "/", Some(&c))
        .await
        .text
        .contains("Delete my account"));
    // wrong password / unticked box / foreign origin: nothing happens
    let r = form(
        &app,
        "/web/account/delete",
        Some(&c),
        Some(ORIGIN),
        "password=nope&confirm=yes",
    )
    .await;
    assert!(r.location().starts_with("/?err="), "{}", r.location());
    let r = form(
        &app,
        "/web/account/delete",
        Some(&c),
        Some(ORIGIN),
        "password=correct+horse+battery",
    )
    .await;
    assert!(r.location().starts_with("/?err="), "{}", r.location());
    let r = form(
        &app,
        "/web/account/delete",
        Some(&c),
        Some("http://evil.example"),
        "password=correct+horse+battery&confirm=yes",
    )
    .await;
    assert_ne!(r.status, axum::http::StatusCode::SEE_OTHER);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    // the real thing
    let r = form(
        &app,
        "/web/account/delete",
        Some(&c),
        Some(ORIGIN),
        "password=correct+horse+battery&confirm=yes",
    )
    .await;
    assert!(r.location().contains("deleted"), "{}", r.location());
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
    let me = api(&app, "GET", "/api/v1/me", Some(&tok), None).await;
    assert_eq!(me.status, axum::http::StatusCode::UNAUTHORIZED);
}
