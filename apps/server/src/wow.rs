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
//! they link) are "verified": only those count toward the game's stored total (and so on
//! all-time leaderboards; /played never counts in a 7-day figure, see `crate::week`). The
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

/// The level a character card shows: Blizzard's (armory refresh, account sync or the
/// post-upload check) when it is newer than the last /played upload, i.e. from a later
/// login or simply higher (levels only go up, so a lagging armory never lowers it). Otherwise
/// the addon's. `level` itself stays the addon's claim, which `check_bnet_level` compares.
macro_rules! shown_level {
    () => {
        "(CASE WHEN bnet_level > 0 AND (bnet_last_login > played_at OR bnet_level > level) \
          THEN bnet_level ELSE level END)"
    };
}
pub(crate) use shown_level;

/// WoW flavours that take /played uploads. Each is kept separate, and the per-character
/// checks, keep-the-higher rule and menu-time carry (`record_total`) apply to every entry
/// here, so a new flavour (Classic) only needs adding here and in the app's `FLAVOURS`.
pub const GAMES: [(&str, &str); 2] = [
    ("wow", "World of Warcraft"),
    ("wow_forever", "World of Warcraft: Forever"),
];
/// Flavours taking /played uploads. Forever uses its own path and does not write
/// an imported game total. Retail is unchanged.
pub const UPLOADS_OPEN: [&str; 2] = ["wow", "wow_forever"];
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
        .route("/api/v1/me/wow/armory", post(add_armory))
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
    let mut m = stash().lock().unwrap_or_else(|e| e.into_inner());
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
        .unwrap_or_else(|e| e.into_inner())
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

#[derive(Deserialize)]
struct ArmoryAdd {
    region: String,
    #[serde(default)]
    realm: String,
    name: String,
    /// `wow` (Armory) or `wow_forever` (first name and surname, no lookup).
    #[serde(default)]
    game: String,
}

fn clean_label(s: &str, max: usize) -> String {
    s.trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

fn class_token(v: &Value) -> String {
    name_of(&v["playable_class"])
        .or_else(|| name_of(&v["character_class"]))
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase()
}

/// Save a Retail character from an Armory profile. `/played` is not touched.
/// The existing account-list check sets the tick when the name and realm are on
/// the linked Battle.net account. A lookup by itself does not.
pub async fn add_armory_character(
    st: &AppState,
    uid: Uuid,
    region: &str,
    realm_input: &str,
    name_input: &str,
    profile: &Value,
    game: &str,
) -> ApiResult<Value> {
    let region = region.trim().to_lowercase();
    if !matches!(region.as_str(), "us" | "eu" | "kr" | "tw") {
        return Err(AppError::bad_request(
            "invalid_region",
            "Choose US, EU, KR, or TW.",
        ));
    }
    let forever = game == "wow_forever";
    let game_id = if forever { "wow_forever" } else { "wow" };
    let name = clean_label(profile["name"].as_str().unwrap_or(name_input), 64);
    let (realm, slug) = if forever {
        (String::new(), String::new())
    } else {
        let realm = clean_label(
            name_of(&profile["realm"]).as_deref().unwrap_or(realm_input),
            64,
        );
        let slug = realm_slug(&realm);
        if realm.chars().count() < 2 || slug.is_empty() {
            return Err(AppError::bad_request(
                "invalid_character",
                "Enter the character's realm and name.",
            ));
        }
        (realm, slug)
    };
    if name.chars().count() < 2 {
        return Err(AppError::bad_request(
            "invalid_character",
            "Enter the character's name.",
        ));
    }
    let class = class_token(profile);
    let level = profile["level"].as_i64().unwrap_or(0).clamp(0, 200) as i32;
    let race = name_of(&profile["race"]);
    let gender = profile["gender"]["type"].as_str().map(|g| g.to_lowercase());
    let spec = name_of(&profile["active_spec"]);
    let bnet_id = profile["id"].as_i64();
    let tag: String = sqlx::query_scalar(
        "SELECT display FROM linked_accounts
         WHERE user_id = $1 AND provider = 'bnet' AND verified LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?
    .unwrap_or_default();
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM wow_characters
         WHERE user_id = $1 AND game_id = $5 AND region = $2
           AND lower(realm) = lower($3) AND lower(name) = lower($4)",
    )
    .bind(uid)
    .bind(&region)
    .bind(&realm)
    .bind(&name)
    .bind(game_id)
    .fetch_optional(&st.db)
    .await?;
    let id = if let Some(id) = existing {
        sqlx::query(
            "UPDATE wow_characters SET
                class = CASE WHEN $2 <> '' THEN $2 ELSE class END,
                level = GREATEST(level, $3),
                race = coalesce(race, $4),
                gender = coalesce(gender, $5),
                bnet_spec = coalesce(bnet_spec, $6),
                bnet_id = coalesce(bnet_id, $7),
                battletag = CASE WHEN battletag = '' THEN $8 ELSE battletag END
             WHERE id = $1 AND user_id = $9",
        )
        .bind(id)
        .bind(&class)
        .bind(level)
        .bind(&race)
        .bind(&gender)
        .bind(&spec)
        .bind(bnet_id)
        .bind(tag.chars().take(64).collect::<String>())
        .bind(uid)
        .execute(&st.db)
        .await?;
        id
    } else {
        let id = Uuid::new_v4();
        // played_at is early so a later addon snapshot is the newer /played, not a stale one.
        let played_at = Utc.timestamp_opt(1_000_000_000, 0).single().unwrap();
        sqlx::query(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, class,
                                        level, played_s, played_at, battletag, race, gender, bnet_spec,
                                        bnet_id, verified)
             VALUES($1, $2, $15, $3, $4, $5, $6, $7, $8, 0, $9, $10, $11, $12, $13, $14, false)",
        )
        .bind(id)
        .bind(uid)
        .bind(&region)
        .bind(&realm)
        .bind(&slug)
        .bind(&name)
        .bind(&class)
        .bind(level)
        .bind(played_at)
        .bind(tag.chars().take(64).collect::<String>())
        .bind(&race)
        .bind(&gender)
        .bind(&spec)
        .bind(bnet_id)
        .bind(game_id)
        .execute(&st.db)
        .await?;
        id
    };
    sqlx::query(
        "UPDATE wow_characters SET faction = coalesce($2, faction) WHERE id = $1 AND user_id = $3",
    )
    .bind(id)
    .bind(profile_faction(profile))
    .bind(uid)
    .execute(&st.db)
    .await?;
    // Same check as a Battle.net refresh. Does not store a new hours total for this user.
    set_verified(st, uid, None).await;
    let verified: bool =
        sqlx::query_scalar("SELECT verified FROM wow_characters WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(uid)
            .fetch_one(&st.db)
            .await?;
    Ok(json!({
        "ok": true,
        "id": id,
        "name": name,
        "realm": realm,
        "region": region,
        "verified": verified,
        "created": existing.is_none(),
    }))
}

async fn fetch_armory_profile(region: &str, realm: &str, name: &str) -> ApiResult<Value> {
    let Some(tok) = app_token().await else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "armory_unavailable",
            "Armory lookup isn't available right now.",
        ));
    };
    let Some(http) = http() else {
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "armory_unavailable",
            "Armory lookup isn't available right now.",
        ));
    };
    let url = format!(
        "https://{}.api.blizzard.com/profile/wow/character/{}/{}?namespace=profile-{}&locale=en_US",
        region.trim().to_lowercase(),
        realm_slug(realm),
        name.trim().to_lowercase(),
        region.trim().to_lowercase(),
    );
    let resp = http.get(&url).bearer_auth(tok).send().await.map_err(|_| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "armory_unavailable",
            "The Armory didn't answer. Try again in a moment.",
        )
    })?;
    if resp.status().as_u16() == 404 {
        return Err(AppError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "No character with that name on that realm.",
        ));
    }
    if !resp.status().is_success() {
        return Err(AppError::new(
            StatusCode::BAD_GATEWAY,
            "armory_unavailable",
            "The Armory didn't answer. Try again in a moment.",
        ));
    }
    resp.json::<Value>().await.map_err(|_| {
        AppError::new(
            StatusCode::BAD_GATEWAY,
            "armory_unavailable",
            "The Armory didn't answer. Try again in a moment.",
        )
    })
}

