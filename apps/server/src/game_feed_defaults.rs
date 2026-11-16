//! Curated official news feeds, store/site links, and Wikipedia titles for
//! popular non-Steam games. Admin overrides on `game_page_meta` still win.

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde_json::{json, Value};

/// BlueTracker WoW RSS (Retail + Forever, with [EU]/[US] duplicates).
pub const WOW_BLUETRACKER: &str = "https://www.bluetracker.gg/rss/wow/";

/// The HTML news page previously stored for WoW Forever — not a feed.
const WOW_FOREVER_HTML_NEWS: &str = "https://worldofwarcraft.blizzard.com/en-us/news";

/// How a curated news feed is applied to one catalogue game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CuratedFeed {
    pub url: &'static str,
    /// Keep only titles containing this (case-insensitive), if set.
    pub title_contains: Option<&'static str>,
    /// Drop titles containing this (case-insensitive), if set.
    pub title_excludes: Option<&'static str>,
    /// Strip a leading `[EU]` / `[US]` tag from the displayed title.
    pub strip_region_prefix: bool,
    /// Collapse near-identical titles (region copies, 1-day date drift).
    pub dedupe: bool,
}

/// Official (or official-community) RSS/Atom URL for a catalogue game id.
pub fn official_news_url(game_id: &str) -> Option<&'static str> {
    official_news(game_id).map(|f| f.url)
}

pub fn official_news(game_id: &str) -> Option<CuratedFeed> {
    Some(match game_id {
        // Roblox posts product updates on the official Developer Forum.
        "roblox" => CuratedFeed {
            url: "https://devforum.roblox.com/c/updates/45.rss",
            title_contains: None,
            title_excludes: None,
            strip_region_prefix: false,
            dedupe: false,
        },
        "wow_forever" => CuratedFeed {
            url: WOW_BLUETRACKER,
            title_contains: Some("Forever"),
            title_excludes: None,
            strip_region_prefix: true,
            dedupe: true,
        },
        "wow" => CuratedFeed {
            url: WOW_BLUETRACKER,
            title_contains: None,
            title_excludes: Some("Forever"),
            strip_region_prefix: true,
            dedupe: true,
        },
        _ => return None,
    })
}

/// Whether a stored admin news URL should be ignored in favour of the curated
/// default. Used so the non-RSS Blizzard news page on WoW Forever is replaced
/// in code only — the live row is not edited.
pub fn ignore_admin_news_url(game_id: &str, url: &str) -> bool {
    if game_id != "wow_forever" {
        return false;
    }
    let got = normalize_url(url);
    let want = normalize_url(WOW_FOREVER_HTML_NEWS);
    got == want
}

/// Preferred English Wikipedia title for a catalogue game id, when the
/// catalogue name would be ambiguous or would miss the article.
pub fn wiki_title(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        "minecraft_bedrock" => "Minecraft",
        "minecraft_java" => "Minecraft (Java Edition)",
        "roblox" => "Roblox",
        "lol" => "League of Legends",
        "valorant" => "Valorant (video game)",
        "fortnite" => "Fortnite",
        "genshin" => "Genshin Impact",
        "hsr" => "Honkai: Star Rail",
        "hearthstone" => "Hearthstone",
        "wow" => "World of Warcraft",
        "wow_classic" => "World of Warcraft",
        "star_citizen" => "Star Citizen",
        "osu" => "Osu!",
        "diablo" => "Diablo (video game)",
        "diablo2" => "Diablo II",
        "diablo3" => "Diablo III",
        "diablo_immortal" => "Diablo Immortal",
        "sc2" => "StarCraft II",
        "starcraft" => "StarCraft (video game)",
        "hots" => "Heroes of the Storm",
        "2xko" => "2XKO",
        "maplestory" => "MapleStory",
        "tibia" => "Tibia (video game)",
        "ragnarok_online" => "Ragnarok Online",
        "lineage_ii" => "Lineage II",
        "crossfire" => "CrossFire (video game)",
        "escape_from_tarkov_arena" => "Escape from Tarkov",
        "goddess_of_victory_nikke" => "Goddess of Victory: Nikke",
        _ => return None,
    })
}

