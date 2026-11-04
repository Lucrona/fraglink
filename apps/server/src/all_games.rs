//! Admin > Games > All games: every game Fraglink knows about in one paged list.
//!
//! The universe is the downloaded games list plus the built-in list, folder rules and the
//! `games` table (Steam names, approved programs), joined with staff edits, fix-ups,
//! retirements, status checks, totals and the audit log. Built in memory per request
//! (about 11k games, a few ms) and paged 50 at a time.
//!
//! Also keeps `game_list_seen` / `game_list_changes` up to date after each list refresh.

use std::collections::{HashMap, HashSet};

use axum::{
    extract::{Path, Query, State},
    Json,
};
use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    admin::{Role, Staff},
    error::ApiResult,
    AppState,
};

pub const PAGE: usize = 50;

type Dt = DateTime<Utc>;

#[derive(Deserialize, Default)]
pub struct AllQ {
    q: Option<String>,
    src: Option<String>,
    status: Option<String>,
    flag: Option<String>,
    sort: Option<String>,
    offset: Option<usize>,
}

#[derive(Default)]
struct Row {
    id: String,
    official: String,
    shown: Option<String>,
    verified: bool,
    sources: Vec<&'static str>,
    appids: Vec<u32>,
    exes: Vec<String>,
    folders: usize,
    aliases: Vec<String>,
    fixups: usize,
    players: i64,
    total_min: i64,
    created: Option<Dt>,
    first_seen: Option<Dt>,
    baseline: bool,
    removed: Option<Dt>,
    list_changed: Option<Dt>,
    updated: Option<Dt>,
    by: Option<String>,
    status: &'static str,
    successor: Option<String>,
    lifecycle: Option<String>,
    partial: bool,
    edited: bool,
}

impl Row {
    fn name(&self) -> &str {
        self.shown.as_deref().unwrap_or(&self.official)
    }
    fn touch(&mut self, at: Option<Dt>, by: Option<&str>) {
        if let Some(at) = at {
            if self.updated.is_none_or(|u| at > u) {
                self.updated = Some(at);
                self.by = by.map(str::to_string);
            }
        }
    }
    fn added(&self) -> Option<Dt> {
        match (self.created, self.first_seen.filter(|_| !self.baseline)) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b).or(self.first_seen),
        }
    }
    fn matches(&self, q: &str) -> bool {
        self.id.to_lowercase().contains(q)
            || self.official.to_lowercase().contains(q)
            || self
                .shown
                .as_ref()
                .is_some_and(|s| s.to_lowercase().contains(q))
            || self.aliases.iter().any(|a| a.to_lowercase().contains(q))
            || self.exes.iter().any(|e| e.contains(q))
            || self.appids.iter().any(|a| a.to_string() == q)
    }
    fn json(&self) -> Value {
        let base = |e: &String| e.rsplit('/').next().unwrap_or(e).to_string();
        let mut shown_exes: Vec<String> = vec![];
        for e in self.exes.iter().map(base) {
            if !shown_exes.contains(&e) {
                shown_exes.push(e);
            }
        }
        json!({
            "id": self.id,
            "name": self.name(),
            "official": self.official,
            "renamed": self.shown.is_some(),
            "verified": self.verified,
            "sources": self.sources,
            "steam_appids": self.appids,
            "exes": shown_exes.iter().take(3).collect::<Vec<_>>(),
            "exe_count": shown_exes.len(),
            "folders": self.folders,
            "fixups": self.fixups,
            "players": self.players,
            "total_min": self.total_min,
            "added": self.added(),
            "added_kind": if self.created.is_some() && self.added() == self.created { "fraglink" }
                else if self.baseline { "before_tracking" } else if self.first_seen.is_some() { "list" } else { "" },
            "removed_from_list": self.removed,
            "list_changed": self.list_changed,
            "updated": self.updated,
            "by": self.by,
            "status": self.status,
            "successor": self.successor,
            "lifecycle": self.lifecycle,
            "partial": self.partial,
            "edited": self.edited,
            "icon": crate::icons::icon_path(&self.id, 32),
        })
    }
}

