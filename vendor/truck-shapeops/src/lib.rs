//! Crate for operationg shapes. Provides boolean operations to Solid, and shape healing for importing shapes from other CAD systems.

// SolveCraft: warnings are not errors in the vendored copy (newer compilers flag more).
#![allow(dead_code)]
#![warn(clippy::all, rust_2018_idioms)]
#![warn(
    missing_docs,
    missing_debug_implementations,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]

/// SolveCraft: report where an operation gives up (set `SHAPEOPS_TRACE`).
macro_rules! tr {
    ($e:expr, $tag:expr) => {
        match $e {
            Some(v) => v,
            None => {
                if std::env::var_os("SHAPEOPS_TRACE").is_some() {
                    eprintln!("shapeops: gave up at {} ({}:{})", $tag, file!(), line!());
                }
                return None;
            }
        }
    };
}
pub(crate) use tr;

mod healing;
pub use healing::{RobustSplitClosedEdgesAndFaces, SplitClosedEdgesAndFaces};
mod transversal;
pub use transversal::{and, or, ShapeOpsCurve, ShapeOpsSurface};
mod alternative;
