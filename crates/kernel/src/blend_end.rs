//! Straight fillets trimmed by the fan of faces at each endpoint.
//! The cylinder stays circular; oblique planes cut ellipses, not tilted circles.

use crate::blend::{plane_normal, unsupported, vtx};
use crate::body::{Solid, from_p3, p3};
use crate::{KernelError, Result};
use solidcraft_geom::Vec3;
use std::collections::HashMap;
use truck_modeling::{
    self as mt, BoundedCurve, Cut, Invertible, ParameterDivision1D, ParametricCurve, ParametricSurface3D, SearchNearestParameter, builder,
};

mod transition;

fn h(p: Vec3, w: f64) -> mt::Vector4 {
    mt::Vector4::new(p.x * w, p.y * w, p.z * w, w)
}
pub(super) fn conic(a: Vec3, m: Vec3, b: Vec3, w: f64) -> mt::Curve {
    mt::Curve::NurbsCurve(mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), vec![h(a, 1.0), h(m, w), h(b, 1.0)])))
}
pub(super) fn trim(mut c: mt::Curve, lo: f64, hi: f64) -> mt::Curve {
    let (a, b) = c.range_tuple();
    if hi < b {
        let _ = c.cut(hi);
    }
    if lo > a {
        c = c.cut(lo);
    }
    c
}

/// First crossing of zero along a seam, short of either original endpoint.
fn crossing(c: &mt::Curve, f: impl Fn(Vec3) -> f64, tol: f64) -> Result<(f64, Vec3)> {
    let (a, b) = c.range_tuple();
    let mut prev = (a, f(from_p3(c.subs(a))));
    if !prev.1.is_finite() {
        return Err(unsupported("an invalid fillet intersection"));
    }
    for i in 1..=128 {
        let t = a + (b - a) * i as f64 / 128.0;
        let y = f(from_p3(c.subs(t)));
        if !y.is_finite() {
            return Err(unsupported("an invalid fillet intersection"));
        }
        if prev.1 * y <= 0.0 {
            let (mut lo, mut hi) = (prev.0, t);
            for _ in 0..60 {
                let mid = (lo + hi) * 0.5;
                let y = f(from_p3(c.subs(mid)));
                if !y.is_finite() {
                    return Err(unsupported("an invalid fillet intersection"));
                }
                if y * prev.1 > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let t = (lo + hi) * 0.5;
            let q = from_p3(c.subs(t));
            if q.dist(from_p3(c.subs(a))) <= tol || q.dist(from_p3(c.subs(b))) <= tol {
                return Err(unsupported("a fillet reaching the next vertex"));
            }
            return Ok((t, q));
        }
        prev = (t, y);
    }
    Err(unsupported("the fillet is larger than its neighbouring faces"))
}

/// Only an inward dihedral joint can extend an uncut fan seam backwards.
/// Use the seam as oriented by its first face, so reversing the solid changes
/// the classification even though its supporting planes are unchanged.
fn reflex_joint(a: &mt::Face, b: &mt::Face, id: mt::EdgeID) -> bool {
    let (Some(na), Some(nb)) = (plane_normal(a), plane_normal(b)) else { return false };
    let Some(e) = a.boundary_iters().into_iter().flatten().find(|e| e.id() == id) else { return false };
    let Some(t) = (vtx(e.back()) - vtx(e.front())).normalized() else { return false };
    t.dot(na.cross(nb)) < -1e-6
}

/// A plane cuts an exact elliptical section out of the circular cylinder.
pub(super) fn on_end(f: &mt::Face, q: Vec3, d: Vec3, p: Vec3) -> Result<Vec3> {
    let n = plane_normal(f).ok_or_else(|| unsupported("this fillet needs rebuilding where it meets an earlier curved blend"))?;
    let den = n.dot(d);
    if den.abs() < 1e-8 {
        return Err(unsupported("an end face parallel to the fillet"));
    }
    Ok(q - d * (n.dot(q - p) / den))
}

/// A cylinder/cylinder intersection, parameterized by the new fillet's angular
/// section. Fit cubic Hermite spans adaptively instead of tilting a circular arc.
pub(super) fn curved_section(f: &mt::Face, base: &mt::Curve, d: Vec3, lo: f64, hi: f64, start: Vec3, end: Vec3, tol: f64) -> Result<mt::Curve> {
    use crate::offset::{Surf, surf_of};
    let Some(Surf::Cylinder { o, a, r, .. }) = surf_of(f, tol) else {
        return Err(unsupported("this end face is not a plane or cylinder"));
    };
    let dd = d - a * d.dot(a);
    let k2 = dd.dot(dd);
    if k2 < 1e-12 {
        return Err(unsupported("parallel fillet cylinders"));
    }
    let l0 = (start - from_p3(base.subs(lo))).dot(d);
    let l1 = (end - from_p3(base.subs(hi))).dot(d);
    let branch = |p: Vec3| ((p - o) - a * (p - o).dot(a)).dot(dd);
    let side = branch(start).signum();
    if side == 0.0 || side * branch(end) <= tol {
        return Err(unsupported("a fillet end section folds back"));
    }
    let eval = |u: f64| -> Result<(Vec3, Vec3)> {
        let q = from_p3(base.subs(u));
        let v = base.der(u);
        let v = Vec3::new(v.x, v.y, v.z);
        let x = q - o;
        let x = x - a * x.dot(a);
        let k1 = x.dot(dd);
        let disc = k1 * k1 - k2 * (x.dot(x) - r * r);
        if !(disc.is_finite() && disc > tol * tol) {
            return Err(unsupported("fillet cylinders do not cross transversely"));
        }
        let l = (-k1 + side * disc.sqrt()) / k2;
        let p = q + d * l;
        let radial = x + dd * l;
        let dl = -radial.dot(v - a * v.dot(a)) / radial.dot(dd);
        let tangent = v + d * dl;
        if !p.is_finite() || !tangent.is_finite() || (l - (l0 + (l1 - l0) * (u - lo) / (hi - lo))).abs() > r * 4.0 {
            return Err(unsupported("an unbounded fillet end section"));
        }
        Ok((p, tangent))
    };
    if eval(lo)?.0.dist(start) > tol * 10.0 || eval(hi)?.0.dist(end) > tol * 10.0 {
        return Err(unsupported("fillet end section misses its endpoints"));
    }
    let mut pending = vec![(lo, hi, 0)];
    let mut ctrl = Vec::new();
    let mut knots = vec![lo; 4];
    while let Some((u, v, depth)) = pending.pop() {
        let (p, dp) = eval(u)?;
        let (q, dq) = eval(v)?;
        let c1 = p + dp * ((v - u) / 3.0);
        let c2 = q - dq * ((v - u) / 3.0);
        let mut accurate = true;
        for t in [0.25, 0.5, 0.75] {
            let b = 1.0 - t;
            let approx = p * (b * b * b) + c1 * (3.0 * b * b * t) + c2 * (3.0 * b * t * t) + q * (t * t * t);
            accurate &= approx.dist(eval(u + (v - u) * t)?.0) <= tol * 0.1;
        }
        if !accurate {
            if depth >= 16 || ctrl.len() + pending.len() * 3 > 12_000 {
                return Err(unsupported("cannot resolve this fillet end section within tolerance"));
            }
            let m = (u + v) * 0.5;
            pending.push((m, v, depth + 1));
            pending.push((u, m, depth + 1));
            continue;
        }
        if ctrl.is_empty() {
            ctrl.push(p3(start));
        }
        ctrl.extend([p3(c1), p3(c2), p3(if v == hi { end } else { q })]);
        knots.extend([v; 3]);
    }
    knots.push(hi);
    let c = mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).map_err(|e| KernelError::Failed(format!("fillet intersection: {e}")))?;
    Ok(mt::Curve::BSplineCurve(c))
}

