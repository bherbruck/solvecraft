//! The SolveCraft document: user and model parameters, and a parametric timeline of features
//! (sketches, extrudes, revolves, fillets, primitives, combines…). Evaluating the timeline with
//! the kernel produces the model (bodies and solved sketches). Evaluation is incremental: a
//! feature is recomputed only when it, a parameter it uses, or an earlier feature changed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod assembly;
mod clipboard;
mod document;
mod eval;
pub mod expr;
mod params;
pub mod project;
mod project3d;

pub use assembly::{IDENTITY, Mat, Occurrence, apply_point, apply_vector, is_identity, mat_inverse, mat_mul, rigid};
pub use document::*;
pub use eval::{FeatureResult, Model, ModelBody, ModelState, SolvedSketch, ThreadInfo, parse_metric_thread, world_state};
pub use params::{ParamRow, ParamUser, cycles};
pub use solvecraft_kernel as kernel;
pub use solvecraft_sketch as sketch;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DocError {
    #[error("expression: {0}")]
    Expr(String),
    #[error("{0}")]
    Invalid(String),
    #[error("unknown {0}")]
    Unknown(String),
    #[error(transparent)]
    Sketch(#[from] solvecraft_sketch::SketchError),
    #[error(transparent)]
    Kernel(#[from] solvecraft_kernel::KernelError),
}

pub type Result<T> = std::result::Result<T, DocError>;

#[cfg(test)]
mod tests;
