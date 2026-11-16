//! Catalogue change history for Admin > Games and the public /games page.
//!
//! Every staff or system edit to the games list (name, detection, icons, merges,
//! Steam sync, the daily list refresh) is written here in the same database
//! transaction as the edit. A failed edit therefore writes no history, and the
//! history insert cannot change hours, sessions, uploads or merge behaviour.
//!
//! Public /games "Recent updates" shows only public-safe rows (added, renamed,
//! icon, manual, and sync summaries of those) that have not been hidden.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    admin::{self, Role, Staff},
    error::{ApiResult, AppError},
    game_overrides::GameOverride,
    names::UNLISTED_TITLE,
    AppState,
};

pub const PAGE: i64 = 50;
pub const PUBLIC_UPDATES: i64 = 5;

/// One history row waiting to be inserted on the current transaction.
pub struct New {
    pub actor_kind: &'static str,
    pub actor_id: Option<Uuid>,
    pub actor_label: String,
    pub action: &'static str,
    pub game_id: Option<String>,
    pub game_name: Option<String>,
    pub summary: String,
    pub diff: Option<Value>,
    pub public: bool,
    pub list_updated: Option<String>,
}

impl New {
    pub fn admin(s: &Staff, action: &'static str, summary: impl Into<String>) -> Self {
        Self {
            actor_kind: "admin",
            actor_id: Some(s.user.id),
            actor_label: s.user.handle.clone(),
            action,
            game_id: None,
            game_name: None,
            summary: clip(summary.into(), 300),
            diff: None,
            public: is_public_action(action),
            list_updated: None,
        }
    }

    pub fn system(label: &str, action: &'static str, summary: impl Into<String>) -> Self {
        Self {
            actor_kind: "system",
            actor_id: None,
            actor_label: label.to_string(),
            action,
            game_id: None,
            game_name: None,
            summary: clip(summary.into(), 300),
            diff: None,
            public: is_public_action(action),
            list_updated: None,
        }
    }

    pub fn game(mut self, id: impl Into<String>, name: Option<String>) -> Self {
        self.game_id = Some(id.into());
        self.game_name = name.filter(|n| !n.is_empty());
        self
    }

    pub fn diff(mut self, diff: Value) -> Self {
        self.diff = Some(diff);
        self
    }

    pub fn public(mut self, yes: bool) -> Self {
        self.public = yes;
        self
    }

    pub fn list_updated(mut self, v: impl Into<String>) -> Self {
        self.list_updated = Some(v.into());
        self
    }
}

pub fn is_public_action(action: &str) -> bool {
    matches!(action, "added" | "renamed" | "icon" | "manual")
}

pub fn list_updated(st: &AppState) -> Option<String> {
    st.games
        .current()
        .map(|b| b.updated)
        .filter(|s| !s.is_empty())
}