async fn add_armory(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<ArmoryAdd>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    if r.game == "wow_forever" {
        let profile = json!({ "name": r.name });
        return Ok(Json(
            add_armory_character(
                &st,
                a.user.id,
                &r.region,
                "",
                &r.name,
                &profile,
                "wow_forever",
            )
            .await?,
        ));
    }
    let profile = fetch_armory_profile(&r.region, &r.realm, &r.name).await?;
    Ok(Json(
        add_armory_character(
            &st, a.user.id, &r.region, &r.realm, &r.name, &profile, "wow",
        )
        .await?,
    ))
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

enum Class {
    Retail,
    Forever,
    Quarantine(&'static str),
}

/// Retail stays realm + name. Live Forever is project 18, a tested "not a beta"
/// answer, and a first name. A surname is optional; unknown client identity is quarantined.
fn classify(e: &CharEntry) -> Class {
    let flav = e.flavour.as_str();
    if flav == "retail" || (flav.is_empty() && e.sv < 2 && !e.realm.trim().is_empty()) {
        return Class::Retail;
    }
    if flav == "forever"
        && e.project == 18
        && e.beta == 0
        && e.confirmed == 1
        && !e.first.trim().is_empty()
    {
        return Class::Forever;
    }
    Class::Quarantine(if flav == "forever_beta" || e.beta == 1 {
        "forever_beta"
    } else if flav == "forever_unconfirmed" || (flav == "forever" && e.confirmed != 1) {
        "forever_unconfirmed"
    } else if flav == "forever" {
        "incomplete_identity"
    } else {
        "flavour_ambiguous"
    })
}

struct ForeverIdentity {
    previous: Option<(Uuid, i64, DateTime<Utc>)>,
    name: String,
    surname: String,
    guid: Option<String>,
}

/// A signed GUID is the strongest identity. Legacy files can fill the surname
/// only when the first-name match is unambiguous; never merge character rows.
async fn forever_identity(
    st: &AppState,
    uid: Uuid,
    e: &CharEntry,
) -> ApiResult<Option<ForeverIdentity>> {
    #[derive(sqlx::FromRow)]
    struct Candidate {
        id: Uuid,
        played_s: i64,
        played_at: DateTime<Utc>,
        name: String,
        surname: Option<String>,
        addon_guid: Option<String>,
    }
    let first = e.first.trim();
    let surname = e.surname.trim();
    // GUID was not covered by the v1/v2 checksum. Never use an unsigned extra field.
    let guid = (e.sv >= 3 && !e.guid.is_empty()).then_some(e.guid.as_str());
    let display = if surname.is_empty() {
        first.to_string()
    } else {
        format!("{first} {surname}")
    };
    let rows: Vec<Candidate> = sqlx::query_as(
        "SELECT id, played_s, played_at, name, surname, addon_guid FROM wow_characters
         WHERE user_id=$1 AND game_id='wow_forever' AND region=$2 AND realm=''
           AND (lower(btrim(first_name))=lower($3) OR addon_guid=$4 OR lower(name)=lower($5))",
    )
    .bind(uid)
    .bind(&e.region)
    .bind(first)
    .bind(guid)
    .bind(&display)
    .fetch_all(&st.db)
    .await?;
    let exact: Vec<_> = rows
        .iter()
        .filter(|r| guid.is_some() && r.addon_guid.as_deref() == guid)
        .collect();
    let compatible: Vec<_> = if !exact.is_empty() {
        exact
    } else {
        rows.iter()
            .filter(|r| {
                // Different known GUIDs are different characters, even with the same first name.
                if guid.is_some() && r.addon_guid.is_some() && r.addon_guid.as_deref() != guid {
                    return false;
                }
                let old_surname = r.surname.as_deref().unwrap_or("").trim();
                surname.is_empty()
                    || old_surname.is_empty()
                    || old_surname.eq_ignore_ascii_case(surname)
            })
            .collect()
    };
    if compatible.len() > 1 {
        return Ok(None);
    }
    let previous = compatible.first().copied();
    let surname = if surname.is_empty() {
        previous
            .and_then(|r| r.surname.as_deref())
            .unwrap_or("")
            .trim()
            .to_string()
    } else {
        surname.to_string()
    };
    let name = if surname.is_empty() {
        first.to_string()
    } else {
        format!("{first} {surname}")
    };
    if rows
        .iter()
        .any(|r| Some(r.id) != previous.map(|p| p.id) && r.name.eq_ignore_ascii_case(&name))
    {
        return Ok(None); // a display-name conflict is not permission to combine identities
    }
    Ok(Some(ForeverIdentity {
        previous: previous.map(|r| (r.id, r.played_s, r.played_at)),
        name,
        surname,
        guid: previous
            .and_then(|r| r.addon_guid.clone())
            .or_else(|| guid.map(str::to_string)),
    }))
}

async fn quarantine(st: &AppState, uid: Uuid, game: &str, e: &CharEntry, reason: &str) {
    let payload = json!({
        "region": e.region, "realm": e.realm, "name": e.name, "class": e.class,
        "level": e.level, "played": e.played, "at": e.at, "flavour": e.flavour,
        "project": e.project, "build": e.build, "first": e.first, "surname": e.surname,
        "beta": e.beta, "confirmed": e.confirmed, "guid": e.guid,
    });
    let _ = sqlx::query(
        "INSERT INTO wow_played_quarantine(id, user_id, claimed_game, reason, detail, payload)
         SELECT $1, $2, $3, $4, $5, $6
         WHERE NOT EXISTS (
             SELECT 1 FROM wow_played_quarantine
             WHERE user_id = $2 AND reason = $4 AND payload->>'name' = $7
               AND payload->>'played' = $8 AND payload->>'at' = $9)",
    )
    .bind(Uuid::new_v4())
    .bind(uid)
    .bind(game)
    .bind(reason)
    .bind("kept for review; no character or hours row was written")
    .bind(&payload)
    .bind(&e.name)
    .bind(e.played.to_string())
    .bind(e.at.to_string())
    .execute(&st.db)
    .await
    .inspect_err(|err| tracing::warn!("wow quarantine: insert for {uid}: {err}"));
}

/// Public Forever profile. A miss does not delete the character or any hour row.
/// Ownership is not decided here.
async fn refresh_forever_profile(st: &AppState, id: Uuid, e: &CharEntry) {
    if !forever_lookup_enabled() {
        return;
    }
    let due: Option<bool> = sqlx::query_scalar(
        "SELECT bnet_checked_at IS NULL OR bnet_checked_at < now() - interval '6 hours'
         FROM wow_characters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await
    .ok()
    .flatten();
    if due == Some(false) {
        return;
    }
    if !fetch_forever_profile(st, id, e).await {
        let st = st.clone();
        let region = e.region.clone();
        let first = e.first.clone();
        let surname = e.surname.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(15 * 60)).await;
            let again = CharEntry {
                region,
                first,
                surname,
                ..empty_entry()
            };
            let _ = fetch_forever_profile(&st, id, &again).await;
        });
    }
}

