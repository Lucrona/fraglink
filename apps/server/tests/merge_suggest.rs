mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use sqlx::PgPool;

/// A tracked `exe:` program the games list knows is suggested (never auto-merged);
/// dismiss hides it, and the normal merge moves its hours.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn suggested_merge_list_dismiss_merge(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let owner = signup(&app, "o@example.com", "olive").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'olive'")
        .execute(&pool)
        .await
        .unwrap();
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'olive'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let db = fraglink_core::signatures::SignatureDb::builtin();
    let mut known = db
        .games()
        .iter()
        .filter(|g| !g.uncertain)
        .flat_map(|g| g.exes.iter().map(move |e| (g.id.clone(), e.to_lowercase())))
        .filter(|(id, e)| db.match_process(e, None).map(|m| &m.id) == Some(id));
    let (a_into, a_exe) = known.next().unwrap();
    let (b_into, b_exe) = known.find(|(i, _)| *i != a_into).unwrap();
    let (a, b) = (format!("exe:{a_exe}"), format!("exe:{b_exe}"));
    for id in [&a, &b, &"exe:zz_nobody_42.exe".to_string()] {
        fraglink_server::names::ensure_game_row(&st, id)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sessions(id, user_id, game_id, started_at, ended_at, duration_s)
                     VALUES($1, $2, $3, now() - interval '2 hours', now() - interval '1 hour', 3600)")
            .bind(uuid::Uuid::new_v4()).bind(uid).bind(id)
            .execute(&pool).await.unwrap();
    }
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/merge_suggestions",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let m = r.json()["merges"].as_array().unwrap().clone();
    assert_eq!(m.len(), 2, "{m:?}");
    let ra = m.iter().find(|x| x["game_id"] == a.as_str()).unwrap();
    assert_eq!(ra["into"], a_into.as_str());
    assert_eq!(ra["players"], 1);
    assert_eq!(ra["seconds"], 3600);
    // nothing moved on its own
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = $1")
        .bind(&a)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);

    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/merge_suggestions/{a}/dismiss"),
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/games/{b}/merge"),
        Some(&owner),
        Some(json!({ "into": b_into, "confirm": b })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE game_id = $1")
        .bind(&b_into)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
    let r = api(
        &app,
        "GET",
        "/api/v1/admin/merge_suggestions",
        Some(&owner),
        None,
    )
    .await;
    assert_eq!(
        r.json()["merges"].as_array().unwrap().len(),
        0,
        "{}",
        r.text
    );
}
