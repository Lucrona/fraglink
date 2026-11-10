//! Seeded Forever ruleset reference data. The migration must not rewrite
//! existing character or hour rows.
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn us_and_eu_rulesets_are_seeded(db: PgPool) {
    let rows: Vec<(String, i32, String, String, String)> = sqlx::query_as(
        "SELECT region, blizzard_id, slug, name, ruleset_type
         FROM wow_forever_rulesets ORDER BY region, blizzard_id",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    let got: Vec<(String, i32, String, String, String)> = rows
        .into_iter()
        .map(|r| (r.0, r.1, r.2, r.3, r.4))
        .collect();
    assert_eq!(
        got,
        vec![
            ("eu".into(), 1, "pvp".into(), "PvP".into(), "pvp".into()),
            (
                "eu".into(),
                2,
                "normal".into(),
                "Normal".into(),
                "normal".into()
            ),
            (
                "eu".into(),
                3,
                "roleplay".into(),
                "Roleplay".into(),
                "roleplay".into()
            ),
            (
                "eu".into(),
                15,
                "roleplay-pvp".into(),
                "Roleplay PvP".into(),
                "roleplay_pvp".into()
            ),
            ("us".into(), 1, "pvp".into(), "PvP".into(), "pvp".into()),
            (
                "us".into(),
                2,
                "normal".into(),
                "Normal".into(),
                "normal".into()
            ),
            (
                "us".into(),
                3,
                "roleplay".into(),
                "Roleplay".into(),
                "roleplay".into()
            ),
            (
                "us".into(),
                15,
                "roleplay-pvp".into(),
                "Roleplay PvP".into(),
                "roleplay_pvp".into()
            ),
        ]
    );
    let wow_col: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM information_schema.columns
         WHERE table_name='wow_characters' AND column_name='ruleset_id')",
    )
    .fetch_one(&db)
    .await
    .unwrap();
    assert!(wow_col);
}
