mod common;
use axum::http::StatusCode;
use common::*;
use sqlx::PgPool;

const PAGE: &str = "<!doctype html><html><head>\n<!--seo--><title>Profile · Fraglink</title>\n<meta name=\"robots\" content=\"noindex\"><!--/seo-->\n</head><body>x</body></html>";

fn app_with_site(pool: PgPool, tag: &str) -> axum::Router {
    let dir = std::env::temp_dir().join(format!("fraglink-seo-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("u.html"), PAGE).unwrap();
    fraglink_server::router(state_with(pool, |c| c.site_dir = dir))
}

async fn set(pool: &PgPool, sql: &'static str) {
    sqlx::query(sql).execute(pool).await.unwrap();
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn only_public_adult_or_opted_in_profiles_are_indexed(pool: PgPool) {
    let app = app_with_site(pool.clone(), "idx");
    for h in [
        "pubadult",
        "privadult",
        "minorpub",
        "minorpriv",
        "testacct",
        "leaving",
        "review",
    ] {
        signup(&app, &format!("{h}@example.com"), h).await;
    }
    set(&pool, "UPDATE users SET public_profile = true WHERE handle IN ('pubadult','minorpub','testacct','leaving','review')").await;
    set(
        &pool,
        "UPDATE users SET public_profile = false WHERE handle IN ('privadult','minorpriv')",
    )
    .await;
    set(
        &pool,
        "UPDATE users SET is_minor = true WHERE handle IN ('minorpub','minorpriv')",
    )
    .await;
    set(
        &pool,
        "UPDATE users SET is_test = true WHERE handle = 'testacct'",
    )
    .await;
    set(
        &pool,
        "UPDATE users SET delete_after = now() + interval '7 days' WHERE handle = 'leaving'",
    )
    .await;
    set(
        &pool,
        "UPDATE users SET age_review_at = now() WHERE handle = 'review'",
    )
    .await;

    // public adult: indexable, canonical on the production host, preview tags
    let r = page(&app, "/u/PubAdult?tab=games", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers.get("x-robots-tag").is_none());
    assert!(!r.text.contains("noindex"), "{}", r.text);
    assert!(r
        .text
        .contains("<link rel=\"canonical\" href=\"https://playfraglink.com/u/pubadult\">"));
    assert!(r.text.contains("og:title") && r.text.contains("(@pubadult)"));
    assert!(r.text.ends_with("<body>x</body></html>"));
    // an under-18 who switched to public themselves (opted in): indexable too
    let r = page(&app, "/u/minorpub", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers.get("x-robots-tag").is_none());
    // private (adult or minor) and under age review: page works, but noindex and no name in the tags
    for h in ["privadult", "minorpriv", "review"] {
        let r = page(&app, &format!("/u/{h}"), None).await;
        assert_eq!(r.status, StatusCode::OK, "{h}");
        assert_eq!(r.headers["x-robots-tag"], "noindex", "{h}");
        assert!(
            r.text
                .contains("<meta name=\"robots\" content=\"noindex\">"),
            "{h}"
        );
        assert!(!r.text.contains(h) && !r.text.contains("canonical"), "{h}");
    }
    // test accounts and leaving accounts look exactly like a name nobody has
    let unknown = page(&app, "/u/nobodyhere", None).await;
    for h in ["testacct", "leaving", "nobodyhere"] {
        let r = page(&app, &format!("/u/{h}"), None).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{h}");
        assert_eq!(r.headers["x-robots-tag"], "noindex", "{h}");
        assert_eq!(r.text, unknown.text, "{h}");
    }

    // sitemap: index -> pages + one profiles file; only the two indexable profiles
    let r = page(&app, "/sitemap.xml", None).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/xml"));
    assert!(r
        .text
        .contains("<loc>https://playfraglink.com/sitemaps/pages.xml</loc>"));
    assert!(r
        .text
        .contains("<loc>https://playfraglink.com/sitemaps/profiles-1.xml</loc>"));
    assert!(!r.text.contains("profiles-2.xml"));
    let r = page(&app, "/sitemaps/pages.xml", None).await;
    for p in ["/", "/download", "/privacy", "/terms"] {
        assert!(
            r.text
                .contains(&format!("<loc>https://playfraglink.com{p}</loc>")),
            "{p}"
        );
    }
    let r = page(&app, "/sitemaps/profiles-1.xml", None).await;
    assert_eq!(r.text.matches("<url>").count(), 2, "{}", r.text);
    assert!(r.text.contains("/u/pubadult<") && r.text.contains("/u/minorpub<"));
    assert_eq!(
        page(&app, "/sitemaps/profiles-2.xml", None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        page(&app, "/sitemaps/profiles-0.xml", None).await.status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn sitemap_pages_profiles_in_files_of_5000(pool: PgPool) {
    let app = app_with_site(pool.clone(), "pg");
    signup(&app, "seed@example.com", "seeduser").await;
    // 5001 public profiles straight in the database (signing up 5000 accounts would be slow)
    sqlx::query(
        "INSERT INTO users(id, email, handle, display_name, password_hash, public_profile)
         SELECT gen_random_uuid(), 'bulk' || g || '@example.com', 'bulk' || g, 'Bulk ' || g, 'x', true
           FROM generate_series(1, 5000) g",
    )
    .execute(&pool)
    .await
    .unwrap();
    set(
        &pool,
        "UPDATE users SET public_profile = true WHERE handle = 'seeduser'",
    )
    .await;
    let r = page(&app, "/sitemap.xml", None).await;
    assert!(
        r.text.contains("profiles-2.xml") && !r.text.contains("profiles-3.xml"),
        "{}",
        r.text
    );
    let a = page(&app, "/sitemaps/profiles-1.xml", None).await;
    let b = page(&app, "/sitemaps/profiles-2.xml", None).await;
    assert_eq!(a.text.matches("<url>").count(), 5000);
    assert_eq!(b.text.matches("<url>").count(), 1);
}
