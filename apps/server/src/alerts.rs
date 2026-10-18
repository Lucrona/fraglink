//! Admin email alerts, sent to support@playfraglink.com by default (Admin > Site settings).
//!
//! A background loop checks once a minute:
//! - `alert_approvals`: new items in Game approvals (unknown game ids from the app and
//!   "track this as a game?" suggestions). At most one email every 15 minutes, listing
//!   everything new since the last one.
//! - `alert_reports`: new player reports (reported profiles and pictures). Same 15 minutes.
//! - `alert_errors`: 5 or more 5xx responses within 10 minutes, or a background job failing.
//!   At most one email an hour.
//! - `alert_daily`: a summary at 9:00 AM Sydney time (new and active users, hours, pending
//!   approvals and reports, errors).
//!
//! Cloudflare Email Sending allows 200 emails a day for everything, so alerts have their own
//! cap of [`DAILY_CAP`] per 24 hours; past it they're skipped (and logged) until the window
//! moves on. Every attempt goes into `alert_log`. Watermarks start at "now" the first time, so
//! turning alerts on never mails a backlog.

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Duration as CDuration, Utc};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// (key, label, help) in display order. All default to on.
pub const ALERTS: [(&str, &str, &str); 4] = [
    ("alert_approvals", "New games waiting for approval", "Unknown game IDs from the app and game suggestions. At most one email every 15 minutes, listing everything new."),
    ("alert_reports", "New player reports", "Reported profiles and pictures. At most one email every 15 minutes."),
    ("alert_errors", "Server errors", "5 or more server errors (5xx) within 10 minutes, or a background job failing. At most one email an hour."),
    ("alert_daily", "Daily summary", "Every day at 9:00 AM Sydney time: new and active users, hours played, pending approvals and reports, and errors."),
];
pub const EMAIL_KEY: &str = "alert_email";
pub const DEFAULT_TO: &str = "support@playfraglink.com";
/// Alert emails per rolling 24 hours (Cloudflare's limit of 200 a day is shared with
/// account emails, so alerts stay well under it).
pub const DAILY_CAP: i64 = 40;
const QUEUE_GAP: CDuration = CDuration::minutes(15);
const ERROR_GAP: CDuration = CDuration::hours(1);
const ERROR_RUN: usize = 5;
const ERROR_WINDOW: Duration = Duration::from_secs(600);
const SITE: &str = "https://playfraglink.com";

pub fn is_key(k: &str) -> bool {
    ALERTS.iter().any(|a| a.0 == k)
}

// ------------------------------------------------------------------ error tracking

#[derive(Default)]
struct Errs {
    /// recent 5xx (time, status, "METHOD /path")
    recent: VecDeque<(Instant, u16, String)>,
    /// 5xx not yet added to the stored daily counter
    unsaved_5xx: i64,
    /// background job failures since the last error email (time, job, error)
    jobs: Vec<(DateTime<Utc>, String, String)>,
    unsaved_jobs: i64,
}

fn errs() -> &'static Mutex<Errs> {
    static E: OnceLock<Mutex<Errs>> = OnceLock::new();
    E.get_or_init(|| Mutex::new(Errs::default()))
}

/// Middleware: count 5xx responses (maintenance-mode 503s don't count).
pub async fn track(req: Request, next: Next) -> Response {
    let what = format!("{} {}", req.method(), req.uri().path());
    let res = next.run(req).await;
    let s = res.status();
    let maintenance = s.as_u16() == 503 && res.headers().contains_key("retry-after");
    if s.is_server_error() && !maintenance {
        let mut e = errs().lock().unwrap();
        e.recent.push_back((Instant::now(), s.as_u16(), what));
        while e.recent.len() > 200 {
            e.recent.pop_front();
        }
        e.unsaved_5xx += 1;
    }
    res
}

/// A background job failed (games list refresh, account purge, Steam sync...).
pub fn job_failed(job: &str, error: &str) {
    let mut e = errs().lock().unwrap();
    if e.jobs.len() < 50 {
        e.jobs.push((
            Utc::now(),
            job.to_string(),
            error.chars().take(300).collect(),
        ));
    }
    e.unsaved_jobs += 1;
}

// ------------------------------------------------------------------ settings and state

async fn switches(st: &AppState) -> ApiResult<Vec<(String, bool, Option<String>)>> {
    Ok(
        sqlx::query_as("SELECT key, enabled, value FROM site_settings WHERE key LIKE 'alert\\_%'")
            .fetch_all(&st.db)
            .await?,
    )
}

