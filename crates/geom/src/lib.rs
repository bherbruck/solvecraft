//! Geometry foundation for SolveCraft: 2D/3D vectors, sketch planes, axis-aligned boxes and
//! triangle meshes with their measures (volume, area, centroid). Units are millimetres.
//!
//! This crate has no knowledge of sketches, documents or the B-rep kernel; every other crate
//! builds on it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod mesh;
mod plane;
mod profile;
mod vec;

pub use mesh::{Aabb3, Mesh, MeshMeasure};
pub use plane::Plane;
pub use profile::{Loop2, MAX_BEZIER_DEGREE, Region2, Seg2, bezier};
pub use vec::{Vec2, Vec3};

/// Geometric tolerance for coincidence tests (mm).
pub const EPS: f64 = 1e-9;
/// Linear tolerance used for "same point" decisions in modelling (mm).
pub const LINEAR_TOL: f64 = 1e-6;

/// `x` if finite, else `fallback`.
pub fn finite_or(x: f64, fallback: f64) -> f64 {
    if x.is_finite() { x } else { fallback }
}

#[cfg(test)]
mod tests;
