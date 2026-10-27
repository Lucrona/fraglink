//! WoW /played from the Fraglink addon (see `fraglink_core::wow_played`).
//!
//! The app reads the addon's SavedVariables after WoW closes and posts each character's
//! signed entry to `POST /api/v1/hours/wow_played`. Every entry is checked:
//!
//! - the checksum must match (an edited file is rejected and flagged for staff);
//! - the BattleTag saved by the addon must be the user's linked Battle.net BattleTag
//!   (when one is linked; a mismatch is rejected and flagged);
//! - a character's /played never goes down, and can't grow faster than real time.
//!
//! Characters also on the user's Battle.net account (`wow.profile` scope, fetched when
//! they link) are "verified": only those count on leaderboards (`hours::WEEK_CTE`). The
//! rest show on the profile as self-reported. The game's total is the sum of all its
//! characters, stored as one `hours_entries` row (source `wow_addon`) whenever it changes.

use crate::api;
use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeZone, Utc};
use fraglink_core::wow_played::{verify, CharEntry, SigCheck};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

/// WoW flavours that take /played uploads. Each is kept separate, and the per-character
/// checks, keep-the-higher rule and menu-time carry (`record_total`) apply to every entry
/// here, so a new flavour (Classic) only needs adding here and in the app's `FLAVOURS`.
pub const GAMES: [(&str, &str); 2] = [
    ("wow", "World of Warcraft"),
    ("wow_forever", "World of Warcraft: Forever"),
];
/// Flavours taking /played uploads now. Forever isn't released: app-tracked only.
pub const UPLOADS_OPEN: [&str; 1] = ["wow"];
const REGIONS: [&str; 4] = ["us", "eu", "kr", "tw"];
/// Slack for clocks and the game's rounding when comparing growth with real time.
const SLACK_S: i64 = 300;
/// One character's /played can't be more than this (same cap as other imports).
const MAX_PLAYED_S: i64 = crate::api::IMPORT_MAX_MIN * 60;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/hours/wow_played", post(upload))
        .route("/api/v1/hours/wow_played/file", post(upload_file))
        .route("/api/v1/me/wow", get(mine))
        .route("/api/v1/me/wow/{id}", post(set_hidden))
        .route("/api/v1/wow/verify/fetch", post(verify_fetch))
        .route("/api/v1/wow/verify/match", post(verify_match))
        .route("/api/v1/wow/verify/signatures", get(verify_signatures))
}

// ------------------------------------------------------------------ verify page
//
// `/connect/battlenet/verify` runs these one by one and shows each real step. The
// Battle.net user token from the link (valid about a day) is kept in memory only, for 15
// minutes, until the page asks for the characters.

type Stash = std::sync::Mutex<std::collections::HashMap<Uuid, (String, std::time::Instant)>>;

fn stash() -> &'static Stash {
    static S: std::sync::OnceLock<Stash> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

pub fn stash_token(uid: Uuid, token: &str) {
    let mut m = stash().lock().unwrap();
    m.retain(|_, v| v.1.elapsed() < Duration::from_secs(900));
    m.insert(uid, (token.to_string(), std::time::Instant::now()));
}

async fn verify_fetch(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let tok = stash()
        .lock()
        .unwrap()
        .remove(&a.user.id)
        .filter(|t| t.1.elapsed() < Duration::from_secs(900));
    let Some((tok, _)) = tok else {
        return Ok(Json(json!({ "need_connect": true })));
    };
    match sync_characters(&st, a.user.id, &tok).await {
        Some(rows) => {
            let mut by: std::collections::BTreeMap<String, i64> = Default::default();
            for (g, r) in rows {
                *by.entry(format!("{g}:{r}")).or_default() += 1;
            }
            let total: i64 = by.values().sum();
            Ok(Json(json!({ "characters": total, "by_region": by })))
        }
        None => Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "bnet_unreachable",
            "Battle.net didn't answer. Try again in a minute.",
        )),
    }
}

async fn verify_match(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    reverify(&st, a.user.id).await;
    let (uploaded, verified): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE verified) FROM wow_characters WHERE user_id = $1",
    )
    .bind(a.user.id)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({ "uploaded": uploaded, "verified": verified })))
}

/// Every stored entry passed its checksum and BattleTag check on upload; this reports
/// how many did and the entries refused in the last 30 days.
async fn verify_signatures(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let ok: i64 = sqlx::query_scalar("SELECT count(*) FROM wow_characters WHERE user_id = $1")
        .bind(a.user.id)
        .fetch_one(&st.db)
        .await?;
    let refused: Vec<(String, String)> = sqlx::query_as(
        "SELECT char_label, kind FROM wow_flags WHERE user_id = $1
           AND kind IN ('wow_checksum', 'wow_battletag') AND created_at > now() - interval '30 days'
         ORDER BY created_at DESC LIMIT 20",
    )
    .bind(a.user.id)
    .fetch_all(&st.db)
    .await?;
    Ok(Json(json!({ "signed_ok": ok,
        "refused": refused.into_iter().map(|(c, k)| json!({ "character": c, "reason": k })).collect::<Vec<_>>() })))
}

/// Blizzard's realm slug: "Area 52" -> "area-52", "Kel'Thas" -> "kelthas".
pub fn realm_slug(realm: &str) -> String {
    let mut out = String::new();
    for c in realm.trim().to_lowercase().chars() {
        if c == '\'' || c == '(' || c == ')' {
            continue;
        }
        if c.is_whitespace() || c == '-' {
            if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        } else {
            out.push(c);
        }
    }
    out.trim_end_matches('-').to_string()
}

