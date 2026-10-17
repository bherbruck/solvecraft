//! solidcraft-cam library: all modules accessible from integration tests
//! and the `solidcraft-cam` binary.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auth;
pub mod config;
pub mod cors;
pub mod error;
pub mod freecad;
pub mod jail;
pub mod jobs;
pub mod origin;
pub mod routes;
pub mod state;

use std::net::SocketAddr;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{delete, get, post},
};
use tokio::net::TcpListener;

use auth::auth_middleware;
use config::{CamConfig, MAX_REQUEST_BYTES};
use cors::cors_middleware;
use routes::{cancel_job_handler, health_handler, job_result_handler, job_status_handler, submit_job_handler, version_handler};
use state::AppState;

/// Build and bind the server, print the startup line, then serve forever.
///
/// Returns the bound port on success (used in integration tests).
pub async fn run(config: CamConfig) -> Result<u16, String> {
    let state = AppState::new(config);

    let unauthenticated = Router::new().route("/version", get(version_handler));

    let authenticated = Router::new()
        .route("/cam/v1/health", get(health_handler))
        .route("/cam/v1/jobs", post(submit_job_handler))
        .route("/cam/v1/jobs/:id/status", get(job_status_handler))
        .route("/cam/v1/jobs/:id/result", get(job_result_handler))
        .route("/cam/v1/jobs/:id", delete(cancel_job_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    let app = Router::new()
        .merge(unauthenticated)
        .merge(authenticated)
        .layer(middleware::from_fn_with_state(state.clone(), cors_middleware))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state.clone());

    // Bind to loopback IPv4 only.
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await.map_err(|e| format!("bind failed: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("local_addr: {e}"))?.port();

    println!("solidcraft-cam token={} address=http://127.0.0.1:{}", state.config.token, port);

    axum::serve(listener, app).await.map_err(|e| format!("serve error: {e}"))?;

    Ok(port)
}

/// Build the router without binding, for testing.
pub fn build_router(state: AppState) -> Router {
    let unauthenticated = Router::new().route("/version", get(version_handler));

    let authenticated = Router::new()
        .route("/cam/v1/health", get(health_handler))
        .route("/cam/v1/jobs", post(submit_job_handler))
        .route("/cam/v1/jobs/:id/status", get(job_status_handler))
        .route("/cam/v1/jobs/:id/result", get(job_result_handler))
        .route("/cam/v1/jobs/:id", delete(cancel_job_handler))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth_middleware));

    Router::new()
        .merge(unauthenticated)
        .merge(authenticated)
        .layer(middleware::from_fn_with_state(state.clone(), cors_middleware))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}