fn clip(s: String, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s
    } else {
        s.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// Write one history row on the same transaction as the catalogue edit.
pub async fn insert(tx: &mut Transaction<'_, Postgres>, st: &AppState, e: New) -> ApiResult<i64> {
    Ok(insert_sql(tx, st, e).await?)
}

pub async fn insert_sql(
    tx: &mut Transaction<'_, Postgres>,
    st: &AppState,
    e: New,
) -> Result<i64, sqlx::Error> {
    let list = e_list(&e, st);
    sqlx::query_scalar(
        "INSERT INTO game_catalogue_history
            (actor_kind, actor_id, actor_label, action, game_id, game_name, summary, diff, public, list_updated)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id",
    )
    .bind(e.actor_kind)
    .bind(e.actor_id)
    .bind(&e.actor_label)
    .bind(e.action)
    .bind(&e.game_id)
    .bind(&e.game_name)
    .bind(&e.summary)
    .bind(&e.diff)
    .bind(e.public)
    .bind(&list)
    .fetch_one(&mut **tx)
    .await
}

fn e_list(e: &New, st: &AppState) -> Option<String> {
    e.list_updated.clone().or_else(|| list_updated(st))
}

/// Field-level `{ field: { old, new } }` for the values that actually changed.
pub fn fields_changed(pairs: &[(&str, Value, Value)]) -> Option<Value> {
    let mut m = serde_json::Map::new();
    for (k, old, new) in pairs {
        if old != new {
            m.insert(k.to_string(), json!({ "old": old, "new": new }));
        }
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

pub fn ov_name(o: Option<&GameOverride>) -> Value {
    json!(o.and_then(|x| x.name.clone()))
}
pub fn ov_hidden(o: Option<&GameOverride>) -> Value {
    json!(o.map(|x| x.hidden).unwrap_or(false))
}
pub fn ov_exes(o: Option<&GameOverride>) -> Value {
    json!(o.map(|x| x.extra_exes.clone()).unwrap_or_default())
}
pub fn ov_aliases(o: Option<&GameOverride>) -> Value {
    json!(o.map(|x| x.aliases.clone()).unwrap_or_default())
}
pub fn ov_hints(o: Option<&GameOverride>) -> Value {
    json!(o.map(|x| x.path_hints.clone()).unwrap_or_default())
}

/// Pick the action and one-line summary for a staff game-edit (overrides).
pub fn edit_kind(diff: &Value, name: &str) -> (&'static str, String, bool) {
    let keys: Vec<&str> = diff
        .as_object()
        .map(|m| m.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    if keys == ["hidden"] {
        let hid = diff["hidden"]["new"].as_bool().unwrap_or(false);
        return if hid {
            ("hidden", format!("Hid {name}"), false)
        } else {
            ("unhidden", format!("Unhid {name}"), false)
        };
    }
    if keys == ["name"] {
        let old = display(&diff["name"]["old"]);
        let new = display(&diff["name"]["new"]);
        return ("renamed", format!("Renamed {old} to {new}"), true);
    }
    let mut bits: Vec<String> = Vec::new();
    for k in [
        "name",
        "hidden",
        "extra_exes",
        "aliases",
        "path_hints",
        "steam_appid",
        "icon_pref",
        "art_pref",
        "lifecycle",
        "art",
    ] {
        if diff.get(k).is_some() {
            bits.push(
                match k {
                    "name" => "name",
                    "hidden" => "visibility",
                    "extra_exes" => "exe signatures",
                    "aliases" => "aliases",
                    "path_hints" => "folder signatures",
                    "steam_appid" => "Steam app id",
                    "icon_pref" => "icon source",
                    "art_pref" => "art source",
                    "lifecycle" => "status",
                    "art" => "art",
                    _ => k,
                }
                .into(),
            );
        }
    }
    let what = if bits.is_empty() {
        "details".into()
    } else {
        bits.join(", ")
    };
    let public = diff.get("name").is_some();
    let summary = if let Some(n) = diff.get("name") {
        format!(
            "Renamed {} to {} and edited {what}",
            display(&n["old"]),
            display(&n["new"])
        )
    } else {
        format!("Edited {what} on {name}")
    };
    ("edited", summary, public)
}

fn display(v: &Value) -> String {
    match v {
        Value::Null => "(none)".into(),
        Value::String(s) if s.is_empty() => "(none)".into(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) => {
            if a.is_empty() {
                "(none)".into()
            } else {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        }
        other => other.to_string(),
    }
}

pub fn game_label(id: &str, name: Option<&str>) -> String {
    name.filter(|n| !n.is_empty() && *n != UNLISTED_TITLE)
        .map(|n| n.to_string())
        .unwrap_or_else(|| id.to_string())
}

/// One summary row plus per-game added/renamed (public) and other list changes (admin-only).
pub async fn record_list_sync(
    tx: &mut Transaction<'_, Postgres>,
    st: &AppState,
    changes: &[(String, &str, Value, Value)],
    updated: &str,
) -> Result<(), sqlx::Error> {
    let count = |k: &str| changes.iter().filter(|c| c.1 == k).count();
    let added = count("new") + count("back");
    let renamed = count("renamed");
    let exes = count("exes");
    let removed = count("removed");
    let mut bits = Vec::new();
    if added > 0 {
        bits.push(format!(
            "{added} game{} added",
            if added == 1 { "" } else { "s" }
        ));
    }
    if renamed > 0 {
        bits.push(format!("{renamed} renamed"));
    }
    if exes > 0 {
        bits.push(format!(
            "{exes} exe change{}",
            if exes == 1 { "" } else { "s" }
        ));
    }
    if removed > 0 {
        bits.push(format!("{removed} removed"));
    }
    let summary = if bits.is_empty() {
        "Games list refreshed".into()
    } else {
        format!("Games list: {}", bits.join(", "))
    };
    insert_sql(
        tx,
        st,
        New::system("system: games list", "list_sync", summary)
            .diff(json!({
                "added": added, "renamed": renamed, "exes": exes, "removed": removed, "list_updated": updated,
            }))
            .public(added + renamed > 0)
            .list_updated(updated),
    )
    .await?;
    for (id, kind, before, after) in changes {
        let name = after
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| before.get("name").and_then(|v| v.as_str()))
            .unwrap_or(id.as_str())
            .to_string();
        let (action, summary, public) = match *kind {
            "new" | "back" => ("added", format!("Added {name}"), true),
            "renamed" => (
                "renamed",
                format!(
                    "Renamed {} to {}",
                    before.get("name").and_then(|v| v.as_str()).unwrap_or(id),
                    after.get("name").and_then(|v| v.as_str()).unwrap_or(id)
                ),
                true,
            ),
            "exes" => (
                "edited",
                format!("Updated exe signatures for {name}"),
                false,
            ),
            "removed" => (
                "removed",
                format!("Removed {name} from the games list"),
                false,
            ),
            _ => continue,
        };
        insert_sql(
            tx,
            st,
            New::system("system: games list", action, summary)
                .game(id, Some(name))
                .diff(json!({ "kind": kind, "before": before, "after": after }))
                .public(public)
                .list_updated(updated),
        )
        .await?;
    }
    Ok(())
}

// ------------------------------------------------------------------ routes

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/games/catalogue", get(catalogue))
        .route("/api/v1/games/updates", get(public_updates))
}

pub fn admin_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/admin/games-history",
            get(admin_list).post(manual_add),
        )
        .route(
            "/api/v1/admin/games-history/{id}/visibility",
            post(set_visibility),
        )
}

#[derive(Deserialize, Default)]
struct ListQ {
    game: Option<String>,
    #[serde(rename = "type")]
    typ: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}

fn page(q: &ListQ, default: i64, max: i64) -> (i64, i64) {
    let limit = q.limit.unwrap_or(default).clamp(1, max);
    let offset = q.offset.unwrap_or(0).max(0);
    (offset, limit)
}

type HistRow = (
    i64,
    DateTime<Utc>,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<Value>,
    bool,
    bool,
    Option<String>,
);

fn to_json(r: HistRow) -> Value {
    json!({
        "id": r.0,
        "at": r.1,
        "actor_kind": r.2,
        "by": r.3,
        "action": r.4,
        "game_id": r.5,
        "game_name": r.6,
        "summary": r.7,
        "diff": r.8,
        "public": r.9,
        "hidden_from_public": r.10,
        "list_updated": r.11,
    })
}

async fn admin_list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (offset, limit) = page(&q, PAGE, 200);
    let game = q.game.as_deref().map(str::trim).filter(|g| !g.is_empty());
    let typ = q
        .typ
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty() && *t != "all");
    let total: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_catalogue_history
         WHERE ($1::text IS NULL OR game_id = $1)
           AND ($2::text IS NULL OR action = $2)",
    )
    .bind(game)
    .bind(typ)
    .fetch_one(&st.db)
    .await?;
    let rows: Vec<HistRow> = sqlx::query_as(
        "SELECT id, created_at, actor_kind, actor_label, action, game_id, game_name,
                summary, diff, public, hidden_from_public, list_updated
         FROM game_catalogue_history
         WHERE ($1::text IS NULL OR game_id = $1)
           AND ($2::text IS NULL OR action = $2)
         ORDER BY created_at DESC, id DESC
         LIMIT $3 OFFSET $4",
    )
    .bind(game)
    .bind(typ)
    .bind(limit)
    .bind(offset)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({
        "entries": rows.into_iter().map(to_json).collect::<Vec<_>>(),
        "total": total,
        "offset": offset,
        "limit": limit,
        "list_updated": list_updated(&st),
    })))
}