fn push_appid(v: &mut Vec<u32>, a: u32) {
    if a > 0 && !v.contains(&a) {
        v.push(a);
    }
}

/// Every game, with sources and status, unsorted.
async fn universe(st: &AppState) -> ApiResult<Vec<Row>> {
    let mut rows: HashMap<String, Row> = HashMap::new();
    fn row<'a>(rows: &'a mut HashMap<String, Row>, id: &str) -> &'a mut Row {
        rows.entry(id.to_string()).or_insert_with(|| Row {
            id: id.to_string(),
            status: "active",
            ..Default::default()
        })
    }
    let skip = |id: &str| id.starts_with("manual:") || id.is_empty();

    // the games list (downloaded, or the bundled copy before the first download)
    if let Some(det) = crate::games::detectable_db(st) {
        for g in det.games() {
            let r = row(&mut rows, &g.slug);
            r.official = g.name.clone();
            r.verified = true;
            r.exes.extend(g.exes.iter().map(|e| e.to_lowercase()));
            r.folders += g.exes.iter().filter(|e| e.contains('/')).count();
            r.sources.push("games list");
        }
    }
    for g in st.sigs.games() {
        let r = row(&mut rows, &g.id);
        if r.official.is_empty() {
            r.official = g.name.clone();
        }
        r.verified = true;
        r.exes
            .extend(g.exes.iter().chain(&g.path_exes).map(|e| e.to_lowercase()));
        r.folders += g.path_hints.len();
        for a in &g.steam_appids {
            push_appid(&mut r.appids, *a);
        }
        r.sources.push("built-in");
    }
    for (id, name) in st.games.override_names(&st.cfg.data_dir).iter() {
        let r = row(&mut rows, id);
        if r.official.is_empty() {
            r.official = name.clone();
        }
        r.verified = true;
        r.sources.push("folder rule");
    }
    let db: Vec<(String, String, Option<i32>, bool, Option<Dt>, Option<Dt>)> = sqlx::query_as(
        "SELECT id, name, steam_appid, verified, created_at, updated_at FROM games WHERE id NOT LIKE 'manual:%'",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, name, appid, verified, created, updated) in db {
        let r = row(&mut rows, &id);
        if r.official.is_empty() {
            r.official = name;
        }
        r.verified |= verified;
        if let Some(a) = appid {
            push_appid(&mut r.appids, a as u32);
        }
        r.created = created;
        r.touch(updated, Some("Steam sync"));
        if let Some(exe) = id.strip_prefix("exe:") {
            r.exes.push(exe.to_lowercase());
            if verified {
                r.sources.push("approved");
            }
        }
        if let Some(a) = id.strip_prefix("steam:").and_then(|a| a.parse().ok()) {
            push_appid(&mut r.appids, a);
        }
    }

    // staff edits
    let edits: HashMap<String, (Dt, Option<String>)> = sqlx::query_as::<_, (String, Dt, Option<String>)>(
        "SELECT o.game_id, o.updated_at, u.handle FROM game_overrides o LEFT JOIN users u ON u.id = o.updated_by",
    )
    .fetch_all(&st.db)
    .await?
    .into_iter()
    .map(|(g, t, h)| (g, (t, h)))
    .collect();
    for (id, o) in crate::game_overrides::current(st).await?.iter() {
        if skip(id) {
            continue;
        }
        let r = row(&mut rows, id);
        r.edited = true;
        r.shown = o.name.clone().filter(|n| n != &r.official);
        if o.hidden {
            r.status = "hidden";
        }
        r.exes.extend(o.extra_exes.iter().map(|e| e.to_lowercase()));
        if !o.extra_exes.is_empty() {
            r.sources.push("extra exe");
        }
        r.folders += o.path_hints.len();
        r.aliases = o.aliases.clone();
        if let Some(a) = o.steam_appid {
            push_appid(&mut r.appids, a as u32);
        }
        if let Some((t, h)) = edits.get(id) {
            r.touch(Some(*t), h.as_deref());
        }
    }

    // fix-ups (detection rules)
    let fix: Vec<(String, String, String, Dt, Option<String>)> = sqlx::query_as(
        "SELECT d.game_id, d.kind, d.pattern, d.updated_at, u.handle
         FROM detection_aliases d LEFT JOIN users u ON u.id = d.updated_by",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, kind, pattern, t, h) in fix {
        if skip(&id) {
            continue;
        }
        let r = row(&mut rows, &id);
        r.fixups += 1;
        if kind == "steam" {
            if let Ok(a) = pattern.parse() {
                push_appid(&mut r.appids, a);
            }
        }
        if kind == "path" && !r.sources.contains(&"folder rule") {
            r.sources.push("folder rule");
        }
        r.touch(Some(t), h.as_deref());
    }

    // retirements and merges
    let ret: Vec<(String, String, bool, bool, Dt, Option<String>)> = sqlx::query_as(
        "SELECT r.game_id, r.successor_id, r.partial, r.merged, r.retired_at, u.handle
         FROM game_retirements r LEFT JOIN users u ON u.id = r.retired_by",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, succ, partial, merged, t, h) in ret {
        if skip(&id) {
            continue;
        }
        let r = row(&mut rows, &id);
        if partial {
            r.partial = true;
        } else {
            r.status = if merged { "merged" } else { "retired" };
            r.successor = Some(succ);
        }
        r.touch(Some(t), h.as_deref());
    }

    // status checks
    let status: Vec<(String, String, Dt, Option<String>)> =
        sqlx::query_as("SELECT game_id, lifecycle, updated_at, checked_by FROM game_status")
            .fetch_all(&st.db)
            .await?;
    for (id, life, t, by) in status {
        if let Some(r) = rows.get_mut(&id) {
            if life != "live" {
                r.lifecycle = Some(life);
            }
            r.touch(Some(t), by.as_deref());
        }
    }

    // games list tracking
    let seen: Vec<(String, Dt, bool, Option<Dt>)> =
        sqlx::query_as("SELECT game_id, first_seen, baseline, removed_at FROM game_list_seen")
            .fetch_all(&st.db)
            .await?;
    for (id, first, baseline, removed) in seen {
        if let Some(r) = rows.get_mut(&id) {
            r.first_seen = Some(first);
            r.baseline = baseline;
            r.removed = removed;
        }
    }
    let changes: Vec<(String, Dt)> = sqlx::query_as(
        "SELECT DISTINCT ON (game_id) game_id, created_at FROM game_list_changes ORDER BY game_id, created_at DESC",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, t) in changes {
        if let Some(r) = rows.get_mut(&id) {
            r.list_changed = Some(t);
            r.touch(Some(t), Some("Games list"));
        }
    }

    // the audit log: last change by anyone (staff or Steam sync)
    let audit: Vec<(String, String, Dt)> = sqlx::query_as(
        "SELECT DISTINCT ON (t) t, actor_handle, created_at FROM (
           SELECT CASE WHEN target LIKE 'game:%' THEN substr(target, 6) ELSE target END AS t,
                  actor_handle, created_at
           FROM admin_audit WHERE action LIKE 'game.%' AND target IS NOT NULL) a
         ORDER BY t, created_at DESC",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, h, t) in audit {
        if let Some(r) = rows.get_mut(&id) {
            r.touch(Some(t), Some(&h));
        }
    }

    // totals
    let totals: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT game_id, count(*) FILTER (WHERE display_min > 0 OR minutes_7d > 0),
                coalesce(sum(display_min), 0)::bigint
         FROM user_game_stats GROUP BY game_id",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, p, m) in totals {
        if let Some(r) = rows.get_mut(&id) {
            r.players = p;
            r.total_min = m;
        }
    }

    let mut out: Vec<Row> = rows
        .into_values()
        .filter(|r| !skip(&r.id))
        .map(|mut r| {
            if r.official.is_empty() {
                r.official = r.id.clone();
            }
            if !r.appids.is_empty() {
                r.sources.insert(0, "steam");
            }
            if r.fixups > 0 {
                r.sources.push("fix-up");
            }
            if r.sources.is_empty() {
                r.sources.push("unlisted");
            }
            let mut seen = HashSet::new();
            r.sources.retain(|s| seen.insert(*s));
            let mut seen = HashSet::new();
            r.exes.retain(|e| seen.insert(e.clone()));
            r
        })
        .collect();
    out.shrink_to_fit();
    Ok(out)
}

