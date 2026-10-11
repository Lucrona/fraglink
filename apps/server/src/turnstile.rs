//! Cloudflare Turnstile bot check for the website's sign-up, sign-in and forgot-password
//! forms (server-side `siteverify`). Off when `TURNSTILE_SECRET` isn't set. The desktop
//! app's endpoints don't use it (they have their own rate limits).

use crate::error::{ApiResult, AppError};
use crate::AppState;
use axum::http::{HeaderMap, StatusCode};
use serde_json::Value;

pub async fn check(st: &AppState, headers: &HeaderMap, token: Option<&str>) -> ApiResult<()> {
    let Some(secret) = st.cfg.turnstile_secret.as_deref() else {
        return Ok(());
    };
    let fail = || {
        AppError::new(
            StatusCode::BAD_REQUEST,
            "bot_check",
            "Please complete the \"I'm human\" check and try again.",
        )
    };
    let token = token
        .map(str::trim)
        .filter(|t| !t.is_empty() && t.len() <= 2048)
        .ok_or_else(fail)?;
    let ip = crate::client_ip(headers);
    let mut form = vec![
        ("secret", secret.to_string()),
        ("response", token.to_string()),
    ];
    if ip != "local" {
        form.push(("remoteip", ip));
    }
    let res = reqwest::Client::new()
        .post(&st.cfg.turnstile_verify_url)
        .timeout(std::time::Duration::from_secs(8))
        .form(&form)
        .send()
        .await;
    let ok = match res {
        Ok(r) => r
            .json::<Value>()
            .await
            .ok()
            .and_then(|v| v.get("success").and_then(Value::as_bool))
            .unwrap_or(false),
        Err(e) => {
            // Cloudflare unreachable: don't lock everyone out; the rate limits still apply.
            tracing::warn!(error = %e, "turnstile siteverify unreachable; allowing");
            true
        }
    };
    if ok {
        Ok(())
    } else {
        Err(fail())
    }
}
