//! Admin sections added with the sidebar console (Nov 2026): Changelog editor, user delete,
//! Hours checker and Health. Same rules as `admin`: staff only (bearer token), the role is
//! checked here, and every change goes into `admin_audit`.
//!
//! - `GET /api/v1/changelog` (public): published changelog entries for `/changelog`.
//! - `GET|POST /api/v1/admin/changelog`, `POST .../{id}/publish|delete` (admin).
//! - `POST /api/v1/admin/users/{id}/delete {confirm}` (admin; never staff or yourself).
//! - `GET /api/v1/admin/users/{id}/hours_check` (staff): the week's headline, daily bars
//!   and per-game totals side by side, with the sessions behind them and any problems.
//! - `POST /api/v1/admin/users/{id}/sessions/{sid}/remove {reason}` and
//!   `POST /api/v1/admin/users/{id}/open_session/close {reason, keep}` (staff).
//! - `GET /api/v1/admin/health` (admin): version, deploy, backups, jobs, errors, email.

use crate::admin::{audit, bad, clean, may_edit_hours, outranks, target, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::{week, AppState};
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/changelog", get(changelog_public))
        .route(
            "/api/v1/admin/changelog",
            get(changelog_list).post(changelog_save),
        )
        .route(
            "/api/v1/admin/changelog/{id}/publish",
            post(changelog_publish),
        )
        .route(
            "/api/v1/admin/changelog/{id}/delete",
            post(changelog_delete),
        )
        .route("/api/v1/admin/users/{id}/delete", post(user_delete))
        .route("/api/v1/admin/users/{id}/hours_check", get(hours_check))
        .route(
            "/api/v1/admin/users/{id}/sessions/{sid}/remove",
            post(session_remove),
        )
        .route(
            "/api/v1/admin/users/{id}/open_session/close",
            post(open_session_close),
        )
        .route("/api/v1/admin/health", get(health))
        .route("/api/v1/admin/bulk/preview", post(bulk_preview))
}

// ---------------------------------------------------------------------------
// Changelog

const PAGE: i64 = 50;
const MAX_ITEMS: usize = 30;
const MAX_ITEM_LEN: usize = 240;

type EntryRow = (
    i64,
    String,
    NaiveDate,
    Option<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Option<DateTime<Utc>>,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
);

const ENTRY_SQL: &str =
    "SELECT c.id, c.area, c.entry_date, c.version, c.new_items, c.changes, c.fixes,
        c.published_at, c.updated_at, u.handle, c.content_format
 FROM changelog_entries c LEFT JOIN users u ON u.id = c.updated_by";

fn entry_json(r: &EntryRow) -> Value {
    json!({
        "id": r.0, "area": r.1, "date": r.2.to_string(), "label": r.2.format("%-d %B %Y").to_string(),
        "version": r.3, "new": r.4, "changes": r.5, "fixes": r.6,
        "published_at": r.7, "updated_at": r.8, "updated_by": r.9, "format": r.10,
        "new_html": crate::content::section(&r.4, r.10.as_deref()),
        "changes_html": crate::content::section(&r.5, r.10.as_deref()),
        "fixes_html": crate::content::section(&r.6, r.10.as_deref()),
    })
}

/// `GET /api/v1/changelog`: published entries, newest first (the page pages them itself).
async fn changelog_public(State(st): State<AppState>) -> ApiResult<impl IntoResponse> {
    let rows: Vec<EntryRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ENTRY_SQL} WHERE c.published_at IS NOT NULL ORDER BY c.entry_date DESC, c.id DESC LIMIT 1000"
    )))
    .fetch_all(&st.db)
    .await?;
    let entries: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({ "area": r.1, "date": r.2.to_string(), "label": r.2.format("%-d %B %Y").to_string(),
                    "version": r.3, "new": r.4, "changes": r.5, "fixes": r.6, "format": r.10,
                    "new_html": crate::content::section(&r.4, r.10.as_deref()),
                    "changes_html": crate::content::section(&r.5, r.10.as_deref()),
                    "fixes_html": crate::content::section(&r.6, r.10.as_deref()) })
        })
        .collect();
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=60")],
        Json(json!({ "entries": entries })),
    ))
}

