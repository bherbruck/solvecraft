//! FreeCAD / ocp-freecad-cam subprocess adapter runner.
//!
//! Spawns `freecad_cam_adapter.py` as a child process, feeds it a JSON payload
//! over stdin, and reads the JSON result from stdout.  The child is killed on
//! cancellation or timeout.
//!
//! # No shell interpolation
//! The executable and adapter path come from validated `CamConfig` fields.
//! They are passed as separate `Command` arguments — never concatenated into a
//! shell string — so no injection is possible.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use solidcraft_manufacture::{ManufactureSettings, PathSegment};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::{CamConfig, PROBE_TIMEOUT_SECS};
use crate::error::CamError;
use crate::jail::jail_child;
use crate::jobs::JobResult;

// ── Wire types (adapter protocol) ─────────────────────────────────────────────

/// JSON payload sent to the adapter on stdin.
#[derive(Serialize)]
struct AdapterInput<'a> {
    step_path: String,
    manufacture: &'a ManufactureSettings,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    probe: bool,
}

/// JSON response read from the adapter on stdout.
#[derive(Deserialize)]
struct AdapterOutput {
    status: String,
    #[serde(default)]
    segments: Vec<PathSegment>,
    #[serde(default)]
    post_output: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Returns `true` if the adapter can be spawned and imports succeed within the
/// probe timeout.  Errors are silently swallowed — caller logs if needed.
pub async fn probe_backend(config: &Arc<CamConfig>) -> bool {
    let input = AdapterInput { step_path: String::new(), manufacture: &ManufactureSettings::default(), probe: true };
    let input_json = match serde_json::to_vec(&input) {
        Ok(j) => j,
        Err(_) => return false,
    };
    let result = tokio::time::timeout(Duration::from_secs(PROBE_TIMEOUT_SECS), run_adapter(config, &input_json)).await;
    match result {
        Ok(Ok(out)) => out.status != "unavailable",
        _ => false,
    }
}

/// Run a CAM job: write the STEP file into the job workspace, call the adapter,
/// return validated [`JobResult`].
///
/// `cancel_rx` fires when the client sends DELETE; the child process is killed.
pub async fn run_job(
    config: Arc<CamConfig>,
    step_bytes: Vec<u8>,
    settings: Arc<ManufactureSettings>,
    job_dir: &Path,
    cancel_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<JobResult, CamError> {
    // Write STEP into the jail.
    let step_path = jail_child(job_dir, "model.step")?;
    tokio::fs::write(&step_path, &step_bytes).await?;

    let input = AdapterInput { step_path: step_path.to_string_lossy().into_owned(), manufacture: &settings, probe: false };
    let input_json = serde_json::to_vec(&input)?;

    let timeout = Duration::from_secs(config.job_timeout_secs);

    let adapter_fut = run_adapter(&config, &input_json);

    tokio::select! {
        r = tokio::time::timeout(timeout, adapter_fut) => {
            match r {
                Ok(inner) => map_adapter_output(inner?),
                Err(_) => Err(CamError::Internal("subprocess timed out".into())),
            }
        }
        _ = cancel_rx => Err(CamError::Cancelled),
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

async fn run_adapter(config: &CamConfig, input_json: &[u8]) -> Result<AdapterOutput, CamError> {
    let mut child = Command::new(&config.freecad_exec)
        .arg(&config.adapter_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| CamError::BackendUnavailable(format!("could not spawn `{}`: {}", config.freecad_exec, e)))?;

    // Write input and close stdin so the adapter sees EOF.
    {
        let mut stdin = child.stdin.take().ok_or_else(|| CamError::Internal("no stdin handle".into()))?;
        stdin.write_all(input_json).await?;
        // stdin dropped here → pipe EOF
    }

    let output = child.wait_with_output().await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(CamError::Internal(format!("adapter exited {}: {}", output.status, stderr.trim())));
    }

    serde_json::from_slice::<AdapterOutput>(&output.stdout).map_err(|e| CamError::Internal(format!("adapter returned invalid JSON: {e}")))
}

fn map_adapter_output(out: AdapterOutput) -> Result<JobResult, CamError> {
    match out.status.as_str() {
        "ok" => {
            // Validate each segment.
            for seg in &out.segments {
                seg.validate().map_err(|e| CamError::Internal(e.to_string()))?;
            }
            Ok(JobResult { segments: out.segments, post_output: out.post_output })
        }
        "unavailable" => Err(CamError::BackendUnavailable(out.message.unwrap_or_else(|| "FreeCAD unavailable".into()))),
        "preflight_error" => Err(CamError::PreflightFailed(out.message.unwrap_or_else(|| "operation cannot be configured from domain data".into()))),
        _ => Err(CamError::Internal(out.message.unwrap_or_else(|| "adapter error".into()))),
    }
}
