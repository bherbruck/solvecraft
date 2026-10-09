//! Clean-up after operations: coplanar neighbouring faces become one face, and two straight
//! edges meeting in line at a vertex nothing else uses become one edge (as Fusion keeps them).

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Solid, from_p3};

fn plane_of(f: &mt::Face) -> Option<(Vec3, f64)> {
    match f.oriented_surface() {
        mt::Surface::Plane(p) => {
            let n = p.normal();
            let n = Vec3::new(n.x, n.y, n.z).normalized()?;
            let o = p.origin();
            Some((n, n.dot(Vec3::new(o.x, o.y, o.z))))
        }
        _ => None,
    }
}

/// A face like `f` with new boundary wires given in `f`'s absolute (stored) orientation:
/// built on the stored surface and turned like `f` (rebuilding on the turned surface can make
/// the mesher pick the wrong side of a trim on periodic surfaces).
pub(crate) fn absolute_face(f: &mt::Face, wires: Vec<mt::Wire>) -> Option<mt::Face> {
    let mut nf = mt::Face::try_new(wires, f.surface()).ok()?;
    if !f.orientation() {
        nf.invert();
    }
    Some(nf)
}

fn find(p: &mut [usize], mut i: usize) -> usize {
    while let Some(&q) = p.get(i) {
        if q == i {
            break;
        }
        i = q;
    }
    i
}

/// Points along an oriented edge (for loop areas and containment).
fn samples(e: &mt::Edge) -> Vec<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    (0..8).map(|k| t0 + (t1 - t0) * k as f64 / 8.0).map(|t| from_p3(c.subs(t))).collect()
}

fn loop_area(edges: &[mt::Edge], n: Vec3) -> f64 {
    let pts: Vec<Vec3> = edges.iter().flat_map(samples).collect();
    let k = pts.len();
    let mut s = Vec3::ZERO;
    for i in 0..k {
        if let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % k)) {
            s += a.cross(*b);
        }
    }
    s.dot(n) * 0.5
}

fn inside(p: Vec3, edges: &[mt::Edge], n: Vec3) -> bool {
    let u = n.any_perp();
    let w = n.cross(u);
    let pts: Vec<(f64, f64)> = edges.iter().flat_map(samples).map(|q| (q.dot(u), q.dot(w))).collect();
    let (px, py) = (p.dot(u), p.dot(w));
    let k = pts.len();
    let mut c = false;
    for i in 0..k {
        let (Some(&(ax, ay)), Some(&(bx, by))) = (pts.get(i), pts.get((i + 1) % k)) else { continue };
        if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
            c = !c;
        }
    }
    c
}

/// Merge each group of edge-connected coplanar faces into one face. Returns None when nothing
/// changed or the result would not be a valid solid.
/// Do the points lie along one of the lines faces were split along on purpose (polylines;
/// chords of a curved line sag by under a tenth of their length)?
pub(crate) fn on_split_line(pts: &[Vec3], keep: &[Vec<Vec3>]) -> bool {
    !pts.is_empty()
        && keep.iter().any(|line| {
            let seg = line.windows(2).filter_map(|w| Some(w.first()?.dist(*w.get(1)?))).fold(0.0, f64::max);
            let tol = 1e-6 * (1.0 + seg) + if line.len() > 2 { seg * 0.1 } else { 0.0 };
            pts.iter().all(|q| line.windows(2).any(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| q.dist_to_segment(*a, *b) <= tol)))
        })
}

/// Points along an edge (ends and three inside).
fn edge_points(e: &mt::Edge) -> Vec<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    (0..=4).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 4.0))).collect()
}

