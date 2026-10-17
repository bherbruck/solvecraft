//! HTTP route handlers for the solidcraft-cam API.
//!
//! ## Endpoint summary
//! | Method | Path                           | Auth | Description            |
//! |--------|--------------------------------|------|------------------------|
//! | GET    | `/version`                     | no   | version / provenance   |
//! | GET    | `/cam/v1/health`               | yes  | backend availability   |
//! | POST   | `/cam/v1/jobs`                 | yes  | submit a CAM job       |
//! | GET    | `/cam/v1/jobs/:id/status`      | yes  | poll job status        |
//! | GET    | `/cam/v1/jobs/:id/result`      | yes  | retrieve job result    |
//! | DELETE | `/cam/v1/jobs/:id`             | yes  | cancel a job           |

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use solidcraft_manufacture::ManufactureSettings;
use uuid::Uuid;

use crate::{
    config::{CAM_API_VERSION, FREECAD_VERSION_PIN, MAX_STEP_BYTES, OCP_FREECAD_CAM_VERSION_PIN},
    error::CamError,
    freecad, jail,
    jobs::JobResult,
    state::AppState,
};

// ── GET /version ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct VersionResponse {
    pub name: &'static str,
    pub version: &'static str,
    pub api_version: u32,
    pub freecad_pin: &'static str,
    pub ocp_freecad_cam_pin: &'static str,
}

pub async fn version_handler() -> Json<VersionResponse> {
    Json(VersionResponse {
        name: "solidcraft-cam",
        version: env!("CARGO_PKG_VERSION"),
        api_version: CAM_API_VERSION,
        freecad_pin: FREECAD_VERSION_PIN,
        ocp_freecad_cam_pin: OCP_FREECAD_CAM_VERSION_PIN,
    })
}

// ── GET /cam/v1/health ────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub backend_available: bool,
}

pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let backend_available = freecad::probe_backend(&state.config).await;
    Json(HealthResponse { status: if backend_available { "ok" } else { "degraded" }, backend_available })
}

// ── POST /cam/v1/jobs ─────────────────────────────────────────────────────────

/// Request body for job submission.
///
/// Exactly one of `step_b64` (base64-encoded STEP bytes) or `step_text`
/// (raw STEP-21 ASCII) must be provided.
#[derive(Deserialize)]
pub struct SubmitRequest {
    /// Base64-encoded (standard alphabet) STEP file bytes.
    #[serde(default)]
    pub step_b64: Option<String>,
    /// Raw STEP-21 file text (ISO 10303-21 is ASCII).
    #[serde(default)]
    pub step_text: Option<String>,
    /// Manufacture setup(s) to evaluate.
    pub manufacture: ManufactureSettings,
}

#[derive(Serialize)]
pub struct SubmitResponse {
    pub job_id: String,
    pub status: &'static str,
}

pub async fn submit_job_handler(State(state): State<AppState>, Json(body): Json<SubmitRequest>) -> Result<impl IntoResponse, CamError> {
    // Decode STEP bytes from whichever form was provided.
    let step_bytes: Vec<u8> = match (body.step_b64, body.step_text) {
        (Some(b64), None) => {
            use base64::{Engine, engine::general_purpose::STANDARD};
            STANDARD.decode(b64.trim()).map_err(|e| CamError::BadRequest(format!("invalid base64: {e}")))?
        }
        (None, Some(text)) => text.into_bytes(),
        (Some(_), Some(_)) => {
            return Err(CamError::BadRequest("provide exactly one of `step_b64` or `step_text`".into()));
        }
        (None, None) => {
            return Err(CamError::BadRequest("one of `step_b64` or `step_text` is required".into()));
        }
    };

    if step_bytes.len() > MAX_STEP_BYTES {
        return Err(CamError::TooLarge);
    }

    // Create per-job temp directory inside the configured base.
    let job_dir = tempfile::Builder::new().prefix("solidcraft-cam-job-").tempdir_in(&state.config.temp_base).map_err(CamError::Io)?;

    // Verify the temp dir is inside the configured base (path jail sanity check).
    jail::check_in_jail(job_dir.path(), &state.config.temp_base)?;

    let job_id = Uuid::new_v4();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

    state.jobs.insert(job_id, cancel_tx).map_err(|_| CamError::StoreFull)?;

    // Spawn the processing task.
    {
        let jobs = Arc::clone(&state.jobs);
        let config = Arc::clone(&state.config);
        let settings = Arc::new(body.manufacture);
        // Move the TempDir into the task; it auto-cleans up when the task ends.
        let job_dir_path = job_dir.path().to_path_buf();

        tokio::spawn(async move {
            let _job_dir_guard = job_dir; // dropped (and deleted) when task completes
            jobs.set_running(job_id);

            let result = freecad::run_job(config, step_bytes, settings, &job_dir_path, cancel_rx).await;

            match result {
                Ok(r) => jobs.finish(job_id, Ok(r)),
                Err(CamError::Cancelled) => {
                    // cancel() already set the phase; nothing to do.
                }
                Err(e) => jobs.finish(job_id, Err(e.to_string())),
            }
        });
    }

    Ok((StatusCode::ACCEPTED, Json(SubmitResponse { job_id: job_id.to_string(), status: "pending" })))
}

// ── GET /cam/v1/jobs/:id/status ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub job_id: String,
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn job_status_handler(State(state): State<AppState>, Path(id_str): Path<String>) -> Result<Json<StatusResponse>, CamError> {
    let id = Uuid::parse_str(&id_str).map_err(|_| CamError::BadRequest("job_id is not a valid UUID".into()))?;

    let (status, error, _result) = state.jobs.phase_and_result(id).ok_or(CamError::NotFound)?;

    Ok(Json(StatusResponse { job_id: id.to_string(), status, error }))
}

// ── GET /cam/v1/jobs/:id/result ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct ResultResponse {
    pub job_id: String,
    pub status: &'static str,
    pub result: JobResult,
}

pub async fn job_result_handler(State(state): State<AppState>, Path(id_str): Path<String>) -> Result<Json<ResultResponse>, CamError> {
    let id = Uuid::parse_str(&id_str).map_err(|_| CamError::BadRequest("job_id is not a valid UUID".into()))?;

    match state.jobs.phase_and_result(id) {
        None => Err(CamError::NotFound),
        Some(("done", _, Some(result))) => Ok(Json(ResultResponse { job_id: id.to_string(), status: "done", result })),
        Some((status, _, _)) => Err(CamError::NotReady(status)),
    }
}

// ── DELETE /cam/v1/jobs/:id ───────────────────────────────────────────────────

pub async fn cancel_job_handler(State(state): State<AppState>, Path(id_str): Path<String>) -> Result<StatusCode, CamError> {
    let id = Uuid::parse_str(&id_str).map_err(|_| CamError::BadRequest("job_id is not a valid UUID".into()))?;

    match state.jobs.cancel(id) {
        Ok(true) => Ok(StatusCode::NO_CONTENT), // cancel signal sent
        Ok(false) => Err(CamError::Conflict),   // job already terminal
        Err(_) => Err(CamError::NotFound),
    }
}
