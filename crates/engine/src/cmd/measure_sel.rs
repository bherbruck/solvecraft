//! Measuring picked geometry (`inspect.measure` with `items`): one item gives its length, area,
//! volume or position; two give the minimum distance between them (with the two closest
//! points) and, where both have a direction (planar faces, straight edges), the angle.

use serde_json::{Value, json};
use solvecraft_geom::{Mesh, Vec3};

use crate::params::bad;
use crate::{Result, Sel, Session};

/// Geometry of a picked item, as points or triangles.
enum Geo {
    Points(Vec<Vec3>),
    Tris(Vec<[Vec3; 3]>),
}

/// Caps that keep a measurement interactive on big meshes.
const MAX_POINTS: usize = 4000;
const MAX_TRIS: usize = 20000;

fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let n = (b - a).cross(c - a);
    if let Some(nn) = n.normalized() {
        let q = p - nn * (p - a).dot(nn);
        if [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12) {
            return q;
        }
    }
    let on_seg = |u: Vec3, v: Vec3| {
        let d = v - u;
        let l2 = d.dot(d);
        let t = if l2 > 0.0 { ((p - u).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        u + d * t
    };
    [on_seg(a, b), on_seg(b, c), on_seg(c, a)].into_iter().min_by(|x, y| x.dist(p).total_cmp(&y.dist(p))).unwrap_or(a)
}

fn polyline_len(pts: &[Vec3]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// A polyline with extra points so no gap is longer than 1/64 of it.
fn densify(pts: &[Vec3]) -> Vec<Vec3> {
    let step = (polyline_len(pts) / 64.0).max(1e-9);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let n = ((w[0].dist(w[1]) / step).ceil() as usize).clamp(1, 64);
        for k in 0..n {
            out.push(w[0].lerp(w[1], k as f64 / n as f64));
        }
    }
    out.extend(pts.last().copied());
    out
}

/// The mesh edge a selection means: by index if it still passes through the point, otherwise
/// the nearest.
fn edge_index(m: &Mesh, index: usize, point: Vec3) -> Option<usize> {
    let near = |e: &Vec<Vec3>| e.windows(2).map(|w| point.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
    let tol = m.bounds().diagonal().max(1.0) * 1e-6;
    if m.edges.get(index).is_some_and(|e| near(e) < tol) {
        return Some(index);
    }
    m.edges.iter().enumerate().min_by(|(_, a), (_, b)| near(a).total_cmp(&near(b))).map(|(i, _)| i)
}

fn face_tris(m: &Mesh, f: usize) -> Vec<[Vec3; 3]> {
    m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf as usize == f).filter_map(|(t, _)| m.tri(t)).collect()
}

/// Geometry, direction (planar face normal, straight edge direction) and a description.
fn describe(s: &Session, sel: &Sel) -> Option<(Geo, Option<Vec3>, Value)> {
    let st = s.world_state();
    Some(match sel {
        Sel::Vertex { point, .. } => (Geo::Points(vec![*point]), None, json!({"type": "vertex", "position": point})),
        Sel::Edge { body, index, point } => {
            let m = st.body(body)?.mesh();
            let index = edge_index(&m, *index, *point)?;
            let pts = m.edges.get(index)?;
            let rim = m.circular_rim(index);
            if rim.len() > 1 {
                let pieces: Vec<&Vec<Vec3>> = rim.iter().filter_map(|&i| m.edges.get(i)).collect();
                let len: f64 = pieces.iter().map(|p| polyline_len(p)).sum();
                let points = pieces.iter().flat_map(|p| densify(p)).collect();
                return Some((Geo::Points(points), None, json!({"type": "edge", "length_mm": len})));
            }
            let (a, b) = (*pts.first()?, *pts.last()?);
            let straight = (b - a).normalized().filter(|d| pts.iter().all(|p| (*p - a).cross(*d).len() < 1e-6 * (1.0 + a.dist(b))));
            let len = polyline_len(pts);
            (Geo::Points(densify(pts)), straight, json!({"type": "edge", "length_mm": len}))
        }
        Sel::Face { body, index, .. } => {
            let m = st.body(body)?.mesh();
            let tris = m.surface_patch(*index).into_iter().flat_map(|f| face_tris(&m, f)).collect::<Vec<_>>();
            let area: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).len() * 0.5).sum();
            let normals: Vec<Vec3> = tris.iter().filter_map(|[a, b, c]| (*b - *a).cross(*c - *a).normalized()).collect();
            let planar = normals.first().copied().filter(|n0| normals.iter().all(|n| n.dot(*n0) > 1.0 - 1e-6));
            (Geo::Tris(tris), planar, json!({"type": "face", "area_mm2": area, "planar": planar.is_some()}))
        }
        Sel::Body { name } => {
            let m = st.body(name)?.mesh();
            let tris: Vec<[Vec3; 3]> = m.triangles.iter().filter_map(|t| m.tri(t)).collect();
            let v = m.measure().volume;
            (Geo::Tris(tris), None, json!({"type": "body", "name": name, "volume_mm3": v}))
        }
        _ => return None,
    })
}