fn merge_coplanar(shell: &mt::Shell, tol: f64, keep: &[Vec<Vec3>]) -> Option<mt::Shell> {
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let planes: Vec<Option<(Vec3, f64)>> = faces.iter().map(plane_of).collect();
    let mut parent: Vec<usize> = (0..faces.len()).collect();
    let mut by_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            by_edge.entry(e.id()).or_default().push(i);
        }
    }
    let mut merged_any = false;
    let edges: HashMap<mt::EdgeID, mt::Edge> = shell.edge_iter().map(|e| (e.id(), e)).collect();
    for (id, fs) in &by_edge {
        if let [a, b] = fs[..]
            && a != b
            && (keep.is_empty() || !edges.get(id).is_some_and(|e| on_split_line(&edge_points(e), keep)))
            && let (Some(Some((na, da))), Some(Some((nb, db)))) = (planes.get(a), planes.get(b))
            && na.dot(*nb) > 1.0 - 1e-9
            && (da - db).abs() < tol
        {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                if let Some(x) = parent.get_mut(ra) {
                    *x = rb;
                }
                merged_any = true;
            }
        }
    }
    if !merged_any {
        return None;
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..faces.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<mt::Face> = Vec::new();
    let mut order: Vec<usize> = groups.keys().copied().collect();
    order.sort_unstable();
    for g in order {
        let members = groups.get(&g)?;
        if members.len() == 1 {
            out.push(faces.get(*members.first()?)?.clone());
            continue;
        }
        let (n, _) = (*planes.get(*members.first()?)?)?;
        // Oriented boundary edges of the group; edges used twice inside it are dropped.
        let mut count: HashMap<mt::EdgeID, usize> = HashMap::new();
        for m in members {
            for e in faces.get(*m)?.boundary_iters().into_iter().flatten() {
                *count.entry(e.id()).or_insert(0) += 1;
            }
        }
        let mut rest: Vec<mt::Edge> = Vec::new();
        for m in members {
            for e in faces.get(*m)?.boundary_iters().into_iter().flatten() {
                if count.get(&e.id()).copied().unwrap_or(0) == 1 {
                    rest.push(e);
                }
            }
        }
        // Chain into loops.
        let mut loops: Vec<Vec<mt::Edge>> = Vec::new();
        while let Some(first) = rest.pop() {
            let start = first.front().clone();
            let mut cur = first.back().clone();
            let mut lp = vec![first];
            let mut guard = 0;
            while cur != start && guard < 100_000 {
                guard += 1;
                let k = rest.iter().position(|e| e.front() == &cur)?;
                let e = rest.remove(k);
                cur = e.back().clone();
                lp.push(e);
            }
            loops.push(lp);
        }
        let (outer, holes): (Vec<Vec<mt::Edge>>, Vec<Vec<mt::Edge>>) = loops.into_iter().partition(|l| loop_area(l, n) > 0.0);
        let surface = faces.get(*members.first()?)?.oriented_surface();
        for o in &outer {
            let mut wires: Vec<mt::Wire> = vec![o.clone().into()];
            for h in &holes {
                let probe = h.first().map(|e| from_p3(e.front().point()))?;
                // The innermost outer loop around the hole owns it.
                let owner = outer.iter().filter(|x| inside(probe, x, n)).min_by(|a, b| loop_area(a, n).total_cmp(&loop_area(b, n)));
                if owner.is_some_and(|x| std::ptr::eq(x, o)) {
                    wires.push(h.clone().into());
                }
            }
            out.push(mt::Face::try_new(wires, surface.clone()).ok()?);
        }
    }
    Some(out.into())
}

/// Join pairs of collinear straight edges at vertices only they use.
fn merge_collinear(shell: &mt::Shell) -> Option<mt::Shell> {
    let mut faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let mut changed = false;
    for _round in 0..10_000 {
        let mut at: HashMap<mt::VertexID, Vec<mt::Edge>> = HashMap::new();
        let mut seen = std::collections::HashSet::new();
        for f in &faces {
            for e in f.edge_iter() {
                if seen.insert(e.id()) {
                    at.entry(e.front().id()).or_default().push(e.absolute_clone());
                    at.entry(e.back().id()).or_default().push(e.absolute_clone());
                }
            }
        }
        let pair = at.iter().find_map(|(v, es)| {
            let [a, b] = &es[..] else { return None };
            if !matches!(a.curve(), mt::Curve::Line(_)) || !matches!(b.curve(), mt::Curve::Line(_)) {
                return None;
            }
            let other = |e: &mt::Edge| if e.front().id() == *v { e.back().clone() } else { e.front().clone() };
            let mid = if a.front().id() == *v { a.front().clone() } else { a.back().clone() };
            let (pa, pm, pb) = (from_p3(other(a).point()), from_p3(mid.point()), from_p3(other(b).point()));
            let (u, w) = ((pm - pa).normalized()?, (pb - pm).normalized()?);
            (u.dot(w) > 1.0 - 1e-10).then(|| (a.clone(), b.clone(), other(a), other(b)))
        });
        let Some((a, b, va, vb)) = pair else { break };
        let joined = builder::line(&va, &vb);
        for f in &mut faces {
            if !f.edge_iter().any(|e| e.id() == a.id()) {
                continue;
            }
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut es: Vec<mt::Edge> = Vec::new();
                for e in w.edge_iter() {
                    if e.id() == b.id() {
                        continue;
                    }
                    if e.id() == a.id() {
                        // a+b (in either order) become the joined edge, in this wire's direction.
                        es.push(if e.front() == &va { joined.clone() } else { joined.inverse() });
                        continue;
                    }
                    es.push(e.clone());
                }
                wires.push(mt::Wire::from(es));
            }
            *f = absolute_face(f, wires)?;
        }
        changed = true;
    }
    changed.then(|| faces.into())
}

