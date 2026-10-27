//! Steam: link a profile (typed SteamID/URL = unverified, Steam OpenID sign-in =
//! verified) and import official playtime with `IPlayerService/GetOwnedGames`.
//! The Web API key comes from `STEAM_API_KEY` and never leaves the server
//! (errors are logged without URLs so the key can't leak into logs).

use crate::error::{ApiResult, AppError};
use crate::{AppState, Config};
use axum::http::StatusCode;
use fraglink_core::signatures::SignatureDb;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;
use uuid::Uuid;

pub const OPENID_NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
/// Steam asks API users to keep usage reasonable; one import per 10 minutes per user.
pub const SYNC_COOLDOWN_SECS: i64 = 600;

#[derive(Clone)]
pub struct SteamClient {
    http: reqwest::Client,
    key: Option<String>,
    api_base: String,
    pub openid_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileRef {
    Id(u64),
    Vanity(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedGame {
    pub appid: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub playtime_forever: i64,
    #[serde(default)]
    pub playtime_2weeks: i64,
    /// 40-hex hash of the public community icon.
    #[serde(default)]
    pub img_icon_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedGames {
    /// Game details are private (Steam returns an empty `response`).
    Private,
    Games(Vec<OwnedGame>),
}

#[derive(Deserialize)]
struct OwnedResp {
    response: OwnedInner,
}
#[derive(Deserialize)]
struct OwnedInner {
    game_count: Option<u32>,
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(Deserialize)]
struct VanityResp {
    response: VanityInner,
}
#[derive(Deserialize)]
struct VanityInner {
    success: i32,
    steamid: Option<String>,
}

pub const PRIVATE_HELP: &str =
    "Your Steam game details are private, so Steam won't share your playtime. \
In Steam: Profile → Edit Profile → Privacy Settings → set \"Game details\" to Public \
(and untick \"Always keep my total playtime private\"), then sync again.";

/// Parse what a user typed: a 17-digit SteamID64, a profile URL or a custom URL name.
pub fn parse_profile(input: &str) -> Option<ProfileRef> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return None;
    }
    let is_id = |t: &str| {
        t.len() == 17 && t.starts_with("7656119") && t.chars().all(|c| c.is_ascii_digit())
    };
    if is_id(s) {
        return s.parse().ok().map(ProfileRef::Id);
    }
    if let Some(rest) = s.split("steamcommunity.com/").nth(1) {
        let mut it = rest.split('/');
        return match (it.next(), it.next()) {
            (Some("profiles"), Some(id)) if is_id(id) => id.parse().ok().map(ProfileRef::Id),
            (Some("id"), Some(v)) if valid_vanity(v) => Some(ProfileRef::Vanity(v.to_string())),
            _ => None,
        };
    }
    valid_vanity(s).then(|| ProfileRef::Vanity(s.to_string()))
}

fn valid_vanity(v: &str) -> bool {
    (2..=32).contains(&v.len())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Parse a `GetOwnedGames` JSON body.
pub fn parse_owned_games(body: &str) -> anyhow::Result<OwnedGames> {
    let r: OwnedResp = serde_json::from_str(body)?;
    Ok(match r.response.game_count {
        None => OwnedGames::Private,
        Some(_) => OwnedGames::Games(r.response.games),
    })
}

pub fn sigs() -> &'static SignatureDb {
    static S: OnceLock<SignatureDb> = OnceLock::new();
    S.get_or_init(SignatureDb::builtin)
}

impl SteamClient {
    pub fn new(cfg: &Config) -> Self {
        SteamClient {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("fraglink-server/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("http client"),
            key: cfg.steam_api_key.clone(),
            api_base: cfg.steam_api_base.clone(),
            openid_url: cfg.steam_openid_url.clone(),
        }
    }

    pub fn configured(&self) -> bool {
        self.key.is_some()
    }

    fn key(&self) -> ApiResult<&str> {
        self.key.as_deref().ok_or_else(|| {
            AppError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "steam_not_configured",
                "Steam import isn't set up on this server (STEAM_API_KEY is missing).",
            )
        })
    }

    async fn get_text(&self, url: String) -> ApiResult<String> {
        let resp = self.http.get(url).send().await.map_err(|e| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                "steam_unreachable",
                format!("Couldn't reach Steam: {}", e.without_url()),
            )
        })?;
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "steam_rate_limited",
                "Steam is rate-limiting us. Try again later.",
            ));
        }
        if !status.is_success() {
            return Err(AppError::new(
                StatusCode::BAD_GATEWAY,
                "steam_error",
                format!("Steam answered {status}."),
            ));
        }
        resp.text().await.map_err(|e| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                "steam_error",
                e.without_url().to_string(),
            )
        })
    }

    pub async fn resolve_vanity(&self, vanity: &str) -> ApiResult<u64> {
        let key = self.key()?;
        let url = format!(
            "{}/ISteamUser/ResolveVanityURL/v1/?key={}&vanityurl={}",
            self.api_base,
            key,
            url::form_urlencoded::byte_serialize(vanity.as_bytes()).collect::<String>()
        );
        let body = self.get_text(url).await?;
        let r: VanityResp = serde_json::from_str(&body).map_err(|_| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                "steam_error",
                "Unexpected answer from Steam.",
            )
        })?;
        match (r.response.success, r.response.steamid) {
            (1, Some(id)) => id.parse().map_err(|_| {
                AppError::new(
                    StatusCode::BAD_GATEWAY,
                    "steam_error",
                    "Unexpected answer from Steam.",
                )
            }),
            _ => Err(AppError::bad_request(
                "steam_profile_not_found",
                "No Steam profile with that custom URL.",
            )),
        }
    }

    pub async fn owned_games(&self, steamid: u64) -> ApiResult<OwnedGames> {
        let key = self.key()?;
        let url = format!(
            "{}/IPlayerService/GetOwnedGames/v1/?key={}&steamid={}&include_appinfo=1&include_played_free_games=1&format=json",
            self.api_base, key, steamid
        );
        let body = self.get_text(url).await?;
        parse_owned_games(&body).map_err(|_| {
            AppError::new(
                StatusCode::BAD_GATEWAY,
                "steam_error",
                "Unexpected answer from Steam.",
            )
        })
    }

    /// Where to send the browser for "Sign in through Steam".
    pub fn openid_redirect(&self, public_url: &str) -> String {
        self.openid_redirect_to(&format!("{public_url}/auth/steam/callback"), public_url)
    }

    /// "Sign in through Steam" URL returning to `return_to` (under `realm`).
    pub fn openid_redirect_to(&self, return_to: &str, realm: &str) -> String {
        let public_url = realm;
        let q: Vec<(&str, &str)> = vec![
            ("openid.ns", OPENID_NS),
            ("openid.mode", "checkid_setup"),
            ("openid.return_to", return_to),
            ("openid.realm", public_url),
            ("openid.identity", IDENTIFIER_SELECT),
            ("openid.claimed_id", IDENTIFIER_SELECT),
        ];
        let qs: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(q)
            .finish();
        format!("{}?{qs}", self.openid_url)
    }

    /// Verify an OpenID positive assertion by asking Steam directly
    /// (`check_authentication`). Returns the SteamID64.
    pub async fn verify_openid(
        &self,
        public_url: &str,
        params: &HashMap<String, String>,
    ) -> ApiResult<u64> {
        self.verify_openid_at(&format!("{public_url}/auth/steam/callback"), params)
            .await
    }

    /// Verify an assertion that must have come back to `expected_return`.
    pub async fn verify_openid_at(
        &self,
        expected_return: &str,
        params: &HashMap<String, String>,
    ) -> ApiResult<u64> {
        let bad = || {
            AppError::bad_request(
                "steam_openid_invalid",
                "Steam sign-in could not be verified.",
            )
        };
        let get = |k: &str| params.get(k).map(String::as_str);
        if get("openid.mode") != Some("id_res") || get("openid.ns") != Some(OPENID_NS) {
            return Err(bad());
        }
        if !get("openid.return_to").is_some_and(|r| r.starts_with(expected_return)) {
            return Err(bad());
        }
        if get("openid.op_endpoint") != Some(self.openid_url.as_str()) {
            return Err(bad());
        }
        let claimed = get("openid.claimed_id").ok_or_else(bad)?;
        let id = claimed
            .strip_prefix("https://steamcommunity.com/openid/id/")
            .filter(|s| s.len() == 17 && s.chars().all(|c| c.is_ascii_digit()))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(bad)?;
        let mut form: Vec<(String, String)> = params
            .iter()
            .filter(|(k, _)| k.starts_with("openid."))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for kv in form.iter_mut() {
            if kv.0 == "openid.mode" {
                kv.1 = "check_authentication".into();
            }
        }
        let resp = self
            .http
            .post(&self.openid_url)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                AppError::new(
                    StatusCode::BAD_GATEWAY,
                    "steam_unreachable",
                    e.without_url().to_string(),
                )
            })?;
        let text = resp.text().await.unwrap_or_default();
        if text.lines().any(|l| l.trim() == "is_valid:true") {
            Ok(id)
        } else {
            Err(bad())
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub games: usize,
    pub minutes_total: i64,
}

