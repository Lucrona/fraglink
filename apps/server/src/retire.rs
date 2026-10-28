//! Retired games (dev.18). See `migrations/0012_game_retirements.sql`.
//!
//! Clients keep sending the old game id (their detection lists still map the program
//! there); the server records that play under the successor instead, so the retired
//! entry's hours stay frozen and the successor starts from zero.

use crate::error::ApiResult;
use crate::AppState;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub type Map = Arc<HashMap<String, String>>;

#[derive(Default)]
pub struct RetireCache {
    map: RwLock<Option<Map>>,
}

/// Re-read every retirement (after a retire / unretire).
pub async fn reload(st: &AppState) -> ApiResult<Map> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT game_id, successor_id FROM game_retirements WHERE NOT partial")
            .fetch_all(&st.db)
            .await?;
    let m: Map = Arc::new(rows.into_iter().collect());
    *st.retired.map.write().unwrap() = Some(m.clone());
    Ok(m)
}

pub async fn current(st: &AppState) -> ApiResult<Map> {
    let snap = st.retired.map.read().unwrap().clone();
    match snap {
        Some(m) => Ok(m),
        None => reload(st).await,
    }
}

/// The id play for `id` is recorded under: its successor when `id` is retired.
pub async fn effective_id(st: &AppState, id: &str) -> ApiResult<String> {
    Ok(current(st)
        .await?
        .get(id)
        .cloned()
        .unwrap_or_else(|| id.to_string()))
}

/// The program a retired game's successor is named after: the first plain exe name
/// the server knows for `id` (signatures, detectable list, staff extra exes).
pub async fn successor_exe(st: &AppState, id: &str) -> ApiResult<Option<String>> {
    if let Some(exe) = id.strip_prefix("exe:") {
        // an approved program: nothing else would identify it
        return Ok(Some(exe.to_string()));
    }
    for (exes, _) in crate::games::rename_patterns(st, id) {
        if let Some(e) = exes.into_iter().find(|e| crate::games::valid_exe(e)) {
            return Ok(Some(e.to_lowercase()));
        }
    }
    let ov = crate::game_overrides::current(st).await?;
    Ok(ov
        .get(id)
        .and_then(|o| o.extra_exes.iter().find(|e| crate::games::valid_exe(e)))
        .map(|e| e.to_lowercase()))
}

/// Every plain exe name the server knows for `id` (signatures / detectable list, staff
/// extra exes), lower-case, in order, de-duplicated. `exe:<name>` ids are their own exe.
pub async fn all_exes(st: &AppState, id: &str) -> ApiResult<Vec<String>> {
    let mut out: Vec<String> = vec![];
    if let Some(exe) = id.strip_prefix("exe:") {
        out.push(exe.to_lowercase());
    }
    for (exes, _) in crate::games::rename_patterns(st, id) {
        out.extend(
            exes.into_iter()
                .filter(|e| crate::games::valid_exe(e))
                .map(|e| e.to_lowercase()),
        );
    }
    if let Some(o) = crate::game_overrides::current(st).await?.get(id) {
        out.extend(
            o.extra_exes
                .iter()
                .filter(|e| crate::games::valid_exe(e))
                .map(|e| e.to_lowercase()),
        );
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|e| seen.insert(e.clone()));
    Ok(out)
}

/// Programs detached from a still-live game by a partial retirement: `(game id, exe)`.
/// Each is served as a rule of its own (`exe:<name>`) in the overrides list.
pub async fn detached_exes(st: &AppState) -> ApiResult<Vec<(String, String)>> {
    let rows: Vec<(String, Vec<String>)> =
        sqlx::query_as("SELECT game_id, exes FROM game_retirements WHERE partial ORDER BY game_id")
            .fetch_all(&st.db)
            .await?;
    Ok(rows
        .into_iter()
        .flat_map(|(g, es)| es.into_iter().map(move |e| (g.clone(), e)))
        .collect())
}

/// Fully retired games (not merged): `(game id, successor id, the programs detached)`.
pub async fn fully_retired(st: &AppState) -> ApiResult<Vec<(String, String, Vec<String>)>> {
    Ok(sqlx::query_as(
        "SELECT game_id, successor_id, exes FROM game_retirements
         WHERE NOT partial AND NOT merged ORDER BY game_id",
    )
    .fetch_all(&st.db)
    .await?)
}
