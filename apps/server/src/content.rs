//! One renderer for public content and the read-only Admin preview. Existing
//! unmarked records retain plain-text rendering; Markdown is explicit per entry.
use crate::admin::{Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::{extract::State, http::header, routing::post, Json, Router};
use linkify::{LinkFinder, LinkKind};
use pulldown_cmark::{html, Event, Parser, Tag, TagEnd};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

pub const MAX_SOURCE: usize = 20_000;
pub fn format_ok(format: Option<&str>) -> ApiResult<()> {
    if format.is_some_and(|f| f != "markdown") {
        return Err(AppError::bad_request(
            "invalid_format",
            "Choose Markdown or plain text.",
        ));
    }
    Ok(())
}
pub fn source(s: &str) -> ApiResult<String> {
    let s: String = s
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();
    if s.chars().count() > MAX_SOURCE {
        return Err(AppError::bad_request(
            "too_long",
            "Use up to 20,000 characters per section.",
        ));
    }
    Ok(s.trim().to_string())
}
pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn links(text: &str) -> Vec<(usize, usize)> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    finder
        .links(text)
        .filter(|l| l.as_str().starts_with("https://") || l.as_str().starts_with("http://"))
        .map(|l| (l.start(), l.end()))
        .collect()
}

pub fn markdown(source: &str) -> String {
    let mut events = Vec::new();
    let (mut in_link, mut in_code) = (false, false);
    for event in Parser::new(source) {
        match event {
            // Raw HTML is displayed literally, never interpreted as markup.
            Event::Html(s) | Event::InlineHtml(s) => events.push(Event::Text(s)),
            Event::SoftBreak => events.push(Event::HardBreak),
            Event::Start(Tag::Link { .. }) => {
                in_link = true;
                events.push(event);
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
                events.push(event);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                in_code = true;
                events.push(event);
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                events.push(event);
            }
            Event::Text(text) if !in_link && !in_code => {
                let mut at = 0;
                for (start, end) in links(&text) {
                    events.push(Event::Text(text[at..start].to_owned().into()));
                    events.push(Event::Start(Tag::Link {
                        link_type: pulldown_cmark::LinkType::Autolink,
                        dest_url: text[start..end].to_owned().into(),
                        title: "".into(),
                        id: "".into(),
                    }));
                    events.push(Event::Text(text[start..end].to_owned().into()));
                    events.push(Event::End(TagEnd::Link));
                    at = end;
                }
                events.push(Event::Text(text[at..].to_owned().into()));
            }
            _ => events.push(event),
        }
    }
    let mut dirty = String::new();
    html::push_html(&mut dirty, events.into_iter());
    ammonia::Builder::default()
        .tags(HashSet::from([
            "p",
            "br",
            "strong",
            "em",
            "a",
            "ul",
            "ol",
            "li",
            "code",
            "pre",
            "blockquote",
        ]))
        .generic_attributes(HashSet::new())
        .tag_attributes(HashMap::from([("a", HashSet::from(["href", "title"]))]))
        .url_schemes(HashSet::from(["http", "https"]))
        .link_rel(Some("noopener noreferrer nofollow"))
        .clean(&dirty)
        .to_string()
}

/// Plain text for News list excerpts; never a second HTML renderer.
pub fn plain_markdown(source: &str) -> String {
    let mut out = String::new();
    for event in Parser::new(source) {
        match event {
            Event::Text(s) | Event::Code(s) | Event::Html(s) | Event::InlineHtml(s) => {
                out.push_str(&s)
            }
            Event::SoftBreak | Event::HardBreak | Event::End(TagEnd::Paragraph | TagEnd::Item) => {
                out.push(' ')
            }
            _ => {}
        }
    }
    out
}