/// Why one entry was refused (flag kind, message).
#[derive(Debug, PartialEq)]
pub struct Reject {
    pub kind: &'static str,
    pub detail: String,
    /// Worth showing staff (an edit or an impossible number), not just stale.
    pub flag: bool,
}

fn rej(kind: &'static str, detail: String, flag: bool) -> Reject {
    Reject { kind, detail, flag }
}

/// The checks of one entry against the character's previous upload `(played_s, at)`.
/// `Ok(false)`: nothing new (same or older answer than the one stored).
pub fn check_entry(
    e: &CharEntry,
    prev: Option<(i64, i64)>,
    linked_tag: Option<&str>,
    now: i64,
) -> Result<bool, Reject> {
    match verify(e) {
        SigCheck::Ok => {}
        SigCheck::Bad => {
            return Err(rej(
                "wow_checksum",
                "checksum doesn't match: the saved entry was edited".into(),
                true,
            ))
        }
        SigCheck::UnknownVersion => {
            return Err(rej(
                "wow_old_addon",
                format!(
                    "addon signature version {} isn't accepted; update the addon",
                    e.sv
                ),
                false,
            ))
        }
    }
    if let Some(tag) = linked_tag {
        if !e.battletag.is_empty() && !e.battletag.eq_ignore_ascii_case(tag) {
            return Err(rej(
                "wow_battletag",
                "the addon's BattleTag isn't the linked Battle.net account".into(),
                true,
            ));
        }
    }
    if e.at > now + 600 || e.at < 1_100_000_000 {
        return Err(rej(
            "wow_clock",
            "the entry's time is in the future".into(),
            false,
        ));
    }
    if e.played <= 0 || e.played > MAX_PLAYED_S || !(0..=200).contains(&e.level) {
        return Err(rej(
            "wow_impossible",
            format!("{} h /played is out of range", e.played / 3600),
            true,
        ));
    }
    let Some((pp, pat)) = prev else {
        return Ok(true);
    };
    if e.at <= pat {
        return Ok(false);
    }
    if e.played < pp {
        return Err(rej(
            "wow_decrease",
            format!(
                "/played went down from {} to {} minutes",
                pp / 60,
                e.played / 60
            ),
            true,
        ));
    }
    let grew = e.played - pp;
    let real = e.at - pat;
    if grew > real + SLACK_S {
        return Err(rej(
            "wow_impossible",
            format!(
                "/played grew {} min in {} min of real time",
                grew / 60,
                real / 60
            ),
            true,
        ));
    }
    Ok(true)
}

#[derive(Deserialize)]
struct UploadReq {
    game_id: String,
    characters: Vec<CharEntry>,
}

fn label(e: &CharEntry) -> String {
    format!("{}-{} ({})", e.name, e.realm, e.region.to_uppercase())
}

async fn flag(st: &AppState, uid: Uuid, game: &str, who: &str, r: &Reject, played: i64) {
    // the same file is often sent again: one flag per character, kind and number
    let _ = sqlx::query(
        "INSERT INTO wow_flags(user_id, game_id, char_label, kind, played_s, detail)
         SELECT $1, $2, $3, $4, $5, $6
         WHERE NOT EXISTS (SELECT 1 FROM wow_flags WHERE user_id = $1 AND char_label = $3
                             AND kind = $4 AND (played_s = $5 OR kind = 'wow_battletag'))",
    )
    .bind(uid)
    .bind(game)
    .bind(who)
    .bind(r.kind)
    .bind(played)
    .bind(&r.detail)
    .execute(&st.db)
    .await;
}

/// The linked Battle.net account: (BattleTag, has the `wow.profile` scope).
pub(crate) async fn bnet_link(st: &AppState, uid: Uuid) -> ApiResult<Option<(String, bool)>> {
    let r: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT display, scopes FROM linked_accounts WHERE user_id = $1 AND provider = 'bnet' AND verified",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?;
    Ok(r.map(|(d, s)| {
        (
            d,
            s.unwrap_or_default().split(' ').any(|x| x == "wow.profile"),
        )
    }))
}

async fn upload(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<UploadReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    ingest(&st, a.user.id, &r.game_id, &r.characters)
        .await
        .map(Json)
}

#[derive(Deserialize)]
struct FileReq {
    game_id: String,
    /// The text of a `FraglinkPlayed.lua` SavedVariables file.
    text: String,
}

/// Biggest `FraglinkPlayed.lua` the website takes (200 characters is about 60 KB).
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

/// The website's manual upload: the same file the app reads, through the same checks.
async fn upload_file(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<FileReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if r.text.len() > MAX_FILE_BYTES {
        return Err(AppError::bad_request(
            "too_big",
            "That file is over 1 MB. Pick FraglinkPlayed.lua from SavedVariables.",
        ));
    }
    let chars = fraglink_core::wow_played::entries_from_saved(&r.text);
    if chars.is_empty() {
        return Err(AppError::bad_request(
            "no_characters",
            "No Fraglink /played entries in that file. Pick FraglinkPlayed.lua, and log out of WoW (or /reload) first so the game saves it.",
        ));
    }
    let mut v = ingest(&st, a.user.id, &r.game_id, &chars).await?;
    v["found"] = chars.len().into();
    Ok(Json(v))
}

