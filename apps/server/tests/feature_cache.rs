mod common;
use common::*;
use fraglink_server::features;
use sqlx::PgPool;

// SQLx runs tests with independent databases inside one process. A flag cached by
// one app must not become another app's settings. Use two states and a database
// change to exercise this deterministically, without timing or sleeps.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn feature_snapshots_belong_to_the_app_state(pool: PgPool) {
    features::forget();
    sqlx::query("INSERT INTO site_settings(key, enabled) VALUES('wow_characters', true)")
        .execute(&pool)
        .await
        .unwrap();
    let first = state(pool.clone());
    let first_clone = first.clone();
    assert!(features::on(&first, "wow_characters").await);
    sqlx::query("UPDATE site_settings SET enabled = false WHERE key = 'wow_characters'")
        .execute(&pool)
        .await
        .unwrap();
    let second = state(pool);
    assert!(
        !features::on(&second, "wow_characters").await,
        "a new app must read its own database, not reuse another app's flag snapshot"
    );
    assert!(
        features::on(&first_clone, "wow_characters").await,
        "clones of one app share its cached snapshot until invalidation"
    );
    features::forget();
    assert!(!features::on(&first, "wow_characters").await);
    assert!(!features::on(&second, "wow_characters").await);
}