fn status_keys(r: &Row) -> Vec<&'static str> {
    let mut k = vec![r.status];
    if r.partial {
        k.push("partial");
    }
    if r.removed.is_some() {
        k.push("removed");
    }
    match r.lifecycle.as_deref() {
        Some("beta") => k.push("beta"),
        Some("test") => k.push("test"),
        Some("early_access") => k.push("early_access"),
        Some("ended") => k.push("ended"),
        _ => {}
    }
    k
}

fn flag_ok(r: &Row, flag: &str, now: Dt) -> bool {
    match flag {
        "hours" => r.total_min > 0,
        "edited" => r.edited,
        "unverified" => !r.verified,
        "new" => r.added().is_some_and(|a| now - a < Duration::days(7)) && !r.baseline,
        "changed" => r.list_changed.is_some_and(|a| now - a < Duration::days(7)),
        _ => true,
    }
}

/// `GET /api/v1/admin/games-all?q=&src=&status=&flag=&sort=&offset=`
pub async fn list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<AllQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let all = universe(&st).await?;
    let everything = all.len();
    let now = Utc::now();
    let needle = q.q.as_deref().unwrap_or("").trim().to_lowercase();
    let flags: Vec<&str> = q
        .flag
        .as_deref()
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    let base: Vec<Row> = all
        .into_iter()
        .filter(|r| needle.is_empty() || r.matches(&needle))
        .filter(|r| flags.iter().all(|f| flag_ok(r, f, now)))
        .collect();
    let src = q.src.as_deref().filter(|v| !v.is_empty() && *v != "all");
    let status = q.status.as_deref().filter(|v| !v.is_empty() && *v != "all");
    // each chip row counts with the other row's choice applied
    let mut by_src: HashMap<&str, usize> = HashMap::new();
    let mut by_status: HashMap<&str, usize> = HashMap::new();
    for r in &base {
        if status.is_none_or(|v| status_keys(r).contains(&v)) {
            for s in &r.sources {
                *by_src.entry(s).or_default() += 1;
            }
        }
        if src.is_none_or(|v| r.sources.contains(&v)) {
            for k in status_keys(r) {
                *by_status.entry(k).or_default() += 1;
            }
        }
    }
    let mut hits: Vec<Row> = base
        .into_iter()
        .filter(|r| src.is_none_or(|v| r.sources.contains(&v)))
        .filter(|r| status.is_none_or(|v| status_keys(r).contains(&v)))
        .collect();
    let by_name = |a: &Row, b: &Row| {
        a.name()
            .to_lowercase()
            .cmp(&b.name().to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    };
    match q.sort.as_deref().unwrap_or("updated") {
        "name" => hits.sort_by(by_name),
        "players" => hits.sort_by(|a, b| b.players.cmp(&a.players).then_with(|| by_name(a, b))),
        "hours" => hits.sort_by(|a, b| b.total_min.cmp(&a.total_min).then_with(|| by_name(a, b))),
        "added" => hits.sort_by(|a, b| b.added().cmp(&a.added()).then_with(|| by_name(a, b))),
        _ => hits.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| by_name(a, b))),
    }
    let total = hits.len();
    let offset = q
        .offset
        .unwrap_or(0)
        .min(total.saturating_sub(1) / PAGE * PAGE);
    let page: Vec<Value> = hits.iter().skip(offset).take(PAGE).map(Row::json).collect();
    Ok(Json(json!({
        "total": total,
        "everything": everything,
        "offset": offset,
        "page_size": PAGE,
        "rows": page,
        "counts": { "src": by_src, "status": by_status },
        "meta": meta(&st).await?,
    })))
}