/// Check and store one batch of entries (the app's upload and the website's file upload).
async fn ingest(st: &AppState, uid: Uuid, game_id: &str, chars: &[CharEntry]) -> ApiResult<Value> {
    let Some((gid, gname)) = GAMES.iter().find(|(g, _)| *g == game_id) else {
        return Err(AppError::bad_request(
            "invalid_game",
            "game_id is wow or wow_forever",
        ));
    };
    if !UPLOADS_OPEN.contains(gid) {
        return Err(AppError::bad_request(
            "forever_not_open",
            "Forever /played uploads open after WoW Forever launches; until then your hours are tracked by the app.",
        ));
    }
    if chars.len() > 200 {
        return Err(AppError::bad_request("too_many", "At most 200 characters."));
    }
    st.rate_limit(format!("wow_played:{uid}"), 30, Duration::from_secs(3600))?;
    let now = Utc::now().timestamp();
    let link = bnet_link(st, uid).await?;
    sqlx::query("INSERT INTO games(id, name) VALUES($1, $2) ON CONFLICT (id) DO NOTHING")
        .bind(gid)
        .bind(gname)
        .execute(&st.db)
        .await?;
    let mut accepted = 0;
    let mut rejected = Vec::new();
    let mut unchanged = Vec::new();
    // per character: /played gained since its last upload (None for a first upload)
    let mut changes = Vec::new();
    for e in chars {
        let who = label(e);
        let slug = realm_slug(&e.realm);
        let prev: Option<(Uuid, i64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, played_s, played_at FROM wow_characters
             WHERE user_id = $1 AND game_id = $2 AND region = $3 AND lower(realm) = lower($4)
               AND lower(name) = lower($5)",
        )
        .bind(uid)
        .bind(gid)
        .bind(&e.region)
        .bind(&e.realm)
        .bind(&e.name)
        .fetch_optional(&st.db)
        .await?;
        let checked = check_entry(
            e,
            prev.map(|p| (p.1, p.2.timestamp())),
            link.as_ref().map(|l| l.0.as_str()),
            now,
        );
        match checked {
            Err(x) => {
                if x.flag {
                    flag(st, uid, gid, &who, &x, e.played).await;
                }
                rejected.push(json!({ "character": who, "name": e.name, "realm": e.realm,
                    "region": e.region, "class": e.class, "level": e.level,
                    "played_min": e.played / 60, "reason": x.kind, "detail": x.detail }));
                continue;
            }
            Ok(false) => {
                changes.push(
                    json!({ "name": e.name, "realm": e.realm, "region": e.region,
                    "delta_s": prev.map(|p| e.played - p.1).unwrap_or(0).max(0), "first": false }),
                );
                unchanged.push(json!({ "character": who }));
                continue;
            }
            Ok(true) => {}
        }
        let at = Utc.timestamp_opt(e.at, 0).single().unwrap_or_else(Utc::now);
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, class,
                                        level, played_s, played_at, battletag)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
             ON CONFLICT (user_id, game_id, region, lower(realm), lower(name)) DO UPDATE
               SET class = excluded.class, level = excluded.level, played_s = excluded.played_s,
                   played_at = excluded.played_at, uploaded_at = now(), realm_slug = excluded.realm_slug,
                   battletag = excluded.battletag
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(uid)
        .bind(gid)
        .bind(e.region.chars().take(8).collect::<String>())
        .bind(e.realm.chars().take(64).collect::<String>())
        .bind(&slug)
        .bind(e.name.chars().take(32).collect::<String>())
        .bind(e.class.chars().take(24).collect::<String>())
        .bind(e.level as i32)
        .bind(e.played)
        .bind(at)
        .bind(e.battletag.chars().take(64).collect::<String>())
        .fetch_one(&st.db)
        .await?;
        sqlx::query("INSERT INTO wow_played_snaps(char_id, played_s, at) VALUES($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(e.played)
            .bind(at)
            .execute(&st.db)
            .await?;
        changes.push(
            json!({ "name": e.name, "realm": e.realm, "region": e.region,
            "delta_s": prev.map(|p| (e.played - p.1).max(0)), "first": prev.is_none() }),
        );
        check_bnet_level(st, uid, gid, id, e).await;
        accepted += 1;
    }
    // every character of the user, so renames, transfers and sales are seen on each upload;
    // the total is stored once here, so the whole upload's /played gain meets the tracked time
    set_verified(st, uid, None).await;
    let (total_min, kept_min) = record_total(st, uid, gid, Recount::KeepHigher).await?;
    let verified: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM wow_characters WHERE user_id = $1 AND game_id = $2 AND verified",
    )
    .bind(uid)
    .bind(gid)
    .fetch_one(&st.db)
    .await?;
    let bnet = match &link {
        None => "none",
        Some((_, false)) => "reconnect",
        Some((_, true)) => "linked",
    };
    Ok(
        json!({ "accepted": accepted, "rejected": rejected, "unchanged": unchanged,
               "changes": changes, "total_min": total_min, "kept_min": kept_min,
               "verified": verified, "bnet": bnet }),
    )
}

/// How a new character sum is stored.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Recount {
    /// Normal path (uploads, Battle.net sync): the higher /played per character (see
    /// `record_total`); a partial upload or a deleted character never lowers the total.
    KeepHigher,
    /// Battle.net unlinked: store the exact sum (unverified characters must stop counting).
    Exact,
    /// Asked for by the user or staff: store the exact sum now, even if it is lower.
    Reset,
}

