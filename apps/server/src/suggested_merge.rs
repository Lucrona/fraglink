//! Suggested Merge. Compares the catalogue with launcher product ids we actually
//! have. Nothing is merged here. A same Steam app id (or another id on the raw
//! games list) can propose a merge; a name on its own never does. Editions and
//! folder-split games are labelled, not offered as merges.
//!
//! Install lists for Epic, GOG, Battle.net, EA, Ubisoft, Xbox and the RSI
//! launcher are read on the player's PC and are not uploaded, so this page
//! cannot see them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::admin::{audit, clean, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/suggested-merge", get(list))
        .route("/api/v1/admin/suggested-merge/dismiss", post(dismiss))
        .route("/api/v1/admin/suggested-merge/reopen", post(reopen))
}

const PAGE: i64 = 50;

#[derive(Clone)]
struct Item {
    key: String,
    op: &'static str,
    launcher: String,
    product: String,
    source: String,
    target: String,
    evidence: String,
    confidence: &'static str,
    why: String,
}

pub async fn list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<ListQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Moderator)?;
    let show = q.show.as_deref().unwrap_or("open");
    let items = gather(&st).await?;
    let keys: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    let decisions = decisions(&st, &keys).await?;
    let mut rows = Vec::new();
    let mut open = 0i64;
    let mut dismissed = 0i64;
    let needle = q.q.as_deref().unwrap_or("").trim().to_lowercase();
    let want_launcher = q.launcher.as_deref().unwrap_or("").trim().to_lowercase();
    let want_op = q.op.as_deref().unwrap_or("").trim().to_lowercase();
    for item in items {
        let d = decisions.get(&item.key);
        let hidden = d.is_some_and(|d| d.status == "dismissed" && d.evidence == item.evidence);
        if hidden {
            dismissed += 1;
        } else {
            open += 1;
        }
        if show == "dismissed" && !hidden {
            continue;
        }
        if show != "dismissed" && show != "all" && hidden {
            continue;
        }
        if !want_launcher.is_empty() && item.launcher.to_lowercase() != want_launcher {
            continue;
        }
        if !want_op.is_empty() && item.op != want_op {
            continue;
        }
        if !needle.is_empty()
            && !item.source.to_lowercase().contains(&needle)
            && !item.target.to_lowercase().contains(&needle)
            && !item.product.to_lowercase().contains(&needle)
            && !item.why.to_lowercase().contains(&needle)
        {
            continue;
        }
        let mut v = json!({
            "key": item.key,
            "op": item.op,
            "launcher": item.launcher,
            "product": item.product,
            "source": item.source,
            "target": item.target,
            "evidence": item.evidence,
            "confidence": item.confidence,
            "why": item.why,
            "dismissed": hidden,
        });
        if let Some(d) = d {
            v["decision"] = json!({
                "status": d.status, "at": d.at, "by": d.by, "reason": d.reason,
                "evidence": d.evidence, "matches": d.evidence == item.evidence,
            });
        }
        rows.push(v);
    }
    let total = rows.len() as i64;
    let off = q
        .offset
        .unwrap_or(0)
        .max(0)
        .min(total.saturating_sub(1).max(0) / PAGE * PAGE);
    let page: Vec<Value> = rows
        .into_iter()
        .skip(off as usize)
        .take(PAGE as usize)
        .collect();
    Ok(Json(json!({
        "total": total, "open": open, "dismissed": dismissed, "offset": off, "page_size": PAGE,
        "rows": page,
        "coverage": coverage(&st),
    })))
}

#[derive(Deserialize)]
pub struct ListQ {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    launcher: Option<String>,
    #[serde(default)]
    op: Option<String>,
    #[serde(default)]
    show: Option<String>,
    #[serde(default)]
    offset: Option<i64>,
}

#[derive(Clone)]
struct Decision {
    evidence: String,
    status: String,
    reason: Option<String>,
    at: DateTime<Utc>,
    by: Option<String>,
}

async fn decisions(st: &AppState, keys: &[String]) -> ApiResult<HashMap<String, Decision>> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String, String, Option<String>, DateTime<Utc>, Option<String>)> =
        sqlx::query_as(
            "SELECT DISTINCT ON (d.suggestion_key) d.suggestion_key, d.evidence, d.status, d.reason, d.decided_at, u.handle
             FROM suggested_merge_decisions d
             LEFT JOIN users u ON u.id = d.decided_by
             WHERE d.suggestion_key = ANY($1)
             ORDER BY d.suggestion_key, d.decided_at DESC, d.id DESC",
        )
        .bind(keys)
        .fetch_all(&st.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(k, e, s, r, at, by)| {
            (
                k,
                Decision {
                    evidence: e,
                    status: s,
                    reason: r,
                    at,
                    by,
                },
            )
        })
        .collect())
}

