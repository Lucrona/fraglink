//! WoW character popup (`GET /api/v1/users/{handle}/wow/{id}`): a summary of the character's
//! Blizzard profile (render, gear, guild, M+, PvP, raids, collections, professions). Fetched
//! with the app's client-credentials token, summarised and stored per character in
//! `wow_char_details` for 12 hours; stale data is served while a background refresh runs
//! (one at a time per character). The 1 MB achievements endpoint is never fetched. Only
//! non-hidden characters where the profile shows them, only while Admin >
//! "Show WoW character details" is on. Retail needs verification; Forever needs
//! the default-off BNET_FOREVER_LOOKUP switch. The guild shows only with the
//! character's "Show guild" switch. No hours here.

use crate::auth::{AuthUser, MaybeUser};
use crate::error::{ApiResult, AppError};
use crate::wow::{app_token, http, name_of};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, Response, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub const TTL_H: i64 = 12;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/users/{handle}/wow/{id}", get(detail))
        .route("/api/v1/users/{handle}/wow/{id}/render.png", get(render))
        .route("/api/v1/users/{handle}/wow/{id}/share", get(share))
        .route("/api/v1/users/{handle}/wow/{id}/avatar", get(avatar))
        .route("/api/v1/me/wow/{id}/guild", post(set_guild))
}

// Avatar lookups are media-only: no character, /played or persistent cache writes.
// Reuse warm popup media; otherwise cache the public URL only in bounded memory.
type AvatarCache = Mutex<HashMap<String, (Option<String>, Instant)>>;
fn avatar_cache() -> &'static AvatarCache {
    static CACHE: OnceLock<AvatarCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn avatar_url_ok(s: &str) -> bool {
    s.starts_with("https://render.worldofwarcraft.com/")
        && (s.ends_with(".jpg") || s.ends_with(".png"))
        && !s.contains("..")
}

pub fn media_avatar(media: &Value, expected_id: Option<i64>) -> Option<String> {
    if expected_id.is_some_and(|id| {
        media["character"]["id"]
            .as_i64()
            .is_some_and(|got| got != id)
    }) {
        return None;
    }
    media["assets"]
        .as_array()?
        .iter()
        .find(|a| a["key"] == "avatar")?["value"]
        .as_str()
        .filter(|s| avatar_url_ok(s))
        .map(str::to_owned)
}

pub fn forever_media_avatar(media: &Value, expected_id: Option<i64>) -> Option<String> {
    media_avatar(media, expected_id)
}

async fn avatar(
    State(st): State<AppState>,
    m: MaybeUser,
    headers: HeaderMap,
    Path((handle, id)): Path<(String, Uuid)>,
) -> ApiResult<(HeaderMap, Json<Value>)> {
    let nf = || {
        AppError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That character isn't shown on this profile.",
        )
    };
    let viewer = m.0.as_ref().map(|a| a.user.id);
    let key = viewer
        .map(|v| v.to_string())
        .unwrap_or_else(|| crate::client_ip(&headers));
    st.rate_limit(format!("wowavatar:{key}"), 480, Duration::from_secs(600))?;
    // Managers may see their own hidden characters. Everyone else must pass the
    // existing profile visibility checks before even reading cached media.
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users u JOIN wow_characters c ON c.user_id=u.id WHERE u.id=$1 AND lower(u.handle)=lower($2) AND c.id=$3)")
        .bind(viewer).bind(&handle).bind(id).fetch_one(&st.db).await?;
    if !owner {
        visible_char(&st, viewer, &handle, id).await?;
    }
    let row: Option<(String, String, String, String, Option<i64>, Option<String>)> = sqlx::query_as(
        "SELECT c.game_id,c.name,c.region,c.realm_slug,c.bnet_id,(SELECT d.data->>'avatar_src' FROM wow_char_details d WHERE d.char_id=c.id AND d.status='ok' AND d.fetched_at > now()-interval '12 hours') FROM wow_characters c WHERE c.id=$1 AND c.game_id IN ('wow','wow_forever')"
    ).bind(id).fetch_optional(&st.db).await?;
    let Some((game, name, region, realm_slug, bnet_id, warm)) = row else {
        return Err(nf());
    };
    if game == "wow_forever" && !crate::wow::forever_lookup_enabled() {
        return Err(nf());
    }
    let mut src = warm.filter(|s| avatar_url_ok(s));
    let media_url = if game == "wow_forever" {
        if region == "us" {
            crate::wow::forever_parts(&name).map(|(first, surname)| {
                let base = crate::wow::forever_profile_url("us", &first, &surname);
                let (path, query) = base
                    .split_once('?')
                    .expect("Forever helper includes namespace");
                format!("{path}/character-media?{query}")
            })
        } else {
            None
        }
    } else if matches!(region.as_str(), "us" | "eu" | "kr" | "tw") && !realm_slug.is_empty() {
        Some(format!(
            "https://{region}.api.blizzard.com/profile/wow/character/{}/{}/character-media?namespace=profile-{region}&locale=en_US",
            realm_slug,
            name.to_lowercase()
        ))
    } else {
        None
    };
    if src.is_none() {
        if let Some(url) = media_url {
            let key = format!("{url}:{bnet_id:?}");
            let lock = lock_for(id);
            let _guard = lock.lock().await;
            let cached = avatar_cache()
                .lock()
                .ok()
                .and_then(|c| c.get(&key).cloned())
                .filter(|(v, at)| {
                    at.elapsed() < Duration::from_secs(if v.is_some() { 43200 } else { 600 })
                });
            if let Some((value, _)) = cached {
                src = value;
            } else {
                if let (Some(token), Some(client)) = (app_token().await, http()) {
                    if let Ok(response) = client.get(&url).bearer_auth(token).send().await {
                        if response.status().is_success() {
                            if let Ok(media) = response.json::<Value>().await {
                                src = forever_media_avatar(&media, bnet_id);
                            }
                        }
                    }
                }
                if let Ok(mut cache) = avatar_cache().lock() {
                    cache.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(43200));
                    if cache.len() < 1024 {
                        cache.insert(key, (src.clone(), Instant::now()));
                    }
                }
            }
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok((headers, Json(json!({"avatar_src": src}))))
}

// ------------------------------------------------------------------ access

