mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

fn games() -> Value {
    json!([
        { "id": "cs2", "name": "Counter-Strike 2", "steam_appid": 730 },
        { "id": "manual:my-mod", "name": "My Mod", "steam_appid": null }
    ])
}

fn ids(v: &Value, k: &str) -> Vec<String> {
    v[k].as_array()
        .unwrap()
        .iter()
        .map(|x| {
            if x.is_string() {
                x.as_str().unwrap().to_string()
            } else {
                x["id"].as_str().unwrap().to_string()
            }
        })
        .collect()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn upload_validation_and_idempotency(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let web = signup(&app, "s@example.com", "syncer").await;
    let dev = pair(&app, &web).await;
    let t = now() - 3 * 86400;
    let (ok1, ok2) = (uuid(), uuid());
    let (long, future, notuuid, unknown, backwards, toomuch) =
        (uuid(), uuid(), "abc".to_string(), uuid(), uuid(), uuid());
    let mut bad_active = session(&toomuch, "cs2", t + 50000, 100);
    bad_active["duration_s"] = json!(5000);
    let b = batch(
        games(),
        vec![
            session(&ok1, "cs2", t, 3600),
            session(&ok2, "manual:my-mod", t + 7200, 1800),
            session(&long, "cs2", t + 10000, 25 * 3600),
            session(&future, "cs2", now() + 3600, 600),
            session(&notuuid, "cs2", t + 20000, 60),
            session(&unknown, "not-listed", t + 30000, 60),
            json!({ "id": backwards, "game_id": "cs2", "started_at": t + 40000, "ended_at": t + 39000, "duration_s": 0, "idle_trimmed_s": 0 }),
            bad_active,
        ],
    );
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&dev),
        Some(b.clone()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let v = r.json();
    assert_eq!(ids(&v, "accepted"), vec![ok1.clone(), ok2.clone()]);
    let rej = ids(&v, "rejected");
    assert_eq!(rej.len(), 6, "{v}");
    let reason = |id: &str| {
        v["rejected"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id)
            .unwrap()["reason"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert!(reason(&long).contains("24 hours"));
    assert!(reason(&future).contains("future"));
    assert!(reason(&notuuid).contains("UUID"));
    assert!(reason(&unknown).contains("unknown game"));
    assert!(reason(&backwards).contains("before it starts"));
    assert!(reason(&toomuch).contains("active time"));

    // re-upload is idempotent (same ids accepted, no duplicates)
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&dev),
        Some(batch(games(), vec![session(&ok1, "cs2", t, 3600)])),
    )
    .await
    .json();
    assert_eq!(ids(&r, "accepted"), vec![ok1.clone()]);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);

    // manual game ids are namespaced per user
    let gid: String = sqlx::query_scalar("SELECT game_id FROM sessions WHERE id = $1::uuid")
        .bind(&ok2)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        gid.starts_with("manual:") && gid.ends_with(":my-mod") && gid.len() > 20,
        "{gid}"
    );

    // overlapping session from another device is rejected; touching within 60 s is fine
    let other = pair(&app, &web).await;
    let (ov, touch) = (uuid(), uuid());
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&other),
        Some(batch(
            games(),
            vec![
                session(&ov, "cs2", t + 600, 600),
                session(&touch, "cs2", t + 3600 - 30, 600),
            ],
        )),
    )
    .await
    .json();
    assert_eq!(ids(&r, "accepted"), vec![touch]);
    assert_eq!(ids(&r, "rejected"), vec![ov]);

    // someone else can't overwrite my session id
    let web2 = signup(&app, "s2@example.com", "intruder").await;
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&web2),
        Some(batch(games(), vec![session(&ok1, "cs2", t - 90000, 60)])),
    )
    .await
    .json();
    assert_eq!(ids(&r, "rejected"), vec![ok1.clone()]);

    // no auth, no upload
    assert_eq!(
        api(&app, "POST", "/api/v1/sessions/upload", None, Some(b))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn too_many_sessions_in_one_batch(pool: PgPool) {
    let app = fraglink_server::router(state(pool));
    let t = signup(&app, "m@example.com", "many").await;
    let sessions: Vec<Value> = (0..501)
        .map(|i| session(&uuid(), "cs2", now() - 10_000_000 + i * 100, 10))
        .collect();
    let r = api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&t),
        Some(batch(games(), sessions)),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn tiers_and_rankings(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let c = signup(&app, "c@example.com", "carol").await;
    let t = now() - 2 * 86400;
    // alice: 2h tracked CS2
    api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&a),
        Some(batch(games(), vec![session(&uuid(), "cs2", t, 7200)])),
    )
    .await;
    // bob: 1h tracked, plus a huge self-reported counter and a huge estimate
    api(
        &app,
        "POST",
        "/api/v1/sessions/upload",
        Some(&b),
        Some(batch(games(), vec![session(&uuid(), "cs2", t, 3600)])),
    )
    .await;
    let r = api(
        &app,
        "POST",
        "/api/v1/hours/import",
        Some(&b),
        Some(json!({ "game_id": "cs2", "minutes": 600000, "source": "manual" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let r = api(&app, "POST", "/api/v1/hours/estimate", Some(&b), Some(json!({ "game_id": "cs2", "eras": [{ "from_year": 2012, "to_year": 2020, "hours_per_week": 40 }] }))).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    // carol: estimate only
    api(&app, "POST", "/api/v1/hours/estimate", Some(&c), Some(json!({ "game_id": "cs2", "eras": [{ "from_year": 2012, "to_year": 2020, "hours_per_week": 40 }] }))).await;
    // Steam counter for alice larger than her sessions: tracked = max(sessions, steam)
    let alice: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'alice'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO steam_playtime(user_id, steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at) VALUES($1, 730, 'cs2', 300, 0, now())")
        .bind(alice).execute(&pool).await.unwrap();

    for period in ["7d", "all"] {
        let r = api(
            &app,
            "GET",
            &format!("/api/v1/leaderboard/cs2?period={period}"),
            Some(&c),
            None,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        let ranks = r.json()["ranks"].as_array().unwrap().clone();
        let handles: Vec<&str> = ranks
            .iter()
            .map(|r| r["handle"].as_str().unwrap())
            .collect();
        assert_eq!(
            handles,
            vec!["alice", "bob"],
            "{period}: Imported/Estimated never rank; carol has no Tracked time"
        );
        if period == "all" {
            assert_eq!(ranks[0]["tracked_min"], 300);
            assert_eq!(ranks[1]["tracked_min"], 60);
        } else {
            assert_eq!(ranks[0]["tracked_min"], 120);
        }
    }
    assert_eq!(
        api(&app, "GET", "/api/v1/leaderboard/cs2", None, None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        api(
            &app,
            "GET",
            "/api/v1/leaderboard/cs2?period=year",
            Some(&a),
            None
        )
        .await
        .status,
        StatusCode::BAD_REQUEST
    );

    // bob's own stats show all three tiers separately
    let s = api(&app, "GET", "/api/v1/stats", Some(&b), None)
        .await
        .json();
    let g = &s["games"][0];
    assert_eq!(g["game_id"], "cs2");
    assert_eq!(g["tracked_min"], 60);
    assert_eq!(g["imported_min"], 600000);
    assert_eq!(g["display_min"], 600000);
    assert_eq!(g["display_tier"], "imported");
    assert_eq!(g["estimated_min"], 9 * 52 * 40 * 60);
    assert_eq!(
        s["overall"]["verified_min"], 600000,
        "estimated never adds to the verified total"
    );
    assert_eq!(s["overall"]["estimated_display"], "~18,700h");
    let s = api(&app, "GET", "/api/v1/stats", Some(&a), None)
        .await
        .json();
    assert_eq!(s["games"][0]["display_tier"], "tracked");
    assert_eq!(s["games"][0]["display_min"], 300);
    assert_eq!(s["games"][0]["minutes_7d"], 120);
}
