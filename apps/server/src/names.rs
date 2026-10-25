//! Server-authoritative game names (0.2.0-dev.14).
//!
//! Clients send a game id only. What friends, the activity feed, stats and the
//! website show comes from the server's own lists: Discord's detectable list,
//! Fraglink's override rules and signatures, names fetched from Steam, and games
//! an admin approved from "Track this as a game?" answers (`games.verified`).
//! Anything else is shown as "a game" (presence / activity) or "Unlisted game"
//! (stats rows); a name typed on a PC never leaves that PC's display.

use crate::error::ApiResult;
use crate::AppState;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use uuid::Uuid;

/// Public game id for a game the server can't name (keeps the user's own id private).
pub const UNLISTED_ID: &str = "unlisted";
/// Public name in presence and activity: "Playing a game", "played a game for 2h".
pub const UNLISTED_NAME: &str = "a game";
/// Row title in stats lists.
pub const UNLISTED_TITLE: &str = "Unlisted game";

/// The server's id for a client-sent game id, or None when it isn't usable.
/// * `manual:<slug>` (tags from older apps) is scoped to the user: `manual:<user>:<slug>`.
/// * `exe:<name>.exe` (a tagged program, dev.14 apps) is lower-cased.
/// * anything else (`wow`, `steam:570`, `star_citizen`) is kept as is.
pub fn canonical_id(user: Uuid, id: &str) -> Option<String> {
    let id = id.trim();
    if id.is_empty() || id.len() > 120 || id.chars().any(char::is_control) {
        return None;
    }
    // WoW ids from apps before dev.14 (Fraglink's own folder rules), mapped onto
    // Discord's entries; Classic PTR builds aren't tracked.
    match id {
        "wow_ptr" | "wow_beta" => return Some("wow".into()),
        "wow_classic_era" | "wow_anniversary" => return Some("wow_classic".into()),
        "wow_classic_ptr" | "wow_classic_era_ptr" => return None,
        _ => {}
    }
    if let Some(exe) = id.strip_prefix("exe:") {
        return crate::games::valid_exe(exe).then(|| format!("exe:{}", exe.trim().to_lowercase()));
    }
    crate::hours::server_game_id(user, id)
}

/// beta.5: a program the user added (`exe:<name>.exe`) that Fraglink's games list now
/// knows counts as that list game from here on. Earlier sessions stay under the
/// `exe:` id until staff merge it (Admin > Games > Merge), so nothing is lost or
/// counted twice.
/// beta.6: checks Discord's detectable list too, not only the built-in games.json.
pub fn list_id_for_exe(st: &AppState, id: &str) -> String {
    exe_to_list(&st.sigs, crate::games::detectable_db(st).as_deref(), id)
}

fn exe_to_list(
    sigs: &fraglink_core::signatures::SignatureDb,
    det: Option<&fraglink_core::detectable::DetectableDb>,
    id: &str,
) -> String {
    let Some(exe) = id.strip_prefix("exe:") else {
        return id.to_string();
    };
    sigs.match_process(exe, None)
        .filter(|g| !g.uncertain)
        .map(|g| g.id.clone())
        // a bare exe name only: path-only entries and generic names (game.exe) never match
        .or_else(|| det?.match_process(exe, None).map(|m| m.game.slug.clone()))
        .unwrap_or_else(|| id.to_string())
}

/// Name from Fraglink's own lists (no database).
pub fn list_name(st: &AppState, id: &str) -> Option<String> {
    if id.starts_with("manual:") || id.starts_with("exe:") {
        return None;
    }
    st.games
        .detectable_name(id)
        .or_else(|| st.games.override_name(&st.cfg.data_dir, id))
        .or_else(|| st.sigs.get(id).map(|g| g.name.clone()))
}

/// Public names for `ids`; ids missing from the map are unlisted.
pub async fn public_names(st: &AppState, ids: &[String]) -> ApiResult<HashMap<String, String>> {
    let mut out = HashMap::new();
    let mut rest = Vec::new();
    let ov = crate::game_overrides::current(st).await?;
    let mut hidden = HashSet::new();
    for id in ids {
        if out.contains_key(id) || rest.contains(id) || hidden.contains(id) {
            continue;
        }
        // staff edits win over every list: hidden = unlisted for everyone
        if let Some(o) = ov.get(id) {
            if o.hidden {
                hidden.insert(id.clone());
                continue;
            }
            if let Some(n) = &o.name {
                out.insert(id.clone(), n.clone());
                continue;
            }
        }
        match list_name(st, id) {
            Some(n) => {
                out.insert(id.clone(), n);
            }
            None => rest.push(id.clone()),
        }
    }
    if !rest.is_empty() {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, name FROM games WHERE verified AND id = ANY($1)")
                .bind(&rest)
                .fetch_all(&st.db)
                .await?;
        out.extend(rows);
        // Steam games nobody named yet get their store name in the background.
        for id in rest.iter().filter(|i| !out.contains_key(*i)) {
            lookup_steam_name(st, id);
        }
    }
    Ok(out)
}