/// The character if `viewer` may see it on `handle`'s profile: same rules as the profile's
/// WoW cards, including Show on profile. Retail needs verification; Forever needs
/// its separate public-lookup gate (which is not ownership proof).
pub(crate) async fn visible_char(
    st: &AppState,
    viewer: Option<Uuid>,
    handle: &str,
    id: Uuid,
) -> ApiResult<(Uuid, bool, String)> {
    let nf = || {
        AppError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            "That character isn't shown on this profile.",
        )
    };
    if !crate::features::on(st, "wow_characters").await {
        return Err(nf());
    }
    let p = crate::friends::find_visible(st, viewer, handle).await?;
    let is_self = viewer == Some(p.id);
    let friend = match viewer {
        Some(v) if !is_self => crate::friends::are_friends(st, v, p.id).await?,
        _ => false,
    };
    if !is_self && !friend && !crate::features::on(st, "public_profiles").await {
        return Err(nf());
    }
    if let (Some(me), false) = (viewer, is_self) {
        let blocked: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM blocks WHERE (blocker = $1 AND blocked = $2) OR (blocker = $2 AND blocked = $1))",
        )
        .bind(me)
        .bind(p.id)
        .fetch_one(&st.db)
        .await?;
        if blocked {
            return Err(nf());
        }
    }
    let (public, delete_after, privacy): (bool, Option<DateTime<Utc>>, Value) =
        sqlx::query_as("SELECT public_profile, delete_after, privacy FROM users WHERE id = $1")
            .bind(p.id)
            .fetch_one(&st.db)
            .await?;
    if delete_after.is_some() && !is_self {
        return Err(nf());
    }
    if !(is_self || friend || public) {
        return Err(nf());
    }
    let show = |k: &str| is_self || crate::account::shows(&privacy, k);
    if !show("games") || !show("wow_characters") {
        return Err(nf());
    }
    let row: Option<(bool, String)> = sqlx::query_as(
        "SELECT show_guild, game_id FROM wow_characters WHERE id = $1 AND user_id = $2
           AND ((game_id = 'wow' AND verified) OR (game_id = 'wow_forever' AND $3)) AND NOT hidden
           AND NOT coalesce((SELECT hide_all FROM wow_prefs WHERE user_id = $2), false)
           AND coalesce((SELECT profile_show FROM wow_prefs WHERE user_id = $2), 'both') IN ('both', game_id)",
    )
    .bind(id).bind(p.id).bind(crate::wow::forever_lookup_enabled())
    .fetch_optional(&st.db).await?;
    let Some((guild, game)) = row else {
        return Err(nf());
    };
    if !is_self
        && crate::account::hidden_games(st, p.id)
            .await?
            .contains(&game)
    {
        return Err(nf());
    }
    Ok((p.id, guild, game))
}

async fn detail(
    State(st): State<AppState>,
    m: MaybeUser,
    headers: HeaderMap,
    Path((handle, id)): Path<(String, Uuid)>,
) -> ApiResult<Json<Value>> {
    let viewer = m.0.as_ref().map(|a| a.user.id);
    let key = viewer
        .map(|v| v.to_string())
        .unwrap_or_else(|| crate::client_ip(&headers));
    st.rate_limit(format!("wowchar:{key}"), 120, Duration::from_secs(600))?;
    let (_, show_guild, _) = visible_char(&st, viewer, &handle, id).await?;
    let mut cached = load(&st, id).await?;
    let fresh = |c: &Option<(String, Value, DateTime<Utc>)>| {
        c.as_ref()
            .is_some_and(|c| Utc::now() - c.2 < chrono::Duration::hours(TTL_H))
    };
    if cached.is_none() {
        // first open: fetch now (others opening it at the same time wait on the same lock)
        let _ = refresh(&st, id).await;
        cached = load(&st, id).await?;
    } else if !fresh(&cached) {
        let st2 = st.clone();
        tokio::spawn(async move {
            let _ = refresh(&st2, id).await;
        });
    }
    let Some((status, mut data, at)) = cached else {
        if let Some(data) = fallback_details(&st, id).await? {
            return Ok(Json(data));
        }
        return Err(AppError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "bnet_unavailable",
            "Couldn't reach Blizzard right now. Try again in a minute.",
        ));
    };
    if status != "ok" {
        return Ok(Json(json!({ "status": "missing", "updated_at": at,
            "message": "Profile private or not found on Blizzard." })));
    }
    if !show_guild {
        data["guild"] = Value::Null;
    }
    data["status"] = json!("ok");
    data["updated_at"] = json!(at);
    Ok(Json(data))
}

async fn load(st: &AppState, id: Uuid) -> ApiResult<Option<(String, Value, DateTime<Utc>)>> {
    Ok(
        sqlx::query_as("SELECT status, data, fetched_at FROM wow_char_details WHERE char_id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?,
    )
}

/// The character's render, cached on disk next to the summary (replaced on refresh).
async fn render(
    State(st): State<AppState>,
    m: MaybeUser,
    Path((handle, id)): Path<(String, Uuid)>,
) -> Response<Body> {
    let nf = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::empty())
            .unwrap()
    };
    let viewer = m.0.as_ref().map(|a| a.user.id);
    if visible_char(&st, viewer, &handle, id).await.is_err() {
        return nf();
    }
    let Ok(Some((_, data, _))) = load(&st, id).await else {
        return nf();
    };
    let Some(src) = data["render_src"].as_str().filter(|s| render_url_ok(s)) else {
        return nf();
    };
    let dir = st.cfg.data_dir.join("wow-renders");
    let path = dir.join(format!("{id}.png"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => Some(b),
        Err(_) => {
            let got = match http() {
                Some(h) => match h.get(src).send().await {
                    Ok(r) if r.status().is_success() => r.bytes().await.ok().map(|b| b.to_vec()),
                    _ => None,
                },
                None => None,
            }
            .filter(|b| b.starts_with(b"\x89PNG"));
            if let Some(b) = &got {
                let _ = tokio::fs::create_dir_all(&dir).await;
                let _ = tokio::fs::write(&path, b).await;
            }
            got
        }
    };
    match bytes {
        Some(b) => Response::builder()
            .header(header::CONTENT_TYPE, "image/png")
            .header(header::CACHE_CONTROL, "private, no-store")
            .body(Body::from(b))
            .unwrap(),
        None => nf(),
    }
}

