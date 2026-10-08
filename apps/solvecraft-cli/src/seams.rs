//! Seam-normalised topology of Fusion's measurements.
//!
//! Fusion splits a closed surface along seams: a full cylinder can be two half faces, and a
//! closed curve crossing a seam becomes two edges with a vertex on the seam. SolveCraft's merged
//! counts already count each closed surface patch once and ignore seam edges and vertices, so
//! the oracle normalises Fusion's side the same way from its face, edge and vertex lists:
//! - faces cut from one periodic surface (cylinder, cone, sphere, torus) and separated only by
//!   seam edges count once;
//! - a seam edge lies on that surface and on no other face's surface: it is dropped;
//! - a vertex left with exactly two edges is no corner (a solid's corners have three or more):
//!   the two edges join, and a curve that closes on itself keeps one vertex.

use std::collections::HashMap;

use serde_json::Value;

type V3 = [f64; 3];

const TOL: f64 = 1e-4;

fn v3(v: &Value) -> Option<V3> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
}
fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
fn unit(a: V3) -> Option<V3> {
    let l = len(a);
    (l > 1e-12).then(|| mul(a, 1.0 / l))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Surf {
    Plane { p: V3, n: V3 },
    Cylinder { o: V3, a: V3, r: f64 },
    Cone { o: V3, a: V3, r: f64, tan: f64 },
    Sphere { c: V3, r: f64 },
    Torus { c: V3, a: V3, big: f64, small: f64 },
    Other,
}

impl Surf {
    fn of(f: &Value) -> Surf {
        let g = |k: &str| v3(&f[k]);
        let n = |k: &str| f[k].as_f64();
        let s = match f["type"].as_str().unwrap_or_default() {
            "plane" => g("centroid").zip(g("plane_normal_outward").and_then(unit)).map(|(p, n)| Surf::Plane { p, n }),
            "cylinder" => (|| Some(Surf::Cylinder { o: g("axis_origin")?, a: unit(g("axis")?)?, r: n("radius_mm")? }))(),
            "cone" => {
                (|| Some(Surf::Cone { o: g("axis_origin")?, a: unit(g("axis")?)?, r: n("radius_mm")?, tan: n("half_angle_deg")?.to_radians().tan() }))(
                )
            }
            "sphere" => (|| Some(Surf::Sphere { c: g("center")?, r: n("radius_mm")? }))(),
            "torus" => (|| Some(Surf::Torus { c: g("center")?, a: unit(g("axis")?)?, big: n("major_radius_mm")?, small: n("minor_radius_mm")? }))(),
            _ => None,
        };
        s.unwrap_or(Surf::Other)
    }

    fn periodic(&self) -> bool {
        matches!(self, Surf::Cylinder { .. } | Surf::Cone { .. } | Surf::Sphere { .. } | Surf::Torus { .. })
    }

    /// Whether the point lies on the surface; `None` when that can't be told (free-form faces).
    fn contains(&self, p: V3) -> Option<bool> {
        let radial = |o: V3, a: V3| {
            let d = sub(p, o);
            let t = dot(d, a);
            (t, len(sub(d, mul(a, t))))
        };
        Some(match *self {
            Surf::Plane { p: q, n } => dot(sub(p, q), n).abs() < TOL,
            Surf::Cylinder { o, a, r } => (radial(o, a).1 - r).abs() < TOL,
            Surf::Cone { o, a, r, tan } => {
                let (t, rho) = radial(o, a);
                (rho - (r + t * tan)).abs() < TOL || (rho - (r - t * tan)).abs() < TOL
            }
            Surf::Sphere { c, r } => (len(sub(p, c)) - r).abs() < TOL,
            Surf::Torus { c, a, big, small } => {
                let (t, rho) = radial(c, a);
                ((rho - big).hypot(t) - small).abs() < TOL
            }
            Surf::Other => return None,
        })
    }

    fn same(&self, o: &Surf) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() < TOL;
        let pt = |a: V3, b: V3| len(sub(a, b)) < TOL;
        let par = |a: V3, b: V3| len(cross(a, b)) < TOL;
        // Same axis line: parallel, and either origin on the other's axis.
        let axis = |o1: V3, a1: V3, o2: V3, a2: V3| par(a1, a2) && len(cross(sub(o2, o1), a1)) < TOL;
        match (*self, *o) {
            (Surf::Cylinder { o: o1, a: a1, r: r1 }, Surf::Cylinder { o: o2, a: a2, r: r2 }) => axis(o1, a1, o2, a2) && close(r1, r2),
            (Surf::Cone { o: o1, a: a1, r: r1, tan: t1 }, Surf::Cone { .. }) => {
                // Same cone: the other's frame circle lies on this one, same opening angle.
                let Surf::Cone { o: o2, a: a2, r: r2, tan: t2 } = *o else { return false };
                axis(o1, a1, o2, a2) && close(t1, t2) && {
                    let t = dot(sub(o2, o1), a1) * dot(a1, a2).signum();
                    close(r2, r1 + t * t1) || close(r2, r1 - t * t1)
                }
            }
            (Surf::Sphere { c: c1, r: r1 }, Surf::Sphere { c: c2, r: r2 }) => pt(c1, c2) && close(r1, r2),
            (Surf::Torus { c: c1, a: a1, big: b1, small: s1 }, Surf::Torus { c: c2, a: a2, big: b2, small: s2 }) => {
                pt(c1, c2) && par(a1, a2) && close(b1, b2) && close(s1, s2)
            }
            _ => false,
        }
    }
}

