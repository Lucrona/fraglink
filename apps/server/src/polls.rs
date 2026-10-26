//! Home page Quick Poll.
//!
//! Public: `GET /api/v1/poll` (the active poll, results, and the caller's vote when signed in)
//! and `POST /api/v1/poll/vote {option_id}` (signed in; one vote per account, can be changed).
//! Results leave out test, banned, deleted and deleting accounts.
//! Admin (Admin role, audited): `GET/POST /api/v1/admin/polls`,
//! `POST /api/v1/admin/polls/{id}/activate`, `POST /api/v1/admin/polls/{id}/close`.

use crate::admin::{audit, Role, Staff};
use crate::auth::{AuthUser, MaybeUser};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/poll", get(active_public))
        .route("/api/v1/poll/vote", post(vote))
        .route("/api/v1/admin/polls", get(admin_list).post(create))
        .route("/api/v1/admin/polls/{id}/activate", post(activate))
        .route("/api/v1/admin/polls/{id}/close", post(close))
}

const COUNTED: &str =
    "v.user_id IN (SELECT id FROM users u WHERE NOT u.is_test AND u.deleted_at IS NULL
     AND u.banned_at IS NULL AND u.delete_after IS NULL)";

/// Percentages that add up to 100 (largest remainder), so the bars never look off.
pub fn percents(counts: &[i64]) -> Vec<i64> {
    let total: i64 = counts.iter().sum();
    if total == 0 {
        return vec![0; counts.len()];
    }
    let mut out: Vec<i64> = counts.iter().map(|c| c * 100 / total).collect();
    let mut rest: Vec<(usize, i64)> = counts
        .iter()
        .enumerate()
        .map(|(i, c)| (i, c * 100 % total))
        .collect();
    rest.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let short = 100 - out.iter().sum::<i64>();
    for (i, _) in rest.into_iter().take(short as usize) {
        out[i] += 1;
    }
    out
}

type PollRow = (i64, String, bool, Option<DateTime<Utc>>, DateTime<Utc>);

async fn poll_json(st: &AppState, p: PollRow, me: Option<uuid::Uuid>) -> ApiResult<Value> {
    let (id, question, active, closed_at, created_at) = p;
    let opts: Vec<(i64, String, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT o.id, o.label, (SELECT count(*) FROM poll_votes v WHERE v.option_id = o.id AND {COUNTED})
         FROM poll_options o WHERE o.poll_id = $1 ORDER BY o.pos, o.id"
    )))
    .bind(id)
    .fetch_all(&st.db)
    .await?;
    let counts: Vec<i64> = opts.iter().map(|o| o.2).collect();
    let pct = percents(&counts);
    let mine: Option<i64> = match me {
        Some(u) => {
            sqlx::query_scalar(
                "SELECT option_id FROM poll_votes WHERE poll_id = $1 AND user_id = $2",
            )
            .bind(id)
            .bind(u)
            .fetch_optional(&st.db)
            .await?
        }
        None => None,
    };
    Ok(json!({
        "id": id, "question": question, "active": active, "closed": closed_at.is_some(),
        "closed_at": closed_at, "created_at": created_at,
        "total": counts.iter().sum::<i64>(),
        "options": opts.iter().zip(pct).map(|(o, p)| json!({"id": o.0, "label": o.1, "votes": o.2, "pct": p})).collect::<Vec<_>>(),
        "my_vote": mine,
    }))
}

async fn active_row(st: &AppState) -> ApiResult<Option<PollRow>> {
    Ok(
        sqlx::query_as(
            "SELECT id, question, active, closed_at, created_at FROM polls WHERE active",
        )
        .fetch_optional(&st.db)
        .await?,
    )
}

async fn active_public(
    State(st): State<AppState>,
    MaybeUser(u): MaybeUser,
) -> ApiResult<impl IntoResponse> {
    let me = u.map(|a| a.user.id);
    let poll = match active_row(&st).await? {
        Some(p) => poll_json(&st, p, me).await?,
        None => Value::Null,
    };
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(json!({ "poll": poll, "signed_in": me.is_some() })),
    ))
}

#[derive(Deserialize)]
struct VoteReq {
    option_id: i64,
}

async fn vote(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<VoteReq>,
) -> ApiResult<Json<Value>> {
    if a.via_cookie {
        crate::api::same_origin(&headers, &st)?;
    }
    st.rate_limit(
        format!("pollvote:{}", a.user.id),
        30,
        std::time::Duration::from_secs(600),
    )?;
    let p = active_row(&st).await?.ok_or_else(AppError::not_found)?;
    if p.3.is_some() {
        return Err(AppError::bad_request("poll_closed", "This poll is closed."));
    }
    let ok: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM poll_options WHERE id = $1 AND poll_id = $2)",
    )
    .bind(r.option_id)
    .bind(p.0)
    .fetch_one(&st.db)
    .await?;
    if !ok {
        return Err(AppError::bad_request(
            "invalid_option",
            "Pick one of the answers.",
        ));
    }
    sqlx::query(
        "INSERT INTO poll_votes(poll_id, user_id, option_id) VALUES($1, $2, $3)
         ON CONFLICT (poll_id, user_id) DO UPDATE SET option_id = EXCLUDED.option_id, voted_at = now()",
    )
    .bind(p.0)
    .bind(a.user.id)
    .bind(r.option_id)
    .execute(&st.db)
    .await?;
    Ok(Json(
        json!({ "poll": poll_json(&st, p, Some(a.user.id)).await? }),
    ))
}