/// Look up a Steam persona name (public community XML, no key needed) and store it as
/// the link's display name. Best effort: failures leave the old value.
pub async fn refresh_persona(state: &AppState, user_id: Uuid, steamid: u64) {
    let base = std::env::var("FRAGLINK_STEAM_COMMUNITY")
        .unwrap_or_else(|_| "https://steamcommunity.com".into());
    let Ok(http) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
    else {
        return;
    };
    let Ok(r) = http
        .get(format!("{base}/profiles/{steamid}?xml=1"))
        .send()
        .await
    else {
        return;
    };
    let body = r.text().await.unwrap_or_default();
    let name = body
        .split_once("<steamID><![CDATA[")
        .and_then(|(_, t)| t.split_once("]]>"))
        .map(|(n, _)| n.trim().chars().take(64).collect::<String>())
        .filter(|n| !n.is_empty());
    if let Some(n) = name {
        let _ = sqlx::query(
            "UPDATE linked_accounts SET display = $3 WHERE user_id = $1 AND provider = 'steam' AND provider_uid = $2",
        )
        .bind(user_id)
        .bind(steamid.to_string())
        .bind(n)
        .execute(&state.db)
        .await;
    }
}

/// Save (or refresh) a Steam link.
pub async fn link(state: &AppState, user_id: Uuid, steamid: u64, verified: bool) -> ApiResult<()> {
    if verified {
        let other: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM linked_accounts WHERE provider = 'steam' AND provider_uid = $1 AND verified AND user_id <> $2",
        )
        .bind(steamid.to_string())
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
        if other.is_some() {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "steam_already_linked",
                "That Steam account is already linked to another Fraglink account.",
            ));
        }
    }
    sqlx::query(
        "INSERT INTO linked_accounts(user_id, provider, provider_uid, display, verified)
         VALUES($1, 'steam', $2, $2, $3)
         ON CONFLICT (user_id, provider) DO UPDATE
           SET provider_uid = excluded.provider_uid, display = excluded.display,
               verified = excluded.verified OR (linked_accounts.verified AND linked_accounts.provider_uid = excluded.provider_uid),
               linked_at = now(), sync_status = CASE WHEN linked_accounts.provider_uid = excluded.provider_uid THEN linked_accounts.sync_status END,
               last_sync_at = CASE WHEN linked_accounts.provider_uid = excluded.provider_uid THEN linked_accounts.last_sync_at END",
    )
    .bind(user_id)
    .bind(steamid.to_string())
    .bind(verified)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Import official playtime for the user's linked Steam account (Tracked tier).
