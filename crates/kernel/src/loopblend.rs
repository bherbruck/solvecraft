//! Blending a whole boundary loop of a planar face whose neighbours are walls standing
//! perpendicular to it (planes, and cylinders with axes along the face normal): a pocket floor,
//! the top outline of an extruded plate. The loop must be smooth (lines and arcs meeting
//! tangentially). The face shrinks by the radius, the walls are trimmed by it, and each loop edge
//! gets a blend face: a cylinder (or plane, for a chamfer) along a line, a torus (or cone) along
//! an arc, all meeting along the profile arcs at the joints. The result is exact.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Solid, from_p3, p3, v3};
use crate::{KernelError, Result};

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

#[derive(Clone, Copy, Debug)]
enum Seg {
    Line {
        a: Vec3,
        b: Vec3,
    },
    /// Arc from `a` to `b` about `c` turning by `angle` about `axis` (right hand, angle > 0).
    Arc {
        a: Vec3,
        c: Vec3,
        axis: Vec3,
        angle: f64,
        mid: Vec3,
    },
}

impl Seg {
    fn start(&self) -> Vec3 {
        match *self {
            Seg::Line { a, .. } | Seg::Arc { a, .. } => a,
        }
    }
    /// Unit tangent (direction of travel) at a point of the segment.
    fn tangent(&self, p: Vec3) -> Option<Vec3> {
        match *self {
            Seg::Line { a, b } => (b - a).normalized(),
            Seg::Arc { c, axis, .. } => axis.cross(p - c).normalized(),
        }
    }
}

/// The exact shape of an oriented loop edge (a line, or an arc found from samples).
fn seg_of(e: &mt::Edge, tol: f64) -> Option<Seg> {
    use mt::{BoundedCurve, ParametricCurve};
    let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
    let c = e.oriented_curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec3> = (0..=16).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    if pts.iter().all(|p| p.dist_to_segment(a, b) < tol) {
        return Some(Seg::Line { a, b });
    }
    let m = *pts.get(8)?;
    let (u, v) = (b - a, m - a);
    let w = u.cross(v);
    let d = 2.0 * w.len2();
    if d < 1e-300 {
        return None;
    }
    let centre = a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / d);
    let r = centre.dist(a);
    if !pts.iter().all(|p| (p.dist(centre) - r).abs() < tol) {
        return None;
    }
    // Turning direction: the travel at a is axis × (a − c).
    let t_a = (*pts.get(1)? - a).normalized()?;
    let mut axis = w.normalized()?;
    if axis.cross(a - centre).dot(t_a) < 0.0 {
        axis = -axis;
    }
    let (ra, rb) = ((a - centre).normalized()?, (b - centre).normalized()?);
    let mut angle = ra.dot(rb).clamp(-1.0, 1.0).acos();
    if ra.cross(rb).dot(axis) < 0.0 {
        angle = std::f64::consts::TAU - angle;
    }
    Some(Seg::Arc { a, c: centre, axis, angle, mid: m })
}

fn plane_normal(f: &mt::Face) -> Option<Vec3> {
    match f.oriented_surface() {
        mt::Surface::Plane(p) => {
            let n = p.normal();
            Vec3::new(n.x, n.y, n.z).normalized()
        }
        _ => None,
    }
}

/// Outward normal of a face at a point on it.
fn face_normal(f: &mt::Face, p: Vec3) -> Option<Vec3> {
    use mt::{ParametricSurface3D, SearchNearestParameter, SearchParameter};
    let s = f.oriented_surface();
    let (u, v) = s.search_parameter(p3(p), None, 100).or_else(|| s.search_nearest_parameter(p3(p), None, 100))?;
    let n = s.normal(u, v);
    Vec3::new(n.x, n.y, n.z).normalized()
}

