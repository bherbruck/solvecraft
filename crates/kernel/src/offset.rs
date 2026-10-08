//! Topology-preserving face offsets for bodies made of planes and cylinders (shells, offset
//! faces). Each face moves along its outward normal by its own distance: a plane is translated,
//! a cylinder changes radius about its axis. Every vertex and edge goes to where its faces'
//! moved surfaces meet; edges are lines or circular arcs. This is exact when the moved
//! surfaces meet the way the old ones did (prismatic parts, bosses, fillets between walls);
//! anything else is refused, so callers can fall back.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3};
use crate::{KernelError, Result, guard};

#[derive(Clone, Copy, Debug)]
enum Surf {
    /// Outward normal and offset: n·x = d.
    Plane { n: Vec3, d: f64 },
    /// Axis point and direction, radius; `convex` when the outward normal points away from
    /// the axis.
    Cylinder { o: Vec3, a: Vec3, r: f64, convex: bool },
}

impl Surf {
    /// Outward unit normal of the surface at `p` (on or near it).
    fn normal(&self, p: Vec3) -> Option<Vec3> {
        match *self {
            Surf::Plane { n, .. } => Some(n),
            Surf::Cylinder { o, a, convex, .. } => {
                let v = p - o;
                let radial = (v - a * v.dot(a)).normalized()?;
                Some(if convex { radial } else { -radial })
            }
        }
    }
    fn moved(&self, s: f64) -> Option<Surf> {
        match *self {
            Surf::Plane { n, d } => Some(Surf::Plane { n, d: d + s }),
            Surf::Cylinder { o, a, r, convex } => {
                let r2 = if convex { r + s } else { r - s };
                (r2 > 1e-9).then_some(Surf::Cylinder { o, a, r: r2, convex })
            }
        }
    }
    fn dist(&self, p: Vec3) -> f64 {
        match *self {
            Surf::Plane { n, d } => n.dot(p) - d,
            Surf::Cylinder { o, a, r, .. } => {
                let v = p - o;
                (v - a * v.dot(a)).len() - r
            }
        }
    }
    /// The affine map taking this surface onto `to` (same kind, same axis).
    fn map_to(&self, to: &Surf) -> Option<mt::Matrix4> {
        match (*self, *to) {
            (Surf::Plane { n, d }, Surf::Plane { d: d2, .. }) => {
                let t = n * (d2 - d);
                Some(mt::Matrix4::from_translation(mt::Vector3::new(t.x, t.y, t.z)))
            }
            (Surf::Cylinder { o, a, r, .. }, Surf::Cylinder { r: r2, .. }) => {
                // Scale by k across the axis: x ↦ o + k (x − o) + (1 − k) a a·(x − o).
                let k = r2 / r;
                let m = |i: usize, j: usize| {
                    let (ai, aj) = ([a.x, a.y, a.z][i], [a.x, a.y, a.z][j]);
                    (if i == j { k } else { 0.0 }) + (1.0 - k) * ai * aj
                };
                let lin = [[m(0, 0), m(0, 1), m(0, 2)], [m(1, 0), m(1, 1), m(1, 2)], [m(2, 0), m(2, 1), m(2, 2)]];
                let ov = [o.x, o.y, o.z];
                let t: Vec<f64> = (0..3).map(|i| ov[i] - (0..3).map(|j| lin[i][j] * ov[j]).sum::<f64>()).collect();
                // Column-major.
                Some(mt::Matrix4::new(
                    lin[0][0],
                    lin[1][0],
                    lin[2][0],
                    0.0,
                    lin[0][1],
                    lin[1][1],
                    lin[2][1],
                    0.0,
                    lin[0][2],
                    lin[1][2],
                    lin[2][2],
                    0.0,
                    *t.first()?,
                    *t.get(1)?,
                    *t.get(2)?,
                    1.0,
                ))
            }
            _ => None,
        }
    }
}

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("offset: {m}"))
}

/// Points and outward normals sampled over a face (boundary and a grid inside its
/// parameter box).
fn samples(f: &mt::Face) -> Vec<(Vec3, Vec3)> {
    use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let surf = f.oriented_surface();
    let mut uv: Vec<(f64, f64)> = Vec::new();
    for w in f.boundaries() {
        for e in w.edge_iter() {
            let c = e.oriented_curve();
            let (t0, t1) = c.range_tuple();
            for k in 0..5 {
                if let Some(x) = surf.search_nearest_parameter(c.subs(t0 + (t1 - t0) * k as f64 / 5.0), uv.last().copied(), 50) {
                    uv.push(x);
                }
            }
        }
    }
    let Some(&(u0, v0)) = uv.first() else { return Vec::new() };
    let (mut lo, mut hi) = ((u0, v0), (u0, v0));
    for (u, v) in &uv {
        lo = (lo.0.min(*u), lo.1.min(*v));
        hi = (hi.0.max(*u), hi.1.max(*v));
    }
    for i in 0..=4 {
        for j in 0..=4 {
            uv.push((lo.0 + (hi.0 - lo.0) * i as f64 / 4.0, lo.1 + (hi.1 - lo.1) * j as f64 / 4.0));
        }
    }
    uv.iter()
        .filter_map(|(u, v)| {
            let n = surf.normal(*u, *v);
            Some((from_p3(surf.subs(*u, *v)), Vec3::new(n.x, n.y, n.z).normalized()?))
        })
        .collect()
}

