//! News posts: the home page "News" box, `/news` (paged list) and `/news/<slug>`.
//!
//! Written in Admin > News (admins only, every change in `admin_audit`). This is separate
//! from the announcement banner (`announcements`, a short one-line alert on the site and in
//! the app); the admin page can promote a post to the banner, which links to the post.
//!
//! Public: `GET /api/v1/news?page=&per=` and `GET /api/v1/news/{slug or id}`.
//! Posts with a future `published_at` stay hidden until then. Pinned posts come first.
//! Admin: `GET/POST /api/v1/admin/news`, `POST/DELETE /api/v1/admin/news/{id}`.

use crate::admin::{audit, Role, Staff};
use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/news", get(list_public))
        .route("/api/v1/news/{key}", get(one_public))
        .route("/api/v1/admin/news", get(admin_list).post(create))
        .route("/api/v1/admin/news/{id}", post(update).delete(remove))
        .route(
            "/api/v1/admin/news-image",
            post(upload_image).layer(axum::extract::DefaultBodyLimit::max(IMG_MAX + 1024)),
        )
        .route("/api/v1/news-img/{name}", get(serve_image))
}

const IMG_MAX: usize = 5 * 1024 * 1024;

fn img_dir(st: &AppState) -> std::path::PathBuf {
    st.cfg.data_dir.join("news-img")
}

/// jpg/png/webp/gif by magic bytes.
fn sniff(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}

async fn upload_image(
    State(st): State<AppState>,
    s: Staff,
    body: axum::body::Bytes,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    if body.is_empty() || body.len() > IMG_MAX {
        return Err(bad(
            "invalid_image",
            "Upload a JPG, PNG, WebP or GIF up to 5 MB.",
        ));
    }
    let (ext, _) = sniff(&body).ok_or_else(|| {
        bad(
            "invalid_image",
            "That file isn't a JPG, PNG, WebP or GIF image.",
        )
    })?;
    use sha2::Digest;
    let h = sha2::Sha256::digest(&body);
    let name = format!(
        "{}.{ext}",
        h.iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let dir = img_dir(&st);
    std::fs::create_dir_all(&dir).map_err(AppError::internal)?;
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, &body).map_err(AppError::internal)?;
    std::fs::rename(&tmp, dir.join(&name)).map_err(AppError::internal)?;
    Ok(Json(json!({ "url": format!("/api/v1/news-img/{name}") })))
}

async fn serve_image(
    State(st): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let ok = name.len() <= 40
        && name
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.' || c.is_ascii_lowercase())
        && name.matches('.').count() == 1;
    if !ok {
        return Err(AppError::not_found());
    }
    let b = tokio::fs::read(img_dir(&st).join(&name))
        .await
        .map_err(|_| AppError::not_found())?;
    let (_, ct) = sniff(&b).ok_or_else(AppError::not_found)?;
    Ok((
        [
            (header::CONTENT_TYPE, ct),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        b,
    ))
}

fn clean_image(l: &str) -> ApiResult<Option<String>> {
    let l = l.trim();
    if l.is_empty() {
        return Ok(None);
    }
    let ok = (l.starts_with("https://") || l.starts_with("/api/v1/news-img/"))
        && l.len() <= 500
        && !l
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\"'<>".contains(c));
    if !ok {
        return Err(bad("invalid_image", "Image links start with https://."));
    }
    Ok(Some(l.to_string()))
}

fn clean_label(l: &str) -> ApiResult<Option<String>> {
    let l = l.trim();
    if l.is_empty() {
        return Ok(None);
    }
    if l.chars().count() > 40 || l.chars().any(|c| c.is_control()) {
        return Err(bad(
            "invalid_label",
            "Button labels can be up to 40 characters.",
        ));
    }
    Ok(Some(l.to_string()))
}

type Row = (
    i64,
    String,
    String,
    String,
    Option<String>,
    bool,
    DateTime<Utc>,
    DateTime<Utc>,
    Option<String>,
    Option<String>,
);
const COLS: &str =
    "id, slug, title, body, link, pinned, published_at, updated_at, image, link_label";

fn excerpt(body: &str, max: usize) -> String {
    let flat = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    let cut = cut
        .rsplit_once(' ')
        .map(|(a, _)| a.to_string())
        .unwrap_or(cut);
    format!("{}…", cut.trim_end_matches(|c: char| !c.is_alphanumeric()))
}

fn to_json(r: Row, full: bool) -> Value {
    let (id, slug, title, body, link, pinned, at, upd, image, link_label) = r;
    let mut v = json!({
        "id": id, "slug": slug, "title": title, "summary": excerpt(&body, 220),
        "link": link, "link_label": link_label, "image": image, "pinned": pinned, "at": at, "updated_at": upd, "url": format!("/news/{slug}"),
    });
    if full {
        v["body"] = json!(body);
    }
    v
}

