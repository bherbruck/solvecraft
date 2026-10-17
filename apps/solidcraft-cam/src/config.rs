//! Compile-time constants and runtime configuration for solidcraft-cam.

use std::path::PathBuf;

use crate::origin::ParsedOrigin;

// ── Pinned external dependencies ──────────────────────────────────────────────

/// FreeCAD version this adapter is written and tested against.
/// Run `freecadcmd --version` to verify the installed version matches.
pub const FREECAD_VERSION_PIN: &str = "1.1.1";

/// ocp-freecad-cam PyPI package version this adapter targets.
/// Install: `pip install ocp-freecad-cam==2.0.2` in the FreeCAD Python environment.
/// Also requires: `cadquery` (2.4+) in the same environment.
pub const OCP_FREECAD_CAM_VERSION_PIN: &str = "2.0.2";

// ── API metadata ──────────────────────────────────────────────────────────────

/// Monotonically-increasing integer; bumped on breaking HTTP API changes.
pub const CAM_API_VERSION: u32 = 1;

// ── Size limits ───────────────────────────────────────────────────────────────

/// Maximum size of an incoming HTTP request body (64 MiB).
pub const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;

/// Maximum STEP file size accepted (50 MiB).
/// Larger files are rejected before writing to disk.
pub const MAX_STEP_BYTES: usize = 50 * 1024 * 1024;

/// Maximum result payload returned to the client (10 MiB).
pub const MAX_RESULT_BYTES: usize = 10 * 1024 * 1024;

/// Maximum number of jobs kept in memory at once.
pub const MAX_STORED_JOBS: usize = 256;

// ── Timeouts ──────────────────────────────────────────────────────────────────

/// Default per-job subprocess timeout (5 minutes).
pub const DEFAULT_JOB_TIMEOUT_SECS: u64 = 300;

/// Backend probe timeout (seconds); intentionally short.
pub const PROBE_TIMEOUT_SECS: u64 = 15;

// ── Runtime configuration ─────────────────────────────────────────────────────

/// Runtime configuration resolved from CLI flags at startup.
#[derive(Debug, Clone)]
pub struct CamConfig {
    /// The ephemeral bearer token (printed to stdout at launch; never logged).
    pub token: String,
    /// Origins permitted by the CORS / PNA policy.
    /// An empty list means CORS headers are never added (same-origin loopback only).
    pub allowed_origins: Vec<ParsedOrigin>,
    /// Base directory under which per-job temp workspaces are created.
    pub temp_base: PathBuf,
    /// Executable used to run the Python adapter
    /// (e.g. `python3`, `freecadcmd`, or an absolute path).
    pub freecad_exec: String,
    /// Absolute path to `freecad_cam_adapter.py` shipped alongside this binary.
    pub adapter_path: PathBuf,
    /// Per-job subprocess hard-kill timeout in seconds.
    pub job_timeout_secs: u64,
}
