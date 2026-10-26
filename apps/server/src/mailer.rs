//! Outgoing email behind one small interface: account emails (confirm your email, password
//! reset, new sign-in alert). Mail goes out only when `FRAGLINK_MAIL=on` AND a provider is
//! configured; otherwise every message is only written to the log (and kept in memory for
//! tests). Nothing that depends on email may block a user while mail is off.
//!
//! Providers (HTTPS APIs, no SMTP), picked by `FRAGLINK_MAIL_PROVIDER`:
//! - `resend` (default when `RESEND_API_KEY` is set): `RESEND_API_KEY`.
//! - `cloudflare` (Cloudflare Email Service, needs Workers Paid): `CLOUDFLARE_EMAIL_TOKEN`
//!   (API token with "Email Sending: Edit") + `CLOUDFLARE_ACCOUNT_ID`.
//!
//! `FRAGLINK_MAIL_FROM` (default `Fraglink <no-reply@playfraglink.com>`) and
//! `FRAGLINK_MAIL_REPLY_TO` (default `support@playfraglink.com`).
//! Sending runs in the background; failures are logged, never shown to the user.

use crate::AppState;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub body: String,
    /// HTML version (sent alongside the plain text), if any
    pub html: Option<String>,
    /// Reply-To override (the contact form sets the sender's address); default: support.
    pub reply_to: Option<String>,
}

#[derive(Debug, Clone)]
enum Provider {
    Resend { key: String },
    Cloudflare { token: String, account: String },
}

struct Settings {
    provider: Option<Provider>,
    from: String,
    reply_to: String,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn settings() -> &'static Settings {
    static S: OnceLock<Settings> = OnceLock::new();
    S.get_or_init(|| {
        let resend = env("RESEND_API_KEY").map(|key| Provider::Resend { key });
        let cloudflare = match (env("CLOUDFLARE_EMAIL_TOKEN"), env("CLOUDFLARE_ACCOUNT_ID")) {
            (Some(token), Some(account)) => Some(Provider::Cloudflare { token, account }),
            _ => None,
        };
        let provider = match env("FRAGLINK_MAIL_PROVIDER").as_deref() {
            Some("cloudflare") => cloudflare,
            Some("resend") => resend,
            _ => resend.or(cloudflare),
        };
        Settings {
            provider,
            from: env("FRAGLINK_MAIL_FROM")
                .unwrap_or_else(|| "Fraglink <no-reply@playfraglink.com>".into()),
            reply_to: env("FRAGLINK_MAIL_REPLY_TO")
                .unwrap_or_else(|| "support@playfraglink.com".into()),
        }
    })
}

/// Last messages "sent" (newest last, at most 50); tests read this.
pub fn outbox() -> &'static Mutex<Vec<Mail>> {
    static O: OnceLock<Mutex<Vec<Mail>>> = OnceLock::new();
    O.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn send(st: &AppState, to: &str, subject: &str, body: &str) {
    deliver_mail(st, to, subject, body, None);
}

/// A templated email (`crate::emails`): plain text + HTML.
pub fn send_email(st: &AppState, to: &str, e: &crate::emails::Email) {
    deliver_mail(st, to, &e.subject, &e.text, Some(e.html.clone()));
}

/// Send now and report the result (admin alerts use this to log and cap what they send).
/// Err when mail is off or no provider is set, without sending.
pub async fn send_now(st: &AppState, to: &str, e: &crate::emails::Email) -> anyhow::Result<()> {
    if !st.cfg.mail_enabled {
        anyhow::bail!("mail is off (FRAGLINK_MAIL)");
    }
    let Some(p) = settings().provider.clone() else {
        anyhow::bail!("no email provider configured");
    };
    let m = Mail {
        to: to.to_string(),
        subject: e.subject.clone(),
        body: e.text.clone(),
        html: Some(e.html.clone()),
        reply_to: None,
    };
    deliver(&p, &m).await
}

fn deliver_mail(st: &AppState, to: &str, subject: &str, body: &str, html: Option<String>) {
    deliver_mail_with(st, to, subject, body, html, None)
}

