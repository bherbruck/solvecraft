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
pub(crate) enum Surf {
    /// Outward normal and offset: n·x = d.
    Plane { n: Vec3, d: f64 },
    /// Axis point and direction, radius; `convex` when the outward normal points away from
    /// the axis.
    Cylinder { o: Vec3, a: Vec3, r: f64, convex: bool },
    /// Centre and radius; `convex` when the outward normal points away from the centre.
    Sphere { c: Vec3, r: f64, convex: bool },
    /// Apex, unit axis (into the cone, away from the apex) and half-angle; `convex` when the
    /// outward normal points away from the axis.
    Cone { v: Vec3, a: Vec3, half: f64, convex: bool },
    /// Axis point and unit axis, the tube's centre circle (radius `big` at height `h` along the
    /// axis) and the tube radius; `convex` when the outward normal points away from the tube's
    /// centre.
    Torus { o: Vec3, a: Vec3, big: f64, h: f64, r: f64, convex: bool },
}

/// The centre of a torus tube nearest `p`.
fn tube_centre(o: Vec3, a: Vec3, big: f64, h: f64, p: Vec3) -> Option<Vec3> {
    let w = p - o;
    let u = (w - a * w.dot(a)).normalized()?;
    Some(o + a * h + u * big)
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
            Surf::Sphere { c, convex, .. } => {
                let radial = (p - c).normalized()?;
                Some(if convex { radial } else { -radial })
            }
            Surf::Cone { v, a, half, convex } => {
                let w = p - v;
                let q = (w - a * w.dot(a)).normalized()?;
                let n = q * half.cos() - a * half.sin();
                Some(if convex { n } else { -n })
            }
            Surf::Torus { o, a, big, h, convex, .. } => {
                let n = (p - tube_centre(o, a, big, h, p)?).normalized()?;
                Some(if convex { n } else { -n })
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
            Surf::Sphere { c, r, convex } => {
                let r2 = if convex { r + s } else { r - s };
                (r2 > 1e-9).then_some(Surf::Sphere { c, r: r2, convex })
            }
            // Moving a cone along its normal moves its apex along the axis.
            Surf::Cone { v, a, half, convex } => {
                let s = if convex { s } else { -s };
                let sh = half.sin();
                (sh > 1e-9).then_some(Surf::Cone { v: v - a * (s / sh), a, half, convex })
            }
            Surf::Torus { o, a, big, h, r, convex } => {
                let r2 = if convex { r + s } else { r - s };
                (r2 > 1e-9).then_some(Surf::Torus { o, a, big, h, r: r2, convex })
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
            Surf::Sphere { c, r, .. } => p.dist(c) - r,
            Surf::Cone { v, a, half, .. } => {
                let w = p - v;
                let h = w.dot(a);
                (w - a * h).len() * half.cos() - h * half.sin()
            }
            Surf::Torus { o, a, big, h, r, .. } => tube_centre(o, a, big, h, p).map(|c| p.dist(c) - r).unwrap_or(f64::INFINITY),
        }
    }
    /// The affine map taking this surface onto `to` (same kind, same axis).
    fn map_to(&self, to: &Surf) -> Option<mt::Matrix4> {
        match (*self, *to) {
            (Surf::Sphere { c, r, .. }, Surf::Sphere { r: r2, .. }) => {
                let k = r2 / r;
                let t = c * (1.0 - k);
                Some(mt::Matrix4::from_translation(mt::Vector3::new(t.x, t.y, t.z)) * mt::Matrix4::from_scale(k))
            }
            (Surf::Cone { v, .. }, Surf::Cone { v: v2, .. }) => {
                let t = v2 - v;
                Some(mt::Matrix4::from_translation(mt::Vector3::new(t.x, t.y, t.z)))
            }
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
                // The previous point's parameters as a hint, unless that lands elsewhere (a
                // singular point such as a pole holds the search).
                let q = c.subs(t0 + (t1 - t0) * k as f64 / 5.0);
                let near = |x: (f64, f64)| from_p3(surf.subs(x.0, x.1)).dist(from_p3(q)) < 1e-6 * (1.0 + from_p3(q).len());
                let found = surf
                    .search_nearest_parameter(q, uv.last().copied(), 50)
                    .filter(|x| near(*x))
                    .or_else(|| surf.search_nearest_parameter(q, None, 100).filter(|x| near(*x)));
                if let Some(x) = found {
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
    // Parameter steps for checking that the normal is steady (not at a cone's apex).
    let (du, dv) = (((hi.0 - lo.0) * 1e-6).max(1e-9), ((hi.1 - lo.1) * 1e-6).max(1e-9));
    let nrm = |u: f64, v: f64| {
        let n = surf.normal(u, v);
        Vec3::new(n.x, n.y, n.z).normalized()
    };
    uv.iter()
        .filter_map(|(u, v)| {
            let n = nrm(*u, *v)?;
            let steady =
                [nrm(*u + du, *v), nrm(*u - du, *v), nrm(*u, *v + dv), nrm(*u, *v - dv)].iter().all(|m| m.is_some_and(|m| m.dot(n) > 1.0 - 1e-6));
            steady.then(|| (from_p3(surf.subs(*u, *v)), n))
        })
        .collect()
}

/// What surface a face lies on (planes and cylinders only).
pub(crate) fn surf_of(f: &mt::Face, tol: f64) -> Option<Surf> {
    if let mt::Surface::Plane(pl) = f.oriented_surface() {
        let n = pl.normal();
        let n = Vec3::new(n.x, n.y, n.z).normalized()?;
        return Some(Surf::Plane { n, d: n.dot(from_p3(pl.origin())) });
    }
    let s = samples(f);
    if let Some(t) = torus_of(f, &s, tol) {
        return Some(t);
    }
    let (p0, n0) = *s.first()?;
    // The normal most square to the first (a half cylinder's two ends are opposite).
    let far = s.iter().map(|x| x.1).min_by(|a, b| a.dot(n0).abs().total_cmp(&b.dot(n0).abs()))?;
    let Some(a) = n0.cross(far).normalized() else { return sphere_or_cone(&s, tol) };
    if s.iter().any(|(_, n)| n.dot(a).abs() > 1e-6) {
        return sphere_or_cone(&s, tol);
    }
    // The axis: where the normal lines through two samples meet (seen along a).
    let (p1, n1) = *s.iter().min_by(|x, y| x.1.dot(n0).abs().total_cmp(&y.1.dot(n0).abs()))?;
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

/// A torus: a surface of revolution whose normals (in the meridian plane) all point from one
/// circle, the tube's centre, at one distance.
fn torus_of(f: &mt::Face, s: &[(Vec3, Vec3)], tol: f64) -> Option<Surf> {
    let mt::Surface::RevolutedCurve(rc) = f.surface() else { return None };
    if *rc.transform() != <mt::Matrix4 as mt::One>::one() {
        return None;
    }
    let (o, ax) = (from_p3(rc.entity().origin()), rc.entity().axis());
    let a = Vec3::new(ax.x, ax.y, ax.z).normalized()?;
    // Meridian coordinates: ρ − r n_ρ = R and h − r n_h = h0 for every sample, least squares
    // in (R, h0, r).
    let mut m = [[0.0f64; 3]; 3];
    let mut rhs = [0.0f64; 3];
    for (p, n) in s {
        let w = *p - o;
        let hh = w.dot(a);
        let radial = w - a * hh;
        let u = radial.normalized()?;
        for (row, val) in [([1.0, 0.0, n.dot(u)], radial.len()), ([0.0, 1.0, n.dot(a)], hh)] {
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += row[i] * row[j];
                }
                rhs[i] += row[i] * val;
            }
        }
    }
    let x = crate::polyhedron::solve3_pub(m, rhs)?;
    let (big, h, r) = (x[0], x[1], x[2]);
    if !(big > 1e-9 && r.abs() > 1e-9 && r.abs() < big) {
        return None;
    }
    let t = Surf::Torus { o, a, big, h, r: r.abs(), convex: r > 0.0 };
    s.iter().all(|(p, n)| t.dist(*p).abs() < tol && t.normal(*p).is_some_and(|m| m.dot(*n) > 1.0 - 1e-6)).then_some(t)
}

/// A sphere (normal lines all through one point) or a cone (tangent planes all through one
/// point, normals at one angle to an axis) through the samples.
fn sphere_or_cone(s: &[(Vec3, Vec3)], tol: f64) -> Option<Surf> {
    // Least squares point nearest all the normal lines: Σ (I − n nᵀ) x = Σ (I − n nᵀ) p.
    let mut m = [[0.0f64; 3]; 3];
    let mut r = [0.0f64; 3];
    for (p, n) in s {
        let nv = [n.x, n.y, n.z];
        let pv = [p.x, p.y, p.z];
        for i in 0..3 {
            for j in 0..3 {
                let a = (if i == j { 1.0 } else { 0.0 }) - nv[i] * nv[j];
                m[i][j] += a;
                r[i] += a * pv[j];
            }
        }
    }
    if let Some(x) = crate::polyhedron::solve3_pub(m, r) {
        let c = Vec3::new(x[0], x[1], x[2]);
        let (p0, n0) = *s.first()?;
        let rad = p0.dist(c);
        let convex = (p0 - c).dot(n0) > 0.0;
        let sp = Surf::Sphere { c, r: rad, convex };
        if rad > tol && s.iter().all(|(p, n)| sp.dist(*p).abs() < tol && sp.normal(*p).is_some_and(|q| q.dot(*n) > 1.0 - 1e-6)) {
            return Some(sp);
        }
    }
    // Cone: the apex lies on every tangent plane, n·(v − p) = 0.
    let mut m = [[0.0f64; 3]; 3];
    let mut r = [0.0f64; 3];
    for (p, n) in s {
        let nv = [n.x, n.y, n.z];
        let k = n.dot(*p);
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += nv[i] * nv[j];
            }
            r[i] += nv[i] * k;
        }
    }
    let x = crate::polyhedron::solve3_pub(m, r)?;
    let v = Vec3::new(x[0], x[1], x[2]);
    // Axis: from the apex toward the samples' centroid (the cone opens that way).
    let cen = s.iter().fold(Vec3::ZERO, |acc, (p, _)| acc + *p) * (1.0 / s.len() as f64);
    // Normals make a constant angle with the axis: fit it from three spread normals.
    let n0 = s.first()?.1;
    let n1 = s.iter().map(|x| x.1).max_by(|a, b| (*a - n0).len().total_cmp(&(*b - n0).len()))?;
    let n2 = s.iter().map(|x| x.1).max_by(|a, b| (*a - n0).cross(n1 - n0).len().total_cmp(&(*b - n0).cross(n1 - n0).len()))?;
    let mut a = (n1 - n0).cross(n2 - n0).normalized()?;
    if a.dot(cen - v) < 0.0 {
        a = -a;
    }
    let half = n0.dot(a).abs().clamp(0.0, 1.0).asin();
    if !(half > 1e-3 && half < std::f64::consts::FRAC_PI_2 - 1e-3) {
        return None;
    }
    let (p0, n0) = *s.first()?;
    let q = {
        let w = p0 - v;
        (w - a * w.dot(a)).normalized()?
    };
    let convex = n0.dot(q) > 0.0;
    let cone = Surf::Cone { v, a, half, convex };
    s.iter().all(|(p, n)| cone.dist(*p).abs() < tol && cone.normal(*p).is_some_and(|m| m.dot(*n) > 1.0 - 1e-6)).then_some(cone)
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
        let sf = surf_of(f, tol * 100.0).ok_or_else(|| fail("only bodies with planes, cylinders, cones, spheres and tori"))?;
        let n = match sf {
            Surf::Plane { n, .. } => n,
            Surf::Cylinder { a, .. } | Surf::Cone { a, .. } | Surf::Torus { a, .. } => a.any_perp(),
            Surf::Sphere { .. } => Vec3::Z,
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
        let p = from_p3(v.point());
        // A cone's apex goes with the cone (its normal is not defined there).
        let apex = o.iter().zip(&n).find_map(|(a, b)| match (a, b) {
            (Surf::Cone { v: va, .. }, Surf::Cone { v: vb, .. }) if va.dist(p) < size * 1e-6 => Some(*vb),
            _ => None,
        });
        let q = match apex {
            Some(q) if n.iter().all(|sf| sf.dist(q).abs() < size * 1e-6) => q,
            _ => move_point(p, &o, &s, &n, size * 1e-6).ok_or_else(|| fail("a corner whose faces don't meet the same way after the move"))?,
        };
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
            let surface = match (o.map_to(n), *o, *n) {
                (Some(mat), _, _) => mt::Transformed::transformed(&f.surface(), mat),
                // A torus: its tube's circle scaled about the tube's centre, revolved again.
                (None, Surf::Torus { o: to, a, big, h, r, .. }, Surf::Torus { r: r2, .. }) => {
                    use mt::{BoundedCurve, ParametricCurve};
                    let mt::Surface::RevolutedCurve(rc) = f.surface() else { return Err(fail("torus surface")) };
                    let curve = rc.entity().entity_curve().clone();
                    let (t0, t1) = curve.range_tuple();
                    let q = from_p3(curve.subs((t0 + t1) / 2.0));
                    let c = tube_centre(to, a, big, h, q).ok_or_else(|| fail("torus"))?;
                    let k = r2 / r;
                    let t = c * (1.0 - k);
                    let mat = mt::Matrix4::from_translation(mt::Vector3::new(t.x, t.y, t.z)) * mt::Matrix4::from_scale(k);
                    let moved = mt::Transformed::transformed(&curve, mat);
                    let mut p = mt::Processor::new(mt::RevolutedCurve::by_revolution(moved, rc.entity().origin(), rc.entity().axis()));
                    if !rc.orientation() {
                        mt::Invertible::invert(&mut p);
                    }
                    mt::Surface::RevolutedCurve(p)
                }
                _ => return Err(fail("surface")),
            };
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

/// Draft walls parallel to `pull` (planes, and cylinders about axes along `pull`) by `angle`
/// about their line on the neutral plane: at height h along `pull` a wall moves along its
/// outward normal by −h·tan(angle), so planes tilt and cylinders become cones. The other
/// faces must be caps square to `pull`; the topology is kept.
pub(crate) fn draft_walls(b: &Body, chosen: &[usize], neutral: &solvecraft_geom::Plane, pull: Vec3, angle: f64) -> Result<Body> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    let size = b.size();
    let tol = (size * 1e-7).max(1e-9);
    let nn = neutral.normal();
    let den = pull.dot(nn);
    if den.abs() < 1e-9 {
        return Err(fail("the neutral plane is parallel to the pull direction"));
    }
    let height = |p: Vec3| (p - neutral.origin).dot(nn) / den;
    let tan = angle.tan();
    let mut old = Vec::with_capacity(faces.len());
    for (i, f) in faces.iter().enumerate() {
        let sf = surf_of(f, tol * 100.0).ok_or_else(|| fail("draft: only bodies with planar and cylindrical faces"))?;
        let wall = match sf {
            Surf::Plane { n, .. } => n.dot(pull).abs() < 1e-9,
            Surf::Cylinder { a, .. } => a.dot(pull).abs() > 1.0 - 1e-9,
            Surf::Sphere { .. } | Surf::Cone { .. } | Surf::Torus { .. } => false,
        };
        let cap = matches!(sf, Surf::Plane { n, .. } if n.dot(pull).abs() > 1.0 - 1e-9);
        if chosen.contains(&i) && !wall {
            return Err(fail("draft: a chosen face isn't parallel to the pull direction"));
        }
        if !chosen.contains(&i) && !cap && !wall {
            return Err(fail("draft: faces next to the drafted walls must be square to the pull direction"));
        }
        old.push(sf);
    }
    // Shifts and moved surfaces at a height.
    let at = |ids: &[usize], h: f64| -> Option<(Vec<Surf>, Vec<f64>, Vec<Surf>)> {
        let mut o = Vec::new();
        let mut s = Vec::new();
        let mut n = Vec::new();
        for i in ids {
            let sf = *old.get(*i)?;
            let sh = if chosen.contains(i) { -h * tan } else { 0.0 };
            o.push(sf);
            s.push(sh);
            n.push(sf.moved(sh)?);
        }
        Some((o, s, n))
    };
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
        let p = from_p3(v.point());
        let fs = vfaces.get(&v.id()).ok_or_else(|| fail("vertex"))?;
        let (o, s, n) = at(fs, height(p)).ok_or_else(|| fail("draft: a wall shrinks to nothing"))?;
        let q = move_point(p, &o, &s, &n, size * 1e-6).ok_or_else(|| fail("draft: a corner whose faces don't meet the same way after the draft"))?;
        newpos.insert(v.id(), q);
    }
    // New surfaces for the drafted walls: planes through their hinge line, cylinders → cones.
    let mut surfaces: Vec<Option<mt::Surface>> = vec![None; faces.len()];
    for (i, sf) in old.iter().enumerate() {
        if !chosen.contains(&i) {
            continue;
        }
        let surface = match *sf {
            Surf::Plane { n, d } => {
                let n2 = (n * angle.cos() + pull * angle.sin()).normalized().ok_or_else(|| fail("draft"))?;
                // A point of the old plane on the neutral plane: the hinge.
                let o = neutral.intersect_ray(n * d, pull).ok_or_else(|| fail("draft: hinge"))?;
                let u = n2.cross(pull).normalized().ok_or_else(|| fail("draft"))?;
                let w = n2.cross(u);
                mt::Surface::Plane(mt::Plane::new(p3(o), p3(o + u), p3(o + w)))
            }
            Surf::Cylinder { o, a, r, convex } => {
                // Generator: the wall at two heights along one radial direction.
                let u = a.any_perp();
                // Over the face's heights (and a little beyond), short of the apex.
                let hs: Vec<f64> = faces.get(i).map(|f| f.vertex_iter().map(|v| height(from_p3(v.point()))).collect()).unwrap_or_default();
                let (lo, hi) = hs.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |m, h| (m.0.min(*h), m.1.max(*h)));
                if !(lo.is_finite() && hi.is_finite()) {
                    return Err(fail("draft: a wall without corners"));
                }
                let pad = (hi - lo).max(size * 1e-3) * 0.25;
                let (mut h0, mut h1) = (lo - pad, hi + pad);
                // Keep the radius positive along the generator.
                let r_at = |h: f64| if convex { r - h * tan } else { r + h * tan };
                for h in [&mut h0, &mut h1] {
                    if r_at(*h) <= r * 1e-3 {
                        *h = if (*h - lo).abs() < (*h - hi).abs() { lo } else { hi };
                    }
                }
                let base = |h: f64| {
                    let rr = if convex { r - h * tan } else { r + h * tan };
                    o + pull * (h - height(o)) + u * rr
                };
                let (pa, pb) = (base(h0), base(h1));
                let line = mt::Curve::Line(mt::Line(p3(pa), p3(pb)));
                let rc = mt::RevolutedCurve::by_revolution(line, p3(o), mt::Vector3::new(a.x, a.y, a.z));
                mt::Surface::RevolutedCurve(mt::Processor::new(rc))
            }
            Surf::Sphere { .. } | Surf::Cone { .. } | Surf::Torus { .. } => {
                return Err(fail("draft: only planes and cylinders along the pull can be drafted"));
            }
        };
        if let Some(slot) = surfaces.get_mut(i) {
            *slot = Some(surface);
        }
    }
    guard("draft", || {
        let verts: HashMap<mt::VertexID, mt::Vertex> = newpos.iter().map(|(k, p)| (*k, builder::vertex(p3(*p)))).collect();
        let mut edges: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for e in b.solid.edge_iter() {
            if edges.contains_key(&e.id()) {
                continue;
            }
            let (Some(a), Some(c)) = (verts.get(&e.absolute_front().id()), verts.get(&e.absolute_back().id())) else { return Err(fail("edge")) };
            let kind = arc_mid(&e, size * 1e-6).ok_or_else(|| fail("draft: edges must be lines or arcs"))?;
            let ne = match kind {
                None => builder::line(a, c),
                Some(m) => {
                    let fs = efaces.get(&e.id()).ok_or_else(|| fail("edge faces"))?;
                    let (o, s, n) = at(fs, height(m)).ok_or_else(|| fail("draft"))?;
                    let m2 = move_point(m, &o, &s, &n, size * 1e-6).ok_or_else(|| fail("draft: an arc that can't follow its faces"))?;
                    builder::circle_arc(a, c, p3(m2))
                }
            };
            edges.insert(e.id(), ne);
        }
        let mut out = Vec::new();
        for (i, f) in faces.iter().enumerate() {
            let wires: Vec<mt::Wire> = f
                .boundaries()
                .iter()
                .map(|w| {
                    w.edge_iter()
                        .filter_map(|e| edges.get(&e.id()).map(|ne| if e.front() == e.absolute_front() { ne.clone() } else { ne.inverse() }))
                        .collect::<Vec<_>>()
                        .into()
                })
                .collect();
            let nf = match surfaces.get(i).cloned().flatten() {
                Some(mut surface) => {
                    // Outward like the old face (checked at a boundary point).
                    let probe = f.boundaries().first().and_then(|w| w.vertex_iter().next()).map(|v| from_p3(v.point()));
                    let want = probe.and_then(|p| old.get(i).and_then(|s| s.normal(p)));
                    if let (Some(p), Some(want)) = (probe, want) {
                        use mt::{ParametricSurface3D, SearchNearestParameter};
                        if let Some((u, v)) = surface.search_nearest_parameter(p3(p), None, 100) {
                            let nn = surface.normal(u, v);
                            if Vec3::new(nn.x, nn.y, nn.z).dot(want) < 0.0 {
                                surface = mt::Invertible::inverse(&surface);
                            }
                        }
                    }
                    mt::Face::try_new(wires, surface).map_err(|e| fail(&e.to_string()))?
                }
                None => {
                    // Unchanged surface: rebuild on the oriented boundary.
                    let mut nf = mt::Face::try_new(
                        f.absolute_boundaries()
                            .iter()
                            .map(|w| {
                                w.edge_iter()
                                    .filter_map(|e| {
                                        edges.get(&e.id()).map(|ne| if e.front() == e.absolute_front() { ne.clone() } else { ne.inverse() })
                                    })
                                    .collect::<Vec<_>>()
                                    .into()
                            })
                            .collect(),
                        f.surface(),
                    )
                    .map_err(|e| fail(&e.to_string()))?;
                    if !f.orientation() {
                        nf.invert();
                    }
                    nf
                }
            };
            out.push(nf);
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| fail(&e.to_string()))?;
        Body::new(solid)
    })
}