/// The game's latest imported entry: (minutes, carried extra, tracked minutes after it).
/// The shown imported total is their sum (the same rule as `user_game_stats`).
pub(crate) async fn last_import(
    st: &AppState,
    uid: Uuid,
    gid: &str,
) -> ApiResult<Option<(i64, i64, i64)>> {
    Ok(sqlx::query_as(
        "WITH i AS (SELECT minutes, carry_min, captured_at FROM hours_entries
                    WHERE user_id = $1 AND game_id = $2 AND tier = 'imported'
                    ORDER BY captured_at DESC, created_at DESC LIMIT 1)
         SELECT i.minutes::bigint, i.carry_min::bigint,
                coalesce((SELECT (sum(LEAST(x.duration_s::numeric,
                    extract(epoch FROM x.ended_at - GREATEST(x.started_at, i.captured_at)))) / 60)::bigint
                  FROM sessions x WHERE x.user_id = $1 AND x.game_id = $2 AND x.ended_at > i.captured_at), 0)::bigint
         FROM i",
    )
    .bind(uid)
    .bind(gid)
    .fetch_optional(&st.db)
    .await?)
}

/// The shown imported total: latest import + carried extra + tracked play after it.
pub(crate) async fn imported_floor(st: &AppState, uid: Uuid, gid: &str) -> ApiResult<Option<i64>> {
    Ok(last_import(st, uid, gid).await?.map(|(m, c, a)| m + c + a))
}

/// The characters that count toward a user's game, one per Battle.net character id
/// (row id, /played seconds):
/// - verified ones (on the linked account's newest listing, see `set_verified`);
/// - ones verified before that are on nobody's listing any more (deleted: they keep their
///   hours) while Battle.net is linked;
/// - never one another user's Battle.net lists now (sold or moved), and a renamed or
///   transferred character counts once (its rows share the id; the verified/higher one).
const COUNTED: &str = "
    WITH c AS (
      SELECT c.id, c.bnet_id, c.played_s, c.verified FROM wow_characters c
      WHERE c.user_id = $1 AND c.game_id = $2
        AND (c.verified OR (c.ever_verified
             AND EXISTS (SELECT 1 FROM linked_accounts l WHERE l.user_id = c.user_id
                           AND l.provider = 'bnet' AND l.verified)
             AND (c.bnet_id IS NULL OR NOT EXISTS (
                  SELECT 1 FROM bnet_characters b WHERE b.game_id = c.game_id AND b.region = c.region
                    AND b.bnet_id = c.bnet_id AND b.user_id <> c.user_id)))))
    SELECT DISTINCT ON (coalesce('b' || bnet_id::text, id::text)) id::text, played_s
    FROM c ORDER BY coalesce('b' || bnet_id::text, id::text), verified DESC, played_s DESC";

/// Store the game's total when the counted characters changed. Each character's /played
/// only goes up (`check_entry`), so per character the higher value is kept; the sum drops
/// only when a character moves to another owner or a rename merges two rows.
///
/// Menu-time extra: minutes the app tracked beyond what the counted characters' /played
/// gained since the last stored total. If a character stopped counting nothing is added
/// (no extra fed by its missing hours); a newly counted one counts in full. A recount or
/// unlink starts the extra again at 0. Returns (sum, kept); `kept` is always None now (no
/// lower sum is held back) and stays in the reply for older clients.
async fn record_total(
    st: &AppState,
    uid: Uuid,
    gid: &str,
    mode: Recount,
) -> ApiResult<(i64, Option<i64>)> {
    let rows: Vec<(String, i64)> = sqlx::query_as(COUNTED)
        .bind(uid)
        .bind(gid)
        .fetch_all(&st.db)
        .await?;
    let (n, v): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE verified) FROM wow_characters
         WHERE user_id = $1 AND game_id = $2",
    )
    .bind(uid)
    .bind(gid)
    .fetch_one(&st.db)
    .await?;
    let now_map: std::collections::BTreeMap<String, i64> = rows.into_iter().collect();
    let sum = (now_map.values().sum::<i64>() / 60).min(i32::MAX as i64);
    let last: Option<(i32, Value)> = sqlx::query_as(
        "SELECT minutes, inputs FROM hours_entries WHERE user_id = $1 AND game_id = $2 AND source = 'wow_addon'
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(uid)
    .bind(gid)
    .fetch_optional(&st.db)
    .await?;
    let prev_map: Option<std::collections::BTreeMap<String, i64>> = last
        .as_ref()
        .and_then(|(_, i)| serde_json::from_value(i["chars"].clone()).ok());
    let mut carry = 0i64;
    let store = match mode {
        Recount::Reset => last.is_some() || sum > 0,
        Recount::Exact => {
            last.as_ref().map(|l| i64::from(l.0)) != Some(sum) && !(last.is_none() && sum == 0)
        }
        Recount::KeepHigher => match last_import(st, uid, gid).await? {
            None => sum > 0,
            Some((m0, c0, after)) => {
                // /played gained by the counted characters (None: one stopped counting)
                let gain = match &prev_map {
                    Some(p) if p.keys().any(|k| !now_map.contains_key(k)) => None,
                    Some(p) => Some(
                        now_map
                            .iter()
                            .map(|(k, s)| (s - p.get(k).copied().unwrap_or(0)).max(0))
                            .sum::<i64>()
                            / 60,
                    ),
                    // totals stored before per-character records: the sum's rise
                    None => Some(sum - m0),
                };
                carry = c0 + gain.filter(|g| *g > 0).map_or(0, |g| (after - g).max(0));
                match &prev_map {
                    Some(p) => *p != now_map,
                    None => sum != m0,
                }
            }
        },
    };
    if store {
        let note = if mode == Recount::Reset {
            "WoW /played (Fraglink addon), recounted"
        } else {
            "WoW /played (Fraglink addon)"
        };
        sqlx::query(
            "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, inputs, note, carry_min)
             VALUES($1, $2, $3, 'imported', 'wow_addon', $4, $5, $6, $7)",
        )
        .bind(Uuid::new_v4())
        .bind(uid)
        .bind(gid)
        .bind(sum as i32)
        .bind(json!({ "characters": n, "verified": v, "reset": mode == Recount::Reset, "chars": now_map }))
        .bind(note)
        .bind(carry.min(i32::MAX as i64) as i32)
        .execute(&st.db)
        .await?;
    }
    Ok((sum, None))
}

