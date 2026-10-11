//! Outgoing email behind one small interface. There's no provider yet, so unless
//! `FRAGLINK_MAIL=on` every message is only written to the log (and kept in memory for
//! tests). Nothing that depends on email may block a user while mail is off.

use crate::AppState;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct Mail {
    pub to: String,
    pub subject: String,
    pub body: String,
}

/// Last messages "sent" (newest last, at most 50); tests read this.
pub fn outbox() -> &'static Mutex<Vec<Mail>> {
    static O: std::sync::OnceLock<Mutex<Vec<Mail>>> = std::sync::OnceLock::new();
    O.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn send(st: &AppState, to: &str, subject: &str, body: &str) {
    if to.trim().is_empty() {
        return;
    }
    let m = Mail {
        to: to.to_string(),
        subject: subject.to_string(),
        body: body.to_string(),
    };
    {
        let mut o = outbox().lock().unwrap();
        o.push(m);
        let n = o.len();
        if n > 50 {
            o.drain(..n - 50);
        }
    }
    if st.cfg.mail_enabled {
        // Provider not wired yet (Resend pending): log loudly so it's noticed.
        tracing::warn!(%subject, "FRAGLINK_MAIL=on but no email provider is configured; not sent");
    } else {
        tracing::info!(%subject, "email (mail off, logged only)");
        tracing::debug!(to = %to, body = %body, "email body");
    }
}

/// Email-dependent rules (like "verify before uploading an avatar") only apply once
/// mail is actually being delivered.
pub fn live(st: &AppState) -> bool {
    st.cfg.mail_enabled
}