fn render_url_ok(s: &str) -> bool {
    s.starts_with("https://render.worldofwarcraft.com/") && s.ends_with(".png") && !s.contains("..")
}

#[derive(Deserialize)]
struct GuildReq {
    show_guild: bool,
}

async fn set_guild(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
    Json(r): Json<GuildReq>,
) -> ApiResult<Json<Value>> {
    crate::api::check_origin(&a, &headers, &st)?;
    let n = sqlx::query(
        "UPDATE wow_characters SET show_guild = $3 WHERE id = $1 AND user_id = $2 AND (verified OR game_id='wow_forever')",
    )
    .bind(id)
    .bind(a.user.id)
    .bind(r.show_guild)
    .execute(&st.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(AppError::not_found());
    }
    // no guild known yet: fetch the profile so the card can show it
    if r.show_guild && load(&st, id).await?.is_none() {
        let st2 = st.clone();
        tokio::spawn(async move {
            let _ = refresh(&st2, id).await;
        });
    }
    Ok(Json(json!({ "ok": true, "show_guild": r.show_guild })))
}

/// Profile cards: kick off a fetch for shown-guild characters with no summary yet.
pub fn warm_guilds(st: &AppState, ids: Vec<Uuid>) {
    if ids.is_empty() {
        return;
    }
    let st = st.clone();
    tokio::spawn(async move {
        for id in ids.into_iter().take(10) {
            let _ = refresh(&st, id).await;
        }
    });
}

// ------------------------------------------------------------------ fetch

enum Fail {
    Missing,
    Limited,
    Other,
}

/// After a 429, no Blizzard profile fetches for a minute.
fn backoff() -> &'static Mutex<Option<Instant>> {
    static B: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(None))
}

fn limited_now() -> bool {
    backoff()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|t| t.elapsed() < Duration::from_secs(60))
}

async fn get_json(h: &reqwest::Client, tok: &str, url: &str) -> Result<Value, Fail> {
    let r = h
        .get(url)
        .bearer_auth(tok)
        .send()
        .await
        .map_err(|_| Fail::Other)?;
    match r.status().as_u16() {
        200 => r.json().await.map_err(|_| Fail::Other),
        404 | 403 => Err(Fail::Missing),
        429 => {
            *backoff().lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
            Err(Fail::Limited)
        }
        _ => Err(Fail::Other),
    }
}

fn lock_for(id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
    static L: OnceLock<Mutex<HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut m = L
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    m.retain(|_, v| Arc::strong_count(v) > 1);
    m.entry(id).or_default().clone()
}

/// Static data that changes per patch/season, cached in memory for a day.
async fn cached_static(h: &reqwest::Client, tok: &str, key: &str, url: String) -> Option<Value> {
    static C: OnceLock<Mutex<HashMap<String, (Value, Instant)>>> = OnceLock::new();
    let c = C.get_or_init(Default::default);
    if let Some((v, at)) = c.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
        if at.elapsed() < Duration::from_secs(86400) {
            return Some(v.clone());
        }
    }
    let v = get_json(h, tok, &url).await.ok()?;
    c.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.to_string(), (v.clone(), Instant::now()));
    Some(v)
}

