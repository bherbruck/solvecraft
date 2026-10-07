//! The B-rep kernel boundary.
//!
//! Everything SolveCraft does with solids goes through this crate: building bodies from planar
//! regions (extrude, revolve) and primitives, booleans, fillets and chamfers, tessellation,
//! measurement and STEP export. The implementation uses the `truck` crates (Apache-2.0; see
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
mod measure;
mod ops;
mod step;
mod topo;

pub use blend::{chamfer, fillet};
pub use body::{Body, EdgeInfo, FaceInfo};
pub use build::{box_solid, cylinder, extrude, extrude_tapered, revolve, sphere, torus};
pub use measure::{BodyMeasure, measure};
pub use ops::{BoolOp, boolean, split_by_plane, transform, transform_matrix};
pub use step::step_export;
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
