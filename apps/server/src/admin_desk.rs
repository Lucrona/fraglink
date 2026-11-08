//! Detection tester, missing artwork, undo, data checks, and review history.
//!
//! The tester and the data checks only read. Undo restores a few audited edits
//! when the row still matches what the audit wrote afterwards. Merges, session
//! deletes and account deletes are labelled irreversible.

use std::collections::BTreeSet;

use crate::admin::{audit, bad, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::week;
use crate::AppState;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use fraglink_core::detector::{DetectSource, Detector};
use fraglink_core::overrides::Overrides;
use fraglink_core::steam::SteamIndex;
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/detect-test", post(detect_test))
        .route("/api/v1/admin/artwork", get(artwork))
        .route("/api/v1/admin/undo", get(undo_list).post(undo_apply))
        .route("/api/v1/admin/data-checks", get(data_checks))
        .route("/api/v1/admin/review-history", get(history))
}

// ---------------------------------------------------------------------------
// Detection tester

#[derive(Deserialize)]
struct DetectReq {
    exe: String,
    #[serde(default)]
    folder: String,
}

async fn detect_test(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<DetectReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let exe = r.exe.trim().replace('\\', "/");
    let exe_name = exe.rsplit('/').next().unwrap_or(&exe).trim().to_string();
    if exe_name.is_empty() || exe_name.len() > 200 || exe_name.chars().any(char::is_control) {
        return Err(bad("bad_exe", "Type an exe name like Wow.exe."));
    }
    let folder = r.folder.trim().trim_end_matches(['/', '\\']);
    if folder.len() > 400 || folder.chars().any(char::is_control) {
        return Err(bad("bad_folder", "That folder path is too long."));
    }
    let path = if folder.is_empty() {
        format!("c:/games/x/{exe_name}")
    } else {
        format!("{folder}/{exe_name}").replace('\\', "/")
    };
    let overrides = match crate::games::overrides_with_approved(&st).await {
        Ok(j) => Overrides::from_json(&j).unwrap_or_else(|_| Overrides::builtin()),
        Err(_) => Overrides::builtin(),
    };
    let mut det = Detector::new((*st.sigs).clone(), SteamIndex::default());
    det.overrides = overrides;
    if let Some(db) = crate::games::detectable_db(&st) {
        det.detectable = db;
    }
    let matched = det.classify_file(&path);
    let ignored = st.sigs.is_ignored(&exe_name);
    let over = det
        .overrides
        .match_process(&exe_name, Some(&path))
        .map(|r| json!({ "game_id": r.id, "name": r.name, "paths": r.paths }));
    let sig = st
        .sigs
        .match_process(&exe_name, Some(&path))
        .map(|g| json!({ "game_id": g.id, "name": g.name, "hints": g.path_hints }));
    let listed = det
        .detectable
        .match_process(&exe_name, Some(&path))
        .map(|m| json!({ "game_id": m.game.slug, "name": m.game.name, "pattern": m.pattern }));
    let mut steps = Vec::new();
    steps.push(json!({
        "layer": "Ignored program",
        "hit": ignored,
        "detail": if ignored { "Signatures say this exe is a launcher or helper, so it is not a game." } else { "Not on the ignore list." },
    }));
    steps.push(json!({
        "layer": "Folder rule",
        "hit": over.is_some(),
        "detail": over.clone().unwrap_or(Value::Null),
    }));
    steps.push(json!({
        "layer": "Built-in signature",
        "hit": sig.is_some(),
        "detail": sig.clone().unwrap_or(Value::Null),
    }));
    steps.push(json!({
        "layer": "Games list",
        "hit": listed.is_some(),
        "detail": listed.clone().unwrap_or(Value::Null),
    }));
    let ambiguous = match (&sig, &listed) {
        (Some(a), Some(b)) if a["game_id"] != b["game_id"] => Some(
            "The signature and the games list name different games. The signature wins, same as the app, before any launcher folder is considered.",
        ),
        _ => None,
    };
    Ok(Json(json!({
        "exe": exe_name,
        "path": path,
        "folder_given": !r.folder.trim().is_empty(),
        "matched": matched.as_ref().map(|d| json!({
            "game_id": d.game_id,
            "name": d.name,
            "source": source_label(d.source),
            "steam_appid": d.steam_appid,
        })),
        "steps": steps,
        "ambiguous": ambiguous,
        "not_reproduced": [
            not_repro("manual_tags", "Per-PC manual tags", "A tag someone set on their own PC is not on the server."),
            not_repro("steam_library", "Steam install folder", "The app can recognise a game because the exe sits in that PC's steamapps folder. Those folders are not uploaded."),
            not_repro("steam_running", "Steam 'now playing' app id", "Only the running Steam client on that PC knows this."),
            not_repro("other_launchers", "Epic, GOG, Battle.net, EA, Ubisoft, Xbox, RSI folders", "Those install lists stay on the PC. They do not decide that something is a game; they can only keep an older epic: or gog: id when the games list already matched the exe inside that install."),
        ],
    })))
}