/// Fetch and store the character's summary. One refresh per character at a time; a
/// refresh that finds a fresh row (done by whoever held the lock) does nothing.
#[allow(clippy::result_unit_err)]
pub async fn refresh(st: &AppState, id: Uuid) -> Result<(), ()> {
    let l = lock_for(id);
    let _g = l.lock().await;
    if let Ok(Some((_, _, at))) = load(st, id).await {
        if Utc::now() - at < chrono::Duration::minutes(5) {
            return Ok(());
        }
    }
    if limited_now() {
        return Err(());
    }
    let row: Option<(String, String, String, String, bool)> = sqlx::query_as(
        "SELECT game_id, region, realm_slug, name, show_guild FROM wow_characters WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await
    .map_err(|_| ())?;
    let Some((game, region, slug, name, show_guild)) = row else {
        return Err(());
    };
    if game == "wow_forever" {
        return refresh_forever(st, id, &region, &name).await;
    }
    if game != "wow" || !matches!(region.as_str(), "us" | "eu" | "kr" | "tw") {
        return Err(());
    }
    let (Some(tok), Some(h)) = (app_token().await, http()) else {
        return Err(());
    };
    let lname = name.to_lowercase();
    let mut base = reqwest::Url::parse(&format!(
        "https://{region}.api.blizzard.com/profile/wow/character/"
    ))
    .map_err(|_| ())?;
    base.path_segments_mut()
        .map_err(|_| ())?
        .pop_if_empty()
        .push(&slug)
        .push(&lname);
    let base = base.to_string();
    let ns = format!("namespace=profile-{region}&locale=en_US");
    let root = match get_json(&h, &tok, &format!("{base}?{ns}")).await {
        Ok(v) => v,
        Err(Fail::Missing) => {
            let _ = sqlx::query(
                "INSERT INTO wow_char_details(char_id, status, data, fetched_at) VALUES ($1, 'missing', '{}', now())
                 ON CONFLICT (char_id) DO UPDATE SET status = 'missing', data = '{}', fetched_at = now()",
            )
            .bind(id)
            .execute(&st.db)
            .await
            .inspect_err(|e| tracing::warn!("wowchar: mark missing {id}: {e}"));
            return Ok(());
        }
        Err(_) => return Err(()),
    };
    // the armory's level onto the card (`wow::shown_level!`). `bnet_checked_at` stays with
    // the upload's cheat check so a refresh never pushes that check back.
    if let Some(lvl) = root["level"].as_i64().filter(|l| *l > 0) {
        let last = root["last_login_timestamp"]
            .as_i64()
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single());
        let _ = sqlx::query(
            "UPDATE wow_characters SET bnet_level = $2,
                    bnet_last_login = CASE WHEN $3::timestamptz IS NULL THEN bnet_last_login
                                           ELSE greatest(bnet_last_login, $3) END
             WHERE id = $1",
        )
        .bind(id)
        .bind(lvl as i32)
        .bind(last)
        .execute(&st.db)
        .await
        .inspect_err(|e| tracing::warn!("wowchar: level for {id}: {e}"));
    }
    let _ = sqlx::query("UPDATE wow_characters SET faction = coalesce($2, faction) WHERE id = $1")
        .bind(id)
        .bind(crate::wow::profile_faction(&root))
        .execute(&st.db)
        .await
        .inspect_err(|e| tracing::warn!("wowchar: faction for {id}: {e}"));
    let sub = |p: &str| format!("{base}/{p}?{ns}");
    let u: Vec<String> = [
        "character-media",
        "equipment",
        "mythic-keystone-profile",
        "pvp-summary",
        "encounters/raids",
        "collections/mounts",
        "collections/pets",
        "professions",
    ]
    .iter()
    .map(|p| sub(p))
    .collect();
    let (media, equip, mplus, pvp, raids, mounts, pets, profs) = tokio::join!(
        get_json(&h, &tok, &u[0]),
        get_json(&h, &tok, &u[1]),
        get_json(&h, &tok, &u[2]),
        get_json(&h, &tok, &u[3]),
        get_json(&h, &tok, &u[4]),
        get_json(&h, &tok, &u[5]),
        get_json(&h, &tok, &u[6]),
        get_json(&h, &tok, &u[7]),
    );
    let ok = |r: Result<Value, Fail>| r.unwrap_or(Value::Null);
    let (media, equip, mplus, pvp, raids) = (ok(media), ok(equip), ok(mplus), ok(pvp), ok(raids));
    let (mounts, pets, profs) = (ok(mounts), ok(pets), ok(profs));

    // current M+ season and its best runs
    let season_ix = cached_static(
        &h,
        &tok,
        &format!("mseason-{region}"),
        format!("https://{region}.api.blizzard.com/data/wow/mythic-keystone/season/index?namespace=dynamic-{region}&locale=en_US"),
    )
    .await;
    let cur_season = season_ix
        .as_ref()
        .and_then(|v| v["current_season"]["id"].as_i64());
    let season = match cur_season {
        Some(s)
            if mplus["seasons"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x["id"].as_i64() == Some(s))) =>
        {
            get_json(
                &h,
                &tok,
                &sub(&format!("mythic-keystone-profile/season/{s}")),
            )
            .await
            .unwrap_or(Value::Null)
        }
        _ => Value::Null,
    };
    // rated PvP brackets the character has
    let mut brackets = Vec::new();
    if let Some(bs) = pvp["brackets"].as_array() {
        for b in bs.iter().take(6) {
            if let Some(u) = b["href"].as_str() {
                if u.starts_with(&format!("https://{region}.api.blizzard.com/")) {
                    if let Ok(v) = get_json(&h, &tok, u).await {
                        brackets.push(v);
                    }
                }
            }
        }
    }
    // newest raid expansion in the journal: the current tier
    let exp_ix = cached_static(
        &h,
        &tok,
        &format!("jexp-{region}"),
        format!("https://{region}.api.blizzard.com/data/wow/journal-expansion/index?namespace=static-{region}&locale=en_US"),
    )
    .await;
    let current_exp = exp_ix.as_ref().and_then(current_expansion);
    // guild rank from the roster, only when the owner shows the guild
    let mut rank = None;
    if show_guild {
        let pre = format!("https://{region}.api.blizzard.com/data/wow/guild/");
        if let Some(href) = root["guild"]["key"]["href"]
            .as_str()
            .filter(|u| u.starts_with(&pre) && !u.contains(".."))
        {
            let u = format!("{}/roster?{ns}", href.split('?').next().unwrap_or(href));
            if let Ok(v) = get_json(&h, &tok, &u).await {
                rank = v["members"].as_array().and_then(|ms| {
                    ms.iter()
                        .find(|m| m["character"]["id"] == root["id"])
                        .and_then(|m| m["rank"].as_i64())
                });
            }
        }
    }
    // gear icons: item media once per item, kept forever
    let mut icons: HashMap<i64, String> = HashMap::new();
    if let Some(items) = equip["equipped_items"].as_array() {
        let ids: Vec<i64> = items
            .iter()
            .filter_map(|i| i["media"]["id"].as_i64())
            .collect();
        let known: Vec<(i64, String)> =
            sqlx::query_as("SELECT item_id, icon FROM wow_item_icons WHERE item_id = ANY($1)")
                .bind(&ids)
                .fetch_all(&st.db)
                .await
                .unwrap_or_default();
        icons.extend(known);
        for mid in ids {
            if icons.contains_key(&mid) {
                continue;
            }
            let u = format!("https://{region}.api.blizzard.com/data/wow/media/item/{mid}?namespace=static-{region}&locale=en_US");
            if let Ok(v) = get_json(&h, &tok, &u).await {
                if let Some(ic) = icon_of(&v) {
                    let _ = sqlx::query("INSERT INTO wow_item_icons(item_id, icon) VALUES ($1, $2) ON CONFLICT (item_id) DO NOTHING")
                        .bind(mid)
                        .bind(&ic)
                        .execute(&st.db)
                        .await
                        .inspect_err(|e| tracing::warn!("wowchar: item icon {mid}: {e}"));
                    icons.insert(mid, ic);
                }
            }
        }
    }
    let data = summarise(&Parts {
        region: &region,
        root: &root,
        media: &media,
        equip: &equip,
        mplus: &mplus,
        season: &season,
        pvp: &pvp,
        brackets: &brackets,
        raids: &raids,
        current_exp,
        mounts: &mounts,
        pets: &pets,
        profs: &profs,
        icons: &icons,
        rank,
    });
    let _ = tokio::fs::remove_file(
        st.cfg
            .data_dir
            .join("wow-renders")
            .join(format!("{id}.png")),
    )
    .await;
    sqlx::query(
        "INSERT INTO wow_char_details(char_id, status, data, fetched_at) VALUES ($1, 'ok', $2, now())
         ON CONFLICT (char_id) DO UPDATE SET status = 'ok', data = $2, fetched_at = now()",
    )
    .bind(id)
    .bind(&data)
    .execute(&st.db)
    .await
    .map_err(|_| ())?;
    Ok(())
}

