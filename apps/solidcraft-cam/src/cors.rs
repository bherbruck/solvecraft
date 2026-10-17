//! CORS and Private Network Access (PNA) middleware.
//!
//! # Policy
//! - The `Origin` header is parsed and matched against the configured allowlist.
//! - If the origin matches, the response receives `Access-Control-Allow-Origin`
//!   set to the exact origin (never a wildcard).
//! - `Vary: Origin` is added whenever CORS headers are emitted.
//! - OPTIONS preflight requests are handled here and short-circuit to 204; they
//!   bypass auth middleware intentionally (browsers do not send credentials in
//!   preflight requests).
//! - When the preflight carries `Access-Control-Request-Private-Network: true`
//!   the response includes `Access-Control-Allow-Private-Network: true`.

use axum::{
    extract::Request,
    http::{
        HeaderValue, Method, StatusCode,
        header::{ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_MAX_AGE, VARY},
    },
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::state::AppState;

const ALLOW_PRIVATE_NETWORK_REQ: &str = "access-control-request-private-network";
const ALLOW_PRIVATE_NETWORK_RES: &str = "access-control-allow-private-network";

/// Axum middleware that handles CORS preflight and injects CORS response headers.
pub async fn cors_middleware(axum::extract::State(state): axum::extract::State<AppState>, request: Request, next: Next) -> Response {
    let origin_value = request.headers().get(axum::http::header::ORIGIN).and_then(|v| v.to_str().ok()).map(|s| s.to_owned());

    let is_preflight = request.method() == Method::OPTIONS;
    let requests_pna = request.headers().contains_key(ALLOW_PRIVATE_NETWORK_REQ);

    // Short-circuit preflight without running the authenticated handler.
    let mut response = if is_preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(request).await };

    // Attach CORS headers only when the origin is in the allowlist.
    if let Some(origin_str) = origin_value {
        let allowed = state.config.allowed_origins.iter().any(|o| o.matches_header(&origin_str));

        if allowed {
            let origin_hv = match HeaderValue::from_str(&origin_str) {
                Ok(v) => v,
                Err(_) => return response,
            };
            let headers = response.headers_mut();
            headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin_hv);
            headers.insert(VARY, HeaderValue::from_static("Origin"));

            if is_preflight {
                headers.insert(ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, DELETE, OPTIONS"));
                headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Authorization, Content-Type"));
                headers.insert(ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("86400"));
                if requests_pna {
                    headers.insert(ALLOW_PRIVATE_NETWORK_RES, HeaderValue::from_static("true"));
                }
            }
        }
    }

    response
}