/// Where the list comes from and when each part last changed (the freshness strip).
async fn meta(st: &AppState) -> ApiResult<Value> {
    let jobs: Vec<(String, Dt, Option<Dt>, Option<String>)> =
        sqlx::query_as("SELECT job, last_run_at, last_ok_at, detail FROM job_runs")
            .fetch_all(&st.db)
            .await?;
    let job = |name: &str| {
        jobs.iter()
            .find(|j| j.0 == name)
            .map(|j| json!({ "last_run_at": j.1, "last_ok_at": j.2, "detail": j.3 }))
    };
    let (steam_rows, steam_last): (i64, Option<Dt>) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE steam_appid IS NOT NULL), max(updated_at) FROM games",
    )
    .fetch_one(&st.db)
    .await?;
    let steam_account: Option<Dt> = sqlx::query_scalar(
        "SELECT max(last_sync_at) FROM linked_accounts WHERE provider = 'steam'",
    )
    .fetch_one(&st.db)
    .await?;
    let (edits, edit_last): (i64, Option<Dt>) =
        sqlx::query_as("SELECT count(*), max(updated_at) FROM game_overrides")
            .fetch_one(&st.db)
            .await?;
    let (fixups, fix_last): (i64, Option<Dt>) =
        sqlx::query_as("SELECT count(*), max(updated_at) FROM detection_aliases")
            .fetch_one(&st.db)
            .await?;
    let (approved, approved_last): (i64, Option<Dt>) = sqlx::query_as(
        "SELECT count(*), max(created_at) FROM games WHERE id LIKE 'exe:%' AND verified",
    )
    .fetch_one(&st.db)
    .await?;
    let week: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, count(*) FROM game_list_changes WHERE created_at > now() - interval '7 days' GROUP BY kind",
    )
    .fetch_all(&st.db)
    .await?;
    let list = st.games.current();
    Ok(json!({
        "games_list": list.map(|b| json!({ "updated": b.updated, "games": b.games })),
        "games_list_job": job("games_list"),
        "list_changes_7d": week.into_iter().collect::<HashMap<_, _>>(),
        "builtin": { "version": st.sigs.version, "games": st.sigs.games().len() },
        "folder_rules": st.games.override_names(&st.cfg.data_dir).len(),
        "steam": { "job": job("steam_sync"), "games": steam_rows, "last_change": steam_last, "last_account_sync": steam_account },
        "edits": { "count": edits, "last": edit_last },
        "fixups": { "count": fixups, "last": fix_last },
        "approved": { "count": approved, "last": approved_last },
    }))
}

