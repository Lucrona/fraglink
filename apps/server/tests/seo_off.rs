// Its own test binary: the feature switches are cached process-wide.
mod common;
use common::*;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn public_profiles_off_means_nothing_indexed(pool: PgPool) {
    let dir = std::env::temp_dir().join(format!("fraglink-seo-off-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("u.html"),
        "<html><head><!--seo--><title>x</title><!--/seo--></head></html>",
    )
    .unwrap();
    let app = fraglink_server::router(state_with(pool.clone(), |c| c.site_dir = dir));
    signup(&app, "pub@example.com", "pubperson").await;
    sqlx::query("UPDATE users SET public_profile = true WHERE handle = 'pubperson'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES ('public_profiles', false) ON CONFLICT (key) DO UPDATE SET enabled = false")
        .execute(&pool).await.unwrap();
    fraglink_server::features::forget();
    let r = page(&app, "/u/pubperson", None).await;
    assert_eq!(r.headers["x-robots-tag"], "noindex");
    assert!(!r.text.contains("canonical"));
    assert!(!page(&app, "/sitemap.xml", None)
        .await
        .text
        .contains("profiles-"));
    fraglink_server::features::forget();
}
