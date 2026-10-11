//! Minimal server-rendered pages (no JavaScript): sign up / sign in, approve a
//! desktop pairing code, link + sync Steam, add Imported/Estimated hours, manage
//! devices, export. The real website (Astro, M4) replaces these.

#![allow(clippy::type_complexity)]

use crate::api::{self, same_origin, GameStat};
use crate::auth::{self, AuthUser, MaybeUser, SESSION_COOKIE};
use crate::error::AppError;
use crate::hours::{self, Era};
use crate::{steam, AppState};
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;
use std::collections::HashMap;
use uuid::Uuid;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(home))
        .route("/signup", get(signup_page).post(signup_post))
        .route("/login", get(login_page).post(login_post))
        .route("/logout", post(logout_post))
        .route("/pair", get(pair_page).post(pair_post))
        .route("/auth/steam/start", get(steam_start))
        .route("/auth/steam/callback", get(steam_callback))
        .route("/web/steam/link", post(web_steam_link))
        .route("/web/steam/sync", post(web_steam_sync))
        .route("/web/estimate", post(web_estimate))
        .route("/web/import", post(web_import))
        .route("/web/hours/{id}/delete", post(web_delete_hours))
        .route("/web/devices/{id}/revoke", post(web_revoke_device))
        .route("/web/friends/add", post(web_friend_add))
        .route("/web/account/delete", post(web_delete_account))
        .route("/web/friends/{handle}/{action}", post(web_friend_action))
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            _ => o.push(c),
        }
    }
    o
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Only same-site relative paths are allowed as redirect targets.
pub fn safe_next(next: Option<&str>) -> String {
    match next {
        Some(n) if n.starts_with('/') && !n.starts_with("//") && !n.contains('\\') => n.to_string(),
        _ => "/".to_string(),
    }
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{t} · Fraglink</title><style>
body{{margin:0;font:14px/1.45 Tahoma,Verdana,sans-serif;background:#0b1830;color:#dfe8f7}}
header{{background:linear-gradient(#1d3a72,#10254d);padding:10px 18px;border-bottom:2px solid #04c1fa}}
header b{{font-size:18px;letter-spacing:1px;color:#fff}} header small{{color:#9fb3d6;margin-left:8px}}
main{{max-width:880px;margin:18px auto;padding:0 16px}} a{{color:#04c1fa}}
.card{{background:#12254a;border:1px solid #274b8a;border-radius:6px;padding:12px 16px;margin:0 0 14px}}
h1{{font-size:20px}} h2{{font-size:15px;margin:0 0 8px;color:#fff}}
input,select{{font:inherit;padding:5px 7px;border-radius:4px;border:1px solid #3a5ea0;background:#0b1830;color:#fff}}
button{{font:inherit;padding:5px 12px;border-radius:4px;border:1px solid #04c1fa;background:#1b4a8a;color:#fff;cursor:pointer}}
table{{width:100%;border-collapse:collapse}} td,th{{padding:4px 6px;border-bottom:1px solid #22406f;text-align:left}}
th{{color:#9fb3d6;font-weight:normal}} td.n{{text-align:right;white-space:nowrap}}
.t-tracked{{color:#4ee07a}} .t-imported{{color:#5fb2ff}} .t-estimated{{color:#c9b36a;text-decoration:underline dotted}}
.msg{{background:#123d2a;border-color:#2e8a5a}} .err{{background:#4a1a1a;border-color:#a33}}
.code{{font:bold 26px monospace;letter-spacing:3px}} small.m{{color:#9fb3d6}} form.inline{{display:inline}}
</style></head><body><header><b>FRAGLINK</b><small>local dev server · M2 preview</small></header><main>{b}</main></body></html>"#,
        t = esc(title),
        b = body
    ))
}

fn session_cookie(state: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if state.cfg.secure_cookies() {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cookie")
}

fn redirect_with_cookie(state: &AppState, to: &str, token: &str) -> Response {
    let mut r = Redirect::to(to).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(state, token, auth::WEB_TOKEN_DAYS * 86400),
    );
    r
}

fn back(msg: &str) -> Response {
    Redirect::to(&format!("/?msg={}", enc(msg))).into_response()
}

fn back_err(e: AppError) -> Response {
    Redirect::to(&format!("/?err={}", enc(&e.message))).into_response()
}

fn tier_mark(tier: &str) -> &'static str {
    match tier {
        "imported" => "◆",
        "estimated" => "~",
        _ => "●",
    }
}

fn hrs(min: i64) -> String {
    format!("{}.{}h", hours::group(min / 60), (min % 60) * 10 / 60)
}

#[derive(Deserialize, Default)]
struct Flash {
    msg: Option<String>,
    err: Option<String>,
}

fn flash(f: &Flash) -> String {
    let mut s = String::new();
    if let Some(m) = &f.msg {
        s += &format!(r#"<div class="card msg">{}</div>"#, esc(m));
    }
    if let Some(e) = &f.err {
        s += &format!(r#"<div class="card err">{}</div>"#, esc(e));
    }
    s
}

async fn home(
    State(st): State<AppState>,
    MaybeUser(u): MaybeUser,
    Query(f): Query<Flash>,
) -> Result<Html<String>, AppError> {
    let Some(a) = u else {
        return Ok(page(
            "Welcome",
            &format!(
                r#"{}<div class="card"><h1>Fraglink sync (preview)</h1><p>Sign in to back up the hours your Fraglink tray app tracks, import your Steam playtime and add older hours you remember. The tray app works fine without an account.</p>
<p><a href="/signup">Create an account</a> · <a href="/login">Sign in</a></p></div>"#,
                flash(&f)
            ),
        ));
    };
    let uid = a.user.id;
    let games = api::game_stats(&st, uid).await?;
    let steam: Option<(String, bool, Option<chrono::DateTime<chrono::Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT provider_uid, verified, last_sync_at, sync_status FROM linked_accounts WHERE user_id = $1 AND provider = 'steam'",
    )
    .bind(uid)
    .fetch_optional(&st.db)
    .await?;
    let mut entries: Vec<(Uuid, String, String, String, i32, Option<String>)> = sqlx::query_as(
        "SELECT h.id, h.game_id, h.tier, h.source, h.minutes, h.note FROM hours_entries h
         WHERE h.user_id = $1 ORDER BY h.created_at DESC",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await?;
    {
        let ids: Vec<String> = entries.iter().map(|e| e.1.clone()).collect();
        let names = crate::names::public_names(&st, &ids).await?;
        for e in &mut entries {
            e.1 = names
                .get(&e.1)
                .cloned()
                .unwrap_or_else(|| crate::names::UNLISTED_TITLE.to_string());
        }
    }
    let devices: Vec<(Uuid, Option<String>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT id, device_name, last_used_at FROM auth_tokens WHERE user_id = $1 AND kind = 'device' AND revoked_at IS NULL ORDER BY created_at DESC",
    )
    .bind(uid)
    .fetch_all(&st.db)
    .await?;

    let mut b = flash(&f);
    b += &format!(
        r#"<div class="card"><h1>Hi {h}</h1><form class="inline" method="post" action="/logout"><button>Sign out</button></form> <a href="/api/v1/export.zip">Export my data (zip)</a> · <a href="/api/v1/export">JSON</a></div>"#,
        h = esc(&a.user.handle)
    );
    // stats
    let verified: i64 = games.iter().map(|g| g.display_min).sum();
    let estimated: i64 = games.iter().map(|g| g.estimated_min).sum();
    b += r#"<div class="card"><h2>Your hours</h2><table><tr><th>Game</th><th class="n">Past 7 days</th><th class="n">Total</th><th class="n">Estimated</th></tr>"#;
    if games.is_empty() {
        b += r#"<tr><td colspan="4"><small class="m">Nothing yet. Pair the tray app, sync Steam or add hours below.</small></td></tr>"#;
    }
    for g in &games {
        let GameStat {
            name,
            display_min,
            display_tier,
            minutes_7d,
            estimated_min,
            ..
        } = g;
        let total = if *display_min > 0 {
            format!(
                r#"<span class="t-{display_tier}">{} {}</span>"#,
                tier_mark(display_tier),
                hrs(*display_min)
            )
        } else {
            "–".into()
        };
        let est = if *estimated_min > 0 {
            format!(
                r#"<span class="t-estimated">~{}h</span>"#,
                hours::group(hours::round_estimate_hours(estimated_min / 60))
            )
        } else {
            String::new()
        };
        b += &format!(
            r#"<tr><td>{}</td><td class="n">{}</td><td class="n">{}</td><td class="n">{}</td></tr>"#,
            esc(name),
            hrs(*minutes_7d),
            total,
            est
        );
    }
    b += &format!(
        r#"</table><p><b>{}</b> verified · <span class="t-estimated">~{}h</span> estimated<br><small class="m"><span class="t-tracked">● Tracked</span> = Fraglink sessions or Steam's own playtime · <span class="t-imported">◆ Imported</span> = a game's own counter you entered (self-reported) · <span class="t-estimated">~ Estimated</span> = your history estimate, never used for ranks.</small></p></div>"#,
        hrs(verified),
        hours::group(hours::round_estimate_hours(estimated / 60))
    );
    // steam
    b += r#"<div class="card"><h2>Steam</h2>"#;
    match &steam {
        None => {
            b += r#"<p><a href="/auth/steam/start"><button type="button">Sign in through Steam</button></a> <small class="m">(verified link)</small></p>
<form method="post" action="/web/steam/link">or paste your profile: <input name="profile" placeholder="steamcommunity.com/id/you or SteamID64" size="36"> <button>Link</button></form>"#;
        }
        Some((sid, verified, last, status)) => {
            b += &format!(
                "<p>Linked: <a href=\"https://steamcommunity.com/profiles/{sid}\">{sid}</a> {} · last sync: {} {}</p>",
                if *verified { "<span class=\"t-tracked\">✓ Verified</span>" } else { "(unverified: <a href=\"/auth/steam/start\">verify by signing in through Steam</a>)" },
                last.map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()).unwrap_or_else(|| "never".into()),
                match status.as_deref() { Some("private") => "· <b>game details are private</b>", _ => "" },
                sid = esc(sid)
            );
            b += r#"<form method="post" action="/web/steam/sync"><button>Import Steam playtime</button></form>"#;
        }
    }
    if !st.steam.configured() {
        b += r#"<p><small class="m">This server has no STEAM_API_KEY, so importing is switched off.</small></p>"#;
    }
    b += &format!(
        r#"<p><small class="m">{}</small></p></div>"#,
        esc(steam::PRIVATE_HELP)
    );
    // estimate + import
    b += r#"<div class="card"><h2>Add hours from before Fraglink</h2>
<form method="post" action="/web/estimate"><b>Estimate</b> (~): <input name="game_name" placeholder="Game, e.g. World of Warcraft" required>
from <input name="from_year" type="number" min="1970" max="2100" value="2005" style="width:5em"> to <input name="to_year" type="number" min="1970" max="2100" value="2008" style="width:5em">
at <input name="hours_per_week" type="number" step="0.5" min="0.5" max="80" value="10" style="width:4em"> h/week <button>Save estimate</button></form>
<form method="post" action="/web/import" style="margin-top:8px"><b>Game counter</b> (◆, e.g. /played): <input name="game_name" placeholder="Game" required> <input name="hours" type="number" step="0.1" min="0.1" placeholder="hours" style="width:6em" required>
<select name="source"><option value="manual">typed in</option><option value="wow_manual">WoW /played</option><option value="xbox_recap">Xbox recap</option></select> <button>Save</button></form>"#;
    if !entries.is_empty() {
        b += "<table style=\"margin-top:10px\">";
        for (id, name, tier, source, minutes, note) in &entries {
            b += &format!(
                r#"<tr><td>{}</td><td class="t-{tier}">{} {}</td><td class="n">{}</td><td>{}</td><td><form class="inline" method="post" action="/web/hours/{id}/delete"><button>Delete</button></form></td></tr>"#,
                esc(name),
                tier_mark(tier),
                esc(tier),
                hrs(*minutes as i64),
                esc(&format!(
                    "{source}{}",
                    note.as_ref().map(|n| format!(" · {n}")).unwrap_or_default()
                )),
            );
        }
        b += "</table>";
    }
    b += "</div>";
    // friends (M3)
    let fl = crate::friends::list(&st, uid).await?;
    b += r#"<div class="card"><h2>Friends</h2><form method="post" action="/web/friends/add">Add by name: <input name="handle" required maxlength="20" placeholder="their Fraglink name"> <button>Send request</button></form>"#;
    for r in &fl.incoming {
        let h = esc(&r.handle);
        b += &format!(
            r#"<div><b>{h}</b> wants to be friends <form class="inline" method="post" action="/web/friends/{h}/accept"><button>Accept</button></form> <form class="inline" method="post" action="/web/friends/{h}/decline"><button>Decline</button></form> <form class="inline" method="post" action="/web/friends/{h}/block"><button>Block</button></form></div>"#
        );
    }
    for r in &fl.outgoing {
        let h = esc(&r.handle);
        b += &format!(
            r#"<div><small class="m">Request sent to {h}</small> <form class="inline" method="post" action="/web/friends/{h}/remove"><button>Cancel</button></form></div>"#
        );
    }
    b += "<table>";
    for f in &fl.friends {
        let h = esc(&f.handle);
        let p = &f.presence;
        let state = match (p.status.as_str(), &p.game_name) {
            ("offline", _) => r#"<small class="m">Offline</small>"#.to_string(),
            (_, Some(g)) => format!(r#"<span class="t-tracked">Playing {}</span>"#, esc(g)),
            ("away", None) => "Away".to_string(),
            _ => "Online".to_string(),
        };
        b += &format!(
            r#"<tr><td>{h}</td><td>{state}</td><td class="n"><form class="inline" method="post" action="/web/friends/{h}/remove"><button>Remove</button></form> <form class="inline" method="post" action="/web/friends/{h}/block"><button>Block</button></form></td></tr>"#
        );
    }
    b += "</table>";
    if fl.friends.is_empty() {
        b += r#"<small class="m">No friends yet. Ask them for their Fraglink name.</small>"#;
    }
    for h in &fl.blocked {
        let h = esc(h);
        b += &format!(
            r#"<div><small class="m">Blocked: {h}</small> <form class="inline" method="post" action="/web/friends/{h}/unblock"><button>Unblock</button></form></div>"#
        );
    }
    b += "</div>";
    // devices
    b += r#"<div class="card"><h2>Paired Fraglink apps</h2>"#;
    if devices.is_empty() {
        b += r#"<small class="m">None yet. In the tray app: Tools → Options → Sign in to sync.</small>"#;
    }
    for (id, name, last) in &devices {
        b += &format!(
            r#"<div>{} <small class="m">last seen {}</small> <form class="inline" method="post" action="/web/devices/{id}/revoke"><button>Sign out this device</button></form></div>"#,
            esc(name.as_deref().unwrap_or("Fraglink desktop")),
            last.format("%Y-%m-%d %H:%M UTC")
        );
    }
    b += "</div>";
    b += r#"<div class="card"><h2>Delete account</h2><p><small class="m">Deletes your account, sessions, hours, Steam link, devices, friends and blocks right away. This can't be undone, so export first if you want a copy. The tray keeps its local data.</small></p>
<form method="post" action="/web/account/delete"><label>Password<br><input name="password" type="password" required autocomplete="current-password"></label> <label><input type="checkbox" name="confirm" value="yes" required> I understand</label> <button>Delete my account</button></form></div>"#;
    Ok(page("Your hours", &b))
}

#[derive(Deserialize)]
struct DeleteForm {
    password: String,
    #[serde(default)]
    confirm: String,
}

async fn web_delete_account(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<DeleteForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    if f.confirm != "yes" {
        return back_err(AppError::bad_request("confirm", "Tick the box to confirm."));
    }
    if let Err(e) = auth::check_login(&st, &a.user.handle, &f.password).await {
        return back_err(e);
    }
    if let Err(e) = sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(a.user.id)
        .execute(&st.db)
        .await
    {
        return back_err(e.into());
    }
    let mut r = Redirect::to("/?msg=Your%20account%20was%20deleted.").into_response();
    r.headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&st, "", 0));
    r
}

#[derive(Deserialize, Default)]
struct NextQ {
    next: Option<String>,
    err: Option<String>,
}

fn auth_form(kind: &str, next: &str, err: Option<&str>) -> String {
    let (title, action, extra, alt) = if kind == "signup" {
        (
            "Create your Fraglink account",
            "/signup",
            r#"<p><label>Handle<br><input name="handle" required minlength="3" maxlength="20" pattern="[A-Za-z0-9_\-]+"></label></p>"#,
            format!(
                r#"Already have an account? <a href="/login?next={}">Sign in</a>"#,
                enc(next)
            ),
        )
    } else {
        (
            "Sign in",
            "/login",
            "",
            format!(
                r#"New here? <a href="/signup?next={}">Create an account</a>"#,
                enc(next)
            ),
        )
    };
    format!(
        r#"{e}<div class="card"><h1>{title}</h1><form method="post" action="{action}"><input type="hidden" name="next" value="{n}">
<p><label>Email<br><input name="email" type="email" required autocomplete="email"></label></p>{extra}
<p><label>Password<br><input name="password" type="password" required minlength="{min}" autocomplete="{ac}"></label></p><p><button>{title}</button></p></form><p>{alt}</p></div>"#,
        e = err
            .map(|e| format!(r#"<div class="card err">{}</div>"#, esc(e)))
            .unwrap_or_default(),
        n = esc(next),
        min = if kind == "signup" { 10 } else { 1 },
        ac = if kind == "signup" {
            "new-password"
        } else {
            "current-password"
        },
    )
}

async fn signup_page(Query(q): Query<NextQ>) -> Html<String> {
    page(
        "Sign up",
        &auth_form("signup", &safe_next(q.next.as_deref()), q.err.as_deref()),
    )
}

async fn login_page(Query(q): Query<NextQ>) -> Html<String> {
    page(
        "Sign in",
        &auth_form("login", &safe_next(q.next.as_deref()), q.err.as_deref()),
    )
}

#[derive(Deserialize)]
struct AuthFormData {
    email: String,
    password: String,
    handle: Option<String>,
    next: Option<String>,
}

async fn signup_post(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<AuthFormData>,
) -> Response {
    let next = safe_next(f.next.as_deref());
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let res = async {
        crate::api::signup_limit(&st, &headers)?;
        let u = auth::create_user(
            &st,
            &f.email,
            &f.password,
            f.handle.as_deref().unwrap_or(""),
        )
        .await?;
        auth::issue_token(&st, u.id, "web", None).await
    }
    .await;
    match res {
        Ok(t) => redirect_with_cookie(&st, &next, &t),
        Err(e) => Redirect::to(&format!(
            "/signup?next={}&err={}",
            enc(&next),
            enc(&e.message)
        ))
        .into_response(),
    }
}

async fn login_post(
    State(st): State<AppState>,
    headers: HeaderMap,
    Form(f): Form<AuthFormData>,
) -> Response {
    let next = safe_next(f.next.as_deref());
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let res = async {
        crate::api::login_limit(&st, &headers)?;
        let u = auth::check_login(&st, &f.email, &f.password).await?;
        if crate::mfa::enabled(&st, u.id).await? {
            return Err(AppError::bad_request(
                "mfa_required",
                "This account uses two-step sign-in. Sign in on the Fraglink website or app instead.",
            ));
        }
        auth::issue_token(&st, u.id, "web", None).await
    }
    .await;
    match res {
        Ok(t) => redirect_with_cookie(&st, &next, &t),
        Err(e) => Redirect::to(&format!(
            "/login?next={}&err={}",
            enc(&next),
            enc(&e.message)
        ))
        .into_response(),
    }
}

async fn logout_post(
    State(st): State<AppState>,
    headers: HeaderMap,
    MaybeUser(u): MaybeUser,
) -> Response {
    if let Some(a) = u {
        if same_origin(&headers, &st).is_ok() {
            let _ = sqlx::query("UPDATE auth_tokens SET revoked_at = now() WHERE id = $1")
                .bind(a.token_id)
                .execute(&st.db)
                .await;
        }
    }
    let mut r = Redirect::to("/").into_response();
    r.headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&st, "", 0));
    r
}

#[derive(Deserialize, Default)]
struct PairQ {
    code: Option<String>,
}

async fn pair_page(MaybeUser(u): MaybeUser, Query(q): Query<PairQ>) -> Response {
    let code = q
        .code
        .map(|c| auth::normalize_user_code(&c))
        .unwrap_or_default();
    let Some(a) = u else {
        let next = if code.is_empty() {
            "/pair".to_string()
        } else {
            format!("/pair?code={code}")
        };
        return Redirect::to(&format!("/login?next={}", enc(&next))).into_response();
    };
    page(
        "Pair the Fraglink app",
        &format!(
            r#"<div class="card"><h1>Connect the Fraglink app</h1><p>Signed in as <b>{h}</b>. Check that this code matches the one shown in the Fraglink app, then approve.</p>
<form method="post" action="/pair"><p><input name="code" class="code" value="{c}" size="10" maxlength="9" required></p><p><button>Approve this device</button></p></form>
<p><small class="m">Only approve codes you started yourself. The app will then upload the game sessions it tracks (game, start/end, active time). Nothing else leaves your PC.</small></p></div>"#,
            h = esc(&a.user.handle),
            c = esc(&code)
        ),
    )
    .into_response()
}

#[derive(Deserialize)]
struct PairForm {
    code: String,
}

async fn pair_post(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<PairForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    match api::approve_code(&st, a.user.id, &f.code).await {
        Ok(name) => page(
            "Paired",
            &format!(r#"<div class="card msg"><h1>Done!</h1><p><b>{}</b> is now connected. You can close this tab and go back to Fraglink.</p></div><p><a href="/">See your hours</a></p>"#, esc(&name)),
        )
        .into_response(),
        Err(e) => page("Pair", &format!(r#"<div class="card err">{}</div><p><a href="/pair">Try again</a></p>"#, esc(&e.message))).into_response(),
    }
}

async fn steam_start(State(st): State<AppState>, MaybeUser(u): MaybeUser) -> Response {
    if u.is_none() {
        return Redirect::to("/login?next=%2Fauth%2Fsteam%2Fstart").into_response();
    }
    Redirect::to(&st.steam.openid_redirect(&st.cfg.public_url)).into_response()
}

async fn steam_callback(
    State(st): State<AppState>,
    a: AuthUser,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if q.get("openid.mode").map(String::as_str) == Some("cancel") {
        return back("Steam sign-in was cancelled.");
    }
    match st.steam.verify_openid(&st.cfg.public_url, &q).await {
        Ok(id) => match steam::link(&st, a.user.id, id, true).await {
            Ok(()) => back("Steam account verified and linked. Press \"Import Steam playtime\" to pull your hours."),
            Err(e) => back_err(e),
        },
        Err(e) => back_err(e),
    }
}

#[derive(Deserialize)]
struct LinkForm {
    profile: String,
}

async fn web_steam_link(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<LinkForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let r = async {
        let id = match steam::parse_profile(&f.profile) {
            Some(steam::ProfileRef::Id(id)) => id,
            Some(steam::ProfileRef::Vanity(v)) => st.steam.resolve_vanity(&v).await?,
            None => {
                return Err(AppError::bad_request(
                    "invalid_steam_profile",
                    "Paste your SteamID64 or steamcommunity.com profile link.",
                ))
            }
        };
        steam::link(&st, a.user.id, id, false).await
    }
    .await;
    match r {
        Ok(()) => back("Steam profile linked (unverified)."),
        Err(e) => back_err(e),
    }
}

async fn web_steam_sync(State(st): State<AppState>, headers: HeaderMap, a: AuthUser) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    match steam::import(&st, a.user.id).await {
        Ok(r) => back(&format!(
            "Imported {} Steam games ({} hours).",
            r.games,
            hours::group(r.minutes_total / 60)
        )),
        Err(e) => back_err(e),
    }
}

#[derive(Deserialize)]
struct EstimateForm {
    game_name: String,
    from_year: i32,
    to_year: i32,
    hours_per_week: f64,
}

async fn web_estimate(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<EstimateForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let req = api::EstimateReq {
        game_id: api::known_game_by_name(&st, &f.game_name),
        game_name: Some(f.game_name.clone()),
        eras: vec![Era {
            from_year: f.from_year,
            to_year: f.to_year,
            hours_per_week: f.hours_per_week,
        }],
        note: None,
    };
    match api::estimate_for(&st, a.user.id, req).await {
        Ok(v) => {
            let mut m = format!(
                "Saved estimate: {} for {}.",
                v["display"].as_str().unwrap_or(""),
                f.game_name.trim()
            );
            if let Some(w) = v["warnings"].as_array().filter(|w| !w.is_empty()) {
                m += &format!(
                    " Note: {}",
                    w.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                );
            }
            back(&m)
        }
        Err(e) => back_err(e),
    }
}

#[derive(Deserialize)]
struct ImportForm {
    game_name: String,
    hours: f64,
    source: String,
}

async fn web_import(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<ImportForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let req = api::ImportReq {
        game_id: api::known_game_by_name(&st, &f.game_name),
        game_name: Some(f.game_name.clone()),
        minutes: (f.hours * 60.0).round() as i64,
        source: f.source,
        note: None,
    };
    match api::import_for(&st, a.user.id, req).await {
        Ok(v) => back(&format!(
            "Saved {} for {} (self-reported).",
            hrs(v["minutes"].as_i64().unwrap_or(0)),
            f.game_name.trim()
        )),
        Err(e) => back_err(e),
    }
}

/// Typed game names that match a known game (case-insensitive) use its id.
async fn web_delete_hours(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    match sqlx::query("DELETE FROM hours_entries WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(a.user.id)
        .execute(&st.db)
        .await
    {
        Ok(_) => back("Deleted."),
        Err(e) => back_err(e.into()),
    }
}

async fn web_revoke_device(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path(id): Path<Uuid>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    match sqlx::query("UPDATE auth_tokens SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND kind = 'device'")
        .bind(id)
        .bind(a.user.id)
        .execute(&st.db)
        .await
    {
        Ok(_) => back("Device signed out. It keeps tracking locally but stops syncing."),
        Err(e) => back_err(e.into()),
    }
}

#[allow(dead_code)]
fn status_page(code: StatusCode, msg: &str) -> Response {
    (
        code,
        page(
            "Error",
            &format!(r#"<div class="card err">{}</div>"#, esc(msg)),
        ),
    )
        .into_response()
}

#[derive(Deserialize)]
struct HandleForm {
    handle: String,
}

async fn web_friend_add(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Form(f): Form<HandleForm>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    match crate::friends::request(&st, &api::person(&a), &f.handle).await {
        Ok(crate::friends::RequestOutcome::Accepted) => {
            back(&format!("You and {} are now friends.", f.handle.trim()))
        }
        Ok(_) => back(&format!("Friend request sent to {}.", f.handle.trim())),
        Err(e) => back_err(e),
    }
}

async fn web_friend_action(
    State(st): State<AppState>,
    headers: HeaderMap,
    a: AuthUser,
    Path((handle, action)): Path<(String, String)>,
) -> Response {
    if let Err(e) = same_origin(&headers, &st) {
        return e.into_response();
    }
    let me = api::person(&a);
    let r = match action.as_str() {
        "accept" => crate::friends::respond(&st, &me, &handle, true)
            .await
            .map(|_| "Friend added."),
        "decline" => crate::friends::respond(&st, &me, &handle, false)
            .await
            .map(|_| "Request declined."),
        "remove" => crate::friends::remove(&st, &me, &handle)
            .await
            .map(|_| "Removed."),
        "block" => crate::friends::block(&st, &me, &handle)
            .await
            .map(|_| "Blocked. They can't see you or send you requests."),
        "unblock" => crate::friends::unblock(&st, &me, &handle)
            .await
            .map(|_| "Unblocked."),
        _ => return back_err(AppError::not_found()),
    };
    match r {
        Ok(m) => back(m),
        Err(e) => back_err(e),
    }
}