fn source_label(s: DetectSource) -> &'static str {
    match s {
        DetectSource::Manual => "manual tag",
        DetectSource::Override => "folder rule",
        DetectSource::Signature => "built-in signature",
        DetectSource::Detectable => "games list",
        DetectSource::SteamLibrary => "Steam library folder",
        DetectSource::SteamRunning => "Steam now playing",
        _ => "launcher folder",
    }
}

fn not_repro(id: &str, label: &str, detail: &str) -> Value {
    json!({ "id": id, "label": label, "detail": detail })
}

// ---------------------------------------------------------------------------
// Missing artwork. Never fetches an icon.

async fn artwork(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<ArtQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let ids = catalogue_ids(&st).await?;
    let custom = crate::game_overrides::custom_icon_dir(&st);
    let cache = st.cfg.data_dir.join("icon-cache");
    let idx = st.games.icon_index();
    let mut rows = Vec::new();
    for id in ids {
        let k = crate::icons::key(&id);
        let has_file = custom.join(format!("{k}-32.png")).is_file()
            || cache.join(format!("{k}-32.png")).is_file();
        if has_file {
            continue;
        }
        let miss = cache.join(format!("{k}.miss")).is_file();
        let listed = idx.by_slug.contains_key(&id);
        let steam = st
            .sigs
            .get(&id)
            .and_then(|g| g.steam_appids.first().copied())
            .is_some()
            || id.starts_with("steam:");
        let status = if miss {
            "unavailable"
        } else if listed || steam {
            "not_cached"
        } else {
            "missing"
        };
        if let Some(want) = q.status.as_deref().filter(|s| !s.is_empty()) {
            if want != status {
                continue;
            }
        }
        let needle = q.q.as_deref().unwrap_or("").trim().to_lowercase();
        let name = crate::names::list_name(&st, &id).unwrap_or_else(|| id.clone());
        if !needle.is_empty()
            && !id.to_lowercase().contains(&needle)
            && !name.to_lowercase().contains(&needle)
        {
            continue;
        }
        rows.push(json!({
            "id": id,
            "name": name,
            "status": status,
            "detail": match status {
                "unavailable" => "A previous look found no official icon. Nothing is fetched again from this page.",
                "not_cached" => "A games-list or Steam source may exist, but no file is cached. Opening the game editor fetches one icon; this queue does not.",
                _ => "No icon file and no games-list or Steam source on this server.",
            },
        }));
    }
    rows.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    let total = rows.len() as i64;
    let off = q.offset.unwrap_or(0).max(0);
    let page: Vec<Value> = rows.into_iter().skip(off as usize).take(50).collect();
    Ok(Json(json!({
        "total": total, "offset": off, "page_size": 50, "rows": page,
        "note": "This list only looks at files already on the server. It does not download icons.",
    })))
}

#[derive(Deserialize)]
struct ArtQ {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
}