/// Points along an edge from Fusion's edge list.
fn edge_points(e: &Value) -> Vec<V3> {
    let (s, t) = (v3(&e["start"]), v3(&e["end"]));
    let mut pts: Vec<V3> = s.into_iter().chain(t).collect();
    let (c, r, a) = (v3(&e["center"]), e["radius_mm"].as_f64(), v3(&e["axis"]).and_then(unit));
    match (e["type"].as_str().unwrap_or_default(), s, t) {
        ("line", Some(s), Some(t)) => pts.push(mul(add(s, t), 0.5)),
        ("arc", Some(s), Some(t)) => {
            if let (Some(c), Some(r)) = (c, r)
                && let Some(m) = unit(add(sub(s, c), sub(t, c)))
            {
                let long = e["length_mm"].as_f64().is_some_and(|l| l > std::f64::consts::PI * r);
                pts.push(add(c, mul(m, if long { -r } else { r })));
            }
        }
        ("circle", Some(s), _) => {
            if let (Some(c), Some(a)) = (c, a) {
                let d = sub(s, c);
                for th in [2.0 * std::f64::consts::FRAC_PI_3, 4.0 * std::f64::consts::FRAC_PI_3] {
                    pts.push(add(c, add(mul(d, th.cos()), mul(cross(a, d), th.sin()))));
                }
            }
        }
        _ => {}
    }
    pts
}

