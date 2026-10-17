//! Preflight, start, status, cancel, result, and export handlers for manufacture jobs.
//!
//! Runtime commands: they never touch the document, the undo stack, or any model evaluation.
//! [`preflight`] and [`start`] extract an immutable STEP snapshot and reject stale or empty
//! geometry; all subsequent commands operate on the captured identity.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_manufacture::ManufactureSettings;
use solidcraft_manufacture::job::{CamJobId, CamJobInput, CamJobResult};
use solidcraft_manufacture::post;

use crate::manufacture_jobs::job_error;
use crate::params::bad;
use crate::{Result, Session};

// ── Snapshot extraction ────────────────────────────────────────────────────────

struct Snapshot {
    setup_name: String,
    settings: Arc<ManufactureSettings>,
    step_bytes: Arc<Vec<u8>>,
    step_sha256: String,
    operation_count: usize,
}

/// Extract and validate the STEP snapshot for the named setup.  Rejects stale or empty
/// geometry before any queuing work happens — this is the common body shared by preflight and
/// start.
fn take_snapshot(s: &Session, setup_name: &str, command: &str) -> Result<Snapshot> {
    ManufactureSettings::validate_name(setup_name).map_err(|e| bad(command, e.to_string()))?;

    let setup = s.doc.manufacture.setups.get(setup_name).ok_or_else(|| bad(command, format!("no manufacture setup named `{setup_name}`")))?;

    if setup.operations.is_empty() {
        return Err(bad(command, "setup has no operations"));
    }
    let operation_count = setup.operations.len();
    let mut settings = ManufactureSettings::default();
    settings.setups.insert(setup_name.to_owned(), setup.clone());
    if let Some(name) = &setup.machine {
        let profile = s
            .doc
            .manufacture
            .machines
            .get(name)
            .cloned()
            .ok_or_else(|| bad(command, format!("setup references missing machine profile `{name}`")))?;
        settings.machines.insert(name.clone(), profile);
    }
    if let Some(name) = &setup.post {
        let profile =
            s.doc.manufacture.posts.get(name).cloned().ok_or_else(|| bad(command, format!("setup references missing post profile `{name}`")))?;
        settings.posts.insert(name.clone(), profile);
    }
    settings.validate().map_err(|e| bad(command, e.to_string()))?;

    // Model must be settled — a live gesture's stale geometry is rejected.
    if s.model.is_stale() {
        return Err(bad(command, "model geometry is stale; finish the modelling gesture before running manufacture"));
    }

    let local = s.model.state();
    if local.bodies.is_empty() {
        return Err(bad(command, "design has no bodies to export as a model snapshot"));
    }

    // Export the model as STEP (immutable snapshot).  Uses the local component-frame geometry
    // plus the doc's occurrence placements to produce a proper STEP assembly.
    let step_bytes =
        solidcraft_io::step_assembly(&s.doc, &s.doc.painted(&local), &s.doc.name).map_err(|e| bad(command, format!("STEP snapshot failed: {e}")))?;

    if step_bytes.is_empty() {
        return Err(bad(command, "STEP snapshot produced no output"));
    }

    let mut hasher = Sha256::new();
    hasher.update(&step_bytes);
    let step_sha256 = format!("{:x}", hasher.finalize());

    Ok(Snapshot { setup_name: setup_name.to_owned(), settings: Arc::new(settings), step_bytes: Arc::new(step_bytes), step_sha256, operation_count })
}

// ── Preflight ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreflightParams {
    setup: String,
}

pub(super) fn preflight(s: &mut Session, params: &Value) -> Result<Value> {
    let p: PreflightParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.preflight", e.to_string()))?;
    let snap = take_snapshot(s, &p.setup, "manufacture.preflight")?;
    let host_available = s.manufacture_jobs.host.as_ref().is_some_and(|h| h.is_available());
    Ok(json!({
        "setup": snap.setup_name,
        "operations": snap.operation_count,
        "step_bytes": snap.step_bytes.len(),
        "step_sha256": snap.step_sha256,
        "scene_generation": s.revision,
        "host_available": host_available,
    }))
}

// ── Start ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartParams {
    setup: String,
    #[serde(default)]
    replace: bool,
}

pub(super) fn start(s: &mut Session, params: &Value) -> Result<Value> {
    let p: StartParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.start", e.to_string()))?;
    let snap = take_snapshot(s, &p.setup, "manufacture.start")?;
    let cam_input = CamJobInput {
        setup: snap.setup_name,
        settings: snap.settings,
        step_bytes: snap.step_bytes,
        scene_generation: s.revision,
        step_sha256: snap.step_sha256,
    };
    let status = s.manufacture_jobs.submit(cam_input, p.replace).map_err(job_error)?;
    Ok(json!(status))
}

// ── Status / Cancel ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobParams {
    job: CamJobId,
}