/// The exact curve an approximate edge (a boolean's intersection curve) really is: a line or a
/// circular arc, when every sample lies on it.
fn exact_curve(e: &mt::Edge, tol: f64) -> Option<mt::Edge> {
    // Only the approximate curves booleans make; exact ones stay as they are.
    if !matches!(e.curve(), mt::Curve::IntersectionCurve(_)) {
        return None;
    }
    fit_exact(e, tol)
}

/// A line or circular arc through the edge's samples, if they all lie on one.
fn fit_exact(e: &mt::Edge, tol: f64) -> Option<mt::Edge> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec3> = (0..=16).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    let (a, b) = (from_p3(e.absolute_front().point()), from_p3(e.absolute_back().point()));
    if e.absolute_front() == e.absolute_back() || a.dist(b) < tol {
        return None;
    }
    if pts.iter().all(|p| p.dist_to_segment(a, b) < tol) {
        return Some(builder::line(e.absolute_front(), e.absolute_back()));
    }
    // A circle through the ends and the middle sample.
    let m = *pts.get(8)?;
    let n = (m - a).cross(b - a).normalized()?;
    let (ab, am) = (b - a, m - a);
    // Centre in the plane: solve |c-a| = |c-b| = |c-m|.
    let d = 2.0 * ab.cross(am).len2();
    if d < 1e-300 {
        return None;
    }
    // Circumcentre: a + (|u|² v×w + |v|² w×u) / (2|w|²), u = b−a, v = m−a, w = u×v.
    let w = ab.cross(am);
    let centre = a + (am.cross(w) * ab.len2() + w.cross(ab) * am.len2()) * (1.0 / d);
    let r = centre.dist(a);
    if pts.iter().all(|p| (p.dist(centre) - r).abs() < tol && (*p - a).dot(n).abs() < tol) {
        return Some(builder::circle_arc(e.absolute_front(), e.absolute_back(), mt::Point3::new(m.x, m.y, m.z)));
    }
    None
}

/// Replace approximate edges that are really lines or arcs with exact ones.
fn exact_edges(shell: &mt::Shell, tol: f64) -> Option<mt::Shell> {
    let mut subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    for e in shell.edge_iter() {
        if !subst.contains_key(&e.id())
            && let Some(x) = exact_curve(&e, tol)
        {
            subst.insert(e.id(), x);
        }
    }
    if subst.is_empty() {
        return None;
    }
    let mut faces = Vec::new();
    for f in shell.face_iter() {
        let mut wires = Vec::new();
        for w in f.absolute_boundaries() {
            let es: Vec<mt::Edge> = w
                .edge_iter()
                .map(|e| match subst.get(&e.id()) {
                    // The new edge runs absolute front → back; match this use's direction.
                    Some(n) => {
                        if e.front() == e.absolute_front() {
                            n.clone()
                        } else {
                            n.inverse()
                        }
                    }
                    None => e.clone(),
                })
                .collect();
            wires.push(mt::Wire::from(es));
        }
        faces.push(absolute_face(f, wires)?);
    }
    Some(faces.into())
}