#[derive(Deserialize)]
struct ListQ {
    #[serde(default)]
    offset: Option<i64>,
    #[serde(default)]
    q: Option<String>,
}

/// `GET /api/v1/admin/changelog?offset&q`: drafts and published, recently edited first.
async fn changelog_list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let off = q.offset.unwrap_or(0).max(0);
    let term = q.q.as_deref().map(|t| clean(t, 80)).unwrap_or_default();
    let pat = crate::admin::like_pattern(&term);
    let filter = "($1 = '' OR c.version ILIKE $2 OR array_to_string(c.new_items || c.changes || c.fixes, ' ') ILIKE $2)";
    let total: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM changelog_entries c WHERE {filter}"
    )))
    .bind(&term)
    .bind(&pat)
    .fetch_one(&st.db)
    .await?;
    let rows: Vec<EntryRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ENTRY_SQL} WHERE {filter} ORDER BY (c.published_at IS NULL) DESC, c.entry_date DESC, c.id DESC LIMIT {PAGE} OFFSET $3"
    )))
    .bind(&term)
    .bind(&pat)
    .bind(off)
    .fetch_all(&st.db)
    .await?;
    let drafts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM changelog_entries WHERE published_at IS NULL")
            .fetch_one(&st.db)
            .await?;
    Ok(Json(json!({
        "total": total, "drafts": drafts, "offset": off, "page_size": PAGE,
        "rows": rows.iter().map(entry_json).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct EntryReq {
    format: Option<String>,
    id: Option<i64>,
    area: String,
    date: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    new: Vec<String>,
    #[serde(default)]
    changes: Vec<String>,
    #[serde(default)]
    fixes: Vec<String>,
}

/// Words that must never reach a public page.
fn banned_words(s: &str) -> bool {
    let l = s.to_lowercase();
    ["xfire", "xf1re"].iter().any(|w| l.contains(w))
}

fn items(v: &[String], what: &str, markdown: bool) -> ApiResult<Vec<String>> {
    let out: Vec<String> = if markdown {
        crate::content::source(&v.join("\n\n"))?;
        v.iter()
            .map(|s| crate::content::source(s))
            .collect::<ApiResult<Vec<_>>>()?
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        v.iter()
            .map(|i| clean(i.trim_start_matches(['-', '*', '•', ' ']), MAX_ITEM_LEN))
            .filter(|i| !i.is_empty())
            .collect()
    };
    if out.len() > MAX_ITEMS {
        return Err(bad(
            "too_many",
            &format!("{what}: at most {MAX_ITEMS} bullets per entry."),
        ));
    }
    if out.iter().any(|i| banned_words(i)) {
        return Err(bad(
            "banned_word",
            "That wording can't go on the public changelog.",
        ));
    }
    Ok(out)
}

/// `POST /api/v1/admin/changelog`: create (draft) or edit an entry.
async fn changelog_save(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<EntryReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if r.area != "App" && r.area != "Website" {
        return Err(bad("bad_area", "Pick App or Website."));
    }
    let date = NaiveDate::parse_from_str(r.date.trim(), "%Y-%m-%d")
        .map_err(|_| bad("bad_date", "Use a date like 2026-11-05."))?;
    let version = r
        .version
        .as_deref()
        .map(|v| clean(v, 40))
        .filter(|v| !v.is_empty());
    if version.as_deref().is_some_and(banned_words) {
        return Err(bad(
            "banned_word",
            "That wording can't go on the public changelog.",
        ));
    }
    crate::content::format_ok(r.format.as_deref())?;
    let md = r.format.as_deref() == Some("markdown");
    let (n, c, f) = (
        items(&r.new, "New", md)?,
        items(&r.changes, "Changes", md)?,
        items(&r.fixes, "Fixes", md)?,
    );
    if n.is_empty() && c.is_empty() && f.is_empty() {
        return Err(bad("empty", "Add at least one bullet."));
    }
    let after = json!({ "area": r.area, "date": date.to_string(), "version": version, "new": n, "changes": c, "fixes": f, "format": r.format });
    let id = if let Some(id) = r.id {
        let before = one(&st, id).await?;
        sqlx::query(
            "UPDATE changelog_entries SET area = $2, entry_date = $3, version = $4, new_items = $5,
               changes = $6, fixes = $7, updated_at = now(), updated_by = $8, content_format = $9 WHERE id = $1",
        )
        .bind(id)
        .bind(&r.area)
        .bind(date)
        .bind(&version)
        .bind(&n)
        .bind(&c)
        .bind(&f)
        .bind(s.user.id)
        .bind(&r.format)
        .execute(&st.db)
        .await?;
        audit(
            &st,
            &s,
            "changelog.edit",
            &format!("changelog:{id}"),
            before,
            after,
        )
        .await?;
        id
    } else {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO changelog_entries(area, entry_date, version, new_items, changes, fixes, created_by, updated_by, content_format)
             VALUES($1, $2, $3, $4, $5, $6, $7, $7, $8) RETURNING id",
        )
        .bind(&r.area)
        .bind(date)
        .bind(&version)
        .bind(&n)
        .bind(&c)
        .bind(&f)
        .bind(s.user.id)
        .bind(&r.format)
        .fetch_one(&st.db)
        .await?;
        audit(
            &st,
            &s,
            "changelog.create",
            &format!("changelog:{id}"),
            Value::Null,
            after,
        )
        .await?;
        id
    };
    Ok(Json(json!({ "ok": true, "entry": one(&st, id).await? })))
}

async fn one(st: &AppState, id: i64) -> ApiResult<Value> {
    let r: Option<EntryRow> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!("{ENTRY_SQL} WHERE c.id = $1")))
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
    r.as_ref().map(entry_json).ok_or_else(AppError::not_found)
}

#[derive(Deserialize)]
struct PublishReq {
    publish: bool,
}

async fn changelog_publish(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(r): Json<PublishReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let before = one(&st, id).await?;
    sqlx::query(
        "UPDATE changelog_entries SET published_at = CASE WHEN $2 THEN COALESCE(published_at, now()) END,
           updated_at = now(), updated_by = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(r.publish)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    let after = one(&st, id).await?;
    let action = if r.publish {
        "changelog.publish"
    } else {
        "changelog.unpublish"
    };
    audit(
        &st,
        &s,
        action,
        &format!("changelog:{id}"),
        json!({ "published_at": before["published_at"] }),
        json!({ "published_at": after["published_at"] }),
    )
    .await?;
    Ok(Json(json!({ "ok": true, "entry": after })))
}

#[derive(Deserialize)]
struct ConfirmWord {
    confirm: String,
}

async fn changelog_delete(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(r): Json<ConfirmWord>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if !r.confirm.trim().eq_ignore_ascii_case("delete") {
        return Err(bad("confirm", "Type delete to confirm."));
    }
    let before = one(&st, id).await?;
    sqlx::query("DELETE FROM changelog_entries WHERE id = $1")
        .bind(id)
        .execute(&st.db)
        .await?;
    audit(
        &st,
        &s,
        "changelog.delete",
        &format!("changelog:{id}"),
        before,
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Users: delete

#[derive(Deserialize)]
struct DeleteReq {
    confirm: String,
}

/// `POST /api/v1/admin/users/{id}/delete {confirm: handle}`: removes the account and
/// everything that belongs to it for good. Admins only; never staff, never yourself.
async fn user_delete(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
    Json(r): Json<DeleteReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (handle, role, banned) = target(&st, id).await?;
    outranks(&s, id, role)?;
    if role.is_some() {
        return Err(bad(
            "is_staff",
            "Remove their staff role first. Staff accounts can't be deleted here.",
        ));
    }
    if !r.confirm.trim().eq_ignore_ascii_case(&handle) {
        return Err(bad("confirm", "Type the exact handle to confirm."));
    }
    let info: (String, DateTime<Utc>, bool) =
        sqlx::query_as("SELECT display_name, created_at, is_test FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(&st.db)
            .await?;
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(id)
        .execute(&st.db)
        .await?;
    st.hub.kick(id);
    crate::avatars::remove_files(&st, id);
    audit(
        &st,
        &s,
        "user.delete",
        &format!("user:{handle}"),
        json!({ "id": id, "handle": handle, "display_name": info.0, "created_at": info.1, "is_test": info.2, "banned": banned }),
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Hours checker

/// A session longer than this in one row is flagged (rows are capped at 24 h).
const LONG_SECS: i64 = 18 * 3600;
/// presence counts an open session only while its heartbeat is this fresh (play_sessions).
const LIVE_SECS: i64 = 150;

type SessRow = (
    Uuid,
    String,
    DateTime<Utc>,
    DateTime<Utc>,
    i32,
    String,
    DateTime<Utc>,
);

/// `GET /api/v1/admin/users/{id}/hours_check`: the person's week in their own time zone,
/// shown three ways (headline, daily bars, per game) plus a database cross-check and the
/// rolling 7-day profile figure, with the sessions behind them and anything that looks off.
async fn hours_check(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let (handle, role, _) = target(&st, id).await?;
    let tz_name = week::zone(&st, Some(id), None).await;
    let tz = week::parse_tz(&tz_name);
    let now = Utc::now();
    let w = week::window(tz, now);
    let data = week::load(&st, w.clone(), &[id], Some(id)).await?;
    let mine = data.user(id);
    let secs = mine.seconds();
    let head_t = week::tenths(secs);
    let day_s = mine.days();
    let day_t = week::apportion(head_t, &day_s);
    let shown = mine.shown(|_| true);
    let gids: Vec<String> = shown.games.iter().map(|g| g.game_id.clone()).collect();
    let names = crate::names::public_names(&st, &gids).await?;
    let name_of = |g: &str| {
        names
            .get(g)
            .cloned()
            .unwrap_or_else(|| st.games.detectable_name(g).unwrap_or_else(|| g.to_string()))
    };
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
    let rolling_min: i64 = sqlx::query_scalar(
        "SELECT COALESCE(sum(minutes_7d), 0)::bigint FROM user_game_stats WHERE user_id = $1",
    )
    .bind(id)
    .fetch_one(&st.db)
    .await?;

    let days: Vec<Value> = (0..week::DAYS)
        .map(|i| {
            json!({ "date": w.dates[i].to_string(), "label": w.dates[i].format("%a %-d %b").to_string(),
                    "seconds": day_s[i], "tenths": day_t[i] })
        })
        .collect();
    let games: Vec<Value> = shown
        .games
        .iter()
        .map(|g| json!({ "game_id": g.game_id, "name": name_of(&g.game_id), "seconds": g.seconds, "tenths": g.tenths }))
        .collect();
    let day_sum_s: i64 = day_s.iter().sum();
    let day_sum_t: i64 = day_t.iter().sum();
    let game_sum_s: i64 = shown.games.iter().map(|g| g.seconds).sum();
    let game_sum_t: i64 = shown.games.iter().map(|g| g.tenths).sum();

    // the sessions behind the week (stored rows; the open one is listed separately)
    let rows: Vec<SessRow> = sqlx::query_as(
        "SELECT id, game_id, started_at, ended_at, duration_s, source, uploaded_at FROM sessions
         WHERE user_id = $1 AND ended_at > to_timestamp($2::bigint) AND started_at < to_timestamp($3::bigint) + interval '1 day'
         ORDER BY started_at LIMIT 1000",
    )
    .bind(id)
    .bind(a)
    .bind(b)
    .fetch_all(&st.db)
    .await?;
    let mut sessions = Vec::with_capacity(rows.len());
    let mut problems = 0usize;
    let mut last_end: Option<DateTime<Utc>> = None;
    for r in &rows {
        let mut flags: Vec<&str> = Vec::new();
        if i64::from(r.4) > LONG_SECS {
            flags.push("long");
        }
        if r.3 > now + chrono::Duration::minutes(5) {
            flags.push("future");
        }
        if last_end.is_some_and(|e| r.2 < e) {
            flags.push("overlap");
        }
        if i64::from(r.4) > (r.3 - r.2).num_seconds() + 1 {
            flags.push("duration");
        }
        last_end = Some(last_end.map_or(r.3, |e| e.max(r.3)));
        problems += flags.len();
        sessions.push(json!({
            "id": r.0, "game_id": r.1, "name": name_of(&r.1), "started_at": r.2, "ended_at": r.3,
            "duration_s": r.4, "source": r.5, "uploaded_at": r.6, "flags": flags,
        }));
    }
    let open: Option<(String, DateTime<Utc>, DateTime<Utc>, String, bool)> = sqlx::query_as(
        "SELECT game_id, session_started_at, heartbeat_at, status, game_hidden FROM presence
         WHERE user_id = $1 AND session_started_at IS NOT NULL AND game_id IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let open_json = open.map(|(g, start, hb, status, hidden)| {
        let stale = (now - hb).num_seconds() > LIVE_SECS || status == "offline";
        let end = now.min(hb + chrono::Duration::seconds(crate::presence::HEARTBEAT_GRACE_SECS));
        let stuck = stale;
        if stuck {
            problems += 1;
        }
        json!({
            "game_id": g, "name": name_of(&g), "started_at": start, "heartbeat_at": hb, "status": status,
            "hidden": hidden, "would_end_at": end, "seconds": (end - start).num_seconds().max(0),
            "stuck": stuck, "counting": !stuck && !hidden,
        })
    });

    let check = |key: &str, label: &str, a: i64, b: i64, unit: &str| json!({ "key": key, "label": label, "a": a, "b": b, "unit": unit, "ok": a == b });
    let checks = vec![
        check(
            "days_tenths",
            "Daily bars add up to the headline",
            day_sum_t,
            head_t,
            "tenths",
        ),
        check(
            "days_seconds",
            "Daily seconds add up to the week",
            day_sum_s,
            secs,
            "seconds",
        ),
        check(
            "games_tenths",
            "Per-game totals add up to the headline",
            game_sum_t,
            head_t,
            "tenths",
        ),
        check(
            "games_seconds",
            "Per-game seconds add up to the week",
            game_sum_s,
            secs,
            "seconds",
        ),
        check(
            "sql_seconds",
            "Database cross-check (leaderboards)",
            sql_secs,
            secs,
            "seconds",
        ),
    ];
    let mismatches = checks.iter().filter(|c| c["ok"] == false).count();
    Ok(Json(json!({
        "user": { "id": id, "handle": handle },
        "may_edit": may_edit_hours(&s, id, role).is_ok(),
        "zone": tz_name,
        "from": w.dates[0].to_string(), "to": w.dates[week::DAYS - 1].to_string(),
        "headline": { "seconds": secs, "tenths": head_t },
        "days": days,
        "games": games,
        "sql_seconds": sql_secs,
        "rolling_7d_minutes": rolling_min,
        "checks": checks,
        "mismatches": mismatches,
        "problems": problems,
        "sessions": sessions,
        "open_session": open_json,
    })))
}

#[derive(Deserialize)]
struct ReasonReq {
    reason: String,
    #[serde(default)]
    keep: bool,
}

fn reason(r: &str) -> ApiResult<String> {
    let r = clean(r, 300);
    if r.chars().count() < 3 {
        return Err(bad("reason", "Say why (a few words)."));
    }
    Ok(r)
}

/// `POST /api/v1/admin/users/{id}/sessions/{sid}/remove {reason}`: delete one bad session.
async fn session_remove(
    State(st): State<AppState>,
    s: Staff,
    Path((id, sid)): Path<(Uuid, Uuid)>,
    Json(r): Json<ReasonReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let why = reason(&r.reason)?;
    let (handle, role, _) = target(&st, id).await?;
    may_edit_hours(&s, id, role)?;
    let row: Option<(String, DateTime<Utc>, DateTime<Utc>, i32, String)> = sqlx::query_as(
        "DELETE FROM sessions WHERE id = $1 AND user_id = $2
         RETURNING game_id, started_at, ended_at, duration_s, source",
    )
    .bind(sid)
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (g, a, b, d, src) = row.ok_or_else(AppError::not_found)?;
    audit(
        &st,
        &s,
        "hours.delete_session",
        &format!("user:{handle} session:{sid}"),
        json!({ "game_id": g, "started_at": a, "ended_at": b, "duration_s": d, "source": src }),
        json!({ "reason": why, "via": "hours_check" }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// `POST /api/v1/admin/users/{id}/open_session/close {reason, keep}`: end a session that
/// stayed open. `keep` stores it up to the last heartbeat (+45 s, as presence would);
/// otherwise it is dropped.
async fn open_session_close(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<Uuid>,
    Json(r): Json<ReasonReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let why = reason(&r.reason)?;
    let (handle, role, _) = target(&st, id).await?;
    may_edit_hours(&s, id, role)?;
    let open: Option<(String, DateTime<Utc>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT game_id, session_started_at, heartbeat_at FROM presence
         WHERE user_id = $1 AND session_started_at IS NOT NULL AND game_id IS NOT NULL",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let (g, start, hb) = open.ok_or_else(|| bad("no_open_session", "No open session."))?;
    let end = Utc::now().min(hb + chrono::Duration::seconds(crate::presence::HEARTBEAT_GRACE_SECS));
    if r.keep && end > start {
        crate::presence::record_live_session(&st, id, &g, start, end).await?;
    } else {
        sqlx::query(
            "UPDATE presence SET session_started_at = NULL WHERE user_id = $1 AND session_started_at = $2",
        )
        .bind(id)
        .bind(start)
        .execute(&st.db)
        .await?;
    }
    audit(
        &st,
        &s,
        "hours.close_session",
        &format!("user:{handle}"),
        json!({ "game_id": g, "started_at": start, "heartbeat_at": hb }),
        json!({ "kept": r.keep, "ended_at": r.keep.then_some(end), "reason": why }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Health

const BACKUP_DIR: &str = "/var/backups/fraglink";

fn mtime(p: &std::path::Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

/// Newest database dump in the backup folder, plus how many dumps are there.
fn backups(dir: &str) -> Value {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            let why = match e.kind() {
                std::io::ErrorKind::NotFound => "folder not found".to_string(),
                std::io::ErrorKind::PermissionDenied => {
                    "no permission to read the folder".to_string()
                }
                k => k.to_string(),
            };
            return json!({ "dir": dir, "readable": false, "error": why });
        }
    };
    let mut dumps: Vec<(DateTime<Utc>, String, u64)> = rd
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".dump") {
                return None;
            }
            let m = e.metadata().ok()?;
            Some((DateTime::<Utc>::from(m.modified().ok()?), name, m.len()))
        })
        .collect();
    dumps.sort_by_key(|a| std::cmp::Reverse(a.0));
    let newest = dumps
        .first()
        .map(|(t, n, b)| json!({ "name": n, "modified": t, "bytes": b }));
    json!({ "dir": dir, "readable": true, "count": dumps.len(), "newest": newest })
}

/// `GET /api/v1/admin/health`: one page to see whether the server is well.
async fn health(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let exe = std::env::current_exe().ok();
    let binary_at = exe.as_deref().and_then(mtime);
    let site_at = mtime(&std::path::Path::new(&st.cfg.site_dir).join("index.html"));
    let jobs: Vec<(String, DateTime<Utc>, Option<DateTime<Utc>>, Option<String>)> =
        sqlx::query_as("SELECT job, last_run_at, last_ok_at, detail FROM job_runs ORDER BY job")
            .fetch_all(&st.db)
            .await?;
    let now = Utc::now();
    let jobs_json: Vec<Value> = jobs
        .iter()
        .map(|(j, run, ok, d)| {
            let failing = ok.is_none_or(|o| o < *run);
            // Only the daily games-list check is expected on a schedule. An old Steam
            // import is not "late": it runs when someone imports, and a missing row
            // means we have not recorded one. Never invent a healthy job.
            let scheduled = j == "games_list";
            json!({ "job": j, "last_run_at": run, "last_ok_at": ok, "detail": d,
                    "failing": failing, "scheduled": scheduled,
                    "stale": scheduled && (now - *run).num_hours() > 48 })
        })
        .collect();
    let migration: Option<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT version, description, installed_on FROM _sqlx_migrations ORDER BY version DESC LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await
    .unwrap_or(None);
    let sent_24h: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM alert_log WHERE ok AND sent_at > now() - interval '24 hours'",
    )
    .fetch_one(&st.db)
    .await?;
    let failed_7d: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM alert_log WHERE NOT ok AND sent_at > now() - interval '7 days'",
    )
    .fetch_one(&st.db)
    .await?;
    let last_fail: Option<(String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT kind, error, sent_at FROM alert_log WHERE NOT ok ORDER BY sent_at DESC LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    let last_ok: Option<DateTime<Utc>> =
        sqlx::query_scalar("SELECT max(sent_at) FROM alert_log WHERE ok")
            .fetch_one(&st.db)
            .await?;
    let errors = crate::admin::recent_errors().await;
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "started_at": crate::admin::STARTED.get().map(|t| now - chrono::Duration::from_std(t.elapsed()).unwrap_or_default()),
        "deploy": { "binary_at": binary_at, "site_at": site_at },
        "migration": migration.map(|(v, d, at)| json!({ "version": v, "description": d, "installed_on": at })),
        "backups": backups(BACKUP_DIR),
        "jobs": jobs_json,
        "errors": errors,
        "email": {
            "live": crate::mailer::live(&st),
            "enabled": st.cfg.mail_enabled,
            "provider": crate::mailer::provider_name(),
            "alerts_sent_24h": sent_24h,
            "alerts_failed_7d": failed_7d,
            "last_ok_at": last_ok,
            "last_failure": last_fail.map(|(k, e, at)| json!({ "kind": k, "error": e.map(|e| clean(&e, 200)), "at": at })),
        },
    })))
}

// ---------------------------------------------------------------------------
// Bulk status: read-only preview. Nothing is written here. Applying still goes
// through the single-game status endpoint, one audited change at a time.

const BULK_MAX: usize = 100;

#[derive(Deserialize)]
struct BulkPreviewReq {
    ids: Vec<String>,
    lifecycle: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    checked: bool,
}

fn bulk_id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 120 && !id.chars().any(char::is_control)
}

/// `POST /api/v1/admin/bulk/preview`: what a status change would do, without doing it.
async fn bulk_preview(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<BulkPreviewReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let life = crate::game_status::parse_lifecycle(&r.lifecycle).ok_or_else(|| {
        bad(
            "bad_status",
            "Status must be live, beta, test, early access or ended.",
        )
    })?;
    let mut ids: Vec<String> = Vec::new();
    for id in r.ids.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if !bulk_id_ok(id) {
            return Err(bad("bad_id", "One of those game ids isn't valid."));
        }
        if !ids.iter().any(|e| e == id) {
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        return Err(bad("empty", "Add at least one game id."));
    }
    if ids.len() > BULK_MAX {
        return Err(bad(
            "too_many",
            &format!("Preview at most {BULK_MAX} games at a time."),
        ));
    }
    let known: Vec<(String, String)> =
        sqlx::query_as("SELECT id, name FROM games WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(&st.db)
            .await?;
    let status: Vec<(String, String, Option<String>, Option<chrono::NaiveDate>)> = sqlx::query_as(
        "SELECT game_id, lifecycle, note, key_date FROM game_status WHERE game_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&st.db)
    .await?;
    let note = clean(&r.note, 500);
    let date = {
        let d = r.date.trim();
        if d.is_empty() {
            None
        } else {
            Some(
                NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .map_err(|_| bad("bad_date", "Use a date like 2026-11-08, or leave it empty."))?
                    .to_string(),
            )
        }
    };
    let rows: Vec<Value> = ids
        .iter()
        .map(|id| {
            let db = known.iter().find(|k| &k.0 == id);
            // Lists first (built-in, folder rules, downloaded list). A database
            // name covers Steam and approved programs that are not on those lists.
            let name = crate::names::list_name(&st, id).or_else(|| db.map(|r| r.1.clone()));
            let found = name.is_some();
            let cur = status.iter().find(|s| &s.0 == id);
            json!({
                "id": id,
                "found": found,
                "name": name.unwrap_or_else(|| id.clone()),
                "before": cur.map(|s| json!({
                    "lifecycle": s.1,
                    "note": s.2,
                    "date": s.3.map(|d| d.to_string()),
                })),
            })
        })
        .collect();
    Ok(Json(json!({
        "lifecycle": life,
        "date": date,
        "note": note,
        "checked": r.checked,
        "rows": rows,
        "missing": rows.iter().filter(|r| r["found"] == false).count(),
    })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn banned_words_are_caught() {
        assert!(super::banned_words("Now like XFire"));
        assert!(super::banned_words("xf1re"));
        assert!(!super::banned_words("Friends list loads faster"));
    }
}
