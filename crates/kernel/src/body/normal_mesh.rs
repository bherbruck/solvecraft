//! Shared display boundaries must resolve normal changes as well as curve sag.
use super::*;
use mt::{BoundedCurve, InnerSpace, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};

struct Sampler {
    surface: evaluation::Surface,
    side: Option<(usize, bool)>,
}
impl Sampler {
    fn new(surface: evaluation::Surface, curve: &mt::Curve, tol: f64) -> Self {
        let (lo, hi) = curve.range_tuple();
        let side = (0..4).flat_map(|side| [false, true].map(move |reverse| (side, reverse))).find(|&(side, reverse)| {
            [0.0, 0.25, 0.5, 0.75, 1.0].into_iter().all(|t| {
                let Some((u, v)) = Self::uv(&surface, side, if reverse { 1.0 - t } else { t }) else {
                    return false;
                };
                (surface.subs(u, v) - curve.subs(lo + (hi - lo) * t)).magnitude() <= (tol * 0.01).max(1e-7)
            })
        });
        Self { surface, side }
    }
    fn uv(surface: &evaluation::Surface, side: usize, t: f64) -> Option<(f64, f64)> {
        let (ur, vr) = surface.try_range_tuple();
        let (ur, vr) = (ur?, vr?);
        Some(match side {
            0 => (ur.0 + (ur.1 - ur.0) * t, vr.0),
            1 => (ur.0, vr.0 + (vr.1 - vr.0) * t),
            2 => (ur.0 + (ur.1 - ur.0) * t, vr.1),
            _ => (ur.1, vr.0 + (vr.1 - vr.0) * t),
        })
    }
    fn normal(&self, curve: &mt::Curve, t: f64) -> Option<Vec3> {
        let (u, v) = if let Some((side, reverse)) = self.side {
            let (lo, hi) = curve.range_tuple();
            let fraction = (t - lo) / (hi - lo);
            Self::uv(&self.surface, side, if reverse { 1.0 - fraction } else { fraction })?
        } else {
            self.surface.search_nearest_parameter(curve.subs(t), None, 30)?
        };
        let n = self.surface.normal(u, v);
        Vec3::new(n.x, n.y, n.z).normalized()
    }
}

