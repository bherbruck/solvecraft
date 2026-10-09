//! Fillets beyond a constant radius: a radius that varies along the edge (from one end to the
//! other) and chord-length fillets (the width across the blend given instead of the radius).
//!
//! Variable radius: a straight, convex or concave edge between two planar faces whose ends each
//! touch one more planar face square to the edge (the configuration of `blend`). Every section
//! square to the edge is a circular arc tangent to both faces, its radius running linearly from
//! one end to the other; the arcs' control points move linearly, so the blend is exactly a
//! rational B-spline of degree 2 across and 1 along.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::blend::{plane_normal, rebuild_face, unsupported, vtx};
use crate::body::{Body, Solid, p3};
use crate::{KernelError, Result, guard};

/// The radius of a chord-length fillet between faces whose in-face directions (away from the
/// edge) make the angle `phi`: the arc spans π − φ, so the chord is 2 r cos(φ/2).
fn chord_radius(chord: f64, phi: f64) -> f64 {
    chord / (2.0 * (phi / 2.0).cos())
}

/// The two planar faces at an edge, the in-face directions away from it (`t1`, `t2`), the
/// first face's normal, the angle between the directions and the side of the material.
struct EdgeFrame {
    i1: usize,
    i2: usize,
    n1: Vec3,
    t1: Vec3,
    t2: Vec3,
    phi: f64,
    side: f64,
}

fn edge_frame(faces: &[&mt::Face], edge: &mt::Edge) -> Result<EdgeFrame> {
    let on_edge: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.edge_iter().any(|e| e.id() == edge.id()))).collect();
    let [i1, i2] = on_edge[..] else { return Err(unsupported("the edge is not between two faces")) };
    let (Some(f1), Some(f2)) = (faces.get(i1), faces.get(i2)) else { return Err(KernelError::Failed("faces".into())) };
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else { return Err(unsupported("both faces next to the edge must be planar")) };
    let into = |f: &mt::Face, n: Vec3| -> Option<Vec3> {
        let e = f.boundary_iters().into_iter().flatten().find(|e| e.id() == edge.id())?;
        let dir = (vtx(e.back()) - vtx(e.front())).normalized()?;
        n.cross(dir).normalized()
    };
    let (Some(t1), Some(t2)) = (into(f1, n1), into(f2, n2)) else { return Err(KernelError::Failed("blend: edge direction".into())) };
    let phi = t1.dot(t2).clamp(-1.0, 1.0).acos();
    let convex = n1.dot(t2) < -1e-9 && n2.dot(t1) < -1e-9;
    let concave = n1.dot(t2) > 1e-9 && n2.dot(t1) > 1e-9;
    if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) || !(convex || concave) {
        return Err(unsupported("the faces at the edge are tangent or the edge is ambiguous"));
    }
    Ok(EdgeFrame { i1, i2, n1, t1, t2, phi, side: if convex { -1.0 } else { 1.0 } })
}

/// Fillet one straight edge with radius `r0` at its end nearest `start` and `r1` at the other.
pub fn fillet_variable(body: &Body, edge_at: Vec3, start: Vec3, r0: f64, r1: f64) -> Result<Body> {
    fillet_variable_points(body, edge_at, start, &[(0.0, r0), (1.0, r1)], false)
}

/// The radius along the edge as a B-spline of the edge parameter (0 at the edge's front):
/// degree, knots and control radii.
struct RadiusLaw {
    deg: usize,
    knots: Vec<f64>,
    radii: Vec<f64>,
}

impl RadiusLaw {
    /// Through (position, radius) points sorted by position from 0 to 1: straight between them,
    /// or `smooth` (cubic pieces with Catmull-Rom slopes, C¹ through every point).
    fn through(pts: &[(f64, f64)], smooth: bool) -> RadiusLaw {
        if !smooth || pts.len() < 3 {
            let mut knots = vec![0.0];
            knots.extend(pts.iter().map(|p| p.0));
            knots.push(1.0);
            return RadiusLaw { deg: 1, knots, radii: pts.iter().map(|p| p.1).collect() };
        }
        let n = pts.len();
        let at = |i: usize| pts.get(i).copied().unwrap_or((0.0, 0.0));
        let slope = |i: usize| {
            let (a, b) = (at(i.saturating_sub(1)), at((i + 1).min(n - 1)));
            if b.0 > a.0 { (b.1 - a.1) / (b.0 - a.0) } else { 0.0 }
        };
        let mut knots = vec![0.0; 4];
        let mut radii = vec![at(0).1];
        for i in 0..n - 1 {
            let ((t0, r0), (t1, r1)) = (at(i), at(i + 1));
            let h = t1 - t0;
            radii.extend([r0 + slope(i) * h / 3.0, r1 - slope(i + 1) * h / 3.0, r1]);
            knots.extend(if i + 2 == n { vec![1.0; 4] } else { vec![t1; 3] });
        }
        RadiusLaw { deg: 3, knots, radii }
    }