pub async fn import(state: &AppState, user_id: Uuid) -> ApiResult<ImportResult> {
    let row: Option<(String, Option<chrono::DateTime<chrono::Utc>>)> = sqlx::query_as(
        "SELECT provider_uid, last_sync_at FROM linked_accounts WHERE user_id = $1 AND provider = 'steam'",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    let (uid, last) = row.ok_or_else(|| {
        AppError::bad_request("steam_not_linked", "Link your Steam account first.")
    })?;
    if let Some(t) = last {
        let age = (chrono::Utc::now() - t).num_seconds();
        if age < SYNC_COOLDOWN_SECS {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "steam_sync_cooldown",
                format!(
                    "Steam was synced {} min ago. Try again in {} min.",
                    age / 60,
                    (SYNC_COOLDOWN_SECS - age) / 60 + 1
                ),
            ));
        }
    }
    let steamid: u64 = uid.parse().map_err(AppError::internal)?;
    let games = match state.steam.owned_games(steamid).await? {
        OwnedGames::Private => {
            sqlx::query("UPDATE linked_accounts SET sync_status = 'private', last_sync_at = now() WHERE user_id = $1 AND provider = 'steam'")
                .bind(user_id)
                .execute(&state.db)
                .await?;
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "steam_private",
                PRIVATE_HELP,
            ));
        }
        OwnedGames::Games(g) => g,
    };
    // Admin > Detection aliases and merged games, so merged Steam hours stay merged
    let ids = crate::detection_aliases::SteamIds::load(state).await?;
    let mut tx = state.db.begin().await?;
    let mut n = 0;
    let mut total = 0;
    for g in games.iter().filter(|g| g.playtime_forever > 0) {
        let minted = fraglink_core::detector::steam_game_id(sigs(), g.appid);
        let gid = ids.id(&minted);
        if gid != minted {
            // counted as a game staff picked: keep that game's row as it is
            sqlx::query(
                "INSERT INTO games(id, name, steam_appid, verified) VALUES($1, $2, $3, true) ON CONFLICT (id) DO NOTHING",
            )
            .bind(&gid)
            .bind(if g.name.is_empty() { gid.clone() } else { g.name.clone() })
            .bind(g.appid as i32)
            .execute(&mut *tx)
            .await?;
        } else {
            let name = sigs()
                .by_steam_appid(g.appid)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| {
                    if g.name.is_empty() {
                        format!("Steam app {}", g.appid)
                    } else {
                        g.name.clone()
                    }
                });
            sqlx::query(
            "INSERT INTO games(id, name, steam_appid, steam_icon_hash, verified) VALUES($1, $2, $3, $4, true)
             ON CONFLICT (id) DO UPDATE SET steam_appid = COALESCE(games.steam_appid, excluded.steam_appid),
               name = CASE WHEN games.verified THEN games.name ELSE excluded.name END, verified = true,
               steam_icon_hash = CASE WHEN games.steam_appid IS NULL OR games.steam_appid = excluded.steam_appid
                                      THEN COALESCE(excluded.steam_icon_hash, games.steam_icon_hash)
                                      ELSE games.steam_icon_hash END",
        )
        .bind(&gid)
        .bind(&name)
        .bind(g.appid as i32)
        .bind(icon_hash(&g.img_icon_url))
        .execute(&mut *tx)
        .await?;
        }
        sqlx::query(
            "INSERT INTO steam_playtime(user_id, steam_appid, game_id, minutes_forever, minutes_2weeks, fetched_at)
             VALUES($1, $2, $3, $4, $5, now())
             ON CONFLICT (user_id, steam_appid) DO UPDATE
               SET game_id = excluded.game_id, minutes_forever = excluded.minutes_forever,
                   minutes_2weeks = excluded.minutes_2weeks, fetched_at = excluded.fetched_at",
        )
        .bind(user_id)
        .bind(g.appid as i32)
        .bind(&gid)
        .bind(g.playtime_forever.min(i32::MAX as i64) as i32)
        .bind(g.playtime_2weeks.min(i32::MAX as i64) as i32)
        .execute(&mut *tx)
        .await?;
        n += 1;
        total += g.playtime_forever;
    }
    sqlx::query("UPDATE linked_accounts SET sync_status = 'ok', last_sync_at = now() WHERE user_id = $1 AND provider = 'steam'")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(ImportResult {
        games: n,
        minutes_total: total,
    })
}

