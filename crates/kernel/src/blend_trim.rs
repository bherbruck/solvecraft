//! Trace an endpoint section across trimmed faces, consuming short intermediate
//! edges when necessary. Every retained boundary is shared by the rebuilt faces.

use crate::blend::{extension_fits, plane_normal, unsupported, vtx};
use crate::blend_end::{conic, curved_section, on_end, trim, uncrossed};
use crate::body::{Body, Solid, from_p3, p3};
use crate::{KernelError, Result};
use solidcraft_geom::Vec3;
use std::collections::{HashMap, HashSet};
use truck_modeling::{self as mt, BoundedCurve, ParameterDivision1D, ParametricCurve, ParametricSurface3D, SearchNearestParameter, builder};

/// Containment in the original trimmed face, including holes. A closed rebuilt
/// loop alone cannot distinguish trimming a corner from adding an exterior lobe.
pub(crate) struct FaceRegion {
    surface: mt::Surface,
    loops: Vec<Vec<(f64, f64)>>,
}

fn reflex_vertex(face: &mt::Face, vertex: &mt::Vertex) -> bool {
    let surf = face.oriented_surface();
    let Some((u, v)) = surf.search_nearest_parameter(vertex.point(), None, 100) else { return false };
    let n = surf.normal(u, v);
    let n = Vec3::new(n.x, n.y, n.z);
    let tangent = |e: &mt::Edge, at_back: bool| {
        let c = e.oriented_curve();
        let (a, b) = c.range_tuple();
        let d = c.der(if at_back { b } else { a });
        Vec3::new(d.x, d.y, d.z).normalized()
    };
    let incoming = face.boundary_iters().into_iter().flatten().find(|e| e.back() == vertex).and_then(|e| tangent(&e, true));
    let outgoing = face.boundary_iters().into_iter().flatten().find(|e| e.front() == vertex).and_then(|e| tangent(&e, false));
    matches!((incoming, outgoing), (Some(a), Some(b)) if a.cross(b).dot(n) < -1e-6)
}

impl FaceRegion {
    pub(crate) fn new(face: &mt::Face, tol: f64) -> Result<Self> {
        let surface = face.surface();
        let mut loops = Vec::new();
        let mut count = 0;
        for wire in face.boundaries() {
            let mut poly = Vec::new();
            for e in wire.edge_iter() {
                let c = e.oriented_curve();
                let mut pts = c.parameter_division(c.range_tuple(), tol.max(1e-6)).1;
                pts.pop();
                count += pts.len();
                if count > 8192 {
                    return Err(unsupported("fillet face containment subdivision limit"));
                }
                for p in pts {
                    let uv = surface.search_nearest_parameter(p, None, 100).ok_or_else(|| unsupported("fillet face boundary parameters"))?;
                    if !uv.0.is_finite() || !uv.1.is_finite() {
                        return Err(unsupported("invalid fillet face parameters"));
                    }
                    poly.push(uv);
                }
            }
            if poly.len() < 3 {
                return Err(unsupported("degenerate fillet end face"));
            }
            loops.push(poly);
        }
        Ok(Self { surface, loops })
    }

    pub(crate) fn contains(&self, p: mt::Point3, tol: f64) -> bool {
        use mt::ParametricSurface;
        let Some((u, v)) = self.surface.search_nearest_parameter(p, None, 100) else { return false };
        if !u.is_finite() || !v.is_finite() || from_p3(self.surface.subs(u, v)).dist(from_p3(p)) > tol * 10.0 {
            return false;
        }
        let mut inside = false;
        for poly in &self.loops {
            for (a, b) in poly.iter().zip(poly.iter().cycle().skip(1)).take(poly.len()) {
                if (a.1 > v) != (b.1 > v) && a.0 + (v - a.1) * (b.0 - a.0) / (b.1 - a.1) > u {
                    inside = !inside;
                }
            }
        }
        inside
    }
}

