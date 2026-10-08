//! The B-rep kernel boundary.
//!
//! Everything SolveCraft does with solids goes through this crate: building bodies from planar
//! regions (extrude, revolve) and primitives, booleans, fillets and chamfers, tessellation,
//! measurement and STEP export and import. The implementation uses the `truck` crates (Apache-2.0; see
//! `plan/adr/0001-geometry-kernel.md`) plus our own local operations (edge fillets and
//! chamfers in `blend.rs`), but no truck type appears in the public API, so the kernel can be
//! replaced without touching the rest of the workspace.
//!
//! Every call into the third-party kernel runs under [`guard`]: a panic inside it becomes a
//! [`KernelError::Internal`] instead of taking the application down.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod blend;
mod body;
mod build;
mod coplanar;
mod curveblend;
mod delete_face;
mod freeform;
mod heal;
mod helix;
mod iges_in;
mod iges_out;
mod loopblend;
mod measure;
mod meshbody;
mod offset;
mod ops;
mod polybool;
mod polyhedron;
mod prism;
pub mod provenance;
mod sew;
mod splitface;
mod step;
mod step_in;
mod step_out;
mod surfaces;
mod sweep_path;
mod topo;

pub use blend::{ChamferSide, chamfer, chamfer_sides, fillet};
pub use body::{Body, EdgeInfo, FaceInfo, FacePaint, Paint};
pub use build::{PathSeg, box_solid, cylinder, extrude, extrude_tapered, loft, revolve, sphere, sweep, torus};
pub use delete_face::delete_faces;
pub use helix::{loft_to_point, sweep_helix};
pub use iges_in::{MAX_IGES_BYTES, iges_import, iges_to_step};
pub use iges_out::iges_export_bodies;
pub use measure::{BodyMeasure, measure};
pub use meshbody::{MAX_TRIANGLES, mesh_body};
pub use ops::{BoolOp, boolean, split_by_plane, transform, transform_matrix};
pub use polybool::planar_boolean;
pub use polyhedron::{HalfSpace, convex_polyhedron, draft, offset_faces, shell};
pub use sew::{EdgeSpec, FaceSpec, SurfSpec, sew};
pub use splitface::{SplitTool, split_body, split_faces, split_faces_with_map};
pub use step::step_export;
pub use step_in::{ImportedBody, StepImport, StepNode, step_import, step_import_shared, step_orientation_errors, step_validate};
pub use step_out::{ExportBody, ExportProduct, StepHeader, step_export_bodies, step_export_products};
pub use surfaces::{copy_faces, extend, patch_edges, patch_region, stitch, thicken, trim};
pub use sweep_path::sweep_path;
pub use topo::{CylinderFace, cylinder_face_at};
pub use topo::{TopoCounts, merged_topology, seam_flags};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("the operation failed: {0}")]
    Failed(String),
    /// The third-party kernel panicked; the model is unchanged.
    #[error("kernel internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// Run third-party kernel code, converting a panic into an error.
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::warn!("kernel panic in {what}: {msg}");
            Err(KernelError::Internal(format!("{what}: {msg}")))
        }
    }
}

#[cfg(test)]
mod tests;