/// What surface a face lies on (planes and cylinders only).
fn surf_of(f: &mt::Face, tol: f64) -> Option<Surf> {
    if let mt::Surface::Plane(pl) = f.oriented_surface() {
        let n = pl.normal();
        let n = Vec3::new(n.x, n.y, n.z).normalized()?;
        return Some(Surf::Plane { n, d: n.dot(from_p3(pl.origin())) });
    }
    let s = samples(f);
    let (p0, n0) = *s.first()?;
    let far = s.iter().map(|x| x.1).min_by(|a, b| a.dot(n0).total_cmp(&b.dot(n0)))?;
    let a = n0.cross(far).normalized()?;
    if s.iter().any(|(_, n)| n.dot(a).abs() > 1e-6) {
        return None;
    }
    // The axis: where the normal lines through two samples meet (seen along a).
    let (p1, n1) = *s.iter().max_by(|x, y| (x.1 - n0).len().total_cmp(&(y.1 - n0).len()))?;
    // p0 + t0 n0 = p1 + t1 n1 (projected): solve in the plane ⟂ a.
    let w = p1 - p0;
    let c = n0.dot(n1);
    let den = 1.0 - c * c;
    if den.abs() < 1e-9 {
        return None;
    }
    let t0 = (w.dot(n0) - c * w.dot(n1)) / den;
    let o = p0 + n0 * t0;
    let o = o - a * (o - p0).dot(a);
    let r = t0.abs();
    // Outward normal away from the axis: p = o + r n (t0 < 0 means o = p − r n).
    let convex = t0 < 0.0;
    let cyl = Surf::Cylinder { o, a, r, convex };
    let ok = s.iter().all(|(p, n)| cyl.dist(*p).abs() < tol && cyl.normal(*p).is_some_and(|m| m.dot(*n) > 1.0 - 1e-6));
    ok.then_some(cyl)
}

/// Move `p` (on all of `old`) onto the moved surfaces `new`: the smallest displacement δ with
/// n_i · δ = s_i for each face's normal n_i at p, then checked against the moved surfaces.
fn move_point(p: Vec3, old: &[Surf], shifts: &[f64], new: &[Surf], tol: f64) -> Option<Vec3> {
    let ns: Vec<Vec3> = old.iter().map(|sf| sf.normal(p)).collect::<Option<_>>()?;
    // Orthonormal basis of the normals' span (tangent faces share a normal).
    let mut basis: Vec<Vec3> = Vec::new();
    for n in &ns {
        let mut v = *n;
        for e in &basis {
            v = v - *e * v.dot(*e);
        }
        if let Some(u) = v.normalized().filter(|_| v.len() > 1e-6) {
            basis.push(u);
        }
    }
    // Least squares in the span: rows n_i · e_k, right-hand sides s_i.
    let r = basis.len();
    let mut m = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for (n, s) in ns.iter().zip(shifts) {
        let row: Vec<f64> = basis.iter().map(|e| n.dot(*e)).collect();
        for i in 0..r {
            for j in 0..r {
                m[i][j] += row[i] * row[j];
            }
            rhs[i] += row[i] * s;
        }
    }
    // Pad unused dimensions with the identity.
    for (i, row) in m.iter_mut().enumerate().skip(r) {
        row[i] = 1.0;
    }
    let c = crate::polyhedron::solve3_pub(m, rhs)?;
    let mut q = p;
    for (k, e) in basis.iter().enumerate() {
        q += *e * c[k];
    }
    new.iter().all(|sf| sf.dist(q).abs() < tol).then_some(q)
}

/// A line or circular arc through an edge (by samples): `None` for other curves.
fn arc_mid(e: &mt::Edge, tol: f64) -> Option<Option<Vec3>> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    let pts: Vec<Vec3> = (0..=16).map(|k| from_p3(c.subs(t0 + (t1 - t0) * k as f64 / 16.0))).collect();
    let (a, b, m) = (*pts.first()?, *pts.last()?, *pts.get(8)?);
    if a.dist(b) < tol {
        return None;
    }
    if pts.iter().all(|p| p.dist_to_segment(a, b) < tol) {
        return Some(None);
    }
    let (u, v) = (b - a, m - a);
    let w = u.cross(v);
    let d = 2.0 * w.len2();
    if d < 1e-300 {
        return None;
    }
    let centre = a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / d);
    let r = centre.dist(a);
    pts.iter().all(|p| (p.dist(centre) - r).abs() < tol && (*p - centre).dot(w.normalized().unwrap_or(Vec3::Z)).abs() < tol).then_some(Some(m))
}