/// `GET /api/v1/admin/games/{id}/history`: the audit log, list changes and key dates
/// for one game, newest first.
pub async fn history(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let mut ev: Vec<(Dt, Value)> = vec![];
    let audit: Vec<(
        Dt,
        String,
        String,
        Option<String>,
        Option<Value>,
        Option<Value>,
    )> = sqlx::query_as(
        "SELECT created_at, actor_handle, action, target, before, after FROM admin_audit
         WHERE target = $1 OR target = 'game:' || $1
            OR (action LIKE 'detection.%' AND (after->>'game_id' = $1 OR before->>'game_id' = $1))
            OR (action = 'game.merge' AND after->>'into' = $1)
            OR (action = 'unknown_game.map' AND after->>'to' = $1)
         ORDER BY created_at DESC LIMIT 200",
    )
    .bind(&id)
    .fetch_all(&st.db)
    .await?;
    for (t, who, action, target, before, after) in audit {
        ev.push((
            t,
            json!({ "at": t, "kind": "audit", "by": who, "action": action, "target": target, "before": before, "after": after }),
        ));
    }
    let list: Vec<(Dt, String, Option<Value>, Option<Value>, Option<String>)> = sqlx::query_as(
        "SELECT created_at, kind, before, after, list_updated FROM game_list_changes
         WHERE game_id = $1 ORDER BY created_at DESC LIMIT 100",
    )
    .bind(&id)
    .fetch_all(&st.db)
    .await?;
    for (t, kind, before, after, version) in list {
        ev.push((
            t,
            json!({ "at": t, "kind": "list", "by": "Games list", "action": kind, "before": before, "after": after, "list_updated": version }),
        ));
    }
    let seen: Option<(Dt, bool)> =
        sqlx::query_as("SELECT first_seen, baseline FROM game_list_seen WHERE game_id = $1")
            .bind(&id)
            .fetch_optional(&st.db)
            .await?;
    if let Some((t, baseline)) = seen {
        ev.push((
            t,
            json!({ "at": t, "kind": "seen", "by": "Games list", "action": if baseline { "on_list_when_tracking_began" } else { "first_seen" } }),
        ));
    }
    let created: Option<Option<Dt>> =
        sqlx::query_scalar("SELECT created_at FROM games WHERE id = $1")
            .bind(&id)
            .fetch_optional(&st.db)
            .await?;
    if let Some(Some(t)) = created {
        ev.push((
            t,
            json!({ "at": t, "kind": "created", "by": null, "action": "first_on_fraglink" }),
        ));
    }
    let check: Option<(Option<Dt>, Option<String>)> =
        sqlx::query_as("SELECT checked_at, checked_by FROM game_status WHERE game_id = $1")
            .bind(&id)
            .fetch_optional(&st.db)
            .await?;
    if let Some((Some(t), by)) = check {
        ev.push((
            t,
            json!({ "at": t, "kind": "check", "by": by, "action": "status_checked" }),
        ));
    }
    ev.sort_by_key(|a| std::cmp::Reverse(a.0));
    Ok(Json(
        json!({ "id": id, "events": ev.into_iter().map(|e| e.1).collect::<Vec<_>>() }),
    ))
}