fn sample<T: Copy>(v: &[T], max: usize) -> Vec<T> {
    let step = v.len().div_ceil(max).max(1);
    v.iter().step_by(step).copied().collect()
}

/// Closest pair of points between two pieces of geometry.
fn closest(a: &Geo, b: &Geo) -> Option<(Vec3, Vec3)> {
    let pts_to_tris = |pts: &[Vec3], tris: &[[Vec3; 3]]| -> Option<(Vec3, Vec3)> {
        let pts = sample(pts, MAX_POINTS);
        let tris = sample(tris, MAX_TRIS);
        let mut best: Option<(f64, Vec3, Vec3)> = None;
        for p in &pts {
            for [x, y, z] in &tris {
                let q = closest_on_triangle(*p, *x, *y, *z);
                let d = p.dist(q);
                if best.is_none_or(|(bd, _, _)| d < bd) {
                    best = Some((d, *p, q));
                }
            }
        }
        best.map(|(_, p, q)| (p, q))
    };
    let verts = |tris: &[[Vec3; 3]]| -> Vec<Vec3> { tris.iter().flat_map(|t| t.iter().copied()).collect() };
    match (a, b) {
        (Geo::Points(p), Geo::Points(q)) => {
            let (p, q) = (sample(p, MAX_POINTS), sample(q, MAX_POINTS));
            let mut best: Option<(f64, Vec3, Vec3)> = None;
            for x in &p {
                for y in &q {
                    let d = x.dist(*y);
                    if best.is_none_or(|(bd, _, _)| d < bd) {
                        best = Some((d, *x, *y));
                    }
                }
            }
            best.map(|(_, x, y)| (x, y))
        }
        (Geo::Points(p), Geo::Tris(t)) => pts_to_tris(p, t),
        (Geo::Tris(t), Geo::Points(p)) => pts_to_tris(p, t).map(|(x, y)| (y, x)),
        (Geo::Tris(t1), Geo::Tris(t2)) => {
            let ab = pts_to_tris(&verts(t1), t2);
            let ba = pts_to_tris(&verts(t2), t1).map(|(x, y)| (y, x));
            [ab, ba].into_iter().flatten().min_by(|x, y| x.0.dist(x.1).total_cmp(&y.0.dist(y.1)))
        }
    }
}

/// `inspect.measure` with `items` (one or two selections).
pub(crate) fn measure_items(s: &Session, items: &Value) -> Result<Value> {
    let cmd = "inspect.measure";
    let list = items.as_array().ok_or_else(|| bad(cmd, "`items` must be a list of selections"))?;
    if list.is_empty() || list.len() > 1000 {
        return Err(bad(cmd, "measure one or two items"));
    }
    let st = s.world_state();
    let mut sels = Vec::new();
    for value in list {
        let mut sel = serde_json::from_value::<Sel>(value.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        // Two kernel halves of one visible item are one measurement target.
        match &mut sel {
            Sel::Edge { body, index, point } => {
                if let Some(b) = st.body(body) {
                    let m = b.mesh();
                    let resolved = edge_index(&m, *index, *point).unwrap_or(*index);
                    *index = m.circular_rim(resolved).first().copied().unwrap_or(resolved);
                }
            }
            Sel::Face { body, index, .. } => {
                if let Some(b) = st.body(body) {
                    *index = b.mesh().surface_patch(*index).first().copied().unwrap_or(*index);
                }
            }
            _ => {}
        }
        let duplicate = sels.iter().any(|x| match (x, &sel) {
            (Sel::Edge { body: a, index: i, .. }, Sel::Edge { body: b, index: j, .. })
            | (Sel::Face { body: a, index: i, .. }, Sel::Face { body: b, index: j, .. }) => a == b && i == j,
            _ => false,
        });
        if !duplicate {
            sels.push(sel);
        }
        if sels.len() > 2 {
            return Err(bad(cmd, "measure one or two items"));
        }
    }
    let mut described = Vec::new();
    for x in &sels {
        described.push(describe(s, x).ok_or_else(|| bad(cmd, "an item can't be measured (pick vertices, edges, faces or bodies)"))?);
    }
    let mut out = json!({"items": described.iter().map(|d| d.2.clone()).collect::<Vec<_>>()});
    if let [(ga, da, _), (gb, db, _)] = described.as_slice() {
        if let Some((p, q)) = closest(ga, gb) {
            out["distance_mm"] = json!(p.dist(q));
            out["from"] = json!(p);
            out["to"] = json!(q);
            out["delta"] = json!(q - p);
        }
        if let (Some(a), Some(b)) = (da, db) {
            let both_faces = matches!(sels.as_slice(), [Sel::Face { .. }, Sel::Face { .. }]);
            let c = a.dot(*b).clamp(-1.0, 1.0);
            let c = if both_faces { c } else { c.abs() };
            out["angle_deg"] = json!(c.acos().to_degrees());
        }
    }
    Ok(out)
}