pub(super) fn shell(source: &mt::Shell, deflection: Deflection) -> MeshedShell {
    let tol = deflection.chord;
    let fallback = || {
        let evaluated: truck_topology::Shell<mt::Point3, mt::Curve, evaluation::Surface> = source
            .face_iter()
            .map(|face| {
                let mut f = truck_topology::Face::new_unchecked(face.absolute_boundaries().clone(), evaluation::Surface(face.surface()));
                if !face.orientation() {
                    f.invert();
                }
                f
            })
            .collect();
        evaluated.robust_triangulation_within(deflection)
    };
    // Endpoint fills use quintic patches. Leave unrelated lofts and sweeps
    // on their existing bounded meshing path.
    if !source.face_iter().any(|f| matches!(f.surface(),mt::Surface::BSplineSurface(s) if s.degrees()==(5,5))) {
        return source.robust_triangulation_within(deflection);
    }
    let mut supports = std::collections::HashMap::new();
    for face in source.face_iter() {
        let surface = face.surface();
        if !matches!(&surface,mt::Surface::BSplineSurface(s) if matches!(s.degrees(),(3,3)|(5,5))) {
            continue;
        }
        for edge in face.edge_iter() {
            supports.entry(edge.id()).or_insert_with(Vec::new).push(Sampler::new(evaluation::Surface(surface.clone()), &edge.curve(), tol));
        }
    }
    if supports.is_empty() {
        return fallback();
    }
    let mut edges = std::collections::HashMap::new();
    for edge in source.edge_iter() {
        edges.entry(edge.id()).or_insert_with(|| {
            let curve = edge.curve();
            // A chord across a tightly curved patch boundary can leave the
            // surface's parameter domain. Interior normal refinement cannot
            // split that shared boundary later, so resolve its sag more closely.
            let boundary_deflection = if supports.contains_key(&edge.id()) {
                Deflection { chord: tol * 0.1, floor: deflection.floor * 0.1, ..deflection }
            } else {
                deflection
            };
            let mut parameters = bounded_curve_parameters(&curve, curve.range_tuple(), boundary_deflection);
            if let Some(samplers) = supports.get(&edge.id()) {
                let mut cache = std::collections::HashMap::new();
                let mut normals =
                    |t: f64| cache.entry(t.to_bits()).or_insert_with(|| samplers.iter().map(|s| s.normal(&curve, t)).collect::<Vec<_>>()).clone();
                for _ in 0..8 {
                    if parameters.len() > 512 {
                        break;
                    }
                    let mut next = Vec::with_capacity(parameters.len() * 2);
                    let mut changed = false;
                    for ab in parameters.windows(2) {
                        let (a, b) = (ab[0], ab[1]);
                        let m = (a + b) * 0.5;
                        next.push(a);
                        let (na, nb, nm) = (normals(a), normals(b), normals(m));
                        let split = na.iter().zip(&nb).zip(&nm).any(|((a, b), m)| {
                            let (Some(a), Some(b), Some(m)) = (a, b, m) else {
                                return false;
                            };
                            (*a + *b).normalized().is_some_and(|n| n.dot(*m) < 1.0_f64.to_radians().cos())
                        });
                        if split && m > a && m < b {
                            next.push(m);
                            changed = true;
                        }
                    }
                    if let Some(last) = parameters.last() {
                        next.push(*last);
                    }
                    parameters = next;
                    if !changed {
                        break;
                    }
                }
            }
            let points = parameters.into_iter().map(|t| curve.subs(t)).collect();
            truck_topology::Edge::new(edge.absolute_front(), edge.absolute_back(), PolylineCurve(points))
        });
    }
    // Preserve the angular mesher's crowded-face work bound. Its fallback
    // relaxes shared curves together, so both incident faces remain watertight.
    if source.face_iter().any(|face| {
        face.edge_iter().filter_map(|e| edges.get(&e.id())).map(|e| e.curve().len().saturating_sub(1)).sum::<usize>() > Deflection::MAX_ANGLE_BOUNDARY
    }) {
        return fallback();
    }
    let evaluated: Option<truck_topology::Shell<mt::Point3, PolylineCurve<mt::Point3>, evaluation::Surface>> = source
        .face_iter()
        .map(|face| {
            let wires: Option<Vec<_>> = face
                .absolute_boundaries()
                .iter()
                .map(|wire| {
                    wire.iter()
                        .map(|edge| {
                            let e = edges.get(&edge.id())?.clone();
                            Some(if edge.orientation() { e } else { e.inverse() })
                        })
                        .collect::<Option<truck_topology::Wire<mt::Point3, PolylineCurve<mt::Point3>>>>()
                })
                .collect();
            let mut face_new = truck_topology::Face::new_unchecked(wires?, evaluation::Surface(face.surface()));
            if !face.orientation() {
                face_new.invert();
            }
            Some(face_new)
        })
        .collect();
    evaluated.and_then(|s| triangulate_presampled(&s, deflection)).unwrap_or_else(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt::Invertible;

    #[test]
    fn a_straight_shared_edge_resolves_both_faces_normal_variation() {
        let patch = |sign: f64| {
            let knots = mt::KnotVec::bezier_knot(5);
            let ctrl = (0..6)
                .map(|i| {
                    (0..6)
                        .map(|j| {
                            let x = if sign > 0.0 { i as f64 / 5.0 } else { 1.0 - i as f64 / 5.0 };
                            let y = sign * j as f64 / 5.0;
                            // z = 10u²(1-u)²y: a straight shared boundary with a
                            // varying tangent plane that chord-only sampling misses.
                            mt::Point3::new(x, y, if i == 2 || i == 3 { y } else { 0.0 })
                        })
                        .collect()
                })
                .collect();
            mt::BSplineSurface::new((knots.clone(), knots), ctrl)
        };
        let a = patch(1.0);
        let b = patch(-1.0);
        let v0 = mt::Vertex::new(a.subs(0.0, 0.0));
        let v1 = mt::Vertex::new(a.subs(1.0, 0.0));
        let shared = mt::Edge::new(&v0, &v1, mt::Curve::BSplineCurve(a.row_curve(0)));
        let face = |patch: mt::BSplineSurface<mt::Point3>, edge: mt::Edge| {
            let v = [edge.front().clone(), edge.back().clone(), mt::Vertex::new(patch.subs(1.0, 1.0)), mt::Vertex::new(patch.subs(0.0, 1.0))];
            let curves = [patch.column_curve(5), patch.row_curve(5).inverse(), patch.column_curve(0).inverse()];
            let wire = std::iter::once(edge)
                .chain((1..4).map(|i| mt::Edge::new(&v[i], &v[(i + 1) % 4], mt::Curve::BSplineCurve(curves[i - 1].clone()))))
                .collect();
            mt::Face::new(vec![wire], mt::Surface::BSplineSurface(patch))
        };
        let source = mt::Shell::from(vec![face(a, shared.clone()), face(b, shared.inverse())]);
        let meshed = shell(&source, Deflection::chord(0.1));
        let faces: Vec<_> = meshed.face_iter().collect();
        let first = faces[0].edge_iter().next().unwrap();
        let second = faces[1].edge_iter().next().unwrap();
        assert_eq!(first.id(), second.id(), "adjacent faces must use one shared boundary");
        assert_ne!(first.orientation(), second.orientation());
        let points = first.curve();
        assert!(points.len() > 4, "normal changes require samples on this straight line: {points:?}");
        assert!(points.len() < 64, "normal sampling must remain bounded");
        assert!(points.iter().all(|p| p.y == 0.0 && p.z == 0.0), "normal refinement must not move the edge");
        assert!(faces.iter().all(|f| f.surface().is_some()));
    }
}
