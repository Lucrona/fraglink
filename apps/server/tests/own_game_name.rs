//! beta.5: a program the user added shows their own name for it on their own views
//! only; friends keep seeing "a game" until it is approved.
mod common;
use common::*;
use fraglink_server::presence::{self, GameRef};
use sqlx::PgPool;

#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn own_name_only_for_self(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    signup(&app, "carol@example.com", "carol").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'carol'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let g = |name: Option<&str>| GameRef {
        id: "exe:oddgame.exe".into(),
        name: name.map(str::to_string),
        started_at: None,
        detail: None,
        hidden: false,
    };
    presence::set_from(&st, uid, "carol", "online", Some(Some(g(Some("Odd Game")))))
        .await
        .unwrap();
    let friends_view = presence::visible_presence(&st, uid).await.unwrap().unwrap();
    assert_eq!(friends_view.game_id.as_deref(), Some("unlisted"));
    let mut mine = friends_view.clone();
    presence::apply_own_name(&st, uid, &mut mine).await.unwrap();
    assert_eq!(mine.game_name.as_deref(), Some("Odd Game"));
    assert_eq!(mine.game_id.as_deref(), Some("exe:oddgame.exe"));
    // no name sent (older app) or game stopped: back to "a game"
    presence::set_from(&st, uid, "carol", "online", Some(None))
        .await
        .unwrap();
    let mut v = presence::visible_presence(&st, uid).await.unwrap().unwrap();
    presence::apply_own_name(&st, uid, &mut v).await.unwrap();
    assert_eq!(v.game_name, None);
}

/// beta.6: the name is in the database, so a server restart (empty memory) keeps it.
#[sqlx::test(migrator = "fraglink_server::MIGRATOR")]
async fn own_name_survives_restart(pool: PgPool) {
    let st = state(pool.clone());
    let app = fraglink_server::router(st.clone());
    signup(&app, "dave@example.com", "dave").await;
    let uid: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users WHERE handle = 'dave'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let g = GameRef {
        id: "exe:oddgame.exe".into(),
        name: Some("Odd Game".into()),
        started_at: None,
        detail: None,
        hidden: false,
    };
    presence::set_from(&st, uid, "dave", "online", Some(Some(g)))
        .await
        .unwrap();
    // a fresh server: nothing in memory
    let st2 = state(pool.clone());
    let mut v = presence::visible_presence(&st2, uid)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(v.game_name.as_deref(), Some("a game"));
    presence::apply_own_name(&st2, uid, &mut v).await.unwrap();
    assert_eq!(v.game_name.as_deref(), Some("Odd Game"));
    let names = presence::own_names_for(&st2, uid, &["exe:oddgame.exe".to_string()])
        .await
        .unwrap();
    assert_eq!(
        names.get("exe:oddgame.exe").map(String::as_str),
        Some("Odd Game")
    );
}