fn key(p: V3) -> [i64; 3] {
    p.map(|x| (x * 1e4).round() as i64)
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

/// Faces, edges and vertices of one Fusion body after merging seam splits; `None` when the
/// lists are missing.
pub fn normalised(body: &Value) -> Option<(usize, usize, usize)> {
    let faces = body["face_list"].as_array()?;
    let edges = body["edge_list"].as_array()?;
    let surfs: Vec<Surf> = faces.iter().map(Surf::of).collect();
    // Group faces cut from one periodic surface.
    let mut group: Vec<usize> = (0..surfs.len()).collect();
    for i in 0..surfs.len() {
        for j in 0..i {
            if surfs[i].periodic() && surfs[i].same(&surfs[j]) {
                group[i] = group[j];
                break;
            }
        }
    }
    let size = |g: usize| group.iter().filter(|x| **x == g).count();
    // Seam edges: on a split group's surface and on no other face's surface.
    let mut seams_in: HashMap<usize, usize> = HashMap::new();
    let mut seam = vec![false; edges.len()];
    for (ei, e) in edges.iter().enumerate() {
        let pts = edge_points(e);
        if pts.is_empty() {
            continue;
        }
        let on = |s: &Surf| pts.iter().map(|p| s.contains(*p)).try_fold(true, |acc, x| Some(acc && x?));
        let carriers: Vec<Option<bool>> = surfs.iter().map(on).collect();
        if carriers.contains(&None) {
            continue;
        }
        let groups: Vec<usize> = (0..surfs.len()).filter(|f| carriers[*f] == Some(true)).map(|f| group[f]).collect();
        if let Some(&g) = groups.first()
            && groups.iter().all(|x| *x == g)
            && surfs[g].periodic()
            && size(g) > 1
        {
            seam[ei] = true;
            *seams_in.entry(g).or_insert(0) += 1;
        }
    }
    let mut face_count = 0;
    for g in 0..surfs.len() {
        let k = size(g);
        if k > 0 {
            face_count += k.saturating_sub(seams_in.get(&g).copied().unwrap_or(0)).max(1);
        }
    }
    // Vertex degrees over the remaining edges; a closed edge counts once at its vertex.
    let mut ends: Vec<(usize, Option<[i64; 3]>, Option<[i64; 3]>)> = Vec::new();
    for (ei, e) in edges.iter().enumerate() {
        if seam[ei] {
            continue;
        }
        let (s, t) = (v3(&e["start"]).map(key), v3(&e["end"]).map(key));
        let t = if t == s { None } else { t };
        ends.push((ends.len(), s, t));
    }
    let mut deg: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, s, t) in &ends {
        for v in [s, t].into_iter().flatten() {
            deg.entry(*v).or_default().push(*i);
        }
    }
    let mut ep: Vec<usize> = (0..ends.len()).collect();
    let mut merged: Vec<[i64; 3]> = Vec::new();
    for (v, es) in &deg {
        if let [i, j] = es[..]
            && i != j
        {
            let (ri, rj) = (find(&mut ep, i), find(&mut ep, j));
            if let Some(x) = ep.get_mut(ri) {
                *x = rj;
            }
            merged.push(*v);
        }
    }
    let mut real: HashMap<usize, bool> = HashMap::new();
    for (i, s, t) in &ends {
        let r = find(&mut ep, *i);
        let has_real = [s, t].into_iter().flatten().any(|v| !merged.contains(v)) || (s.is_none() && t.is_none());
        *real.entry(r).or_insert(false) |= has_real;
    }
    let closed = real.values().filter(|r| !**r).count();
    let vertices = deg.keys().filter(|v| !merged.contains(v)).count() + closed;
    Some((face_count, real.len(), vertices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A cylinder (r 5, height 10) split by Fusion into two halves along two seams.
    #[test]
    fn split_cylinder_counts_once() {
        let cyl = json!({"type": "cylinder", "axis_origin": [0, 0, 0], "axis": [0, 0, 1], "radius_mm": 5.0});
        let plane = |z: f64, n: f64| json!({"type": "plane", "centroid": [0, 0, z], "plane_normal_outward": [0, 0, n]});
        let arc = |z: f64, s: [f64; 3], t: [f64; 3]| json!({"type": "arc", "start": s, "end": t, "center": [0, 0, z], "radius_mm": 5.0, "axis": [0, 0, 1], "length_mm": 15.707963});
        let line = |x: f64| json!({"type": "line", "start": [x, 0, 0], "end": [x, 0, 10]});
        let body = json!({
            "face_list": [cyl.clone(), cyl, plane(0.0, -1.0), plane(10.0, 1.0)],
            "edge_list": [line(5.0), line(-5.0), arc(0.0, [5.0, 0.0, 0.0], [-5.0, 0.0, 0.0]), arc(0.0, [-5.0, 0.0, 0.0], [5.0, 0.0, 0.0]), arc(10.0, [5.0, 0.0, 10.0], [-5.0, 0.0, 10.0]), arc(10.0, [-5.0, 0.0, 10.0], [5.0, 0.0, 10.0])],
        });
        assert_eq!(normalised(&body), Some((3, 2, 2)));
    }

    /// A closed curve split into two edges at a seam vertex joins again (Fusion part 61's keyway).
    #[test]
    fn degree_two_vertices_join() {
        let n = |s: [f64; 3], t: [f64; 3]| json!({"type": "nurbs", "start": s, "end": t});
        let body = json!({"face_list": [], "edge_list": [n([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), n([1.0, 0.0, 0.0], [0.0, 0.0, 0.0])]});
        assert_eq!(normalised(&body), Some((0, 1, 1)));
    }
}
