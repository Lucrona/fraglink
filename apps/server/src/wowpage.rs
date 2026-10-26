//! The Battle.net page (`/connect/battlenet`): the account, its verified WoW characters
//! (searchable, paged for accounts with 1000+), a spec pick per character, per-character
//! "hide from profile" and "Hide all" (also covers characters verified later). Hidden
//! characters still count toward hours. Race/class/spec icons come from Blizzard's
//! render CDN through `/api/v1/wow/icons/<name>.jpg`, cached on disk.

use crate::api;
use crate::auth::AuthUser;
use crate::error::{ApiResult, AppError};
use crate::wow::{app_token, bnet_link, http, name_of, namespace};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, Response, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/me/bnet", get(summary))
        .route("/api/v1/me/wow/list", get(list))
        .route("/api/v1/me/wow/hide_all", post(hide_all))
        .route("/api/v1/me/wow/{id}/spec", post(set_spec))
        .route("/api/v1/wow/icons/{file}", get(icon))
}

async fn hide_all_on(st: &AppState, uid: Uuid) -> ApiResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT hide_all FROM wow_prefs WHERE user_id = $1")
            .bind(uid)
            .fetch_optional(&st.db)
            .await?
            .unwrap_or(false),
    )
}

async fn summary(State(st): State<AppState>, a: AuthUser) -> ApiResult<Json<Value>> {
    let uid = a.user.id;
    let link = bnet_link(&st, uid).await?;
    let row: Option<(DateTime<Utc>,)> = sqlx::query_as(
        "SELECT linked_at FROM linked_accounts WHERE user_id = $1 AND provider = 'bnet' AND verified",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?;
    let (checked, regions): (Option<DateTime<Utc>>, Vec<String>) = sqlx::query_as(
        "SELECT max(fetched_at), coalesce(array_agg(DISTINCT region ORDER BY region), '{}')
         FROM bnet_characters WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    let (verified, unverified): (i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE verified), count(*) FILTER (WHERE NOT verified)
         FROM wow_characters WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    Ok(Json(json!({
        "linked": link.is_some(),
        "needs_reconnect": matches!(link, Some((_, false))),
        "battletag": link.map(|l| l.0),
        "linked_at": row.map(|r| r.0),
        "last_verified": checked,
        "regions": regions,
        "verified": verified,
        "unverified": unverified,
        "hide_all": hide_all_on(&st, uid).await?,
    })))
}

#[derive(Deserialize)]
struct ListQ {
    q: Option<String>,
    sort: Option<String>,
    class: Option<String>,
    vis: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
}

/// One page of the user's verified characters, plus the filtered count and totals.
async fn list(
    State(st): State<AppState>,
    a: AuthUser,
    Query(p): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    let uid = a.user.id;
    if p.offset.unwrap_or(0) == 0 {
        backfill(&st, uid);
    }
    let ha = hide_all_on(&st, uid).await?;
    let q = p.q.unwrap_or_default().trim().to_lowercase();
    let q = q.chars().take(64).collect::<String>();
    let like = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let class = p.class.unwrap_or_default();
    let vis = match p.vis.as_deref() {
        Some("shown") => 1,
        Some("hidden") => 2,
        _ => 0,
    };
    let order = match p.sort.as_deref() {
        Some("level") => "level DESC, played_s DESC, lower(name)",
        Some("name") => "lower(name), lower(realm)",
        Some("realm") => "lower(realm), lower(name)",
        _ => "played_s DESC, lower(name)",
    };
    let limit = p.limit.unwrap_or(25).clamp(1, 100);
    let offset = p.offset.unwrap_or(0).clamp(0, 1_000_000);
    // `$4` hide_all: every character counts as hidden
    let filter = "user_id = $1 AND verified
        AND ($2 = '' OR lower(name) LIKE $3 OR lower(realm) LIKE $3)
        AND ($5 = '' OR upper(class) = upper($5))
        AND ($6 = 0 OR ($6 = 1 AND NOT (hidden OR $4)) OR ($6 = 2 AND (hidden OR $4)))";
    #[allow(clippy::type_complexity)]
    let rows: Vec<(Uuid, String, String, String, String, String, i32, i64, bool, Option<String>, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT id, game_id, region, realm, name, class, level, played_s, hidden, race, gender, spec, bnet_spec
             FROM wow_characters WHERE {filter} ORDER BY {order} LIMIT $7 OFFSET $8"
        )))
        .bind(uid)
        .bind(&q)
        .bind(&like)
        .bind(ha)
        .bind(&class)
        .bind(vis)
        .bind(limit)
        .bind(offset)
        .fetch_all(&st.db)
        .await?;
    let (matched,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM wow_characters WHERE {filter}"
    )))
    .bind(uid)
    .bind(&q)
    .bind(&like)
    .bind(ha)
    .bind(&class)
    .bind(vis)
    .fetch_one(&st.db)
    .await?;
    let (total, total_s, classes): (i64, i64, Vec<String>) = sqlx::query_as(
        "SELECT count(*), coalesce(sum(played_s), 0)::bigint,
                coalesce(array_agg(DISTINCT upper(class)) FILTER (WHERE class <> ''), '{}')
         FROM wow_characters WHERE user_id = $1 AND verified",
    )
    .bind(uid)
    .fetch_one(&st.db)
    .await?;
    let items: Vec<Value> = rows
        .into_iter()
        .map(
            |(id, g, r, realm, name, class, level, s, h, race, gender, spec, bspec)| {
                json!({ "id": id, "game_id": g, "region": r, "realm": realm, "name": name,
                    "class": class, "level": level, "played_min": s / 60,
                    "hidden": h || ha, "own_hidden": h, "race": race, "gender": gender,
                    "spec": spec.clone().or(bspec.clone()), "spec_picked": spec.is_some(),
                    "active_spec": bspec })
            },
        )
        .collect();
    Ok(Json(json!({
        "items": items, "matched": matched, "total": total,
        "total_min": total_s / 60, "classes": classes, "hide_all": ha,
        "offset": offset, "limit": limit,
    })))
}

