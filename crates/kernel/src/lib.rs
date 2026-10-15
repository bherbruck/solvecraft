//! The B-rep kernel boundary.
//!
//! Everything SolidCraft does with solids goes through this crate: building bodies from planar
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
mod blend_corner;
mod blend_end;
mod blend_fit;
mod blend_trim;
mod blend_variable;
mod blend_vertex;
mod body;
mod boolean_diagnostics;
mod boolean_guard;
mod boolean_record;
mod bore_guard;
mod build;
mod clearance;
mod coplanar;
mod curveblend;
mod delete_face;
mod emboss;
mod face_params;
#[cfg(test)]
mod face_source_tests;
mod fill;
mod freeform;
pub mod fuzz;
mod heal;
mod helix;
pub mod history;
mod iges_in;
mod iges_out;
#[cfg(test)]
mod intersection_curve_tests;
#[cfg(test)]
mod issue_1080_tests;
#[cfg(test)]
mod issue_616_tests;
#[cfg(test)]
mod issue_674_tests;
#[cfg(test)]
mod issue_679_tests;
#[cfg(test)]
mod issue_788_tests;
#[cfg(test)]
mod issue_851_tests;
#[cfg(test)]
mod issue_871_tests;
#[cfg(test)]
mod join_between_caps_tests;
mod lip;
mod loopblend;
mod measure;
mod mesh_convert;
mod meshbody;
#[cfg(test)]
mod modify_history_tests;
mod native;
mod numbered;
mod offset;
mod ops;
mod outline;
#[cfg(test)]
mod outline_tests;
mod par;
pub use par::{for_each_on_threads, map_on_threads};
#[cfg(test)]
mod pinch_tests;
mod planar_faces;
#[cfg(test)]
mod planar_faces_tests;
mod plane_fit;
mod polybool;
mod polyhedron;
mod prism;
mod prism_curve;
mod prism_measure;
pub mod provenance;
mod record;
mod sew;
mod smooth_edges;
mod spline_conv;
mod splitface;
mod step;
mod step_in;
mod step_out;
mod surface_extend;
mod surface_kind;
mod surface_offset;
mod surface_ruled;
mod surface_untrim;
mod surfaces;
mod sweep_path;
mod tangent_cylinders;
mod tess;
#[cfg(test)]
mod thread_display_tests;
#[cfg(test)]
mod thread_tests;
mod topo;
mod tspline;
#[cfg(test)]
mod untouched_faces_tests;
pub mod watch;

pub use blend::{ChamferSide, chamfer, chamfer_sides, fillet, fillet_each, fillet_g2, fillet_max, fillet_max_each};
pub use blend_corner::fillet_radii;
pub use blend_variable::{RadiusSpline, Taper, fillet_chord, fillet_tapered, fillet_variable, fillet_variable_points, fillet_variable_spline, taper};
pub use body::{Body, CapSignature, DISPLAY_ANGLE, EdgeInfo, FaceInfo, FacePaint, FaceSource, Paint};
pub use boolean_diagnostics::{BooleanDiagnostics, BooleanFallback, BooleanStrategy, capture_boolean_diagnostics};
pub use build::{
    PathSeg, box_solid, canonical_revolve_axis, cylinder, extrude, extrude_tapered, extrude_tapered_between, loft, revolve, sphere, sweep, torus,
};
pub use delete_face::delete_faces;
pub use emboss::{cylinder_emboss_anchor, emboss_cylinder, emboss_cylinder_at, has_cylinder_emboss_wall, infer_cylinder_emboss};
pub use helix::{loft_to_point, sweep_helix, sweep_helix_mesh};
pub use iges_in::{MAX_IGES_BYTES, iges_import, iges_to_step};
pub use iges_out::iges_export_bodies;
pub use lip::raise_band;
pub use measure::{BodyMeasure, measure};
pub use mesh_convert::convert_prismatic_mesh;
pub use meshbody::{MAX_TRIANGLES, mesh_body};
pub use native::{decode_native_body, encode_native_body, native_preview_mesh};
pub use offset::with_same_surface;
pub use ops::{BoolOp, boolean, can_join_touching_faces, join_apart, split_by_plane, transform, transform_matrix, union_apart};
pub use polybool::{faceted_boolean, planar_boolean};
pub use polyhedron::{HalfSpace, convex_polyhedron, draft, offset_faces, shell};
pub use prism::{PLANAR_FIT_TOL, extrude_planar_regions, planar_regions_boolean};
pub use prism_measure::certified_prism_equivalent;
pub use sew::{EdgeSpec, FaceSpec, SurfSpec, sew, sew_touching};
pub use splitface::{SplitTool, split_body, split_faces, split_faces_with_map};
pub use step::step_export;
pub use step_in::{ImportedBody, StepImport, StepNode, step_cache_alive, step_import, step_import_shared, step_orientation_errors, step_validate};
pub use step_out::{ExportBody, ExportProduct, StepHeader, step_export_bodies, step_export_products};
pub use surface_extend::{ExtendLimit, ExtendMode, ExtendOptions, extend_surface};
pub use surface_kind::{FaceSurface, face_surface};
pub use surface_offset::offset_surface;
pub use surface_ruled::{Ruled, RuledCorners, RuledMode, ruled_surface};
pub use surface_untrim::{UntrimHoles, untrim};
pub use surfaces::{
    copy_faces, extend, extrude_sheet, loft_sheet, patch_edges, patch_region, remove_faces, reverse_faces, revolve_sheet, stitch, sweep_sheet,
    sweep_sheet_points, thicken, trim, unstitch,
};
pub use sweep_path::sweep_path;
pub use topo::{CylinderFace, cylinder_face_at};
pub use topo::{TopoCounts, merged_topology, seam_flags};
pub use tspline::tspline_sheet;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("the operation failed: {0}")]
    Failed(String),
    /// A blend or other size too large for the geometry around it (a smaller size may work);
    /// shown like any feature that isn't supported yet.
    #[error("the operation failed: not supported yet: {0}")]
    TooLarge(String),
    /// The third-party kernel panicked; the model is unchanged.
    #[error("kernel internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;

thread_local! {
    static GUARDED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Is this thread inside [`guard`]? (A panic now will be caught and turned into an error; the
/// engine's crash reports mark it recovered.)
pub fn guarded() -> bool {
    GUARDED.with(std::cell::Cell::get) > 0
}

/// Run third-party kernel code, converting a panic into an error.
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
        }
    }
    GUARDED.with(|g| g.set(g.get() + 1));
    let depth = Depth;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    drop(depth);
    match r {
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