// ------------------------------------------------------------------ admin

async fn admin_list(State(st): State<AppState>, s: Staff) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let rows: Vec<PollRow> = sqlx::query_as(
        "SELECT id, question, active, closed_at, created_at FROM polls ORDER BY created_at DESC, id DESC LIMIT 100",
    )
    .fetch_all(&st.db)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        out.push(poll_json(&st, r, None).await?);
    }
    Ok(Json(json!({ "polls": out })))
}

fn clean(t: &str, max: usize) -> String {
    t.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .chars()
        .take(max)
        .collect()
}

#[derive(Deserialize)]
struct CreateReq {
    question: String,
    options: Vec<String>,
    #[serde(default)]
    activate: bool,
}

/// Question up to 140 chars, 2 to 6 non-empty distinct options up to 60 chars.
pub fn validate(q: &str, opts: &[String]) -> Result<(String, Vec<String>), &'static str> {
    let q = clean(q, 141);
    if q.is_empty() || q.chars().count() > 140 {
        return Err("Give the poll a question of up to 140 characters.");
    }
    let mut out: Vec<String> = Vec::new();
    for o in opts {
        let o = clean(o, 61);
        if o.is_empty() {
            continue;
        }
        if o.chars().count() > 60 {
            return Err("Answers can be up to 60 characters.");
        }
        if out.iter().any(|x| x.eq_ignore_ascii_case(&o)) {
            return Err("Two answers are the same.");
        }
        out.push(o);
    }
    if !(2..=6).contains(&out.len()) {
        return Err("A poll needs 2 to 6 answers.");
    }
    Ok((q, out))
}

async fn create(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<CreateReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let (q, opts) =
        validate(&r.question, &r.options).map_err(|m| AppError::bad_request("invalid_poll", m))?;
    let mut tx = st.db.begin().await?;
    if r.activate {
        sqlx::query("UPDATE polls SET active = false WHERE active")
            .execute(&mut *tx)
            .await?;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO polls(question, active, created_by) VALUES($1, $2, $3) RETURNING id",
    )
    .bind(&q)
    .bind(r.activate)
    .bind(s.user.id)
    .fetch_one(&mut *tx)
    .await?;
    for (i, o) in opts.iter().enumerate() {
        sqlx::query("INSERT INTO poll_options(poll_id, pos, label) VALUES($1, $2, $3)")
            .bind(id)
            .bind(i as i32 + 1)
            .bind(o)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    let after = json!({"id": id, "question": q, "options": opts, "active": r.activate});
    audit(
        &st,
        &s,
        "poll.create",
        &format!("poll:{id}"),
        Value::Null,
        after.clone(),
    )
    .await?;
    Ok(Json(json!({ "poll": after })))
}

async fn activate(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let mut tx = st.db.begin().await?;
    sqlx::query("UPDATE polls SET active = false WHERE active AND id <> $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let n = sqlx::query("UPDATE polls SET active = true, closed_at = NULL WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    tx.commit().await?;
    audit(
        &st,
        &s,
        "poll.activate",
        &format!("poll:{id}"),
        Value::Null,
        json!({"active": true}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

async fn close(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let n = sqlx::query("UPDATE polls SET closed_at = coalesce(closed_at, now()) WHERE id = $1")
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
        "poll.close",
        &format!("poll:{id}"),
        Value::Null,
        json!({"closed": true}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percents_sum_to_100() {
        assert_eq!(percents(&[0, 0, 0]), vec![0, 0, 0]);
        assert_eq!(percents(&[1, 1, 1]).iter().sum::<i64>(), 100);
        assert_eq!(percents(&[1, 2]), vec![33, 67]);
        assert_eq!(percents(&[5]), vec![100]);
    }

    #[test]
    fn validate_rules() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(validate("Q?", &s(&["a"])).is_err());
        assert!(validate("", &s(&["a", "b"])).is_err());
        assert!(validate("Q?", &s(&["a", "A"])).is_err());
        assert!(validate("Q?", &s(&["1", "2", "3", "4", "5", "6", "7"])).is_err());
        let (q, o) = validate(" Q? ", &s(&["a", " ", "b"])).unwrap();
        assert_eq!((q.as_str(), o.len()), ("Q?", 2));
    }
}
