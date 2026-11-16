//! Wikipedia / Wikidata fallback for non-Steam game info and a short blurb.
//! No paid key. Requests identify Fraglink and share the process-wide feed gate.

use crate::game_pages::{self, acquire, fetch_follow, upsert_details, InfoBits};
use crate::AppState;
use chrono::NaiveDate;
use serde_json::{json, Value};
use url::Url;

const EXTRACT_MAX: usize = 600;

#[derive(Default, Clone)]
pub struct WikiBits {
    pub title: Option<String>,
    pub extract: Option<String>,
    pub url: Option<String>,
    pub wikidata_id: Option<String>,
    pub info: InfoBits,
}

/// Fetch (or reuse) Wikipedia/Wikidata details for a non-Steam game.
pub async fn refresh_wiki(
    st: &AppState,
    game_id: &str,
    game_name: &str,
) -> anyhow::Result<Option<WikiBits>> {
    let Some(base) = st.cfg.wikipedia_url.clone() else {
        return Ok(None);
    };
    if !game_pages::cache_due(
        st,
        game_id,
        "details_ok",
        "details_fetched_at",
        st.cfg.game_store_ttl_secs,
    )
    .await?
    {
        return Ok(None);
    }
    if !acquire(st) {
        return Ok(None);
    }
    let mapped = crate::game_feed_defaults::wiki_title(game_id).map(|s| s.to_string());
    let resolved = if let Some(t) = mapped {
        t
    } else {
        match search_title(&base, game_name).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                upsert_details(st, game_id, None, None, false, Some("wiki")).await?;
                return Ok(None);
            }
            Err(e) => {
                tracing::debug!("wiki search {game_id}: {e}");
                upsert_details(st, game_id, None, None, false, Some("wiki")).await?;
                return Ok(None);
            }
        }
    };
    let summary = match fetch_summary(&base, &resolved).await {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("wiki summary {game_id} ({resolved}): {e}");
            upsert_details(st, game_id, None, None, false, Some("wiki")).await?;
            return Ok(None);
        }
    };
    let mut bits = match parse_summary(&summary) {
        Some(b) => b,
        None => {
            upsert_details(st, game_id, None, None, false, Some("wiki")).await?;
            return Ok(None);
        }
    };
    if let (Some(wd_base), Some(qid)) = (st.cfg.wikidata_url.clone(), bits.wikidata_id.clone()) {
        if acquire(st) {
            match fetch_entity(&wd_base, &qid).await {
                Ok(info) => bits.info = merge_info(bits.info, info),
                Err(e) => tracing::debug!("wikidata {qid}: {e}"),
            }
        }
    }
    if bits.extract.is_none() && bits.info.developer.is_none() && bits.info.publisher.is_none() {
        upsert_details(st, game_id, None, None, false, Some("wiki")).await?;
        return Ok(None);
    }
    let payload = wiki_payload(&bits);
    upsert_details(st, game_id, None, Some(&payload), true, Some("wiki")).await?;
    Ok(Some(bits))
}

fn wiki_payload(b: &WikiBits) -> Value {
    json!({
        "kind": "wiki",
        "title": b.title,
        "extract": b.extract,
        "url": b.url,
        "wikidata_id": b.wikidata_id,
        "developers": b.info.developer,
        "publishers": b.info.publisher,
        "release_date": b.info.release_date,
        "genres": b.info.genres,
        "platforms": b.info.platforms,
        "short_description": b.extract,
    })
}

pub fn bits_from_cached(v: &Value) -> Option<WikiBits> {
    if v.get("kind").and_then(|k| k.as_str()) != Some("wiki")
        && v.get("extract").is_none()
        && v.get("wikidata_id").is_none()
    {
        return None;
    }
    let text = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    Some(WikiBits {
        title: text("title"),
        extract: text("extract").or_else(|| text("short_description")),
        url: text("url"),
        wikidata_id: text("wikidata_id"),
        info: InfoBits {
            developer: text("developers").or_else(|| text("developer")),
            publisher: text("publishers").or_else(|| text("publisher")),
            release_date: text("release_date"),
            genres: text("genres"),
            platforms: text("platforms"),
        },
    })
}

fn api_url(base: &str, path: &str, pairs: &[(&str, &str)]) -> anyhow::Result<Url> {
    let mut u = Url::parse(&format!("{}{path}", base.trim_end_matches('/')))?;
    u.query_pairs_mut().extend_pairs(pairs);
    Ok(u)
}

