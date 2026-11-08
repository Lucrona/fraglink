//! Read-only preview of a game merge. Nothing here inserts, updates, or deletes.
//!
//! Displayed hours are not the sum of the two games. The stats view keeps the later
//! imported counter and the higher Steam snapshot. This preview says so, and it
//! refuses a number when that choice cannot be made.

use crate::error::{ApiResult, AppError};

fn bad(code: &'static str, msg: &str) -> AppError {
    AppError::bad_request(code, msg)
}
use crate::AppState;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

const MAX_USERS: usize = 200;
const MAX_ROWS: i64 = 5_000;

struct GameRef {
    id: String,
    name: String,
    in_database: bool,
    on_list: bool,
}

#[derive(Clone)]
struct Session {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    dur: i64,
}

#[derive(Clone)]
struct Import {
    minutes: i64,
    carry: i64,
    at: DateTime<Utc>,
    created: DateTime<Utc>,
}

#[derive(Clone)]
struct Steam {
    minutes: i64,
    fetched: DateTime<Utc>,
}

pub async fn build(st: &AppState, source: &str, target: &str) -> ApiResult<Value> {
    let source = source.trim();
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return Err(bad("bad_target", "Pick the game that stays."));
    }
    if source == target {
        return Err(bad("bad_target", "Pick a different game."));
    }
    let from = lookup(st, source).await?;
    let into = lookup(st, target).await?;
    if !acceptable_target(&into) {
        return Err(bad(
            "bad_target",
            "That game isn't in the catalogue or the database.",
        ));
    }
    if !acceptable_source(&from) {
        return Err(bad(
            "not_found",
            "That game isn't in the catalogue or the database.",
        ));
    }
    let retired: Option<String> = sqlx::query_scalar(
        "SELECT successor_id FROM game_retirements WHERE game_id = $1 AND NOT partial",
    )
    .bind(source)
    .fetch_optional(&st.db)
    .await?;
    let mut blocked = Vec::new();
    if let Some(to) = retired {
        blocked.push(format!("Already merged or retired into {to}."));
    }
    if crate::retire::effective_id(st, target).await? != target {
        blocked.push("The game that would stay is itself retired.".into());
    }
    if let Some(why) =
        crate::suggested_merge::blocks_merge(st, source, target, &from.name, &into.name)
    {
        blocked.push(why.to_string());
    }

    let (moves, stays, users) = load(st, source, target).await?;
    let aliases: i64 =
        sqlx::query_scalar("SELECT count(*) FROM detection_aliases WHERE game_id = $1")
            .bind(source)
            .fetch_one(&st.db)
            .await?;

    let minted = source.starts_with("steam:")
        || source.starts_with("exe:")
        || source.starts_with("epic:")
        || source.starts_with("gog:");
    let target_minted = target.starts_with("steam:")
        || target.starts_with("exe:")
        || target.starts_with("epic:")
        || target.starts_with("gog:");
    if minted && !target_minted && (moves.is_empty() || stays.is_empty()) {
        blocked.push(
            "This Steam or program id is a catalogue link, not a merge. Saved rows stay where they are."
                .into(),
        );
    }
    let (projection, unknown) = project(&users);
    let mut warnings = warnings(&moves, &stays, &projection);
    if !from.in_database {
        warnings.insert(
            0,
            "This id has no database row. A merge would only record that future reports of it count as the game that stays. No hour rows would move."
                .into(),
        );
    }
    if unknown {
        blocked.push(
            "The shown total after a merge can't be calculated safely, so merging stays closed."
                .into(),
        );
    }
    let can_merge = blocked.is_empty();
    Ok(json!({
        "read_only": true,
        "can_merge": can_merge,
        "blocked": blocked,
        "stays": card(&into),
        "moves": card(&from),
        "moving_rows": rows_json(&moves),
        "staying_rows": rows_json(&stays),
        "aliases": aliases,
        "alias_note": "Detection fix-ups are not rewritten. A merge records that future reports of the old id count as the game that stays.",
        "projection": projection,
        "warnings": warnings,
    }))
}

fn card(g: &GameRef) -> Value {
    json!({
        "id": g.id,
        "name": g.name,
        "in_database": g.in_database,
        "on_list": g.on_list,
    })
}

struct Bag {
    players: i64,
    sessions: Vec<Session>,
    session_seconds: i64,
    imports: Vec<Import>,
    estimated_minutes: i64,
    estimated_rows: i64,
    steam: Vec<Steam>,
}

fn rows_json(s: &Bag) -> Value {
    json!({
        "players": s.players,
        "sessions": s.sessions.len(),
        "session_seconds": s.session_seconds,
        "imported_rows": s.imports.len(),
        "imported_minutes": s.imports.iter().map(|i| i.minutes).collect::<Vec<_>>(),
        "estimated_rows": s.estimated_rows,
        "estimated_minutes_stored": s.estimated_minutes,
        "steam_rows": s.steam.len(),
        "steam_minutes": s.steam.iter().map(|s| s.minutes).collect::<Vec<_>>(),
    })
}