/// Topological closure alone does not detect a cap crossing another boundary or a hole.
/// Check the rebuilt planar loops, including boundaries away from the old endpoint.
pub(super) fn uncrossed(f: &mt::Face, tol: f64) -> Result<()> {
    let normal = plane_normal(f);
    let mut segments = Vec::new();
    for (wi, wire) in f.boundaries().iter().enumerate() {
        let mut points = Vec::new();
        for e in wire.edge_iter() {
            let c = e.oriented_curve();
            let mut ps = c.parameter_division(c.range_tuple(), tol.max(1e-6)).1;
            ps.pop(); // the next edge supplies the shared endpoint
            points.extend(ps.into_iter().map(from_p3));
        }
        if points.len() < 3 || points.len() + segments.len() > 8192 {
            return Err(unsupported("cannot validate this fillet boundary within the segment limit"));
        }
        for (i, a) in points.iter().enumerate() {
            let b = points.get((i + 1) % points.len()).ok_or_else(|| unsupported("empty fillet boundary"))?;
            segments.push((wi, i, points.len(), *a, *b));
        }
    }
    for (i, (wi, ei, count, a, b)) in segments.iter().enumerate() {
        for (wj, ej, _, c, d) in segments.iter().skip(i + 1) {
            if wi == wj && (ej - ei == 1 || ej - ei + 1 == *count) {
                continue;
            }
            let (ab, cd) = (*b - *a, *d - *c);
            let crosses = if let Some(n) = normal {
                let (sa, sb) = (ab.cross(*c - *a).dot(n), ab.cross(*d - *a).dot(n));
                let (sc, sd) = (cd.cross(*a - *c).dot(n), cd.cross(*b - *c).dot(n));
                sa * sb < 0.0 && sc * sd < 0.0
            } else {
                // Closest interior points of two 3D chords; endpoint distances
                // below also cover parallel segments and clamped solutions.
                let w = *a - *c;
                let (aa, bb, cc) = (ab.dot(ab), ab.dot(cd), cd.dot(cd));
                let den = aa * cc - bb * bb;
                let s = (bb * cd.dot(w) - cc * ab.dot(w)) / den;
                let t = (aa * cd.dot(w) - bb * ab.dot(w)) / den;
                den > aa * cc * 1e-12
                    && (0.0..=1.0).contains(&s)
                    && (0.0..=1.0).contains(&t)
                    && (*a + ab * s).dist(*c + cd * t) <= tol.max(1e-6) * 3.0
            };
            let touches =
                a.dist_to_segment(*c, *d).min(b.dist_to_segment(*c, *d)).min(c.dist_to_segment(*a, *b)).min(d.dist_to_segment(*a, *b)) <= tol;
            if crosses || touches {
                return Err(unsupported("the fillet crosses another face boundary"));
            }
        }
    }
    Ok(())
}

pub(crate) fn can_trim(solid: &Solid, edge: &mt::Edge) -> bool {
    matches!(edge.curve(), mt::Curve::Line(_))
        && solid.face_iter().filter(|f| f.edge_iter().any(|e| e.id() == edge.id())).all(|f| plane_normal(f).is_some())
}

pub(crate) fn fillet_one(solid: &Solid, edge: &mt::Edge, size: f64, r: f64) -> Result<(Solid, crate::record::Rebuild)> {
    fillet_one_impl(solid, edge, size, r, false)
}

pub(crate) fn with_transitions(solid: &Solid, edge: &mt::Edge, size: f64, r: f64) -> Result<(Solid, crate::record::Rebuild)> {
    fillet_one_impl(solid, edge, size, r, true)
}

