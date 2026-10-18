//! Unusual hours: flagged in Admin and alerted (nothing removed); test accounts left out.
mod common;
use axum::http::StatusCode;
use common::*;
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn unusual_hours_are_flagged_not_removed(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let boss = signup(&app, "o@example.com", "boss").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'boss'")
        .execute(&pool)
        .await
        .unwrap();
    for h in ["longone", "grinder", "normal", "testbot"] {
        signup(&app, &format!("{h}@example.com"), h).await;
    }
    sqlx::query("UPDATE users SET is_test = true WHERE handle = 'testbot'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true) ON CONFLICT DO NOTHING")
        .execute(&pool).await.unwrap();
    let add = |who: &'static str, hours_ago: i32, secs: i32| {
        let pool = pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
                 SELECT gen_random_uuid(), id, 'wow', now() - make_interval(hours => $2) - make_interval(secs => $3),
                        now() - make_interval(hours => $2), $3 FROM users WHERE handle = $1",
            )
            .bind(who)
            .bind(hours_ago)
            .bind(secs)
            .execute(&pool)
            .await
            .unwrap();
        }
    };
    add("longone", 1, 17 * 3600).await; // one 17-hour session
    add("testbot", 1, 20 * 3600).await; // test accounts never flag
    add("normal", 1, 5 * 3600).await;
    // 21 hours (3 x 7 h, within each Sydney day) on 3 days in a row
    sqlx::query(
        "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
         SELECT gen_random_uuid(), u.id, 'wow', t, t + interval '7 hours', 7 * 3600
         FROM users u,
              LATERAL (SELECT ((((now() AT TIME ZONE 'Australia/Sydney')::date - d) + h * interval '1 hour')
                               AT TIME ZONE 'Australia/Sydney') AS t
                       FROM generate_series(2, 4) d, unnest(ARRAY[0, 8, 16]) h) x
         WHERE u.handle = 'grinder'",
    )
    .execute(&pool)
    .await
    .unwrap();
    let found = fraglink_server::alerts::find_unusual_hours(&st)
        .await
        .unwrap();
    let handles: Vec<&str> = found.iter().map(|r| r.1.as_str()).collect();
    assert!(handles.contains(&"longone"), "{found:?}");
    let g = found
        .iter()
        .find(|r| r.1 == "grinder")
        .expect("grinder flagged");
    assert!(g.2.contains("3 days in a row"), "{}", g.2);
    assert!(
        !handles.contains(&"testbot") && !handles.contains(&"normal"),
        "{found:?}"
    );

    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    fraglink_server::alerts::check_hours(&st).await.unwrap();
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(before, after, "nothing removed");
    let kind: String =
        sqlx::query_scalar("SELECT kind FROM alert_log ORDER BY sent_at DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(kind, "hours");
    // flagged ones aren't found again
    assert!(fraglink_server::alerts::find_unusual_hours(&st)
        .await
        .unwrap()
        .is_empty());

    let u = api(
        &app,
        "GET",
        "/api/v1/admin/users?q=longone",
        Some(&boss),
        None,
    )
    .await;
    let row = &u.json()["users"][0];
    assert!(row["hours_flag_at"].is_string(), "{}", u.text);
    let id = row["id"].as_str().unwrap().to_string();
    let d = api(
        &app,
        "GET",
        &format!("/api/v1/admin/users/{id}"),
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(
        d["hours_flag_reason"]
            .as_str()
            .unwrap()
            .contains("one session of 17"),
        "{d}"
    );
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/users/{id}/hours_flag/clear"),
        Some(&boss),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit WHERE action = 'user.hours_flag_clear'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}
