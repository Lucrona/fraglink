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
    presence::apply_own_name(&st, uid, &mut mine);
    assert_eq!(mine.game_name.as_deref(), Some("Odd Game"));
    assert_eq!(mine.game_id.as_deref(), Some("exe:oddgame.exe"));
    // no name sent (older app) or game stopped: back to "a game"
    presence::set_from(&st, uid, "carol", "online", Some(None))
        .await
        .unwrap();
    let mut v = presence::visible_presence(&st, uid).await.unwrap().unwrap();
    presence::apply_own_name(&st, uid, &mut v);
    assert_eq!(v.game_name, None);
}
