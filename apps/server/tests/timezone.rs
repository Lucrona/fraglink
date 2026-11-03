//! Settings > Account > Time zone: a saved zone wins over the browser's `?tz=`, which wins
//! over Australia/Sydney, on every 7-day figure; bad names are refused; a person's own week
//! is in their own zone wherever it is shown, while ranked lists use the viewer's window.
mod common;
use common::*;
use serde_json::{json, Value};
use sqlx::PgPool;

fn n(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("not a number: {v}"))
}

async fn save(app: &axum::Router, t: &str, tz: Value) -> Resp {
    api(
        app,
        "POST",
        "/api/v1/me/timezone",
        Some(t),
        Some(json!({ "timezone": tz })),
    )
    .await
}

async fn get(app: &axum::Router, path: &str, t: &str) -> Value {
    let r = api(app, "GET", path, Some(t), None).await;
    assert_eq!(r.status, 200, "{path}: {}", r.text);
    r.json()
}

async fn uid(pool: &PgPool, handle: &str) -> uuid::Uuid {
    sqlx::query_scalar("SELECT id FROM users WHERE handle = $1")
        .bind(handle)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn saved_zone_then_browser_then_sydney(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let a = signup(&app, "a@example.com", "alice").await;
    let id = uid(&pool, "alice").await;
    let tz = |v: Value| v["tz"].as_str().unwrap().to_string();

    // nothing saved: the browser's zone, else (missing or bogus) Sydney
    assert_eq!(
        get(&app, "/api/v1/me", &a).await["user"]["timezone"],
        Value::Null
    );
    assert_eq!(
        tz(get(&app, "/api/v1/home/week?tz=Europe/Berlin", &a).await),
        "Europe/Berlin"
    );
    assert_eq!(
        tz(get(&app, "/api/v1/home/week", &a).await),
        "Australia/Sydney"
    );
    assert_eq!(
        tz(get(&app, "/api/v1/home/week?tz=Mars/Base", &a).await),
        "Australia/Sydney"
    );
    use fraglink_server::week::zone;
    assert_eq!(zone(&st, None, None).await, "Australia/Sydney");
    assert_eq!(zone(&st, Some(id), Some("Asia/Tokyo")).await, "Asia/Tokyo");

    // saved: wins over any browser value
    let r = save(&app, &a, json!("America/Los_Angeles")).await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(r.json()["timezone"], "America/Los_Angeles");
    assert_eq!(
        get(&app, "/api/v1/me", &a).await["user"]["timezone"],
        "America/Los_Angeles"
    );
    for q in ["", "?tz=Europe/Berlin", "?tz=Mars/Base"] {
        let w = get(&app, &format!("/api/v1/home/week{q}"), &a).await;
        assert_eq!(tz(w), "America/Los_Angeles", "{q}");
    }
    assert_eq!(
        zone(&st, Some(id), Some("Asia/Tokyo")).await,
        "America/Los_Angeles"
    );
    assert_eq!(zone(&st, Some(id), None).await, "America/Los_Angeles");

    // automatic again (null or empty): back to the browser's
    for v in [Value::Null, json!(""), json!("  ")] {
        save(&app, &a, json!("Asia/Tokyo")).await;
        let r = save(&app, &a, v.clone()).await;
        assert_eq!(r.status, 200, "{v}: {}", r.text);
        assert_eq!(r.json()["timezone"], Value::Null);
        assert_eq!(
            tz(get(&app, "/api/v1/home/week?tz=Europe/Berlin", &a).await),
            "Europe/Berlin"
        );
    }
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn invalid_zone_is_refused(pool: PgPool) {
    let app = fraglink_server::router(state(pool.clone()));
    let a = signup(&app, "a@example.com", "alice").await;
    save(&app, &a, json!("Europe/London")).await;
    let long = "A/".repeat(40);
    for bad in [
        json!("Mars/Olympus_Mons"),
        json!("Sydney"),
        json!("+10:00"),
        json!("Europe/London; DROP TABLE users"),
        json!(long),
        json!(42),
    ] {
        let r = save(&app, &a, bad.clone()).await;
        assert!(
            r.status == 400 || r.status == 422,
            "{bad}: {} {}",
            r.status,
            r.text
        );
        if r.status == 400 {
            assert_eq!(r.json()["error"], "invalid_timezone", "{bad}: {}", r.text);
        }
    }
    // nothing changed, and the column itself refuses junk too
    assert_eq!(
        get(&app, "/api/v1/me", &a).await["user"]["timezone"],
        "Europe/London"
    );
    assert!(
        sqlx::query("UPDATE users SET timezone = '' WHERE handle = 'alice'")
            .execute(&pool)
            .await
            .is_err()
    );
    // signed-out and cross-site saves are refused
    let r = api(
        &app,
        "POST",
        "/api/v1/me/timezone",
        None,
        Some(json!({"timezone": "UTC"})),
    )
    .await;
    assert_eq!(r.status, 401, "{}", r.text);
}

/// Someone in Los Angeles who saved their zone: every 7-day figure of theirs, wherever it is
/// shown and whoever looks (a Sydney friend, the desktop app with no browser zone, the
/// signature card, the old web page, staff, exports), equals their home headline in LA.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn los_angeles_user_week_matches_headline_everywhere(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    let a = signup(&app, "a@example.com", "alice").await;
    let b = signup(&app, "b@example.com", "bob").await;
    let s = signup(&app, "s@example.com", "staffer").await;
    sqlx::query("UPDATE users SET role = 'owner' WHERE handle = 'staffer'")
        .execute(&pool)
        .await
        .unwrap();
    for (t, h) in [(&a, "bob"), (&b, "alice")] {
        api(
            &app,
            "POST",
            "/api/v1/friends/request",
            Some(t),
            Some(json!({ "handle": h })),
        )
        .await;
    }
    for t in [&a, &b] {
        api(
            &app,
            "POST",
            "/api/v1/me/privacy",
            Some(t),
            Some(json!({"public_profile": true})),
        )
        .await;
    }
    assert_eq!(
        save(&app, &a, json!("America/Los_Angeles")).await.status,
        200
    );
    assert_eq!(save(&app, &b, json!("Australia/Sydney")).await.status, 200);
    sqlx::query("INSERT INTO games (id, name, verified) VALUES ('wow', 'World of Warcraft', true), ('cs2', 'Counter-Strike 2', true) ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, verified = true")
        .execute(&pool)
        .await
        .unwrap();
    // a session every 3 hours for 8 days (distinct lengths), so the LA and Sydney windows,
    // whose starts are ~18 hours apart, can't give the same total
    for i in 0..64i64 {
        let g = if i % 3 == 0 { "cs2" } else { "wow" };
        sqlx::query(
            "INSERT INTO sessions (id, user_id, game_id, started_at, ended_at, duration_s)
             SELECT gen_random_uuid(), id, $1, now() - make_interval(hours => $2::int * 3 + 1), now() - make_interval(hours => $2::int * 3), $3 FROM users WHERE handle = 'alice'",
        )
        .bind(g)
        .bind(i as i32)
        .bind((600 + i * 37) as i32)
        .execute(&pool)
        .await
        .unwrap();
    }
    // imports never count in a week
    sqlx::query("INSERT INTO hours_entries (id, user_id, game_id, tier, source, minutes) SELECT gen_random_uuid(), id, 'wow', 'imported', 'manual', 600000 FROM users WHERE handle = 'alice'")
        .execute(&pool)
        .await
        .unwrap();
    let id = uid(&pool, "alice").await;
    let la = fraglink_server::week::for_user(&st, id, "America/Los_Angeles", true)
        .await
        .unwrap();
    let syd = fraglink_server::week::for_user(&st, id, "Australia/Sydney", true)
        .await
        .unwrap();
    let (la, syd) = (la.shown(|_| true), syd.shown(|_| true));
    assert_ne!(
        la.seconds, syd.seconds,
        "the test needs the two zones to differ"
    );

    // the headline, whatever the browser says
    let w = get(&app, "/api/v1/home/week?tz=Asia/Tokyo", &a).await;
    assert_eq!(w["tz"], "America/Los_Angeles");
    let (secs, minutes, tenths) = (n(&w["seconds"]), n(&w["minutes"]), n(&w["tenths"]));
    assert_eq!(secs, la.seconds);
    assert_eq!((minutes, tenths), (la.minutes, la.tenths));
    let per = |id: &str| {
        w["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["game_id"] == id)
            .cloned()
            .unwrap()
    };
    let sum = |v: &Value, k: &str| -> i64 { v.as_array().unwrap().iter().map(|g| n(&g[k])).sum() };

    // own pages, with a wrong or no browser zone
    for q in ["", "?tz=Asia/Tokyo"] {
        let h = get(&app, &format!("/api/v1/home{q}"), &a).await;
        assert_eq!(
            (n(&h["me"]["minutes_7d"]), n(&h["me"]["tenths_7d"])),
            (minutes, tenths),
            "home {q}"
        );
        let s7 = get(&app, &format!("/api/v1/stats{q}"), &a).await;
        assert_eq!(
            (
                n(&s7["overall"]["minutes_7d"]),
                n(&s7["overall"]["tenths_7d"])
            ),
            (minutes, tenths),
            "stats {q}"
        );
        let sep = if q.is_empty() { "?" } else { "&" };
        let mg = get(&app, &format!("/api/v1/me/games{q}{sep}limit=50"), &a).await;
        assert_eq!(sum(&mg["games"], "tenths_7d"), tenths, "my games {q}");
        let ex = get(&app, &format!("/api/v1/export{q}"), &a).await;
        assert_eq!(sum(&ex["game_stats"], "minutes_7d"), minutes, "export {q}");
        assert_eq!(
            ex["timezone"], "America/Los_Angeles",
            "export has the setting"
        );
        assert_eq!(ex["week_timezone"], "America/Los_Angeles");
        // own leaderboard window = own zone
        for g in ["wow", "cs2"] {
            let lb = get(
                &app,
                &format!("/api/v1/leaderboard/{g}?period=7d{}", q.replace('?', "&")),
                &a,
            )
            .await;
            let r = lb["ranks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["handle"] == "alice")
                .cloned()
                .unwrap();
            assert_eq!(
                n(&r["seconds"]),
                n(&per(g)["seconds"]),
                "leaderboard {g} {q}"
            );
        }
    }

    // others looking at alice's own week: her zone, not theirs (bob saved Sydney and his
    // browser says Tokyo; the desktop app sends nothing)
    for q in ["", "?tz=Asia/Tokyo"] {
        let p = get(&app, &format!("/api/v1/users/alice/profile{q}"), &b).await;
        assert_eq!(
            (
                n(&p["overall"]["minutes_7d"]),
                n(&p["overall"]["tenths_7d"])
            ),
            (minutes, tenths),
            "profile {q}"
        );
        let fs = get(&app, &format!("/api/v1/friends/alice/summary{q}"), &b).await;
        assert_eq!(n(&fs["week_seconds"]), secs, "friend card {q}");
        assert_eq!(n(&fs["last_7d_seconds"]), secs, "friend card {q}");
    }
    let adm = get(&app, &format!("/api/v1/admin/users/{id}"), &s).await;
    assert_eq!(
        sum(&adm["games"], "minutes_7d"),
        minutes,
        "admin view: {adm}"
    );
    let card = fraglink_server::sig::card_for(&st, "alice")
        .await
        .unwrap()
        .unwrap();
    assert!(
        card.line2
            .starts_with(&format!("{} h this week", (minutes + 30) / 60)),
        "{}",
        card.line2
    );
    let web = page(&app, "/", Some(&a)).await;
    assert_eq!(web.status, 200);
    for g in ["wow", "cs2"] {
        let m = n(&get(&app, "/api/v1/stats", &a).await["games"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["game_id"] == g)
            .unwrap()["minutes_7d"]);
        let cell = format!(r#"<td class="n">{}.{}h</td>"#, m / 60, (m % 60) * 10 / 60);
        assert!(web.text.contains(&cell), "web page: {cell}");
    }
    let r = api(&app, "GET", "/api/v1/export.zip", Some(&a), None).await;
    assert_eq!(r.status, 200);
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(r.bytes.clone())).unwrap();
    let mut prof = String::new();
    std::io::Read::read_to_string(&mut z.by_name("profile.json").unwrap(), &mut prof).unwrap();
    let prof: Value = serde_json::from_str(&prof).unwrap();
    assert_eq!(prof["timezone"], "America/Los_Angeles");

    // ranked lists use the viewer's window: bob (Sydney) sees alice's Sydney-window seconds
    let lb = get(
        &app,
        "/api/v1/leaderboard/wow?period=7d&tz=America/Los_Angeles",
        &b,
    )
    .await;
    let r = lb["ranks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["handle"] == "alice")
        .cloned()
        .unwrap();
    assert_eq!(
        n(&r["seconds"]),
        syd.games
            .iter()
            .find(|g| g.game_id == "wow")
            .map_or(0, |g| g.seconds),
        "viewer's window"
    );
    // lifetime keeps imports
    assert!(n(&get(&app, "/api/v1/home", &a).await["me"]["total_min"]) >= 600_000);
}