pub async fn enabled(st: &AppState, key: &str) -> bool {
    match switches(st).await {
        Ok(rows) => rows
            .iter()
            .find(|r| r.0 == key)
            .map(|r| r.1)
            .unwrap_or(true),
        Err(_) => false,
    }
}

pub async fn recipient(st: &AppState) -> String {
    switches(st)
        .await
        .ok()
        .and_then(|rows| {
            rows.into_iter()
                .find(|r| r.0 == EMAIL_KEY)
                .and_then(|r| r.2)
        })
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_TO.to_string())
}

async fn state(st: &AppState, key: &str) -> ApiResult<Option<(Option<DateTime<Utc>>, i64)>> {
    Ok(
        sqlx::query_as("SELECT at, n FROM alert_state WHERE key = $1")
            .bind(key)
            .fetch_optional(&st.db)
            .await?,
    )
}

async fn set_at(st: &AppState, key: &str, at: DateTime<Utc>) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO alert_state(key, at) VALUES($1, $2) ON CONFLICT (key) DO UPDATE SET at = $2",
    )
    .bind(key)
    .bind(at)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn add_n(st: &AppState, key: &str, n: i64) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO alert_state(key, n) VALUES($1, $2) ON CONFLICT (key) DO UPDATE SET n = alert_state.n + $2",
    )
    .bind(key)
    .bind(n)
    .execute(&st.db)
    .await?;
    Ok(())
}

/// Alert emails sent in the last 24 hours.
pub async fn sent_24h(st: &AppState) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM alert_log WHERE ok AND sent_at > now() - interval '24 hours'",
    )
    .fetch_one(&st.db)
    .await?)
}

/// Send one alert (respecting the cap) and log it. Ok(false) when skipped or failed.
pub async fn send(st: &AppState, kind: &str, e: &crate::emails::Email) -> ApiResult<bool> {
    let to = recipient(st).await;
    if sent_24h(st).await? >= DAILY_CAP {
        tracing::warn!(
            kind,
            "alert email skipped: daily cap of {DAILY_CAP} reached"
        );
        log(st, kind, &to, &e.subject, false, Some("daily cap reached")).await?;
        return Ok(false);
    }
    let r = crate::mailer::send_now(st, &to, e).await;
    let err = r.as_ref().err().map(|x| x.to_string());
    match &err {
        None => tracing::info!(kind, "alert email sent"),
        Some(x) => tracing::warn!(kind, error = %x, "alert email not sent"),
    }
    log(st, kind, &to, &e.subject, err.is_none(), err.as_deref()).await?;
    Ok(err.is_none())
}

async fn log(
    st: &AppState,
    kind: &str,
    to: &str,
    subject: &str,
    ok: bool,
    err: Option<&str>,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO alert_log(kind, recipient, subject, ok, error) VALUES($1, $2, $3, $4, $5)",
    )
    .bind(kind)
    .bind(to)
    .bind(subject)
    .bind(ok)
    .bind(err)
    .execute(&st.db)
    .await?;
    Ok(())
}

/// Debounce: true when `key` last went out more than `gap` ago (or never).
async fn due(st: &AppState, key: &str, gap: CDuration) -> ApiResult<bool> {
    Ok(match state(st, key).await? {
        Some((Some(at), _)) => Utc::now() - at >= gap,
        _ => true,
    })
}

/// The watermark for `key`, set to now (and None returned) the first time.
async fn watermark(st: &AppState, key: &str) -> ApiResult<Option<DateTime<Utc>>> {
    match state(st, key).await? {
        Some((Some(at), _)) => Ok(Some(at)),
        _ => {
            set_at(st, key, Utc::now()).await?;
            Ok(None)
        }
    }
}

fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ------------------------------------------------------------------ the checks

