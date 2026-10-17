//! Strict `Origin` header parsing with userinfo and path-injection defences.
//!
//! An HTTP origin is `scheme://host[:port]` with **no** userinfo, path, query or fragment.
//! This module enforces that definition, rejects the `null` origin, and provides an
//! allowlist type used by the CORS middleware.
//!
//! # Security properties
//! - Rejects any value containing `@` (userinfo: `user@host`, `user:pass@host`).
//! - Rejects values containing `/`, `?`, or `#` (path / query / fragment injection).
//! - Rejects the string `null` (sent by browsers from `data:`, `file:`, sandboxed iframes).
//! - Accepts only `http` and `https` schemes (lowercase).
//! - Compares hosts case-insensitively (RFC 7230 § 2.7.3).
//! - Handles IPv6 bracket notation (`[::1]`).

use serde::{Deserialize, Serialize};
use std::fmt;

/// The scheme component of a parsed origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Http,
    Https,
}

impl fmt::Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scheme::Http => f.write_str("http"),
            Scheme::Https => f.write_str("https"),
        }
    }
}

/// A validated, parsed HTTP origin.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParsedOrigin {
    pub scheme: Scheme,
    /// Lowercase host (possibly bracketed IPv6 address e.g. `[::1]`).
    pub host: String,
    /// Explicit port, if present in the header value.
    pub port: Option<u16>,
}

/// Errors produced by [`ParsedOrigin::parse`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OriginParseError {
    #[error("origin is empty or the string `null`")]
    NullOrEmpty,
    #[error("missing `://` separator (or multiple `://`)")]
    MissingScheme,
    #[error("scheme must be `http` or `https`")]
    InvalidScheme,
    #[error("origin must not contain userinfo (the `@` character)")]
    UserinfoPresent,
    #[error("origin must not contain a path, query or fragment")]
    PathNotAllowed,
    #[error("host part is empty")]
    EmptyHost,
    #[error("host contains invalid characters")]
    InvalidHost,
    #[error("port number is malformed or out of range")]
    InvalidPort,
    #[error("IPv6 address bracket is not closed")]
    UnterminatedIpv6,
}

impl ParsedOrigin {
    /// Parse and validate an `Origin` header value.
    ///
    /// Returns `Err` for any of the rejection conditions documented in the module header.
    pub fn parse(s: &str) -> Result<Self, OriginParseError> {
        if s.is_empty() || s.eq_ignore_ascii_case("null") {
            return Err(OriginParseError::NullOrEmpty);
        }

        // Split at the FIRST "://".
        let (scheme_str, rest) = s.split_once("://").ok_or(OriginParseError::MissingScheme)?;

        // Ensure the authority portion itself contains no extra "://" sequences.
        if rest.contains("://") {
            return Err(OriginParseError::MissingScheme);
        }

        let scheme = match scheme_str {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            _ => return Err(OriginParseError::InvalidScheme),
        };

        // Userinfo check: '@' anywhere in the authority is forbidden.
        if rest.contains('@') {
            return Err(OriginParseError::UserinfoPresent);
        }

        // No path / query / fragment in a serialised origin.
        if rest.contains('/') || rest.contains('?') || rest.contains('#') {
            return Err(OriginParseError::PathNotAllowed);
        }

        let (host, port) = parse_host_port(rest)?;

        Ok(Self { scheme, host, port })
    }

    /// Canonical serialised form: `scheme://host` or `scheme://host:port`.
    pub fn serialized(&self) -> String {
        match self.port {
            Some(p) => format!("{}://{}:{}", self.scheme, self.host, p),
            None => format!("{}://{}", self.scheme, self.host),
        }
    }

    /// Returns `true` when `header_value` parses to the same origin as `self`.
    ///
    /// Host comparison is case-insensitive. An unparseable value always returns `false`.
    pub fn matches_header(&self, header_value: &str) -> bool {
        match Self::parse(header_value) {
            Ok(parsed) => self.scheme == parsed.scheme && self.host.eq_ignore_ascii_case(&parsed.host) && self.port == parsed.port,
            Err(_) => false,
        }
    }
}

