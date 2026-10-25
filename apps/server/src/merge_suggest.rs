//! Suggested merges (0.3.0-beta.5). When a program someone tracked themselves
//! (`exe:<name>.exe`, with recorded sessions) matches a game on Fraglink's games list,
//! it is listed in Admin > Game approvals with its players and hours. Staff merge it with
//! the normal Admin > Games > Merge (which moves everything and retires the `exe:` id)
//! or dismiss it. Nothing is merged automatically.

use crate::admin::{audit, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/merge_suggestions", get(list))
        .route(
            "/api/v1/admin/merge_suggestions/{id}/dismiss",
            post(dismiss),
        )
}

/// Look for `exe:` entries with sessions that the games list now knows and record new
/// ones. Retired or merged entries are skipped; dismissed ones stay dismissed.
pub async fn refresh(st: &AppState) -> ApiResult<()> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT s.game_id FROM sessions s
         WHERE s.game_id LIKE 'exe:%'
           AND NOT EXISTS (SELECT 1 FROM game_retirements r WHERE r.game_id = s.game_id AND NOT r.partial)
           AND NOT EXISTS (SELECT 1 FROM merge_suggestions m WHERE m.game_id = s.game_id)",
    )
    .fetch_all(&st.db)
    .await?;
    for id in ids {
        let into = crate::names::list_id_for_exe(st, &id);
        if into == id {
            continue;
        }
        let into = crate::retire::effective_id(st, &into).await?;
        sqlx::query(
            "INSERT INTO merge_suggestions(game_id, into_id) VALUES($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(&id)
        .bind(&into)
        .execute(&st.db)
        .await?;
    }
    Ok(())
}

/// Open suggestions first seen after `mark` (for the approvals alert): (exe id, target, first seen).
pub async fn new_since(
    st: &AppState,
    mark: DateTime<Utc>,
) -> ApiResult<Vec<(String, String, DateTime<Utc>)>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT m.game_id, m.into_id, m.first_at FROM merge_suggestions m WHERE {OPEN} AND m.first_at > $1 ORDER BY 3 LIMIT 51"
    )))
    .bind(mark)
    .fetch_all(&st.db)
    .await?)
}

pub async fn open_count(st: &AppState) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM merge_suggestions m WHERE {OPEN}"
    )))
    .fetch_one(&st.db)
    .await?)
}

const OPEN: &str = "m.status = 'new' AND NOT EXISTS (SELECT 1 FROM game_retirements r WHERE r.game_id = m.game_id AND NOT r.partial)";

async fn list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    refresh(&st).await?;
    let rows: Vec<(String, String, DateTime<Utc>, i64, i64)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT m.game_id, m.into_id, m.first_at,
                    (SELECT count(DISTINCT user_id) FROM sessions WHERE game_id = m.game_id),
                    (SELECT coalesce(sum(duration_s), 0)::bigint FROM sessions WHERE game_id = m.game_id)
             FROM merge_suggestions m WHERE {OPEN} ORDER BY m.first_at DESC LIMIT 300"
        )))
        .fetch_all(&st.db)
        .await?;
    let mut items = Vec::new();
    for (id, into, at, players, secs) in rows {
        let name = crate::names::public_name(&st, &id).await?;
        let into_name = crate::names::public_name(&st, &into).await?;
        items.push(
            json!({ "game_id": id, "exe": id.trim_start_matches("exe:"), "name": name, "into": into,
                "into_name": into_name, "players": players, "seconds": secs, "first_seen": at }),
        );
    }
    Ok(Json(json!({ "merges": items })))
}

async fn dismiss(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let r = sqlx::query(
        "UPDATE merge_suggestions SET status = 'dismissed', decided_by = $2, decided_at = now()
         WHERE game_id = $1 AND status = 'new'",
    )
    .bind(&id)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::not_found());
    }
    audit(
        &st,
        &s,
        "merge_suggestion.dismiss",
        &format!("game:{id}"),
        Value::Null,
        json!({ "status": "dismissed" }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
