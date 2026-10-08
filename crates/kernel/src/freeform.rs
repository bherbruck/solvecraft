//! Free-form profile edges: cubic Béziers as B-spline curves and conics as rational quadratic
//! NURBS curves (exact ellipses, parabolas and hyperbolas from sketches).

use solvecraft_geom::{Plane, Seg2};
use truck_modeling as mt;

use crate::body::p3;

/// The edge for a free-form segment between two existing vertices (None for lines and arcs).
pub(crate) fn edge(plane: &Plane, s: &Seg2, a: &mt::Vertex, b: &mt::Vertex) -> Option<mt::Edge> {
    match *s {
        Seg2::Cubic { p1, p2, .. } => Some(mt::builder::bezier(a, b, vec![p3(plane.to_world(p1)), p3(plane.to_world(p2))])),
        Seg2::Conic { apex, w, .. } => {
            let (pa, pb, px) = (a.point(), b.point(), p3(plane.to_world(apex)));
            let ctrl = vec![
                mt::Vector4::new(pa.x, pa.y, pa.z, 1.0),
                mt::Vector4::new(px.x * w, px.y * w, px.z * w, w),
                mt::Vector4::new(pb.x, pb.y, pb.z, 1.0),
            ];
            let curve = mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), ctrl));
            Some(mt::Edge::new(a, b, mt::Curve::NurbsCurve(curve)))
        }
        Seg2::Line { .. } | Seg2::Arc { .. } => None,
    }
}