fn parse_host_port(authority: &str) -> Result<(String, Option<u16>), OriginParseError> {
    if authority.is_empty() {
        return Err(OriginParseError::EmptyHost);
    }

    if authority.starts_with('[') {
        // IPv6 literal: `[addr]` or `[addr]:port`
        let close = authority.find(']').ok_or(OriginParseError::UnterminatedIpv6)?;
        let host = &authority[..=close]; // includes the brackets
        let after = &authority[close + 1..];
        let port = if after.is_empty() {
            None
        } else if let Some(port_str) = after.strip_prefix(':') {
            let p: u16 = port_str.parse().map_err(|_| OriginParseError::InvalidPort)?;
            Some(p)
        } else {
            return Err(OriginParseError::InvalidHost);
        };
        validate_host_chars(host)?;
        return Ok((host.to_lowercase(), port));
    }

    // Non-IPv6: find the last colon for port separation.
    if let Some(colon) = authority.rfind(':') {
        let host_part = &authority[..colon];
        let port_part = &authority[colon + 1..];
        // A non-bracketed host must not itself contain a colon.
        if host_part.contains(':') {
            return Err(OriginParseError::InvalidHost);
        }
        if host_part.is_empty() {
            return Err(OriginParseError::EmptyHost);
        }
        let port: u16 = port_part.parse().map_err(|_| OriginParseError::InvalidPort)?;
        validate_host_chars(host_part)?;
        Ok((host_part.to_lowercase(), Some(port)))
    } else {
        validate_host_chars(authority)?;
        Ok((authority.to_lowercase(), None))
    }
}

