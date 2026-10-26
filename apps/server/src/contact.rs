//! Website contact form (`/contact`): `POST /api/v1/contact {name, email, topic, message,
//! turnstile}`. Same Turnstile bot check as sign-in, plus rate limits per IP (3 an hour)
//! and per sender address (3 a day). Sends one plain-text mail to support
//! (`FRAGLINK_CONTACT_TO`, default support@playfraglink.com) with Reply-To set to the sender.

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/v1/contact", post(send))
}

pub const TOPICS: [&str; 5] = [
    "Help with the app",
    "Account",
    "Bug report",
    "Privacy",
    "Other",
];

#[derive(Deserialize)]
pub struct ContactReq {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub topic: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub turnstile: Option<String>,
}

fn one_line(s: &str, max: usize) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Checked, cleaned fields or a message for the visitor.
pub fn validate(r: &ContactReq) -> Result<(String, String, String, String), &'static str> {
    let name = one_line(&r.name, 60);
    let email = r.email.trim().to_string();
    let topic = one_line(&r.topic, 40);
    let message: String = r
        .message
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect::<String>()
        .trim()
        .to_string();
    if name.is_empty() {
        return Err("Please add your name.");
    }
    if !crate::alerts::valid_email(&email) {
        return Err("Please enter a valid email address so we can reply.");
    }
    if !TOPICS.contains(&topic.as_str()) {
        return Err("Please pick a topic.");
    }
    if message.chars().count() < 10 {
        return Err("Please write a little more (at least 10 characters).");
    }
    if message.chars().count() > 4000 {
        return Err("Please keep it under 4,000 characters.");
    }
    Ok((name, email, topic, message))
}

pub fn to_address() -> String {
    std::env::var("FRAGLINK_CONTACT_TO")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "support@playfraglink.com".into())
}

async fn send(
    State(st): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ContactReq>,
) -> ApiResult<Json<Value>> {
    let (name, email, topic, message) =
        validate(&r).map_err(|m| AppError::bad_request("invalid_contact", m))?;
    crate::turnstile::check(&st, &headers, r.turnstile.as_deref()).await?;
    let ip = crate::client_ip(&headers);
    st.rate_limit(format!("contact:{ip}"), 3, Duration::from_secs(3600))?;
    st.rate_limit(
        format!("contact-email:{}", email.to_lowercase()),
        3,
        Duration::from_secs(24 * 3600),
    )?;
    let subject = format!("[Contact] {topic}: {name}");
    let body = format!(
        "From: {name} <{email}>\nTopic: {topic}\n\n{message}\n\n--\nSent with the contact form on playfraglink.com. Reply to answer them."
    );
    crate::mailer::send_reply_to(&st, &to_address(), &subject, &body, &email);
    tracing::info!(%topic, "contact form message");
    Ok(Json(json!({ "ok": true })))
}