/// Local fields only, used when Blizzard has no public profile (Forever under 10 is a 404).
async fn fallback_details(st: &AppState, id: Uuid) -> ApiResult<Option<Value>> {
    let row: Option<(
        String,
        String,
        String,
        String,
        i32,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT c.game_id, c.name, c.region, c.realm, c.level, c.class, c.faction,
                    (SELECT wr.name FROM wow_forever_rulesets wr WHERE wr.id = c.ruleset_id)
             FROM wow_characters c WHERE c.id = $1",
    )
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let Some((game, name, region, realm, level, class, faction, ruleset)) = row else {
        return Ok(None);
    };
    let forever = game == "wow_forever";
    let display = if forever {
        ruleset
    } else if realm.is_empty() {
        None
    } else {
        Some(realm)
    };
    Ok(Some(json!({
        "status": "ok",
        "game_id": game,
        "name": name,
        "region": region,
        "realm": display,
        "level": level,
        "class": class,
        "faction": faction,
        "gear": [],
        "stats": [],
        "armory": Value::Null,
        "armory_unavailable": forever,
        "updated_at": Utc::now(),
    })))
}

async fn store_minimal_forever(st: &AppState, id: Uuid, name: &str) -> Result<(), ()> {
    let Some(data) = fallback_details(st, id).await.map_err(|_| ())? else {
        return Err(());
    };
    let mut data = data;
    data["name"] = json!(name);
    sqlx::query(
        "INSERT INTO wow_char_details(char_id,status,data,fetched_at) VALUES($1,'ok',$2,now())
         ON CONFLICT(char_id) DO UPDATE SET status='ok',data=$2,fetched_at=now()",
    )
    .bind(id)
    .bind(data)
    .execute(&st.db)
    .await
    .map_err(|_| ())?;
    Ok(())
}

