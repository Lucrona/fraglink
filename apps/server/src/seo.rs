//! Search engines: the sitemap (an index plus paged files) and the profile page HTML with
//! per-profile robots, canonical and preview tags.
//!
//! Only profiles that are public (adults, or under-18s who switched their profile to public
//! themselves), not test accounts, not banned, deleted, leaving or under age review, are ever
//! indexable, and only while Admin > Public profiles is on. Everything else gets `noindex`.
//! Test, unknown, deleted and leaving accounts answer 404 exactly like an unknown name, so the
//! page never tells anyone a hidden account exists.

use crate::web::esc;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;

/// Profiles per sitemap file (the protocol allows 50,000).
pub const PROFILES_PER_FILE: i64 = 5000;
/// The public pages in the sitemap (path on the canonical site).
pub const PAGES: &[&str] = &[
    "/",
    "/download",
    "/news",
    "/privacy",
    "/terms",
    "/faq",
    "/changelog",
    "/contact",
    "/status",
    "/media",
];
/// SQL condition (on `users u`) for a profile search engines may index (a macro so the
/// queries stay `&'static str`).
macro_rules! indexable {
    () => {
        "u.public_profile AND NOT u.is_test AND u.deleted_at IS NULL AND u.banned_at IS NULL
         AND u.delete_after IS NULL AND u.age_review_at IS NULL"
    };
}
const SEO_START: &str = "<!--seo-->";
const SEO_END: &str = "<!--/seo-->";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/sitemap.xml", get(index))
        .route("/sitemaps/{file}", get(file))
        .route("/u/{*rest}", get(profile_page))
}

fn enc(h: &str) -> String {
    url::form_urlencoded::byte_serialize(h.as_bytes()).collect()
}

fn base(st: &AppState) -> &str {
    st.cfg.canonical_url.trim_end_matches('/')
}

fn xml(body: String) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body,
    )
        .into_response()
}

async fn profile_count(st: &AppState) -> Result<i64, crate::error::AppError> {
    if !crate::features::on(st, "public_profiles").await {
        return Ok(0);
    }
    Ok(
        sqlx::query_scalar(concat!("SELECT count(*) FROM users u WHERE ", indexable!()))
            .fetch_one(&st.db)
            .await?,
    )
}

async fn index(State(st): State<AppState>) -> Result<Response, crate::error::AppError> {
    let n = profile_count(&st).await?;
    let files = (n + PROFILES_PER_FILE - 1) / PROFILES_PER_FILE;
    let b = base(&st);
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<sitemapindex xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    s += &format!("<sitemap><loc>{b}/sitemaps/pages.xml</loc></sitemap>\n");
    for i in 1..=files {
        s += &format!("<sitemap><loc>{b}/sitemaps/profiles-{i}.xml</loc></sitemap>\n");
    }
    s += "</sitemapindex>\n";
    Ok(xml(s))
}

async fn file(
    State(st): State<AppState>,
    Path(file): Path<String>,
) -> Result<Response, crate::error::AppError> {
    let b = base(&st);
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n";
    if file == "pages.xml" {
        let mut s = String::from(head);
        for p in PAGES {
            s += &format!("<url><loc>{b}{p}</loc></url>\n");
        }
        return Ok(xml(s + "</urlset>\n"));
    }
    let page = file
        .strip_prefix("profiles-")
        .and_then(|r| r.strip_suffix(".xml"))
        .and_then(|n| n.parse::<i64>().ok())
        .filter(|n| *n >= 1);
    let total = profile_count(&st).await?;
    let Some(page) = page.filter(|p| (*p - 1) * PROFILES_PER_FILE < total) else {
        return Ok(StatusCode::NOT_FOUND.into_response());
    };
    let handles: Vec<String> = sqlx::query_scalar(concat!(
        "SELECT u.handle FROM users u WHERE ",
        indexable!(),
        " ORDER BY u.created_at, u.id LIMIT $1 OFFSET $2"
    ))
    .bind(PROFILES_PER_FILE)
    .bind((page - 1) * PROFILES_PER_FILE)
    .fetch_all(&st.db)
    .await?;
    let mut s = String::from(head);
    for h in handles {
        s += &format!("<url><loc>{b}/u/{}</loc></url>\n", esc(&enc(&h)));
    }
    Ok(xml(s + "</urlset>\n"))
}