    /// Greville abscissae: where each control radius sits along the edge.
    fn greville(&self) -> Vec<f64> {
        (0..self.radii.len()).map(|j| self.knots.iter().skip(j + 1).take(self.deg).sum::<f64>() / self.deg as f64).collect()
    }

    /// The radius at `t` (sampled for checks).
    fn eval(&self, t: f64) -> f64 {
        let kv = mt::KnotVec::from(self.knots.clone());
        let b = kv.try_bspline_basis_functions(self.deg, t.clamp(0.0, 1.0)).unwrap_or_default();
        b.iter().zip(&self.radii).map(|(x, r)| x * r).sum()
    }
}

/// Fillet one straight edge with a radius that varies along it: (position, radius) points with
/// positions 0 (the end nearest `start`) to 1 (the other end) and any between, the radius
/// straight between them or `smooth`.
pub fn fillet_variable_points(body: &Body, edge_at: Vec3, start: Vec3, points: &[(f64, f64)], smooth: bool) -> Result<Body> {
    body.require_brep("fillet")?;
    if points.len() < 2 || points.len() > 100 {
        return Err(KernelError::Invalid("a variable radius needs 2…100 points".into()));
    }
    for (t, r) in points {
        if !(r.is_finite() && *r > 1e-6 && *r < 1e6) {
            return Err(KernelError::Invalid("fillet radii must be positive".into()));
        }
        if !(t.is_finite() && (0.0..=1.0).contains(t)) {
            return Err(KernelError::Invalid("radius positions run from 0 to 1 along the edge".into()));
        }
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    pts.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
    if pts.first().map(|p| p.0) != Some(0.0) || pts.last().map(|p| p.0) != Some(1.0) || pts.len() < 2 {
        return Err(KernelError::Invalid("give the radius at both ends of the edge".into()));
    }
    let edge = crate::surfaces::edge_near(body, edge_at).ok_or_else(|| KernelError::Invalid(format!("no edge at {edge_at:?}")))?;
    let size = body.size();
    guard("variable fillet", || {
        let solid: &Solid = &body.solid;
        if !matches!(edge.curve(), mt::Curve::Line(_)) {
            return Err(unsupported("only straight edges take a variable radius"));
        }
        let shells = solid.boundaries();
        let (si, shell) = shells
            .iter()
            .enumerate()
            .find(|(_, sh)| sh.edge_iter().any(|e| e.id() == edge.id()))
            .ok_or_else(|| KernelError::Failed("edge not found".into()))?;
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        let fr = edge_frame(&faces, &edge)?;
        let (v0, v1) = (edge.absolute_front().clone(), edge.absolute_back().clone());
        // Positions from the end nearest `start` (the edge keeps its own direction).
        if vtx(&v1).dist(start) < vtx(&v0).dist(start) {
            pts = pts.iter().rev().map(|(t, r)| (1.0 - t, *r)).collect();
        }
        let law = RadiusLaw::through(&pts, smooth);
        let (ra, rb) = (law.eval(0.0), law.eval(1.0));
        // The largest radius along the edge decides whether the blend fits.
        let rmax = (0..=200).map(|k| law.eval(k as f64 / 200.0)).fold(0.0, f64::max);
        if (0..=200).any(|k| law.eval(k as f64 / 200.0) <= 1e-6) {
            return Err(KernelError::Invalid("the radius must stay positive along the edge".into()));
        }
        let (p0, p1) = (vtx(&v0), vtx(&v1));
        let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
        let tol = (size * 1e-7).max(1e-9);
        let back = |r: f64| r / (fr.phi / 2.0).tan();
        // Each end: the end face and its edges along the two faces.
        struct End {
            v: mt::Vertex,
            g: usize,
            e1: mt::Edge,
            e2: mt::Edge,
        }
        let mut ends = Vec::new();
        for (v, p, r) in [(&v0, p0, ra), (&v1, p1, rb)] {
            let at_v: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.vertex_iter().any(|x| &x == v))).collect();
            let others: Vec<usize> = at_v.iter().copied().filter(|i| *i != fr.i1 && *i != fr.i2).collect();
            let [g] = others[..] else { return Err(unsupported("each end of the edge must touch exactly one more face")) };
            let gf = faces.get(g).ok_or_else(|| KernelError::Failed("face".into()))?;
            let ng = plane_normal(gf).ok_or_else(|| unsupported("the faces at the edge ends must be planar"))?;
            if ng.dot(d).abs() < 1.0 - 1e-9 {
                return Err(unsupported("the faces at the edge ends must be perpendicular to the edge"));
            }
            let shared = |fi: usize| -> Result<mt::Edge> {
                let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
                f.edge_iter()
                    .find(|e| e.id() != edge.id() && (e.front() == v || e.back() == v) && gf.edge_iter().any(|x| x.id() == e.id()))
                    .ok_or_else(|| unsupported("unexpected corner topology"))
            };
            let (e1, e2) = (shared(fr.i1)?, shared(fr.i2)?);
            for (e, t) in [(&e1, fr.t1), (&e2, fr.t2)] {
                if !matches!(e.curve(), mt::Curve::Line(_)) {
                    return Err(unsupported("the edges at the corners must be straight"));
                }
                let other = if e.front() == v { vtx(e.back()) } else { vtx(e.front()) };
                let along = (other - p).dot(t);
                if (other - p - t * along).len() > tol * 100.0 + 1e-7 || along <= back(r.max(rmax)) + tol {
                    return Err(unsupported("the blend is larger than the neighbouring faces"));
                }
            }
            ends.push(End { v: v.clone(), g, e1, e2 });
        }
        let [end0, end1] = &ends[..] else { return Err(KernelError::Failed("ends".into())) };
        if end0.g == end1.g {
            return Err(unsupported("both ends touch the same face"));
        }
        let (s0, s1) = (back(ra), back(rb));
        let (pa0, pb0, pa1, pb1) = (p0 + fr.t1 * s0, p0 + fr.t2 * s0, p1 + fr.t1 * s1, p1 + fr.t2 * s1);
        let (a0, b0, a1, b1) = (builder::vertex(p3(pa0)), builder::vertex(p3(pb0)), builder::vertex(p3(pa1)), builder::vertex(p3(pb1)));
        let arc_mid = |p: Vec3, s: f64, r: f64| {
            let c = p + fr.t1 * s + fr.n1 * (r * fr.side);
            c + (p - c).normalized().unwrap_or(-fr.n1) * r
        };
        let c0 = builder::circle_arc(&a0, &b0, p3(arc_mid(p0, s0, ra)));
        let c1 = builder::circle_arc(&a1, &b1, p3(arc_mid(p1, s1, rb)));
        // Contact curves: the edge moved along each face by the setback of the radius there.
        let g = law.greville();
        let contact = |t: Vec3, a: &mt::Vertex, b: &mt::Vertex| -> Result<mt::Edge> {
            if law.deg == 1 && law.radii.len() == 2 {
                return Ok(builder::line(a, b));
            }
            let ctrl: Vec<mt::Point3> = g.iter().zip(&law.radii).map(|(gj, rj)| p3(p0 + (p1 - p0) * *gj + t * back(*rj))).collect();
            let c = mt::BSplineCurve::try_new(mt::KnotVec::from(law.knots.clone()), ctrl).map_err(|e| KernelError::Failed(format!("fillet: {e}")))?;
            mt::Edge::try_new(a, b, mt::Curve::BSplineCurve(c)).map_err(|e| KernelError::Failed(format!("fillet: {e}")))
        };
        let la = contact(fr.t1, &a0, &a1)?;
        let lb = contact(fr.t2, &b0, &b1)?;
        let mut subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for (end, a, b) in [(end0, &a0, &b0), (end1, &a1, &b1)] {
            for (e, nv) in [(&end.e1, a), (&end.e2, b)] {
                let (f, bk) = (e.absolute_front(), e.absolute_back());
                let ne = if f == &end.v { builder::line(nv, bk) } else { builder::line(f, nv) };
                subst.insert(e.id(), ne);
            }
        }
        let mut new_faces: Vec<mt::Face> = Vec::with_capacity(faces.len() + 1);
        for (i, f) in faces.iter().enumerate() {
            let nf = if i == fr.i1 {
                let mut m = subst.clone();
                m.insert(edge.id(), la.clone());
                rebuild_face(f, &m, &[])?
            } else if i == fr.i2 {
                let mut m = subst.clone();
                m.insert(edge.id(), lb.clone());
                rebuild_face(f, &m, &[])?
            } else if i == end0.g {
                rebuild_face(f, &subst, std::slice::from_ref(&c0))?
            } else if i == end1.g {
                rebuild_face(f, &subst, std::slice::from_ref(&c1))?
            } else {
                (*f).clone()
            };
            new_faces.push(nf);
        }
        // The blend surface: rational quadratic arcs (tangent points and the edge point, middle
        // weight sin(φ/2)) at each end, joined linearly.
        let w = (fr.phi / 2.0).sin();
        let h = |p: Vec3, wt: f64| mt::Vector4::new(p.x * wt, p.y * wt, p.z * wt, wt);
        let row = |t: Vec3, wt: f64, off: bool| -> Vec<mt::Vector4> {
            g.iter().zip(&law.radii).map(|(gj, rj)| h(p0 + (p1 - p0) * *gj + if off { t * back(*rj) } else { Vec3::ZERO }, wt)).collect()
        };
        let ctrl = vec![row(fr.t1, 1.0, true), row(fr.t1, w, false), row(fr.t2, 1.0, true)];
        let bsp = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(2), mt::KnotVec::from(law.knots.clone())), ctrl);
        let mut surf = mt::Surface::NurbsSurface(mt::NurbsSurface::new(bsp));
        // Outward: away from the arc centre on a convex edge, toward it on a concave one.
        let (c, mid) = (p0 + fr.t1 * s0 + fr.n1 * (ra * fr.side), arc_mid(p0, s0, ra));
        let outward = (mid - c) * (-fr.side);
        {
            use mt::{ParametricSurface3D, SearchParameter};
            if let Some((u, v)) = surf.search_parameter(p3(mid), None, 100) {
                let nn = surf.normal(u, v);
                if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
                    surf = mt::Invertible::inverse(&surf);
                }
            }
        }
        // The blend face uses each new edge opposite to its neighbour.
        let f1_forward =
            faces.get(fr.i1).is_some_and(|f| f.boundaries().iter().any(|w| w.edge_iter().any(|e| e.id() == edge.id() && e.front() == &v0)));
        let wire: mt::Wire = if f1_forward {
            vec![c0.clone(), lb.clone(), c1.inverse(), la.inverse()].into()
        } else {
            vec![la.clone(), c1.clone(), lb.inverse(), c0.inverse()].into()
        };
        let blend = mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("fillet face: {e}")))?;
        new_faces.push(blend);
        let mut shells = solid.boundaries().clone();
        if let Some(sh) = shells.get_mut(si) {
            *sh = new_faces.into();
        }
        Body::new(Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("variable fillet produced an invalid solid: {e}")))?)
    })
}

