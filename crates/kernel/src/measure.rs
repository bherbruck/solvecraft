use serde::Serialize;
use solvecraft_geom::{Aabb3, Mesh, Vec3};

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

/// Per-face sums: area, 6 × signed volume and 24 × first moments (divergence theorem, origin 0).
fn face_sums(m: &Mesh, nf: usize) -> Vec<(f64, f64, Vec3)> {
    let mut out = vec![(0.0, 0.0, Vec3::ZERO); nf];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let (Some([a, b, c]), Some(slot)) = (m.tri(t), out.get_mut(*f as usize)) else { continue };
        let d = a.dot(b.cross(c));
        slot.0 += (b - a).cross(c - a).len() * 0.5;
        slot.1 += d;
        slot.2 += (a + b + c) * d;
    }
    out
}

/// Measure a body from a fine tessellation (chord tolerance 5e-5 of the body size). Faces whose
/// fine tessellation disagrees with a coarse one (a meshing failure) use the coarse result.
pub fn measure(b: &Body) -> Result<BodyMeasure> {
    let size = b.size();
    let fine = b.tessellate((size * 5e-5).max(1e-4))?;
    let coarse = b.tessellate((size * 1e-3).max(1e-3))?;
    let nf = b.face_count();
    let (sf, sc) = (face_sums(&fine, nf), face_sums(&coarse, nf));
    let (mut area, mut v6, mut mom) = (0.0, 0.0, Vec3::ZERO);
    for (f, c) in sf.iter().zip(&sc) {
        let bad = (f.0 - c.0).abs() > 0.01 * c.0.max(f.0) + 1e-9;
        let pick = if bad { c } else { f };
        area += pick.0;
        v6 += pick.1;
        mom += pick.2;
    }
    let volume = v6 / 6.0;
    let centroid = if v6.abs() > 1e-12 { mom / (4.0 * v6) } else { fine.bounds().center() };
    Ok(BodyMeasure {
        volume,
        area,
        centroid,
        bbox: coarse.bounds().union(&fine.bounds()),
        faces: b.face_count(),
        edges: b.edge_count(),
        vertices: b.vertex_count(),
        shells: b.shell_count(),
        merged: merged_topology(b, &coarse)?,
    })
}