/// The chosen faces and every face on the same surface joined to them (the halves of a hole's
/// wall), as face indices. Pieces of a face split on purpose (Split Face) stay apart.
pub fn with_same_surface(b: &Body, chosen: &[usize]) -> Vec<usize> {
    let faces: Vec<mt::Face> = b.solid.face_iter().cloned().collect();
    let size = b.size();
    let tol = (size * 1e-7).max(1e-9) * 100.0;
    let surfs: Vec<Option<Surf>> = faces.iter().map(|f| surf_of(f, tol)).collect();
    let same = |x: &Surf, y: &Surf| match (*x, *y) {
        (Surf::Plane { n, d }, Surf::Plane { n: m, d: e }) => n.dot(m) > 1.0 - 1e-9 && (d - e).abs() < tol,
        (Surf::Cylinder { o, a, r, convex }, Surf::Cylinder { o: o2, a: a2, r: r2, convex: c2 }) => {
            a.dot(a2).abs() > 1.0 - 1e-9 && (r - r2).abs() < tol && convex == c2 && { (o2 - o - a * (o2 - o).dot(a)).len() < tol }
        }
        (Surf::Sphere { c, r, convex }, Surf::Sphere { c: c2, r: r2, convex: k2 }) => c.dist(c2) < tol && (r - r2).abs() < tol && convex == k2,
        (Surf::Cone { v, a, half, convex }, Surf::Cone { v: v2, a: a2, half: h2, convex: k2 }) => {
            v.dist(v2) < tol && a.dot(a2) > 1.0 - 1e-9 && (half - h2).abs() < 1e-9 && convex == k2
        }
        (Surf::Torus { o, a, big, h, r, convex }, Surf::Torus { o: o2, a: a2, big: b2, h: h2, r: r2, convex: k2 }) => {
            (o + a * h).dist(o2 + a2 * h2) < tol && a.dot(a2).abs() > 1.0 - 1e-9 && (big - b2).abs() < tol && (r - r2).abs() < tol && convex == k2
        }
        _ => false,
    };
    let keep = b.split_keep();
    let mut out: Vec<usize> = chosen.to_vec();
    let mut grew = true;
    while grew {
        grew = false;
        for (i, f) in faces.iter().enumerate() {
            if out.contains(&i) {
                continue;
            }
            let Some(Some(si)) = surfs.get(i) else { continue };
            let joins = out.iter().any(|j| {
                surfs.get(*j).and_then(|x| x.as_ref()).is_some_and(|sj| same(si, sj))
                    && faces.get(*j).is_some_and(|g| {
                        g.edge_iter().any(|e| {
                            f.edge_iter().any(|x| x.id() == e.id())
                                && (keep.is_empty() || !crate::heal::on_split_line(&crate::heal::edge_points(&e), keep))
                        })
                    })
            });
            if joins {
                out.push(i);
                grew = true;
            }
        }
    }
    out
}