/// Chord-length fillet: each edge (straight, between planar faces) rounded so that the blend
/// is `chord` wide across; edges with the same face angle share a radius.
pub fn fillet_chord(body: &Body, edges: &[Vec3], chord: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    if !(chord.is_finite() && chord > 1e-6 && chord < 1e6) {
        return Err(KernelError::Invalid("the chord length must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    // Radius per edge from its face angle.
    let mut groups: Vec<(f64, Vec<Vec3>)> = Vec::new();
    for p in edges {
        let e = crate::surfaces::edge_near(body, *p).ok_or_else(|| KernelError::Invalid(format!("no edge at {p:?}")))?;
        let shell =
            body.solid.boundaries().iter().find(|sh| sh.edge_iter().any(|x| x.id() == e.id())).ok_or_else(|| KernelError::Failed("edge".into()))?;
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        let fr = edge_frame(&faces, &e)?;
        let r = chord_radius(chord, fr.phi);
        match groups.iter_mut().find(|(gr, _)| (gr - r).abs() <= 1e-9 * r) {
            Some((_, v)) => v.push(*p),
            None => groups.push((r, vec![*p])),
        }
    }
    let mut cur = body.clone();
    for (r, pts) in groups {
        cur = crate::fillet(&cur, &pts, r)?;
    }
    Ok(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, measure};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }

    /// The material a variable fillet removes from a right-angled edge of length L, radius
    /// running r0 → r1: ∫ (1 − π/4) r(t)² dt = (1 − π/4) L (r0² + r0 r1 + r1²) / 3.
    #[test]
    fn variable_radius_on_a_box_edge() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // The edge along y at x = 10, z = 30; radius 2 at y = 0, 5 at y = 20.
        let f = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 5.0).unwrap();
        let removed = (1.0 - std::f64::consts::FRAC_PI_4) * 20.0 * (4.0 + 10.0 + 25.0) / 3.0;
        let v = measure(&f).unwrap().volume;
        assert!(rel(v, 6000.0 - removed) < 1e-4, "{v} vs {}", 6000.0 - removed);
        // Started from the other end: the same solid.
        let h = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 20.0, 30.0), 5.0, 2.0).unwrap();
        assert!(rel(measure(&h).unwrap().volume, v) < 1e-9);
        // Equal radii give the constant fillet.
        let g = fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 3.0, 3.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 3.0).unwrap();
        assert!(rel(measure(&g).unwrap().volume, measure(&k).unwrap().volume) < 1e-6);
        // Too big for the faces: refused.
        assert!(fillet_variable(&b, Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0), 2.0, 50.0).is_err());
    }

    /// A radius through a middle point: straight pieces (exact: each piece removes
    /// (1 − π/4) L (a² + ab + b²) / 3), and smooth (the removed volume integrates the law).
    #[test]
    fn variable_radius_through_a_middle_point() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        let (at, start) = (Vec3::new(10.0, 10.0, 30.0), Vec3::new(10.0, 0.0, 30.0));
        let k = 1.0 - std::f64::consts::FRAC_PI_4;
        let pts = [(0.0, 2.0), (0.5, 4.0), (1.0, 2.0)];
        let f = fillet_variable_points(&b, at, start, &pts, false).unwrap();
        let removed = 2.0 * k * 10.0 * (4.0 + 8.0 + 16.0) / 3.0;
        let v = measure(&f).unwrap().volume;
        assert!(rel(v, 6000.0 - removed) < 1e-4, "{v} vs {}", 6000.0 - removed);
        assert_eq!(f.unmeshed_faces(), 0);
        let s = fillet_variable_points(&b, at, start, &pts, true).unwrap();
        let law = RadiusLaw::through(&pts, true);
        let n = 20000;
        let integral: f64 = (0..n).map(|i| law.eval((i as f64 + 0.5) / n as f64).powi(2)).sum::<f64>() / n as f64 * 20.0;
        assert!((law.eval(0.5) - 4.0).abs() < 1e-12 && (law.eval(0.0) - 2.0).abs() < 1e-12);
        let v = measure(&s).unwrap().volume;
        assert!(rel(v, 6000.0 - k * integral) < 1e-4, "{v} vs {}", 6000.0 - k * integral);
        // Unsorted, from the other end: the same.
        let r = fillet_variable_points(&b, at, Vec3::new(10.0, 20.0, 30.0), &[(1.0, 2.0), (0.0, 2.0), (0.5, 4.0)], true).unwrap();
        assert!(rel(measure(&r).unwrap().volume, v) < 1e-9);
        // A middle radius too big for the faces, missing ends, bad positions: refused.
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 40.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (0.5, 4.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (f64::NAN, 4.0), (1.0, 2.0)], false).is_err());
        assert!(fillet_variable_points(&b, at, start, &[(0.0, 2.0), (1.0, -1.0)], false).is_err());
    }

    #[test]
    fn chord_length_on_a_right_angle() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
        // A right angle: chord c gives radius c / √2.
        let f = fillet_chord(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0).unwrap();
        let k = crate::fillet(&b, &[Vec3::new(10.0, 10.0, 30.0)], 4.0 / 2f64.sqrt()).unwrap();
        assert!(rel(measure(&f).unwrap().volume, measure(&k).unwrap().volume) < 1e-9);
    }
}
