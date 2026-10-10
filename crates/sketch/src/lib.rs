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
mod dims;
mod linalg;
mod link;
mod model;
mod offset;
mod profiles;
mod shape;
mod solver;
mod text;
mod wire;

pub use curves::{
    MAX_SPLINE_POINTS, bspline_beziers, conic_point, ellipse_point, end_curvature, fit_beziers, spline_end_tangent, spline_point, spline_polyline,
};
pub use dims::{DimFrame, DimLayout, chain_centres, default_text, dim_frame, dim_layout, encode_text};
pub use link::{Link, LinkGeom, LinkKind, LinkSource, MAX_LINK_ENTITIES};
pub use model::{Constraint, ConstraintKind, Curve, CurveKind, SPoint, Sketch, SketchError, SketchView};
pub use offset::OffsetChain;
pub use profiles::{Profile, find_drawn_profiles, find_profiles, merge_regions};
pub use shape::{Shape, intersections};
pub use solver::{Hold, SolveReport, SolveStatus, measure_dimension, solve, solve_holding};
pub use text::{MAX_TEXT_CHARS, text_geometry};
pub use wire::{MAX_FIT_POINTS, MAX_WIRE_POINTS, Wire, fit_polyline};

#[cfg(test)]
mod tests;
