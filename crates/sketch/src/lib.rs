//! Sketches: points and curves on a plane, geometric constraints and driving dimensions, a
//! Levenberg–Marquardt constraint solver with degree-of-freedom analysis, and closed-profile
//! (region) detection for features.
//!
//! The solver is our own clean-room implementation (residual functions per constraint, local
//! finite-difference Jacobians, damped Gauss–Newton per connected component, rank analysis by
//! Gaussian elimination). See `plan/architecture.md` §sketch.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod linalg;
mod link;
mod model;
mod profiles;
mod solver;

pub use link::{Link, LinkGeom, LinkKind, LinkSource, MAX_LINK_ENTITIES};
pub use model::{Constraint, ConstraintKind, Curve, CurveKind, SPoint, Sketch, SketchError};
pub use profiles::{Profile, find_drawn_profiles, find_profiles, merge_regions};
pub use solver::{SolveReport, SolveStatus, solve};

#[cfg(test)]
mod tests;