/// Latest posts for the home box (pinned first).
pub async fn latest(st: &AppState, n: i64) -> ApiResult<Value> {
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM news_posts WHERE published_at <= now()
         ORDER BY pinned DESC, published_at DESC, id DESC LIMIT $1"
    )))
    .bind(n)
    .fetch_all(&st.db)
    .await?;
    Ok(json!(rows
        .into_iter()
        .map(|r| to_json(r, false))
        .collect::<Vec<_>>()))
}

#[derive(Deserialize)]
struct PageQ {
    page: Option<i64>,
    per: Option<i64>,
    q: Option<String>,
}

async fn page(st: &AppState, q: &PageQ, public: bool) -> ApiResult<Value> {
    let per = q.per.unwrap_or(10).clamp(1, 50);
    let pg = q.page.unwrap_or(1).max(1);
    let search =
        q.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| {
                format!(
                    "%{}%",
                    s.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            });
    let filter = format!(
        "WHERE {} AND ($1::text IS NULL OR title ILIKE $1 OR body ILIKE $1)",
        if public {
            "published_at <= now()"
        } else {
            "true"
        }
    );
    let total: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT count(*) FROM news_posts {filter}"
    )))
    .bind(&search)
    .fetch_one(&st.db)
    .await?;
    let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM news_posts {filter}
         ORDER BY pinned DESC, published_at DESC, id DESC LIMIT $2 OFFSET $3"
    )))
    .bind(&search)
    .bind(per)
    .bind((pg - 1) * per)
    .fetch_all(&st.db)
    .await?;
    let now = Utc::now();
    Ok(json!({
        "items": rows.into_iter().map(|r| {
            let scheduled = r.6 > now;
            let mut v = to_json(r, !public);
            if !public { v["scheduled"] = json!(scheduled); }
            v
        }).collect::<Vec<_>>(),
        "total": total, "page": pg, "per": per,
        "pages": ((total + per - 1) / per).max(1),
    }))
}

async fn list_public(
    State(st): State<AppState>,
    Query(q): Query<PageQ>,
) -> ApiResult<impl IntoResponse> {
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json(page(&st, &q, true).await?),
    ))
}

async fn one_public(
    State(st): State<AppState>,
    Path(key): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let id: Option<i64> = key.parse().ok();
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM news_posts WHERE (slug = $1 OR id = $2) AND published_at <= now()"
    )))
    .bind(key.to_lowercase())
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let row = row.ok_or_else(AppError::not_found)?;
    let id = row.0;
    // Neighbours for "Newer / Older" links on the post page.
    let at = row.6;
    let older: Option<(String, String)> = sqlx::query_as(
        "SELECT slug, title FROM news_posts WHERE published_at <= now() AND (published_at, id) < ($1, $2)
         ORDER BY published_at DESC, id DESC LIMIT 1",
    )
    .bind(at)
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let newer: Option<(String, String)> = sqlx::query_as(
        "SELECT slug, title FROM news_posts WHERE published_at <= now() AND (published_at, id) > ($1, $2)
         ORDER BY published_at ASC, id ASC LIMIT 1",
    )
    .bind(at)
    .bind(id)
    .fetch_optional(&st.db)
    .await?;
    let nb = |o: Option<(String, String)>| {
        o.map(|(s, t)| json!({"slug": s, "title": t, "url": format!("/news/{s}")}))
    };
    Ok((
        [(header::CACHE_CONTROL, "public, max-age=30")],
        Json(json!({ "post": to_json(row, true), "older": nb(older), "newer": nb(newer) })),
    ))
}

// ------------------------------------------------------------------ admin

async fn admin_list(
    State(st): State<AppState>,
    s: Staff,
    Query(q): Query<PageQ>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    Ok(Json(page(&st, &q, false).await?))
}

#[derive(Deserialize)]
struct PostReq {
    title: Option<String>,
    body: Option<String>,
    /// "" clears it.
    link: Option<String>,
    link_label: Option<String>,
    /// "" clears it.
    image: Option<String>,
    pinned: Option<bool>,
    published_at: Option<DateTime<Utc>>,
}

fn bad(code: &'static str, msg: &str) -> AppError {
    AppError::bad_request(code, msg)
}

fn clean_title(t: &str) -> ApiResult<String> {
    let t: String = t
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if t.is_empty() || t.chars().count() > 140 {
        return Err(bad(
            "invalid_title",
            "Give the post a title of up to 140 characters.",
        ));
    }
    Ok(t)
}

fn clean_body(b: &str) -> ApiResult<String> {
    let b: String = b
        .replace("\r\n", "\n")
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if b.chars().count() > 20000 {
        return Err(bad("invalid_body", "Posts can be up to 20,000 characters."));
    }
    Ok(b)
}

fn clean_link(l: &str) -> ApiResult<Option<String>> {
    let l = l.trim();
    if l.is_empty() {
        return Ok(None);
    }
    let ok = (l.starts_with("https://") || (l.starts_with('/') && !l.starts_with("//")))
        && l.len() <= 300
        && !l.chars().any(|c| c.is_whitespace() || c.is_control());
    if !ok {
        return Err(bad("invalid_link", "Links start with https:// or /."));
    }
    Ok(Some(l.to_string()))
}