#[derive(Deserialize)]
struct ManualBody {
    summary: String,
    game_id: Option<String>,
    game_name: Option<String>,
    /// Default true: manual notes show on /games unless later hidden.
    #[serde(default = "default_true")]
    public: bool,
}

fn default_true() -> bool {
    true
}

async fn manual_add(
    State(st): State<AppState>,
    s: Staff,
    Json(b): Json<ManualBody>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let summary = admin::clean(&b.summary, 300);
    if summary.is_empty() {
        return Err(admin::bad("empty", "Write a short summary."));
    }
    let game_id = b
        .game_id
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(|g| g.to_string());
    if let Some(id) = &game_id {
        if id.len() > 80 || id.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(admin::bad("bad_game", "That isn't a game id."));
        }
    }
    let name = b
        .game_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(|n| n.chars().take(80).collect::<String>());
    let mut entry = New::admin(&s, "manual", summary).public(b.public);
    if let Some(id) = game_id {
        entry = entry.game(id, name);
    }
    let mut tx = st.db.begin().await?;
    let id = insert(&mut tx, &st, entry).await?;
    tx.commit().await?;
    Ok(Json(json!({ "id": id, "ok": true })))
}

#[derive(Deserialize)]
struct VisBody {
    hidden: bool,
}

async fn set_visibility(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(b): Json<VisBody>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let n = sqlx::query("UPDATE game_catalogue_history SET hidden_from_public = $2 WHERE id = $1")
        .bind(id)
        .bind(b.hidden)
        .execute(&st.db)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(Json(
        json!({ "ok": true, "id": id, "hidden_from_public": b.hidden }),
    ))
}