async fn search_title(base: &str, name: &str) -> anyhow::Result<Option<String>> {
    let q = format!("{name} video game");
    let url = api_url(
        base,
        "/w/api.php",
        &[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", &q),
            ("srlimit", "5"),
            ("srprop", "snippet"),
            ("format", "json"),
        ],
    )?;
    let text = fetch_follow(url.as_str()).await?;
    let v: Value = serde_json::from_str(&text)?;
    let hits = v
        .pointer("/query/search")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let want = norm(name);
    for hit in &hits {
        let title = hit.get("title").and_then(|t| t.as_str()).unwrap_or("");
        let snippet = hit
            .get("snippet")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if title.is_empty() {
            continue;
        }
        let nt = norm(title);
        let gamey = snippet.contains("video game")
            || snippet.contains("videogame")
            || snippet.contains("computer game")
            || title.contains("(video game)")
            || nt == want
            || nt.starts_with(&want);
        if gamey {
            return Ok(Some(title.to_string()));
        }
    }
    if let Some(title) = hits
        .first()
        .and_then(|h| h.get("title"))
        .and_then(|t| t.as_str())
    {
        if norm(title) == want {
            return Ok(Some(title.to_string()));
        }
    }
    Ok(None)
}

async fn fetch_summary(base: &str, title: &str) -> anyhow::Result<Value> {
    let mut u = Url::parse(&format!(
        "{}/api/rest_v1/page/summary/",
        base.trim_end_matches('/')
    ))?;
    u.path_segments_mut()
        .map_err(|_| anyhow::anyhow!("bad wiki base"))?
        .pop_if_empty()
        .push(&title.replace(' ', "_"));
    let text = fetch_follow(u.as_str()).await?;
    Ok(serde_json::from_str(&text)?)
}

pub fn parse_summary(v: &Value) -> Option<WikiBits> {
    if v.get("type").and_then(|t| t.as_str()) == Some("disambiguation") {
        return None;
    }
    let title = v
        .get("title")
        .and_then(|t| t.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())?;
    let extract = v
        .get("extract")
        .and_then(|t| t.as_str())
        .map(trim_extract)
        .filter(|s| !s.is_empty());
    let url = v
        .pointer("/content_urls/desktop/page")
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            Some(format!(
                "https://en.wikipedia.org/wiki/{}",
                title.replace(' ', "_")
            ))
        });
    let wikidata_id = v
        .get("wikibase_item")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());
    Some(WikiBits {
        title: Some(title),
        extract,
        url,
        wikidata_id,
        info: InfoBits::default(),
    })
}