/// What a signed-out visitor (or a crawler) may learn about `/u/<name>`.
pub enum ProfileSeo {
    /// unknown, test, deleted, banned or leaving: same as a name nobody has
    Missing,
    /// exists but not indexable (private, under age review, public profiles off)
    NoIndex,
    Index {
        handle: String,
        display_name: String,
    },
}

pub async fn profile_seo(st: &AppState, name: &str) -> Result<ProfileSeo, crate::error::AppError> {
    let h = name.trim().trim_start_matches('@');
    if h.is_empty() || h.len() > 64 {
        return Ok(ProfileSeo::Missing);
    }
    let row: Option<(String, String, bool, bool)> = sqlx::query_as(concat!(
        "SELECT u.handle, u.display_name, u.is_test OR u.delete_after IS NOT NULL, ",
        indexable!(),
        " FROM users u
          WHERE (lower(u.handle) = lower($1)
                 OR u.id = (SELECT user_id FROM handle_redirects WHERE old_handle = lower($1)))
            AND u.deleted_at IS NULL AND u.banned_at IS NULL
          ORDER BY lower(u.handle) = lower($1) DESC LIMIT 1"
    ))
    .bind(h)
    .fetch_optional(&st.db)
    .await?;
    Ok(match row {
        None | Some((_, _, true, _)) => ProfileSeo::Missing,
        Some((handle, display_name, false, true))
            if crate::features::on(st, "public_profiles").await =>
        {
            ProfileSeo::Index {
                handle,
                display_name,
            }
        }
        Some(_) => ProfileSeo::NoIndex,
    })
}

/// The tags that replace the page's `<!--seo-->…<!--/seo-->` block.
pub fn profile_head(base: &str, seo: &ProfileSeo) -> String {
    match seo {
        ProfileSeo::Index {
            handle,
            display_name,
        } => {
            let url = format!("{base}/u/{}", enc(handle));
            let title = format!("{display_name} (@{handle}) · Fraglink");
            let desc = format!(
                "{display_name}'s gaming profile on Fraglink: games, hours played and recent activity."
            );
            format!(
                "<title>{t}</title>\n<meta name=\"description\" content=\"{d}\">\n<link rel=\"canonical\" href=\"{u}\">\n\
                 <meta property=\"og:type\" content=\"profile\">\n<meta property=\"og:site_name\" content=\"Fraglink\">\n\
                 <meta property=\"og:title\" content=\"{t}\">\n<meta property=\"og:description\" content=\"{d}\">\n\
                 <meta property=\"og:url\" content=\"{u}\">\n<meta property=\"og:image\" content=\"{base}/assets/og-card.png\">\n\
                 <meta property=\"og:image:width\" content=\"1200\">\n<meta property=\"og:image:height\" content=\"630\">\n\
                 <meta name=\"twitter:card\" content=\"summary_large_image\">",
                t = esc(&title),
                d = esc(&desc),
                u = esc(&url),
            )
        }
        _ => "<title>Profile · Fraglink</title>\n<meta name=\"robots\" content=\"noindex\">\n\
              <meta name=\"description\" content=\"Fraglink: a lightweight gaming tracker for Windows.\">"
            .to_string(),
    }
}

/// `GET /u/<name>`: the website's profile page (`u.html` from the site files) with the right
/// robots header and head tags. The page itself is still drawn by the site's script.
async fn profile_page(State(st): State<AppState>, Path(rest): Path<String>) -> Response {
    let name = rest.split('/').next().unwrap_or("");
    let seo = match profile_seo(&st, name).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };
    let page = match std::fs::read_to_string(st.cfg.site_dir.join("u.html")) {
        Ok(p) => p,
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };
    let head = profile_head(base(&st), &seo);
    let html = match (page.find(SEO_START), page.find(SEO_END)) {
        (Some(a), Some(z)) if a < z => {
            format!("{}{head}{}", &page[..a], &page[z + SEO_END.len()..])
        }
        _ => page,
    };
    let status = match seo {
        ProfileSeo::Missing => StatusCode::NOT_FOUND,
        _ => StatusCode::OK,
    };
    let mut r = (
        status,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        html,
    )
        .into_response();
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    if !matches!(seo, ProfileSeo::Index { .. }) {
        h.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    }
    r
}