/// Recount a user's WoW total from their characters now, dropping the carried menu-time
/// extra. Staff only (admin user tools, audit-logged).
pub async fn recount(st: &AppState, uid: Uuid, game_id: &str) -> ApiResult<i64> {
    let Some((gid, _)) = GAMES.iter().find(|(g, _)| *g == game_id) else {
        return Err(AppError::bad_request(
            "invalid_game",
            "game_id is wow or wow_forever",
        ));
    };
    Ok(record_total(st, uid, gid, Recount::Reset).await?.0)
}

// ------------------------------------------------------------------ Battle.net

fn bnet_creds() -> Option<(String, String)> {
    let g = |k| {
        std::env::var(k)
            .ok()
            .filter(|v: &String| !v.trim().is_empty())
    };
    Some((g("BNET_CLIENT_ID")?, g("BNET_CLIENT_SECRET")?))
}

pub(crate) fn http() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .ok()
}

/// Profile namespace of a game in a region. Forever's isn't known before launch:
/// `BNET_FOREVER_NAMESPACE` (e.g. `profile-forever-{region}`) turns it on.
pub(crate) fn namespace(game: &str, region: &str) -> Option<String> {
    match game {
        "wow" => Some(format!("profile-{region}")),
        _ => std::env::var("BNET_FOREVER_NAMESPACE")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .map(|t| t.replace("{region}", region)),
    }
}

/// At Battle.net link time (the user token lasts about a day): the WoW characters on
/// the account in every region, then re-mark this user's characters as verified.
/// The (game, region) of each character found; `None` when Battle.net didn't answer.
pub async fn sync_characters(
    st: &AppState,
    uid: Uuid,
    token: &str,
) -> Option<Vec<(String, String)>> {
    let http = http()?;
    #[allow(clippy::type_complexity)]
    let mut rows: Vec<(
        String,
        String,
        String,
        String,
        String,
        Option<i32>,
        [Option<String>; 3],
        Option<i64>,
    )> = Vec::new();
    let mut any_ok = false;
    for (game, _) in GAMES {
        for r in REGIONS {
            let Some(ns) = namespace(game, r) else {
                continue;
            };
            let url = format!(
                "https://{r}.api.blizzard.com/profile/user/wow?namespace={ns}&locale=en_US"
            );
            let Ok(resp) = http.get(&url).bearer_auth(token).send().await else {
                continue;
            };
            if resp.status() == StatusCode::NOT_FOUND {
                any_ok = true; // no characters in this region
                continue;
            }
            let Ok(v) = resp.error_for_status() else {
                continue;
            };
            let Ok(v) = v.json::<Value>().await else {
                continue;
            };
            any_ok = true;
            for acct in v["wow_accounts"].as_array().into_iter().flatten() {
                for c in acct["characters"].as_array().into_iter().flatten() {
                    let (Some(name), Some(slug)) =
                        (c["name"].as_str(), c["realm"]["slug"].as_str())
                    else {
                        continue;
                    };
                    rows.push((
                        game.to_string(),
                        r.to_string(),
                        slug.to_string(),
                        c["realm"]["name"].as_str().unwrap_or_default().to_string(),
                        name.to_string(),
                        c["level"].as_i64().map(|l| l as i32),
                        [
                            name_of(&c["playable_race"]),
                            c["gender"]["type"].as_str().map(|g| g.to_lowercase()),
                            name_of(&c["playable_class"]),
                        ],
                        c["id"].as_i64(),
                    ));
                }
            }
        }
    }
    if !any_ok {
        tracing::warn!("bnet characters: no region answered for {uid}");
        return None;
    }
    let _ = sqlx::query("DELETE FROM bnet_characters WHERE user_id = $1")
        .bind(uid)
        .execute(&st.db)
        .await;
    for (g, r, slug, rn, name, lvl, [race, gender, class], bid) in &rows {
        let _ = sqlx::query(
            "INSERT INTO bnet_characters(user_id, game_id, region, realm_slug, realm_name, name, level,
                                         race, gender, class_name, bnet_id)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT DO NOTHING",
        )
        .bind(uid)
        .bind(g)
        .bind(r)
        .bind(slug)
        .bind(rn)
        .bind(name)
        .bind(lvl)
        .bind(race)
        .bind(gender)
        .bind(class)
        .bind(bid)
        .execute(&st.db)
        .await;
    }
    tracing::info!("bnet characters: {} for {uid}", rows.len());
    reverify(st, uid).await;
    crate::wowpage::after_sync(st, uid).await;
    Some(rows.into_iter().map(|r| (r.0, r.1)).collect())
}

