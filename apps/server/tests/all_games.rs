mod common;
use axum::http::StatusCode;
use common::*;
use sqlx::PgPool;

const LIST_1: &str = r#"[
 {"id":"452295596917784577","name":"Star Citizen","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]},
 {"id":"1234821572246896730","name":"Gray Zone Warfare","executables":[{"name":"gzwclientsteam-win64-shipping.exe","os":"win32","is_launcher":false}]},
 {"id":"999","name":"No Icon Game","executables":[{"name":"noicon.exe","os":"win32","is_launcher":false}]}
]"#;
const LIST_2: &str = r#"[
 {"id":"452295596917784577","name":"Star Citizen Alpha","executables":[{"name":"starcitizen.exe","os":"win32","is_launcher":false}]},
 {"id":"1234821572246896730","name":"Gray Zone Warfare","executables":[{"name":"gzwclientsteam-win64-shipping.exe","os":"win32","is_launcher":false},{"name":"gzw.exe","os":"win32","is_launcher":false}]},
 {"id":"1000","name":"Brand New Game","executables":[{"name":"brandnew.exe","os":"win32","is_launcher":false}]}
]"#;

fn install(st: &fraglink_server::AppState, dir: &std::path::Path, raw: &str) {
    std::fs::write(dir.join("detectable/discord.json"), raw).unwrap();
    let built = fraglink_server::games::build_from_cache(dir, &st.sigs).unwrap();
    st.games.install(built);
}

async fn kinds(pool: &PgPool) -> Vec<(String, String)> {
    sqlx::query_as("SELECT game_id, kind FROM game_list_changes ORDER BY game_id, kind")
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn all_games_list_tracking_and_history(pool: PgPool) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("detectable")).unwrap();
    let data = dir.path().to_path_buf();
    let st = state_with(pool.clone(), move |c| c.data_dir = data);
    install(&st, dir.path(), LIST_1);
    let app = fraglink_server::router(st.clone());
    let boss = signup(&app, "allg@example.com", "allgboss").await;
    let plain = signup(&app, "allp@example.com", "allgplain").await;
    sqlx::query("UPDATE users SET role = 'admin' WHERE handle = 'allgboss'")
        .execute(&pool)
        .await
        .unwrap();

    // first run: a baseline, no change rows
    fraglink_server::all_games::track_list(&st).await;
    let seen: (i64, i64) =
        sqlx::query_as("SELECT count(*), count(*) FILTER (WHERE baseline) FROM game_list_seen")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(seen, (3, 3));
    assert!(kinds(&pool).await.is_empty());
    let job: (bool, String) = sqlx::query_as(
        "SELECT last_ok_at IS NOT NULL, detail FROM job_runs WHERE job = 'games_list'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(job.0, "{job:?}");

    // the next list: one renamed, one new, one with a new exe, one gone
    install(&st, dir.path(), LIST_2);
    fraglink_server::all_games::track_list(&st).await;
    let k: Vec<String> = kinds(&pool).await.into_iter().map(|x| x.1).collect();
    for want in ["new", "renamed", "exes", "removed"] {
        assert!(k.iter().any(|x| x == want), "{want} missing from {k:?}");
    }
    // running again with the same list records nothing more
    let before = kinds(&pool).await.len();
    fraglink_server::all_games::track_list(&st).await;
    assert_eq!(kinds(&pool).await.len(), before);

    // staff only
    let r = api(&app, "GET", "/api/v1/admin/games-all", Some(&plain), None).await;
    assert_ne!(r.status, StatusCode::OK);

    // whole universe, 50 per page, with counts and freshness
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?sort=name",
        Some(&boss),
        None,
    )
    .await
    .json();
    let everything = d["everything"].as_u64().unwrap();
    assert!(everything > 50, "{everything}");
    assert_eq!(d["rows"].as_array().unwrap().len(), 50);
    assert_eq!(d["page_size"], 50);
    assert!(
        d["counts"]["src"]["games list"].as_u64().unwrap() >= 2,
        "{}",
        d["counts"]
    );
    assert_eq!(d["meta"]["games_list"]["games"], 3);
    assert!(d["meta"]["games_list_job"]["last_run_at"].is_string());
    let last = (everything as usize - 1) / 50 * 50;
    let p = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?sort=name&offset=999999",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(p["offset"], last);

    // search + filters
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?q=brand%20new",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(d["total"], 1, "{d}");
    let new_id = d["rows"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(d["rows"][0]["added_kind"], "list");
    assert!(d["rows"][0]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s == "games list"));
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?flag=new",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert!(
        d["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == new_id.as_str()),
        "{d}"
    );
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?q=star%20citizen%20alpha",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(d["total"], 1, "{d}");
    let sc = d["rows"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(d["rows"][0]["added_kind"], "before_tracking");
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?src=games%20list&status=removed",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(d["total"], 0, "removed games are off the list: {d}");

    // history merges the list changes and the staff audit log
    let r = api(
        &app,
        "POST",
        &format!("/api/v1/admin/games/{sc}/status"),
        Some(&boss),
        Some(serde_json::json!({ "lifecycle": "beta", "checked": true })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let h = api(
        &app,
        "GET",
        &format!("/api/v1/admin/games/{sc}/history"),
        Some(&boss),
        None,
    )
    .await
    .json();
    let acts: Vec<String> = h["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap_or("").to_string())
        .collect();
    assert!(acts.iter().any(|a| a == "renamed"), "{acts:?}");
    assert!(
        acts.iter().any(|a| a == "on_list_when_tracking_began"),
        "{acts:?}"
    );
    assert!(acts.iter().any(|a| a.starts_with("game.")), "{acts:?}");
    let d = api(
        &app,
        "GET",
        "/api/v1/admin/games-all?q=star%20citizen%20alpha",
        Some(&boss),
        None,
    )
    .await
    .json();
    assert_eq!(d["rows"][0]["by"], "allgboss");
    assert_eq!(d["rows"][0]["lifecycle"], "beta");
}