pub async fn public_name(st: &AppState, id: &str) -> ApiResult<Option<String>> {
    Ok(public_names(st, &[id.to_string()]).await?.remove(id))
}

/// `(game_id, game_name)` as friends may see them.
pub fn public_pair(id: &str, names: &HashMap<String, String>) -> (String, String) {
    match names.get(id) {
        Some(n) => (id.to_string(), n.clone()),
        None => (UNLISTED_ID.to_string(), UNLISTED_NAME.to_string()),
    }
}

/// Make sure a `games` row exists for a recorded session (the name is only a
/// fallback for the database; display always goes through [`public_names`]).
pub async fn ensure_game_row(st: &AppState, id: &str) -> ApiResult<()> {
    let listed = list_name(st, id);
    // An id nobody knows (not in the games list, not steam:/exe:/manual:) is queued in
    // Game approvals; until someone decides it shows as "Playing a game".
    if listed.is_none()
        && !id.starts_with("steam:")
        && !id.starts_with("exe:")
        && !id.starts_with("manual:")
        && crate::steam::sigs().get(id).is_none()
    {
        sqlx::query(
            "INSERT INTO unknown_games(game_id) SELECT $1 WHERE NOT EXISTS (SELECT 1 FROM games WHERE id = $1 AND verified)
             ON CONFLICT (game_id) DO UPDATE SET last_at = now()",
        )
        .bind(id)
        .execute(&st.db)
        .await?;
    }
    let name = listed.unwrap_or_else(|| UNLISTED_TITLE.to_string());
    let appid = id
        .strip_prefix("steam:")
        .and_then(|a| a.parse::<i32>().ok());
    sqlx::query(
        "INSERT INTO games(id, name, steam_appid) VALUES($1, $2, $3) ON CONFLICT (id) DO NOTHING",
    )
    .bind(id)
    .bind(name)
    .bind(appid)
    .execute(&st.db)
    .await?;
    Ok(())
}

/// A `steam:<appid>` game nobody named yet: look its name up on the Steam store
/// once per process (in the background) and keep it as a verified name.
pub fn lookup_steam_name(st: &AppState, id: &str) {
    let Some(appid) = id
        .strip_prefix("steam:")
        .and_then(|a| a.parse::<u32>().ok())
    else {
        return;
    };
    static TRIED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    if !TRIED
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(appid)
    {
        return;
    }
    let Some(base) = st.cfg.steam_store_url.clone() else {
        return;
    };
    let st = st.clone();
    let id = id.to_string();
    tokio::spawn(async move {
        let url = format!("{base}/api/appdetails?appids={appid}&filters=basic");
        let name = async {
            let v: serde_json::Value = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .ok()?
                .get(url)
                .send()
                .await
                .ok()?
                .json()
                .await
                .ok()?;
            let d = &v[appid.to_string()];
            if d["success"] != true {
                return None;
            }
            let n: String = d["data"]["name"]
                .as_str()?
                .chars()
                .filter(|c| !c.is_control())
                .take(120)
                .collect();
            (!n.trim().is_empty()).then(|| n.trim().to_string())
        }
        .await;
        let Some(name) = name else {
            tracing::info!("steam store: no name for app {appid}");
            return;
        };
        let r = sqlx::query(
            "INSERT INTO games(id, name, steam_appid, verified) VALUES($1, $2, $3, true)
             ON CONFLICT (id) DO UPDATE SET name = excluded.name, verified = true WHERE NOT games.verified",
        )
        .bind(&id)
        .bind(&name)
        .bind(appid as i32)
        .execute(&st.db)
        .await;
        match r {
            Ok(_) => tracing::info!("steam store: {id} is {name:?}"),
            Err(e) => tracing::warn!("steam store: saving {id} failed: {e}"),
        }
    });
}

