//! Request context for sign-ins: IP, a short device description from the User-Agent,
//! and a coarse location (country) from an optional offline database
//! (`FRAGLINK_DATA_DIR/geo/country.mmdb`, e.g. DB-IP "IP to Country Lite", CC BY 4.0).
//! No lookup ever leaves the server.

use crate::AppState;
use axum::http::HeaderMap;

#[derive(Debug, Clone, Default)]
pub struct ReqCtx {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
}

impl ReqCtx {
    pub fn from_headers(st: &AppState, headers: &HeaderMap) -> Self {
        let ip = crate::client_ip(headers);
        let ip = (ip != "local").then_some(ip);
        let user_agent = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.chars().take(300).collect::<String>());
        let location = ip.as_deref().and_then(|i| country(st, i));
        ReqCtx {
            ip,
            user_agent,
            location,
        }
    }

    /// "Windows · Chrome", "Fraglink app" ...
    pub fn device(&self) -> String {
        describe_ua(self.user_agent.as_deref().unwrap_or(""))
    }
}

pub fn describe_ua(ua: &str) -> String {
    let l = ua.to_lowercase();
    if ua.is_empty() {
        return "Unknown device".into();
    }
    if l.contains("fraglink") || l.starts_with("reqwest") {
        return "Fraglink app".into();
    }
    let os = if l.contains("windows") {
        "Windows"
    } else if l.contains("android") {
        "Android"
    } else if l.contains("iphone") || l.contains("ipad") {
        "iOS"
    } else if l.contains("mac os") {
        "macOS"
    } else if l.contains("linux") {
        "Linux"
    } else {
        "Unknown OS"
    };
    let br = if l.contains("edg/") {
        "Edge"
    } else if l.contains("opr/") {
        "Opera"
    } else if l.contains("firefox/") {
        "Firefox"
    } else if l.contains("chrome/") {
        "Chrome"
    } else if l.contains("safari/") {
        "Safari"
    } else {
        "browser"
    };
    format!("{os} · {br}")
}

fn reader(st: &AppState) -> Option<&'static maxminddb::Reader<Vec<u8>>> {
    static R: std::sync::OnceLock<Option<maxminddb::Reader<Vec<u8>>>> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let p = st.cfg.data_dir.join("geo").join("country.mmdb");
        maxminddb::Reader::open_readfile(&p).ok()
    })
    .as_ref()
}

pub fn country(st: &AppState, ip: &str) -> Option<String> {
    let ip: std::net::IpAddr = ip.parse().ok()?;
    let r = reader(st)?;
    let c: maxminddb::geoip2::Country = r.lookup(ip).ok()?;
    let names = c.country?.names?;
    names.get("en").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ua() {
        assert_eq!(super::describe_ua("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Safari/537.36"), "Windows · Chrome");
        assert_eq!(super::describe_ua("Fraglink/0.2.0"), "Fraglink app");
        assert_eq!(super::describe_ua(""), "Unknown device");
    }
}