fn coverage(st: &AppState) -> Value {
    let raw = std::fs::read(st.cfg.data_dir.join("detectable/discord.json")).is_ok();
    let unavailable = [
        ("epic", "Epic Games Store"),
        ("gog", "GOG Galaxy"),
        ("battlenet", "Battle.net"),
        ("ea", "EA app"),
        ("ubisoft", "Ubisoft Connect"),
        ("xbox", "Xbox app"),
        ("rsi", "RSI launcher"),
    ];
    let mut sources = vec![
        json!({
            "id": "steam_list",
            "label": "Steam app ids on the games list",
            "available": true,
            "detail": "Discord's games list keeps Steam app ids. Those are compared with the catalogue.",
        }),
        json!({
            "id": "steam_builtin",
            "label": "Steam app ids on built-in signatures",
            "available": true,
            "detail": "The signatures shipped with the server.",
        }),
        json!({
            "id": "steam_rows",
            "label": "Steam app ids on game rows and detection fix-ups",
            "available": true,
            "detail": "Stored when a Steam import or a staff fix-up recorded an app id.",
        }),
        json!({
            "id": "other_skus",
            "label": "Other launcher ids on the games list",
            "available": raw,
            "detail": if raw {
                "The downloaded games list is on disk, so other distributor ids in it are used. Ids that are not in that file are not invented."
            } else {
                "The built list this server is serving only kept Steam app ids. Other distributor ids are not in that copy, so none are guessed."
            },
        }),
    ];
    for (id, label) in unavailable {
        sources.push(json!({
            "id": id,
            "label": format!("{label} install list"),
            "available": false,
            "detail": "Read on the player's PC for the Installed games list. It is not uploaded, and this page does not connect a new account to go and get it.",
        }));
    }
    json!({ "sources": sources })
}