async fn check_approvals(st: &AppState) -> ApiResult<()> {
    if !enabled(st, "alert_approvals").await {
        return Ok(());
    }
    let Some(mark) = watermark(st, "approvals_mark").await? else {
        return Ok(());
    };
    let rows: Vec<(String, String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT 'Unknown game ID', game_id, exe, first_at FROM unknown_games WHERE status = 'new' AND first_at > $1
         UNION ALL
         SELECT 'Suggested game', name, exe, first_at FROM game_suggestions WHERE status = 'new' AND first_at > $1
         ORDER BY 4 LIMIT 51",
    )
    .bind(mark)
    .fetch_all(&st.db)
    .await?;
    if rows.is_empty() || !due(st, "approvals_sent", QUEUE_GAP).await? {
        return Ok(());
    }
    let pending: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM unknown_games WHERE status = 'new') + (SELECT count(*) FROM game_suggestions WHERE status = 'new')",
    )
    .fetch_one(&st.db)
    .await?;
    let newest = rows.iter().map(|r| r.3).max().unwrap_or(mark);
    let n = rows.len().min(50) as i64;
    let mut lines: Vec<String> = rows
        .iter()
        .take(50)
        .map(|(k, id, exe, _)| match exe {
            Some(x) if !x.is_empty() => format!("{k}: {id} ({x})"),
            _ => format!("{k}: {id}"),
        })
        .collect();
    if rows.len() > 50 {
        lines.push("…and more. Open Game approvals for the full list.".into());
    }
    let e = crate::emails::alert(
        &format!(
            "Fraglink: {} waiting for approval",
            plural(n, "new game", "new games")
        ),
        "New games waiting for approval",
        &format!(
            "{} since the last alert. {} waiting in total.",
            plural(n, "new item", "new items"),
            pending
        ),
        &lines,
        ("Open Game approvals", &format!("{SITE}/admin#approvals")),
    );
    set_at(st, "approvals_sent", Utc::now()).await?;
    if send(st, "approvals", &e).await? {
        set_at(st, "approvals_mark", newest).await?;
    }
    Ok(())
}

async fn check_reports(st: &AppState) -> ApiResult<()> {
    if !enabled(st, "alert_reports").await {
        return Ok(());
    }
    let Some(mark) = watermark(st, "reports_mark").await? else {
        return Ok(());
    };
    let rows: Vec<(String, String, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT 'Profile', u.handle, r.reason, r.created_at FROM profile_reports r JOIN users u ON u.id = r.target
           WHERE r.created_at > $1 AND r.resolved_at IS NULL
         UNION ALL
         SELECT 'Picture', u.handle, r.reason, r.created_at FROM avatar_reports r JOIN users u ON u.id = r.user_id
           WHERE r.created_at > $1 AND r.resolved_at IS NULL
         ORDER BY 4 LIMIT 51",
    )
    .bind(mark)
    .fetch_all(&st.db)
    .await?;
    if rows.is_empty() || !due(st, "reports_sent", QUEUE_GAP).await? {
        return Ok(());
    }
    let newest = rows.iter().map(|r| r.3).max().unwrap_or(mark);
    let n = rows.len().min(50) as i64;
    let lines: Vec<String> = rows
        .iter()
        .take(50)
        .map(|(k, h, why, _)| match why {
            Some(w) if !w.trim().is_empty() => format!("{k} of @{h}: “{}”", w.trim()),
            _ => format!("{k} of @{h}"),
        })
        .collect();
    let e = crate::emails::alert(
        &format!(
            "Fraglink: {}",
            plural(n, "new player report", "new player reports")
        ),
        "New player reports",
        &format!(
            "{} since the last alert.",
            plural(n, "new report", "new reports")
        ),
        &lines,
        ("Review reports", &format!("{SITE}/admin#avatars")),
    );
    set_at(st, "reports_sent", Utc::now()).await?;
    if send(st, "reports", &e).await? {
        set_at(st, "reports_mark", newest).await?;
    }
    Ok(())
}

async fn check_errors(st: &AppState) -> ApiResult<()> {
    // Save counters for the daily summary first (survives restarts).
    let (n5, nj) = {
        let mut e = errs().lock().unwrap();
        let v = (e.unsaved_5xx, e.unsaved_jobs);
        e.unsaved_5xx = 0;
        e.unsaved_jobs = 0;
        v
    };
    if n5 > 0 {
        add_n(st, "errors_5xx", n5).await?;
    }
    if nj > 0 {
        add_n(st, "errors_jobs", nj).await?;
    }
    if !enabled(st, "alert_errors").await {
        errs().lock().unwrap().jobs.clear();
        return Ok(());
    }
    let (recent, jobs) = {
        let mut e = errs().lock().unwrap();
        while e
            .recent
            .front()
            .is_some_and(|r| r.0.elapsed() > ERROR_WINDOW)
        {
            e.recent.pop_front();
        }
        if e.recent.len() < ERROR_RUN && e.jobs.is_empty() {
            return Ok(());
        }
        (
            e.recent
                .iter()
                .map(|r| (r.1, r.2.clone()))
                .collect::<Vec<_>>(),
            e.jobs.clone(),
        )
    };
    if !due(st, "errors_sent", ERROR_GAP).await? {
        return Ok(());
    }
    {
        let mut e = errs().lock().unwrap();
        e.recent.clear();
        e.jobs.clear();
    }
    let mut lines: Vec<String> = jobs
        .iter()
        .map(|(at, job, err)| format!("{} UTC · {job} failed: {err}", at.format("%H:%M")))
        .collect();
    // most frequent failing endpoints
    let mut by: Vec<(String, usize)> = Vec::new();
    for (code, what) in &recent {
        let k = format!("{code} {what}");
        match by.iter_mut().find(|b| b.0 == k) {
            Some(b) => b.1 += 1,
            None => by.push((k, 1)),
        }
    }
    by.sort_by_key(|b| std::cmp::Reverse(b.1));
    lines.extend(by.iter().take(10).map(|(k, n)| format!("{n}× {k}")));
    let mut parts = Vec::new();
    if recent.len() >= ERROR_RUN {
        parts.push(format!(
            "{} in the last 10 minutes",
            plural(recent.len() as i64, "server error", "server errors")
        ));
    }
    if !jobs.is_empty() {
        parts.push(plural(
            jobs.len() as i64,
            "background job failure",
            "background job failures",
        ));
    }
    let e = crate::emails::alert(
        "Fraglink: server errors",
        "Server errors",
        &format!(
            "{}. Next error alert in an hour at the earliest.",
            parts.join(" and ")
        ),
        &lines,
        ("Open the Server tab", &format!("{SITE}/admin#server")),
    );
    set_at(st, "errors_sent", Utc::now()).await?;
    send(st, "errors", &e).await?;
    Ok(())
}

