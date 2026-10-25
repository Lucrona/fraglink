//! Detection fix-ups staff edit in Admin > Detection (0.3.0-beta.4): Steam app ids
//! and game ids that count as another game, and folder + exe rules. The app gets
//! them with the override rules; the daily Steam sync uses the same list, so a
//! merged game's Steam hours are not split back out.

use crate::admin::{audit, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use fraglink_core::overrides::{Alias, OverrideFile, Rule};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Row {
    pub id: i64,
    pub kind: String,
    pub pattern: String,
    pub game_id: String,
    pub note: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/detection", get(list).post(add))
        .route("/api/v1/admin/detection/{id}/delete", post(remove))
}

pub async fn load(st: &AppState) -> ApiResult<Vec<Row>> {
    Ok(sqlx::query_as(
        "SELECT id, kind, pattern, game_id, note FROM detection_aliases ORDER BY kind, lower(pattern)",
    )
    .fetch_all(&st.db)
    .await?)
}

/// id -> id aliases: Admin rows (Steam ones keyed by the id that app id gets) plus merged games.
pub fn alias_map(rows: &[Row], merged: &[(String, String)]) -> HashMap<String, String> {
    let mut m: HashMap<String, String> = merged.iter().cloned().collect();
    for r in rows {
        let from = match r.kind.as_str() {
            // the id the apps and the sync give this app id (a known game's own id)
            "steam" => match r.pattern.trim().parse::<u32>() {
                Ok(n) => fraglink_core::detector::steam_game_id(crate::steam::sigs(), n),
                Err(_) => continue,
            },
            "id" => r.pattern.trim().to_string(),
            _ => continue,
        };
        m.insert(from.to_lowercase(), r.game_id.clone());
    }
    m
}

/// Follow the aliases from `id` (a few hops, so A -> B -> C works and loops stop).
pub fn resolve(map: &HashMap<String, String>, id: &str) -> String {
    let mut cur = id.to_string();
    for _ in 0..4 {
        match map.get(&cur.to_lowercase()) {
            Some(n) if *n != cur => cur = n.clone(),
            _ => break,
        }
    }
    cur
}

/// `Some(normalised)` for a usable folder + exe rule.
pub fn valid_path(p: &str) -> Option<String> {
    let p = p.trim().replace('\\', "/").to_lowercase();
    let p = p.trim_start_matches('/').to_string();
    let exe = p.rsplit('/').next()?;
    (p.contains('/')
        && p.len() <= 200
        && crate::games::valid_exe(exe)
        && !p.contains(':')
        && !p.split('/').any(|s| s.is_empty() || s == ".."))
    .then_some(p)
}

/// Add the list to the overrides sent to the apps: aliases, and path rules first
/// (so they beat plain exe-name rules).
pub fn apply(
    f: &mut OverrideFile,
    rows: &[Row],
    aliases: &HashMap<String, String>,
    name: impl Fn(&str) -> String,
) {
    let mut a: Vec<_> = aliases.iter().collect();
    a.sort();
    for (from, to) in a {
        if !f.aliases.iter().any(|x| x.from.eq_ignore_ascii_case(from)) {
            f.aliases.push(Alias {
                from: from.clone(),
                to: to.clone(),
            });
        }
    }
    let mut at = 0;
    for r in rows.iter().filter(|r| r.kind == "path") {
        let Some(p) = valid_path(&r.pattern) else {
            continue;
        };
        f.rules.insert(
            at,
            Rule {
                id: r.game_id.clone(),
                name: name(&r.game_id),
                exes: vec![p],
                paths: vec![],
                channel: None,
                note: Some(r.note.clone().unwrap_or_else(|| "detection fix-up".into())),
            },
        );
        at += 1;
    }
}

/// Steam app id -> game id for the Steam sync, same order as the app: Admin
/// aliases and merges on top of the usual `steam:N` / signature id.
pub struct SteamIds(HashMap<String, String>);

impl SteamIds {
    pub async fn load(st: &AppState) -> ApiResult<Self> {
        let rows = load(st).await?;
        Ok(SteamIds(alias_map(&rows, &merged(st).await?)))
    }
    pub fn id(&self, minted: &str) -> String {
        resolve(&self.0, minted)
    }
}

pub async fn merged(st: &AppState) -> ApiResult<Vec<(String, String)>> {
    Ok(sqlx::query_as(
        "SELECT game_id, successor_id FROM game_retirements WHERE merged ORDER BY game_id",
    )
    .fetch_all(&st.db)
    .await?)
}

async fn list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows = load(&st).await?;
    Ok(Json(json!({ "rules": rows.iter().map(|r| json!({
        "id": r.id, "kind": r.kind, "pattern": r.pattern, "game_id": r.game_id, "note": r.note,
    })).collect::<Vec<_>>() })))
}