/// Faces whose surface is flat but not a plane (a revolved line square to the axis, a swept
/// line) get a real plane, so they merge with their coplanar neighbours and blend like planes.
fn planar_surfaces(shell: &mt::Shell, tol: f64) -> Option<mt::Shell> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let mut changed = false;
    let mut faces = Vec::new();
    for f in shell.face_iter() {
        let surf = f.surface();
        if matches!(surf, mt::Surface::Plane(_)) {
            faces.push(f.clone());
            continue;
        }
        // Parameter box of the boundary, then a grid of surface points in it.
        let mut uv: Vec<(f64, f64)> = Vec::new();
        let mut pts: Vec<Vec3> = Vec::new();
        for w in f.absolute_boundaries() {
            for e in w.edge_iter() {
                let c = e.oriented_curve();
                let (t0, t1) = c.range_tuple();
                for k in 0..6 {
                    let p = c.subs(t0 + (t1 - t0) * k as f64 / 6.0);
                    pts.push(from_p3(p));
                    if let Some(x) = surf.search_nearest_parameter(p, uv.last().copied(), 50) {
                        uv.push(x);
                    }
                }
            }
        }
        if uv.len() < 3 || pts.len() < 3 {
            faces.push(f.clone());
            continue;
        }
        let (u0, u1) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, x| (a.0.min(x.0), a.1.max(x.0)));
        let (v0, v1) = uv.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, x| (a.0.min(x.1), a.1.max(x.1)));
        for i in 0..=4 {
            for j in 0..=4 {
                pts.push(from_p3(surf.subs(u0 + (u1 - u0) * i as f64 / 4.0, v0 + (v1 - v0) * j as f64 / 4.0)));
            }
        }
        // The plane through the points (normal from the surface).
        let (um, vm) = ((u0 + u1) / 2.0, (v0 + v1) / 2.0);
        let nn = surf.normal(um, vm);
        let Some(n) = Vec3::new(nn.x, nn.y, nn.z).normalized() else {
            faces.push(f.clone());
            continue;
        };
        let o = from_p3(surf.subs(um, vm));
        if pts.iter().any(|p| (*p - o).dot(n).abs() > tol) {
            faces.push(f.clone());
            continue;
        }
        let u = n.any_perp();
        let w = n.cross(u);
        let plane = mt::Plane::new(crate::body::p3(o), crate::body::p3(o + u), crate::body::p3(o + w));
        match mt::Face::try_new(f.absolute_boundaries().clone(), mt::Surface::Plane(plane)) {
            Ok(mut nf) => {
                if !f.orientation() {
                    nf.invert();
                }
                faces.push(nf);
                changed = true;
            }
            Err(_) => faces.push(f.clone()),
        }
    }
    changed.then(|| faces.into())
}

/// Heal a solid; returns the input unchanged when nothing applies or healing fails.
pub fn heal(solid: Solid, size: f64) -> Solid {
    heal_keep(solid, size, &[])
}

/// [`heal`], keeping faces apart along lines they were split along on purpose.
pub(crate) fn heal_keep(solid: Solid, size: f64, keep: &[Vec<Vec3>]) -> Solid {
    let tol = (size * 1e-7).max(1e-9);
    let mut shells = Vec::new();
    let mut changed = false;
    for sh in solid.boundaries() {
        let mut cur = sh.clone();
        if let Some(m) = planar_surfaces(&cur, (size * 1e-7).max(1e-9)) {
            cur = m;
            changed = true;
        }
        if let Some(m) = exact_edges(&cur, (size * 1e-5).max(1e-7)) {
            cur = m;
            changed = true;
        }
        if let Some(m) = merge_coplanar(&cur, tol, keep) {
            cur = m;
            changed = true;
        }
        if let Some(m) = merge_collinear(&cur) {
            cur = m;
            changed = true;
        }
        shells.push(cur);
    }
    if !changed {
        return solid;
    }
    Solid::try_new(shells).unwrap_or(solid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_arcs_become_exact() {
        // A cubic (non-rational) spline through a quarter circle.
        let (a, b) = (builder::vertex(mt::Point3::new(10.0, 0.0, 0.0)), builder::vertex(mt::Point3::new(0.0, 10.0, 0.0)));
        let pts: Vec<Vec3> = (0..=40)
            .map(|k| {
                let t = std::f64::consts::FRAC_PI_2 * k as f64 / 40.0;
                Vec3::new(10.0 * t.cos(), 10.0 * t.sin(), 0.0)
            })
            .collect();
        let curve = mt::Curve::BSplineCurve(crate::build::interpolate_cubic(&pts).expect("spline"));
        let e = mt::Edge::new(&a, &b, curve);
        let x = fit_exact(&e, 1e-2).expect("an arc");
        use mt::{BoundedCurve, ParametricCurve};
        let c = x.curve();
        let (t0, t1) = c.range_tuple();
        let mid = c.subs((t0 + t1) / 2.0);
        assert!(((mid.x * mid.x + mid.y * mid.y).sqrt() - 10.0).abs() < 1e-9, "{mid:?}");
    }
}
