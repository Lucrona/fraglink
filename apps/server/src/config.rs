//! Configuration from environment variables (no config files, no secrets in the repo).

#[derive(Debug, Clone)]
pub struct Config {
    /// `DATABASE_URL`, e.g. `postgres://fraglink:fraglink@localhost:5432/fraglink`.
    pub database_url: String,
    /// `FRAGLINK_BIND` (default `127.0.0.1:8787`).
    pub bind: String,
    /// `FRAGLINK_PUBLIC_URL`: how browsers reach this server (default `http://localhost:8787`).
    pub public_url: String,
    /// `STEAM_API_KEY`: Steam Web API key. Without it Steam import is disabled.
    pub steam_api_key: Option<String>,
    /// `STEAM_API_BASE` (default `https://api.steampowered.com`; tests point it at a mock).
    pub steam_api_base: String,
    /// `STEAM_OPENID_URL` (default `https://steamcommunity.com/openid/login`).
    pub steam_openid_url: String,
    /// Seconds without any message before a presence socket is dropped (default 150).
    pub presence_idle_secs: u64,
    /// `FRAGLINK_APP_ICON_CDN` (default `https://cdn.discordapp.com`): where the
    /// detectable list's `icon_hash` icons are fetched from (server side only).
    pub app_icon_cdn_url: String,
    /// `FRAGLINK_STEAM_ICON_CDN`: base of Steam community icons (server side only).
    pub steam_cdn_url: String,
    /// `FRAGLINK_DATA_DIR` (default `/var/lib/fraglink`): cached game lists and the
    /// optional `detect-overrides.json`.
    pub data_dir: std::path::PathBuf,
    /// `FRAGLINK_DETECTABLE_URL` (default Discord's public detectable list).
    pub detectable_url: String,
    /// `STEAM_STORE_URL` (default `https://store.steampowered.com`, `off` disables):
    /// names for Steam games that aren't on our lists.
    pub steam_store_url: Option<String>,
    /// `TURNSTILE_SECRET`: Cloudflare Turnstile secret. Unset = no bot check (local, tests).
    pub turnstile_secret: Option<String>,
    /// `TURNSTILE_VERIFY_URL` (default Cloudflare's siteverify).
    pub turnstile_verify_url: String,
    /// `FRAGLINK_MAIL=on` sends email (with a provider key, see `mailer`); anything else only logs it.
    pub mail_enabled: bool,
    /// `FRAGLINK_STEAM_ART_CDN`: base for Steam store header art (`<base>/<appid>/header.jpg`).
    pub steam_art_cdn: String,
    /// `FRAGLINK_SITE_URL` (default `https://beta.playfraglink.com`): website links in emails.
    pub site_url: String,
    /// `FRAGLINK_CANONICAL_URL` (default `https://playfraglink.com`): the one public address
    /// search engines see (sitemap and canonical links), whichever host served the request.
    pub canonical_url: String,
    /// `FRAGLINK_SITE_DIR` (default `/var/www/fraglink-beta`): the built website, for the
    /// profile page the server sends with per-profile robots tags.
    pub site_dir: std::path::PathBuf,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        Ok(Config {
            database_url: var("DATABASE_URL").ok_or_else(|| {
                anyhow::anyhow!("DATABASE_URL is not set (see apps/server/README.md)")
            })?,
            bind: var("FRAGLINK_BIND").unwrap_or_else(|| "127.0.0.1:8787".into()),
            public_url: var("FRAGLINK_PUBLIC_URL")
                .unwrap_or_else(|| "http://localhost:8787".into())
                .trim_end_matches('/')
                .to_string(),
            steam_api_key: var("STEAM_API_KEY"),
            steam_api_base: var("STEAM_API_BASE")
                .unwrap_or_else(|| "https://api.steampowered.com".into())
                .trim_end_matches('/')
                .to_string(),
            steam_openid_url: var("STEAM_OPENID_URL")
                .unwrap_or_else(|| "https://steamcommunity.com/openid/login".into()),
            presence_idle_secs: crate::presence::IDLE_TIMEOUT_SECS,
            app_icon_cdn_url: var("FRAGLINK_APP_ICON_CDN")
                .unwrap_or_else(|| "https://cdn.discordapp.com".into()),
            steam_cdn_url: var("FRAGLINK_STEAM_ICON_CDN").unwrap_or_else(|| {
                "https://cdn.cloudflare.steamstatic.com/steamcommunity/public/images/apps".into()
            }),
            data_dir: var("FRAGLINK_DATA_DIR")
                .unwrap_or_else(|| "/var/lib/fraglink".into())
                .into(),
            detectable_url: var("FRAGLINK_DETECTABLE_URL")
                .unwrap_or_else(|| crate::games::DISCORD_URL.into()),
            steam_store_url: match var("STEAM_STORE_URL").as_deref() {
                Some("off") => None,
                Some(u) => Some(u.trim_end_matches('/').to_string()),
                None => Some("https://store.steampowered.com".into()),
            },
            turnstile_secret: var("TURNSTILE_SECRET"),
            turnstile_verify_url: var("TURNSTILE_VERIFY_URL").unwrap_or_else(|| {
                "https://challenges.cloudflare.com/turnstile/v0/siteverify".into()
            }),
            mail_enabled: var("FRAGLINK_MAIL").as_deref() == Some("on"),
            steam_art_cdn: var("FRAGLINK_STEAM_ART_CDN")
                .unwrap_or_else(|| "https://cdn.cloudflare.steamstatic.com/steam/apps".into())
                .trim_end_matches('/')
                .to_string(),
            site_url: var("FRAGLINK_SITE_URL")
                .unwrap_or_else(|| "https://beta.playfraglink.com".into())
                .trim_end_matches('/')
                .to_string(),
            canonical_url: var("FRAGLINK_CANONICAL_URL")
                .unwrap_or_else(|| "https://playfraglink.com".into())
                .trim_end_matches('/')
                .to_string(),
            site_dir: var("FRAGLINK_SITE_DIR")
                .unwrap_or_else(|| "/var/www/fraglink-beta".into())
                .into(),
        })
    }

    /// Config for tests: no Steam key unless given.
    pub fn for_tests(database_url: &str) -> Self {
        Config {
            database_url: database_url.into(),
            bind: "127.0.0.1:0".into(),
            public_url: "http://localhost:8787".into(),
            steam_api_key: None,
            steam_api_base: "http://127.0.0.1:9".into(),
            steam_openid_url: "http://127.0.0.1:9/openid/login".into(),
            presence_idle_secs: crate::presence::IDLE_TIMEOUT_SECS,
            app_icon_cdn_url: "http://127.0.0.1:9".into(),
            steam_cdn_url: "http://127.0.0.1:9/apps".into(),
            data_dir: std::env::temp_dir().join("fraglink-server-tests"),
            detectable_url: "http://127.0.0.1:9/detectable".into(),
            steam_store_url: None,
            turnstile_secret: None,
            turnstile_verify_url: "http://127.0.0.1:9/siteverify".into(),
            mail_enabled: false,
            steam_art_cdn: "http://127.0.0.1:9/apps".into(),
            site_url: "http://localhost:8787".into(),
            canonical_url: "https://playfraglink.com".into(),
            site_dir: std::env::temp_dir().join("fraglink-site-tests"),
        }
    }

    pub fn secure_cookies(&self) -> bool {
        self.public_url.starts_with("https://")
    }

    /// `scheme://host[:port]` of the public URL (for Origin checks).
    pub fn public_origin(&self) -> String {
        match url::Url::parse(&self.public_url) {
            Ok(u) => u.origin().ascii_serialization(),
            Err(_) => self.public_url.clone(),
        }
    }
}
