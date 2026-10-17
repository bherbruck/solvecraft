//! Authentication middleware: validates `Authorization: Bearer <token>`.
//!
//! # Security properties
//! - Token is compared in constant time to avoid timing side-channels.
//! - Token is NEVER accepted in URL query parameters or path segments.
//! - Token is NEVER logged (callers must not log request headers).

use axum::{extract::Request, middleware::Next, response::Response};

use crate::{error::CamError, state::AppState};

/// Generates a cryptographically-random bearer token (32 bytes → 64-char lowercase hex).
pub fn generate_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut acc, b| {
        use std::fmt::Write as _;
        let _ = write!(acc, "{:02x}", b);
        acc
    })
}

/// Axum middleware that enforces `Authorization: Bearer <token>`.
///
/// Must run on authenticated routes only; the `/version` endpoint is excluded.
pub async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, CamError> {
    let auth = request.headers().get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).ok_or(CamError::Unauthorized)?;

    let token = auth.strip_prefix("Bearer ").ok_or(CamError::Unauthorized)?;

    if !constant_time_eq(token.as_bytes(), state.config.token.as_bytes()) {
        return Err(CamError::Unauthorized);
    }

    Ok(next.run(request).await)
}

/// Byte-slice equality in constant time (avoids timing side-channels).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        // Length difference leaks; but token length is public (always 64 chars).
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_64_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tokens_are_unique() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn constant_time_eq_same() {
        assert!(constant_time_eq(b"abc", b"abc"));
    }

    #[test]
    fn constant_time_eq_different() {
        assert!(!constant_time_eq(b"abc", b"xyz"));
    }

    #[test]
    fn constant_time_eq_different_length() {
        assert!(!constant_time_eq(b"ab", b"abc"));
    }

    #[test]
    fn constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }
}