fn slugify(t: &str) -> String {
    let mut s = String::new();
    for c in t.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
        if s.len() >= 60 {
            break;
        }
    }
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "post".into()
    } else {
        s
    }
}

async fn get_row(st: &AppState, id: i64) -> ApiResult<Option<Row>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLS} FROM news_posts WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(&st.db)
    .await?)
}

async fn create(
    State(st): State<AppState>,
    s: Staff,
    Json(r): Json<PostReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let title = clean_title(r.title.as_deref().unwrap_or(""))?;
    let body = clean_body(r.body.as_deref().unwrap_or(""))?;
    let link = clean_link(r.link.as_deref().unwrap_or(""))?;
    let image = clean_image(r.image.as_deref().unwrap_or(""))?;
    let label = clean_label(r.link_label.as_deref().unwrap_or(""))?;
    let base = slugify(&title);
    let mut slug = base.clone();
    let mut n = 2;
    while sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM news_posts WHERE slug = $1)")
        .bind(&slug)
        .fetch_one(&st.db)
        .await?
    {
        slug = format!("{base}-{n}");
        n += 1;
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO news_posts(slug, title, body, link, pinned, published_at, created_by, image, link_label)
         VALUES($1, $2, $3, $4, $5, coalesce($6, now()), $7, $8, $9) RETURNING id",
    )
    .bind(&slug)
    .bind(&title)
    .bind(&body)
    .bind(&link)
    .bind(r.pinned.unwrap_or(false))
    .bind(r.published_at)
    .bind(s.user.id)
    .bind(&image)
    .bind(&label)
    .fetch_one(&st.db)
    .await?;
    let after = get_row(&st, id)
        .await?
        .map(|r| to_json(r, true))
        .unwrap_or(Value::Null);
    audit(
        &st,
        &s,
        "news.create",
        &format!("news:{id}"),
        Value::Null,
        after.clone(),
    )
    .await?;
    Ok(Json(json!({ "post": after })))
}

async fn update(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
    Json(r): Json<PostReq>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let before = get_row(&st, id).await?.ok_or_else(AppError::not_found)?;
    let title = match &r.title {
        Some(t) => clean_title(t)?,
        None => before.2.clone(),
    };
    let body = match &r.body {
        Some(b) => clean_body(b)?,
        None => before.3.clone(),
    };
    let link = match &r.link {
        Some(l) => clean_link(l)?,
        None => before.4.clone(),
    };
    let image = match &r.image {
        Some(l) => clean_image(l)?,
        None => before.8.clone(),
    };
    let label = match &r.link_label {
        Some(l) => clean_label(l)?,
        None => before.9.clone(),
    };
    let pinned = r.pinned.unwrap_or(before.5);
    let at = r.published_at.unwrap_or(before.6);
    // The slug stays put so shared links keep working.
    sqlx::query(
        "UPDATE news_posts SET title = $2, body = $3, link = $4, pinned = $5, published_at = $6, image = $7, link_label = $8,
         updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(&title)
    .bind(&body)
    .bind(&link)
    .bind(pinned)
    .bind(at)
    .bind(&image)
    .bind(&label)
    .execute(&st.db)
    .await?;
    let after = get_row(&st, id)
        .await?
        .map(|r| to_json(r, true))
        .unwrap_or(Value::Null);
    let action = if r.title.is_none()
        && r.body.is_none()
        && r.link.is_none()
        && r.image.is_none()
        && r.link_label.is_none()
        && r.published_at.is_none()
        && r.pinned.is_some()
    {
        if pinned {
            "news.pin"
        } else {
            "news.unpin"
        }
    } else {
        "news.edit"
    };
    audit(
        &st,
        &s,
        action,
        &format!("news:{id}"),
        to_json(before, true),
        after.clone(),
    )
    .await?;
    Ok(Json(json!({ "post": after })))
}

async fn remove(
    State(st): State<AppState>,
    s: Staff,
    Path(id): Path<i64>,
) -> ApiResult<Json<Value>> {
    s.need(Role::Admin)?;
    let before = get_row(&st, id).await?.ok_or_else(AppError::not_found)?;
    sqlx::query("DELETE FROM news_posts WHERE id = $1")
        .bind(id)
        .execute(&st.db)
        .await?;
    audit(
        &st,
        &s,
        "news.delete",
        &format!("news:{id}"),
        to_json(before, true),
        Value::Null,
    )
    .await?;
    Ok(Json(json!({ "deleted": id })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs_and_excerpts() {
        assert_eq!(
            super::slugify("Beta 1 is out! What's new?"),
            "beta-1-is-out-what-s-new"
        );
        assert_eq!(super::slugify("!!!"), "post");
        let e = super::excerpt(&"word ".repeat(100), 30);
        assert!(e.ends_with('…') && e.chars().count() <= 31);
        assert_eq!(super::sniff(b"GIF89a..").map(|x| x.0), Some("gif"));
        assert!(super::sniff(b"<svg>").is_none());
        assert!(super::clean_image("javascript:x").is_err());
        assert!(super::clean_image("https://a.b/c.png\"x").is_err());
    }
}
