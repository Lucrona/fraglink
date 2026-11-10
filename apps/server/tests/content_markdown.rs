mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;
async fn admin(app: &axum::Router, pool: &PgPool) -> String {
    let token = signup(app, "md@example.com", "mdadmin").await;
    sqlx::query("UPDATE users SET role='admin' WHERE handle='mdadmin'")
        .execute(pool)
        .await
        .unwrap();
    token
}
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn preview_is_admin_only_read_only_and_news_matches_public(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let token = admin(&app, &pool).await;
    let ordinary = signup(&app, "reader@example.com", "mdreader").await;
    let body = json!({"kind":"news","format":"markdown","body":"  **Bold**\nnext\n\n- first\n\n- [safe](https://example.com)\n\n[bad](javascript:alert%281%29)\n<script>alert(1)</script>  "});
    for auth in [None, Some(ordinary.as_str())] {
        assert_ne!(
            api(
                &app,
                "POST",
                "/api/v1/admin/markdown/preview",
                auth,
                Some(body.clone())
            )
            .await
            .status,
            StatusCode::OK
        );
    }
    let before: (i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM news_posts),(SELECT count(*) FROM changelog_entries),(SELECT count(*) FROM admin_audit)").fetch_one(&pool).await.unwrap();
    let p = api(
        &app,
        "POST",
        "/api/v1/admin/markdown/preview",
        Some(&token),
        Some(body.clone()),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{}", p.text);
    assert_eq!(p.headers["cache-control"], "private, no-store");
    let after: (i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM news_posts),(SELECT count(*) FROM changelog_entries),(SELECT count(*) FROM admin_audit)").fetch_one(&pool).await.unwrap();
    assert_eq!(before, after);
    let r=api(&app,"POST","/api/v1/admin/news",Some(&token),Some(json!({"title":"Fixture Markdown","body":body["body"],"format":"markdown","body_html":"<script>forged</script>"}))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let post = r.json()["post"].clone();
    assert!(!post["summary"].as_str().unwrap().contains("**"));
    let html = post["body_html"].as_str().unwrap();
    assert_eq!(p.json()["html"], post["body_html"]);
    assert!(html.contains("<strong>Bold</strong>"));
    assert!(html.contains("<br>"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("href=\"javascript:"));
    let public = api(
        &app,
        "GET",
        &format!("/api/v1/news/{}", post["slug"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    assert_eq!(public.status, StatusCode::OK);
    assert_eq!(public.json()["post"]["body_html"], p.json()["html"]);
    for bad in [
        json!({"kind":"news","format":"html","body":"x"}),
        json!({"kind":"news","format":"markdown","body":"x".repeat(20001)}),
        json!({"kind":"unknown"}),
    ] {
        assert_eq!(
            api(
                &app,
                "POST",
                "/api/v1/admin/markdown/preview",
                Some(&token),
                Some(bad)
            )
            .await
            .status,
            StatusCode::BAD_REQUEST
        );
    }
}
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn changelog_markdown_preserves_source_and_matches_preview(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let token = admin(&app, &pool).await;
    let source = format!(
        "  - **First**\n\n- *Second* with `code`\n\nA paragraph\nwith a break\n\n{}  ",
        "More details. ".repeat(30)
    );
    let entry = json!({"kind":"changelog","area":"Website","date":"2026-11-11","format":"markdown","new":[source],"changes":["\n\n"],"fixes":[]});
    let preview = api(
        &app,
        "POST",
        "/api/v1/admin/markdown/preview",
        Some(&token),
        Some(entry.clone()),
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.text);
    let saved = api(
        &app,
        "POST",
        "/api/v1/admin/changelog",
        Some(&token),
        Some(entry.clone()),
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.text);
    let e = saved.json()["entry"].clone();
    assert_eq!(e["new_html"], preview.json()["new_html"]);
    assert_eq!(e["changes_html"], "");
    assert_eq!(
        e["new"][0].as_str().unwrap(),
        entry["new"][0].as_str().unwrap().trim()
    );
    assert!(e["new"][0].as_str().unwrap().len() > 240);
    assert_eq!(e["new_html"].as_str().unwrap().matches("<li>").count(), 2);
    assert!(!e["new_html"].as_str().unwrap().contains("<li></li>"));
    let id = e["id"].as_i64().unwrap();
    assert_eq!(
        api(&app, "GET", "/api/v1/changelog", None, None)
            .await
            .json()["entries"],
        json!([])
    );
    let published = api(
        &app,
        "POST",
        &format!("/api/v1/admin/changelog/{id}/publish"),
        Some(&token),
        Some(json!({"publish":true})),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK, "{}", published.text);
    let public = api(&app, "GET", "/api/v1/changelog", None, None).await;
    assert_eq!(
        public.json()["entries"][0]["new_html"],
        preview.json()["new_html"]
    );
}
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn legacy_posts_and_drafts_are_not_rewritten(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let token = admin(&app, &pool).await;
    let post=api(&app,"POST","/api/v1/admin/news",Some(&token),Some(json!({"title":"Legacy fixture","body":"**literal**\n- one\n- two\n\nhttps://example.com"}))).await.json()["post"].clone();
    assert!(post["format"].is_null());
    assert!(post["body_html"]
        .as_str()
        .unwrap()
        .contains("**literal**<br>- one<br>- two"));
    let entry = api(
        &app,
        "POST",
        "/api/v1/admin/changelog",
        Some(&token),
        Some(json!({"area":"Website","date":"2026-11-10","new":["- old bullet","","**literal**"]})),
    )
    .await
    .json()["entry"]
        .clone();
    assert!(entry["format"].is_null());
    assert_eq!(entry["new"], json!(["old bullet", "literal**"]));
    let snapshot: Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(n) FROM news_posts n UNION ALL SELECT to_jsonb(c) FROM changelog_entries c").fetch_all(&pool).await.unwrap();
    api(
        &app,
        "POST",
        "/api/v1/admin/markdown/preview",
        Some(&token),
        Some(json!({"kind":"news","format":"markdown","body":"**new**"})),
    )
    .await;
    api(&app, "GET", "/api/v1/changelog", None, None).await;
    api(
        &app,
        "GET",
        &format!("/api/v1/news/{}", post["slug"].as_str().unwrap()),
        None,
        None,
    )
    .await;
    let after: Vec<Value>=sqlx::query_scalar("SELECT to_jsonb(n) FROM news_posts n UNION ALL SELECT to_jsonb(c) FROM changelog_entries c").fetch_all(&pool).await.unwrap();
    assert_eq!(snapshot, after);
}