async fn gather(st: &AppState) -> ApiResult<Vec<Item>> {
    let mut hits: Vec<(String, String, String, &'static str)> = Vec::new();
    if let Some(db) = crate::games::detectable_db(st) {
        for g in db.games() {
            for app in &g.steam {
                hits.push((
                    "steam".into(),
                    app.to_string(),
                    g.slug.clone(),
                    "games list",
                ));
            }
        }
    }
    for g in st.sigs.games() {
        for app in &g.steam_appids {
            hits.push(("steam".into(), app.to_string(), g.id.clone(), "built-in"));
        }
    }
    let rows: Vec<(String, Option<i32>)> = sqlx::query_as(
        "SELECT id, steam_appid FROM games WHERE steam_appid IS NOT NULL OR id LIKE 'steam:%'",
    )
    .fetch_all(&st.db)
    .await?;
    for (id, app) in rows {
        if let Some(app) = app {
            hits.push(("steam".into(), app.to_string(), id.clone(), "game row"));
        }
        if let Some(rest) = id.strip_prefix("steam:") {
            if rest.chars().all(|c| c.is_ascii_digit()) && !rest.is_empty() {
                hits.push(("steam".into(), rest.to_string(), id, "game row"));
            }
        }
    }
    let aliases: Vec<(String, String)> =
        sqlx::query_as("SELECT pattern, game_id FROM detection_aliases WHERE kind = 'steam'")
            .fetch_all(&st.db)
            .await?;
    for (pattern, game) in aliases {
        if pattern.chars().all(|c| c.is_ascii_digit()) {
            hits.push(("steam".into(), pattern, game, "fix-up"));
        }
    }
    extra_skus(st, &mut hits);

    let mut groups: BTreeMap<(String, String), Vec<(String, &'static str)>> = BTreeMap::new();
    for (launcher, product, id, origin) in hits {
        let id = crate::retire::effective_id(st, &id).await?;
        let entry = groups.entry((launcher, product)).or_default();
        if !entry.iter().any(|(e, _)| e == &id) {
            entry.push((id, origin));
        }
    }

    let mut items = Vec::new();
    for ((launcher, product), ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        let names = names_of(st, &ids).await?;
        let target = canonical(&ids, &names);
        for (source, origin) in &ids {
            if source == &target {
                continue;
            }
            let apart = kept_apart(st, source, &target);
            let editions = edition_clash(
                names.get(source).map(String::as_str).unwrap_or(source),
                names.get(&target).map(String::as_str).unwrap_or(&target),
            ) && !is_minted(source);
            let op = if apart || editions { "look" } else { "merge" };
            let why = if apart {
                format!("{launcher} id {product} is on both, but folder hints keep them as different games. Not a merge. ({origin})")
            } else if editions {
                format!("{launcher} id {product} is on both, and the names look like different editions or sequels. Not a merge. ({origin})")
            } else {
                format!("{launcher} id {product} is on {source} and {target}. Same launcher product, so {source} can be merged into {target}. ({origin})")
            };
            let evidence = format!(
                "{launcher}:{product}|{source}|{target}|{op}|{}",
                names.get(source).cloned().unwrap_or_default()
            );
            items.push(Item {
                key: format!("{op}:{launcher}:{product}:{source}->{target}"),
                op,
                launcher: launcher.clone(),
                product: product.clone(),
                source: source.clone(),
                target: target.clone(),
                evidence,
                confidence: if op == "merge" && is_minted(source) {
                    "high"
                } else if op == "merge" {
                    "medium"
                } else {
                    "look"
                },
                why,
            });
        }
    }
    // Programs players tracked that the games list now detects. Same dismiss store as before.
    crate::merge_suggest::refresh(st).await?;
    let programs: Vec<(String, String)> = sqlx::query_as(
        "SELECT m.game_id, m.into_id FROM merge_suggestions m
         WHERE m.status = 'new'
           AND NOT EXISTS (SELECT 1 FROM game_retirements r WHERE r.game_id = m.game_id AND NOT r.partial)",
    )
    .fetch_all(&st.db)
    .await?;
    for (source, target) in programs {
        let exe = source.trim_start_matches("exe:").to_string();
        items.push(Item {
            key: format!("program:{source}"),
            op: "merge",
            launcher: "tracked program".into(),
            product: exe.clone(),
            source: source.clone(),
            target: target.clone(),
            evidence: format!("exe:{exe}|into:{target}"),
            confidence: "high",
            why: format!(
                "{exe} was tracked as its own entry and the games list now detects it as {target}. This is a game merge, not a launcher link."
            ),
        });
    }
    items.sort_by(|a, b| a.target.cmp(&b.target).then(a.source.cmp(&b.source)));
    Ok(items)
}

fn extra_skus(st: &AppState, hits: &mut Vec<(String, String, String, &'static str)>) {
    let Ok(raw) = std::fs::read(st.cfg.data_dir.join("detectable/discord.json")) else {
        return;
    };
    let Ok(apps) = serde_json::from_slice::<Vec<RawApp>>(&raw) else {
        return;
    };
    let Some(db) = crate::games::detectable_db(st) else {
        return;
    };
    let slug: HashMap<&str, &str> = db
        .games()
        .iter()
        .map(|g| (g.id.as_str(), g.slug.as_str()))
        .collect();
    for app in apps {
        let Some(slug) = slug.get(app.id.as_str()).copied() else {
            continue;
        };
        for sku in app.third_party_skus.unwrap_or_default() {
            let dist = sku.distributor.trim().to_lowercase();
            if dist.is_empty() || dist == "steam" {
                continue;
            }
            let Some(id) = sku_id(&sku.id) else { continue };
            hits.push((dist, id, slug.to_string(), "games list"));
        }
    }
}

fn sku_id(v: &Option<serde_json::Value>) -> Option<String> {
    match v {
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            (!s.is_empty() && s.len() <= 80 && !s.chars().any(char::is_control))
                .then(|| s.to_string())
        }
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|n| n.to_string()),
        _ => None,
    }
}

#[derive(serde::Deserialize)]
struct RawApp {
    id: String,
    #[serde(default)]
    third_party_skus: Option<Vec<RawSku>>,
}

#[derive(serde::Deserialize)]
struct RawSku {
    #[serde(default)]
    distributor: String,
    #[serde(default)]
    id: Option<serde_json::Value>,
}

fn is_minted(id: &str) -> bool {
    id.starts_with("steam:")
        || id.starts_with("exe:")
        || id.starts_with("epic:")
        || id.starts_with("gog:")
}

fn canonical(ids: &[(String, &'static str)], names: &HashMap<String, String>) -> String {
    ids.iter()
        .filter(|(id, _)| !is_minted(id))
        .min_by(|a, b| {
            names
                .get(&a.0)
                .map(|s| s.len())
                .unwrap_or(99)
                .cmp(&names.get(&b.0).map(|s| s.len()).unwrap_or(99))
                .then(a.0.cmp(&b.0))
        })
        .or_else(|| ids.iter().min_by(|a, b| a.0.cmp(&b.0)))
        .map(|(id, _)| id.clone())
        .unwrap_or_default()
}

async fn names_of(
    st: &AppState,
    ids: &[(String, &'static str)],
) -> ApiResult<HashMap<String, String>> {
    let mut out = HashMap::new();
    for (id, _) in ids {
        let name = crate::names::list_name(st, id).or(crate::names::public_name(st, id)
            .await?
            .filter(|n| n != "a game"));
        if let Some(name) = name {
            out.insert(id.clone(), name);
        }
    }
    Ok(out)
}

fn kept_apart(st: &AppState, a: &str, b: &str) -> bool {
    let (ea, ha) = hints(st, a);
    let (eb, hb) = hints(st, b);
    if ha.is_empty() || hb.is_empty() {
        return false;
    }
    let share = ea
        .iter()
        .any(|e| eb.iter().any(|f| e.eq_ignore_ascii_case(f)));
    share && ha != hb
}

fn hints(st: &AppState, id: &str) -> (Vec<String>, Vec<String>) {
    let Some(g) = st.sigs.get(id) else {
        return (Vec::new(), Vec::new());
    };
    let mut exes: Vec<String> = g
        .exes
        .iter()
        .chain(&g.path_exes)
        .map(|e| e.to_lowercase())
        .collect();
    exes.sort();
    exes.dedup();
    let mut hints = g.path_hints.clone();
    hints.sort();
    (exes, hints)
}

fn edition_clash(a: &str, b: &str) -> bool {
    let ma = marks(a);
    let mb = marks(b);
    // "Quest" and "Quest 2" are not the same game. Identical mark sets are.
    ma != mb
}

fn marks(name: &str) -> BTreeSet<String> {
    const MARKS: &[&str] = &[
        "2",
        "3",
        "4",
        "5",
        "ii",
        "iii",
        "iv",
        "remastered",
        "remaster",
        "definitive",
        "goty",
        "classic",
        "forever",
        "retail",
        "ptr",
        "beta",
        "anniversary",
        "deluxe",
        "legacy",
        "hd",
    ];
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| {
            MARKS.contains(&w.as_str()) || (w.len() <= 2 && w.chars().all(|c| c.is_ascii_digit()))
        })
        .collect()
}

#[derive(Deserialize)]
struct DecideReq {
    key: String,
    #[serde(default)]
    reason: Option<String>,
}

async fn dismiss(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<DecideReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    decide(&st, &s, &r, "dismissed").await
}

async fn reopen(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<DecideReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    decide(&st, &s, &r, "reopened").await
}

async fn decide(st: &AppState, s: &Staff, r: &DecideReq, status: &str) -> ApiResult<Json<Value>> {
    let key = r.key.trim();
    if let Some(id) = key.strip_prefix("program:") {
        if status == "dismissed" {
            let n = sqlx::query(
                "UPDATE merge_suggestions SET status = 'dismissed', decided_by = $2, decided_at = now()
                 WHERE game_id = $1 AND status = 'new'",
            )
            .bind(id)
            .bind(s.user.id)
            .execute(&st.db)
            .await?
            .rows_affected();
            if n == 0 {
                return Err(AppError::not_found());
            }
            audit(
                st,
                s,
                "merge_suggestion.dismiss",
                &format!("game:{id}"),
                Value::Null,
                json!({ "status": "dismissed" }),
            )
            .await?;
            return Ok(Json(json!({ "ok": true })));
        }
        sqlx::query(
            "UPDATE merge_suggestions SET status = 'new', decided_by = NULL, decided_at = NULL WHERE game_id = $1 AND status = 'dismissed'",
        )
        .bind(id)
        .execute(&st.db)
        .await?;
        audit(
            st,
            s,
            "merge_suggestion.reopen",
            &format!("game:{id}"),
            Value::Null,
            json!({ "status": "new" }),
        )
        .await?;
        return Ok(Json(json!({ "ok": true })));
    }
    let items = gather(st).await?;
    let item = items
        .iter()
        .find(|i| i.key == key)
        .ok_or_else(AppError::not_found)?;
    let why = r
        .reason
        .as_deref()
        .map(|t| clean(t, 300))
        .filter(|t| !t.is_empty());
    sqlx::query(
        "INSERT INTO suggested_merge_decisions(suggestion_key, evidence, status, reason, decided_by)
         VALUES($1, $2, $3, $4, $5)",
    )
    .bind(&item.key)
    .bind(&item.evidence)
    .bind(status)
    .bind(&why)
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    let action = if status == "dismissed" {
        "suggested_merge.dismiss"
    } else {
        "suggested_merge.reopen"
    };
    audit(
        st,
        s,
        action,
        &format!("suggest:{}", item.key),
        Value::Null,
        json!({ "status": status, "evidence": item.evidence, "reason": why, "source": item.source, "target": item.target }),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}