/// A character is verified while it is on the linked Battle.net account (its `wow.profile`
/// list), its latest accepted entry was saved under that account's BattleTag, and no other
/// user's Battle.net listed the same character id more recently (sold or moved: the
/// current holder gets the hours). Several WoW licences on one Battle.net (WoW1, WoW2)
/// share the BattleTag, so all of them count. Other users holding one of this user's
/// character ids are rechecked too, so a sold character leaves the old owner's total.
/// `mode` None: the caller stores this user's totals itself (an upload).
async fn set_verified(st: &AppState, uid: Uuid, mode: Option<Recount>) {
    // ids from this user's listing onto their uploaded rows
    let _ = sqlx::query(
        "UPDATE wow_characters c SET bnet_id = b.bnet_id FROM bnet_characters b
         WHERE c.user_id = $1 AND b.user_id = c.user_id AND b.game_id = c.game_id AND b.region = c.region
           AND b.realm_slug = c.realm_slug AND lower(b.name) = lower(c.name) AND b.bnet_id IS NOT NULL
           AND c.bnet_id IS DISTINCT FROM b.bnet_id",
    )
    .bind(uid)
    .execute(&st.db)
    .await;
    let mut users: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT o.user_id FROM wow_characters o JOIN wow_characters c
           ON c.user_id = $1 AND o.game_id = c.game_id AND o.region = c.region AND o.bnet_id = c.bnet_id
         WHERE o.user_id <> $1",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await
    .unwrap_or_default();
    users.insert(0, uid);
    let _ = sqlx::query(
        "UPDATE wow_characters c SET verified = EXISTS (
             SELECT 1 FROM bnet_characters b WHERE b.user_id = c.user_id AND b.game_id = c.game_id
                AND b.region = c.region AND b.realm_slug = c.realm_slug AND lower(b.name) = lower(c.name))
            AND EXISTS (SELECT 1 FROM linked_accounts l WHERE l.user_id = c.user_id
                           AND l.provider = 'bnet' AND l.verified AND c.battletag <> ''
                           AND lower(l.display) = lower(c.battletag))
            AND (c.bnet_id IS NULL OR NOT EXISTS (
                 SELECT 1 FROM bnet_characters o WHERE o.user_id <> c.user_id AND o.game_id = c.game_id
                    AND o.region = c.region AND o.bnet_id = c.bnet_id
                    AND o.fetched_at > coalesce((SELECT max(m.fetched_at) FROM bnet_characters m
                        WHERE m.user_id = c.user_id AND m.game_id = c.game_id AND m.region = c.region
                          AND m.bnet_id = c.bnet_id), '-infinity')))
         WHERE c.user_id = ANY($1)",
    )
    .bind(&users)
    .execute(&st.db)
    .await;
    let _ = sqlx::query(
        "UPDATE wow_characters SET ever_verified = true
         WHERE user_id = ANY($1) AND verified AND NOT ever_verified",
    )
    .bind(&users)
    .execute(&st.db)
    .await;
    // Verification decides what counts, so refresh the stored totals.
    for (i, u) in users.iter().enumerate() {
        let m = match (i, mode) {
            (0, None) => continue,
            (0, Some(m)) => m,
            _ => Recount::KeepHigher,
        };
        for (gid, _) in GAMES {
            let _ = record_total(st, *u, gid, m).await;
        }
    }
}

/// Recompute `verified` for all of a user's characters (after link / unlink). A
/// character stays verified only while it is on the linked account; the BattleTag
/// check was made when its entry was accepted.
pub async fn reverify(st: &AppState, uid: Uuid) {
    set_verified(st, uid, Some(Recount::KeepHigher)).await;
}

/// Battle.net unlinked: forget its characters; nothing counts on leaderboards any more.
pub async fn on_unlink(st: &AppState, uid: Uuid) {
    let _ = sqlx::query("DELETE FROM bnet_characters WHERE user_id = $1")
        .bind(uid)
        .execute(&st.db)
        .await;
    // Unverified characters must stop counting, so store the exact (lower) sum here.
    set_verified(st, uid, Some(Recount::Exact)).await;
}

pub(crate) async fn app_token() -> Option<String> {
    static T: std::sync::OnceLock<tokio::sync::Mutex<Option<(String, std::time::Instant)>>> =
        std::sync::OnceLock::new();
    let m = T.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut g = m.lock().await;
    if let Some((t, at)) = g.as_ref() {
        if at.elapsed() < Duration::from_secs(12 * 3600) {
            return Some(t.clone());
        }
    }
    let (id, secret) = bnet_creds()?;
    let v: Value = http()?
        .post("https://oauth.battle.net/token")
        .basic_auth(id, Some(secret))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let t = v["access_token"].as_str()?.to_string();
    *g = Some((t.clone(), std::time::Instant::now()));
    Some(t)
}

