//! Per-game status kept by staff (dev.18): lifecycle (live / beta / test / early access /
//! ended), an expected end or launch date, a note, and when it was last checked and by
//! whom (a staff handle, or "Fraglink check" for the weekly routine via the CLI
//! `fraglink-server game-status`). See `migrations/0013_game_status.sql`.

use crate::error::ApiResult;
use crate::AppState;
use serde_json::{json, Value};
use sqlx::PgPool;

pub const LIFECYCLES: [&str; 5] = ["live", "beta", "test", "early_access", "ended"];

/// Changes to apply; `None` keeps the current value. Empty `date` / `note` clear them.
#[derive(Debug, Default, Clone)]
pub struct Update {
    pub lifecycle: Option<String>,
    pub date: Option<String>,
    pub note: Option<String>,
    /// stamps `checked_at = now()` with this name
    pub checked_by: Option<String>,
}

type Row = (
    String,
    Option<chrono::NaiveDate>,
    Option<String>,
    Option<chrono::DateTime<chrono::Utc>>,
    Option<String>,
);

fn to_json(r: Row) -> Value {
    json!({ "lifecycle": r.0, "date": r.1, "note": r.2, "checked_at": r.3, "checked_by": r.4 })
}

pub async fn get_db(db: &PgPool, id: &str) -> ApiResult<Option<Value>> {
    let r: Option<Row> = sqlx::query_as(
        "SELECT lifecycle, key_date, note, checked_at, checked_by FROM game_status WHERE game_id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(r.map(to_json))
}

pub async fn get(st: &AppState, id: &str) -> ApiResult<Option<Value>> {
    get_db(&st.db, id).await
}

/// Lifecycle accepted as `early access`, `Early-Access`, `ea`, ...
pub fn parse_lifecycle(s: &str) -> Option<&'static str> {
    let k = s.trim().to_lowercase().replace([' ', '-'], "_");
    let k = if k == "ea" { "early_access".into() } else { k };
    LIFECYCLES.iter().copied().find(|l| *l == k)
}

/// Apply `u` and return the new status.
pub async fn set(db: &PgPool, id: &str, u: Update) -> Result<Value, String> {
    let life = match &u.lifecycle {
        Some(l) => Some(
            parse_lifecycle(l)
                .ok_or_else(|| format!("lifecycle must be one of {}", LIFECYCLES.join(", ")))?,
        ),
        None => None,
    };
    let date: Option<Option<chrono::NaiveDate>> = match u.date.as_deref().map(str::trim) {
        None => None,
        Some("") | Some("none") => Some(None),
        Some(d) => Some(Some(
            chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
                .map_err(|_| format!("date must be YYYY-MM-DD, not {d}"))?,
        )),
    };
    let note: Option<Option<String>> = u.note.as_deref().map(|n| {
        let n: String = n.trim().chars().take(500).collect();
        (!n.is_empty()).then_some(n)
    });
    let by: Option<String> = u
        .checked_by
        .map(|b| b.trim().chars().take(60).collect::<String>())
        .filter(|b| !b.is_empty());
    sqlx::query(
        "INSERT INTO game_status(game_id, lifecycle, key_date, note, checked_at, checked_by)
         VALUES($1, COALESCE($2, 'live'), $4, $6, CASE WHEN $7::text IS NULL THEN NULL ELSE now() END, $7)
         ON CONFLICT (game_id) DO UPDATE SET
             lifecycle = COALESCE($2, game_status.lifecycle),
             key_date = CASE WHEN $3 THEN $4 ELSE game_status.key_date END,
             note = CASE WHEN $5 THEN $6 ELSE game_status.note END,
             checked_at = CASE WHEN $7::text IS NULL THEN game_status.checked_at ELSE now() END,
             checked_by = COALESCE($7, game_status.checked_by),
             updated_at = now()",
    )
    .bind(id)
    .bind(life)
    .bind(date.is_some())
    .bind(date.flatten())
    .bind(note.is_some())
    .bind(note.flatten())
    .bind(by)
    .execute(db)
    .await
    .map_err(|e| e.to_string())?;
    get_db(db, id)
        .await
        .map_err(|e| e.message)?
        .ok_or_else(|| "not saved".into())
}
