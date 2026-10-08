//! Sketches: points and curves on a plane, geometric constraints and driving dimensions, a
//! Levenberg–Marquardt constraint solver with degree-of-freedom analysis, and closed-profile
//! (region) detection for features.
//!
//! The solver is our own clean-room implementation (residual functions per constraint, local
//! finite-difference Jacobians, damped Gauss–Newton per connected component, rank analysis by
//! Gaussian elimination). See `plan/architecture.md` §sketch.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod curves;
mod linalg;
mod link;
mod model;
mod profiles;
mod shape;
mod solver;
mod text;
mod wire;

pub use curves::{
    MAX_SPLINE_POINTS, bspline_beziers, conic_point, ellipse_point, end_curvature, fit_beziers, spline_end_tangent, spline_point, spline_polyline,
};
pub use link::{Link, LinkGeom, LinkKind, LinkSource, MAX_LINK_ENTITIES};
pub use model::{Constraint, ConstraintKind, Curve, CurveKind, SPoint, Sketch, SketchError};
pub use profiles::{Profile, find_drawn_profiles, find_profiles, merge_regions};
pub use shape::{Shape, intersections};
pub use solver::{SolveReport, SolveStatus, solve};
pub use text::{MAX_TEXT_CHARS, text_geometry};
pub use wire::{MAX_WIRE_POINTS, Wire};

#[cfg(test)]
mod tests;