/// A valid Steam icon hash (40 lower-case hex chars), or None.
pub fn icon_hash(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    (s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_parsing() {
        assert_eq!(
            parse_profile("76561197960287930"),
            Some(ProfileRef::Id(76561197960287930))
        );
        assert_eq!(
            parse_profile("https://steamcommunity.com/profiles/76561197960287930/"),
            Some(ProfileRef::Id(76561197960287930))
        );
        assert_eq!(
            parse_profile("https://steamcommunity.com/id/gabelogannewell"),
            Some(ProfileRef::Vanity("gabelogannewell".into()))
        );
        assert_eq!(
            parse_profile("lucrona"),
            Some(ProfileRef::Vanity("lucrona".into()))
        );
        assert_eq!(parse_profile(""), None);
        assert_eq!(parse_profile("https://steamcommunity.com/groups/x"), None);
        assert_eq!(parse_profile("has space"), None);
    }

    #[test]
    fn owned_games_parsing() {
        let body = include_str!("../tests/fixtures/steam_owned_games.json");
        let OwnedGames::Games(g) = parse_owned_games(body).unwrap() else {
            panic!("expected games")
        };
        assert_eq!(g.len(), 4);
        assert_eq!(g[0].appid, 730);
        assert!(matches!(
            parse_owned_games(include_str!(
                "../tests/fixtures/steam_owned_games_private.json"
            ))
            .unwrap(),
            OwnedGames::Private
        ));
        assert!(parse_owned_games("<html>").is_err());
    }

    #[test]
    fn openid_redirect_has_required_params() {
        let mut cfg = Config::for_tests("postgres://x");
        cfg.steam_openid_url = "https://steamcommunity.com/openid/login".into();
        let c = SteamClient::new(&cfg);
        let u = c.openid_redirect("http://localhost:8787");
        assert!(u.starts_with("https://steamcommunity.com/openid/login?openid.ns=http%3A%2F%2Fspecs.openid.net%2Fauth%2F2.0"));
        assert!(u.contains("openid.mode=checkid_setup"));
        assert!(
            u.contains("openid.return_to=http%3A%2F%2Flocalhost%3A8787%2Fauth%2Fsteam%2Fcallback")
        );
    }
}