/// Plain-text mail with its own Reply-To (the website contact form).
pub fn send_reply_to(st: &AppState, to: &str, subject: &str, body: &str, reply_to: &str) {
    deliver_mail_with(st, to, subject, body, None, Some(reply_to.to_string()))
}

fn deliver_mail_with(
    st: &AppState,
    to: &str,
    subject: &str,
    body: &str,
    html: Option<String>,
    reply_to: Option<String>,
) {
    if to.trim().is_empty() {
        return;
    }
    let m = Mail {
        to: to.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
        html,
        reply_to,
    };
    {
        let mut o = outbox().lock().unwrap();
        o.push(m.clone());
        let n = o.len();
        if n > 50 {
            o.drain(..n - 50);
        }
    }
    if !st.cfg.mail_enabled {
        tracing::info!(%subject, "email (mail off, logged only)");
        tracing::debug!(to = %to, body = %body, "email body");
        return;
    }
    let Some(p) = settings().provider.clone() else {
        tracing::warn!(%subject, "FRAGLINK_MAIL=on but no email provider key is set; not sent");
        return;
    };
    match tokio::runtime::Handle::try_current() {
        Ok(h) => {
            h.spawn(async move {
                if let Err(e) = deliver(&p, &m).await {
                    tracing::warn!(subject = %m.subject, error = %e, "email not sent");
                } else {
                    tracing::info!(subject = %m.subject, "email sent");
                }
            });
        }
        Err(_) => tracing::warn!(%subject, "no async runtime; email not sent"),
    }
}

async fn deliver(p: &Provider, m: &Mail) -> anyhow::Result<()> {
    let s = settings();
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?;
    let req = match p {
        Provider::Resend { key } => {
            let mut b = serde_json::json!({
                "from": s.from, "to": [m.to], "reply_to": m.reply_to.as_ref().unwrap_or(&s.reply_to),
                "subject": m.subject, "text": m.body,
            });
            if let Some(h) = &m.html {
                b["html"] = h.clone().into();
            }
            http.post("https://api.resend.com/emails")
                .bearer_auth(key)
                .json(&b)
        }
        Provider::Cloudflare { token, account } => {
            let mut b = serde_json::json!({
                "from": cf_address(&s.from), "to": m.to, "reply_to": m.reply_to.as_ref().unwrap_or(&s.reply_to),
                "subject": m.subject, "text": m.body,
            });
            if let Some(h) = &m.html {
                b["html"] = h.clone().into();
            }
            http.post(format!(
                "https://api.cloudflare.com/client/v4/accounts/{account}/email/sending/send"
            ))
            .bearer_auth(token)
            .json(&b)
        }
    };
    let r = req.send().await?;
    let status = r.status();
    if !status.is_success() {
        // provider error text only (never the key); cut short
        let t: String = r
            .text()
            .await
            .unwrap_or_default()
            .chars()
            .take(300)
            .collect();
        anyhow::bail!("{status}: {t}");
    }
    Ok(())
}

/// `Name <addr>` as Cloudflare's `{address, name}` object; a bare address stays a string.
fn cf_address(from: &str) -> serde_json::Value {
    match from.rsplit_once('<') {
        Some((name, rest)) if rest.ends_with('>') => serde_json::json!({
            "address": rest.trim_end_matches('>').trim(),
            "name": name.trim().trim_matches('"'),
        }),
        _ => serde_json::Value::String(from.trim().to_string()),
    }
}

/// Email-dependent rules (like "verify before uploading an avatar") only apply once
/// mail is actually being delivered.
pub fn live(st: &AppState) -> bool {
    st.cfg.mail_enabled && settings().provider.is_some()
}

#[cfg(test)]
mod tests {
    #[test]
    fn cf_address_parses_names() {
        let v = super::cf_address("Fraglink <no-reply@playfraglink.com>");
        assert_eq!(v["address"], "no-reply@playfraglink.com");
        assert_eq!(v["name"], "Fraglink");
        assert_eq!(super::cf_address("a@b.c"), "a@b.c");
    }
}