fn fillet_one_impl(solid: &Solid, edge: &mt::Edge, size: f64, r: f64, transitions: bool) -> Result<(Solid, crate::record::Rebuild)> {
    if !(r.is_finite() && r > 0.0 && (size * 4.0).is_finite() && size > 0.0) {
        return Err(KernelError::Invalid("fillet radius and body size must be finite and positive".into()));
    }
    if !matches!(edge.curve(), mt::Curve::Line(_)) {
        return Err(unsupported("an endpoint-trimmed fillet needs a straight edge"));
    }
    let (si, sh) = solid
        .boundaries()
        .iter()
        .enumerate()
        .find(|(_, s)| s.edge_iter().any(|e| e.id() == edge.id()))
        .ok_or_else(|| unsupported("missing fillet edge"))?;
    let faces: Vec<_> = sh.face_iter().collect();
    let on =
        |id: mt::EdgeID| -> Vec<usize> { faces.iter().enumerate().filter(|(_, f)| f.edge_iter().any(|e| e.id() == id)).map(|(i, _)| i).collect() };
    let fs = on(edge.id());
    let [fa, fb] = fs[..] else {
        return Err(unsupported("a non-manifold edge"));
    };
    let (Some(f1), Some(f2)) = (faces.get(fa), faces.get(fb)) else {
        return Err(unsupported("missing side faces"));
    };
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else {
        return Err(unsupported("fillet side faces must be planar"));
    };
    let into = |f: &mt::Face, n: Vec3| -> Option<Vec3> {
        let e = f.boundary_iters().into_iter().flatten().find(|e| e.id() == edge.id())?;
        n.cross(vtx(e.back()) - vtx(e.front())).normalized()
    };
    let (Some(t1), Some(t2)) = (into(f1, n1), into(f2, n2)) else {
        return Err(unsupported("fillet side directions"));
    };
    let sign = if n1.dot(t2) > 1e-6 && n2.dot(t1) > 1e-6 {
        1.0
    } else if n1.dot(t2) < -1e-6 && n2.dot(t1) < -1e-6 {
        -1.0
    } else {
        return Err(unsupported("tangent or inconsistent fillet side faces"));
    };
    let (v0, v1) = (edge.absolute_front(), edge.absolute_back());
    let (p0, p1) = (vtx(v0), vtx(v1));
    let d = (p1 - p0).normalized().ok_or_else(|| unsupported("zero length fillet"))?;
    let alpha = t1.dot(t2).clamp(-1.0, 1.0).acos();
    if !(alpha > 1e-3 && alpha < std::f64::consts::PI - 1e-3) {
        return Err(unsupported("the faces at the edge are tangent"));
    }
    let s = r / (alpha * 0.5).tan();
    if !(s.is_finite() && s > 0.0 && s < size) {
        return Err(unsupported("the fillet setback exceeds the body size"));
    }
    let w = (alpha * 0.5).sin();
    let tol = (size * 1e-8).max(1e-7);
    // Extensions through a reflex endpoint must not escape the part. The
    // original vertex envelope is conservative for curved outer boundaries;
    // refusing those cases is preferable to creating an exterior protrusion.
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for v in solid.vertex_iter() {
        bounds.add(vtx(&v));
    }
    let in_bounds = |q: Vec3| {
        q.is_finite()
            && q.x >= bounds.min.x - tol
            && q.x <= bounds.max.x + tol
            && q.y >= bounds.min.y - tol
            && q.y <= bounds.max.y + tol
            && q.z >= bounds.min.z - tol
            && q.z <= bounds.max.z + tol
    };
    let mut subst = HashMap::new();
    let mut ends = Vec::new();
    let mut face_caps: HashMap<usize, Vec<mt::Edge>> = HashMap::new();
    let mut patches = Vec::new();
    let mut removed = Vec::new();
    for (v, p) in [(v0, p0), (v1, p1)] {
        // Walk the endpoint fan from side A to side B, retaining every intervening face.
        let mut fan = Vec::new();
        let mut fi = fa;
        let mut incoming = edge.id();
        for _ in 0..32 {
            let f = faces.get(fi).ok_or_else(|| unsupported("missing endpoint face"))?;
            let seam =
                f.edge_iter().find(|e| e.id() != incoming && (e.front() == v || e.back() == v)).ok_or_else(|| unsupported("open endpoint fan"))?;
            let next = on(seam.id()).into_iter().find(|f| *f != fi).ok_or_else(|| unsupported("open endpoint fan"))?;
            if next == fa || fan.iter().any(|(e, _): &(mt::Edge, usize)| e.id() == seam.id()) {
                return Err(unsupported("cyclic endpoint fan"));
            }
            incoming = seam.id();
            fan.push((seam, next));
            fi = next;
            if fi == fb {
                break;
            }
        }
        if fi != fb {
            return Err(unsupported("too many faces at the fillet endpoint"));
        }
        let exact = (|| -> Result<_> {
            if fan.iter().take(fan.len().saturating_sub(1)).any(|(_, i)| faces.get(*i).is_none_or(|f| plane_normal(f).is_none())) {
                return Err(unsupported("this fillet needs rebuilding where it meets an earlier curved blend"));
            }
            let base = conic(p + t1 * s, p, p + t2 * s, w);
            let center = p + t1 * s + n1 * (sign * r);
            let mut local_subst = HashMap::new();
            let mut points = Vec::new();
            let count = fan.len();
            for (i, (seam, next)) in fan.iter().enumerate() {
                let oriented = if seam.front() == v { seam.clone() } else { seam.inverse() };
                let c = oriented.oriented_curve();
                let residual = |q: Vec3| {
                    if i == 0 {
                        (q - p).dot(t1) - s
                    } else if i + 1 == count {
                        (q - p).dot(t2) - s
                    } else {
                        let x = q - center;
                        (x - d * x.dot(d)).len() - r
                    }
                };
                let hit = crossing(&c, residual, tol);
                let (t, q) = match hit {
                    Ok(hit) => hit,
                    Err(_) if i > 0 && i + 1 < count && matches!(c, mt::Curve::Line(_)) => {
                        // A reflex joint in the end-face fan may extend its shared seam.
                        // Loop intersection and global bounds checks cannot establish
                        // that this local extension lies on the material side.
                        let a = fan.get(i - 1).and_then(|(_, fi)| faces.get(*fi));
                        let b = faces.get(*next);
                        if !matches!((a, b), (Some(a), Some(b)) if reflex_joint(a, b, seam.id())) {
                            return Err(unsupported("a convex endpoint joint cannot extend its seam backwards"));
                        }
                        let back = vtx(oriented.back());
                        let away = (back - p).normalized().ok_or_else(|| unsupported("zero length endpoint seam"))?;
                        let ext = mt::Curve::Line(mt::Line(p3(p), p3(p - away * size)));
                        let (_, q) = crossing(&ext, residual, tol)
                            .map_err(|_| unsupported("this fillet needs a different arrangement of faces at its end"))?;
                        (0.0, q)
                    }
                    Err(e) => return Err(e),
                };
                let q0 = q - d * (q - p).dot(d);
                if !in_bounds(q) {
                    return Err(unsupported("the fillet endpoint leaves the part's bounds"));
                }
                let u = if i == 0 {
                    0.0
                } else if i + 1 == count {
                    1.0
                } else {
                    base.search_nearest_parameter(p3(q0), None, 100).ok_or_else(|| unsupported("endpoint seam misses the fillet"))?
                };
                if !(0.0..=1.0).contains(&u) || from_p3(base.subs(u)).dist(q0) > tol * 10.0 {
                    return Err(unsupported("endpoint seam misses the fillet arc"));
                }
                if points.last().is_some_and(|(prev, _): &(f64, mt::Vertex)| u <= *prev + 1e-8) {
                    return Err(unsupported("fillet crosses the endpoint faces out of order"));
                }
                let nv = builder::vertex(p3(q));
                let (_, last) = c.range_tuple();
                let ne = if matches!(c, mt::Curve::Line(_)) {
                    builder::line(&nv, oriented.back())
                } else {
                    mt::Edge::try_new(&nv, oriented.back(), trim(c, t, last)).map_err(|e| KernelError::Failed(format!("fillet seam: {e}")))?
                };
                local_subst.insert(seam.id(), if oriented.front() == seam.absolute_front() { ne } else { ne.inverse() });
                points.push((u, nv));
            }
            let mut caps = Vec::new();
            for (i, pair) in points.windows(2).enumerate() {
                let [(lo, a), (hi, b)] = pair else {
                    return Err(unsupported("fillet endpoint"));
                };
                let (_, g) = fan.get(i).ok_or_else(|| unsupported("fillet endpoint face"))?;
                let f = faces.get(*g).ok_or_else(|| unsupported("fillet endpoint face"))?;
                let ca = on_end(f, p + t1 * s, d, p)?;
                let cb = on_end(f, p + t2 * s, d, p)?;
                let curve = trim(conic(ca, p, cb, w), *lo, *hi);
                // A nearly parallel end plane can give an ellipse with a large
                // excursion between its endpoints, even when those endpoints fit.
                let samples = curve.parameter_division(curve.range_tuple(), tol.max(1e-6)).1;
                if samples.len() > 8192 || samples.into_iter().any(|q| !in_bounds(from_p3(q))) {
                    return Err(unsupported("the fillet end section leaves the part's bounds"));
                }
                let cap = mt::Edge::try_new(a, b, curve).map_err(|e| KernelError::Failed(format!("fillet cap: {e}")))?;
                caps.push((*g, cap));
            }
            let a = points.first().map(|x| x.1.clone()).ok_or_else(|| unsupported("fillet endpoint"))?;
            let b = points.last().map(|x| x.1.clone()).ok_or_else(|| unsupported("fillet endpoint"))?;
            Ok((a, b, caps, local_subst))
        })();
        match exact {
            Ok((a, b, caps, replacements)) => {
                subst.extend(replacements);
                for (fi, cap) in &caps {
                    face_caps.entry(*fi).or_default().push(cap.clone());
                }
                ends.push((a, b, caps));
            }
            Err(original) if !transitions => return Err(original),
            Err(_) => {
                let direction = if v == v0 { d } else { -d };
                let end = transition::build(&faces, &fan, v, p, direction, t1, t2, n1, n2, sign, s, r, w, size, tol, bounds, fa, fb)?;
                subst.extend(end.replacements);
                removed.extend(end.removed);
                for (fi, cap) in end.connections {
                    face_caps.entry(fi).or_default().push(cap);
                }
                patches.extend(end.patches.into_iter().map(|patch| (v == v0, patch)));
                ends.push((end.a, end.b, end.sections.into_iter().map(|section| (usize::MAX, section)).collect()));
            }
        }
    }
    let [(a0, b0, c0), (a1, b1, c1)] = &ends[..] else {
        return Err(unsupported("fillet endpoints"));
    };
    for (f, a, b) in [(f1, a0, a1), (f2, b0, b1)] {
        let (a, b) = (vtx(a), vtx(b));
        if (b - a).dot(d) <= tol * 100.0 || !crate::blend::extension_fits(f, a + d * tol * 20.0, b - d * tol * 20.0, tol) {
            return Err(unsupported("fillet contact leaves the side face"));
        }
    }
    let (la, lb) = (builder::line(a0, a1), builder::line(b0, b1));
    let mut rec = crate::record::Rebuild::default();
    rec.pieces(&subst);
    let mut out = Vec::new();
    for (i, f) in faces.iter().enumerate() {
        let mut sub = subst.clone();
        if i == fa {
            sub.insert(edge.id(), la.clone());
        }
        if i == fb {
            sub.insert(edge.id(), lb.clone());
        }
        let caps = face_caps.remove(&i).unwrap_or_default();
        let rebuilt = crate::blend::rebuild_face_without(f, &sub, &caps, &removed)?;
        if i == fa || i == fb || !caps.is_empty() {
            uncrossed(&rebuilt, tol)?;
        }
        rec.face(&rebuilt, f);
        rec.pieces(&sub);
        out.push(rebuilt);
    }
    let mut es: Vec<_> = c0.iter().map(|(_, e)| e.clone()).collect();
    es.push(lb);
    es.extend(c1.iter().rev().map(|(_, e)| e.inverse()));
    es.push(la.inverse());
    let mut wire: mt::Wire = es.into();
    let forward = f1.boundary_iters().into_iter().flatten().any(|e| e.id() == edge.id() && e.front() == v0);
    if !forward {
        wire = wire.inverse();
    }
    let (q0, q1) = (p0 - d * size, p1 + d * size);
    let ctrl = vec![vec![h(q0 + t1 * s, 1.0), h(q1 + t1 * s, 1.0)], vec![h(q0, w), h(q1, w)], vec![h(q0 + t2 * s, 1.0), h(q1 + t2 * s, 1.0)]];
    let mut surf =
        mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new((mt::KnotVec::bezier_knot(2), mt::KnotVec::bezier_knot(1)), ctrl)));
    let mid = from_p3(conic(p0 + t1 * s, p0, p0 + t2 * s, w).subs(0.5));
    let center = p0 + t1 * s + n1 * (sign * r);
    if let Some((u, v)) = surf.search_nearest_parameter(p3(mid), None, 100) {
        let n = surf.normal(u, v);
        if Vec3::new(n.x, n.y, n.z).dot((center - mid) * sign) < 0.0 {
            surf = mt::Invertible::inverse(&surf);
        }
    }
    let cylinder = mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("fillet cylinder: {e}")))?;
    rec.blend(&cylinder, [edge.id()]);
    out.push(cylinder);
    for (at_start, mut patch) in patches {
        if at_start != forward {
            patch = mt::Face::try_new(patch.boundaries().iter().map(|w| w.inverse()).collect(), patch.oriented_surface())
                .map_err(|e| KernelError::Failed(format!("transition orientation: {e}")))?;
        }
        rec.blend(&patch, [edge.id()]);
        out.push(patch);
    }
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.get_mut(si) {
        *sh = out.into();
    }
    Solid::try_new(shells).map(|s| (s, rec)).map_err(|e| KernelError::Failed(format!("fillet endpoints: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, BoolOp};
    use mt::ParametricSurface;
    use solidcraft_geom::{Loop2, Plane, Vec2};

    fn basin(fall: bool) -> Body {
        let rect = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let slab = crate::box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let pocket =
            crate::loft(&[(Plane::XY.offset(30.0), rect(20.0, 4.0, 80.0, 50.0)), (Plane::XY.offset(10.0), rect(30.0, 20.0, 70.0, 40.0))]).unwrap();
        let mut body = crate::boolean(&slab, &pocket, BoolOp::Cut).unwrap().unwrap();
        if fall {
            let floor = crate::loft(&[(Plane::XY.offset(10.0), rect(30.0, 20.0, 70.0, 40.0)), (Plane::XY.offset(6.0), rect(45.0, 26.0, 55.0, 34.0))])
                .unwrap();
            body = crate::boolean(&body, &floor, BoolOp::Cut).unwrap().unwrap();
        }
        body
    }
    fn check(body: &Body, p: Vec3, r: f64) -> Body {
        let out = check_geometry(body, p, r);
        let delta = crate::measure(&out).unwrap().volume - crate::measure(body).unwrap().volume;
        assert!(delta.abs() > 1e-6 && delta.abs() < 500.0, "volume delta {delta}");
        out
    }
    fn check_geometry(body: &Body, p: Vec3, r: f64) -> Body {
        let out = crate::fillet(body, &[p], r).unwrap_or_else(|e| panic!("fillet {p:?}, radius {r}: {e}"));
        assert!(out.validity().is_empty());
        assert_eq!(out.unmeshed_faces(), 0);
        for f in out.solid.face_iter() {
            let s = crate::body::evaluation::Surface(f.surface());
            for e in f.edge_iter() {
                let c = e.curve();
                let (a, b) = c.range_tuple();
                let mut hint = None;
                for j in 0..=16 {
                    let q = c.subs(a + (b - a) * j as f64 / 16.0);
                    let (u, v) = s.search_nearest_parameter(q, hint, 100).unwrap();
                    // A continuation seed can leave the bounded spline domain.
                    // Independently retry projection before checking the actual gap.
                    let (u, v) =
                        if from_p3(s.subs(u, v)).dist(from_p3(q)) >= 1e-4 { s.search_nearest_parameter(q, None, 100).unwrap() } else { (u, v) };
                    hint = Some((u, v));
                    assert!(
                        from_p3(s.subs(u, v)).dist(from_p3(q)) < 1e-4,
                        "boundary off face: sample {j} uv=({u},{v}), gap={}, spline={}, fresh={:?}",
                        from_p3(s.subs(u, v)).dist(from_p3(q)),
                        matches!(s.0, mt::Surface::BSplineSurface(_)),
                        s.search_nearest_parameter(q, None, 100)
                    );
                }
            }
        }
        out
    }
    #[test]
    fn concave_oblique_end_faces() {
        check(&basin(false), Vec3::new(25.0, 12.0, 20.0), 2.0);
    }
    #[test]
    fn concave_four_face_endpoints() {
        let b = basin(true);
        check(&b, Vec3::new(25.0, 12.0, 20.0), 2.0);
        check(&b, Vec3::new(50.0, 20.0, 10.0), 2.0);
    }

    /// One tenth of the reported basin: 10-degree side/back walls, a shallow front
    /// wall, and a floor dropping 0.4 mm to an 8 mm square drain seat.
    pub(super) fn shallow_basin() -> Body {
        let rect = |x0, y0, x1, y1| Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]);
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let floor = rect(15.0 + inset, 35.0 - inset, 75.0 - inset, 45.0 - inset);
        let slab = crate::box_solid(Vec3::ZERO, Vec3::new(90.0, 48.0, 16.0)).unwrap();
        let wall_tool = crate::loft(&[(Plane::XY.offset(16.0), rect(15.0, 1.8, 75.0, 45.0)), (Plane::XY.offset(4.0), floor.clone())]).unwrap();
        let body = crate::boolean(&slab, &wall_tool, BoolOp::Cut).unwrap().unwrap();
        let drain_tool =
            crate::loft(&[(Plane::XY.offset(4.0), floor), (Plane::XY.offset(3.6), rect(41.0, 36.0 - inset, 49.0, 45.0 - inset - 1.0))]).unwrap();
        crate::boolean(&body, &drain_tool, BoolOp::Cut).unwrap().unwrap()
    }

    #[test]
    fn basin_floor_edges_and_back_corners() {
        let b = shallow_basin();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        for p in [
            Vec3::new(45.0, 35.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(15.0 + inset * 0.5, 45.0 - inset * 0.5, 10.0),
            Vec3::new(75.0 - inset * 0.5, 45.0 - inset * 0.5, 10.0),
        ] {
            check(&b, p, 1.5);
        }
    }

    #[test]
    fn adjacent_basin_floor_fillets() {
        let b = shallow_basin();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let left = Vec3::new(15.0 + inset, 40.0 - inset, 4.0);
        let back = Vec3::new(45.0, 45.0 - inset, 4.0);
        let left = check(&b, left, 1.5);
        crate::blend_fit::check(&left, &[back], crate::blend_fit::Size::Round(1.5)).unwrap();
        check(&left, back, 1.5);
    }

    #[test]
    fn traced_end_continuation_does_not_exempt_an_end_hole() {
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let left = Vec3::new(15.0 + inset, 40.0 - inset, 4.0);
        let back = Vec3::new(45.0, 45.0 - inset, 4.0);
        let body = crate::fillet(&shallow_basin(), &[left], 1.5).unwrap();
        let rounded = crate::fillet(&body, &[back], 1.5).unwrap();
        // Drill through the actual end section on the earlier left round. A hole
        // farther down the floor need not intersect this concave fillet at all.
        let q = rounded
            .solid
            .face_iter()
            .last()
            .unwrap()
            .edge_iter()
            .filter(|e| !matches!(e.curve(), mt::Curve::Line(_)))
            .map(|e| {
                let c = e.curve();
                let (a, b) = c.range_tuple();
                from_p3(c.subs((a + b) * 0.5))
            })
            .find(|p| p.x < 20.0)
            .unwrap();
        let tool = crate::box_solid(Vec3::new(q.x - 0.05, q.y - 0.05, -1.0), Vec3::new(q.x + 0.05, q.y + 0.05, 12.0)).unwrap();
        let body = crate::boolean(&body, &tool, BoolOp::Cut).unwrap().unwrap();
        assert!(crate::fillet(&body, &[back], 1.5).is_err());
    }

    #[test]
    fn all_basin_floor_fillets() {
        let b = shallow_basin();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let picks = [
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(45.0, 35.0 - inset, 4.0),
        ];
        let mut cur = b.clone();
        for p in picks {
            cur = check(&cur, p, 1.5);
        }
        let combined = crate::fillet(&b, &picks, 1.5).unwrap();
        assert!(combined.validity().is_empty());
        assert_eq!(combined.unmeshed_faces(), 0);
        assert!((crate::measure(&cur).unwrap().volume - crate::measure(&combined).unwrap().volume).abs() < 1e-4);
    }

    #[test]
    fn floor_fillets_with_existing_rim_export_and_replay() {
        let b = shallow_basin();
        let rim_edges = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
        let mut b = crate::fillet(&b, &rim_edges, 0.3).unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        for p in [
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(45.0, 35.0 - inset, 4.0),
        ] {
            b = check(&b, p, 1.5);
        }
        let volume = crate::measure(&b).unwrap().volume;
        let export = crate::ExportBody { name: "Rounded basin".into(), body: &b, color: None };
        let step = crate::step_export_bodies(&[export], &crate::StepHeader::default()).unwrap();
        let imported = crate::step_import(&step).unwrap();
        assert_eq!(imported.bodies.len(), 1);
        let restored = &imported.bodies[0].body;
        assert!(restored.validity().is_empty());
        assert_eq!(restored.unmeshed_faces(), 0);
        assert!((crate::measure(restored).unwrap().volume - volume).abs() < 0.05);
        let hole = crate::cylinder(Vec3::new(45.0, 38.0, -1.0), Vec3::Z, 1.0, 6.0).unwrap();
        let drilled = crate::boolean(&b, &hole, BoolOp::Cut).unwrap().unwrap();
        assert!(drilled.validity().is_empty());
        assert_eq!(drilled.unmeshed_faces(), 0);
        assert!(crate::measure(&drilled).unwrap().volume < volume);
    }

    #[test]
    fn tracing_preserves_holes_in_rebuilt_end_faces() {
        let hole = crate::cylinder(Vec3::new(20.5, 38.0, -1.0), Vec3::Z, 0.2, 10.0).unwrap();
        let b = crate::boolean(&shallow_basin(), &hole, BoolOp::Cut).unwrap().unwrap();
        assert!(!b.tessellate(1e-3).unwrap().contains(Vec3::new(20.5, 38.0, 1.0)));
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let b = check(&b, Vec3::new(15.0 + inset, 40.0 - inset, 4.0), 1.5);
        let b = check(&b, Vec3::new(45.0, 45.0 - inset, 4.0), 1.5);
        let mesh = b.tessellate(1e-3).unwrap();
        assert!(!mesh.contains(Vec3::new(20.5, 38.0, 1.0)));
        assert!(mesh.contains(Vec3::new(21.0, 38.0, 1.0)));
    }

    #[test]
    fn vertical_corners_after_floor_rounds() {
        let mut b = shallow_basin();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        for p in [
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(45.0, 35.0 - inset, 4.0),
        ] {
            b = check(&b, p, 1.5);
        }
        let before: Vec<_> = b.solid.vertex_iter().map(|v| vtx(&v)).collect();
        let volume = crate::measure(&b).unwrap().volume;
        for x in [15.0 + inset * 0.5, 75.0 - inset * 0.5] {
            for y in [45.0 - inset * 0.5, (1.8 + 35.0 - inset) * 0.5] {
                let out = check_geometry(&b, Vec3::new(x, y, 10.0), 1.5);
                let delta = crate::measure(&out).unwrap().volume - volume;
                assert!(delta > 1e-6 && delta < 500.0, "volume delta {delta}");
                check_transition_tangency(&out);
                assert_eq!(before, b.solid.vertex_iter().map(|v| vtx(&v)).collect::<Vec<_>>());
            }
        }
    }

    fn check_transition_tangency(out: &Body) {
        for patch in out.solid.face_iter().filter(|f| matches!(f.surface(), mt::Surface::BSplineSurface(_))) {
            for edge in patch.edge_iter() {
                let neighbor = out.solid.face_iter().find(|f| f.id() != patch.id() && f.edge_iter().any(|e| e.id() == edge.id())).unwrap();
                if plane_normal(neighbor).is_some() && !matches!(edge.curve(), mt::Curve::Line(_)) {
                    continue;
                }
                let curve = edge.curve();
                let (a, b) = curve.range_tuple();
                let (mut patch_hint, mut neighbor_hint) = (None, None);
                for i in 0..=32 {
                    // At a singular corner the normal is direction-dependent. Check
                    // each boundary's limiting direction close to both endpoints.
                    let mut fraction = i as f64 / 32.0;
                    if i == 0 || i == 32 {
                        // Cubically reparameterized edges have almost no physical
                        // separation from their pole at a fixed parameter epsilon.
                        let start = curve.subs(if i == 0 { a } else { b });
                        let length = from_p3(curve.subs(a)).dist(from_p3(curve.subs(b)));
                        let (mut low, mut high) = (0.0, 1.0);
                        for _ in 0..40 {
                            let mid = (low + high) * 0.5;
                            let f = if i == 0 { mid } else { 1.0 - mid };
                            if from_p3(curve.subs(a + (b - a) * f)).dist(from_p3(start)) < length * 1e-4 {
                                low = mid;
                            } else {
                                high = mid;
                            }
                        }
                        fraction = if i == 0 { high } else { 1.0 - high };
                    }
                    let p = curve.subs(a + (b - a) * fraction);
                    let normal = |f: &mt::Face, hint: &mut Option<(f64, f64)>| {
                        let s = crate::body::evaluation::Surface(f.oriented_surface());
                        let (u, v) = s.search_nearest_parameter(p, *hint, 100).unwrap();
                        let (u, v) =
                            if from_p3(s.subs(u, v)).dist(from_p3(p)) >= 1e-4 { s.search_nearest_parameter(p, None, 100).unwrap() } else { (u, v) };
                        assert!(from_p3(s.subs(u, v)).dist(from_p3(p)) < 1e-4);
                        *hint = Some((u, v));
                        let n = s.normal(u, v);
                        Vec3::new(n.x, n.y, n.z)
                    };
                    let angle = normal(patch, &mut patch_hint).dot(normal(neighbor, &mut neighbor_hint)).clamp(-1.0, 1.0).acos().to_degrees();
                    assert!(
                        angle < 0.02,
                        "transition kink {angle} degrees at {fraction}, point {p:?}, parameters {patch_hint:?}/{neighbor_hint:?}, edge endpoints {:?}/{:?}, spline neighbor {}",
                        edge.front().point(),
                        edge.back().point(),
                        matches!(neighbor.surface(), mt::Surface::BSplineSurface(_))
                    );
                }
            }
        }
    }

    #[test]
    fn rim_transition_interior_normals_do_not_ripple() {
        use mt::{InnerSpace, ParametricSurface};
        let body = shallow_basin();
        let rim = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
        let body = crate::fillet(&body, &rim, 0.3).unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let body = check_geometry(&body, Vec3::new(15.0 + inset * 0.5, (1.8 + 35.0 - inset) * 0.5, 10.0), 1.5);
        // The old surface passed the normal-travel check below but had an
        // axial curvature jump exceeding 5/r at the constant-radius section.
        // Tangency alone left a narrow highlight resembling a pinched crease.
        let mut checked = 0;
        for face in body.solid.face_iter() {
            let mt::Surface::BSplineSurface(s) = face.surface() else { continue };
            if !matches!(s.degrees(), (3, 3) | (5, 5)) || s.subs(0.5, 0.5).z <= 12.0 {
                continue;
            }
            let surface = crate::body::evaluation::Surface(face.oriented_surface());
            for edge in face.edge_iter() {
                let neighbor = body.solid.face_iter().find(|f| f.id() != face.id() && f.edge_iter().any(|e| e.id() == edge.id())).unwrap();
                let Some(crate::offset::Surf::Cylinder { a: axis, r, .. }) = crate::offset::surf_of(neighbor, 1e-6) else { continue };
                if (r - 1.5).abs() > 1e-6 {
                    continue;
                }
                let curve = edge.oriented_curve();
                let (lo, hi) = curve.range_tuple();
                for fraction in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9] {
                    let p = curve.subs(lo + (hi - lo) * fraction);
                    let (u, v) = surface.search_nearest_parameter(p, None, 100).unwrap();
                    let su = surface.uder(u, v);
                    let sv = surface.vder(u, v);
                    let a = mt::Vector3::new(axis.x, axis.y, axis.z);
                    let det = su.dot(su) * sv.dot(sv) - su.dot(sv).powi(2);
                    let ud = (a.dot(su) * sv.dot(sv) - a.dot(sv) * su.dot(sv)) / det;
                    let vd = (a.dot(sv) * su.dot(su) - a.dot(su) * su.dot(sv)) / det;
                    let curvature = (surface.uuder(u, v) * ud * ud + surface.uvder(u, v) * (2.0 * ud * vd) + surface.vvder(u, v) * vd * vd)
                        .dot(surface.normal(u, v));
                    // Keep the axial jump below one tenth of the cylinder's
                    // transverse curvature (the former spike was over 5/r).
                    assert!((curvature * r).abs() < 0.1, "rim join axial curvature {} / radius at {fraction}", curvature * r);
                    // Also enter the patch in the cylinder's axial direction:
                    // matching only the curvature exactly on the edge could
                    // move the pinch a short distance into the transition.
                    let inward = ((0.5 - u) * ud + (0.5 - v) * vd).signum();
                    let normal = surface.normal(u, v);
                    for step in [0.01, 0.02, 0.04] {
                        let distance = inward * r * step;
                        let next = surface.normal(u + ud * distance, v + vd * distance);
                        let angle = normal.dot(next).clamp(-1.0, 1.0).acos();
                        assert!(angle < 0.5 * step, "pinch inside rim join at {fraction}, distance {step} r, normal turn {angle}");
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked == 9 || checked == 18, "must check all one or two transition/corner-cylinder sections: {checked}");
        let patches: Vec<_> = body
            .solid
            .face_iter()
            .filter_map(|f| match f.surface() {
                mt::Surface::BSplineSurface(s) if matches!(s.degrees(), (3, 3) | (5, 5)) && s.subs(0.5, 0.5).z > 12.0 => Some(s),
                _ => None,
            })
            .collect();
        assert!(!patches.is_empty());
        for patch in patches {
            let surface = crate::body::evaluation::Surface(mt::Surface::BSplineSurface(patch));
            // Interior paths through the long, shallow front-rim transition. A
            // boundary-only G1 check missed its alternating inward/outward puckers.
            // Permit gradual changes of direction, but no large extra normal travel.
            for j in (30..=75).step_by(5) {
                let normals: Vec<_> = (15..=75).map(|i| surface.normal(i as f64 / 100.0, j as f64 / 100.0)).collect();
                let angle = |a: mt::Vector3, b: mt::Vector3| a.dot(b).clamp(-1.0, 1.0).acos();
                let travel: f64 = normals.windows(2).map(|n| angle(n[0], n[1])).sum();
                let net = angle(normals[0], *normals.last().unwrap());
                assert!(travel < 1.3 * net + 0.03, "rim transition ripples at v={}: normal travel {travel}, net {net}", j as f64 / 100.0);
            }
        }
    }

    #[test]
    fn front_corner_transition_patch() {
        let b = shallow_basin();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let volume = crate::measure(&b).unwrap().volume;
        for x in [15.0 + inset * 0.5, 75.0 - inset * 0.5] {
            let out = check_geometry(&b, Vec3::new(x, (1.8 + 35.0 - inset) * 0.5, 10.0), 1.5);
            let delta = crate::measure(&out).unwrap().volume - volume;
            assert!(delta > 1e-6 && delta < 500.0, "volume delta {delta}");
            check_transition_tangency(&out);
        }
    }

    #[test]
    fn corner_to_rim_transition_patch() {
        let b = shallow_basin();
        let picks = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
        let rim = crate::fillet(&b, &picks, 0.3).unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let volume = crate::measure(&rim).unwrap().volume;
        for x in [15.0 + inset * 0.5, 75.0 - inset * 0.5] {
            for y in [45.0 - inset * 0.5, (1.8 + 35.0 - inset) * 0.5] {
                let out = check_geometry(&rim, Vec3::new(x, y, 10.0), 1.5);
                let delta = crate::measure(&out).unwrap().volume - volume;
                assert!(delta > 1e-6 && delta < 500.0, "volume delta {delta}");
                check_transition_tangency(&out);
            }
        }
    }

    #[test]
    fn transition_internal_seams_are_tangent() {
        use mt::InnerSpace;
        let mut body = shallow_basin();
        let rim = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
        body = crate::fillet(&body, &rim, 0.3).unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        for p in [
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(45.0, 35.0 - inset, 4.0),
        ] {
            body = check(&body, p, 1.5);
        }
        body = check_geometry(&body, Vec3::new(15.0 + inset * 0.5, 45.0 - inset * 0.5, 10.0), 1.5);
        check_transition_tangency(&body);
        for face in body.solid.face_iter() {
            let mt::Surface::BSplineSurface(s) = face.surface() else { continue };
            if s.degrees() != (5, 5) {
                continue;
            }
            let surface = crate::body::evaluation::Surface(mt::Surface::BSplineSurface(s));
            let mut max_angle = 0.0_f64;
            let mut worst = (0.0, 0.0, 0.0, 0.0);
            for i in 1..64 {
                for j in 1..64 {
                    let (u, v) = (i as f64 / 64.0, j as f64 / 64.0);
                    // Leave only the immediate neighborhood of singular tips out.
                    if u.min(1.0 - u).hypot(1.0 - v) < 0.08 {
                        continue;
                    }
                    let n = surface.normal(u, v);
                    for (a, b) in [(u + 1.0 / 64.0, v), (u, v + 1.0 / 64.0)] {
                        let angle = n.dot(surface.normal(a, b)).clamp(-1.0, 1.0).acos().to_degrees();
                        if angle > max_angle {
                            max_angle = angle;
                            worst = (u, v, a, b);
                        }
                    }
                }
            }
            assert!(
                max_angle < 45.0,
                "transition normal wrinkles between adjacent samples: {max_angle} degrees, at {worst:?}, midpoint {:?}",
                surface.subs(0.5, 0.5)
            );
        }
        let mesh = body.display_mesh(0.01).unwrap();
        let mut seen = std::collections::HashSet::new();
        let mut index = 0;
        let mut joins = 0;
        for face in body.solid.face_iter() {
            for edge in face.edge_iter() {
                if !seen.insert(edge.id()) {
                    continue;
                }
                let adjacent: Vec<_> = body.solid.face_iter().filter(|f| f.edge_iter().any(|e| e.id() == edge.id())).collect();
                let patch = |f: &&mt::Face| matches!(f.surface(), mt::Surface::BSplineSurface(s) if matches!(s.degrees(),(3,3)|(5,5)));
                let cylinder = |f: &&mt::Face| matches!(f.surface(), mt::Surface::NurbsSurface(s) if matches!(s.degrees(),(2,1)|(1,2)));
                if adjacent.len() == 2 && adjacent.iter().any(patch) && adjacent.iter().all(|f| patch(f) || cylinder(f)) {
                    assert!(
                        !mesh.shaded_edge_visible(index),
                        "a smooth transition join is still drawn: edge {index}, {:?} to {:?}",
                        edge.front().point(),
                        edge.back().point()
                    );
                    joins += 1;
                }
                index += 1;
            }
        }
        assert!(joins > 0);
        // Check the actual display normals, too: exact surface tangency alone
        // does not catch a mesher projecting boundary points outside a patch.
        // Include a translated, scaled body to expose endpoint conditioning.
        let placed =
            crate::transform_matrix(&body, [[10.0, 0.0, 0.0, 0.0], [0.0, 10.0, 0.0, 0.0], [0.0, 0.0, 10.0, 0.0], [0.0, -480.0, 690.0, 1.0]]).unwrap();
        for (b, tolerance) in [&body, &placed].into_iter().flat_map(|b| [1e-3, 1e-4].into_iter().map(move |t| (b, t))) {
            let mesh = b.display_mesh(b.size() * tolerance).unwrap();
            let fs: Vec<_> = b.solid.face_iter().collect();
            for (edge, faces) in mesh.edges.iter().zip(&mesh.edge_faces) {
                let [a, b2] = faces.as_slice() else { continue };
                if ![*a, *b2].iter().all(|f| matches!(fs[*f as usize].surface(), mt::Surface::BSplineSurface(s) if s.degrees() == (5, 5))) {
                    continue;
                }
                for p in edge.iter().skip(1).take(edge.len().saturating_sub(2)) {
                    // Very close samples can be merged into the singular
                    // endpoint by the triangulator. That display vertex has a
                    // direction-dependent normal; test the regular seam.
                    if edge.first().into_iter().chain(edge.last()).any(|end| p.dist(*end) < b.size() * 1e-6) {
                        continue;
                    }
                    let normals: Vec<_> = [a, b2]
                        .into_iter()
                        .map(|face| {
                            let index = mesh
                                .triangles
                                .iter()
                                .zip(&mesh.tri_face)
                                .filter(|(_, f)| *f == face)
                                .flat_map(|(t, _)| t)
                                .min_by(|a, b| mesh.positions[**a as usize].dist(*p).total_cmp(&mesh.positions[**b as usize].dist(*p)))
                                .unwrap();
                            assert!(mesh.positions[*index as usize].dist(*p) < 1e-5);
                            mesh.normals[*index as usize]
                        })
                        .collect();
                    let angle = normals[0].dot(normals[1]).clamp(-1.0, 1.0).acos().to_degrees();
                    assert!(angle < 0.02, "display seam normal mismatch {angle} at {p:?}");
                }
            }
        }
    }

    #[test]
    fn basin_rim_floor_and_all_vertical_corner_transitions() {
        let mut body = shallow_basin();
        let rim = [Vec3::new(45.0, 1.8, 16.0), Vec3::new(45.0, 45.0, 16.0), Vec3::new(15.0, 23.4, 16.0), Vec3::new(75.0, 23.4, 16.0)];
        body = crate::fillet(&body, &rim, 0.3).unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        for p in [
            Vec3::new(15.0 + inset, 40.0 - inset, 4.0),
            Vec3::new(75.0 - inset, 40.0 - inset, 4.0),
            Vec3::new(45.0, 45.0 - inset, 4.0),
            Vec3::new(45.0, 35.0 - inset, 4.0),
        ] {
            body = check(&body, p, 1.5);
        }
        let mut measured = crate::measure(&body).unwrap();
        for x in [15.0 + inset * 0.5, 75.0 - inset * 0.5] {
            for y in [45.0 - inset * 0.5, (1.8 + 35.0 - inset) * 0.5] {
                let old = measured.volume;
                body = check_geometry(&body, Vec3::new(x, y, 10.0), 1.5);
                measured = crate::measure(&body).unwrap();
                let delta = measured.volume - old;
                assert!(delta > 1e-6 && delta < 500.0, "volume delta {delta}");
                check_transition_tangency(&body);
            }
        }
        assert!(measured.bbox.min.x >= -1e-4 && measured.bbox.min.y >= -1e-4 && measured.bbox.min.z >= -1e-4);
        assert!(measured.bbox.max.x <= 90.0 + 1e-4 && measured.bbox.max.y <= 48.0 + 1e-4 && measured.bbox.max.z <= 16.0 + 1e-4);
        let step = crate::step_export_bodies(
            &[crate::ExportBody { name: "Basin transitions".into(), body: &body, color: None }],
            &crate::StepHeader::default(),
        )
        .unwrap();
        let imported = crate::step_import(&step).unwrap();
        let restored = &imported.bodies[0].body;
        assert!(restored.validity().is_empty());
        assert_eq!(restored.unmeshed_faces(), 0);
        assert!((crate::measure(restored).unwrap().volume - measured.volume).abs() < 0.05);
    }

    #[test]
    fn convex_fan_joint_cannot_use_backward_extension() {
        // Complement the shallow basin: the same seam and cylinder still cross
        // backwards, but the joint now faces outwards instead of into the pocket.
        let slab = crate::box_solid(Vec3::ZERO, Vec3::new(90.0, 48.0, 16.0)).unwrap();
        let b = crate::boolean(&slab, &shallow_basin(), BoolOp::Cut).unwrap().unwrap();
        let inset = 12.0 * 10.0_f64.to_radians().tan();
        let pick = Vec3::new(45.0, 35.0 - inset, 4.0);
        let (index, _) = b.nearest_edge(pick, 1e-4).unwrap().unwrap();
        let edges = Body::unique_edges(&b.solid);
        let before: Vec<_> = b.solid.vertex_iter().map(|v| vtx(&v)).collect();
        let err = fillet_one(&b.solid, &edges[index], b.size(), 1.5).unwrap_err();
        assert!(err.to_string().contains("convex endpoint joint"), "{err}");
        assert!(crate::fillet(&b, &[pick], 1.5).is_err());
        assert_eq!(before, b.solid.vertex_iter().map(|v| vtx(&v)).collect::<Vec<_>>());
    }

    #[test]
    fn endpoint_trim_rejects_nonfinite_inputs_and_setback() {
        let b = basin(false);
        let (index, _) = b.nearest_edge(Vec3::new(25.0, 12.0, 20.0), 1e-4).unwrap().unwrap();
        let edges = Body::unique_edges(&b.solid);
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0, f64::MAX] {
            assert!(fillet_one(&b.solid, &edges[index], b.size(), invalid).is_err());
            assert!(fillet_one(&b.solid, &edges[index], invalid, 2.0).is_err());
        }
        let line = mt::Curve::Line(mt::Line(p3(Vec3::ZERO), p3(Vec3::X)));
        assert!(crossing(&line, |_| f64::NAN, 1e-6).is_err());
        assert!(crossing(&line, |p| if p.x == 0.0 { f64::INFINITY } else { p.x - 0.5 }, 1e-6).is_err());
    }

    #[test]
    fn oversized_endpoint_trim_preserves_input() {
        let b = basin(true);
        let before: Vec<_> = b.solid.vertex_iter().map(|v| vtx(&v)).collect();
        for r in [100.0, 1000.0] {
            assert!(crate::fillet(&b, &[Vec3::new(25.0, 12.0, 20.0)], r).is_err(), "radius {r}");
        }
        let after: Vec<_> = b.solid.vertex_iter().map(|v| vtx(&v)).collect();
        assert_eq!(before, after);
        assert!(b.validity().is_empty());
    }

    #[test]
    fn endpoint_section_cannot_cross_a_hole() {
        let b = basin(false);
        let pick = Vec3::new(25.0, 12.0, 20.0);
        let rounded = crate::fillet(&b, &[pick], 2.0).unwrap();
        // Put a small through-hole in the floor exactly where the new elliptical
        // endpoint would pass. The rails still fit their side faces.
        let q = rounded
            .solid
            .face_iter()
            .filter(|f| plane_normal(f).is_none())
            .flat_map(|f| f.edge_iter())
            .filter(|e| !matches!(e.curve(), mt::Curve::Line(_)))
            .map(|e| {
                let c = e.curve();
                let (a, b) = c.range_tuple();
                from_p3(c.subs((a + b) * 0.5))
            })
            .find(|p| (p.z - 10.0).abs() < 1e-6)
            .unwrap();
        // Keep the fixture planar: a cylinder cut at this shallow wall/floor
        // meeting can fail before it reaches the endpoint-containment check.
        let hole = crate::box_solid(Vec3::new(q.x - 0.15, q.y - 0.15, -1.0), Vec3::new(q.x + 0.15, q.y + 0.15, 12.0)).unwrap();
        let drilled = crate::boolean(&b, &hole, BoolOp::Cut).unwrap().unwrap();
        assert!(crate::fillet(&drilled, &[pick], 2.0).is_err());
        assert!(drilled.validity().is_empty());
    }
}

#[cfg(test)]
#[path = "blend_end/flow_tests.rs"]
mod flow_tests;