fn roots(c: &mt::Curve, f: impl Fn(Vec3) -> f64, tol: f64) -> Result<Vec<(f64, Vec3)>> {
    let (lo, hi) = c.range_tuple();
    if matches!(c, mt::Curve::Line(_)) {
        // The two callers use a plane or squared cylinder distance. On a line
        // these are exactly quadratic: do not miss two nearby crossings just
        // because the endpoints have the same sign.
        let (f0, fm, f1) = (f(from_p3(c.subs(lo))), f(from_p3(c.subs((lo + hi) * 0.5))), f(from_p3(c.subs(hi))));
        let (a, b, cc) = (2.0 * (f0 + f1 - 2.0 * fm), 4.0 * fm - 3.0 * f0 - f1, f0);
        if ![a, b, cc].iter().all(|x| x.is_finite()) {
            return Err(unsupported("invalid fillet boundary intersection"));
        }
        let eps = a.abs().max(b.abs()).max(cc.abs()).max(1.0) * 1e-12;
        let mut ts = if a.abs() <= eps {
            if b.abs() <= eps { Vec::new() } else { vec![-cc / b] }
        } else {
            let disc = b * b - 4.0 * a * cc;
            if !disc.is_finite() {
                return Err(unsupported("invalid fillet boundary intersection"));
            }
            let disc_tol = (b * b + (4.0 * a * cc).abs()) * f64::EPSILON * 64.0;
            if disc.abs() <= disc_tol && (0.0..=1.0).contains(&(-b / (2.0 * a))) {
                return Err(unsupported("a fillet section is tangent to a seam"));
            }
            if disc < 0.0 {
                Vec::new()
            } else {
                let q = -0.5 * (b + if b >= 0.0 { disc.sqrt() } else { -disc.sqrt() });
                if q.abs() <= eps { vec![-b / (2.0 * a)] } else { vec![q / a, cc / q] }
            }
        };
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let mut hits = Vec::new();
        for u in ts.into_iter().filter(|u| (0.0..=1.0).contains(u)) {
            let t = lo + (hi - lo) * u;
            let p = from_p3(c.subs(t));
            if p.dist(from_p3(c.subs(lo))) <= tol || p.dist(from_p3(c.subs(hi))) <= tol || (2.0 * a * u + b).abs() <= eps {
                return Err(unsupported("a fillet section touches a seam or its endpoint"));
            }
            hits.push((t, p));
        }
        return Ok(hits);
    }
    let mut out = Vec::new();
    let mut divisions = c.parameter_division((lo, hi), tol.max(1e-6)).0;
    // Also divide long, almost straight splines: a cylinder can cross a straight
    // chord twice even when tessellation does not need to subdivide it.
    divisions.extend((0..=256).map(|i| lo + (hi - lo) * i as f64 / 256.0));
    divisions.sort_by(f64::total_cmp);
    divisions.dedup_by(|a, b| (*a - *b).abs() <= (hi - lo).abs() * 1e-12);
    if divisions.len() > 8192 {
        return Err(unsupported("fillet boundary subdivision limit"));
    }
    let mut prev = (lo, f(from_p3(c.subs(lo))));
    for u in divisions.into_iter().skip(1) {
        let y = f(from_p3(c.subs(u)));
        if !prev.1.is_finite() || !y.is_finite() {
            return Err(unsupported("invalid fillet boundary intersection"));
        }
        if (prev.1 < 0.0) != (y < 0.0) {
            let (mut a, mut b) = (prev.0, u);
            for _ in 0..55 {
                let m = (a + b) * 0.5;
                let z = f(from_p3(c.subs(m)));
                if !z.is_finite() {
                    return Err(unsupported("invalid fillet boundary intersection"));
                }
                if (z < 0.0) == (prev.1 < 0.0) {
                    a = m;
                } else {
                    b = m;
                }
            }
            let t = (a + b) * 0.5;
            let p = from_p3(c.subs(t));
            if p.dist(from_p3(c.subs(lo))) <= tol || p.dist(from_p3(c.subs(hi))) <= tol {
                return Err(unsupported("a fillet section passes exactly through a vertex"));
            }
            out.push((t, p));
        }
        prev = (u, y);
    }
    Ok(out)
}