async fn lookup(st: &AppState, id: &str) -> ApiResult<GameRef> {
    let in_database: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id = $1)")
        .bind(id)
        .fetch_one(&st.db)
        .await?;
    let on_list = crate::names::list_name(st, id).is_some();
    let name = crate::names::public_name(st, id)
        .await?
        .filter(|n| n != "a game")
        .or_else(|| crate::names::list_name(st, id))
        .unwrap_or_else(|| id.to_string());
    Ok(GameRef {
        id: id.to_string(),
        name,
        in_database,
        on_list,
    })
}

fn acceptable_source(g: &GameRef) -> bool {
    g.in_database
        || g.on_list
        || g.id.starts_with("steam:")
        || g.id.starts_with("exe:")
        || g.id.starts_with("epic:")
        || g.id.starts_with("gog:")
}

fn acceptable_target(g: &GameRef) -> bool {
    g.in_database || g.on_list
}

async fn load(
    st: &AppState,
    source: &str,
    target: &str,
) -> ApiResult<(Bag, Bag, Vec<(String, Bag, Bag)>)> {
    let ids = [source.to_string(), target.to_string()];
    let n: i64 = sqlx::query_scalar(
        "SELECT (SELECT count(*) FROM sessions WHERE game_id = ANY($1))
              + (SELECT count(*) FROM hours_entries WHERE game_id = ANY($1))
              + (SELECT count(*) FROM steam_playtime WHERE game_id = ANY($1))",
    )
    .bind(&ids[..])
    .fetch_one(&st.db)
    .await?;
    if n > MAX_ROWS {
        return Err(bad(
            "too_many",
            "Too many rows to preview safely. Nothing was changed.",
        ));
    }
    let sessions: Vec<(uuid::Uuid, String, DateTime<Utc>, DateTime<Utc>, i32)> = sqlx::query_as(
        "SELECT user_id, game_id, started_at, ended_at, duration_s FROM sessions WHERE game_id = ANY($1)",
    )
    .bind(&ids[..])
    .fetch_all(&st.db)
    .await?;
    let hours: Vec<(
        uuid::Uuid,
        String,
        String,
        i32,
        i32,
        DateTime<Utc>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT user_id, game_id, tier, minutes, carry_min, captured_at, created_at
         FROM hours_entries WHERE game_id = ANY($1)",
    )
    .bind(&ids[..])
    .fetch_all(&st.db)
    .await?;
    let steam: Vec<(uuid::Uuid, String, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT user_id, game_id, minutes_forever, fetched_at FROM steam_playtime WHERE game_id = ANY($1)",
    )
    .bind(&ids[..])
    .fetch_all(&st.db)
    .await?;
    let mut by: std::collections::BTreeMap<uuid::Uuid, (Bag, Bag)> =
        std::collections::BTreeMap::new();
    for (u, gid, start, end, dur) in sessions {
        let pair = by.entry(u).or_insert_with(|| (Bag::new(), Bag::new()));
        let bag = if gid == source {
            &mut pair.0
        } else {
            &mut pair.1
        };
        bag.sessions.push(Session {
            start,
            end,
            dur: i64::from(dur),
        });
        bag.session_seconds += i64::from(dur);
    }
    for (u, gid, tier, minutes, carry, at, created) in hours {
        let pair = by.entry(u).or_insert_with(|| (Bag::new(), Bag::new()));
        let bag = if gid == source {
            &mut pair.0
        } else {
            &mut pair.1
        };
        if tier == "imported" {
            bag.imports.push(Import {
                minutes: i64::from(minutes),
                carry: i64::from(carry),
                at,
                created,
            });
        } else if tier == "estimated" {
            bag.estimated_rows += 1;
            bag.estimated_minutes += i64::from(minutes);
        }
    }
    for (u, gid, minutes, fetched) in steam {
        let pair = by.entry(u).or_insert_with(|| (Bag::new(), Bag::new()));
        let bag = if gid == source {
            &mut pair.0
        } else {
            &mut pair.1
        };
        bag.steam.push(Steam {
            minutes: i64::from(minutes),
            fetched,
        });
    }
    if by.len() > MAX_USERS {
        return Err(bad(
            "too_many",
            "Too many players to preview safely. Nothing was changed.",
        ));
    }
    let mut users = Vec::new();
    let mut moves = Bag::new();
    let mut stays = Bag::new();
    for (u, (a, b)) in by {
        moves.add(&a);
        stays.add(&b);
        users.push((u.to_string(), a, b));
    }
    moves.players = users.iter().filter(|(_, a, _)| !a.is_empty()).count() as i64;
    stays.players = users.iter().filter(|(_, _, b)| !b.is_empty()).count() as i64;
    Ok((moves, stays, users))
}

