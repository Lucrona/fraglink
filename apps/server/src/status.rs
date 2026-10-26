//! Public status page (`/status`).
//!
//! `GET /api/v1/status`: live checks done now (API answering, database query, live
//! connection hub accepting app connections, maintenance switch) plus the last 20 incidents.
//! Admin (Admin role, audited): `GET/POST /api/v1/admin/incidents`,
//! `POST /api/v1/admin/incidents/{id}/resolve`, `POST /api/v1/admin/incidents/{id}/delete`.

use crate::admin::{audit, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/status", get(public))
        .route("/api/v1/admin/incidents", get(admin_list).post(create))
        .route("/api/v1/admin/incidents/{id}/resolve", post(resolve))
        .route("/api/v1/admin/incidents/{id}/delete", post(remove))
}

type Row = (
    i64,
    String,
    String,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn row_json(r: Row) -> Value {
    json!({ "id": r.0, "title": r.1, "body": r.2, "severity": r.3, "started_at": r.4, "resolved_at": r.5 })
}

async fn list(st: &AppState, limit: i64) -> ApiResult<Vec<Value>> {
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT id, title, body, severity, started_at, resolved_at FROM incidents
         ORDER BY started_at DESC, id DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(&st.db)
    .await?;
    Ok(rows.into_iter().map(row_json).collect())
}

async fn public(State(st): State<AppState>) -> ApiResult<impl IntoResponse> {
    let t = Instant::now();
    let db_ok = matches!(
        tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&st.db)
        )
        .await,
        Ok(Ok(1))
    );
    let db_ms = t.elapsed().as_millis() as u64;
    let hub_ok = st.hub.live_sockets() < crate::presence::max_sockets();
    let maintenance = db_ok && crate::features::on(&st, "maintenance").await;
    let incidents = if db_ok { list(&st, 20).await? } else { vec![] };
    Ok((
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({
            "checked_at": Utc::now(),
            "version": env!("CARGO_PKG_VERSION"),
            "api": true,
            "database": { "ok": db_ok, "ms": db_ms },
            "live": { "ok": hub_ok && db_ok },
            "maintenance": maintenance,
            "incidents": incidents,
        })),
    ))
}

async fn admin_list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    Ok(Json(json!({ "incidents": list(&st, 200).await? })))
}

#[derive(Deserialize)]
pub struct CreateReq {
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    severity: String,
    /// When it started (RFC 3339); default now.
    #[serde(default)]
    started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    resolved_at: Option<DateTime<Utc>>,
}

pub fn validate(
    title: &str,
    body: &str,
    severity: &str,
) -> Result<(String, String, String), &'static str> {
    let title = title.trim();
    let body = body.trim();
    let sev = if severity.is_empty() {
        "minor"
    } else {
        severity
    };
    if title.is_empty() || title.chars().count() > 120 {
        return Err("Title: 1 to 120 characters.");
    }
    if body.chars().count() > 2000 {
        return Err("Details: up to 2,000 characters.");
    }
    if !["minor", "major", "maintenance"].contains(&sev) {
        return Err("Severity must be minor, major or maintenance.");
    }
    Ok((title.into(), body.into(), sev.into()))
}

async fn create(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<CreateReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (title, body, sev) = validate(&r.title, &r.body, &r.severity)
        .map_err(|m| AppError::bad_request("invalid_incident", m))?;
    let started = r.started_at.unwrap_or_else(Utc::now);
    if r.resolved_at.is_some_and(|e| e < started) {
        return Err(AppError::bad_request(
            "invalid_incident",
            "Resolved before it started.",
        ));
    }
    let row: Row = sqlx::query_as(
        "INSERT INTO incidents(title, body, severity, started_at, resolved_at, created_by)
         VALUES($1, $2, $3, $4, $5, $6)
         RETURNING id, title, body, severity, started_at, resolved_at",
    )
    .bind(&title)
    .bind(&body)
    .bind(&sev)
    .bind(started)
    .bind(r.resolved_at)
    .bind(s.user.id)
    .fetch_one(&st.db)
    .await?;
    let after = row_json(row);
    audit(
        &st,
        &s,
        "incident.create",
        &format!("incident:{}", after["id"]),
        Value::Null,
        after.clone(),
    )
    .await?;
    Ok(Json(json!({ "incident": after })))
}

async fn resolve(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let n = sqlx::query(
        "UPDATE incidents SET resolved_at = now() WHERE id = $1 AND resolved_at IS NULL",
    )
    .bind(id)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    audit(
        &st,
        &s,
        "incident.resolve",
        &format!("incident:{id}"),
        Value::Null,
        json!({"resolved": true}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn remove(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let before: Option<Row> = sqlx::query_as(
        "DELETE FROM incidents WHERE id = $1 RETURNING id, title, body, severity, started_at, resolved_at",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let Some(b) = before else {
        return Err(AppError::not_found());
    };
    audit(
        &st,
        &s,
        "incident.delete",
        &format!("incident:{id}"),
        row_json(b),
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn incident_validation() {
        assert!(super::validate("Login down", "", "").is_ok());
        assert!(super::validate("", "", "minor").is_err());
        assert!(super::validate("x", "", "catastrophic").is_err());
    }
}