pub fn section(items: &[String], format: Option<&str>) -> String {
    if format == Some("markdown") {
        return markdown(&items.join("\n\n"));
    }
    if items.is_empty() {
        return String::new();
    }
    format!(
        "<ul>{}</ul>",
        items
            .iter()
            .map(|s| format!("<li>{}</li>", escape(s)))
            .collect::<String>()
    )
}
fn legacy_links(text: &str) -> String {
    let (mut out, mut at) = (String::new(), 0);
    for (start, end) in links(text)
        .into_iter()
        .filter(|(a, _)| text[*a..].starts_with("https://"))
    {
        out.push_str(&escape(&text[at..start]));
        let url = escape(&text[start..end]);
        out.push_str(&format!("<a href=\"{url}\" rel=\"noopener\">{url}</a>"));
        at = end;
    }
    out.push_str(&escape(&text[at..]));
    out
}
pub fn news(body: &str, format: Option<&str>) -> String {
    if format == Some("markdown") {
        return markdown(body);
    }
    // Match the previous News renderer: blank lines separate paragraphs,
    // single newlines become <br>, leading dashes and Markdown stay literal.
    body.split("\n\n")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|p| format!("<p>{}</p>", legacy_links(p).replace('\n', "<br>")))
        .collect()
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/admin/markdown/preview", post(preview))
}
#[derive(Deserialize)]
struct Preview {
    kind: String,
    format: Option<String>,
    #[serde(default)]
    body: String,
    #[serde(default)]
    new: Vec<String>,
    #[serde(default)]
    changes: Vec<String>,
    #[serde(default)]
    fixes: Vec<String>,
}
async fn preview(
    State(st): State<AppState>,
    staff: Staff,
    Json(r): Json<Preview>,
) -> ApiResult<impl axum::response::IntoResponse> {
    staff.need(Role::Admin)?;
    st.rate_limit(
        format!("mdpreview:{}", staff.user.id),
        240,
        Duration::from_secs(60),
    )?;
    format_ok(r.format.as_deref())?;
    let format = r.format.as_deref();
    let result: Value = match r.kind.as_str() {
        "news" => {
            let body = source(&r.body)?;
            json!({"html": news(&body, format)})
        }
        "changelog" => {
            for items in [&r.new, &r.changes, &r.fixes] {
                if items.len() > 30 {
                    return Err(AppError::bad_request(
                        "too_many",
                        "Use up to 30 items per section.",
                    ));
                }
                source(&items.join("\n\n"))?;
            }
            json!({"new_html": section(&r.new.iter().map(|s| source(s)).collect::<ApiResult<Vec<_>>>()?, format), "changes_html": section(&r.changes.iter().map(|s| source(s)).collect::<ApiResult<Vec<_>>>()?, format), "fixes_html": section(&r.fixes.iter().map(|s| source(s)).collect::<ApiResult<Vec<_>>>()?, format)})
        }
        _ => {
            return Err(AppError::bad_request(
                "invalid_kind",
                "Choose News or Changelog.",
            ))
        }
    };
    // No audit, save, publish or database writes in this endpoint.
    Ok(([(header::CACHE_CONTROL, "private, no-store")], Json(result)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting_and_breaks() {
        let h = markdown("**Bold** and *italic* with `code`\nnext line\n\n- first\n\n- [second](https://example.com)");
        for expected in [
            "<strong>Bold</strong>",
            "<em>italic</em>",
            "<code>code</code>",
            "<br>",
            "<ul>",
            "https://example.com",
        ] {
            assert!(h.contains(expected), "{h}");
        }
        assert!(!h.contains("<li></li>"));
        assert_eq!(h.matches("<li>").count(), 2);
    }
    #[test]
    fn xss_and_raw_html_are_not_executable() {
        for attack in [
            "<script>alert(1)</script>",
            "<img src=x onerror=alert(1)>",
            "<iframe src=https://evil.test></iframe>",
            "<svg/onload=alert(1)>",
            "[bad](javascript:alert%281%29)",
            "[bad](JaVaScRiPt:alert%281%29)",
            "[bad](jav&#x61;script:alert%281%29)",
            "[bad](data:text/html;base64,PHNjcmlwdD4=)",
            "[bad](vbscript:alert%281%29)",
            "![image](https://evil.test/track.png)",
        ] {
            let h = markdown(attack).to_lowercase();
            for forbidden in [
                "<script",
                "<img",
                "<iframe",
                "<svg",
                "href=\"javascript:",
                "href=\"data:",
                "href=\"vbscript:",
            ] {
                assert!(!h.contains(forbidden), "{attack} => {h}");
            }
        }
    }
    #[test]
    fn links_are_linkified_but_code_is_not() {
        let h = markdown(
            "https://example.com\n\n`https://example.com`\n\n```\nhttps://example.com\n```",
        );
        assert_eq!(h.matches("<a ").count(), 1);
        assert!(h.contains("noopener noreferrer nofollow"));
    }
    #[test]
    fn legacy_entries_stay_plain() {
        assert_eq!(
            section(&["**literal** <b>".into()], None),
            "<ul><li>**literal** &lt;b&gt;</li></ul>"
        );
        let h = news("**literal**\n- one\n- two\n\nhttps://example.com", None);
        assert!(h.starts_with("<p>**literal**<br>- one<br>- two</p>"));
        assert!(!h.contains("<strong>"));
        assert!(h.contains("<a href="));
    }
    #[test]
    fn empty_sections_and_source_limits() {
        assert_eq!(section(&[], Some("markdown")), "");
        assert_eq!(section(&[], None), "");
        assert!(source(&"x".repeat(MAX_SOURCE + 1)).is_err());
        assert!(format_ok(Some("html")).is_err());
    }
}