#[derive(Deserialize)]
struct HideAllReq {
    hide_all: bool,
}

async fn hide_all(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Json(r): Json<HideAllReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    sqlx::query(
        "INSERT INTO wow_prefs(user_id, hide_all) VALUES($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET hide_all = excluded.hide_all",
    )
    .bind(a.user.id)
    .bind(r.hide_all)
    .execute(&st.db)
    .await?;
    Ok(Json(json!({ "ok": true, "hide_all": r.hide_all })))
}

#[derive(Deserialize)]
struct SpecReq {
    /// a spec name of the character's class; null goes back to Battle.net's active spec
    spec: Option<String>,
}

async fn set_spec(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
    Json(r): Json<SpecReq>,
) -> ApiResult<Json<Value>> {
    api::check_origin(&a, &headers, &st)?;
    let class: Option<String> =
        sqlx::query_scalar("SELECT class FROM wow_characters WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(a.user.id)
            .fetch_optional(&st.db)
            .await?;
    let Some(class) = class else {
        return Err(AppError::not_found());
    };
    if let Some(s) = &r.spec {
        if !specs_of(&class).contains(&s.as_str()) {
            return Err(AppError::bad_request(
                "invalid_spec",
                "That spec isn't one of this character's class.",
            ));
        }
    }
    sqlx::query("UPDATE wow_characters SET spec = $3 WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(a.user.id)
        .bind(&r.spec)
        .execute(&st.db)
        .await?;
    Ok(Json(json!({ "ok": true, "spec": r.spec })))
}

/// Spec names per class (the addon's class token, e.g. `DEATHKNIGHT`).
pub fn specs_of(class: &str) -> &'static [&'static str] {
    match class.to_uppercase().as_str() {
        "WARRIOR" => &["Arms", "Fury", "Protection"],
        "PALADIN" => &["Holy", "Protection", "Retribution"],
        "HUNTER" => &["Beast Mastery", "Marksmanship", "Survival"],
        "ROGUE" => &["Assassination", "Outlaw", "Subtlety"],
        "PRIEST" => &["Discipline", "Holy", "Shadow"],
        "DEATHKNIGHT" => &["Blood", "Frost", "Unholy"],
        "SHAMAN" => &["Elemental", "Enhancement", "Restoration"],
        "MAGE" => &["Arcane", "Fire", "Frost"],
        "WARLOCK" => &["Affliction", "Demonology", "Destruction"],
        "MONK" => &["Brewmaster", "Mistweaver", "Windwalker"],
        "DRUID" => &["Balance", "Feral", "Guardian", "Restoration"],
        "DEMONHUNTER" => &["Havoc", "Vengeance", "Devourer"],
        "EVOKER" => &["Devastation", "Preservation", "Augmentation"],
        _ => &[],
    }
}

/// After a Battle.net character fetch: copy race/gender onto the user's characters, then
/// (in the background) look up the active spec of verified ones that don't have it yet.
pub async fn after_sync(st: &AppState, uid: Uuid) {
    let _ = sqlx::query(
        "UPDATE wow_characters c SET race = coalesce(b.race, c.race), gender = coalesce(b.gender, c.gender)
         FROM bnet_characters b WHERE c.user_id = $1 AND b.user_id = c.user_id AND b.game_id = c.game_id
           AND b.region = c.region AND b.realm_slug = c.realm_slug AND lower(b.name) = lower(c.name)",
    )
    .bind(uid)
    .execute(&st.db)
    .await;
    let st = st.clone();
    tokio::spawn(async move { fill_specs(&st, uid).await });
}

/// Characters verified before race/gender/spec were stored (or whose fetch failed) get
/// them filled in the background when the page opens; at most once per user per 10 minutes.
fn backfill(st: &AppState, uid: Uuid) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static SEEN: OnceLock<Mutex<HashMap<Uuid, Instant>>> = OnceLock::new();
    {
        let mut m = SEEN
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        m.retain(|_, t| t.elapsed() < Duration::from_secs(600));
        if m.contains_key(&uid) {
            return;
        }
        m.insert(uid, Instant::now());
    }
    let st = st.clone();
    tokio::spawn(async move { fill_specs(&st, uid).await });
}