/// True when the Sydney date of `last` is before today's (Sydney) and it's past 9:00 there.
fn daily_due(now: DateTime<Utc>, last: DateTime<Utc>) -> bool {
    use chrono::Timelike;
    let tz = chrono_tz::Australia::Sydney;
    let n = now.with_timezone(&tz);
    n.hour() >= 9 && last.with_timezone(&tz).date_naive() < n.date_naive()
}

async fn check_daily(st: &AppState) -> ApiResult<()> {
    let Some(last) = watermark(st, "daily_sent").await? else {
        return Ok(());
    };
    if !daily_due(Utc::now(), last) {
        return Ok(());
    }
    set_at(st, "daily_sent", Utc::now()).await?;
    if !enabled(st, "alert_daily").await {
        return Ok(());
    }
    let e = daily_email(st).await?;
    if send(st, "daily", &e).await? {
        sqlx::query("UPDATE alert_state SET n = 0 WHERE key IN ('errors_5xx', 'errors_jobs')")
            .execute(&st.db)
            .await?;
    }
    Ok(())
}

pub async fn daily_email(st: &AppState) -> ApiResult<crate::emails::Email> {
    let (total, new, active, secs, pending, reports): (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM users WHERE NOT is_test AND deleted_at IS NULL),
                (SELECT count(*) FROM users WHERE NOT is_test AND deleted_at IS NULL AND created_at > now() - interval '24 hours'),
                (SELECT count(DISTINCT t.user_id) FROM auth_tokens t JOIN users u ON u.id = t.user_id
                   WHERE NOT u.is_test AND t.last_used_at > now() - interval '24 hours'),
                (SELECT coalesce(sum(s.duration_s), 0)::bigint FROM sessions s
                   WHERE s.started_at > now() - interval '24 hours' AND s.user_id NOT IN (SELECT id FROM users WHERE is_test)),
                (SELECT count(*) FROM unknown_games WHERE status = 'new') + (SELECT count(*) FROM game_suggestions WHERE status = 'new'),
                (SELECT count(*) FROM profile_reports WHERE resolved_at IS NULL) + (SELECT count(*) FROM avatar_reports WHERE resolved_at IS NULL)",
    )
    .fetch_one(&st.db)
    .await?;
    let (e5, ej) = {
        let e = errs().lock().unwrap();
        (e.unsaved_5xx, e.unsaved_jobs)
    };
    let s5 = state(st, "errors_5xx").await?.map(|s| s.1).unwrap_or(0) + e5;
    let sj = state(st, "errors_jobs").await?.map(|s| s.1).unwrap_or(0) + ej;
    let alerts = sent_24h(st).await?;
    let hours = (secs as f64 / 360.0).round() / 10.0;
    let lines = vec![
        format!("New users: {new} (total {total})"),
        format!("Active users: {active}"),
        format!("Hours played: {hours}"),
        format!("Waiting in Game approvals: {pending}"),
        format!("Open player reports: {reports}"),
        format!("Server errors: {s5} · background job failures: {sj}"),
        format!("Alert emails sent: {alerts} of {DAILY_CAP} allowed a day"),
    ];
    let date = Utc::now()
        .with_timezone(&chrono_tz::Australia::Sydney)
        .format("%a %-d %b");
    Ok(crate::emails::alert(
        &format!("Fraglink daily summary, {date}"),
        "Daily summary",
        "The last 24 hours on Fraglink (test accounts not counted; errors since the last summary).",
        &lines,
        ("Open Admin", &format!("{SITE}/admin")),
    ))
}