/// The body with each face moved along its outward normal by `shift(face index, normal at a
/// face point)`, keeping the topology. Faces must be planes or cylinders.
pub(crate) fn offset_body(b: &Body, shift: impl Fn(usize, Vec3) -> f64) -> Result<Body> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    let size = b.size();
    let tol = (size * 1e-7).max(1e-9);
    let mut old = Vec::with_capacity(faces.len());
    let mut new = Vec::with_capacity(faces.len());
    let mut shifts = Vec::with_capacity(faces.len());
    for (i, f) in faces.iter().enumerate() {
        let sf = surf_of(f, tol * 100.0).ok_or_else(|| fail("only bodies with planar and cylindrical faces"))?;
        let n = match sf {
            Surf::Plane { n, .. } => n,
            Surf::Cylinder { a, .. } => a.any_perp(),
        };
        let s = shift(i, n);
        old.push(sf);
        new.push(sf.moved(s).ok_or_else(|| fail("a cylinder shrinks to nothing"))?);
        shifts.push(s);
    }
    let pick = |ids: &[usize]| -> (Vec<Surf>, Vec<f64>, Vec<Surf>) {
        (
            ids.iter().filter_map(|i| old.get(*i).copied()).collect(),
            ids.iter().filter_map(|i| shifts.get(*i).copied()).collect(),
            ids.iter().filter_map(|i| new.get(*i).copied()).collect(),
        )
    };
    // Vertex → its faces; edge → its faces.
    let mut vfaces: HashMap<mt::VertexID, Vec<usize>> = HashMap::new();
    let mut efaces: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for v in f.vertex_iter() {
            let e = vfaces.entry(v.id()).or_default();
            if !e.contains(&i) {
                e.push(i);
            }
        }
        for ed in f.edge_iter() {
            let e = efaces.entry(ed.id()).or_default();
            if !e.contains(&i) {
                e.push(i);
            }
        }
    }
    let mut newpos: HashMap<mt::VertexID, Vec3> = HashMap::new();
    for v in b.solid.vertex_iter() {
        let fs = vfaces.get(&v.id()).ok_or_else(|| fail("vertex"))?;
        let (o, s, n) = pick(fs);
        let q = move_point(from_p3(v.point()), &o, &s, &n, size * 1e-6)
            .ok_or_else(|| fail("a corner whose faces don't meet the same way after the move"))?;
        newpos.insert(v.id(), q);
    }
    guard("offset", || {
        let verts: HashMap<mt::VertexID, mt::Vertex> = newpos.iter().map(|(k, p)| (*k, builder::vertex(p3(*p)))).collect();
        let mut edges: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for e in b.solid.edge_iter() {
            if edges.contains_key(&e.id()) {
                continue;
            }
            let (Some(a), Some(c)) = (verts.get(&e.absolute_front().id()), verts.get(&e.absolute_back().id())) else { return Err(fail("edge")) };
            if from_p3(a.point()).dist(from_p3(c.point())) < size * 1e-9 {
                return Err(fail("an edge vanishes (the offset is too large)"));
            }
            let kind = arc_mid(&e, size * 1e-6).ok_or_else(|| fail("edges must be lines or arcs"))?;
            let ne = match kind {
                None => builder::line(a, c),
                Some(m) => {
                    let fs = efaces.get(&e.id()).ok_or_else(|| fail("edge faces"))?;
                    let (o, s, n) = pick(fs);
                    let m2 =
                        move_point(m, &o, &s, &n, size * 1e-6).ok_or_else(|| fail("an edge whose faces don't meet the same way after the move"))?;
                    builder::circle_arc(a, c, p3(m2))
                }
            };
            edges.insert(e.id(), ne);
        }
        let mut out = Vec::new();
        for (i, f) in faces.iter().enumerate() {
            let wires: Vec<mt::Wire> = f
                .absolute_boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .filter_map(|e| edges.get(&e.id()).map(|ne| if e.front() == e.absolute_front() { ne.clone() } else { ne.inverse() }))
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let (Some(o), Some(n)) = (old.get(i), new.get(i)) else { return Err(fail("face")) };
            let mat = o.map_to(n).ok_or_else(|| fail("surface"))?;
            let surface = mt::Transformed::transformed(&f.surface(), mat);
            let mut nf = mt::Face::try_new(wires, surface).map_err(|e| fail(&e.to_string()))?;
            if !f.orientation() {
                nf.invert();
            }
            out.push(nf);
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| fail(&e.to_string()))?;
        Body::new(solid)
    })
}