#[derive(Clone)]
struct Hit {
    edge: mt::Edge,
    t: f64,
    v: mt::Vertex,
}

pub(crate) fn fillet_one(solid: &Solid, edge: &mt::Edge, size: f64, r: f64) -> Result<(Solid, crate::record::Rebuild)> {
    if !(r.is_finite() && r > 0.0 && (size * 4.0).is_finite() && size > 0.0) {
        return Err(unsupported("invalid fillet radius or body size"));
    }
    let (si, shell) = solid
        .boundaries()
        .iter()
        .enumerate()
        .find(|(_, sh)| sh.edge_iter().any(|e| e.id() == edge.id()))
        .ok_or_else(|| unsupported("missing fillet shell"))?;
    let faces: Vec<_> = shell.face_iter().collect();
    let owners = |id| faces.iter().enumerate().filter(|(_, f)| f.edge_iter().any(|e| e.id() == id)).map(|(i, _)| i).collect::<Vec<_>>();
    let side = owners(edge.id());
    let [fa, fb] = side[..] else {
        return Err(unsupported("non-manifold fillet edge"));
    };
    let (f1, f2) = (faces.get(fa).ok_or_else(|| unsupported("side face"))?, faces.get(fb).ok_or_else(|| unsupported("side face"))?);
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else {
        return Err(unsupported("fillet sides must be planar"));
    };
    let inward = |f: &mt::Face, n: Vec3| {
        let e = f.boundary_iters().into_iter().flatten().find(|e| e.id() == edge.id())?;
        n.cross(vtx(e.back()) - vtx(e.front())).normalized()
    };
    let (Some(t1), Some(t2)) = (inward(f1, n1), inward(f2, n2)) else {
        return Err(unsupported("fillet side directions"));
    };
    let sign = if n1.dot(t2) > 1e-6 && n2.dot(t1) > 1e-6 {
        1.0
    } else if n1.dot(t2) < -1e-6 && n2.dot(t1) < -1e-6 {
        -1.0
    } else {
        return Err(unsupported("tangent fillet sides"));
    };
    let (p0, p1) = (vtx(edge.absolute_front()), vtx(edge.absolute_back()));
    let d = (p1 - p0).normalized().ok_or_else(|| unsupported("zero length fillet"))?;
    let alpha = t1.dot(t2).clamp(-1.0, 1.0).acos();
    if !(alpha > 1e-3 && alpha < std::f64::consts::PI - 1e-3) {
        return Err(unsupported("tangent fillet sides"));
    }
    let s = r / (alpha * 0.5).tan();
    if !(s.is_finite() && s > 0.0 && s < size) {
        return Err(unsupported("fillet setback exceeds body size"));
    }
    let w = (alpha * 0.5).sin();
    let tol = (size * 1e-8).max(1e-7);
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    for v in solid.vertex_iter() {
        bounds.add(vtx(&v));
    }
    let fits = |p: Vec3| {
        p.is_finite()
            && p.x >= bounds.min.x - tol
            && p.x <= bounds.max.x + tol
            && p.y >= bounds.min.y - tol
            && p.y <= bounds.max.y + tol
            && p.z >= bounds.min.z - tol
            && p.z <= bounds.max.z + tol
    };
    let base = conic(p0 + t1 * s, p0, p0 + t2 * s, w);
    let center = p0 + t1 * s + n1 * (sign * r);
    let radial = |p: Vec3| {
        let x = p - center;
        let x = x - d * x.dot(d);
        x.dot(x) - r * r
    };
    let angle = |p: Vec3| -> Option<f64> {
        let q = p - d * (p - p0).dot(d);
        let u = base.search_nearest_parameter(p3(q), None, 100)?;
        ((0.0..=1.0).contains(&u) && from_p3(base.subs(u)).dist(q) < tol * 10.0).then_some(u)
    };
    // Contact lines can pass a short edge left by an earlier blend. Locate them
    // against the entire trimmed side face, not just the two incident edges.
    let contact = |f: &mt::Face, t: Vec3| -> Result<[Hit; 2]> {
        let mut hits = Vec::new();
        for e in f.edge_iter() {
            if e.id() == edge.id() {
                continue;
            }
            let c = e.curve();
            for (u, p) in roots(&c, |q| (q - p0).dot(t) - s, tol)? {
                hits.push(Hit { edge: e.absolute_clone(), t: u, v: builder::vertex(p3(p)) });
            }
        }
        hits.sort_by(|a, b| (vtx(&a.v) - p0).dot(d).total_cmp(&(vtx(&b.v) - p0).dot(d)));
        let mid = p0.dist(p1) * 0.5;
        let pair = hits
            .windows(2)
            .find(|h| (vtx(&h[0].v) - p0).dot(d) < mid && (vtx(&h[1].v) - p0).dot(d) > mid)
            .ok_or_else(|| unsupported("fillet contacts leave the side face"))?;
        let [a, b] = pair else {
            return Err(unsupported("fillet contacts"));
        };
        let (qa, qb) = (vtx(&a.v), vtx(&b.v));
        if !extension_fits(f, qa + d * tol * 20.0, qb - d * tol * 20.0, tol) {
            return Err(unsupported("fillet contact crosses a side boundary"));
        }
        Ok([a.clone(), b.clone()])
    };
    let aa = contact(f1, t1)?;
    let bb = contact(f2, t2)?;
    let mut cuts: HashMap<mt::EdgeID, Hit> = HashMap::new();
    let mut caps: HashMap<usize, Vec<mt::Edge>> = HashMap::new();
    let mut ends = Vec::new();
    for (start, goal) in aa.iter().zip(&bb) {
        if cuts.insert(start.edge.id(), start.clone()).is_some() || cuts.insert(goal.edge.id(), goal.clone()).is_some() {
            return Err(unsupported("fillet sections cross the same seam twice"));
        }
        let mut current = start.clone();
        let mut fi = owners(start.edge.id()).into_iter().find(|f| *f != fa).ok_or_else(|| unsupported("open fillet endpoint"))?;
        let mut u0 = 0.0;
        let mut chain = Vec::new();
        for _ in 0..32 {
            let f = faces.get(fi).ok_or_else(|| unsupported("fillet endpoint face"))?;
            if fi == fa || fi == fb {
                return Err(unsupported("fillet endpoint crosses a side face"));
            }
            let mut candidates = Vec::new();
            for e in f.edge_iter() {
                if e.id() == current.edge.id() {
                    continue;
                }
                if e.id() == goal.edge.id() {
                    candidates.push((1.0, goal.clone()));
                    continue;
                }
                for (t, p) in roots(&e.curve(), radial, tol)? {
                    if let Some(u) = angle(p)
                        && u > u0 + 1e-8
                        && u < 1.0 - 1e-8
                    {
                        candidates.push((u, Hit { edge: e.absolute_clone(), t, v: builder::vertex(p3(p)) }));
                    }
                }
            }
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut next = None;
            let region = FaceRegion::new(f, tol)?;
            for (u1, hit) in candidates {
                let curve = if plane_normal(f).is_some() {
                    let origin = f.vertex_iter().next().map(|v| vtx(&v)).ok_or_else(|| unsupported("empty end face"))?;
                    let ca = on_end(f, p0 + t1 * s, d, origin)?;
                    let cm = on_end(f, p0, d, origin)?;
                    let cb = on_end(f, p0 + t2 * s, d, origin)?;
                    trim(conic(ca, cm, cb, w), u0, u1)
                } else {
                    let Ok(c) = curved_section(f, &base, d, u0, u1, vtx(&current.v), vtx(&hit.v), tol) else { continue };
                    c
                };
                let (lo, hi) = curve.range_tuple();
                if from_p3(curve.subs(lo)).dist(vtx(&current.v)) > tol * 10.0 || from_p3(curve.subs(hi)).dist(vtx(&hit.v)) > tol * 10.0 {
                    continue;
                }
                let samples = curve.parameter_division((lo, hi), tol.max(1e-6)).1;
                if samples.len() > 8192 || samples.iter().any(|p| !fits(from_p3(*p))) {
                    return Err(unsupported("fillet section leaves the part bounds"));
                }
                if !region.contains(curve.subs((lo + hi) * 0.5), tol)
                    || samples.iter().skip(1).take(samples.len().saturating_sub(2)).any(|p| !region.contains(*p, tol))
                {
                    // Cutting a convex edge can expose more of an end surface
                    // at its reflex boundary corner. Allow only this local
                    // case: both cut seams must meet at the original vertex,
                    // and the section must stay behind both material planes.
                    let local_reflex = sign < 0.0
                        && [edge.absolute_front(), edge.absolute_back()].into_iter().any(|v| {
                            (current.edge.front() == v || current.edge.back() == v)
                                && (hit.edge.front() == v || hit.edge.back() == v)
                                && reflex_vertex(f, v)
                        });
                    if !local_reflex
                        || samples.iter().any(|q| {
                            let x = from_p3(*q) - p0;
                            x.dot(n1) > tol || x.dot(n2) > tol
                        })
                    {
                        continue;
                    }
                }
                next = Some((u1, hit, curve));
                break;
            }
            let (u1, hit, curve) = next.ok_or_else(|| unsupported("cannot trace the fillet section across its end faces"))?;
            let cap = mt::Edge::try_new(&current.v, &hit.v, curve).map_err(|e| KernelError::Failed(format!("fillet section: {e}")))?;
            caps.entry(fi).or_default().push(cap.clone());
            chain.push(cap);
            if hit.edge.id() == goal.edge.id() {
                break;
            }
            if cuts.insert(hit.edge.id(), hit.clone()).is_some() {
                return Err(unsupported("fillet sections cross the same seam twice"));
            }
            fi = owners(hit.edge.id()).into_iter().find(|f| *f != fi).ok_or_else(|| unsupported("open end face"))?;
            current = hit;
            u0 = u1;
        }
        if chain.last().is_none_or(|e| e.back() != &goal.v) {
            return Err(unsupported("fillet endpoint tracing limit"));
        }
        ends.push(chain);
    }
    let la = builder::line(&aa[0].v, &aa[1].v);
    let lb = builder::line(&bb[0].v, &bb[1].v);
    caps.entry(fa).or_default().push(la.clone());
    caps.entry(fb).or_default().push(lb.clone());
    // Flood original vertices only up to the cut edges. This removes consumed
    // short seams while leaving every unaffected edge and hole in place.
    let all = Body::unique_edges(solid);
    let mut removed = HashSet::from([edge.absolute_front().id(), edge.absolute_back().id()]);
    loop {
        let before = removed.len();
        for e in &all {
            if cuts.contains_key(&e.id()) {
                continue;
            }
            if removed.contains(&e.front().id()) || removed.contains(&e.back().id()) {
                removed.insert(e.front().id());
                removed.insert(e.back().id());
            }
        }
        if removed.len() == before {
            break;
        }
    }
    let mut replacements = HashMap::new();
    for (id, hit) in &cuts {
        let e = &hit.edge;
        let (ra, rb) = (removed.contains(&e.front().id()), removed.contains(&e.back().id()));
        let c = e.curve();
        let (lo, hi) = c.range_tuple();
        let replacement = match (ra, rb) {
            (true, false) => mt::Edge::try_new(&hit.v, e.back(), trim(c, hit.t, hi)),
            (false, true) => mt::Edge::try_new(e.front(), &hit.v, trim(c, lo, hit.t)),
            _ => return Err(unsupported("fillet cut does not isolate a corner")),
        }
        .map_err(|e| KernelError::Failed(format!("fillet trimmed seam: {e}")))?;
        replacements.insert(*id, replacement);
    }
    let mut out = Vec::new();
    let mut rec = crate::record::Rebuild::default();
    rec.pieces(&replacements);
    for (fi, f) in faces.iter().enumerate() {
        let mut keep = Vec::new();
        let mut changed = false;
        for e in f.boundary_iters().into_iter().flatten() {
            if let Some(ne) = replacements.get(&e.id()) {
                keep.push(if e.orientation() { ne.clone() } else { ne.inverse() });
                changed = true;
            } else if removed.contains(&e.front().id()) {
                changed = true;
            } else {
                keep.push(e.clone());
            }
        }
        if !changed {
            out.push((*f).clone());
            continue;
        }
        let mut add = caps.remove(&fi).unwrap_or_default();
        let mut wires = Vec::new();
        // Preserve the original outer-wire-before-holes ordering.
        while !keep.is_empty() {
            let first = keep.remove(0);
            let mut es = vec![first.clone()];
            let mut v = first.back().clone();
            while v != *first.front() {
                let next = if let Some(i) = keep.iter().position(|e| *e.front() == v) {
                    keep.remove(i)
                } else if let Some(i) = add.iter().position(|e| *e.front() == v || *e.back() == v) {
                    let e = add.remove(i);
                    if *e.front() == v { e } else { e.inverse() }
                } else {
                    return Err(unsupported("fillet leaves an open face boundary"));
                };
                v = next.back().clone();
                es.push(next);
            }
            wires.push(mt::Wire::from(es));
        }
        if !add.is_empty() {
            return Err(unsupported("fillet section does not connect to the retained face"));
        }
        if wires.is_empty() {
            continue;
        }
        let rebuilt = mt::Face::try_new(wires, f.oriented_surface()).map_err(|e| KernelError::Failed(format!("fillet end face: {e}")))?;
        uncrossed(&rebuilt, tol)?;
        rec.face(&rebuilt, f);
        out.push(rebuilt);
    }
    let [c0, c1] = &ends[..] else {
        return Err(unsupported("fillet endpoints"));
    };
    let mut es = c0.clone();
    es.push(lb);
    es.extend(c1.iter().rev().map(|e| e.inverse()));
    es.push(la.inverse());
    let mut wire = mt::Wire::from(es);
    let forward = f1.boundary_iters().into_iter().flatten().any(|e| e.id() == edge.id() && e.front() == edge.absolute_front());
    if !forward {
        wire = wire.inverse();
    }
    let (q0, q1) = (p0 - d * size, p1 + d * size);
    let h = |p: Vec3, w: f64| mt::Vector4::new(p.x * w, p.y * w, p.z * w, w);
    let ctrl = vec![vec![h(q0 + t1 * s, 1.0), h(q1 + t1 * s, 1.0)], vec![h(q0, w), h(q1, w)], vec![h(q0 + t2 * s, 1.0), h(q1 + t2 * s, 1.0)]];
    let mut surf =
        mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new((mt::KnotVec::bezier_knot(2), mt::KnotVec::bezier_knot(1)), ctrl)));
    let mid = from_p3(base.subs(0.5));
    let (u, v) = surf.search_nearest_parameter(p3(mid), None, 100).ok_or_else(|| unsupported("fillet surface parameters"))?;
    let n = surf.normal(u, v);
    if Vec3::new(n.x, n.y, n.z).dot((center - mid) * sign) < 0.0 {
        surf = mt::Invertible::inverse(&surf);
    }
    let cylinder = mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("fillet cylinder: {e}")))?;
    rec.blend(&cylinder, [edge.id()]);
    out.push(cylinder);
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.get_mut(si) {
        *sh = out.into();
    }
    Solid::try_new(shells).map(|s| (s, rec)).map_err(|e| KernelError::Failed(format!("fillet traced solid: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_intersections_cannot_skip_a_narrow_pair() {
        let line = mt::Curve::Line(mt::Line(p3(Vec3::ZERO), p3(Vec3::X * 100.0)));
        let center = 50.003;
        let radius = 0.001;
        let hits = roots(&line, |p| (p.x - center).powi(2) - radius * radius, 1e-7).unwrap();
        assert_eq!(hits.len(), 2);
        assert!((hits[0].1.x - (center - radius)).abs() < 1e-6);
        assert!((hits[1].1.x - (center + radius)).abs() < 1e-6);
        assert!(roots(&line, |p| (p.x - center).powi(2), 1e-7).is_err());
    }
}
