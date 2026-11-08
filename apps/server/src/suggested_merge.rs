//! Suggested Merge. Compares the catalogue with launcher product ids we actually
//! have. Nothing is merged here. A same Steam app id (or another id on the raw
//! games list) can propose a merge; a name on its own never does. Editions and
//! folder-split games are labelled, not offered as merges.
//!
//! Install lists for Epic, GOG, Battle.net, EA, Ubisoft, Xbox and the RSI
//! launcher are read on the player's PC and are not uploaded, so this page
//! cannot see them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::admin::{audit, bad, clean, Role, Staff};
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
        .route("/api/v1/admin/suggested-merge/link", post(link))
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
    source_name: String,
    target_name: String,
    origin: String,
    source_records: i64,
    target_records: i64,
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
            && !item.source_name.to_lowercase().contains(&needle)
            && !item.target_name.to_lowercase().contains(&needle)
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
            "source_name": item.source_name,
            "target_name": item.target_name,
            "origin": item.origin,
            "source_records": item.source_records,
            "target_records": item.target_records,
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

    let all_ids: Vec<String> = groups
        .values()
        .flat_map(|ids| ids.iter().map(|(id, _)| id.clone()))
        .collect();
    let saved = persisted_records(st, &all_ids).await?;
    let mut items = Vec::new();
    for ((launcher, product), ids) in groups {
        if ids.len() < 2 {
            continue;
        }
        let names = names_of(st, &ids).await?;
        let target = canonical(&ids, &names);
        let routed_to = routed_steam(st, launcher.as_str(), product.as_str()).await?;
        for (source, origin) in &ids {
            if source == &target {
                continue;
            }
            let apart = kept_apart(st, source, &target);
            let editions = edition_clash(
                names.get(source).map(String::as_str).unwrap_or(source),
                names.get(&target).map(String::as_str).unwrap_or(&target),
            ) && !is_minted(source);
            let src_n = saved.get(source).copied().unwrap_or(0);
            let dst_n = saved.get(&target).copied().unwrap_or(0);
            let routed = routed_to.as_deref() == Some(target.as_str());
            // A shared launcher id is not itself a merge. Merge is only for two
            // saved histories. Linking is a fix-up and does not move those rows.
            let op = if apart || editions {
                if src_n + dst_n == 0 {
                    continue;
                }
                "look"
            } else if src_n > 0 && dst_n > 0 {
                "merge"
            } else if !routed && src_n > 0 && dst_n == 0 && is_minted(source) {
                "map"
            } else {
                continue;
            };
            let why = if apart {
                format!("{launcher} id {product} is on both, but folder hints keep them as different games. Not a merge. ({origin})")
            } else if editions {
                format!("{launcher} id {product} is on both, and the names look like different editions or sequels. Not a merge. ({origin})")
            } else if op == "map" {
                format!("{launcher} id {product} is already the catalogue game {target}. Link it so future reports use that game. Saved rows stay on {source}. ({origin})")
            } else {
                format!("{launcher} id {product} has saved rows on both {source} and {target}. Combining them is a merge, not a link. ({origin})")
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
                source_name: names.get(source).cloned().unwrap_or_default(),
                target_name: names.get(&target).cloned().unwrap_or_default(),
                origin: origin.to_string(),
                source_records: src_n,
                target_records: dst_n,
                evidence,
                confidence: if op == "merge" && is_minted(source) {
                    "high"
                } else if op == "merge" {
                    "medium"
                } else if op == "map" {
                    "link"
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
        let source_name = crate::names::public_name(st, &source)
            .await?
            .unwrap_or_default();
        let target_name = crate::names::public_name(st, &target)
            .await?
            .unwrap_or_default();
        let prog_ids = vec![source.clone(), target.clone()];
        let prog_saved = persisted_records(st, &prog_ids).await?;
        let src_n = prog_saved.get(&source).copied().unwrap_or(0);
        let dst_n = prog_saved.get(&target).copied().unwrap_or(0);
        let op = if src_n > 0 && dst_n > 0 {
            "merge"
        } else {
            "map"
        };
        items.push(Item {
            key: format!("program:{source}"),
            op,
            launcher: "tracked program".into(),
            product: exe.clone(),
            source: source.clone(),
            target: target.clone(),
            source_name,
            target_name,
            origin: "tracked program".into(),
            source_records: src_n,
            target_records: dst_n,
            evidence: format!("exe:{exe}|into:{target}|{op}"),
            confidence: "high",
            why: if op == "merge" {
                format!("{exe} and {target} both have saved rows. Combining them is a merge and can change the shown total.")
            } else {
                format!("{exe} was tracked on its own and the games list now knows it as {target}. Link it for future reports. Saved rows stay on {source}.")
            },
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

async fn persisted_records(st: &AppState, ids: &[String]) -> ApiResult<HashMap<String, i64>> {
    let mut out: HashMap<String, i64> = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    for sql in [
        "SELECT game_id, count(*)::bigint FROM sessions WHERE game_id = ANY($1) GROUP BY game_id",
        "SELECT game_id, count(*)::bigint FROM hours_entries WHERE game_id = ANY($1) GROUP BY game_id",
        "SELECT game_id, count(*)::bigint FROM steam_playtime WHERE game_id = ANY($1) GROUP BY game_id",
    ] {
        let rows: Vec<(String, i64)> = sqlx::query_as(sql).bind(ids).fetch_all(&st.db).await?;
        for (id, n) in rows {
            *out.entry(id).or_default() += n;
        }
    }
    Ok(out)
}

/// Where this Steam app id already routes, if signatures or a fix-up name one game.
async fn routed_steam(st: &AppState, launcher: &str, product: &str) -> ApiResult<Option<String>> {
    if launcher != "steam" {
        return Ok(None);
    }
    let Ok(app) = product.parse::<u32>() else {
        return Ok(None);
    };
    if let Some(g) = st.sigs.by_steam_appid(app) {
        return Ok(Some(g.id.clone()));
    }
    if let Some(db) = crate::games::detectable_db(st) {
        if let Some(g) = db.unique_steam(app) {
            return Ok(Some(g.slug.clone()));
        }
    }
    let alias: Option<String> = sqlx::query_scalar(
        "SELECT game_id FROM detection_aliases WHERE kind = 'steam' AND pattern = $1",
    )
    .bind(product)
    .fetch_optional(&st.db)
    .await?;
    Ok(alias)
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

/// Folder splits and edition names are not merges, even when a launcher id matches.
pub(crate) fn blocks_merge(
    st: &AppState,
    source: &str,
    target: &str,
    source_name: &str,
    target_name: &str,
) -> Option<&'static str> {
    if kept_apart(st, source, target) {
        return Some(
            "Folder hints keep these as different games. A shared launcher id is not a merge.",
        );
    }
    if edition_clash(source_name, target_name) && !is_minted(source) {
        return Some(
            "The names look like different editions or sequels. A shared launcher id is not a merge.",
        );
    }
    None
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

#[derive(Deserialize)]
struct LinkReq {
    source: String,
    into: String,
}

/// Record a detection fix-up. Does not move sessions, hour rows, or Steam snapshots.
async fn link(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<LinkReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let source = r.source.trim();
    let into = r.into.trim();
    if source.is_empty() || into.is_empty() || source == into {
        return Err(bad("bad_target", "Pick the catalogue game to link to."));
    }
    if !is_minted(source) {
        return Err(bad(
            "not_a_link",
            "Linking is for a Steam id or a tracked program, not two catalogue games.",
        ));
    }
    let names = names_of(
        &st,
        &[(source.to_string(), "link"), (into.to_string(), "link")],
    )
    .await?;
    if let Some(why) = blocks_merge(
        &st,
        source,
        into,
        names.get(source).map(String::as_str).unwrap_or(source),
        names.get(into).map(String::as_str).unwrap_or(into),
    ) {
        return Err(bad("blocked", why));
    }
    let ids = vec![source.to_string(), into.to_string()];
    let before = persisted_records(&st, &ids).await?;
    let (kind, pattern) = if let Some(app) = source.strip_prefix("steam:") {
        if !app.chars().all(|c| c.is_ascii_digit()) || app.is_empty() {
            return Err(bad("bad_target", "That is not a Steam app id."));
        }
        let owners: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM games WHERE steam_appid = $1 AND id NOT LIKE 'steam:%'",
        )
        .bind(app.parse::<i32>().unwrap_or(0))
        .fetch_all(&st.db)
        .await?;
        let mut owners = owners;
        if let Some(g) = st.sigs.by_steam_appid(app.parse().unwrap_or(0)) {
            if !owners.iter().any(|id| id == &g.id) {
                owners.push(g.id.clone());
            }
        }
        if owners.len() > 1 {
            return Err(bad(
                "ambiguous",
                "More than one catalogue game has this Steam id, so it was not linked.",
            ));
        }
        if owners.len() == 1 && owners[0] != into {
            return Err(bad(
                "ambiguous",
                "This Steam id already belongs to a different catalogue game.",
            ));
        }
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT game_id FROM detection_aliases WHERE kind = 'steam' AND pattern = $1",
        )
        .bind(app)
        .fetch_optional(&st.db)
        .await?;
        if let Some(cur) = existing {
            if cur != into {
                return Err(bad(
                    "ambiguous",
                    "This Steam id is already linked to a different game. It was not changed.",
                ));
            }
            return Ok(Json(
                json!({ "ok": true, "already": true, "moved_rows": 0 }),
            ));
        }
        ("steam", app.to_string())
    } else {
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT game_id FROM detection_aliases WHERE kind = 'id' AND lower(pattern) = lower($1)",
        )
        .bind(source)
        .fetch_optional(&st.db)
        .await?;
        if let Some(cur) = existing {
            if cur != into {
                return Err(bad(
                    "ambiguous",
                    "That program is already linked to a different game. It was not changed.",
                ));
            }
            return Ok(Json(
                json!({ "ok": true, "already": true, "moved_rows": 0 }),
            ));
        }
        ("id", source.to_string())
    };
    let known: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM games WHERE id = $1)")
        .bind(into)
        .fetch_one(&st.db)
        .await?;
    if !known && crate::names::list_name(&st, into).is_none() {
        return Err(bad("bad_target", "That catalogue game doesn't exist."));
    }
    sqlx::query(
        "INSERT INTO detection_aliases(kind, pattern, game_id, note, updated_by)
         VALUES($1, $2, $3, $4, $5)",
    )
    .bind(kind)
    .bind(&pattern)
    .bind(into)
    .bind("linked from Suggested Merge; no hours were moved")
    .bind(s.user.id)
    .execute(&st.db)
    .await?;
    let after = persisted_records(&st, &ids).await?;
    if before != after {
        return Err(crate::error::AppError::internal("link changed saved rows"));
    }
    audit(
        &st,
        &s,
        "suggested_merge.link",
        &format!("suggest:{source}"),
        Value::Null,
        json!({ "into": into, "kind": kind, "pattern": pattern, "records": before }),
    )
    .await?;
    Ok(Json(
        json!({ "ok": true, "already": false, "moved_rows": 0 }),
    ))
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