/// The alert loop (spawned from main).
pub async fn run(st: AppState) {
    tokio::time::sleep(Duration::from_secs(30)).await;
    let mut tick = tokio::time::interval(Duration::from_secs(60));
    loop {
        tick.tick().await;
        for (name, r) in [
            ("approvals", check_approvals(&st).await),
            ("reports", check_reports(&st).await),
            ("errors", check_errors(&st).await),
            ("daily", check_daily(&st).await),
        ] {
            if let Err(e) = r {
                tracing::warn!("alert check {name} failed: {}", e.message);
            }
        }
    }
}

// ------------------------------------------------------------------ admin

pub fn valid_email(s: &str) -> bool {
    let s = s.trim();
    let Some((local, domain)) = s.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
        && s.len() <= 200
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>' || c == ',')
}

pub async fn admin_view(st: &AppState) -> ApiResult<Value> {
    let rows: Vec<(String, bool, Option<String>, DateTime<Utc>, Option<String>)> = sqlx::query_as(
        "SELECT s.key, s.enabled, s.value, s.updated_at, u.handle FROM site_settings s
         LEFT JOIN users u ON u.id = s.updated_by WHERE s.key LIKE 'alert\\_%'",
    )
    .fetch_all(&st.db)
    .await?;
    let items: Vec<Value> = ALERTS
        .iter()
        .map(|(k, label, help)| {
            let r = rows.iter().find(|r| r.0 == *k);
            json!({ "key": k, "label": label, "help": help, "enabled": r.map(|r| r.1).unwrap_or(true),
                    "updated_at": r.map(|r| r.3), "updated_by": r.and_then(|r| r.4.clone()) })
        })
        .collect();
    let recent: Vec<(String, String, bool, Option<String>, DateTime<Utc>)> = sqlx::query_as(
        "SELECT kind, subject, ok, error, sent_at FROM alert_log ORDER BY sent_at DESC LIMIT 10",
    )
    .fetch_all(&st.db)
    .await?;
    Ok(json!({
        "alerts": items,
        "recipient": recipient(st).await,
        "default_recipient": DEFAULT_TO,
        "sent_24h": sent_24h(st).await?,
        "cap": DAILY_CAP,
        "mail_live": crate::mailer::live(st),
        "recent": recent.into_iter().map(|(k, s, ok, e, at)| json!({"kind": k, "subject": s, "ok": ok, "error": e, "at": at})).collect::<Vec<_>>(),
    }))
}

pub async fn set_recipient(st: &AppState, to: &str, by: uuid::Uuid) -> ApiResult<()> {
    if !valid_email(to) {
        return Err(AppError::bad_request(
            "invalid_email",
            "Enter one email address.",
        ));
    }
    sqlx::query(
        "INSERT INTO site_settings(key, enabled, value, updated_by) VALUES($1, true, $2, $3)
         ON CONFLICT (key) DO UPDATE SET value = $2, updated_by = $3, updated_at = now()",
    )
    .bind(EMAIL_KEY)
    .bind(to.trim())
    .bind(by)
    .execute(&st.db)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    #[test]
    fn daily_runs_once_after_nine_sydney() {
        // 2026-10-18 is AEDT (UTC+11): 9:00 Sydney = 22:00 UTC the day before.
        let last = Utc.with_ymd_and_hms(2026, 10, 17, 22, 0, 30).unwrap(); // 18 Oct 9:00:30
        let before_nine = Utc.with_ymd_and_hms(2026, 10, 18, 21, 59, 0).unwrap(); // 19 Oct 8:59
        let after_nine = Utc.with_ymd_and_hms(2026, 10, 18, 22, 1, 0).unwrap(); // 19 Oct 9:01
        assert!(!super::daily_due(before_nine, last));
        assert!(super::daily_due(after_nine, last));
        assert!(!super::daily_due(after_nine, after_nine));
    }

    #[test]
    fn recipient_validation() {
        assert!(super::valid_email("support@playfraglink.com"));
        assert!(!super::valid_email("a@b"));
        assert!(!super::valid_email("a@b.com, c@d.com"));
        assert!(!super::valid_email("Name <a@b.com>"));
    }
    use chrono::Utc;
}