async fn fetch_entity(base: &str, qid: &str) -> anyhow::Result<InfoBits> {
    if !qid.starts_with('Q') || !qid[1..].chars().all(|c| c.is_ascii_digit()) {
        anyhow::bail!("bad wikidata id");
    }
    let url = api_url(
        base,
        "/w/api.php",
        &[
            ("action", "wbgetentities"),
            ("ids", qid),
            ("props", "claims"),
            ("format", "json"),
        ],
    )?;
    let text = fetch_follow(url.as_str()).await?;
    let v: Value = serde_json::from_str(&text)?;
    let claims = v
        .pointer(&format!("/entities/{qid}/claims"))
        .cloned()
        .unwrap_or(json!({}));
    let ids_for = |prop: &str, limit: usize| -> Vec<String> {
        claims
            .get(prop)
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|c| c.pointer("/mainsnak/datavalue/value/id"))
            .filter_map(|x| x.as_str())
            .map(|s| s.to_string())
            .take(limit)
            .collect()
    };
    let mut need: Vec<String> = Vec::new();
    let mut dev_ids = ids_for("P178", 6);
    if dev_ids.is_empty() {
        // Some pages (e.g. Roblox) record a creator instead of a game developer.
        dev_ids = ids_for("P170", 6);
    }
    let pub_ids = ids_for("P123", 6);
    let genre_ids = ids_for("P136", 8);
    let plat_ids = ids_for("P400", 16);
    for id in dev_ids
        .iter()
        .chain(pub_ids.iter())
        .chain(genre_ids.iter())
        .chain(plat_ids.iter())
    {
        if !need.contains(id) {
            need.push(id.clone());
        }
    }
    let mut labels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if !need.is_empty() {
        let joined = need.join("|");
        let url = api_url(
            base,
            "/w/api.php",
            &[
                ("action", "wbgetentities"),
                ("ids", &joined),
                ("props", "labels"),
                ("languages", "en"),
                ("format", "json"),
            ],
        )?;
        // Second request is not separately gated so a page can finish in one pass.
        if let Ok(text) = fetch_follow(url.as_str()).await {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(ents) = v.get("entities").and_then(|e| e.as_object()) {
                    for (id, ent) in ents {
                        if let Some(lab) = ent.pointer("/labels/en/value").and_then(|x| x.as_str())
                        {
                            if !lab.is_empty() {
                                labels.insert(id.clone(), lab.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    let join = |ids: &[String], tidy: fn(String) -> String| -> Option<String> {
        let parts: Vec<String> = ids
            .iter()
            .filter_map(|id| labels.get(id).cloned())
            .collect();
        if parts.is_empty() {
            None
        } else {
            Some(tidy(parts.join(", ")))
        }
    };
    let released = claims
        .get("P577")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| c.pointer("/mainsnak/datavalue/value/time"))
        .filter_map(|x| x.as_str())
        .filter_map(format_wd_date)
        .min();
    Ok(InfoBits {
        developer: join(&dev_ids, tidy_list),
        publisher: join(&pub_ids, tidy_list),
        release_date: released,
        genres: join(&genre_ids, tidy_list),
        platforms: join(&plat_ids, tidy_platforms),
    })
}

fn merge_info(base: InfoBits, extra: InfoBits) -> InfoBits {
    InfoBits {
        developer: extra.developer.or(base.developer),
        publisher: extra.publisher.or(base.publisher),
        release_date: extra.release_date.or(base.release_date),
        genres: extra.genres.or(base.genres),
        platforms: extra.platforms.or(base.platforms),
    }
}

pub fn trim_extract(s: &str) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= EXTRACT_MAX {
        return t;
    }
    let mut out = String::new();
    for (i, c) in t.chars().enumerate() {
        if i >= EXTRACT_MAX {
            break;
        }
        out.push(c);
    }
    if let Some(i) = out.rfind(". ") {
        out.truncate(i + 1);
        return out;
    }
    if let Some(i) = out.rfind(' ') {
        out.truncate(i);
    }
    out.push('…');
    out
}

pub fn format_wd_date(raw: &str) -> Option<String> {
    let d = raw
        .get(..10)
        .or(Some(raw))
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())?;
    Some(d.format("%-d %b %Y").to_string())
}

fn tidy_list(s: String) -> String {
    let parts: Vec<String> = game_pages::split_list(&s)
        .into_iter()
        .filter(|p| !is_qid(p))
        .take(8)
        .collect();
    parts.join(", ")
}

fn tidy_platforms(s: String) -> String {
    let prefer = [
        "Windows",
        "macOS",
        "Mac OS",
        "Linux",
        "Xbox",
        "PlayStation",
        "Nintendo",
        "iOS",
        "Android",
        "Steam Deck",
    ];
    let parts = game_pages::split_list(&s);
    let mut out = Vec::new();
    for p in &parts {
        if is_qid(p) {
            continue;
        }
        let low = p.to_ascii_lowercase();
        if low.contains("architecture") || low.contains("instruction set") {
            continue;
        }
        let known = prefer.iter().any(|k| p.contains(k));
        if (known || out.len() < 8) && !out.iter().any(|x: &String| x.eq_ignore_ascii_case(p)) {
            out.push(p.clone());
        }
        if out.len() >= 8 {
            break;
        }
    }
    out.join(", ")
}

fn is_qid(s: &str) -> bool {
    let t = s.trim();
    t.starts_with('Q') && t.len() > 1 && t[1..].chars().all(|c| c.is_ascii_digit())
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minecraft_summary() {
        let v = json!({
            "type": "standard",
            "title": "Minecraft",
            "extract": "Minecraft is a 2011 sandbox game developed and published by Mojang Studios.",
            "wikibase_item": "Q49740",
            "content_urls": { "desktop": { "page": "https://en.wikipedia.org/wiki/Minecraft" } }
        });
        let b = parse_summary(&v).unwrap();
        assert_eq!(b.title.as_deref(), Some("Minecraft"));
        assert_eq!(b.wikidata_id.as_deref(), Some("Q49740"));
        assert!(b.extract.unwrap().contains("sandbox"));
        assert!(b.url.unwrap().contains("Minecraft"));
    }

    #[test]
    fn skips_disambiguation() {
        let v = json!({"type": "disambiguation", "title": "Rust"});
        assert!(parse_summary(&v).is_none());
    }

    #[test]
    fn extract_is_trimmed() {
        let long = "Word. ".repeat(200);
        let t = trim_extract(&long);
        assert!(t.chars().count() <= EXTRACT_MAX);
        assert!(t.ends_with('.'));
    }

    #[test]
    fn wikidata_date_format() {
        assert_eq!(
            format_wd_date("2011-11-18T00:00:00Z").as_deref(),
            Some("18 Nov 2011")
        );
    }

    #[test]
    fn cached_wiki_payload_roundtrip() {
        let b = WikiBits {
            title: Some("Roblox".into()),
            extract: Some("Roblox is a game platform.".into()),
            url: Some("https://en.wikipedia.org/wiki/Roblox".into()),
            wikidata_id: Some("Q12873706".into()),
            info: InfoBits {
                developer: Some("Roblox Corporation".into()),
                publisher: Some("Roblox Corporation".into()),
                release_date: Some("1 Sep 2006".into()),
                genres: Some("Massively multiplayer online game".into()),
                platforms: Some("Windows, iOS, Android".into()),
            },
        };
        let v = wiki_payload(&b);
        let back = bits_from_cached(&v).unwrap();
        assert_eq!(back.info.developer.as_deref(), Some("Roblox Corporation"));
        assert_eq!(back.extract.as_deref(), Some("Roblox is a game platform."));
    }
}
