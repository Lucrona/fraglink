//! Search / sort / page helpers for per-user lists (games, friends). Lists are filtered
//! and sorted on the server and only one page goes over the wire.

use serde::Deserialize;
use serde_json::Value;

pub const MAX_LIMIT: usize = 100;

#[derive(Deserialize, Default, Clone)]
pub struct PageQuery {
    pub q: Option<String>,
    pub sort: Option<String>,
    pub show: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

impl PageQuery {
    pub fn term(&self) -> String {
        self.q.as_deref().unwrap_or("").trim().to_lowercase()
    }
    pub fn offset(&self) -> usize {
        self.offset.unwrap_or(0)
    }
    pub fn limit(&self, default: usize) -> usize {
        self.limit.unwrap_or(default).clamp(1, MAX_LIMIT)
    }
}

fn num(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or(0)
}

/// Sort a games list: total (default), week, recent, name.
pub fn sort_games(items: &mut [Value], sort: Option<&str>) {
    let name = |v: &Value| v["name"].as_str().unwrap_or("").to_lowercase();
    match sort.unwrap_or("total") {
        "name" => items.sort_by_key(name),
        "recent" => items.sort_by(|a, b| {
            b["last_played_at"]
                .as_str()
                .unwrap_or("")
                .cmp(a["last_played_at"].as_str().unwrap_or(""))
        }),
        "week" => items.sort_by(|a, b| {
            (num(b, "minutes_7d"), num(b, "total_min"))
                .cmp(&(num(a, "minutes_7d"), num(a, "total_min")))
        }),
        _ => items.sort_by(|a, b| {
            num(b, "total_min")
                .cmp(&num(a, "total_min"))
                .then_with(|| name(a).cmp(&name(b)))
        }),
    }
}

/// Filter by a case-insensitive substring of `fields`, then cut one page. Returns (page, matches).
pub fn page(
    items: Vec<Value>,
    term: &str,
    fields: &[&str],
    offset: usize,
    limit: usize,
) -> (Vec<Value>, usize) {
    let hits: Vec<Value> = items
        .into_iter()
        .filter(|v| {
            term.is_empty()
                || fields.iter().any(|f| {
                    v[*f]
                        .as_str()
                        .is_some_and(|s| s.to_lowercase().contains(term))
                })
        })
        .collect();
    let total = hits.len();
    (hits.into_iter().skip(offset).take(limit).collect(), total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn filter_sort_page() {
        let mut v: Vec<Value> = (0..250)
            .map(|i| json!({"name": format!("Game {i}"), "total_min": i, "minutes_7d": 0}))
            .collect();
        sort_games(&mut v, None);
        assert_eq!(v[0]["name"], "Game 249");
        let (p, n) = page(v, "game 1", &["name"], 0, 5);
        assert_eq!(n, 111);
        assert_eq!(p.len(), 5);
    }
}