fn project(users: &[(String, Bag, Bag)]) -> (Value, bool) {
    let mut unknown = false;
    let mut display = 0i64;
    let mut shown_imported = 0i64;
    let mut kept_imported = 0i64;
    let mut estimated = 0i64;
    let mut session_minutes = 0i64;
    let mut steam_used = 0i64;
    let mut notes = Vec::new();
    for (_, a, b) in users {
        let sessions: Vec<&Session> = a.sessions.iter().chain(b.sessions.iter()).collect();
        let seconds: i64 = sessions.iter().map(|s| s.dur).sum();
        let session_min = seconds / 60;
        session_minutes += session_min;
        let steam_rows: Vec<&Steam> = a.steam.iter().chain(b.steam.iter()).collect();
        let steam_min = steam_rows.iter().map(|s| s.minutes).max().unwrap_or(0);
        steam_used += steam_min;
        if steam_rows.len() > 1 {
            let sum: i64 = steam_rows.iter().map(|s| s.minutes).sum();
            if sum != steam_min {
                notes.push(
                    "Two Steam snapshots are not added. The shown tracked figure uses the higher one.",
                );
            }
        }
        let steam_from = steam_rows.iter().map(|s| s.fetched).max();
        let steam_after = steam_from.map(|t| clip_minutes(&sessions, t)).unwrap_or(0);
        let tracked = session_min.max(steam_min + steam_after);
        let mut imports: Vec<&Import> = a.imports.iter().chain(b.imports.iter()).collect();
        imports.sort_by(|x, y| y.at.cmp(&x.at).then(y.created.cmp(&x.created)));
        estimated += a.estimated_minutes + b.estimated_minutes;
        let shown = if imports.is_empty() {
            tracked
        } else {
            if imports.len() > 1
                && imports[0].at == imports[1].at
                && imports[0].created == imports[1].created
                && imports[0].minutes != imports[1].minutes
            {
                unknown = true;
            }
            let win = imports[0];
            shown_imported += win.minutes;
            kept_imported += imports.iter().skip(1).map(|i| i.minutes).sum::<i64>();
            if imports.len() > 1 {
                notes.push(
                    "Two imported counters are not added. The later one is the shown counter. The earlier one stays stored.",
                );
            }
            let after = clip_minutes(&sessions, win.at);
            tracked.max(win.minutes + win.carry + after)
        };
        display += shown;
    }
    notes.sort();
    notes.dedup();
    let status = if unknown { "unknown" } else { "known" };
    (
        json!({
            "status": status,
            "display_minutes": if unknown { Value::Null } else { json!(display) },
            "session_minutes": session_minutes,
            "steam_minutes_used": steam_used,
            "imported_minutes_shown": if unknown { Value::Null } else { json!(shown_imported) },
            "imported_minutes_not_added": kept_imported,
            "estimated_minutes_stored": estimated,
            "notes": notes,
            "rule": "Shown hours use the later imported counter and the higher Steam snapshot. They are not the sum of both games. Estimated minutes stay stored and are not part of the shown total.",
        }),
        unknown,
    )
}

fn clip_minutes(sessions: &[&Session], from: DateTime<Utc>) -> i64 {
    let seconds: i64 = sessions
        .iter()
        .map(|s| {
            if s.end <= from {
                return 0;
            }
            let start = s.start.max(from);
            let span = (s.end - start).num_seconds().max(0);
            s.dur.min(span)
        })
        .sum();
    seconds / 60
}

fn warnings(moves: &Bag, stays: &Bag, projection: &Value) -> Vec<String> {
    let mut w = Vec::new();
    if moves.sessions.is_empty()
        && moves.imports.is_empty()
        && moves.estimated_rows == 0
        && moves.steam.is_empty()
    {
        w.push("The entry that would move has no sessions, imported hours, estimated hours, or Steam minutes.".into());
    }
    if projection["imported_minutes_not_added"]
        .as_i64()
        .unwrap_or(0)
        > 0
    {
        w.push(format!(
            "Imported minutes that stay stored but are not added into the shown total: {}.",
            projection["imported_minutes_not_added"]
        ));
    }
    if moves.estimated_minutes + stays.estimated_minutes > 0 {
        w.push(
            "Estimated minutes stay stored. They are already excluded from the shown total.".into(),
        );
    }
    if projection["status"] == "unknown" {
        w.push(
            "Two imported counters share a timestamp and disagree, so the shown total is not predicted."
                .into(),
        );
    }
    w
}

impl Bag {
    fn new() -> Self {
        Self {
            players: 0,
            sessions: Vec::new(),
            session_seconds: 0,
            imports: Vec::new(),
            estimated_minutes: 0,
            estimated_rows: 0,
            steam: Vec::new(),
        }
    }
    fn is_empty(&self) -> bool {
        self.sessions.is_empty()
            && self.imports.is_empty()
            && self.estimated_rows == 0
            && self.steam.is_empty()
    }
    fn add(&mut self, o: &Bag) {
        self.session_seconds += o.session_seconds;
        self.sessions.extend(o.sessions.iter().cloned());
        self.imports.extend(o.imports.iter().cloned());
        self.estimated_minutes += o.estimated_minutes;
        self.estimated_rows += o.estimated_rows;
        self.steam.extend(o.steam.iter().cloned());
    }
}