async fn fill_specs(st: &AppState, uid: Uuid) {
    let rows: Vec<(Uuid, String, String, String, String)> = sqlx::query_as(
        "SELECT id, game_id, region, realm_slug, name FROM wow_characters
         WHERE user_id = $1 AND verified AND (bnet_spec IS NULL OR race IS NULL)
         ORDER BY played_s DESC LIMIT 60",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await
    .unwrap_or_default();
    if rows.is_empty() {
        return;
    }
    let (Some(tok), Some(http)) = (app_token().await, http()) else {
        tracing::warn!("wow backfill: no Battle.net app token");
        return;
    };
    let mut ok = 0usize;
    let total = rows.len();
    for (id, g, region, slug, name) in rows {
        let Some(ns) = namespace(&g, &region) else {
            continue;
        };
        let url = format!(
            "https://{region}.api.blizzard.com/profile/wow/character/{slug}/{}?namespace={ns}&locale=en_US",
            name.to_lowercase()
        );
        let Ok(Ok(resp)) = http
            .get(&url)
            .bearer_auth(&tok)
            .send()
            .await
            .map(|r| r.error_for_status())
        else {
            continue;
        };
        let Ok(v) = resp.json::<Value>().await else {
            continue;
        };
        ok += 1;
        let _ = sqlx::query(
            "UPDATE wow_characters SET bnet_spec = coalesce($2, bnet_spec), race = coalesce($3, race),
                    gender = coalesce($4, gender) WHERE id = $1",
        )
        .bind(id)
        .bind(name_of(&v["active_spec"]))
        .bind(name_of(&v["race"]))
        .bind(v["gender"]["type"].as_str().map(|g| g.to_lowercase()))
        .execute(&st.db)
        .await;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    tracing::info!("wow backfill: {ok}/{total} characters for {uid}");
}

/// `classicon_*`, `achievement_character_*`, spec spell icons: only `[a-z0-9_]` names.
pub fn icon_name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 64
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

async fn icon(State(st): State<AppState>, Path(file): Path<String>) -> Response<Body> {
    let nf = || {
        Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CACHE_CONTROL, "public, max-age=3600")
            .body(Body::empty())
            .unwrap()
    };
    let Some(name) = file.strip_suffix(".jpg").filter(|n| icon_name_ok(n)) else {
        return nf();
    };
    let dir = st.cfg.data_dir.join("wow-icons");
    let path = dir.join(format!("{name}.jpg"));
    let miss = dir.join(format!("{name}.miss"));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => Some(b),
        Err(_) => {
            let fresh_miss = tokio::fs::metadata(&miss)
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|e| e < Duration::from_secs(86400));
            if fresh_miss {
                None
            } else {
                let got = match http() {
                    Some(h) => match h
                        .get(format!(
                            "https://render.worldofwarcraft.com/us/icons/56/{name}.jpg"
                        ))
                        .send()
                        .await
                    {
                        Ok(r) if r.status().is_success() => {
                            r.bytes().await.ok().map(|b| b.to_vec())
                        }
                        _ => None,
                    },
                    None => None,
                };
                let _ = tokio::fs::create_dir_all(&dir).await;
                match &got {
                    Some(b) if b.len() > 100 && b.starts_with(&[0xff, 0xd8]) => {
                        let _ = tokio::fs::write(&path, b).await;
                    }
                    _ => {
                        let _ = tokio::fs::write(&miss, b"").await;
                    }
                }
                got.filter(|b| b.len() > 100 && b.starts_with(&[0xff, 0xd8]))
            }
        }
    };
    match bytes {
        Some(b) => Response::builder()
            .header(header::CONTENT_TYPE, "image/jpeg")
            .header(header::CACHE_CONTROL, "public, max-age=2592000, immutable")
            .body(Body::from(b))
            .unwrap(),
        None => nf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_names() {
        assert!(icon_name_ok("classicon_deathknight"));
        assert!(icon_name_ok("achievement_character_human_male"));
        assert!(!icon_name_ok("../etc/passwd"));
        assert!(!icon_name_ok("A"));
        assert!(!icon_name_ok(""));
    }

    #[test]
    fn specs() {
        assert_eq!(specs_of("druid").len(), 4);
        assert!(specs_of("DEATHKNIGHT").contains(&"Unholy"));
        assert!(specs_of("nope").is_empty());
    }
}
