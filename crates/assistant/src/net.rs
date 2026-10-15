//! What the assistant may talk to: the URL policy and the transport types the app implements.
//!
//! The crate itself opens no sockets (it must build for the browser); the desktop app hands in
//! an [`HttpStream`] that does the POST. Every URL is checked with [`check_url`] first.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::settings::Secret;

/// Longest URL accepted from the settings.
pub const MAX_URL_LEN: usize = 2048;

/// Largest error body kept to explain a failed request.
pub const MAX_ERROR_BODY: usize = 64 * 1024;

/// A cancellation flag shared by the UI thread and a turn's worker threads.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// One HTTPS (or loopback HTTP) POST.
pub struct HttpRequest {
    pub url: String,
    /// Header name and value. Values may hold an API key, so they are secrets.
    pub headers: Vec<(String, Secret)>,
    pub body: Vec<u8>,
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| n.as_str()).collect();
        f.debug_struct("HttpRequest").field("url", &self.url).field("headers", &names).field("body_bytes", &self.body.len()).finish()
    }
}

/// How a request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// The server answered with a non-success status; `body` is what it said (capped).
    Status { code: u16, retry_after: Option<u64>, body: String },
    /// No connection, TLS failure, timeout, or the stream broke off.
    Transport(String),
    /// Cancelled by the user.
    Cancelled,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Status { code, body, .. } if body.is_empty() => write!(f, "the server answered {code}"),
            HttpError::Status { code, body, .. } => write!(f, "the server answered {code}: {body}"),
            HttpError::Transport(m) => write!(f, "{m}"),
            HttpError::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Runs a POST and feeds the response body to the callback chunk by chunk; the callback returns
/// `false` to stop reading. Returns when the body ends, the callback stops it or it fails.
pub type HttpStream = Arc<dyn Fn(&HttpRequest, &Cancel, &mut dyn FnMut(&[u8]) -> bool) -> Result<(), HttpError> + Send + Sync>;

/// A URL split into the parts the policy looks at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedUrl {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    /// Path, query and fragment, as written (empty or starting with `/`).
    pub rest: String,
}

/// Split `url` into scheme, host, port and the rest. Refuses anything that is not
/// `http(s)://host[:port][/…]`: user info (`user:pass@`), control characters, spaces and empty
/// hosts.
pub fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    if url.len() > MAX_URL_LEN {
        return Err("the address is too long".into());
    }
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("the address holds spaces or control characters".into());
    }
    let (scheme, after) = url.split_once("://").ok_or("the address must start with https:// (or http://)")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!("the address scheme `{scheme}` is not supported, use https://"));
    }
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, rest) = after.split_at(end);
    if authority.contains('@') {
        return Err("the address must not hold a user name or password".into());
    }
    let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
        let (h, tail) = v6.split_once(']').ok_or("the address has an unclosed [")?;
        let port = match tail {
            "" => None,
            t => Some(t.strip_prefix(':').ok_or("the address has text after the IPv6 host")?),
        };
        (h.to_ascii_lowercase(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), Some(p)),
            None => (authority.to_ascii_lowercase(), None),
        }
    };
    if host.is_empty() || (!authority.starts_with('[') && host.contains(':')) {
        return Err("the address has no valid host".into());
    }
    let port = match port {
        None => None,
        Some(p) => Some(p.parse::<u16>().map_err(|_| "the address has an invalid port".to_string())?),
    };
    Ok(ParsedUrl { scheme, host, port, rest: rest.to_string() })
}

/// True for `localhost`, names under `.localhost`, `127.0.0.0/8` and `::1`.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_end_matches('.');
    if h == "localhost" || h.ends_with(".localhost") || h == "::1" {
        return true;
    }
    let parts: Vec<&str> = h.split('.').collect();
    parts.len() == 4
        && parts[0] == "127"
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

/// Whether the assistant may POST to `url`: always for `https`; for `http` only to a loopback
/// host, or to any host when `allow_http` (the user's opt-in for a server on the local network).
pub fn check_url(url: &str, allow_http: bool) -> Result<ParsedUrl, String> {
    let parsed = parse_url(url)?;
    if parsed.scheme == "http" && !allow_http && !is_loopback_host(&parsed.host) {
        return Err(format!(
            "`{}` would be reached without encryption; use https:// or allow unencrypted HTTP for this host in the settings",
            parsed.host
        ));
    }
    Ok(parsed)
}

/// `base` + `path` with exactly one `/` between them.
pub fn join_url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_always_allowed() {
        for u in ["https://api.anthropic.com", "https://api.openai.com/v1", "HTTPS://Example.COM:8443/x?y#z", "https://[2001:db8::1]:443/v1"] {
            assert!(check_url(u, false).is_ok(), "{u}");
        }
    }

    #[test]
    fn plain_http_only_to_loopback_unless_opted_in() {
        for u in ["http://localhost:11434/v1", "http://127.0.0.1:1234", "http://[::1]:8080/v1", "http://foo.localhost/v1", "http://127.255.0.9/"] {
            assert!(check_url(u, false).is_ok(), "{u}");
        }
        for u in
            ["http://example.com/v1", "http://192.168.1.20:1234/v1", "http://127.0.0.1.evil.com/", "http://1270.0.0.1/", "http://localhost.evil.com/"]
        {
            assert!(check_url(u, false).is_err(), "{u}");
            assert!(check_url(u, true).is_ok(), "{u} with opt-in");
        }
    }

    #[test]
    fn hostile_addresses_are_refused() {
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL_LEN));
        for u in [
            "",
            "example.com",
            "ftp://example.com",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pw@example.com/",
            "https://user@example.com/",
            "https://",
            "https://:443/",
            "https://example.com:99999/",
            "https://example.com:abc/",
            "https://[::1/",
            "https://[::1]x/",
            "http://::1/",
            "https://exa mple.com/",
            "https://example.com/\nHost: evil",
            long.as_str(),
        ] {
            assert!(check_url(u, true).is_err(), "{u:?}");
        }
    }

    #[test]
    fn urls_join_with_one_slash() {
        assert_eq!(join_url("https://a.b/v1/", "/messages"), "https://a.b/v1/messages");
        assert_eq!(join_url("https://a.b", "messages"), "https://a.b/messages");
    }

    #[test]
    fn requests_never_print_header_values() {
        let r = HttpRequest { url: "https://a.b".into(), headers: vec![("x-api-key".into(), Secret::new("sk-ant-secret"))], body: vec![1, 2, 3] };
        let shown = format!("{r:?}");
        assert!(!shown.contains("sk-ant-secret"));
        assert!(shown.contains("x-api-key"));
    }
}