/// Curated official site or shop URL for a popular non-Steam catalogue game.
pub fn official_site_url(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        "wow" | "wow_classic" => "https://worldofwarcraft.blizzard.com/en-us/",
        "wow_forever" | "wow_forever_beta" => "https://worldofwarcraft.blizzard.com/en-us/",
        "minecraft_bedrock" | "minecraft_java" => "https://www.minecraft.net/",
        "roblox" => "https://www.roblox.com/",
        "lol" => "https://www.leagueoflegends.com/",
        "valorant" => "https://playvalorant.com/",
        "fortnite" => "https://www.fortnite.com/",
        "genshin" => "https://genshin.hoyoverse.com/",
        "hsr" => "https://hsr.hoyoverse.com/",
        "hearthstone" => "https://hearthstone.blizzard.com/",
        "star_citizen" => "https://robertsspaceindustries.com/",
        "osu" => "https://osu.ppy.sh/",
        "diablo" | "diablo2" | "diablo3" | "diablo_immortal" => {
            "https://us.shop.battle.net/en-us/family/diablo"
        }
        "sc2" | "starcraft" => "https://starcraft.blizzard.com/",
        "hots" => "https://heroesofthestorm.blizzard.com/",
        "maplestory" | "maplestory_2" | "maplestory_n" => "https://www.nexon.com/maplestory/",
        _ => return None,
    })
}

/// Button label for a store or official site URL.
pub fn official_link_label(url: &str) -> &'static str {
    let host = url_host(url);
    if host.contains("steampowered.com") || host.contains("steamcommunity.com") {
        "View on Steam"
    } else if host.contains("battle.net") || host.contains("blizzard.com") {
        "View on Battle.net"
    } else {
        "Official site"
    }
}

fn url_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.trim_start_matches("www.").to_ascii_lowercase())
        })
        .unwrap_or_default()
}

fn normalize_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// Filter, strip region tags, and dedupe a parsed news feed for one game.
/// `feed_url` is the URL that was actually fetched. If it is not this game's
/// curated feed, items are only capped.
pub fn refine_news(game_id: &str, feed_url: &str, items: Vec<Value>) -> Vec<Value> {
    let max = crate::game_pages::MAX_NEWS;
    let Some(cfg) = official_news(game_id) else {
        return items.into_iter().take(max).collect();
    };
    if normalize_url(feed_url) != normalize_url(cfg.url) {
        return items.into_iter().take(max).collect();
    }
    let mut out: Vec<Value> = items
        .into_iter()
        .filter(|it| title_passes(it_title(it), cfg))
        .collect();
    // Dedupe first so a leading [US]/[EU] tag can still be used to prefer US.
    if cfg.dedupe {
        out = dedupe_news(out);
    }
    if cfg.strip_region_prefix {
        for it in &mut out {
            if let Some(t) = it.get("title").and_then(|x| x.as_str()) {
                let (_, display) = strip_region_prefix(t);
                it["title"] = json!(display);
            }
        }
    }
    out.into_iter().take(max).collect()
}

fn title_passes(title: &str, cfg: CuratedFeed) -> bool {
    if let Some(need) = cfg.title_contains {
        if !contains_ci(title, need) {
            return false;
        }
    }
    if let Some(drop) = cfg.title_excludes {
        if contains_ci(title, drop) {
            return false;
        }
    }
    true
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    hay.to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn it_title(it: &Value) -> &str {
    it.get("title").and_then(|t| t.as_str()).unwrap_or("")
}

/// Returns the region tag (US/EU) if present, and the title with it removed.
pub fn strip_region_prefix(title: &str) -> (Option<&'static str>, String) {
    let t = title.trim();
    let up = t.to_ascii_uppercase();
    for (tag, name) in [("[US]", "US"), ("[EU]", "EU")] {
        if up.starts_with(tag) {
            return (Some(name), t[tag.len()..].trim().to_string());
        }
    }
    (None, t.to_string())
}

/// Collapse near-identical titles. Prefer a US copy when one exists in the
/// group; otherwise keep the most recent.
pub fn dedupe_news(items: Vec<Value>) -> Vec<Value> {
    let parsed: Vec<NewsKey> = items.iter().map(NewsKey::from_item).collect();
    let mut used = vec![false; parsed.len()];
    let mut keep: Vec<usize> = Vec::new();
    for i in 0..parsed.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];
        used[i] = true;
        for j in (i + 1)..parsed.len() {
            if used[j] {
                continue;
            }
            if parsed[i].near_duplicate(&parsed[j]) {
                used[j] = true;
                group.push(j);
            }
        }
        keep.push(pick_group(&parsed, &group));
    }
    keep.sort_by(|&a, &b| parsed[a].cmp_recency(&parsed[b]).reverse());
    keep.into_iter().map(|i| items[i].clone()).collect()
}