async fn catalogue_ids(st: &AppState) -> ApiResult<Vec<String>> {
    let mut ids = BTreeSet::new();
    if let Some(db) = crate::games::detectable_db(st) {
        for g in db.games() {
            ids.insert(g.slug.clone());
        }
    }
    for g in st.sigs.games() {
        ids.insert(g.id.clone());
    }
    let db: Vec<String> = sqlx::query_scalar("SELECT id FROM games WHERE id NOT LIKE 'manual:%'")
        .fetch_all(&st.db)
        .await?;
    ids.extend(db);
    Ok(ids.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Undo. Only edits whose before/after is still the whole change.

const IRREVERSIBLE: &[(&str, &str)] = &[
    (
        "game.merge",
        "A merge moves sessions and retires the old id. It is not undone here.",
    ),
    (
        "user.delete",
        "Deleting an account removes its rows. It is not undone here.",
    ),
    (
        "hours.delete_session",
        "The session row is gone. It is not undone here.",
    ),
    (
        "hours.close_session",
        "Closing a session writes or drops time. It is not undone here.",
    ),
    (
        "game.delete",
        "Deleting a game removes its row. It is not undone here.",
    ),
    ("game.icon_upload", "The previous picture was not kept."),
    ("game.art", "The previous picture was not kept."),
];

async fn undo_list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows: Vec<(
        i64,
        DateTime<Utc>,
        String,
        String,
        String,
        Option<Value>,
        Option<Value>,
    )> = sqlx::query_as(
        "SELECT id, created_at, actor_handle, action, COALESCE(target, ''), before, after
         FROM admin_audit ORDER BY id DESC LIMIT 80",
    )
    .fetch_all(&st.db)
    .await?;
    let mut out = Vec::new();
    for (id, at, who, action, target, before, after) in rows {
        let (undo, why) = undo_kind(&action);
        out.push(json!({
            "id": id, "at": at, "by": who, "action": action, "target": target,
            "before": before, "after": after, "undo": undo, "why": why,
        }));
    }
    Ok(Json(json!({ "rows": out })))
}

fn undo_kind(action: &str) -> (bool, &'static str) {
    if let Some((_, why)) = IRREVERSIBLE.iter().find(|(a, _)| *a == action) {
        return (false, why);
    }
    match action {
        "game.status" => (
            true,
            "Puts the status, date and note back, if they are still what this edit set.",
        ),
        "detection.edit" => (
            true,
            "Puts the fix-up's game and note back, if they are still what this edit set.",
        ),
        "detection.add" => (
            true,
            "Removes the fix-up, if it is still the one this edit added.",
        ),
        "detection.delete" => (
            true,
            "Puts the fix-up back. A note that was not stored in the audit stays empty.",
        ),
        "site_setting" => (
            true,
            "Puts the switch or the link back, if it is still what this edit set.",
        ),
        "hours.review" => (
            true,
            "Opens the Hours Review flag again, if this review still matches the current evidence.",
        ),
        "suggested_merge.dismiss" | "merge_suggestion.dismiss" => (
            true,
            "Shows the suggestion again. It does not un-merge anything.",
        ),
        _ => (false, "This action is not undone from here."),
    }
}

#[derive(Deserialize)]
struct UndoReq {
    id: i64,
    confirm: String,
}

async fn undo_apply(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<UndoReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if r.confirm.trim() != "UNDO" {
        return Err(bad("confirm", "Type UNDO to confirm."));
    }
    let row: Option<(String, String, Option<Value>, Option<Value>)> = sqlx::query_as(
        "SELECT action, COALESCE(target, ''), before, after FROM admin_audit WHERE id = $1",
    )
    .bind(r.id)
    .fetch_optional(&st.db)
    .await?;
    let (action, target, before, after) = row.ok_or_else(AppError::not_found)?;
    let before = before.unwrap_or(Value::Null);
    let after = after.unwrap_or(Value::Null);
    let (ok, _) = undo_kind(&action);
    if !ok {
        return Err(bad(
            "not_reversible",
            "That action is not undone from here.",
        ));
    }
    apply_undo(&st, &s, &action, &target, &before, &after).await?;
    audit(
        &st,
        &s,
        "undo",
        &format!("audit:{}", r.id),
        json!({ "action": action, "after": after }),
        json!({ "restored": before }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn apply_undo(
    st: &AppState,
    s: &Staff,
    action: &str,
    target: &str,
    before: &Value,
    after: &Value,
) -> ApiResult<()> {
    match action {
        "game.status" => undo_status(st, target, before, after).await,
        "detection.edit" => undo_detection_edit(st, target, before, after).await,
        "detection.add" => undo_detection_add(st, target, after).await,
        "detection.delete" => undo_detection_delete(st, before).await,
        "site_setting" => undo_setting(st, s, target, before, after).await,
        "hours.review" => undo_hours_review(st, s, after).await,
        "suggested_merge.dismiss" => undo_suggest(st, s, target).await,
        "merge_suggestion.dismiss" => undo_program(st, target).await,
        _ => Err(bad(
            "not_reversible",
            "That action is not undone from here.",
        )),
    }
}

async fn undo_status(st: &AppState, id: &str, before: &Value, after: &Value) -> ApiResult<()> {
    let cur = crate::game_status::get(st, id)
        .await?
        .unwrap_or(Value::Null);
    if !same_status(&cur, after) {
        return Err(bad(
            "stale",
            "The status has changed since this edit. Nothing was put back.",
        ));
    }
    if before.is_null() {
        sqlx::query("DELETE FROM game_status WHERE game_id = $1")
            .bind(id)
            .execute(&st.db)
            .await?;
        return Ok(());
    }
    let upd = crate::game_status::Update {
        lifecycle: before["lifecycle"].as_str().map(str::to_string),
        date: Some(before["date"].as_str().unwrap_or("").to_string()),
        note: Some(before["note"].as_str().unwrap_or("").to_string()),
        checked_by: before["checked_by"].as_str().map(str::to_string),
    };
    crate::game_status::set(&st.db, id, upd)
        .await
        .map_err(|e| bad("bad_status", &e))?;
    Ok(())
}

fn same_status(cur: &Value, after: &Value) -> bool {
    cur["lifecycle"] == after["lifecycle"]
        && cur["date"] == after["date"]
        && cur["note"] == after["note"]
}

async fn undo_detection_edit(
    st: &AppState,
    target: &str,
    before: &Value,
    after: &Value,
) -> ApiResult<()> {
    let id: i64 = target
        .trim_start_matches("detection:")
        .parse()
        .map_err(|_| bad("bad_target", "That fix-up id is not in the audit row."))?;
    let cur: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT game_id, note FROM detection_aliases WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    let (game, note) = cur.ok_or_else(|| bad("stale", "That fix-up is already gone."))?;
    if game != after["game_id"].as_str().unwrap_or("") || note.as_deref() != after["note"].as_str()
    {
        return Err(bad("stale", "That fix-up has changed since this edit."));
    }
    sqlx::query(
        "UPDATE detection_aliases SET game_id = $2, note = $3, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(before["game_id"].as_str().unwrap_or(""))
    .bind(before["note"].as_str())
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn undo_detection_add(st: &AppState, target: &str, after: &Value) -> ApiResult<()> {
    let id: i64 = target
        .trim_start_matches("detection:")
        .parse()
        .map_err(|_| bad("bad_target", "That fix-up id is not in the audit row."))?;
    let cur: Option<(String, String, String)> =
        sqlx::query_as("SELECT kind, pattern, game_id FROM detection_aliases WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    let (kind, pattern, game) = cur.ok_or_else(|| bad("stale", "That fix-up is already gone."))?;
    if kind != after["kind"].as_str().unwrap_or("")
        || pattern != after["pattern"].as_str().unwrap_or("")
        || game != after["game_id"].as_str().unwrap_or("")
    {
        return Err(bad("stale", "That fix-up has changed since it was added."));
    }
    sqlx::query("DELETE FROM detection_aliases WHERE id = $1")
        .bind(id)
        .execute(&st.db)
        .await?;
    Ok(())
}

async fn undo_detection_delete(st: &AppState, before: &Value) -> ApiResult<()> {
    let kind = before["kind"].as_str().unwrap_or("");
    let pattern = before["pattern"].as_str().unwrap_or("");
    let game = before["game_id"].as_str().unwrap_or("");
    if kind.is_empty() || pattern.is_empty() || game.is_empty() {
        return Err(bad(
            "not_reversible",
            "The audit row does not have the whole fix-up.",
        ));
    }
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM detection_aliases WHERE kind = $1 AND pattern = $2)",
    )
    .bind(kind)
    .bind(pattern)
    .fetch_one(&st.db)
    .await?;
    if exists {
        return Err(bad("stale", "A fix-up for that pattern already exists."));
    }
    sqlx::query(
        "INSERT INTO detection_aliases(kind, pattern, game_id, note) VALUES($1, $2, $3, NULL)",
    )
    .bind(kind)
    .bind(pattern)
    .bind(game)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn undo_setting(
    st: &AppState,
    s: &Staff,
    key: &str,
    before: &Value,
    after: &Value,
) -> ApiResult<()> {
    if before.get("enabled").is_some() {
        let cur = crate::features::on(st, key).await;
        let was = after["enabled"].as_bool().unwrap_or(!cur);
        if cur != was {
            return Err(bad("stale", "That switch has changed since this edit."));
        }
        let back = before["enabled"].as_bool().unwrap_or(false);
        crate::features::set(st, key, back, s.user.id).await?;
        return Ok(());
    }
    if before.get("value").is_some() {
        let cur = if key == crate::features::HOME_COUNTS_KEY {
            crate::features::home_counts(st).await
        } else {
            crate::features::links(st)
                .await
                .get(key)
                .cloned()
                .unwrap_or_default()
        };
        let was = after["value"].as_str().unwrap_or("");
        if cur != was {
            return Err(bad("stale", "That value has changed since this edit."));
        }
        let back = before["value"].as_str().unwrap_or("");
        if key == crate::features::HOME_COUNTS_KEY {
            crate::features::set_home_counts(st, back, s.user.id).await?;
        } else if crate::features::is_link(key) {
            crate::features::set_link(st, key, back, s.user.id).await?;
        } else {
            return Err(bad(
                "not_reversible",
                "That setting is not undone from here.",
            ));
        }
        return Ok(());
    }
    Err(bad(
        "not_reversible",
        "That setting edit did not record a value to put back.",
    ))
}

async fn undo_hours_review(st: &AppState, s: &Staff, after: &Value) -> ApiResult<()> {
    let key = after["flag_key"].as_str().unwrap_or("");
    let evidence = after["evidence"].as_str().unwrap_or("");
    if key.is_empty() {
        return Err(bad(
            "not_reversible",
            "That review did not record which flag it was.",
        ));
    }
    sqlx::query(
        "INSERT INTO hours_flag_reviews(flag_key, kind, evidence, status, reason, actor_id)
         VALUES($1, $2, $3, 'reopened', 'undo', $4)",
    )
    .bind(key)
    .bind(after["kind"].as_str().unwrap_or(""))
    .bind(evidence)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn undo_suggest(st: &AppState, s: &Staff, target: &str) -> ApiResult<()> {
    let key = target.trim_start_matches("suggest:");
    sqlx::query(
        "INSERT INTO suggested_merge_decisions(suggestion_key, evidence, status, reason, decided_by)
         SELECT suggestion_key, evidence, 'reopened', 'undo', $2
         FROM suggested_merge_decisions WHERE suggestion_key = $1
         ORDER BY id DESC LIMIT 1",
    )
    .bind(key)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn undo_program(st: &AppState, target: &str) -> ApiResult<()> {
    let id = target.trim_start_matches("game:");
    sqlx::query(
        "UPDATE merge_suggestions SET status = 'new', decided_by = NULL, decided_at = NULL
         WHERE game_id = $1 AND status = 'dismissed'",
    )
    .bind(id)
    .execute(&st.db)
    .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Data checks. Read only.

async fn data_checks(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    Ok(Json(json!({
        "duplicate_launchers": duplicate_launchers(&st).await?,
        "conflicting_rules": conflicting_rules(&st).await?,
        "hours": hours_problems(&st).await?,
        "note": "Nothing is changed from this page. A duplicate Steam id is also listed under Suggested Merge when a merge would be safe.",
    })))
}

async fn duplicate_launchers(st: &AppState) -> ApiResult<Vec<Value>> {
    let rows: Vec<(i32, i64)> = sqlx::query_as(
        "SELECT steam_appid, count(DISTINCT id) FROM (
            SELECT id, steam_appid FROM games WHERE steam_appid IS NOT NULL
            UNION ALL
            SELECT game_id, pattern::int FROM detection_aliases
             WHERE kind = 'steam' AND pattern ~ '^[0-9]{1,9}$'
         ) x GROUP BY steam_appid HAVING count(DISTINCT id) > 1
         ORDER BY steam_appid LIMIT 100",
    )
    .fetch_all(&st.db)
    .await?;
    let mut out = Vec::new();
    for (app, n) in rows {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM games WHERE steam_appid = $1
             UNION SELECT game_id FROM detection_aliases WHERE kind = 'steam' AND pattern = $2",
        )
        .bind(app)
        .bind(app.to_string())
        .fetch_all(&st.db)
        .await?;
        out.push(json!({
            "launcher": "steam", "product": app.to_string(), "games": n, "ids": ids,
            "fix": "none",
        }));
    }
    Ok(out)
}

async fn conflicting_rules(st: &AppState) -> ApiResult<Vec<Value>> {
    // A pattern can only point at one game (unique index). A conflict is a Steam
    // fix-up that names a different game from the built-in signature for that app id.
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT pattern, game_id FROM detection_aliases WHERE kind = 'steam' AND pattern ~ '^[0-9]{1,9}$'",
    )
    .fetch_all(&st.db)
    .await?;
    let mut out = Vec::new();
    for (pattern, game) in rows {
        let Ok(app) = pattern.parse::<u32>() else {
            continue;
        };
        let Some(sig) = st.sigs.by_steam_appid(app) else {
            continue;
        };
        if sig.id == game {
            continue;
        }
        out.push(json!({
            "kind": "steam", "pattern": pattern, "games": 2,
            "ids": [sig.id, game],
            "detail": "This Steam app id is a built-in game, and a fix-up sends it somewhere else. Nothing was changed.",
        }));
    }
    Ok(out)
}

async fn hours_problems(st: &AppState) -> ApiResult<Value> {
    let bad_len: Vec<(uuid::Uuid, String, i32, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, game_id, duration_s, started_at, ended_at FROM sessions
         WHERE duration_s < 0
            OR duration_s > EXTRACT(EPOCH FROM (ended_at - started_at)) + 1
         ORDER BY started_at DESC LIMIT 50",
    )
    .fetch_all(&st.db)
    .await?;
    let length: Vec<Value> = bad_len
        .into_iter()
        .map(|(id, g, d, a, b)| {
            json!({ "session_id": id, "game_id": g, "duration_s": d, "started_at": a, "ended_at": b,
                    "detail": "The stored length does not match the start and end. Nothing was changed." })
        })
        .collect();
    let users: Vec<uuid::Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT user_id FROM sessions
         WHERE started_at > now() - interval '8 days'
         ORDER BY user_id LIMIT 80",
    )
    .fetch_all(&st.db)
    .await?;
    let mut week_off = Vec::new();
    for id in users {
        let tz_name = week::zone(st, Some(id), None).await;
        let w = week::window(week::parse_tz(&tz_name), Utc::now());
        let data = week::load(st, w.clone(), &[id], Some(id)).await?;
        let secs = data.user(id).seconds();
        let (a, b) = (w.bounds[0], w.bounds[week::DAYS]);
        let sql_secs: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COALESCE(sum({}), 0)::bigint FROM play_sessions s WHERE s.user_id = $1 AND {}",
            week::secs_sql("$2", "$3"),
            week::in_window_sql("$2", "$3")
        )))
        .bind(id)
        .bind(a)
        .bind(b)
        .fetch_one(&st.db)
        .await?;
        if sql_secs != secs {
            week_off.push(json!({
                "user_id": id, "zone": tz_name, "week_seconds": secs, "sql_seconds": sql_secs,
                "detail": "This player's local week does not match the shared SQL total. Hours were not rewritten.",
            }));
        }
    }
    Ok(json!({
        "length_mismatches": length,
        "week_mismatches": week_off,
        "week_note": "Compared with the shared week (seven local days, tracked sessions, the player's saved time zone). The rolling 7-day profile figure is a different window and is not flagged.",
    }))
}

// ---------------------------------------------------------------------------
// Review history: the audit log, not a second copy of it.

#[derive(Deserialize)]
struct HistQ {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    group: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
}

async fn history(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<HistQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let group = q.group.as_deref().unwrap_or("");
    let like = match group {
        "reviewed" => "hours.review%",
        "dismissed" => "%dismiss%",
        "merged" => "game.merge",
        "corrected" => "game.%",
        _ => "%",
    };
    let term = q.q.as_deref().unwrap_or("").trim().to_lowercase();
    let pat = crate::admin::like_pattern(&term);
    let off = q.offset.unwrap_or(0).max(0);
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM admin_audit
         WHERE action LIKE $1
           AND ($2 = '' OR lower(action) LIKE $3 OR lower(COALESCE(target, '')) LIKE $3
                OR lower(actor_handle) LIKE $3)",
    )
    .bind(like)
    .bind(&term)
    .bind(&pat)
    .fetch_one(&st.db)
    .await?;
    let rows: Vec<(
        i64,
        DateTime<Utc>,
        String,
        String,
        Option<String>,
        Option<Value>,
        Option<Value>,
    )> = sqlx::query_as(
        "SELECT id, created_at, actor_handle, action, target, before, after
         FROM admin_audit
         WHERE action LIKE $1
           AND ($2 = '' OR lower(action) LIKE $3 OR lower(COALESCE(target, '')) LIKE $3
                OR lower(actor_handle) LIKE $3)
         ORDER BY created_at DESC, id DESC
         LIMIT 50 OFFSET $4",
    )
    .bind(like)
    .bind(&term)
    .bind(&pat)
    .bind(off)
    .fetch_all(&st.db)
    .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|(id, at, who, action, target, before, after)| {
            json!({
                "id": id, "at": at,
                "by": if who.is_empty() { Value::Null } else { json!(who) },
                "action": action, "target": target, "before": before, "after": after,
            })
        })
        .collect();
    Ok(Json(json!({
        "total": total, "offset": off, "page_size": 50, "rows": out,
        "note": "Taken from the audit log. A blank name means the row did not record one.",
    })))
}