/// Sanity check of a verified character against Blizzard's public profile (at most every
/// 6 hours): Blizzard saw a later login with a lower level than the addon claims -> flag.
async fn check_bnet_level(st: &AppState, uid: Uuid, gid: &str, id: Uuid, e: &CharEntry) {
    let due: Option<bool> = sqlx::query_scalar(
        "SELECT verified AND (bnet_checked_at IS NULL OR bnet_checked_at < now() - interval '6 hours')
         FROM wow_characters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await
    .ok()
    .flatten();
    if due != Some(true) {
        return;
    }
    let Some(ns) = namespace(gid, &e.region) else {
        return;
    };
    let Some(tok) = app_token().await else { return };
    let Some(http) = http() else { return };
    let url = format!(
        "https://{}.api.blizzard.com/profile/wow/character/{}/{}?namespace={ns}&locale=en_US",
        e.region,
        realm_slug(&e.realm),
        e.name.to_lowercase()
    );
    let Ok(Ok(resp)) = http
        .get(&url)
        .bearer_auth(tok)
        .send()
        .await
        .map(|r| r.error_for_status())
    else {
        return;
    };
    let Ok(v) = resp.json::<Value>().await else {
        return;
    };
    let level = v["level"].as_i64();
    let last = v["last_login_timestamp"]
        .as_i64()
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single());
    let _ = sqlx::query(
        "UPDATE wow_characters SET bnet_level = $2, bnet_last_login = $3, bnet_checked_at = now(),
                bnet_spec = coalesce($4, bnet_spec), race = coalesce($5, race), gender = coalesce($6, gender),
                bnet_id = coalesce(bnet_id, $7)
         WHERE id = $1",
    )
    .bind(id)
    .bind(level.map(|l| l as i32))
    .bind(last)
    .bind(name_of(&v["active_spec"]))
    .bind(name_of(&v["race"]))
    .bind(v["gender"]["type"].as_str().map(|g| g.to_lowercase()))
    .bind(v["id"].as_i64())
    .execute(&st.db)
    .await;
    if let (Some(l), Some(t)) = (level, last) {
        if t.timestamp() > e.at && e.level > l {
            let r = rej(
                "wow_bnet_mismatch",
                format!(
                    "addon says level {}, Battle.net says {l} after a later login",
                    e.level
                ),
                true,
            );
            flag(st, uid, gid, &label(e), &r, e.played).await;
        }
    }
}

/// A Blizzard `{ "name": "..." }` object (or a plain string with `locale` set).
pub(crate) fn name_of(v: &Value) -> Option<String> {
    v["name"]
        .as_str()
        .or_else(|| v["name"]["en_US"].as_str())
        .map(|s| s.chars().take(40).collect())
}

// ------------------------------------------------------------------ profile

/// Characters for a profile page. `all`: the owner's own view (hidden ones too).
/// Profile cards list only Battle.net-verified characters (`verified_only`); the owner's
/// /connect/wow page lists all of them so unverified ones can be verified.
pub async fn characters(
    st: &AppState,
    uid: Uuid,
    all: bool,
    verified_only: bool,
) -> ApiResult<Vec<Value>> {
    #[derive(sqlx::FromRow)]
    struct Row {
        id: Uuid,
        game_id: String,
        region: String,
        realm: String,
        name: String,
        class: String,
        level: i32,
        played_s: i64,
        played_at: DateTime<Utc>,
        verified: bool,
        hid: bool,
        race: Option<String>,
        gender: Option<String>,
        spec: Option<String>,
        show_guild: bool,
        guild: Option<String>,
        featured: Option<i16>,
    }
    let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, game_id, region, realm, name, class, level, played_s, played_at, verified,
                    hidden OR coalesce((SELECT hide_all FROM wow_prefs WHERE user_id = $1), false) AS hid,
                    race, gender, coalesce(spec, bnet_spec) AS spec, show_guild,
                    CASE WHEN show_guild THEN (SELECT d.data->'guild'->>'name' FROM wow_char_details d
                         WHERE d.char_id = wow_characters.id AND d.status = 'ok') END AS guild,
                    featured
             FROM wow_characters WHERE user_id = $1
               AND ($2 OR NOT (hidden OR coalesce((SELECT hide_all FROM wow_prefs WHERE user_id = $1), false)))
               AND (NOT $3 OR verified)
             ORDER BY game_id, featured NULLS LAST, played_s DESC LIMIT 200",
        )
        .bind(uid)
        .bind(all)
        .bind(verified_only)
        .fetch_all(&st.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|c| {
            json!({ "id": c.id, "game_id": c.game_id, "region": c.region, "realm": c.realm,
                "name": c.name, "class": c.class, "level": c.level, "played_min": c.played_s / 60,
                "played_at": c.played_at, "verified": c.verified, "hidden": c.hid, "race": c.race,
                "gender": c.gender, "spec": c.spec, "show_guild": c.show_guild, "guild": c.guild,
                "featured": c.featured })
        })
        .collect())
}

async fn mine(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let link = bnet_link(&st, a.user.id).await?;
    let chars = characters(&st, a.user.id, true, false).await?;
    let bnet = match &link {
        None => "none",
        Some((_, false)) => "reconnect",
        Some((_, true)) => "linked",
    };
    Ok(Json(json!({ "characters": chars, "bnet": bnet })))
}

#[derive(Deserialize)]
struct HiddenReq {
    hidden: bool,
}

async fn set_hidden(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
    Json(r): Json<HiddenReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let n = sqlx::query(
        "UPDATE wow_characters SET hidden = $3, featured = CASE WHEN $3 THEN NULL ELSE featured END
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(a.user.id)
    .bind(r.hidden)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    Ok(Json(json!({ "ok": true, "hidden": r.hidden })))
}

// ------------------------------------------------------------------ admin

/// Admin > Hours review items for failed addon checks.
pub async fn admin_flags(st: &AppState, days: i32) -> ApiResult<Vec<Value>> {
    let rows: Vec<(Uuid, String, String, Option<String>, String, String, Option<i64>, String, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT f.user_id, u.handle, f.game_id, g.name, f.char_label, f.kind, f.played_s, f.detail, f.created_at
             FROM wow_flags f JOIN users u ON u.id = f.user_id LEFT JOIN games g ON g.id = f.game_id
             WHERE f.created_at > now() - make_interval(days => $1) AND u.deleted_at IS NULL
             ORDER BY f.created_at DESC LIMIT 200",
        )
        .bind(days)
        .fetch_all(&st.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(u, h, g, n, who, k, s, d, at)| {
            json!({ "kind": k, "user_id": u, "handle": h, "game_id": g, "game_name": n,
                    "when": at, "minutes": s.map(|s| s / 60), "detail": format!("{who}: {d}") })
        })
        .collect())
}