fn empty_entry() -> CharEntry {
    CharEntry {
        region: String::new(),
        realm: String::new(),
        name: String::new(),
        class: String::new(),
        level: 0,
        played: 0,
        at: 0,
        sv: 2,
        battletag: String::new(),
        flavour: String::new(),
        project: 0,
        build: String::new(),
        first: String::new(),
        surname: String::new(),
        guid: String::new(),
        beta: 0,
        confirmed: 0,
        sig: String::new(),
    }
}

/// Returns true when Battle.net answered. A failure leaves the last good fields.
async fn fetch_forever_profile(st: &AppState, id: Uuid, e: &CharEntry) -> bool {
    let Some(tok) = app_token().await else {
        return false;
    };
    let Some(http) = http() else { return false };
    let url = forever_profile_url(&e.region, &e.first, &e.surname);
    let Ok(resp) = http.get(&url).bearer_auth(tok).send().await else {
        return false;
    };
    if !resp.status().is_success() {
        let _ = sqlx::query("UPDATE wow_characters SET bnet_checked_at = now() WHERE id = $1")
            .bind(id)
            .execute(&st.db)
            .await;
        return false;
    }
    let Ok(v) = resp.json::<Value>().await else {
        return false;
    };
    let level = v["level"].as_i64();
    let _ = sqlx::query(
        "UPDATE wow_characters SET bnet_level = coalesce($2, bnet_level),
                bnet_spec = coalesce($3, bnet_spec), race = coalesce($4, race),
                gender = coalesce($5, gender), bnet_checked_at = now(), faction = coalesce($6, faction)
         WHERE id = $1",
    )
    .bind(id)
    .bind(level.map(|l| l as i32))
    .bind(name_of(&v["active_spec"]))
    .bind(name_of(&v["race"]))
    .bind(v["gender"]["type"].as_str().map(|g| g.to_lowercase()))
    .bind(profile_faction(&v))
    .execute(&st.db)
    .await
    .inspect_err(|err| tracing::warn!("forever profile: update {id}: {err}"));
    true
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
    .await
    .inspect_err(|e| tracing::warn!("wow flags: insert for {uid}: {e}"));
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
            "That WoW version is not taking /played uploads.",
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
        match classify(e) {
            Class::Retail if *gid == "wow" => {}
            Class::Forever if *gid == "wow_forever" => {}
            other => {
                let why = match other {
                    Class::Quarantine(w) => w,
                    _ => "flavour_mismatch",
                };
                quarantine(st, uid, gid, e, why).await;
                rejected.push(json!({ "character": label(e), "name": e.name, "realm": e.realm,
                    "region": e.region, "class": e.class, "level": e.level,
                    "played_min": e.played / 60, "reason": why,
                    "detail": "This entry was kept for review and was not stored as a character." }));
                continue;
            }
        }
        // Authenticate the entry before using any optional identity metadata.
        if *gid == "wow_forever" {
            if let Err(x) = check_entry(e, None, link.as_ref().map(|l| l.0.as_str()), now) {
                if x.flag {
                    flag(st, uid, gid, &who, &x, e.played).await;
                }
                quarantine(st, uid, gid, e, x.kind).await;
                rejected.push(json!({"character":who,"name":e.name,"realm":e.realm,"region":e.region,
                    "class":e.class,"level":e.level,"played_min":e.played/60,"reason":x.kind,"detail":x.detail}));
                continue;
            }
        }
        let identity = if *gid == "wow_forever" {
            let Some(identity) = forever_identity(st, uid, e).await? else {
                quarantine(st, uid, gid, e, "ambiguous_identity").await;
                rejected.push(json!({"character":who,"name":e.name,"realm":e.realm,"region":e.region,
                    "reason":"ambiguous_identity","detail":"More than one character could match this entry."}));
                continue;
            };
            Some(identity)
        } else {
            None
        };
        let stored_name = identity
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| e.name.clone());
        let prev: Option<(Uuid, i64, DateTime<Utc>)> = if let Some(identity) = &identity {
            identity.previous
        } else {
            sqlx::query_as(
                "SELECT id, played_s, played_at FROM wow_characters
                 WHERE user_id=$1 AND game_id=$2 AND region=$3 AND lower(realm)=lower($4) AND lower(name)=lower($5)",
            ).bind(uid).bind(gid).bind(&e.region).bind(&e.realm).bind(&stored_name).fetch_optional(&st.db).await?
        };
        let checked = check_entry(
            e,
            prev.map(|p| (p.1, p.2.timestamp())),
            link.as_ref().map(|l| l.0.as_str()),
            now,
        );
        if checked.is_ok() {
            if let Some(identity) = &identity {
                if let Some((id, _, at)) = identity.previous {
                    // A newer addon can enrich the same snapshot, but an older file
                    // cannot put stale display metadata back over a newer upload.
                    if e.at >= at.timestamp() {
                        let updated = sqlx::query("UPDATE wow_characters SET name=$2, first_name=$3, surname=$4, addon_guid=$5
                            WHERE id=$1 AND user_id=$6 AND (addon_guid IS NULL OR addon_guid IS NOT DISTINCT FROM $5)
                              AND ($5::text IS NOT NULL OR surname IS NULL OR surname='' OR lower(surname)=lower($4) OR $4='')")
                            .bind(id).bind(&identity.name).bind(e.first.trim()).bind(&identity.surname).bind(&identity.guid).bind(uid)
                            .execute(&st.db).await?;
                        if updated.rows_affected() == 0 {
                            quarantine(st, uid, gid, e, "ambiguous_identity").await;
                            rejected.push(json!({"character":who,"name":e.name,"realm":e.realm,"region":e.region,
                                "reason":"ambiguous_identity","detail":"The character identity changed during this upload."}));
                            continue;
                        }
                    }
                }
            }
        }
        match checked {
            Err(x) => {
                if x.flag {
                    flag(st, uid, gid, &who, &x, e.played).await;
                }
                if *gid == "wow_forever" {
                    quarantine(st, uid, gid, e, x.kind).await;
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
        let (realm, slug, name) = if *gid == "wow_forever" {
            (String::new(), String::new(), stored_name.clone())
        } else {
            (
                e.realm.chars().take(64).collect(),
                slug,
                stored_name.clone(),
            )
        };
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO wow_characters(id, user_id, game_id, region, realm, realm_slug, name, class,
                                        level, played_s, played_at, battletag, flavour, project_id,
                                        client_build, first_name, surname, verified, addon_guid)
             VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, false, $18)
             ON CONFLICT (user_id, game_id, region, lower(realm), lower(name)) DO UPDATE
               SET class = excluded.class, level = excluded.level, played_s = excluded.played_s,
                   played_at = excluded.played_at, uploaded_at = now(), realm_slug = excluded.realm_slug,
                   battletag = excluded.battletag, flavour = excluded.flavour,
                   project_id = excluded.project_id, client_build = excluded.client_build,
                   first_name = excluded.first_name, surname = excluded.surname,
                   addon_guid = coalesce(wow_characters.addon_guid, excluded.addon_guid)
             RETURNING id",
        )
        .bind(Uuid::new_v4())
        .bind(uid)
        .bind(gid)
        .bind(e.region.chars().take(8).collect::<String>())
        .bind(realm.chars().take(64).collect::<String>())
        .bind(&slug)
        .bind(if *gid == "wow_forever" { name.clone() } else { name.chars().take(64).collect::<String>() })
        .bind(e.class.chars().take(24).collect::<String>())
        .bind(e.level as i32)
        .bind(e.played)
        .bind(at)
        .bind(e.battletag.chars().take(64).collect::<String>())
        .bind(e.flavour.chars().take(32).collect::<String>())
        .bind(e.project as i32)
        .bind(e.build.chars().take(32).collect::<String>())
        .bind(e.first.trim())
        .bind(identity.as_ref().map(|i| i.surname.clone()).unwrap_or_else(|| e.surname.trim().to_string()))
        .bind(identity.as_ref().and_then(|i| i.guid.as_deref()))
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
        if *gid == "wow" {
            check_bnet_level(st, uid, gid, id, e).await;
        } else {
            // Public profile only, and only when the flag is on. It does not verify ownership.
            refresh_forever_profile(st, id, e).await;
        }
        accepted += 1;
    }
    // Same path for Retail and Forever. `gid` is the game being uploaded, so a
    // Forever file writes wow_forever only and a Retail file writes wow only.
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
/// The shown imported total is their sum (the same rule as `user_game_stats`). The tracked
/// minutes include the game running now (`play_sessions`).
pub(crate) async fn last_import(
    st: &AppState,
    uid: Uuid,
    gid: &str,
) -> ApiResult<Option<(i64, i64, i64)>> {
    Ok(sqlx::query_as(
        "WITH i AS (SELECT minutes, carry_min, captured_at FROM hours_entries
                    WHERE user_id = $1 AND game_id = $2 AND tier = 'imported'
                    ORDER BY captured_at DESC, created_at DESC LIMIT 1)
         SELECT i.minutes::bigint, coalesce(i.carry_min, 0)::bigint,
                coalesce((SELECT (sum(LEAST(x.duration_s::numeric,
                    extract(epoch FROM x.ended_at - GREATEST(x.started_at, i.captured_at)))) / 60)::bigint
                  FROM play_sessions x WHERE x.user_id = $1 AND x.game_id = $2 AND x.ended_at > i.captured_at), 0)::bigint
         FROM i",
    )
    .bind(uid)
    .bind(gid)
    .fetch_optional(&st.db)
    .await?)
}

