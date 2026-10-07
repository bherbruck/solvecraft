use serde::Serialize;
use solvecraft_geom::{Aabb3, Vec3};

use crate::Result;
use crate::body::Body;
use crate::topo::{TopoCounts, merged_topology};

/// Measured properties of a body.
#[derive(Clone, Debug, Serialize)]
pub struct BodyMeasure {
    pub volume: f64,
    pub area: f64,
    pub centroid: Vec3,
    pub bbox: Aabb3,
    /// Raw kernel topology counts (closed curves and surfaces may be split in pieces).
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub shells: usize,
    /// Counts after merging split pieces of the same analytic surface/curve (comparable with
    /// systems that use periodic faces and edges).
    pub merged: TopoCounts,
}

/// Measure a body from a fine tessellation (chord tolerance 5e-5 of the body size). Curved
/// faces are slightly under-estimated (relative error typically below 2e-4).
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    let size = b.size();
    let m2 = b.tessellate((size * 5e-5).max(1e-4))?;
    let a2 = m2.measure();
    Ok(BodyMeasure {
        volume: a2.volume,
        area: a2.area,
        centroid: a2.centroid,
        bbox: m2.bounds(),
        faces: b.face_count(),
        edges: b.edge_count(),
        vertices: b.vertex_count(),
        shells: b.shell_count(),
        merged: merged_topology(b, &m2)?,
    })
}