struct NewsKey {
    stem: String,
    title_date: Option<NaiveDate>,
    at: Option<DateTime<Utc>>,
    region: Option<&'static str>,
}

impl NewsKey {
    fn from_item(it: &Value) -> Self {
        let raw = it_title(it);
        let (region, display) = strip_region_prefix(raw);
        let (stem, title_date) = stem_and_date(&display, it.get("at").and_then(|x| x.as_str()));
        let at = it
            .get("at")
            .and_then(|x| x.as_str())
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|t| t.with_timezone(&Utc));
        NewsKey {
            stem,
            title_date,
            at,
            region,
        }
    }

    fn near_duplicate(&self, other: &Self) -> bool {
        if self.stem.is_empty() || self.stem != other.stem {
            return false;
        }
        match (self.title_date, other.title_date) {
            (Some(a), Some(b)) => (a - b).num_days().abs() <= 1,
            (None, None) => match (self.at, other.at) {
                (Some(a), Some(b)) => (a - b).num_hours().abs() <= 24,
                _ => true,
            },
            // One title has a date and the other does not: only merge if posted close together.
            (Some(_), None) | (None, Some(_)) => match (self.at, other.at) {
                (Some(a), Some(b)) => (a - b).num_hours().abs() <= 24,
                _ => false,
            },
        }
    }

    fn cmp_recency(&self, other: &Self) -> std::cmp::Ordering {
        self.at.cmp(&other.at)
    }
}

fn pick_group(keys: &[NewsKey], group: &[usize]) -> usize {
    let mut best = group[0];
    for &i in &group[1..] {
        let a = &keys[best];
        let b = &keys[i];
        let a_us = a.region == Some("US");
        let b_us = b.region == Some("US");
        if a_us != b_us {
            if b_us {
                best = i;
            }
            continue;
        }
        if b.cmp_recency(a).is_gt() {
            best = i;
        }
    }
    best
}

fn stem_and_date(display: &str, at: Option<&str>) -> (String, Option<NaiveDate>) {
    let year_hint = at
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|t| t.year())
        .unwrap_or_else(|| Utc::now().year());
    let (without, date) = take_dates(display, year_hint);
    let stem: String = without
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (stem, date)
}

const MONTHS: [(&str, u32); 12] = [
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("may", 5),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
];

/// Pull a calendar date out of a title and return the title with that span removed.
fn take_dates(s: &str, year_hint: i32) -> (String, Option<NaiveDate>) {
    let low = s.to_ascii_lowercase();
    let tokens: Vec<&str> = low.split_whitespace().collect();
    // "november 14, 2026" / "november 14" / "november 13, 2026"
    for i in 0..tokens.len() {
        if let Some(month) = month_num(tokens[i].trim_matches(|c: char| !c.is_ascii_alphabetic())) {
            if i + 1 < tokens.len() {
                if let Some(day) = parse_day(tokens[i + 1]) {
                    let year = if i + 2 < tokens.len() {
                        parse_year(tokens[i + 2]).unwrap_or(year_hint)
                    } else {
                        year_hint
                    };
                    if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
                        let cut = cut_tokens(
                            s,
                            i,
                            if i + 2 < tokens.len() && parse_year(tokens[i + 2]).is_some() {
                                3
                            } else {
                                2
                            },
                        );
                        return (cut, Some(d));
                    }
                }
            }
        }
        // "15 november" / "15 november, 2026" / "-15 november"
        if let Some(day) = parse_day(tokens[i]) {
            if i + 1 < tokens.len() {
                if let Some(month) =
                    month_num(tokens[i + 1].trim_matches(|c: char| !c.is_ascii_alphabetic()))
                {
                    let year = if i + 2 < tokens.len() {
                        parse_year(tokens[i + 2]).unwrap_or(year_hint)
                    } else {
                        year_hint
                    };
                    if let Some(d) = NaiveDate::from_ymd_opt(year, month, day) {
                        let n = if i + 2 < tokens.len() && parse_year(tokens[i + 2]).is_some() {
                            3
                        } else {
                            2
                        };
                        let cut = cut_tokens(s, i, n);
                        return (cut, Some(d));
                    }
                }
            }
        }
    }
    (s.to_string(), None)
}