/// Fill missing Battle.net character ids (rows from before ids were stored) from the public
/// character profile, once per row, then recheck the users touched: a rename or move found
/// this way merges into one character. A profile Blizzard doesn't answer (deleted,
/// renamed since, private) leaves the id empty and the row counting as before.
pub async fn backfill_ids(st: AppState) {
    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let rows: Vec<(Uuid, Uuid, String, String, String, String)> = sqlx::query_as(
        "SELECT id, user_id, game_id, region, realm_slug, name FROM wow_characters
         WHERE bnet_id IS NULL AND bnet_id_tried_at IS NULL ORDER BY user_id LIMIT 2000",
    )
    .fetch_all(&st.db)
    .await
    .unwrap_or_default();
    if rows.is_empty() {
        return;
    }
    let (Some(tok), Some(http)) = (app_token().await, http()) else {
        return;
    };
    let (mut found, mut users) = (0, std::collections::BTreeSet::new());
    for (id, uid, gid, region, slug, name) in &rows {
        let Some(ns) = namespace(gid, region) else {
            continue;
        };
        let url = format!(
            "https://{region}.api.blizzard.com/profile/wow/character/{slug}/{}?namespace={ns}&locale=en_US",
            name.to_lowercase()
        );
        let bid = match http.get(&url).bearer_auth(&tok).send().await {
            Ok(r) if r.status().is_success() => {
                r.json::<Value>().await.ok().and_then(|v| v["id"].as_i64())
            }
            Ok(r) if r.status().as_u16() == 404 => None,
            _ => continue, // try again next start
        };
        let _ = sqlx::query(
            "UPDATE wow_characters SET bnet_id = coalesce(bnet_id, $2), bnet_id_tried_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(bid)
        .execute(&st.db)
        .await;
        if let Some(b) = bid {
            found += 1;
            let _ = sqlx::query(
                "UPDATE bnet_characters SET bnet_id = $5 WHERE user_id = $1 AND game_id = $2 AND region = $3
                   AND realm_slug = $4 AND lower(name) = lower($6) AND bnet_id IS NULL",
            )
            .bind(uid)
            .bind(gid)
            .bind(region)
            .bind(slug)
            .bind(b)
            .bind(name)
            .execute(&st.db)
            .await;
            users.insert(*uid);
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    for u in users {
        reverify(&st, u).await;
    }
    tracing::info!("bnet ids: filled {found} of {} characters", rows.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    use fraglink_core::wow_played::{sign_with, SIG_KEYS};

    fn e(played: i64, at: i64, tag: &str) -> CharEntry {
        let mut c = CharEntry {
            region: "us".into(),
            realm: "Area 52".into(),
            name: "Abc".into(),
            class: "MAGE".into(),
            level: 80,
            played,
            at,
            sv: 1,
            battletag: tag.into(),
            sig: String::new(),
        };
        c.sig = sign_with(SIG_KEYS[0].1, &c);
        c
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn slugs() {
        assert_eq!(realm_slug("Area 52"), "area-52");
        assert_eq!(realm_slug("Kel'Thas"), "kelthas");
        assert_eq!(realm_slug("Aggra (Português)"), "aggra-português");
    }

    #[test]
    fn checks() {
        let ok = e(100_000, NOW - 60, "A#1");
        assert_eq!(check_entry(&ok, None, None, NOW), Ok(true));
        assert_eq!(
            check_entry(&ok, Some((90_000, NOW - 20_000)), Some("a#1"), NOW),
            Ok(true)
        );
        // stale
        assert_eq!(
            check_entry(&ok, Some((100_000, NOW - 60)), None, NOW),
            Ok(false)
        );
        // edited
        let mut x = ok.clone();
        x.played += 3600;
        assert_eq!(
            check_entry(&x, None, None, NOW).unwrap_err().kind,
            "wow_checksum"
        );
        // wrong BattleTag
        assert_eq!(
            check_entry(&ok, None, Some("B#2"), NOW).unwrap_err().kind,
            "wow_battletag"
        );
        // goes down
        assert_eq!(
            check_entry(&ok, Some((200_000, NOW - 9000)), None, NOW)
                .unwrap_err()
                .kind,
            "wow_decrease"
        );
        // faster than real time: +10,000 s in 1,000 s
        assert_eq!(
            check_entry(&ok, Some((90_000, NOW - 1060)), None, NOW)
                .unwrap_err()
                .kind,
            "wow_impossible"
        );
        // future
        let f = e(100_000, NOW + 3600, "");
        assert_eq!(
            check_entry(&f, None, None, NOW).unwrap_err().kind,
            "wow_clock"
        );
        let mut v = ok.clone();
        v.sv = 0;
        assert_eq!(
            check_entry(&v, None, None, NOW).unwrap_err().kind,
            "wow_old_addon"
        );
    }
}
