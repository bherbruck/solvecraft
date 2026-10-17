//! Integration tests for solidcraft-cam: auth, CORS/origin, bounds, path jail,
//! and job state via the axum router (no real TCP bind).

use axum::{
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use serde_json::{Value, json};
use tower::ServiceExt; // for `oneshot`

use solidcraft_cam::{
    build_router,
    config::{CamConfig, DEFAULT_JOB_TIMEOUT_SECS, MAX_REQUEST_BYTES},
    origin::ParsedOrigin,
    state::AppState,
};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn test_config(origins: Vec<&str>) -> CamConfig {
    let allowed_origins = origins.into_iter().map(|s| ParsedOrigin::parse(s).unwrap()).collect();
    CamConfig {
        token: "test-token-aabbccdd".into(),
        allowed_origins,
        temp_base: std::env::temp_dir(),
        freecad_exec: "false".into(), // always fails — fine for auth/bounds tests
        adapter_path: std::path::PathBuf::from("/dev/null"),
        job_timeout_secs: DEFAULT_JOB_TIMEOUT_SECS,
    }
}

fn make_router(config: CamConfig) -> axum::Router {
    build_router(AppState::new(config))
}

async fn body_json(body: Body) -> Value {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

// ── /version: no auth required ───────────────────────────────────────────────

#[tokio::test]
async fn version_no_auth() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder().uri("/version").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp.into_body()).await;
    assert_eq!(body["name"], "solidcraft-cam");
    assert_eq!(body["api_version"], 1);
}

#[tokio::test]
async fn version_returns_pinned_versions() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder().uri("/version").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = body_json(resp.into_body()).await;
    assert!(body["freecad_pin"].as_str().is_some());
    assert!(body["ocp_freecad_cam_pin"].as_str().is_some());
}

// ── Authentication ────────────────────────────────────────────────────────────

#[tokio::test]
async fn authenticated_route_without_token_returns_401() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder().method(Method::GET).uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_route_with_wrong_token_returns_401() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status")
        .header(header::AUTHORIZATION, "Bearer wrong-token")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_route_with_correct_token_passes_auth() {
    let app = make_router(test_config(vec![]));
    // Should get 404 (job not found), not 401 — confirming auth passed.
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_ne!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_in_query_string_is_rejected() {
    // Even if the query carries the right token, the header is missing → 401.
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status?token=test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_with_empty_token_is_rejected() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status")
        .header(header::AUTHORIZATION, "Bearer ")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── CORS / Origin allowlist ───────────────────────────────────────────────────

#[tokio::test]
async fn cors_header_added_for_allowed_origin() {
    let app = make_router(test_config(vec!["http://localhost:3000"]));
    let req = Request::builder().uri("/version").header("origin", "http://localhost:3000").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.headers().get("access-control-allow-origin").and_then(|v| v.to_str().ok()), Some("http://localhost:3000"));
}

#[tokio::test]
async fn cors_header_not_added_for_disallowed_origin() {
    let app = make_router(test_config(vec!["http://localhost:3000"]));
    let req = Request::builder().uri("/version").header("origin", "http://evil.com").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn cors_header_not_added_when_no_origins_configured() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder().uri("/version").header("origin", "http://localhost:3000").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn cors_preflight_returns_204_with_headers() {
    let app = make_router(test_config(vec!["http://localhost:3000"]));
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/cam/v1/jobs")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert!(resp.headers().get("access-control-allow-origin").is_some());
    assert!(resp.headers().get("access-control-allow-methods").is_some());
}

#[tokio::test]
async fn pna_preflight_includes_allow_private_network() {
    let app = make_router(test_config(vec!["http://localhost:3000"]));
    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/cam/v1/jobs")
        .header("origin", "http://localhost:3000")
        .header("access-control-request-method", "POST")
        .header("access-control-request-private-network", "true")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.headers().get("access-control-allow-private-network").and_then(|v| v.to_str().ok()), Some("true"));
}

#[tokio::test]
async fn origin_with_userinfo_in_header_is_ignored() {
    // Even if an attacker sends userinfo in the header, we never match it.
    let app = make_router(test_config(vec!["http://localhost:3000"]));
    let req = Request::builder().uri("/version").header("origin", "http://user:pass@localhost:3000").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

// ── Request body size limits ──────────────────────────────────────────────────

#[tokio::test]
async fn oversized_json_body_is_rejected() {
    let app = make_router(test_config(vec![]));
    // Send a body that exceeds MAX_REQUEST_BYTES.
    let big_body = "x".repeat(MAX_REQUEST_BYTES + 1);
    let req = Request::builder()
        .method(Method::POST)
        .uri("/cam/v1/jobs")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(big_body))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// ── Job submit: missing/bad STEP data ────────────────────────────────────────

#[tokio::test]
async fn submit_without_step_returns_400() {
    let app = make_router(test_config(vec![]));
    let body = json!({ "manufacture": {} });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/cam/v1/jobs")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_with_both_step_fields_returns_400() {
    let app = make_router(test_config(vec![]));
    let body = json!({
        "step_text": "ISO-10303-21;",
        "step_b64": "ABCD",
        "manufacture": {}
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/cam/v1/jobs")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn submit_with_invalid_base64_returns_400() {
    let app = make_router(test_config(vec![]));
    let body = json!({
        "step_b64": "not!valid!base64!!!",
        "manufacture": {}
    });
    let req = Request::builder()
        .method(Method::POST)
        .uri("/cam/v1/jobs")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Job status / result: unknown ID ──────────────────────────────────────────

#[tokio::test]
async fn status_unknown_job_returns_404() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/status")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn result_unknown_job_returns_404() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000/result")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_unknown_job_returns_404() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::DELETE)
        .uri("/cam/v1/jobs/00000000-0000-0000-0000-000000000000")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
} // ── Status with invalid UUID ──────────────────────────────────────────────────

#[tokio::test]
async fn status_malformed_uuid_returns_400() {
    let app = make_router(test_config(vec![]));
    let req = Request::builder()
        .method(Method::GET)
        .uri("/cam/v1/jobs/not-a-uuid/status")
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── Cancel already-terminal job returns 409 ───────────────────────────────────

#[tokio::test]
async fn cancel_terminal_job_returns_409() {
    use solidcraft_cam::{jobs::JobResult, state::AppState};

    let config = test_config(vec![]);
    let state = AppState::new(config);
    let jobs = std::sync::Arc::clone(&state.jobs);
    let app = build_router(state);

    // Manually plant a Done job in the store.
    let id = uuid::Uuid::new_v4();
    let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
    jobs.insert(id, tx).unwrap();
    jobs.set_running(id);
    jobs.finish(id, Ok(JobResult { segments: vec![], post_output: None }));
    assert_eq!(jobs.status(id), Some("done"));

    // DELETE should now return 409 Conflict, not 204.
    let req = Request::builder()
        .method(Method::DELETE)
        .uri(format!("/cam/v1/jobs/{id}"))
        .header(header::AUTHORIZATION, "Bearer test-token-aabbccdd")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    // Phase must NOT have been overwritten to Cancelled.
    assert_eq!(jobs.status(id), Some("done"));
}