pub(super) fn status(s: &mut Session, params: &Value) -> Result<Value> {
    let p: JobParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.status", e.to_string()))?;
    Ok(json!(s.manufacture_jobs.status(p.job).map_err(job_error)?))
}

pub(super) fn cancel(s: &mut Session, params: &Value) -> Result<Value> {
    let p: JobParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.cancel", e.to_string()))?;
    let current = s.manufacture_jobs.status(p.job).map_err(job_error)?;
    if current.progress.phase.terminal() {
        return Ok(json!(current));
    }
    let handle = s.manufacture_jobs.handle(p.job).map_err(job_error)?;
    Ok(json!(handle.cancel().map_err(job_error)?))
}

// ── Result ────────────────────────────────────────────────────────────────────

pub(super) fn result(s: &mut Session, params: &Value) -> Result<Value> {
    let p: JobParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.result", e.to_string()))?;
    let cam_result = s.manufacture_jobs.result(p.job).map_err(job_error)?;
    // Validate all segments on the way out (provenenance guarantee).
    for (i, seg) in cam_result.segments.iter().enumerate() {
        seg.validate().map_err(|e| bad("manufacture.result", format!("segment[{i}] invalid: {e}")))?;
    }
    Ok(json!({
        "job": cam_result.job,
        "provenance": cam_result.provenance,
        "segments": cam_result.segments,
        "post_output": cam_result.post_output,
    }))
}

// ── Export ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportParams {
    job: CamJobId,
    path: String,
    #[serde(default)]
    overwrite: bool,
}

fn gcode_bytes_from_result(s: &Session, cam_result: &CamJobResult) -> Result<Vec<u8>> {
    if let Some(gcode) = &cam_result.post_output {
        return Ok(gcode.as_bytes().to_vec());
    }
    let setup = s
        .doc
        .manufacture
        .setups
        .get(&cam_result.provenance.setup)
        .ok_or_else(|| bad("manufacture.gcode", "setup referenced by job provenance no longer exists"))?;
    let post_name =
        setup.post.as_ref().ok_or_else(|| bad("manufacture.gcode", "setup has no post profile; assign one or use companion post output"))?;
    let profile =
        s.doc.manufacture.posts.get(post_name).ok_or_else(|| bad("manufacture.gcode", format!("post profile `{post_name}` is not defined")))?;
    let tool = setup.tools.first().ok_or_else(|| bad("manufacture.gcode", "setup has no tools for local posting"))?;
    let gcode = post::post_toolpath(&cam_result.segments, profile, tool).map_err(|e| bad("manufacture.gcode", e))?;
    Ok(gcode.into_bytes())
}

pub(super) fn gcode(s: &mut Session, params: &Value) -> Result<Value> {
    let p: JobParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.gcode", e.to_string()))?;
    let cam_result = s.manufacture_jobs.result(p.job).map_err(job_error)?;
    let source = if cam_result.post_output.is_some() { "companion" } else { "local" };
    let bytes = gcode_bytes_from_result(s, &cam_result)?;
    let text = String::from_utf8(bytes).map_err(|e| bad("manufacture.gcode", format!("gcode is not UTF-8: {e}")))?;
    Ok(json!({
        "job": cam_result.job,
        "provenance": cam_result.provenance,
        "source": source,
        "gcode": text,
    }))
}

pub(super) fn export(s: &mut Session, params: &Value) -> Result<Value> {
    let p: ExportParams = serde_json::from_value(params.clone()).map_err(|e| bad("manufacture.export", e.to_string()))?;

    // Bounded path: non-empty, no control characters, max 4096 bytes.
    if p.path.is_empty() || p.path.len() > 4096 || p.path.chars().any(char::is_control) {
        return Err(bad("manufacture.export", "a bounded destination path is required (no control characters, ≤ 4096 bytes)"));
    }
    // VFS path-scope check.
    solidcraft_io::vfs::check(&p.path).map_err(|e| bad("manufacture.export", e.to_string()))?;

    let cam_result = s.manufacture_jobs.result(p.job).map_err(job_error)?;

    // G-code when companion or local post can produce it; otherwise canonical segment JSON.
    let content: Vec<u8> = match gcode_bytes_from_result(s, &cam_result) {
        Ok(bytes) => bytes,
        Err(_) => serde_json::to_vec_pretty(&json!({
            "provenance": cam_result.provenance,
            "segments": cam_result.segments,
        }))
        .map_err(|e| bad("manufacture.export", format!("could not serialise segments: {e}")))?,
    };

    let path = std::path::Path::new(&p.path);
    let write_result =
        if p.overwrite { solidcraft_io::write_atomic(path, &content, false) } else { solidcraft_io::safe_write::create_atomic(path, &content) };
    write_result.map_err(|e| bad("manufacture.export", format!("could not export: {e}")))?;

    Ok(json!({
        "job": p.job,
        "path": p.path,
        "bytes": content.len(),
        "provenance": cam_result.provenance,
    }))
}