fn month_num(tok: &str) -> Option<u32> {
    MONTHS.iter().find(|(n, _)| *n == tok).map(|(_, m)| *m)
}

fn parse_day(tok: &str) -> Option<u32> {
    let d: u32 = tok
        .trim_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    (1..=31).contains(&d).then_some(d)
}

fn parse_year(tok: &str) -> Option<i32> {
    let y: i32 = tok
        .trim_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    (1990..=2100).contains(&y).then_some(y)
}

fn cut_tokens(original: &str, start: usize, count: usize) -> String {
    let spans: Vec<(usize, usize)> = original
        .split_whitespace()
        .scan(0usize, |pos, tok| {
            if let Some(rel) = original[*pos..].find(tok) {
                let a = *pos + rel;
                let b = a + tok.len();
                *pos = b;
                Some((a, b))
            } else {
                None
            }
        })
        .collect();
    if start >= spans.len() {
        return original.to_string();
    }
    let end = (start + count - 1).min(spans.len() - 1);
    let mut out = String::new();
    out.push_str(original[..spans[start].0].trim_end());
    out.push(' ');
    if end + 1 < spans.len() {
        out.push_str(original[spans[end].1..].trim_start());
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_map_covers_popular_non_steam() {
        assert_eq!(wiki_title("minecraft_bedrock"), Some("Minecraft"));
        assert_eq!(wiki_title("roblox"), Some("Roblox"));
        assert_eq!(wiki_title("lol"), Some("League of Legends"));
        assert_eq!(
            official_news_url("roblox"),
            Some("https://devforum.roblox.com/c/updates/45.rss")
        );
        assert_eq!(official_news_url("wow_forever"), Some(WOW_BLUETRACKER));
        assert_eq!(official_news_url("wow"), Some(WOW_BLUETRACKER));
        assert_eq!(
            official_news("wow_forever").unwrap().title_contains,
            Some("Forever")
        );
        assert_eq!(
            official_news("wow").unwrap().title_excludes,
            Some("Forever")
        );
        // A distinct product — do not silently reuse retail WoW's article.
        assert_eq!(wiki_title("wow_forever"), None);
        assert_eq!(official_news_url("minecraft_bedrock"), None);
        assert_eq!(
            official_site_url("minecraft_bedrock"),
            Some("https://www.minecraft.net/")
        );
        assert_eq!(
            official_site_url("wow_forever"),
            Some("https://worldofwarcraft.blizzard.com/en-us/")
        );
        assert_eq!(
            official_link_label("https://store.steampowered.com/app/730"),
            "View on Steam"
        );
        assert_eq!(
            official_link_label("https://worldofwarcraft.blizzard.com/en-us/"),
            "View on Battle.net"
        );
        assert_eq!(
            official_link_label("https://www.minecraft.net/"),
            "Official site"
        );
    }

    #[test]
    fn wow_forever_html_news_url_is_ignored() {
        assert!(ignore_admin_news_url(
            "wow_forever",
            "https://worldofwarcraft.blizzard.com/en-us/news"
        ));
        assert!(ignore_admin_news_url(
            "wow_forever",
            "https://worldofwarcraft.blizzard.com/en-us/news/"
        ));
        assert!(!ignore_admin_news_url("wow", WOW_FOREVER_HTML_NEWS));
        assert!(!ignore_admin_news_url(
            "wow_forever",
            "https://www.bluetracker.gg/rss/wow/"
        ));
    }

    fn item(title: &str, at: &str, url: &str) -> Value {
        json!({ "title": title, "at": at, "url": url })
    }

    #[test]
    fn wow_forever_keeps_forever_titles_strips_region_and_dedupes() {
        let items = vec![
            item(
                "[EU] Player-Level Dungeon XP Adjustment -15 November",
                "2026-11-15T01:18:42+00:00",
                "https://bt/eu-xp",
            ),
            item(
                "[US] Player-Level Dungeon XP Adjustment - November 14",
                "2026-11-15T01:18:23+00:00",
                "https://bt/us-xp",
            ),
            item(
                "[EU] WoW: Forever Hotfixes - November 11, 2026",
                "2026-11-14T01:55:46+00:00",
                "https://bt/eu-hf-11",
            ),
            item(
                "[US] WoW: Forever Hotfixes - November 13, 2026",
                "2026-11-14T01:53:01+00:00",
                "https://bt/us-hf-13",
            ),
            item(
                "[EU] WoW: Forever Hotfixes - November 13, 2026",
                "2026-11-14T01:53:01+00:00",
                "https://bt/eu-hf-13",
            ),
            item(
                "[US] Watch the WoW: Forever Short Story, “Cost of Honor”",
                "2026-11-13T18:00:00+00:00",
                "https://bt/us-story",
            ),
            item(
                "[EU] Watch the WoW: Forever Short Story, “Cost of Honor”",
                "2026-11-13T18:00:00+00:00",
                "https://bt/eu-story",
            ),
        ];
        let forever = refine_news("wow_forever", WOW_BLUETRACKER, items.clone());
        let titles: Vec<&str> = forever
            .iter()
            .map(|i| i["title"].as_str().unwrap())
            .collect();
        assert!(titles.iter().all(|t| t.contains("Forever")));
        assert!(titles.iter().all(|t| !t.starts_with('[')));
        let hf: Vec<_> = forever
            .iter()
            .filter(|i| i["title"].as_str().unwrap().contains("Hotfixes"))
            .collect();
        assert_eq!(hf.len(), 2, "{forever:?}");
        assert!(hf.iter().any(|i| {
            i["title"] == "WoW: Forever Hotfixes - November 13, 2026"
                && i["url"] == "https://bt/us-hf-13"
        }));
        assert!(hf
            .iter()
            .any(|i| { i["title"] == "WoW: Forever Hotfixes - November 11, 2026" }));
        let stories: Vec<_> = forever
            .iter()
            .filter(|i| i["title"].as_str().unwrap().contains("Cost of Honor"))
            .collect();
        assert_eq!(stories.len(), 1);
        assert_eq!(stories[0]["url"], "https://bt/us-story");

        let retail = refine_news("wow", WOW_BLUETRACKER, items);
        let rtitles: Vec<&str> = retail
            .iter()
            .map(|i| i["title"].as_str().unwrap())
            .collect();
        assert!(rtitles
            .iter()
            .all(|t| !t.to_ascii_lowercase().contains("forever")));
        assert_eq!(rtitles.len(), 1, "{rtitles:?}");
        assert_eq!(
            rtitles[0],
            "Player-Level Dungeon XP Adjustment - November 14"
        );
        assert_eq!(retail[0]["url"], "https://bt/us-xp");
    }

    #[test]
    fn refine_skips_filter_for_a_different_feed_url() {
        let items = vec![item(
            "[US] WoW: Forever Hotfixes - November 13, 2026",
            "2026-11-14T01:53:01+00:00",
            "https://example/x",
        )];
        let out = refine_news("wow_forever", "https://example.com/other.xml", items);
        // Not the curated URL, so the title is left as-is (and not filtered).
        assert_eq!(
            out[0]["title"],
            "[US] WoW: Forever Hotfixes - November 13, 2026"
        );
    }
}
