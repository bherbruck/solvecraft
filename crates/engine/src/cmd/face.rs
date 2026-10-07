//! Sketches made from a planar body face, so a face can be extruded like a profile (Extrude
//! with `face`, Press Pull). The face boundary is copied into a new sketch on the face: straight
//! edges become lines, circular edges circles or arcs, anything else a chain of lines.

use serde_json::{Value, json};
use solvecraft_geom::{Mesh, Vec2, Vec3};

use crate::params::bad;
use crate::{Result, Session};

fn point_triangle_dist(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

/// The outward normal of the planar body face at `p`.
pub(super) fn face_normal(s: &Session, p: Vec3) -> Option<Vec3> {
    let (m, f, d) = face_at(s, p)?;
    let tol = (m.bounds().diagonal() * 1e-3).max(1e-3);
    if d > tol * 10.0 {
        return None;
    }
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf == f)
        .filter_map(|(t, _)| m.tri(t))
        .find_map(|[a, b, c]| (b - a).cross(c - a).normalized())
}

/// The body face under a point: (mesh, face index, distance).
fn face_at(s: &Session, p: Vec3) -> Option<(std::sync::Arc<Mesh>, u32, f64)> {
    let st = s.model.state();
    let mut best: Option<(std::sync::Arc<Mesh>, u32, f64)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        let mut near: Option<(u32, f64)> = None;
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = point_triangle_dist(p, a, bb, c);
            if near.is_none_or(|(_, nd)| d < nd) {
                near = Some((*f, d));
            }
        }
        if let Some((f, d)) = near
            && best.as_ref().is_none_or(|x| d < x.2)
        {
            best = Some((m.clone(), f, d));
        }
    }
    best
}

/// Circle through three points.
fn circumcircle(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let o = Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
    let r = o.dist(a);
    (o.is_finite() && r.is_finite() && r < 1e8).then_some((o, r))
}

fn xy(v: Vec2) -> Value {
    json!([v.x, v.y])
}

/// Start a sketch on the planar face at `point` holding a copy of the face boundary. Returns
/// the sketch id and a point inside the face (sketch coordinates). The sketch is left active.
pub(super) fn sketch_of_face(s: &mut Session, point: Vec3, cmd: &str) -> Result<(u64, Vec2)> {
    let (m, f, d) = face_at(s, point).ok_or_else(|| bad(cmd, "no body face at `face`"))?;
    let tol = (m.bounds().diagonal() * 1e-3).max(1e-3);
    if d > tol * 10.0 {
        return Err(bad(cmd, "no body face at `face`"));
    }
    let tris: Vec<[Vec3; 3]> = m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf == f).filter_map(|(t, _)| m.tri(t)).collect();
    let normal = tris.iter().find_map(|[a, b, c]| (*b - *a).cross(*c - *a).normalized()).ok_or_else(|| bad(cmd, "degenerate face"))?;
    if tris.iter().any(|[a, b, c]| (*b - *a).cross(*c - *a).normalized().is_some_and(|n| n.dot(normal) < 1.0 - 1e-6)) {
        return Err(bad(cmd, "only planar faces can be extruded"));
    }
    // A point well inside the face: the centroid of its largest triangle.
    let inside = tris
        .iter()
        .max_by(|x, y| (x[1] - x[0]).cross(x[2] - x[0]).len().total_cmp(&(y[1] - y[0]).cross(y[2] - y[0]).len()))
        .map(|[a, b, c]| (*a + *b + *c) / 3.0)
        .unwrap_or(point);
    let edges: Vec<Vec<Vec3>> =
        m.face_edges(f).into_iter().filter(|e| !m.seams.get(*e).copied().unwrap_or(false)).filter_map(|e| m.edges.get(e).cloned()).collect();
    if edges.is_empty() || edges.len() > 5000 {
        return Err(bad(cmd, "the face has no usable boundary"));
    }
    let run = |s: &mut Session, id: &str, p: Value| -> Result<Value> {
        let spec = crate::find_command(id).ok_or_else(|| bad(cmd, id))?;
        (spec.run)(s, &p)
    };
    let r = run(s, "SketchCreate", json!({"plane": {"face": [inside.x, inside.y, inside.z]}}))?;
    let sid = r.get("sketch").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "could not start a sketch on the face"))?;
    s.refresh();
    let plane = s.model.state().sketch(sid).map(|ss| ss.plane).ok_or_else(|| bad(cmd, "could not start a sketch on the face"))?;
    for e in &edges {
        let pts: Vec<Vec2> = e.iter().map(|p| plane.to_local(*p)).collect();
        let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { continue };
        let closed = pts.len() > 3 && first.dist(last) < tol;
        let circle = if pts.len() >= 4 {
            let n = pts.len() - usize::from(closed);
            let (a, b, c) = (pts.first(), pts.get(n / 3), pts.get(2 * n / 3));
            match (a, b, c) {
                (Some(a), Some(b), Some(c)) => circumcircle(*a, *b, *c).filter(|(o, r)| pts.iter().all(|q| (q.dist(*o) - r).abs() < tol)),
                _ => None,
            }
        } else {
            None
        };
        match circle {
            Some((o, r)) if closed => run(s, "CircleCenterRadius", json!({"center": xy(o), "radius": r}))?,
            Some(_) => {
                let mid = pts.get(pts.len() / 2).copied().unwrap_or(first);
                run(s, "ArcThreePoint", json!({"start": xy(first), "end": xy(last), "through": xy(mid)}))?
            }
            None => {
                let mut list: Vec<Value> = Vec::new();
                for q in &pts {
                    // Drop points that repeat (a closed polyline ends where it starts).
                    if list.last().is_none_or(|l| l != &xy(*q)) {
                        list.push(xy(*q));
                    }
                }
                let closed_poly = list.len() > 2 && list.first() == list.last();
                if closed_poly {
                    list.pop();
                }
                run(s, "DrawPolyline", json!({"points": list, "closed": closed_poly}))?
            }
        };
    }
    Ok((sid, plane.to_local(inside)))
}
