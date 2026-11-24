//! Staff-started walk of catalogue games that have no /games banner.
//!
//! Nothing starts on its own. An admin clicks "Fetch all missing" (or Resume)
//! on the Missing artwork page. The next due game is picked with one indexed
//! query against `art_backfill_queue`. Failed looks write a `.miss` and are
//! not tried again this run (and a fresh miss is not retried in a tight loop).
//! After a process restart a running job is marked stopped; staff can resume it.

use crate::admin::bad;
use crate::error::ApiResult;
use crate::AppState;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio::time::{sleep, Instant};

/// Test instrumentation: incremented once per `pick_due` SQL statement.
static PICK_SQL: AtomicU64 = AtomicU64::new(0);

pub fn pick_sql_count() -> u64 {
    PICK_SQL.load(Ordering::SeqCst)
}

pub fn reset_pick_sql_count() {
    PICK_SQL.store(0, Ordering::SeqCst);
}

/// Process-local handle: only one worker loop, and a stop can wake it.
#[derive(Default)]
pub struct Ctl {
    worker: AtomicBool,
    gen: AtomicU64,
    wake: Notify,
}

impl Ctl {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

#[derive(Clone, Debug)]
pub struct Status {
    pub enabled: bool,
    pub every_secs: u64,
    pub status: String,
    pub done: i64,
    pub found: i64,
    pub failed: i64,
    pub remaining: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub started_by: Option<String>,
    pub worker_alive: bool,
}

impl Status {
    fn json(&self) -> Value {
        json!({
            "enabled": self.enabled,
            "every_secs": self.every_secs,
            "status": self.status,
            "done": self.done,
            "found": self.found,
            "failed": self.failed,
            "remaining": self.remaining,
            "started_at": self.started_at,
            "updated_at": self.updated_at,
            "started_by": self.started_by,
            "worker_alive": self.worker_alive,
            "can_start": self.enabled && self.status != "running",
            "can_stop": self.enabled && self.status == "running",
            "can_resume": self.enabled && self.status == "stopped" && self.remaining > 0,
        })
    }
}

/// After a process start: a job that was running is stopped, not resumed.
pub async fn pause_on_boot(st: &AppState) {
    let res = sqlx::query(
        "UPDATE art_backfill_run SET status = 'stopped', updated_at = now()
         WHERE status = 'running'",
    )
    .execute(&st.db)
    .await;
    match res {
        Ok(r) if r.rows_affected() > 0 => {
            tracing::info!(
                "art backfill: marked {} running job(s) stopped after restart",
                r.rows_affected()
            );
        }
        Err(e) => tracing::debug!("art backfill boot: {e}"),
        Ok(_) => {}
    }
}

pub async fn status(st: &AppState) -> ApiResult<Value> {
    Ok(current(st).await?.json())
}

async fn current(st: &AppState) -> ApiResult<Status> {
    let row: Option<(
        String,
        DateTime<Utc>,
        DateTime<Utc>,
        Option<String>,
        i64,
        i64,
        i64,
        i64,
    )> = sqlx::query_as(
        "SELECT status, started_at, updated_at, started_by, done, found, failed, remaining
         FROM art_backfill_run
         ORDER BY id DESC
         LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    let worker_alive = st.art_backfill.worker.load(Ordering::SeqCst);
    let mut s = Status {
        enabled: st.cfg.art_backfill,
        every_secs: st.cfg.art_backfill_every_secs,
        status: "idle".into(),
        done: 0,
        found: 0,
        failed: 0,
        remaining: 0,
        started_at: None,
        updated_at: None,
        started_by: None,
        worker_alive,
    };
    if let Some((status, started, updated, by, done, found, failed, remaining)) = row {
        s.status = status;
        s.started_at = Some(started);
        s.updated_at = Some(updated);
        s.started_by = by;
        s.done = done;
        s.found = found;
        s.failed = failed;
        s.remaining = remaining;
    }
    Ok(s)
}

pub async fn start(st: &AppState, by: &str) -> ApiResult<Value> {
    require_on(st)?;
    let cur = current(st).await?;
    if cur.status == "running" && cur.worker_alive {
        return Ok(cur.json());
    }
    if cur.status == "running" && !cur.worker_alive {
        // DB said running but no worker (should not happen after pause_on_boot).
        return resume(st, by).await;
    }
    let queued = build_queue(st).await?;
    sqlx::query(
        "INSERT INTO art_backfill_run
            (status, started_at, updated_at, started_by, done, found, failed, remaining)
         VALUES ('running', now(), now(), $1, 0, 0, 0, $2)",
    )
    .bind(by)
    .bind(queued as i64)
    .execute(&st.db)
    .await?;
    spawn_worker(st);
    tracing::info!(by, queued, "art backfill: started");
    current(st).await.map(|s| s.json())
}

pub async fn resume(st: &AppState, by: &str) -> ApiResult<Value> {
    require_on(st)?;
    let cur = current(st).await?;
    if cur.status == "running" && cur.worker_alive {
        return Ok(cur.json());
    }
    if cur.status == "finished" || cur.status == "idle" {
        return Err(bad(
            "nothing_to_resume",
            "There is no stopped backfill to resume. Use Fetch all missing to start one.",
        ));
    }
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM art_backfill_queue WHERE state = 'pending'")
            .fetch_one(&st.db)
            .await?;
    sqlx::query(
        "UPDATE art_backfill_run
         SET status = 'running', updated_at = now(), started_by = $1, remaining = $2
         WHERE id = (SELECT id FROM art_backfill_run ORDER BY id DESC LIMIT 1)",
    )
    .bind(by)
    .bind(remaining)
    .execute(&st.db)
    .await?;
    spawn_worker(st);
    tracing::info!(by, remaining, "art backfill: resumed");
    current(st).await.map(|s| s.json())
}

pub async fn stop(st: &AppState) -> ApiResult<Value> {
    require_on(st)?;
    sqlx::query(
        "UPDATE art_backfill_run
         SET status = 'stopped', updated_at = now()
         WHERE status = 'running'",
    )
    .execute(&st.db)
    .await?;
    st.art_backfill.gen.fetch_add(1, Ordering::SeqCst);
    st.art_backfill.wake.notify_waiters();
    wait_worker_down(st).await;
    tracing::info!("art backfill: stopped");
    current(st).await.map(|s| s.json())
}

async fn wait_worker_down(st: &AppState) {
    let start = Instant::now();
    while st.art_backfill.worker.load(Ordering::SeqCst) {
        if start.elapsed() > Duration::from_secs(20) {
            tracing::debug!("art backfill: worker still winding down after stop");
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
}

fn require_on(st: &AppState) -> ApiResult<()> {
    if st.cfg.art_backfill {
        Ok(())
    } else {
        Err(bad(
            "backfill_off",
            "Art backfill is switched off on this server.",
        ))
    }
}

/// One indexed query: the next pending game that is due.
pub async fn pick_due(st: &AppState) -> anyhow::Result<Option<String>> {
    PICK_SQL.fetch_add(1, Ordering::SeqCst);
    let id: Option<String> = sqlx::query_scalar(
        "SELECT game_id
         FROM art_backfill_queue
         WHERE state = 'pending' AND next_due_at <= now()
         ORDER BY game_id
         LIMIT 1",
    )
    .fetch_optional(&st.db)
    .await?;
    Ok(id)
}

async fn build_queue(st: &AppState) -> ApiResult<usize> {
    sqlx::query("DELETE FROM art_backfill_queue")
        .execute(&st.db)
        .await?;
    let ids = crate::admin_desk::catalogue_ids(st).await?;
    let mut due: Vec<String> = Vec::new();
    for id in ids {
        if crate::art::has_banner(st, &id) {
            continue;
        }
        if crate::art::miss_remaining(st, &id).is_some() {
            continue;
        }
        due.push(id);
    }
    // Fresh misses are not in this pass (they would only become due after the
    // .miss ttl). They can be picked up on a later start.
    let n = due.len();
    for chunk in due.chunks(500) {
        let ids: Vec<String> = chunk.to_vec();
        sqlx::query(
            "INSERT INTO art_backfill_queue (game_id, state, next_due_at)
             SELECT game_id, 'pending', '-infinity'::timestamptz
             FROM unnest($1::text[]) AS t(game_id)
             ON CONFLICT (game_id) DO NOTHING",
        )
        .bind(&ids)
        .execute(&st.db)
        .await?;
    }
    Ok(n)
}

fn spawn_worker(st: &AppState) {
    if !st.cfg.art_backfill {
        return;
    }
    if st.art_backfill.worker.swap(true, Ordering::SeqCst) {
        return;
    }
    let gen = st.art_backfill.gen.load(Ordering::SeqCst);
    let st = st.clone();
    tokio::spawn(async move {
        let mut since_log = 0u64;
        loop {
            if !alive(&st, gen).await {
                break;
            }
            match step(&st).await {
                Ok(Step::Did) => {
                    since_log += 1;
                    if since_log.is_multiple_of(25) {
                        if let Ok(s) = current(&st).await {
                            tracing::info!(
                                done = s.done,
                                found = s.found,
                                failed = s.failed,
                                remaining = s.remaining,
                                "art backfill: progress"
                            );
                        }
                    }
                    if !wait_or_stop(&st, gen).await {
                        break;
                    }
                }
                Ok(Step::Idle) => {
                    if pending_count(&st).await.unwrap_or(1) == 0 {
                        let _ = finish(&st).await;
                        tracing::info!("art backfill: finished");
                        break;
                    }
                    if !wait_or_stop(&st, gen).await {
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!("art backfill: {e}");
                    if !wait_or_stop(&st, gen).await {
                        break;
                    }
                }
            }
        }
        st.art_backfill.worker.store(false, Ordering::SeqCst);
    });
}

enum Step {
    Did,
    Idle,
}

async fn step(st: &AppState) -> anyhow::Result<Step> {
    let Some(id) = pick_due(st).await? else {
        return Ok(Step::Idle);
    };
    if crate::art::has_banner(st, &id) {
        mark(st, &id, true).await?;
        return Ok(Step::Did);
    }
    if let Some(wait) = crate::art::miss_remaining(st, &id) {
        let due = Utc::now()
            + chrono::Duration::from_std(wait).unwrap_or_else(|_| chrono::Duration::days(1));
        sqlx::query(
            "UPDATE art_backfill_queue SET next_due_at = $2 WHERE game_id = $1 AND state = 'pending'",
        )
        .bind(&id)
        .bind(due)
        .execute(&st.db)
        .await?;
        return Ok(Step::Idle);
    }
    let found = crate::art::retry(st, &id).await;
    mark(st, &id, found).await?;
    Ok(Step::Did)
}

async fn mark(st: &AppState, id: &str, found: bool) -> anyhow::Result<()> {
    let state = if found { "found" } else { "miss" };
    sqlx::query(
        "UPDATE art_backfill_queue
         SET state = $2, next_due_at = 'infinity'
         WHERE game_id = $1",
    )
    .bind(id)
    .bind(state)
    .execute(&st.db)
    .await?;
    let remaining = pending_count(st).await?;
    sqlx::query(
        "UPDATE art_backfill_run
         SET done = done + 1,
             found = found + CASE WHEN $1 THEN 1 ELSE 0 END,
             failed = failed + CASE WHEN $1 THEN 0 ELSE 1 END,
             remaining = $2,
             updated_at = now()
         WHERE id = (SELECT id FROM art_backfill_run ORDER BY id DESC LIMIT 1)
           AND status = 'running'",
    )
    .bind(found)
    .bind(remaining)
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn pending_count(st: &AppState) -> anyhow::Result<i64> {
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM art_backfill_queue WHERE state = 'pending'")
            .fetch_one(&st.db)
            .await?;
    Ok(n)
}

async fn finish(st: &AppState) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE art_backfill_run
         SET status = 'finished', remaining = 0, updated_at = now()
         WHERE status = 'running'",
    )
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn alive(st: &AppState, gen: u64) -> bool {
    if !st.cfg.art_backfill {
        return false;
    }
    if st.art_backfill.gen.load(Ordering::SeqCst) != gen {
        return false;
    }
    match current(st).await {
        Ok(s) => s.status == "running",
        Err(_) => false,
    }
}

async fn wait_or_stop(st: &AppState, gen: u64) -> bool {
    let secs = st.cfg.art_backfill_every_secs;
    if secs == 0 {
        return alive(st, gen).await;
    }
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if !alive(st, gen).await {
            return false;
        }
        let left = until.saturating_duration_since(Instant::now());
        let slice = left.min(Duration::from_millis(250));
        tokio::select! {
            _ = sleep(slice) => {}
            _ = st.art_backfill.wake.notified() => {
                return alive(st, gen).await;
            }
        }
    }
    alive(st, gen).await
}
