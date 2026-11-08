//! Hours Review "mark as reviewed".
//!
//! A review stores the evidence it was judging (the seconds, the session length,
//! the overlap). The queue hides a flag only while that evidence is still what
//! the database shows. A new or changed session makes a different evidence
//! string, so the flag comes back. Reviewing never deletes or rewrites sessions.

use crate::admin::{audit, bad, clean, Role, Staff};
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/hours/flags/review", post(review))
        .route("/api/v1/admin/hours/flags/reopen", post(reopen))
}

#[derive(Clone)]
struct Latest {
    evidence: String,
    status: String,
    reason: Option<String>,
    at: DateTime<Utc>,
    by: Option<String>,
}

async fn latest_for(
    st: &AppState,
    keys: &[String],
) -> ApiResult<std::collections::HashMap<String, Latest>> {
    if keys.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<(String, String, String, Option<String>, DateTime<Utc>, Option<String>)> =
        sqlx::query_as(
            "SELECT DISTINCT ON (r.flag_key) r.flag_key, r.evidence, r.status, r.reason, r.created_at, u.handle
             FROM hours_flag_reviews r
             LEFT JOIN users u ON u.id = r.actor_id
             WHERE r.flag_key = ANY($1)
             ORDER BY r.flag_key, r.created_at DESC, r.id DESC",
        )
        .bind(keys)
        .fetch_all(&st.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(k, e, s, r, at, by)| {
            (
                k,
                Latest {
                    evidence: e,
                    status: s,
                    reason: r,
                    at,
                    by,
                },
            )
        })
        .collect())
}

/// Attach review state and drop rows the queue should not show.
/// `show` is `open` (default), `reviewed`, or `all`.
pub async fn filter(st: &AppState, mut flags: Vec<Value>, show: &str) -> ApiResult<Value> {
    let keys: Vec<String> = flags
        .iter()
        .filter_map(|f| f["flag_key"].as_str().map(str::to_string))
        .collect();
    let latest = latest_for(st, &keys).await?;
    let mut open = 0usize;
    let mut reviewed = 0usize;
    for f in &mut flags {
        let key = f["flag_key"].as_str().unwrap_or("");
        let evidence = f["evidence"].as_str().unwrap_or("");
        if let Some(l) = latest.get(key) {
            let matches = l.status == "reviewed" && l.evidence == evidence;
            if matches {
                reviewed += 1;
            } else {
                open += 1;
            }
            f["review"] = json!({
                "status": l.status,
                "matches": matches,
                "at": l.at,
                "by": l.by,
                "reason": l.reason,
                "evidence": l.evidence,
            });
        } else {
            open += 1;
            f["review"] = Value::Null;
        }
    }
    let shown: Vec<Value> = flags
        .into_iter()
        .filter(|f| match show {
            "reviewed" => f["review"]["matches"] == true,
            "all" => true,
            _ => f["review"]["matches"] != true,
        })
        .collect();
    Ok(json!({
        "flags": shown,
        "open": open,
        "reviewed": reviewed,
        "show": if show == "reviewed" || show == "all" { show } else { "open" },
    }))
}

#[derive(Deserialize)]
struct ReviewReq {
    flag_key: String,
    #[serde(default)]
    days: Option<i32>,
    #[serde(default)]
    reason: Option<String>,
}

fn reason(r: &Option<String>) -> ApiResult<Option<String>> {
    let Some(r) = r.as_deref() else {
        return Ok(None);
    };
    let r = clean(r, 300);
    Ok((!r.is_empty()).then_some(r))
}

async fn find_flag(st: &AppState, key: &str, days: i32) -> ApiResult<Value> {
    let body = crate::admin::hours_flag_list(st, days).await?;
    body.into_iter()
        .find(|f| f["flag_key"].as_str() == Some(key))
        .ok_or_else(|| bad("not_found", "That flag is not in the current queue."))
}

async fn review(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<ReviewReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let why = reason(&r.reason)?;
    let days = r.days.unwrap_or(30).clamp(1, 365);
    let flag = find_flag(&st, r.flag_key.trim(), days).await?;
    let key = flag["flag_key"].as_str().unwrap_or("").to_string();
    let evidence = flag["evidence"].as_str().unwrap_or("").to_string();
    let kind = flag["kind"].as_str().unwrap_or("").to_string();
    if key.is_empty() || evidence.is_empty() {
        return Err(bad("bad_flag", "That flag has no stable identity."));
    }
    sqlx::query(
        "INSERT INTO hours_flag_reviews(flag_key, kind, evidence, status, reason, actor_id)
         VALUES($1, $2, $3, 'reviewed', $4, $5)",
    )
    .bind(&key)
    .bind(&kind)
    .bind(&evidence)
    .bind(&why)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    audit(
        &st,
        &s,
        "hours.review",
        &format!("hours:{key}"),
        Value::Null,
        json!({ "flag_key": key, "kind": kind, "evidence": evidence, "reason": why }),
    )
    .await?;
    Ok(Json(
        json!({ "ok": true, "flag_key": key, "evidence": evidence }),
    ))
}

async fn reopen(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<ReviewReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let why = reason(&r.reason)?;
    let key = r.flag_key.trim();
    if key.is_empty() {
        return Err(bad("bad_flag", "Missing flag."));
    }
    let days = r.days.unwrap_or(365).clamp(1, 365);
    let flag = find_flag(&st, key, days).await?;
    let evidence = flag["evidence"].as_str().unwrap_or("");
    let latest = latest_for(&st, &[key.to_string()]).await?;
    let Some(l) = latest.get(key) else {
        return Err(bad("not_reviewed", "That flag is not marked reviewed."));
    };
    if l.status != "reviewed" || l.evidence != evidence {
        return Err(bad(
            "not_reviewed",
            "The current evidence is not the one that was reviewed.",
        ));
    }
    sqlx::query(
        "INSERT INTO hours_flag_reviews(flag_key, kind, evidence, status, reason, actor_id)
         VALUES($1, $2, $3, 'reopened', $4, $5)",
    )
    .bind(key)
    .bind(flag["kind"].as_str().unwrap_or(""))
    .bind(evidence)
    .bind(&why)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    audit(
        &st,
        &s,
        "hours.review_reopen",
        &format!("hours:{key}"),
        json!({ "evidence": l.evidence }),
        json!({ "evidence": evidence, "reason": why }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