/// Forever uses only probed realmless resources and a separate item-media namespace.
/// No ownership, character identity, /played, ledger or quarantine writes here.
async fn refresh_forever(st: &AppState, id: Uuid, region: &str, name: &str) -> Result<(), ()> {
    if !crate::wow::forever_lookup_enabled() || region != "us" {
        return Err(());
    }
    let (first, surname) = name.trim().split_once(' ').ok_or(())?;
    if surname.trim().is_empty() {
        return Err(());
    }
    let (Some(tok), Some(h)) = (app_token().await, http()) else {
        return Err(());
    };
    let url = crate::wow::forever_profile_url("us", first, surname);
    let base = url.split('?').next().ok_or(())?;
    let root = match get_json(&h, &tok, &url).await {
        Ok(v) => v,
        Err(_) => {
            // Keep a previous full summary. Under-10 and other public misses get a local fallback.
            if let Ok(Some((status, data, _))) = load(st, id).await {
                if status == "ok"
                    && data.get("armory_unavailable").and_then(|v| v.as_bool()) != Some(true)
                {
                    return Ok(());
                }
            }
            let _ = store_minimal_forever(st, id, name).await;
            return Ok(());
        }
    };
    if let Some(rid) = root["ruleset"]["id"].as_i64() {
        let _ = crate::wow::assign_forever_ruleset(&st.db, id, "us", rid).await;
    }
    if root["id"].as_i64().is_none()
        || root["name"].as_str().is_none()
        || root["surname"].as_str().is_none()
        || !format!(
            "{} {}",
            root["name"].as_str().unwrap_or(""),
            root["surname"].as_str().unwrap_or("")
        )
        .eq_ignore_ascii_case(name)
    {
        return Err(());
    }
    let bid: Option<i64> = sqlx::query_scalar("SELECT bnet_id FROM wow_characters WHERE id=$1")
        .bind(id)
        .fetch_one(&st.db)
        .await
        .map_err(|_| ())?;
    if bid.is_some() && bid != root["id"].as_i64() {
        return Err(());
    }
    let sub = |part: &str| format!("{base}/{part}?namespace=profile-forever-us&locale=en_US");
    let urls = [
        "equipment",
        "statistics",
        "character-media",
        "specializations",
        "professions",
        "pvp-summary",
    ]
    .map(sub);
    let (equip, stats, media, specs, profs, pvp) = tokio::join!(
        get_json(&h, &tok, &urls[0]),
        get_json(&h, &tok, &urls[1]),
        get_json(&h, &tok, &urls[2]),
        get_json(&h, &tok, &urls[3]),
        get_json(&h, &tok, &urls[4]),
        get_json(&h, &tok, &urls[5])
    );
    let equip = equip.unwrap_or(Value::Null);
    let mut icons = HashMap::new();
    for item in equip["equipped_items"]
        .as_array()
        .into_iter()
        .flatten()
        .take(32)
    {
        if let Some(mid) = item["media"]["id"].as_i64() {
            let media = cached_static(&h,&tok,&format!("forever-item-{mid}"),format!("https://us.api.blizzard.com/data/wow/media/item/{mid}?namespace=static-forever-us&locale=en_US")).await;
            if let Some(icon) = media.as_ref().and_then(icon_of) {
                icons.insert(mid, icon);
            }
        }
    }
    let data = summarise_forever(
        &root,
        &equip,
        &stats.unwrap_or(Value::Null),
        &media.unwrap_or(Value::Null),
        &specs.unwrap_or(Value::Null),
        &profs.unwrap_or(Value::Null),
        &pvp.unwrap_or(Value::Null),
        &icons,
    );
    sqlx::query(
        "INSERT INTO wow_char_details(char_id,status,data,fetched_at) VALUES($1,'ok',$2,now())
        ON CONFLICT(char_id) DO UPDATE SET status='ok',data=$2,fetched_at=now()",
    )
    .bind(id)
    .bind(data)
    .execute(&st.db)
    .await
    .map_err(|_| ())?;
    // Only a disposable image cache, not a character/hour row.
    let _ = tokio::fs::remove_file(
        st.cfg
            .data_dir
            .join("wow-renders")
            .join(format!("{id}.png")),
    )
    .await;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn summarise_forever(
    root: &Value,
    equip: &Value,
    stats: &Value,
    media: &Value,
    specs: &Value,
    profs: &Value,
    pvp: &Value,
    icons: &HashMap<i64, String>,
) -> Value {
    let n = Value::Null;
    let mut out = summarise(&Parts {
        region: "us",
        root,
        media,
        equip,
        mplus: &n,
        season: &n,
        pvp,
        brackets: &[],
        raids: &n,
        current_exp: None,
        mounts: &n,
        pets: &n,
        profs,
        icons,
        rank: None,
    });
    out["game_id"] = json!("wow_forever");
    out["name"] = json!(format!(
        "{} {}",
        root["name"].as_str().unwrap_or(""),
        root["surname"].as_str().unwrap_or("")
    )
    .trim());
    if let Some(ruleset) = name_of(&root["ruleset"]) {
        out["realm"] = json!(ruleset);
    } else {
        out["realm"] = Value::Null;
    }
    let level = root["level"].as_i64().unwrap_or(0);
    if level < 10 {
        out["armory"] = Value::Null;
        out["armory_unavailable"] = json!(true);
    } else {
        out["armory"] = json!(format!(
            "https://worldofwarcraft.blizzard.com/en-us/forever/us/armory/character/{}",
            crate::wow::forever_slug(
                root["name"].as_str().unwrap_or(""),
                root["surname"].as_str().unwrap_or("")
            )
        ));
    }
    out["avatar_src"] = media["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["key"] == "avatar"))
        .and_then(|x| x["value"].as_str())
        .filter(|u| {
            u.starts_with("https://render.worldofwarcraft.com/")
                && u.ends_with(".jpg")
                && !u.contains("..")
        })
        .map_or(Value::Null, |u| json!(u));
    let active = specs["specialization_groups"]
        .as_array()
        .and_then(|a| a.iter().find(|g| g["is_active"] == true));
    out["spec"] = active
        .and_then(|g| g["specializations"].as_array())
        .and_then(|a| {
            a.iter()
                .max_by_key(|x| x["spent_points"].as_i64().unwrap_or(0))
        })
        .and_then(|x| name_of(&x["specialization"]))
        .map_or(Value::Null, |x| json!(x));
    let mut mapped = Vec::new();
    for (key, label, percent) in [
        ("health", "Health", false),
        ("power", "Power", false),
        ("strength", "Strength", false),
        ("agility", "Agility", false),
        ("stamina", "Stamina", false),
        ("intellect", "Intellect", false),
        ("spirit", "Spirit", false),
        ("armor", "Armor", false),
        ("defense", "Defense", false),
        ("attack_power", "Attack power", false),
        ("melee_crit", "Melee crit", true),
        ("spell_crit", "Spell crit", true),
        ("dodge", "Dodge", true),
        ("parry", "Parry", true),
        ("block", "Block", true),
    ] {
        let raw = &stats[key];
        let value = raw
            .as_f64()
            .or_else(|| raw["effective"].as_f64())
            .or_else(|| raw["value"].as_f64());
        if let Some(v) = value.filter(|v| v.is_finite()) {
            let label = if key == "power" {
                name_of(&stats["power_type"]).unwrap_or_else(|| label.into())
            } else {
                label.into()
            };
            mapped.push(json!({"name":label,"value":v,"percent":percent}));
        }
    }
    out["stats"] = json!(mapped);
    out
}

/// Public-only share metadata. No refresh/network call; hidden/private failures are
/// indistinguishable from an unknown id, even for an authenticated owner.
pub(crate) async fn share_metadata(st: &AppState, handle: &str, id: Uuid) -> ApiResult<Value> {
    let (_, guild, game) = visible_char(st, None, handle, id).await?;
    let row: (String, i32, String) = sqlx::query_as(concat!(
        "SELECT name, ",
        crate::wow::shown_level!(),
        " AS level, class FROM wow_characters WHERE id=$1"
    ))
    .bind(id)
    .fetch_one(&st.db)
    .await?;
    let data = load(st, id)
        .await?
        .map(|(_, d, _)| d)
        .unwrap_or(Value::Null);
    Ok(
        json!({"id":id,"name":row.0,"level":row.1,"class":row.2,"game_id":game,
        "guild":if guild {data["guild"]["name"].clone()} else {Value::Null},
        "render":data["render"]==true,"details_enabled":true}),
    )
}
async fn share(
    State(st): State<AppState>,
    Path((handle, id)): Path<(String, Uuid)>,
) -> ApiResult<Json<Value>> {
    share_metadata(&st, &handle, id).await.map(Json)
}

/// Highest expansion id in the journal index, skipping the "Current Season" pseudo-tier.
pub fn current_expansion(ix: &Value) -> Option<i64> {
    ix["tiers"]
        .as_array()?
        .iter()
        .filter(|t| {
            !t["name"]
                .as_str()
                .unwrap_or("")
                .to_lowercase()
                .contains("season")
        })
        .filter_map(|t| t["id"].as_i64())
        .max()
}

/// Icon name from an item media document (`.../icons/56/<name>.jpg`).
pub fn icon_of(media: &Value) -> Option<String> {
    let a = media["assets"].as_array()?;
    let u = a.iter().find(|x| x["key"] == "icon")?["value"].as_str()?;
    let f = u.rsplit('/').next()?.strip_suffix(".jpg")?.to_lowercase();
    crate::wowpage::icon_name_ok(&f).then_some(f)
}

pub struct Parts<'a> {
    pub region: &'a str,
    pub root: &'a Value,
    pub media: &'a Value,
    pub equip: &'a Value,
    pub mplus: &'a Value,
    pub season: &'a Value,
    pub pvp: &'a Value,
    pub brackets: &'a [Value],
    pub raids: &'a Value,
    pub current_exp: Option<i64>,
    pub mounts: &'a Value,
    pub pets: &'a Value,
    pub profs: &'a Value,
    pub icons: &'a HashMap<i64, String>,
    pub rank: Option<i64>,
}

fn s(v: &Value) -> Value {
    v.as_str()
        .map(|x| json!(x.chars().take(80).collect::<String>()))
        .unwrap_or(Value::Null)
}

/// The stored popup document: only what the popup shows.
pub fn summarise(p: &Parts) -> Value {
    let r = p.root;
    let realm_slug = r["realm"]["slug"].as_str().unwrap_or("").to_lowercase();
    let name = r["name"].as_str().unwrap_or("");
    let armory = format!(
        "https://worldofwarcraft.blizzard.com/en-us/worldsoul/{}/armory/character/{}/{}",
        p.region,
        realm_slug,
        name.to_lowercase()
    );
    let render_src = p.media["assets"].as_array().and_then(|a| {
        ["main-raw", "main"].iter().find_map(|k| {
            a.iter()
                .find(|x| x["key"] == *k)
                .and_then(|x| x["value"].as_str())
                .map(String::from)
        })
    });
    let ids = |v: &Value, f: fn(&Value) -> Option<i64>| -> Vec<i64> {
        v.as_array()
            .map(|a| a.iter().filter_map(f).collect())
            .unwrap_or_default()
    };
    let gear: Vec<Value> = p.equip["equipped_items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|i| {
                    json!({
                        "slot": s(&i["slot"]["type"]), "slot_name": s(&i["slot"]["name"]),
                        "name": s(&i["name"]), "quality": s(&i["quality"]["type"]),
                        "ilvl": i["level"]["value"].as_i64(),
                        "icon": i["media"]["id"].as_i64().and_then(|m| p.icons.get(&m).cloned()),
                        "id": i["item"]["id"].as_i64(),
                        "bonus": ids(&i["bonus_list"], |x| x.as_i64()),
                        "ench": i["enchantments"].as_array().and_then(|a| a.iter().find(|e| e["enchantment_slot"]["id"].as_i64().unwrap_or(0) == 0).or(a.first())).and_then(|e| e["enchantment_id"].as_i64()),
                        "gems": ids(&i["sockets"], |x| x["item"]["id"].as_i64()),
                        "pcs": ids(&i["set"]["items"], |x| if x["is_equipped"].as_bool() == Some(true) { x["item"]["id"].as_i64() } else { None }),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let runs: Vec<Value> = p.season["best_runs"]
        .as_array()
        .map(|a| {
            let mut v: Vec<Value> = a
                .iter()
                .map(|b| {
                    json!({ "dungeon": s(&b["dungeon"]["name"]), "level": b["keystone_level"].as_i64(),
                        "timed": b["is_completed_within_time"].as_bool().unwrap_or(false),
                        "rating": b["mythic_rating"]["rating"].as_f64().map(|x| x.round() as i64) })
                })
                .collect();
            v.sort_by_key(|x| -x["level"].as_i64().unwrap_or(0));
            v.truncate(8);
            v
        })
        .unwrap_or_default();
    let mrating = p.season["mythic_rating"]["rating"]
        .as_f64()
        .or_else(|| p.mplus["current_mythic_rating"]["rating"].as_f64())
        .map(|x| x.round() as i64)
        .filter(|x| *x > 0);
    let rated: Vec<Value> = p
        .brackets
        .iter()
        .filter_map(|b| {
            let t = b["bracket"]["type"].as_str()?;
            let label = match t {
                "ARENA_2v2" => "2v2".to_string(),
                "ARENA_3v3" => "3v3".to_string(),
                "BATTLEGROUNDS" => "Rated BG".to_string(),
                x if x.starts_with("SHUFFLE") => "Solo Shuffle".to_string(),
                x if x.starts_with("BLITZ") => "Blitz".to_string(),
                x => x.replace('_', " "),
            };
            Some(json!({ "bracket": label, "rating": b["rating"].as_i64() }))
        })
        .collect();
    // raids: the current tier, or the most recent tier with any kills
    let exps = p.raids["expansions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let tier = |e: &Value| -> Value {
        let inst: Vec<Value> = e["instances"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|i| {
                        let modes: Vec<Value> = i["modes"]
                            .as_array()
                            .map(|m| {
                                m.iter()
                                    .map(|d| json!({ "difficulty": s(&d["difficulty"]["type"]),
                                        "label": s(&d["difficulty"]["name"]),
                                        "done": d["progress"]["completed_count"].as_i64().unwrap_or(0),
                                        "total": d["progress"]["total_count"].as_i64().unwrap_or(0) }))
                                    .collect()
                            })
                            .unwrap_or_default();
                        json!({ "name": s(&i["instance"]["name"]), "id": i["instance"]["id"], "modes": modes })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mut inst = inst;
        inst.sort_by_key(|i| -i["id"].as_i64().unwrap_or(0));
        inst.truncate(4);
        json!({ "expansion": s(&e["expansion"]["name"]), "instances": inst })
    };
    let eid = |e: &Value| e["expansion"]["id"].as_i64().unwrap_or(0);
    let cur = p
        .current_exp
        .and_then(|c| exps.iter().find(|e| eid(e) == c))
        .map(tier);
    let recent = if cur.is_none() {
        exps.iter()
            .filter(|e| {
                !e["expansion"]["name"]
                    .as_str()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains("season")
            })
            .max_by_key(|e| eid(e))
            .map(tier)
    } else {
        None
    };
    let mut profs: Vec<Value> = Vec::new();
    for k in ["primaries", "secondaries"] {
        if let Some(a) = p.profs[k].as_array() {
            for pr in a {
                let pname = s(&pr["profession"]["name"]);
                let last = pr["tiers"].as_array().and_then(|t| t.last()).cloned();
                let (tname, sk, mx) = match &last {
                    Some(t) => (
                        s(&t["tier"]["name"]),
                        t["skill_points"].as_i64(),
                        t["max_skill_points"].as_i64(),
                    ),
                    None => (
                        Value::Null,
                        pr["skill_points"].as_i64(),
                        pr["max_skill_points"].as_i64(),
                    ),
                };
                profs.push(json!({ "name": pname, "tier": tname, "skill": sk, "max": mx, "primary": k == "primaries" }));
            }
        }
    }
    let guild = r["guild"]["name"]
        .as_str()
        .map(|g| json!({ "name": g, "rank": p.rank }))
        .unwrap_or(Value::Null);
    json!({
        "name": name, "region": p.region, "realm": name_of(&r["realm"]), "realm_slug": realm_slug,
        "level": r["level"].as_i64(), "race": name_of(&r["race"]), "class": name_of(&r["character_class"]),
        "class_id": r["character_class"]["id"].as_i64(),
        "spec": name_of(&r["active_spec"]), "faction": crate::wow::profile_faction(r).map(str::to_string).or_else(|| name_of(&r["faction"])),
        "title": r["active_title"]["display_string"].as_str().map(|t| t.replace("{name}", name)),
        "ilvl": r["equipped_item_level"].as_i64(), "achievement_points": r["achievement_points"].as_i64(),
        "last_login": r["last_login_timestamp"].as_i64(),
        "guild": guild, "render": render_src.is_some(), "render_src": render_src,
        "avatar_src": media_avatar(p.media, r["id"].as_i64()),
        "gear": gear,
        "mplus": { "rating": mrating, "runs": runs },
        "pvp": { "honor_level": p.pvp["honor_level"].as_i64(), "honorable_kills": p.pvp["honorable_kills"].as_i64(), "rated": rated },
        "raids": { "current": cur, "recent": recent },
        "collections": { "mounts": p.mounts["mounts"].as_array().map(|a| a.len()), "pets": p.pets["pets"].as_array().map(|a| a.len()) },
        "professions": profs,
        "armory": armory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_shapes() {
        let root = json!({ "id": 7, "name": "Lucrona", "level": 80, "realm": { "name": "Frostmourne", "slug": "frostmourne" },
            "race": { "name": "Dracthyr" }, "character_class": { "name": "Warrior", "id": 1 }, "active_spec": { "name": "Protection" },
            "faction": { "name": "Alliance" }, "active_title": { "display_string": "{name} the Kingslayer" },
            "equipped_item_level": 78, "achievement_points": 15440, "guild": { "name": "Seraphim" } });
        let media = json!({ "assets": [{ "key": "main-raw", "value": "https://render.worldofwarcraft.com/us/character/x/1-main-raw.png" }, { "key": "avatar", "value": "https://render.worldofwarcraft.com/us/character/x/1-avatar.jpg" }] });
        let equip = json!({ "equipped_items": [{ "slot": { "type": "HEAD", "name": "Head" }, "name": "Helm", "quality": { "type": "EPIC" },
            "level": { "value": 83 }, "media": { "id": 5 }, "item": { "id": 19019 }, "bonus_list": [6652, 1498],
            "enchantments": [{ "enchantment_id": 7001, "enchantment_slot": { "id": 0 } }], "sockets": [{ "item": { "id": 213743 } }, {}],
            "set": { "items": [{ "item": { "id": 1 }, "is_equipped": true }, { "item": { "id": 2 }, "is_equipped": false }] } }] });
        let raids = json!({ "expansions": [
            { "expansion": { "id": 68, "name": "Legion" }, "instances": [] },
            { "expansion": { "id": 124, "name": "Warlords of Draenor" }, "instances": [{ "instance": { "id": 477, "name": "Highmaul" },
              "modes": [{ "difficulty": { "type": "LFR", "name": "Raid Finder" }, "progress": { "completed_count": 5, "total_count": 7 } }] }] } ] });
        let profs = json!({ "primaries": [{ "profession": { "name": "Inscription" }, "tiers": [
            { "tier": { "name": "Classic Inscription" }, "skill_points": 1, "max_skill_points": 300 },
            { "tier": { "name": "Shadowlands Inscription" }, "skill_points": 1, "max_skill_points": 100 } ] }],
            "secondaries": [{ "profession": { "name": "Archaeology" }, "skill_points": 10, "max_skill_points": 950 }] });
        let icons: HashMap<i64, String> = [(5, "132963".to_string())].into();
        let n = Value::Null;
        let v = summarise(&Parts {
            region: "us",
            root: &root,
            media: &media,
            equip: &equip,
            mplus: &n,
            season: &n,
            pvp: &json!({ "honor_level": 4, "honorable_kills": 13007 }),
            brackets: &[],
            raids: &raids,
            current_exp: Some(514),
            mounts: &json!({ "mounts": [1, 2] }),
            pets: &n,
            profs: &profs,
            icons: &icons,
            rank: Some(2),
        });
        assert_eq!(v["title"], "Lucrona the Kingslayer");
        assert_eq!(v["armory"], "https://worldofwarcraft.blizzard.com/en-us/worldsoul/us/armory/character/frostmourne/lucrona");
        assert_eq!(
            v["avatar_src"],
            "https://render.worldofwarcraft.com/us/character/x/1-avatar.jpg"
        );
        assert_eq!(v["gear"][0]["icon"], "132963");
        assert_eq!(v["gear"][0]["ilvl"], 83);
        assert_eq!(v["gear"][0]["id"], 19019);
        assert_eq!(v["gear"][0]["bonus"], json!([6652, 1498]));
        assert_eq!(v["gear"][0]["ench"], 7001);
        assert_eq!(v["gear"][0]["gems"], json!([213743]));
        assert_eq!(v["gear"][0]["pcs"], json!([1]));
        assert!(v["raids"]["current"].is_null());
        assert_eq!(v["raids"]["recent"]["expansion"], "Warlords of Draenor");
        assert_eq!(v["professions"][0]["tier"], "Shadowlands Inscription");
        assert_eq!(v["professions"][1]["skill"], 10);
        assert_eq!(v["collections"]["mounts"], 2);
        assert!(v["collections"]["pets"].is_null());
        assert!(v["mplus"]["rating"].is_null());
        assert_eq!(v["guild"]["rank"], 2);
        assert_eq!(v["pvp"]["honorable_kills"], 13007);
        assert!(v.get("played_s").is_none());
    }

    #[test]
    fn icons_and_tiers() {
        let m = json!({ "assets": [{ "key": "icon", "value": "https://render.worldofwarcraft.com/us/icons/56/inv_helm_01.jpg" }] });
        assert_eq!(icon_of(&m).as_deref(), Some("inv_helm_01"));
        assert!(
            icon_of(&json!({ "assets": [{ "key": "icon", "value": "https://x/a%20b.jpg" }] }))
                .is_none()
        );
        let ix = json!({ "tiers": [{ "id": 503, "name": "Current Season" }, { "id": 514, "name": "Midnight" }, { "id": 124, "name": "WoD" }] });
        assert_eq!(current_expansion(&ix), Some(514));
        assert!(render_url_ok(
            "https://render.worldofwarcraft.com/us/character/a/1-main-raw.png"
        ));
        assert!(!render_url_ok("https://evil.example/x.png"));
    }
}