/// Blend the loop of `edges` (by id) if it is a whole smooth boundary loop of a planar face with
/// perpendicular walls. `None` when the edges are not such a loop.
pub(crate) fn loop_blend(solid: &Solid, edge_ids: &[mt::EdgeID], size: f64, r: f64, round: bool) -> Option<Result<Solid>> {
    let tol = (size * 1e-6).max(1e-7);
    for (si, shell) in solid.boundaries().iter().enumerate() {
        let faces: Vec<&mt::Face> = shell.face_iter().collect();
        for (fi, f) in faces.iter().enumerate() {
            let Some(n) = plane_normal(f) else { continue };
            for (wi, w) in f.boundaries().iter().enumerate() {
                let ids: Vec<mt::EdgeID> = w.edge_iter().map(|e| e.id()).collect();
                if ids.len() != edge_ids.len() || !edge_ids.iter().all(|e| ids.contains(e)) {
                    continue;
                }
                return Some(build(solid, si, &faces, fi, wi, n, size, tol, r, round));
            }
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn build(solid: &Solid, si: usize, faces: &[&mt::Face], fi: usize, wi: usize, n: Vec3, size: f64, tol: f64, r: f64, round: bool) -> Result<Solid> {
    let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
    let wire = f.boundaries().get(wi).cloned().ok_or_else(|| KernelError::Failed("loop".into()))?;
    let edges: Vec<mt::Edge> = wire.edge_iter().cloned().collect();
    let k = edges.len();
    let segs: Vec<Seg> = edges
        .iter()
        .map(|e| seg_of(e, (size * 1e-5).max(1e-6)))
        .collect::<Option<_>>()
        .ok_or_else(|| unsupported("loop edges must be lines or arcs"))?;
    // Smooth joints.
    for i in 0..k {
        let (Some(prev), Some(cur)) = (segs.get((i + k - 1) % k), segs.get(i)) else { continue };
        let p = cur.start();
        let (Some(ta), Some(tb)) = (prev.tangent(p), cur.tangent(p)) else { return Err(unsupported("degenerate loop edge")) };
        if ta.dot(tb) < 1.0 - 1e-6 {
            return Err(unsupported("blending a loop with sharp corners"));
        }
    }
    // Walls: the other face of each edge, perpendicular to the face; all on one side.
    let mut walls: Vec<usize> = Vec::with_capacity(k);
    let mut side = 0.0;
    for (e, s) in edges.iter().zip(&segs) {
        let wf = faces
            .iter()
            .position(|g| !std::ptr::eq(*g, *f) && g.edge_iter().any(|x| x.id() == e.id()))
            .ok_or_else(|| unsupported("an edge with one face"))?;
        let mid = match *s {
            Seg::Line { a, b } => (a + b) * 0.5,
            Seg::Arc { mid, .. } => mid,
        };
        let g = faces.get(wf).ok_or_else(|| KernelError::Failed("wall".into()))?;
        let gn = face_normal(g, mid).ok_or_else(|| unsupported("wall normal"))?;
        if gn.dot(n).abs() > 1e-6 {
            return Err(unsupported("the walls must stand perpendicular to the face"));
        }
        // Into the face from the loop: left of travel.
        let m = n.cross(s.tangent(mid).ok_or_else(|| unsupported("tangent"))?);
        let sd = if gn.dot(m) > 0.0 { 1.0 } else { -1.0 };
        if side != 0.0 && sd != side {
            return Err(unsupported("a loop that is partly convex and partly concave"));
        }
        side = sd;
        walls.push(wf);
    }
    // Offsets: into the face by r; along the wall by r (up a concave wall, down a convex one).
    let into = |s: &Seg, p: Vec3| -> Vec3 { s.tangent(p).map(|t| n.cross(t)).unwrap_or(Vec3::ZERO) };
    let lift = n * (r * side);
    let pa: Vec<Vec3> = (0..k).map(|i| segs.get(i).map(|s| s.start() + into(s, s.start()) * r).unwrap_or_default()).collect();
    let pb: Vec<Vec3> = (0..k).map(|i| segs.get(i).map(|s| s.start() + lift).unwrap_or_default()).collect();
    // The offset arcs must not collapse.
    for s in &segs {
        if let Seg::Arc { a, c, .. } = *s {
            let toward = into(s, a).dot(c - a) > 0.0;
            if toward && a.dist(c) <= r + tol {
                return Err(unsupported("the blend is larger than a corner radius"));
            }
        }
    }
    let va: Vec<mt::Vertex> = pa.iter().map(|p| builder::vertex(p3(*p))).collect();
    let vb: Vec<mt::Vertex> = pb.iter().map(|p| builder::vertex(p3(*p))).collect();
    let centre = |i: usize| pa.get(i).copied().unwrap_or_default() + lift;
    let profile_mid = |i: usize| {
        let (Some(s), c) = (segs.get(i), centre(i)) else { return Vec3::ZERO };
        c + (s.start() - c).normalized().unwrap_or(n) * r
    };
    // Profiles at the joints (face side → wall side).
    let profiles: Vec<mt::Edge> = (0..k)
        .map(|i| {
            let (Some(a), Some(b)) = (va.get(i), vb.get(i)) else {
                return builder::line(&builder::vertex(p3(Vec3::ZERO)), &builder::vertex(p3(Vec3::X)));
            };
            if round { builder::circle_arc(a, b, p3(profile_mid(i))) } else { builder::line(a, b) }
        })
        .collect();
    let rail = |vs: &[mt::Vertex], i: usize, offset: &dyn Fn(Vec3, &Seg) -> Vec3| -> Option<mt::Edge> {
        let s = segs.get(i)?;
        let (a, b) = (vs.get(i)?, vs.get((i + 1) % k)?);
        Some(match *s {
            Seg::Line { .. } => builder::line(a, b),
            Seg::Arc { mid, .. } => builder::circle_arc(a, b, p3(offset(mid, s))),
        })
    };
    let face_off = |p: Vec3, s: &Seg| p + into(s, p) * r;
    let wall_off = |p: Vec3, _: &Seg| p + lift;
    let face_rails: Vec<mt::Edge> =
        (0..k).map(|i| rail(&va, i, &face_off)).collect::<Option<_>>().ok_or_else(|| KernelError::Failed("rail".into()))?;
    let wall_rails: Vec<mt::Edge> =
        (0..k).map(|i| rail(&vb, i, &wall_off)).collect::<Option<_>>().ok_or_else(|| KernelError::Failed("rail".into()))?;
    // Seams between neighbouring walls at the joints get shorter.
    let loop_vertex: Vec<mt::Vertex> = edges.iter().map(|e| e.front().clone()).collect();
    let mut seam_subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    let loop_ids: Vec<mt::EdgeID> = edges.iter().map(|e| e.id()).collect();
    for (i, v) in loop_vertex.iter().enumerate() {
        for g in faces {
            for e in g.edge_iter() {
                if loop_ids.contains(&e.id()) || seam_subst.contains_key(&e.id()) || !(e.front() == v || e.back() == v) {
                    continue;
                }
                let (fa, fb) = (e.absolute_front().clone(), e.absolute_back().clone());
                let other = if &fa == v { &fb } else { &fa };
                if !matches!(e.curve(), mt::Curve::Line(_)) {
                    return Err(unsupported("the wall seams must be straight"));
                }
                let along = (from_p3(other.point()) - from_p3(v.point())).dot(n) * side;
                if along <= r + tol {
                    return Err(unsupported("the blend is taller than the walls"));
                }
                let nb = vb.get(i).ok_or_else(|| KernelError::Failed("joint".into()))?;
                let ne = if &fa == v { builder::line(nb, &fb) } else { builder::line(&fa, nb) };
                seam_subst.insert(e.id(), ne);
            }
        }
    }
    // Rebuild faces.
    let idx_of = |id: mt::EdgeID| loop_ids.iter().position(|x| *x == id);
    let rebuild = |g: &mt::Face, rails: &[mt::Edge]| -> Result<mt::Face> {
        let mut wires = Vec::new();
        for w in g.absolute_boundaries() {
            let mut es = Vec::new();
            for e in w.edge_iter() {
                let ne = if let Some(i) = idx_of(e.id()) {
                    let rl = rails.get(i).ok_or_else(|| KernelError::Failed("rail".into()))?;
                    // Rails run in loop order (joint i → i+1).
                    if e.front() == loop_vertex.get(i).ok_or_else(|| KernelError::Failed("joint".into()))? { rl.clone() } else { rl.inverse() }
                } else if let Some(s) = seam_subst.get(&e.id()) {
                    if e.front() == e.absolute_front() { s.clone() } else { s.inverse() }
                } else {
                    e.clone()
                };
                es.push(ne);
            }
            wires.push(mt::Wire::from(es));
        }
        crate::heal::absolute_face(g, wires).ok_or_else(|| KernelError::Failed("loop blend: a face could not be rebuilt".into()))
    };
    let mut out: Vec<mt::Face> = Vec::new();
    for (gi, g) in faces.iter().enumerate() {
        if gi == fi {
            out.push(rebuild(g, &face_rails)?);
        } else if walls.contains(&gi) {
            out.push(rebuild(g, &wall_rails)?);
        } else if g.edge_iter().any(|e| seam_subst.contains_key(&e.id())) {
            out.push(rebuild(g, &[])?);
        } else {
            out.push((*g).clone());
        }
    }
    // Blend faces.
    use mt::{ParametricSurface3D, SearchNearestParameter};
    for i in 0..k {
        let (Some(s), Some(pr), Some(pr_next), Some(fr), Some(wr)) =
            (segs.get(i), profiles.get(i), profiles.get((i + 1) % k), face_rails.get(i), wall_rails.get(i))
        else {
            continue;
        };
        let swept: mt::Shell = match *s {
            Seg::Line { a, b } => vec![builder::tsweep(pr, v3(b - a))].into(),
            Seg::Arc { c, axis, angle, .. } => builder::rsweep(pr, p3(c), v3(axis), mt::Rad(angle)),
        };
        let mut surf = swept.face_iter().next().map(|x| x.oriented_surface()).ok_or_else(|| KernelError::Failed("blend surface".into()))?;
        // Outward: toward the profile centre on a concave loop, away from it on a convex one.
        let (c, mid) = (centre(i), profile_mid(i));
        let outward = (c - mid) * side;
        if let Some((u, v)) = surf.search_nearest_parameter(p3(mid), None, 100) {
            let nn = surf.normal(u, v);
            if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
                surf = mt::Invertible::inverse(&surf);
            }
        }
        let w: mt::Wire = vec![fr.inverse(), pr.clone(), wr.clone(), pr_next.inverse()].into();
        let face = mt::Face::try_new(vec![w.clone()], surf.clone()).map_err(|e| KernelError::Failed(format!("blend face: {e}")))?;
        out.push(face);
    }
    let mut shells = solid.boundaries().clone();
    if let Some(sh) = shells.get_mut(si) {
        *sh = out.into();
    }
    Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("loop blend produced an invalid solid: {e}")))
}
