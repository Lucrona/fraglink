#![allow(dead_code)]
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use fraglink_server::{AppState, Config};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;

pub const ORIGIN: &str = "http://localhost:8787";

pub fn state(pool: PgPool) -> AppState {
    AppState::new(pool, Config::for_tests("postgres://unused"))
}

pub fn state_with(pool: PgPool, f: impl FnOnce(&mut Config)) -> AppState {
    let mut c = Config::for_tests("postgres://unused");
    f(&mut c);
    AppState::new(pool, c)
}

pub struct Resp {
    pub status: StatusCode,
    pub headers: axum::http::HeaderMap,
    pub text: String,
    pub bytes: Vec<u8>,
}

impl Resp {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.text).unwrap_or(Value::Null)
    }
    pub fn location(&self) -> String {
        self.headers
            .get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string())
            .unwrap_or_default()
    }
    pub fn cookie(&self) -> Option<String> {
        let c = self.headers.get(header::SET_COOKIE)?.to_str().ok()?;
        let v = c.split(';').next()?.strip_prefix("fl_session=")?;
        (!v.is_empty()).then(|| v.to_string())
    }
}

pub async fn send(app: &Router, req: Request<Body>) -> Resp {
    let r = app.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    Resp {
        status,
        headers,
        text: String::from_utf8_lossy(&bytes).into_owned(),
        bytes: bytes.to_vec(),
    }
}

/// JSON API call with an optional bearer token.
pub async fn api(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> Resp {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let req = match body {
        Some(v) => b
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    send(app, req).await
}

/// Browser-style request: cookie + form body + Origin header.
pub async fn form(
    app: &Router,
    path: &str,
    cookie: Option<&str>,
    origin: Option<&str>,
    body: &str,
) -> Resp {
    let mut b = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, format!("fl_session={c}"));
    }
    if let Some(o) = origin {
        b = b.header("origin", o);
    }
    send(app, b.body(Body::from(body.to_string())).unwrap()).await
}

pub async fn page(app: &Router, path: &str, cookie: Option<&str>) -> Resp {
    let mut b = Request::builder().uri(path);
    if let Some(c) = cookie {
        b = b.header(header::COOKIE, format!("fl_session={c}"));
    }
    send(app, b.body(Body::empty()).unwrap()).await
}

pub async fn signup(app: &Router, email: &str, handle: &str) -> String {
    let r = api(
        app,
        "POST",
        "/api/v1/auth/signup",
        None,
        Some(json!({ "email": email, "password": "correct horse battery", "handle": handle, "birth_date": "1990-01-15" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    r.json()["token"].as_str().unwrap().to_string()
}

/// Full device-code pairing; returns the device token.
pub async fn pair(app: &Router, web_token: &str) -> String {
    let s = api(
        app,
        "POST",
        "/api/v1/device/start",
        None,
        Some(json!({ "device_name": "Test PC" })),
    )
    .await
    .json();
    let r = api(
        app,
        "POST",
        "/api/v1/device/approve",
        Some(web_token),
        Some(json!({ "user_code": s["user_code"] })),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let p = api(
        app,
        "POST",
        "/api/v1/device/poll",
        None,
        Some(json!({ "device_code": s["device_code"] })),
    )
    .await
    .json();
    assert_eq!(p["status"], "approved");
    p["token"].as_str().unwrap().to_string()
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn session(id: &str, game: &str, start: i64, secs: i64) -> Value {
    json!({ "id": id, "game_id": game, "started_at": start, "ended_at": start + secs, "duration_s": secs, "idle_trimmed_s": 0 })
}

pub fn batch(games: Value, sessions: Vec<Value>) -> Value {
    json!({ "client_version": "0.2.0-test", "games": games, "sessions": sessions })
}

pub fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}