fn validate_host_chars(host: &str) -> Result<(), OriginParseError> {
    if host.is_empty() {
        return Err(OriginParseError::EmptyHost);
    }
    // Reject ASCII control characters (< 0x21) and DEL (0x7f).
    if host.bytes().any(|b| b < 0x21 || b == 0x7f) {
        return Err(OriginParseError::InvalidHost);
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Valid origins ─────────────────────────────────────────────────────────

    #[test]
    fn parse_http_localhost_port() {
        let o = ParsedOrigin::parse("http://localhost:3000").unwrap();
        assert_eq!(o.scheme, Scheme::Http);
        assert_eq!(o.host, "localhost");
        assert_eq!(o.port, Some(3000));
        assert_eq!(o.serialized(), "http://localhost:3000");
    }

    #[test]
    fn parse_https_no_port() {
        let o = ParsedOrigin::parse("https://example.com").unwrap();
        assert_eq!(o.scheme, Scheme::Https);
        assert_eq!(o.host, "example.com");
        assert_eq!(o.port, None);
        assert_eq!(o.serialized(), "https://example.com");
    }

    #[test]
    fn parse_ipv4_loopback() {
        let o = ParsedOrigin::parse("http://127.0.0.1:8080").unwrap();
        assert_eq!(o.host, "127.0.0.1");
        assert_eq!(o.port, Some(8080));
    }

    #[test]
    fn parse_ipv6_loopback_with_port() {
        let o = ParsedOrigin::parse("http://[::1]:4000").unwrap();
        assert_eq!(o.host, "[::1]");
        assert_eq!(o.port, Some(4000));
        assert_eq!(o.serialized(), "http://[::1]:4000");
    }

    #[test]
    fn parse_ipv6_no_port() {
        let o = ParsedOrigin::parse("http://[::1]").unwrap();
        assert_eq!(o.host, "[::1]");
        assert_eq!(o.port, None);
    }

    #[test]
    fn host_stored_lowercase() {
        let o = ParsedOrigin::parse("https://Example.COM:443").unwrap();
        assert_eq!(o.host, "example.com");
    }

    // ── Rejection: null / empty ───────────────────────────────────────────────

    #[test]
    fn reject_empty() {
        assert!(matches!(ParsedOrigin::parse(""), Err(OriginParseError::NullOrEmpty)));
    }

    #[test]
    fn reject_null_lowercase() {
        assert!(matches!(ParsedOrigin::parse("null"), Err(OriginParseError::NullOrEmpty)));
    }

    #[test]
    fn reject_null_uppercase() {
        assert!(matches!(ParsedOrigin::parse("NULL"), Err(OriginParseError::NullOrEmpty)));
    }

    // ── Rejection: userinfo attacks ───────────────────────────────────────────

    #[test]
    fn reject_userinfo_colon_password() {
        // Classic credential-injection attempt
        assert!(matches!(ParsedOrigin::parse("http://user:pass@evil.com"), Err(OriginParseError::UserinfoPresent)));
    }

    #[test]
    fn reject_userinfo_bare_at() {
        assert!(matches!(ParsedOrigin::parse("http://user@evil.com"), Err(OriginParseError::UserinfoPresent)));
    }

    #[test]
    fn reject_userinfo_with_port() {
        assert!(matches!(ParsedOrigin::parse("http://user:pass@evil.com:80"), Err(OriginParseError::UserinfoPresent)));
    }

    #[test]
    fn reject_at_after_host() {
        // Attempt to sneak '@' after the host (RFC-invalid but we must still reject)
        assert!(matches!(ParsedOrigin::parse("http://trusted.com@evil.com"), Err(OriginParseError::UserinfoPresent)));
    }

    // ── Rejection: path / query / fragment ───────────────────────────────────

    #[test]
    fn reject_path_component() {
        assert!(matches!(ParsedOrigin::parse("http://host/path"), Err(OriginParseError::PathNotAllowed)));
    }

    #[test]
    fn reject_query_component() {
        assert!(matches!(ParsedOrigin::parse("http://host?q=1"), Err(OriginParseError::PathNotAllowed)));
    }

    #[test]
    fn reject_fragment_component() {
        assert!(matches!(ParsedOrigin::parse("http://host#frag"), Err(OriginParseError::PathNotAllowed)));
    }

    // ── Rejection: scheme ────────────────────────────────────────────────────

    #[test]
    fn reject_missing_scheme_separator() {
        assert!(matches!(ParsedOrigin::parse("localhost:3000"), Err(OriginParseError::MissingScheme)));
    }

    #[test]
    fn reject_ftp_scheme() {
        assert!(matches!(ParsedOrigin::parse("ftp://host"), Err(OriginParseError::InvalidScheme)));
    }

    #[test]
    fn reject_file_scheme() {
        assert!(matches!(ParsedOrigin::parse("file://host"), Err(OriginParseError::InvalidScheme)));
    }

    #[test]
    fn reject_double_scheme_injection() {
        // e.g. "http://trusted://evil" — "rest" would contain "://evil"
        assert!(ParsedOrigin::parse("http://trusted.com://evil.com").is_err());
    }

    // ── Rejection: IPv6 ──────────────────────────────────────────────────────

    #[test]
    fn reject_bare_ipv6_no_brackets() {
        assert!(ParsedOrigin::parse("http://::1").is_err());
    }

    #[test]
    fn reject_unterminated_ipv6_bracket() {
        assert!(matches!(ParsedOrigin::parse("http://[::1"), Err(OriginParseError::UnterminatedIpv6)));
    }

    // ── Allowlist matching ───────────────────────────────────────────────────

    #[test]
    fn matches_exact_origin() {
        let allowed = ParsedOrigin::parse("http://localhost:3000").unwrap();
        assert!(allowed.matches_header("http://localhost:3000"));
    }

    #[test]
    fn no_match_different_port() {
        let allowed = ParsedOrigin::parse("http://localhost:3000").unwrap();
        assert!(!allowed.matches_header("http://localhost:3001"));
    }

    #[test]
    fn no_match_different_scheme() {
        let allowed = ParsedOrigin::parse("http://localhost:3000").unwrap();
        assert!(!allowed.matches_header("https://localhost:3000"));
    }

    #[test]
    fn no_match_different_host() {
        let allowed = ParsedOrigin::parse("http://localhost:3000").unwrap();
        assert!(!allowed.matches_header("http://evil.com:3000"));
    }

    #[test]
    fn host_comparison_is_case_insensitive() {
        let allowed = ParsedOrigin::parse("http://Example.COM:80").unwrap();
        assert!(allowed.matches_header("http://example.com:80"));
        assert!(allowed.matches_header("http://EXAMPLE.COM:80"));
    }

    #[test]
    fn unparseable_header_never_matches() {
        let allowed = ParsedOrigin::parse("http://localhost").unwrap();
        assert!(!allowed.matches_header("http://user@localhost"));
        assert!(!allowed.matches_header("null"));
        assert!(!allowed.matches_header(""));
    }
}