/// Tracked minutes of the game in `[from, to]` (sessions clipped to the window).
async fn tracked_between(
    st: &AppState,
    uid: Uuid,
    gid: &str,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> ApiResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT coalesce(sum(GREATEST(0, LEAST(x.duration_s::numeric,
                    extract(epoch FROM LEAST(x.ended_at, $4) - GREATEST(x.started_at, $3))))), 0)::bigint / 60
         FROM play_sessions x
         WHERE x.user_id = $1 AND x.game_id = $2 AND x.ended_at > $3 AND x.started_at < $4",
    )
    .bind(uid)
    .bind(gid)
    .bind(from)
    .bind(to)
    .fetch_optional(&st.db)
    .await?
    .unwrap_or(0))
}

/// The shown imported total: latest import + carried extra + tracked play after it.
pub(crate) async fn imported_floor(st: &AppState, uid: Uuid, gid: &str) -> ApiResult<Option<i64>> {
    Ok(last_import(st, uid, gid).await?.map(|(m, c, a)| m + c + a))
}

/// The characters that count toward a user's game, one per Battle.net character id
/// (row id, /played seconds):
/// - verified ones (on the linked account's newest listing, see `set_verified`);
/// - accepted Forever characters that are not verified yet (the Forever account list
///   is still off, so they have no tick, but their /played is this game's imported total);
/// - ones verified before that are on nobody's listing any more (deleted: they keep their
///   hours) while Battle.net is linked;
/// - never one another user's Battle.net lists now (sold or moved), and a renamed or
///   transferred character counts once (its rows share the id; the verified/higher one).
const COUNTED: &str = "
    WITH c AS (
      SELECT c.id, c.bnet_id, c.played_s, c.verified FROM wow_characters c
      WHERE c.user_id = $1 AND c.game_id = $2
        AND (c.verified
             OR (c.game_id = 'wow_forever' AND (c.bnet_id IS NULL OR NOT EXISTS (
                  SELECT 1 FROM bnet_characters b WHERE b.game_id = c.game_id AND b.region = c.region
                    AND b.bnet_id = c.bnet_id AND b.user_id <> c.user_id)))
             OR (c.ever_verified
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
    // Older imported rows have no per-character inputs. That is not an error.
    let last: Option<(i32, Option<Value>)> = sqlx::query_as(
        "SELECT minutes, inputs FROM hours_entries WHERE user_id = $1 AND game_id = $2 AND source = 'wow_addon'
         ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(uid)
    .bind(gid)
    .fetch_optional(&st.db)
    .await?;
    let prev_map: Option<std::collections::BTreeMap<String, i64>> = last
        .as_ref()
        .and_then(|(_, i)| i.as_ref())
        .and_then(|i| serde_json::from_value(i["chars"].clone()).ok());
    // The time this total is "as of": the newest snapshot among the counted characters,
    // never before the last stored total and never in the future. Not the upload time:
    // after a WoW crash the addon's file still holds the /played from before that
    // session, so play between the snapshot and the upload would be carried as menu
    // time and then counted again once a later snapshot's /played includes it.
    let ids: Vec<&String> = now_map.keys().collect();
    let (as_of, prev_at): (Option<DateTime<Utc>>, Option<DateTime<Utc>>) = sqlx::query_as(
        "WITH p AS (SELECT max(captured_at) AS at FROM hours_entries
                    WHERE user_id = $1 AND game_id = $2 AND tier = 'imported')
         SELECT LEAST(now(), coalesce(
                    GREATEST(p.at, (SELECT max(played_at) FROM wow_characters
                        WHERE user_id = $1 AND game_id = $2 AND id::text = ANY($3))),
                    p.at, now())),
                p.at
         FROM p",
    )
    .bind(uid)
    .bind(gid)
    .bind(&ids)
    .fetch_one(&st.db)
    .await?;
    let as_of = as_of.unwrap_or_else(Utc::now);
    let mut carry = 0i64;
    let store = match mode {
        Recount::Reset => last.is_some() || sum > 0,
        Recount::Exact => {
            last.as_ref().map(|l| i64::from(l.0)) != Some(sum) && !(last.is_none() && sum == 0)
        }
        Recount::KeepHigher => match last_import(st, uid, gid).await? {
            None => sum > 0,
            Some((m0, c0, _)) => {
                // tracked play between the last total and this one's snapshot only
                let after = tracked_between(st, uid, gid, prev_at.unwrap_or(as_of), as_of).await?;
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
            "INSERT INTO hours_entries(id, user_id, game_id, tier, source, minutes, inputs, note, carry_min,
                                       captured_at)
             VALUES($1, $2, $3, 'imported', 'wow_addon', $4, $5, $6, $7, $8)",
        )
        .bind(Uuid::new_v4())
        .bind(uid)
        .bind(gid)
        .bind(sum as i32)
        .bind(json!({ "characters": n, "verified": v, "reset": mode == Recount::Reset, "chars": now_map }))
        .bind(note)
        .bind(carry.min(i32::MAX as i64) as i32)
        .bind(as_of)
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

fn flag_on(key: &str) -> bool {
    matches!(
        std::env::var(key).ok().as_deref().map(str::trim),
        Some("on") | Some("1")
    )
}

/// The user-token account list for Forever. Off until that endpoint has been tested.
/// A public profile is not this flag and never marks a character verified.
pub(crate) fn forever_account_enabled() -> bool {
    flag_on("BNET_FOREVER_ACCOUNT")
}

/// Public Forever profile lookups. Off leaves imports working and keeps the last
/// good profile fields. It does not verify ownership.
pub(crate) fn forever_lookup_enabled() -> bool {
    flag_on("BNET_FOREVER_LOOKUP")
}

/// `profile-forever-{region}` and `/profile/wow/character/{first}-{surname}`.
/// Not the Retail realm-slug URL.
pub(crate) fn forever_profile_url(region: &str, first: &str, surname: &str) -> String {
    let part = |s: &str| {
        s.trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
    };
    format!(
        "https://{region}.api.blizzard.com/profile/wow/character/{}-{}?namespace=profile-forever-{region}&locale=en_US",
        part(first),
        part(surname)
    )
}

/// Namespace for generic realm-based Retail requests. Forever must use its
/// dedicated realmless account/profile helpers, never this path.
pub(crate) fn namespace(game: &str, region: &str) -> Option<String> {
    match game {
        "wow" => Some(format!("profile-{region}")),
        _ => None,
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
    // Forever has a separate, non-reconciling account path below.
    let game = "wow";
    for r in REGIONS {
        let Some(ns) = namespace(game, r) else {
            continue;
        };
        let url =
            format!("https://{r}.api.blizzard.com/profile/user/wow?namespace={ns}&locale=en_US");
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
                let Some(name) = c["name"].as_str() else {
                    continue;
                };
                let Some(slug) = c["realm"]["slug"].as_str() else {
                    continue;
                };
                let slug = slug.to_string();
                let realm_name = c["realm"]["name"].as_str().unwrap_or_default().to_string();
                rows.push((
                    game.to_string(),
                    r.to_string(),
                    slug,
                    realm_name,
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
    let forever = super::wow_forever_account::sync(st, uid, token).await;
    if !any_ok {
        if let Some(count) = forever {
            return Some(vec![("wow_forever".into(), "us".into()); count]);
        }
        tracing::warn!("bnet characters: no region answered for {uid}");
        return None;
    }
    let _ = sqlx::query("DELETE FROM bnet_characters WHERE user_id = $1 AND game_id = 'wow'")
        .bind(uid)
        .execute(&st.db)
        .await
        .inspect_err(|e| tracing::warn!("bnet characters: clear for {uid}: {e}"));
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
        .await
        .inspect_err(|e| tracing::warn!("bnet characters: insert for {uid}: {e}"));
    }
    copy_bnet_levels(&st.db, uid).await;
    tracing::info!("bnet characters: {} for {uid}", rows.len());
    reverify(st, uid).await;
    crate::wowpage::after_sync(st, uid).await;
    let mut found: Vec<_> = rows.into_iter().map(|r| (r.0, r.1)).collect();
    if let Some(count) = forever {
        found.extend(vec![("wow_forever".into(), "us".into()); count]);
    }
    Some(found)
}

/// The account listing's levels onto the user's uploaded characters (`shown_level!`).
/// The listing is fetched live, so it is the newest level Blizzard has.
pub async fn copy_bnet_levels(db: &sqlx::PgPool, uid: Uuid) {
    let _ = sqlx::query(
        "UPDATE wow_characters c SET bnet_level = b.level FROM bnet_characters b
         WHERE c.game_id = 'wow' AND c.user_id = $1 AND b.user_id = c.user_id AND b.game_id = c.game_id AND b.region = c.region
           AND (b.bnet_id = c.bnet_id OR (b.realm_slug = c.realm_slug AND lower(b.name) = lower(c.name)))
           AND b.level > 0 AND c.bnet_level IS DISTINCT FROM b.level",
    )
    .bind(uid)
    .execute(db)
    .await
    .inspect_err(|e| tracing::warn!("bnet characters: level copy for {uid}: {e}"));
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AcctName {
    region: String,
    realm_slug: String,
    realm_name: String,
    name: String,
    bnet_id: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OwnedChar {
    id: Uuid,
    region: String,
    name: String,
    realm: String,
    realm_slug: String,
    played_s: i64,
    played_at: DateTime<Utc>,
    verified: bool,
    ever_verified: bool,
    battletag: String,
    created_at: DateTime<Utc>,
    bnet_id: Option<i64>,
}

fn same_character(row: &OwnedChar, acct: &AcctName) -> bool {
    row.region == acct.region
        && row.realm_slug == acct.realm_slug
        && row.name.eq_ignore_ascii_case(&acct.name)
}

fn tag_matches(row: &OwnedChar, tag: &str) -> bool {
    !tag.is_empty() && row.battletag.eq_ignore_ascii_case(tag)
}

/// The newest /played is lower than an older one. Those two counters are not combined.
fn counters_disagree(rows: &[OwnedChar]) -> bool {
    let Some(newest) = rows.iter().max_by_key(|r| r.played_at) else {
        return false;
    };
    rows.iter().any(|r| r.played_s > newest.played_s)
}

fn display_realm(gid: &str, acct: &AcctName, rows: &[&OwnedChar]) -> String {
    if gid == "wow_forever" {
        return String::new();
    }
    if !acct.realm_name.is_empty() {
        return acct.realm_name.clone();
    }
    if let Some(row) = rows
        .iter()
        .find(|r| r.realm_slug == acct.realm_slug && !r.realm.is_empty())
    {
        return row.realm.clone();
    }
    rows.iter()
        .find(|r| !r.realm.is_empty())
        .map(|r| r.realm.clone())
        .unwrap_or_default()
}

fn forever_parts(name: &str) -> Option<(String, String)> {
    let mut parts = name.split_whitespace();
    let first = parts.next()?.trim();
    let surname = parts.collect::<Vec<_>>().join(" ");
    if first.is_empty() || surname.trim().is_empty() {
        None
    } else {
        Some((first.to_string(), surname.trim().to_string()))
    }
}

/// Retail rename/merge reconciliation only. Forever ownership verification must
/// never enter this path, even when its account feature is enabled.
pub async fn reconcile_linked_characters(
    st: &AppState,
    uid: Uuid,
    _include_forever: bool,
) -> ApiResult<()> {
    let tag: Option<String> = sqlx::query_scalar(
        "SELECT display FROM linked_accounts
         WHERE user_id = $1 AND provider = 'bnet' AND verified LIMIT 1",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?;
    let Some(tag) = tag.filter(|t| !t.trim().is_empty()) else {
        return Ok(());
    };
    reconcile_game(st, uid, "wow", &tag).await?;
    Ok(())
}

async fn reconcile_game(st: &AppState, uid: Uuid, gid: &str, tag: &str) -> ApiResult<()> {
    if gid != "wow" {
        return Ok(());
    }
    let accounts: Vec<AcctName> = sqlx::query_as(
        "SELECT region, realm_slug, realm_name, name, bnet_id FROM bnet_characters
         WHERE user_id = $1 AND game_id = $2 AND bnet_id IS NOT NULL",
    )
    .bind(uid)
    .bind(gid)
    .fetch_all(&st.db)
    .await?;
    if accounts.is_empty() {
        return Ok(());
    }
    let mut mine: Vec<OwnedChar> = sqlx::query_as(
        "SELECT id, region, name, realm, realm_slug, played_s, played_at, verified, ever_verified,
                battletag, created_at, bnet_id
         FROM wow_characters WHERE user_id = $1 AND game_id = $2",
    )
    .bind(uid)
    .bind(gid)
    .fetch_all(&st.db)
    .await?;
    let mut merged = false;
    let mut seen = Vec::<i64>::new();
    for acct in &accounts {
        if seen.contains(&acct.bnet_id) {
            continue;
        }
        seen.push(acct.bnet_id);
        if gid == "wow_forever" && forever_parts(&acct.name).is_none() {
            hold(
                st,
                uid,
                gid,
                acct,
                "rename_forever_name",
                "Forever account name has no surname, so it was not applied.",
            )
            .await?;
            continue;
        }
        let locals: Vec<OwnedChar> = mine
            .iter()
            .filter(|r| {
                r.region == acct.region
                    && r.bnet_id == Some(acct.bnet_id)
                    && (r.verified || r.ever_verified)
                    && tag_matches(r, tag)
            })
            .cloned()
            .collect();
        if locals.is_empty() {
            continue;
        }
        let target = mine.iter().find(|r| same_character(r, acct)).cloned();
        let other_owner: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM wow_characters
               WHERE user_id <> $1 AND game_id = $2 AND region = $3 AND bnet_id = $4)",
        )
        .bind(uid)
        .bind(gid)
        .bind(&acct.region)
        .bind(acct.bnet_id)
        .fetch_one(&st.db)
        .await?;
        let target_is_local = target
            .as_ref()
            .is_some_and(|t| locals.iter().any(|r| r.id == t.id));
        let would_change = locals.iter().any(|r| !same_character(r, acct))
            || locals.len() > 1
            || matches!(&target, Some(t) if !target_is_local);
        if other_owner && would_change {
            hold(
                st,
                uid,
                gid,
                acct,
                "rename_other_owner",
                "Another account already has this Blizzard character id.",
            )
            .await?;
            continue;
        }
        if let Some(t) = &target {
            if t.bnet_id.is_some() && t.bnet_id != Some(acct.bnet_id) {
                hold(
                    st,
                    uid,
                    gid,
                    acct,
                    "rename_other_id",
                    "The new name already belongs to a different Blizzard character id.",
                )
                .await?;
                continue;
            }
            let on_account = tag_matches(t, tag);
            if !target_is_local && !on_account {
                hold(
                    st,
                    uid,
                    gid,
                    acct,
                    "rename_unverified",
                    "The new name is on a character that is not verified to this account.",
                )
                .await?;
                continue;
            }
        }
        let mut pool = locals;
        if let Some(t) = &target {
            if !pool.iter().any(|r| r.id == t.id) && tag_matches(t, tag) {
                pool.push(t.clone());
            }
        }
        if counters_disagree(&pool) {
            hold(
                st,
                uid,
                gid,
                acct,
                "rename_counters",
                "The newer /played is lower than an older one, so the rows were left as they are.",
            )
            .await?;
            continue;
        }
        let Some(keep) = pool
            .iter()
            .filter(|r| !same_character(r, acct))
            .min_by_key(|r| (r.created_at, r.id))
            .cloned()
            .or_else(|| pool.iter().min_by_key(|r| (r.created_at, r.id)).cloned())
        else {
            continue;
        };
        let drops: Vec<OwnedChar> = pool.into_iter().filter(|r| r.id != keep.id).collect();
        if drops.is_empty() && same_character(&keep, acct) {
            continue;
        }
        let dropped_ids: Vec<Uuid> = drops.iter().map(|d| d.id).collect();
        let played_s = std::iter::once(&keep)
            .chain(drops.iter())
            .map(|r| r.played_s)
            .max()
            .unwrap_or(keep.played_s);
        apply_rename(st, uid, gid, acct, &keep, &drops).await?;
        mine.retain(|r| r.id != keep.id && !dropped_ids.contains(&r.id));
        let mut now = keep.clone();
        now.name = acct.name.clone();
        let realm_rows = std::iter::once(&keep)
            .chain(drops.iter())
            .collect::<Vec<_>>();
        now.realm = display_realm(gid, acct, &realm_rows);
        now.realm_slug = if gid == "wow_forever" {
            String::new()
        } else {
            acct.realm_slug.clone()
        };
        now.played_s = played_s;
        now.bnet_id = Some(acct.bnet_id);
        mine.push(now);
        merged = merged || !dropped_ids.is_empty();
    }
    if merged {
        record_total(st, uid, gid, Recount::KeepHigher).await?;
    }
    Ok(())
}

async fn hold(
    st: &AppState,
    uid: Uuid,
    gid: &str,
    acct: &AcctName,
    reason: &str,
    detail: &str,
) -> ApiResult<()> {
    let bid = acct.bnet_id.to_string();
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM wow_played_quarantine
           WHERE user_id = $1 AND claimed_game = $2 AND reason = $3
             AND payload->>'bnet_id' = $4 AND payload->>'region' = $5)",
    )
    .bind(uid)
    .bind(gid)
    .bind(reason)
    .bind(&bid)
    .bind(&acct.region)
    .fetch_one(&st.db)
    .await?;
    if exists {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO wow_played_quarantine(id, user_id, claimed_game, reason, detail, payload)
         VALUES($1, $2, $3, $4, $5, $6)",
    )
    .bind(Uuid::new_v4())
    .bind(uid)
    .bind(gid)
    .bind(reason)
    .bind(detail)
    .bind(json!({
        "bnet_id": acct.bnet_id,
        "region": acct.region,
        "name": acct.name,
        "realm_slug": acct.realm_slug,
    }))
    .execute(&st.db)
    .await?;
    Ok(())
}

async fn remember_name<'e, E>(
    ex: E,
    char_id: Uuid,
    uid: Uuid,
    name: &str,
    realm: &str,
    realm_slug: &str,
) -> ApiResult<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(
        "INSERT INTO wow_character_names(char_id, user_id, name, realm, realm_slug)
         SELECT $1, $2, $3, $4, $5
         WHERE NOT EXISTS (
             SELECT 1 FROM wow_character_names
             WHERE char_id = $1 AND lower(name) = lower($3) AND lower(realm) = lower($4))",
    )
    .bind(char_id)
    .bind(uid)
    .bind(name)
    .bind(realm)
    .bind(realm_slug)
    .execute(ex)
    .await?;
    Ok(())
}

async fn apply_rename(
    st: &AppState,
    uid: Uuid,
    gid: &str,
    acct: &AcctName,
    keep: &OwnedChar,
    drops: &[OwnedChar],
) -> ApiResult<()> {
    let new_slug = if gid == "wow_forever" {
        String::new()
    } else {
        acct.realm_slug.clone()
    };
    let realm_rows = std::iter::once(keep)
        .chain(drops.iter())
        .collect::<Vec<_>>();
    let new_realm = display_realm(gid, acct, &realm_rows);
    let new_name = acct.name.chars().take(64).collect::<String>();
    let (first, surname) = if gid == "wow_forever" {
        forever_parts(&new_name).unwrap_or_default()
    } else {
        (String::new(), String::new())
    };
    let max_played = std::iter::once(keep)
        .chain(drops.iter())
        .map(|r| r.played_s)
        .max()
        .unwrap_or(keep.played_s);
    let played_at = std::iter::once(keep)
        .chain(drops.iter())
        .max_by_key(|r| (r.played_s, r.played_at))
        .map(|r| r.played_at)
        .unwrap_or(keep.played_at);
    let mut tx = st.db.begin().await?;
    let name_changed = !keep.name.eq_ignore_ascii_case(&new_name)
        || keep.realm != new_realm
        || keep.realm_slug != new_slug;
    if name_changed {
        remember_name(
            &mut *tx,
            keep.id,
            uid,
            &keep.name,
            &keep.realm,
            &keep.realm_slug,
        )
        .await?;
    }
    for drop in drops {
        if !drop.name.eq_ignore_ascii_case(&new_name) || drop.realm != new_realm {
            remember_name(
                &mut *tx,
                keep.id,
                uid,
                &drop.name,
                &drop.realm,
                &drop.realm_slug,
            )
            .await?;
        }
        let before: Value =
            sqlx::query_scalar("SELECT to_jsonb(c) FROM wow_characters c WHERE id = $1")
                .bind(drop.id)
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(
            "INSERT INTO wow_played_snaps(char_id, played_s, at)
             SELECT $1, played_s, at FROM wow_played_snaps WHERE char_id = $2
             ON CONFLICT (char_id, at) DO UPDATE
               SET played_s = GREATEST(wow_played_snaps.played_s, EXCLUDED.played_s)",
        )
        .bind(keep.id)
        .bind(drop.id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE wow_character_names SET char_id = $1 WHERE char_id = $2")
            .bind(keep.id)
            .bind(drop.id)
            .execute(&mut *tx)
            .await?;
        let after = json!({
            "kept_id": keep.id,
            "name": new_name,
            "realm": new_realm,
            "played_s": max_played,
        });
        sqlx::query(
            "INSERT INTO wow_character_merges(user_id, kept_id, dropped_id, before, after)
             VALUES($1, $2, $3, $4, $5)",
        )
        .bind(uid)
        .bind(keep.id)
        .bind(drop.id)
        .bind(&before)
        .bind(&after)
        .execute(&mut *tx)
        .await?;
        let removed = sqlx::query("DELETE FROM wow_characters WHERE id = $1 AND user_id = $2")
            .bind(drop.id)
            .bind(uid)
            .execute(&mut *tx)
            .await?;
        if removed.rows_affected() != 1 {
            return Err(AppError::bad_request(
                "rename_failed",
                "The duplicate character was not removed.",
            ));
        }
    }
    sqlx::query(
        "UPDATE wow_characters SET name = $2, realm = $3, realm_slug = $4, played_s = $5, played_at = $6,
                first_name = CASE WHEN $7 = '' THEN first_name ELSE $7 END,
                surname = CASE WHEN $8 = '' THEN surname ELSE $8 END
         WHERE id = $1 AND user_id = $9",
    )
    .bind(keep.id)
    .bind(&new_name)
    .bind(&new_realm)
    .bind(&new_slug)
    .bind(max_played)
    .bind(played_at)
    .bind(&first)
    .bind(&surname)
    .bind(uid)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// A character is verified while it is on the linked Battle.net account (its `wow.profile`
/// list), its latest accepted entry was saved under that account's BattleTag, and no other
/// user's Battle.net listed the same character id more recently (sold or moved: the
/// current holder gets the hours). Several WoW licences on one Battle.net (WoW1, WoW2)
/// share the BattleTag, so all of them count. Other users holding one of this user's
/// character ids are rechecked too, so a sold character leaves the old owner's total.
/// `mode` None: the caller stores this user's totals itself (an upload).
async fn set_verified(st: &AppState, uid: Uuid, mode: Option<Recount>) {
    // Retail ids from this user's listing onto their uploaded rows.
    // Forever uses its own ownership-only path below.
    let _ = sqlx::query(
        "UPDATE wow_characters c SET bnet_id = b.bnet_id FROM bnet_characters b
         WHERE c.game_id = 'wow' AND c.user_id = $1 AND b.user_id = c.user_id AND b.game_id = c.game_id AND b.region = c.region
           AND b.realm_slug = c.realm_slug AND lower(b.name) = lower(c.name) AND b.bnet_id IS NOT NULL
           AND c.bnet_id IS DISTINCT FROM b.bnet_id",
    )
    .bind(uid)
    .execute(&st.db)
    .await
    .inspect_err(|e| tracing::warn!("bnet characters: id copy for {uid}: {e}"));
    // Forever never enters Retail reconciliation or total recording.
    if forever_account_enabled() {
        if let Err(e) = super::wow_forever_account::verify_cached(&st.db, uid).await {
            tracing::warn!("Forever verification for {uid}: {e}");
        }
    }
    if let Err(e) = reconcile_linked_characters(st, uid, false).await {
        tracing::warn!("wow rename: {uid}: {e:?}");
    }
    let mut users: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT o.user_id FROM wow_characters o JOIN wow_characters c
           ON c.user_id = $1 AND o.game_id = c.game_id AND o.region = c.region AND o.bnet_id = c.bnet_id
         WHERE o.user_id <> $1 AND c.game_id = 'wow'",
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
         WHERE c.user_id = ANY($1) AND c.game_id = 'wow'",
    )
    .bind(&users)
    .execute(&st.db)
    .await
    .inspect_err(|e| tracing::warn!("bnet characters: verified update: {e}"));
    let _ = sqlx::query(
        "UPDATE wow_characters SET ever_verified = true
         WHERE user_id = ANY($1) AND game_id = 'wow' AND verified AND NOT ever_verified",
    )
    .bind(&users)
    .execute(&st.db)
    .await
    .inspect_err(|e| tracing::warn!("bnet characters: ever_verified update: {e}"));
    // Verification decides what counts, so refresh the stored totals.
    for (i, u) in users.iter().enumerate() {
        let m = match (i, mode) {
            (0, None) => continue,
            (0, Some(m)) => m,
            _ => Recount::KeepHigher,
        };
        let _ = record_total(st, *u, "wow", m).await;
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
        .await
        .inspect_err(|e| tracing::warn!("bnet characters: clear on unlink for {uid}: {e}"));
    // Unlink revokes Forever ownership only; its hours and history stay untouched.
    let _ = sqlx::query(
        "UPDATE wow_characters SET verified = false WHERE user_id = $1 AND game_id = 'wow_forever'",
    )
    .bind(uid)
    .execute(&st.db)
    .await;
    // Retail keeps its existing exact recount on unlink.
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
    if gid != "wow" {
        return; // Forever metadata uses its guarded realmless lookup.
    }
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
                bnet_id = coalesce(bnet_id, $7), faction = coalesce($8, faction)
         WHERE id = $1",
    )
    .bind(id)
    .bind(level.map(|l| l as i32))
    .bind(last)
    .bind(name_of(&v["active_spec"]))
    .bind(name_of(&v["race"]))
    .bind(v["gender"]["type"].as_str().map(|g| g.to_lowercase()))
    .bind(v["id"].as_i64())
    .bind(profile_faction(&v))
    .execute(&st.db)
    .await
    .inspect_err(|e| tracing::warn!("wow armory: update character {id}: {e}"));
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

/// Blizzard's stable API value, not a localized faction name or a race guess.
pub fn profile_faction(profile: &Value) -> Option<&str> {
    profile["faction"]["type"]
        .as_str()
        .filter(|f| matches!(*f, "HORDE" | "ALLIANCE"))
}

/// Characters for a profile page. `all`: the owner's own view (hidden ones too).
/// Profile cards list Battle.net-verified Retail characters (`verified_only`). Forever
/// characters are included too, without a tick, until `BNET_FOREVER_ACCOUNT` verifies
/// them. The owner's /connect/wow page lists every row so unverified ones can be verified.
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
        faction: Option<String>,
    }
    let rows: Vec<Row> = sqlx::query_as(
            concat!("SELECT id, game_id, region, realm, name, class, ", shown_level!(), " AS level, played_s, played_at, verified,
                    hidden OR coalesce((SELECT hide_all FROM wow_prefs WHERE user_id = $1), false) AS hid,
                    race, gender, coalesce(spec, bnet_spec) AS spec, show_guild,
                    CASE WHEN show_guild THEN (SELECT d.data->'guild'->>'name' FROM wow_char_details d
                         WHERE d.char_id = wow_characters.id AND d.status = 'ok') END AS guild,
                    featured, coalesce(faction, (SELECT CASE upper(d.data->>'faction') WHEN 'HORDE' THEN 'HORDE' WHEN 'ALLIANCE' THEN 'ALLIANCE' END FROM wow_char_details d WHERE d.char_id = wow_characters.id AND d.status = 'ok')) AS faction
             FROM wow_characters WHERE user_id = $1
               AND ($2 OR NOT (hidden OR coalesce((SELECT hide_all FROM wow_prefs WHERE user_id = $1), false)))
               AND (NOT $3 OR verified OR game_id = 'wow_forever')
             ORDER BY game_id, featured NULLS LAST, played_s DESC LIMIT 200"),
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
                "featured": c.featured, "faction": c.faction })
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
    let rows: Vec<(i64, Uuid, String, String, Option<String>, String, String, Option<i64>, String, DateTime<Utc>)> =
        sqlx::query_as(
            "SELECT f.id, f.user_id, u.handle, f.game_id, g.name, f.char_label, f.kind, f.played_s, f.detail, f.created_at
             FROM wow_flags f JOIN users u ON u.id = f.user_id LEFT JOIN games g ON g.id = f.game_id
             WHERE f.created_at > now() - make_interval(days => $1) AND u.deleted_at IS NULL
             ORDER BY f.created_at DESC LIMIT 200",
        )
        .bind(days)
        .fetch_all(&st.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, u, h, g, n, who, k, s, d, at)| {
            json!({ "kind": k, "flag_key": format!("wow:{id}"),
                    "evidence": format!("{k}:{}:{d}", s.unwrap_or(0)),
                    "user_id": u, "handle": h, "game_id": g, "game_name": n,
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
         WHERE game_id = 'wow' AND bnet_id IS NULL AND bnet_id_tried_at IS NULL ORDER BY user_id LIMIT 2000",
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
        .await
        .inspect_err(|e| tracing::warn!("wow armory: bnet id for {id}: {e}"));
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
            .await
            .inspect_err(|e| tracing::warn!("bnet characters: bnet id for {uid}: {e}"));
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
            flavour: String::new(),
            project: 0,
            build: String::new(),
            first: String::new(),
            surname: String::new(),
            guid: String::new(),
            beta: 0,
            confirmed: 0,
            sig: String::new(),
        };
        c.sig = sign_with(SIG_KEYS[0].1, &c);
        c
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn faction_uses_only_stable_blizzard_type() {
        assert_eq!(
            profile_faction(&json!({"faction": {"type": "HORDE", "name": "Horde"}})),
            Some("HORDE")
        );
        assert_eq!(
            profile_faction(&json!({"faction": {"type": "ALLIANCE"}})),
            Some("ALLIANCE")
        );
        assert_eq!(
            profile_faction(&json!({"faction": {"type": "NEUTRAL"}})),
            None
        );
        assert_eq!(
            profile_faction(&json!({"faction": {"name": "Alliance"}})),
            None
        );
        assert_eq!(profile_faction(&json!({})), None);
    }

    #[test]
    fn generic_namespace_never_routes_forever_through_realm_urls() {
        assert_eq!(namespace("wow", "us").as_deref(), Some("profile-us"));
        assert!(namespace("wow_forever", "us").is_none());
        assert!(namespace("wow_forever", "eu").is_none());
    }

    #[test]
    fn forever_profile_url_has_no_realm() {
        let url = forever_profile_url("eu", "Vale", "Mocken");
        assert_eq!(
            url,
            "https://eu.api.blizzard.com/profile/wow/character/vale-mocken?namespace=profile-forever-eu&locale=en_US"
        );
        assert!(!url.contains("realm"));
        assert!(!forever_account_enabled());
        assert!(!forever_lookup_enabled());
    }

    #[test]
    fn rename_rules() {
        assert_eq!(forever_parts("Vale Renamed").unwrap().1, "Renamed");
        assert!(forever_parts("Vale").is_none());
        let row = |played_s, played_at| OwnedChar {
            id: Uuid::nil(),
            region: "us".into(),
            name: "A".into(),
            realm: "Area 52".into(),
            realm_slug: "area-52".into(),
            played_s,
            played_at,
            verified: true,
            ever_verified: true,
            battletag: "A#1".into(),
            created_at: played_at,
            bnet_id: Some(1),
        };
        let older = row(9_000, Utc.timestamp_opt(NOW - 5_000, 0).unwrap());
        let newer_lower = row(4_000, Utc.timestamp_opt(NOW - 1_000, 0).unwrap());
        let newer_higher = row(12_000, Utc.timestamp_opt(NOW - 1_000, 0).unwrap());
        assert!(counters_disagree(&[older.clone(), newer_lower]));
        assert!(!counters_disagree(&[older, newer_higher]));
    }

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
