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