/// After a games list refresh: remember first-seen dates and write what changed to
/// `game_list_changes`. The first run only records a baseline. Recorded as job `games_list`.
pub async fn track_list(st: &AppState) {
    let Some(det) = crate::games::detectable_db(st) else {
        return;
    };
    let updated = det.updated.clone();
    let mut list: Vec<(String, String, Vec<String>)> = det
        .games()
        .iter()
        .map(|g| {
            let mut e: Vec<String> = g.exes.iter().map(|e| e.to_lowercase()).collect();
            e.sort();
            e.dedup();
            (g.slug.clone(), g.name.clone(), e)
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));
    list.dedup_by(|a, b| a.0 == b.0);
    match track(st, &list, &updated).await {
        Ok(d) => crate::admin::record_job(st, "games_list", Some(&d), true).await,
        Err(e) => {
            tracing::warn!("games list tracking: {e:?}");
            crate::admin::record_job(st, "games_list", Some("tracking failed"), false).await;
        }
    }
}

async fn track(
    st: &AppState,
    list: &[(String, String, Vec<String>)],
    updated: &str,
) -> Result<String, sqlx::Error> {
    let mut tx = st.db.begin().await?;
    // one tracker at a time
    sqlx::query("SELECT pg_advisory_xact_lock(530053)")
        .execute(&mut *tx)
        .await?;
    let old: Vec<(String, String, Vec<String>, Option<Dt>)> =
        sqlx::query_as("SELECT game_id, name, exes, removed_at FROM game_list_seen")
            .fetch_all(&mut *tx)
            .await?;
    let first = old.is_empty();
    let old: HashMap<String, (String, Vec<String>, Option<Dt>)> = old
        .into_iter()
        .map(|(id, n, mut e, r)| {
            e.sort();
            (id, (n, e, r))
        })
        .collect();
    let mut upsert: Vec<&(String, String, Vec<String>)> = vec![];
    // (game, kind, before, after)
    let mut changes: Vec<(String, &str, Value, Value)> = vec![];
    for g in list {
        match old.get(&g.0) {
            None => {
                upsert.push(g);
                if !first {
                    changes.push((
                        g.0.clone(),
                        "new",
                        Value::Null,
                        json!({ "name": g.1, "exes": g.2 }),
                    ));
                }
            }
            Some((n, e, removed)) => {
                let mut touched = false;
                if removed.is_some() {
                    changes.push((g.0.clone(), "back", Value::Null, json!({ "name": g.1 })));
                    touched = true;
                }
                if n != &g.1 {
                    changes.push((
                        g.0.clone(),
                        "renamed",
                        json!({ "name": n }),
                        json!({ "name": g.1 }),
                    ));
                    touched = true;
                }
                if e != &g.2 {
                    changes.push((
                        g.0.clone(),
                        "exes",
                        json!({ "exes": e }),
                        json!({ "exes": g.2 }),
                    ));
                    touched = true;
                }
                if touched {
                    upsert.push(g);
                }
            }
        }
    }
    let present: HashSet<&str> = list.iter().map(|g| g.0.as_str()).collect();
    let active = old.values().filter(|o| o.2.is_none()).count();
    let mut gone: Vec<String> = old
        .iter()
        .filter(|(id, o)| o.2.is_none() && !present.contains(id.as_str()))
        .map(|(id, _)| id.clone())
        .collect();
    // a broken download that lost most of the list isn't a mass removal
    if list.len() * 2 < active {
        gone.clear();
    }
    for id in &gone {
        let name = old.get(id).map(|o| o.0.clone()).unwrap_or_default();
        changes.push((id.clone(), "removed", json!({ "name": name }), Value::Null));
    }
    for chunk in upsert.chunks(2000) {
        let ids: Vec<&str> = chunk.iter().map(|g| g.0.as_str()).collect();
        let names: Vec<&str> = chunk.iter().map(|g| g.1.as_str()).collect();
        // text[] per row, passed as one joined string (Postgres has no array of arrays here)
        let exes: Vec<String> = chunk.iter().map(|g| g.2.join("\n")).collect();
        sqlx::query(
            "INSERT INTO game_list_seen(game_id, name, exes, baseline)
             SELECT i, n, CASE WHEN e = '' THEN '{}'::text[] ELSE string_to_array(e, E'\\n') END, $4
             FROM unnest($1::text[], $2::text[], $3::text[]) AS t(i, n, e)
             ON CONFLICT (game_id) DO UPDATE SET name = excluded.name, exes = excluded.exes,
               removed_at = NULL, updated_at = now()",
        )
        .bind(&ids)
        .bind(&names)
        .bind(&exes)
        .bind(first)
        .execute(&mut *tx)
        .await?;
    }
    if !gone.is_empty() {
        sqlx::query("UPDATE game_list_seen SET removed_at = now(), updated_at = now() WHERE game_id = ANY($1)")
            .bind(&gone)
            .execute(&mut *tx)
            .await?;
    }
    for chunk in changes.chunks(2000) {
        let ids: Vec<&str> = chunk.iter().map(|c| c.0.as_str()).collect();
        let kinds: Vec<&str> = chunk.iter().map(|c| c.1).collect();
        let before: Vec<Value> = chunk.iter().map(|c| c.2.clone()).collect();
        let after: Vec<Value> = chunk.iter().map(|c| c.3.clone()).collect();
        sqlx::query(
            "INSERT INTO game_list_changes(game_id, kind, before, after, list_updated)
             SELECT i, k, NULLIF(b, 'null'::jsonb), NULLIF(a, 'null'::jsonb), $5
             FROM unnest($1::text[], $2::text[], $3::jsonb[], $4::jsonb[]) AS t(i, k, b, a)",
        )
        .bind(&ids)
        .bind(&kinds)
        .bind(&before)
        .bind(&after)
        .bind(updated)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let count = |k: &str| changes.iter().filter(|c| c.1 == k).count();
    Ok(if first {
        format!("{} games (first run: baseline)", list.len())
    } else {
        format!(
            "{} games; {} new, {} renamed, {} exe changes, {} removed",
            list.len(),
            count("new") + count("back"),
            count("renamed"),
            count("exes"),
            count("removed")
        )
    })
}