/// Admin: approve a "Track this as a game?" program under a name. Its `exe:` game
/// keeps every session already recorded and shows the approved name from now on.
pub async fn approve(db: &sqlx::PgPool, exe: &str, name: &str) -> anyhow::Result<String> {
    let exe = exe.trim();
    let name = name.trim();
    if !crate::games::valid_exe(exe) {
        anyhow::bail!("{exe:?} isn't a program name like Game.exe");
    }
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        anyhow::bail!("the name must be 1-80 characters");
    }
    let id = format!("exe:{}", exe.to_lowercase());
    sqlx::query(
        "INSERT INTO games(id, name, verified) VALUES($1, $2, true)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, verified = true",
    )
    .bind(&id)
    .bind(name)
    .execute(db)
    .await?;
    sqlx::query("UPDATE game_suggestions SET status = 'accepted' WHERE lower(exe) = lower($1)")
        .bind(exe)
        .execute(db)
        .await?;
    Ok(id)
}

/// Admin: take an approval back (the game is shown as unlisted again).
pub async fn unapprove(db: &sqlx::PgPool, exe: &str) -> anyhow::Result<u64> {
    let id = format!("exe:{}", exe.trim().to_lowercase());
    Ok(
        sqlx::query("UPDATE games SET verified = false WHERE id = $1")
            .bind(&id)
            .execute(db)
            .await?
            .rows_affected(),
    )
}

/// Where sessions for `id` should go: an unknown id an admin mapped to a real game.
pub async fn resolve_alias(st: &AppState, id: &str) -> ApiResult<String> {
    let m: Option<String> = sqlx::query_scalar(
        "SELECT mapped_to FROM unknown_games WHERE game_id = $1 AND status = 'mapped'",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?
    .flatten();
    Ok(m.unwrap_or_else(|| id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ids() {
        let u = Uuid::nil();
        assert_eq!(canonical_id(u, "wow").as_deref(), Some("wow"));
        assert_eq!(
            canonical_id(u, "exe:Odd Game.EXE").as_deref(),
            Some("exe:odd game.exe")
        );
        assert_eq!(canonical_id(u, "exe:C:\\x.exe"), None);
        assert_eq!(
            canonical_id(u, "manual:my-thing").as_deref(),
            Some("manual:00000000000000000000000000000000:my-thing")
        );
        assert_eq!(canonical_id(u, " "), None);
        assert_eq!(canonical_id(u, "wow_ptr").as_deref(), Some("wow"));
        assert_eq!(
            canonical_id(u, "wow_classic_era").as_deref(),
            Some("wow_classic")
        );
        assert_eq!(canonical_id(u, "wow_classic_ptr"), None);
        let names: HashMap<String, String> =
            [("wow".to_string(), "World of Warcraft".to_string())].into();
        assert_eq!(public_pair("wow", &names).1, "World of Warcraft");
        assert_eq!(
            public_pair("exe:odd.exe", &names),
            ("unlisted".into(), "a game".into())
        );
    }
}

#[cfg(test)]
mod exe_list_tests {
    #[test]
    fn known_exe_counts_as_list_game_unknown_stays() {
        let db = fraglink_core::signatures::SignatureDb::builtin();
        let (g, exe) = db
            .games()
            .iter()
            .filter(|g| !g.uncertain)
            .flat_map(|g| g.exes.iter().map(move |e| (g, e.to_lowercase())))
            .find(|(g, e)| db.match_process(e, None).map(|m| &m.id) == Some(&g.id))
            .unwrap();
        assert_eq!(super::exe_to_list(&db, None, &format!("exe:{exe}")), g.id);
        assert_eq!(
            super::exe_to_list(&db, None, "exe:zz_not_a_game_42.exe"),
            "exe:zz_not_a_game_42.exe"
        );
        assert_eq!(super::exe_to_list(&db, None, &g.id), g.id);
    }

    #[test]
    fn discord_list_exe_counts_as_list_game() {
        // beta.6: a program only Discord's detectable list knows maps onto that game too
        let db = fraglink_core::signatures::SignatureDb::builtin();
        let det = fraglink_core::detectable::DetectableDb::bundled();
        let (slug, exe) = det
            .games()
            .iter()
            .flat_map(|g| g.exes.iter().map(move |e| (g, e)))
            .filter(|(_, e)| !e.contains('/') && db.match_process(e, None).is_none())
            .find(|(g, e)| {
                det.match_process(e, None)
                    .is_some_and(|m| m.game.slug == g.slug)
            })
            .map(|(g, e)| (g.slug.clone(), e.clone()))
            .unwrap();
        let id = format!("exe:{exe}");
        assert_eq!(super::exe_to_list(&db, None, &id), id);
        assert_eq!(super::exe_to_list(&db, Some(&det), &id), slug);
        assert_eq!(
            super::exe_to_list(&db, Some(&det), "exe:zz_not_a_game_42.exe"),
            "exe:zz_not_a_game_42.exe"
        );
    }
}
