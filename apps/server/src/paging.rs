//! Search / sort / page helpers for per-user lists (games, friends). Lists are filtered
//! and sorted on the server and only one page goes over the wire.

use serde::Deserialize;
use serde_json::Value;

pub const MAX_LIMIT: usize = 100;

#[derive(Deserialize, Default, Clone)]
pub struct PageQuery {
    pub q: Option<String>,
    pub sort: Option<String>,
    /// `asc` or `desc`; without it each sort uses its natural direction (name A→Z, the rest
    /// biggest/newest first).
    pub dir: Option<String>,
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

/// Sort a games list: total (default), week, recent, name; `dir` flips it (see PageQuery::dir).
/// Ties always go by name A→Z, and games never played stay last when sorting by recent.
pub fn sort_games(items: &mut [Value], sort: Option<&str>, dir: Option<&str>) {
    use std::cmp::Ordering;
    let name = |v: &Value| v["name"].as_str().unwrap_or("").to_lowercase();
    let key = sort.unwrap_or("total");
    let natural_asc = key == "name";
    let asc = match dir {
        Some("asc") => true,
        Some("desc") => false,
        _ => natural_asc,
    };
    let flip = |o: Ordering| if asc { o } else { o.reverse() };
    items.sort_by(|a, b| {
        let primary = match key {
            "name" => flip(name(a).cmp(&name(b))),
            "recent" => {
                let (x, y) = (
                    a["last_played_at"].as_str().unwrap_or(""),
                    b["last_played_at"].as_str().unwrap_or(""),
                );
                match (x.is_empty(), y.is_empty()) {
                    (true, false) => Ordering::Greater,
                    (false, true) => Ordering::Less,
                    _ => flip(x.cmp(y)),
                }
            }
            "week" => flip(
                (num(a, "minutes_7d"), num(a, "total_min"))
                    .cmp(&(num(b, "minutes_7d"), num(b, "total_min"))),
            ),
            _ => flip(num(a, "total_min").cmp(&num(b, "total_min"))),
        };
        primary.then_with(|| name(a).cmp(&name(b)))
    });
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
        sort_games(&mut v, None, None);
        assert_eq!(v[0]["name"], "Game 249");
        let (p, n) = page(v, "game 1", &["name"], 0, 5);
        assert_eq!(n, 111);
        assert_eq!(p.len(), 5);
    }

    #[test]
    fn every_sort_flips_with_dir() {
        let g = |n: &str, t: i64, w: i64, at: &str| json!({"name": n, "total_min": t, "minutes_7d": w, "last_played_at": if at.is_empty() { Value::Null } else { json!(at) }});
        let base = vec![
            g("Beta", 10, 5, "2026-10-02"),
            g("alpha", 30, 0, ""),
            g("Gamma", 20, 9, "2026-10-09"),
        ];
        let order = |sort: &str, dir: Option<&str>| {
            let mut v = base.clone();
            sort_games(&mut v, Some(sort), dir);
            v.iter()
                .map(|x| x["name"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(order("total", None), ["alpha", "Gamma", "Beta"]);
        assert_eq!(order("total", Some("desc")), ["alpha", "Gamma", "Beta"]);
        assert_eq!(order("total", Some("asc")), ["Beta", "Gamma", "alpha"]);
        assert_eq!(order("name", None), ["alpha", "Beta", "Gamma"]);
        assert_eq!(order("name", Some("desc")), ["Gamma", "Beta", "alpha"]);
        assert_eq!(order("week", Some("desc")), ["Gamma", "Beta", "alpha"]);
        assert_eq!(order("week", Some("asc")), ["alpha", "Beta", "Gamma"]);
        // never played stays last either way
        assert_eq!(order("recent", None), ["Gamma", "Beta", "alpha"]);
        assert_eq!(order("recent", Some("asc")), ["Beta", "Gamma", "alpha"]);
    }
}