async fn public_updates(
    State(st): State<AppState>,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    let (_, limit) = page(&q, PUBLIC_UPDATES, 20);
    let rows: Vec<HistRow> = sqlx::query_as(
        "SELECT id, created_at, actor_kind, actor_label, action, game_id, game_name,
                summary, diff, public, hidden_from_public, list_updated
         FROM game_catalogue_history
         WHERE public AND NOT hidden_from_public
         ORDER BY created_at DESC, id DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&st.db)
    .await?;
    // Public payload: no actor handles, no hidden flags, no staff-only diffs for detection etc.
    // Manual and added/renamed/icon diffs are names and icons only.
    let entries: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            json!({
                "id": r.0,
                "at": r.1,
                "action": r.4,
                "game_id": r.5,
                "game_name": r.6,
                "summary": r.7,
                "list_updated": r.11,
            })
        })
        .collect();
    Ok(Json(
        json!({ "updates": entries, "list_updated": list_updated(&st) }),
    ))
}

#[derive(Deserialize, Default)]
struct CatQ {
    q: Option<String>,
}

async fn catalogue(
    State(st): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<CatQ>,
) -> ApiResult<Response> {
    let term = q.q.as_deref().unwrap_or("").trim().to_lowercase();
    let mut games = public_catalogue(&st).await?;
    if !term.is_empty() {
        games.retain(|(id, name)| {
            name.to_lowercase().contains(&term) || id.to_lowercase().contains(&term)
        });
    }
    let total = games.len() as i64;
    // Compact [id, name] pairs only — no icon URLs, hours or user fields.
    let payload = json!({
        "games": games.iter().map(|(id, name)| json!([id, name])).collect::<Vec<_>>(),
        "total": total,
    });
    if !term.is_empty() {
        return Ok(Json(payload).into_response());
    }
    let body = serde_json::to_vec(&payload).map_err(|e| {
        AppError::new(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "encode",
            e.to_string(),
        )
    })?;
    let etag = crate::games::etag_of(&body);
    let gz = crate::games::gzip(&body).ok();
    Ok(crate::games::respond(&headers, &etag, &body, gz.as_deref()))
}

/// Every supported/detected game: icon + name only, no hours or player data.
/// Hidden and unlisted entries are left out. Alphabetical by name, then id.
pub async fn public_catalogue(st: &AppState) -> ApiResult<Vec<(String, String)>> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, String> = BTreeMap::new();

    if let Some(db) = crate::games::detectable_db(st) {
        for g in db.games() {
            if !g.slug.is_empty() && !g.name.is_empty() {
                map.insert(g.slug.clone(), g.name.clone());
            }
        }
    }
    for g in crate::steam::sigs().games() {
        if !g.id.is_empty() && !g.name.is_empty() {
            map.entry(g.id.clone()).or_insert_with(|| g.name.clone());
        }
    }
    for (id, name) in st.games.override_names(&st.cfg.data_dir).iter() {
        if !id.is_empty() && !name.is_empty() {
            map.entry(id.clone()).or_insert_with(|| name.clone());
        }
    }

    let rows: Vec<(String, String, bool, Option<String>, Option<bool>)> = sqlx::query_as(
        "SELECT g.id, g.name, g.verified, o.name, o.hidden
         FROM games g
         LEFT JOIN game_overrides o ON o.game_id = g.id",
    )
    .fetch_all(&st.db)
    .await?;

    let hidden: std::collections::HashSet<String> = rows
        .iter()
        .filter(|r| r.4.unwrap_or(false))
        .map(|r| r.0.clone())
        .collect();

    for (id, name, verified, ov_name, hid) in &rows {
        if hid.unwrap_or(false) {
            map.remove(id);
            continue;
        }
        let shown = ov_name.as_deref().filter(|n| !n.is_empty()).unwrap_or(name);
        if !*verified && !map.contains_key(id) {
            continue;
        }
        if shown == UNLISTED_TITLE {
            map.remove(id);
            continue;
        }
        if map.contains_key(id) || *verified {
            map.insert(id.clone(), shown.to_string());
        }
    }
    for id in &hidden {
        map.remove(id);
    }

    let mut out: Vec<(String, String)> = map.into_iter().collect();
    out.sort_by(|a, b| {
        a.1.to_lowercase()
            .cmp(&b.1.to_lowercase())
            .then_with(|| a.0.cmp(&b.0))
    });
    Ok(out)
}