#[derive(Deserialize)]
struct AddReq {
    kind: String,
    pattern: String,
    game_id: String,
    note: Option<String>,
}

async fn add(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<AddReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let game = r.game_id.trim();
    let pattern = match r.kind.as_str() {
        "steam" => r
            .pattern
            .trim()
            .parse::<u32>()
            .map(|n| n.to_string())
            .map_err(|_| AppError::bad_request("bad_pattern", "A Steam app id is a number."))?,
        "id" => r.pattern.trim().to_string(),
        "path" => valid_path(&r.pattern).ok_or_else(|| {
            AppError::bad_request(
                "bad_pattern",
                "Use folder/exe, e.g. world of warcraft/_retail_/wow.exe",
            )
        })?,
        _ => return Err(AppError::bad_request("bad_kind", "Pick steam, id or path.")),
    };
    if pattern.is_empty()
        || pattern.len() > 200
        || game.is_empty()
        || pattern.eq_ignore_ascii_case(game)
    {
        return Err(AppError::bad_request("bad_rule", "Fill in both sides."));
    }
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id = $1)")
        .bind(game)
        .fetch_one(&st.db)
        .await?;
    if !known {
        return Err(AppError::bad_request(
            "unknown_game",
            "No game with that id.",
        ));
    }
    let note = r.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO detection_aliases(kind, pattern, game_id, note) VALUES($1, $2, $3, $4)
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(&r.kind)
    .bind(&pattern)
    .bind(game)
    .bind(note)
    .fetch_optional(&st.db)
    .await?;
    let id =
        id.ok_or_else(|| AppError::bad_request("exists", "There is already a rule for that."))?;
    audit(
        &st,
        &s,
        "detection.add",
        &format!("detection:{id}"),
        Value::Null,
        json!({ "kind": r.kind, "pattern": pattern, "game_id": game, "note": note }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

async fn remove(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let gone: Option<(String, String, String)> = sqlx::query_as(
        "DELETE FROM detection_aliases WHERE id = $1 RETURNING kind, pattern, game_id",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (kind, pattern, game_id) = gone.ok_or_else(AppError::not_found)?;
    audit(
        &st,
        &s,
        "detection.delete",
        &format!("detection:{id}"),
        json!({ "kind": kind, "pattern": pattern, "game_id": game_id }),
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "deleted": id })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, pattern: &str, game: &str) -> Row {
        Row {
            id: 1,
            kind: kind.into(),
            pattern: pattern.into(),
            game_id: game.into(),
            note: None,
        }
    }

    #[test]
    fn steam_alias_and_merge_chain() {
        let rows = vec![
            row("steam", "793460", "112_operator"),
            row("id", "Old_Game", "mid"),
        ];
        let m = alias_map(&rows, &[("mid".into(), "new".into())]);
        assert_eq!(resolve(&m, "steam:793460"), "112_operator");
        assert_eq!(resolve(&m, "old_game"), "new");
        assert_eq!(resolve(&m, "steam:1"), "steam:1");
        let looped = alias_map(&[row("id", "a", "b"), row("id", "b", "a")], &[]);
        assert!(["a", "b"].contains(&resolve(&looped, "a").as_str()));
    }

    #[test]
    fn path_rules_are_checked_and_go_first() {
        assert_eq!(
            valid_path(r"\World of Warcraft\_retail_\Wow.exe").as_deref(),
            Some("world of warcraft/_retail_/wow.exe")
        );
        assert_eq!(valid_path("wow.exe"), None);
        assert_eq!(valid_path("c:/games/wow.exe"), None);
        assert_eq!(valid_path("a/../wow.exe"), None);
        assert_eq!(valid_path("folder/readme.txt"), None);
        let mut f: OverrideFile =
            serde_json::from_str(r#"{"schema":1,"version":"t","rules":[]}"#).unwrap();
        f.rules.push(Rule {
            id: "wow".into(),
            name: "WoW".into(),
            exes: vec!["wow.exe".into()],
            paths: vec![],
            channel: None,
            note: None,
        });
        let rows = vec![
            row("path", "_forever_/wow.exe", "wow_forever"),
            row("steam", "793460", "112_operator"),
        ];
        apply(&mut f, &rows, &alias_map(&rows, &[]), |id| {
            id.to_uppercase()
        });
        assert_eq!(f.rules[0].id, "wow_forever");
        assert_eq!(f.rules[0].exes, vec!["_forever_/wow.exe".to_string()]);
        assert_eq!(f.aliases.len(), 1);
        assert_eq!(f.aliases[0].to, "112_operator");
    }
}
