//! Topology counts comparable with kernels that use periodic faces and closed edges.
//!
//! Our kernel splits a full cylinder into two half faces and a circle into two arcs. Faces
//! are classified from their tessellation (plane, cylinder, sphere, other); adjacent faces on
//! the same analytic surface are merged, edges between merged faces disappear, and chains of
//! edges through vertices that only join two pieces of the same edge are counted once.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use solvecraft_geom::{Mesh, Vec3};

use crate::Result;
use crate::body::{Body, from_p3};

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct TopoCounts {
    pub faces: usize,
    pub edges: usize,
    pub vertices: usize,
    pub face_types: BTreeMap<String, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Surf {
    Plane { n: Vec3, d: f64 },
    Cylinder { axis: Vec3, p: Vec3, r: f64 },
    Sphere { c: Vec3, r: f64 },
    Cone { axis: Vec3, apex: Vec3, cos: f64 },
    Other(usize),
}

impl Surf {
    fn name(&self) -> &'static str {
        match self {
            Surf::Plane { .. } => "plane",
            Surf::Cylinder { .. } => "cylinder",
            Surf::Sphere { .. } => "sphere",
            Surf::Cone { .. } => "cone",
            Surf::Other(_) => "other",
        }
    }
    fn same(&self, o: &Surf, tol: f64) -> bool {
        match (*self, *o) {
            (Surf::Plane { n, d }, Surf::Plane { n: n2, d: d2 }) => n.dot(n2) > 1.0 - 1e-6 && (d - d2).abs() < tol,
            (Surf::Cylinder { axis, p, r }, Surf::Cylinder { axis: a2, p: p2, r: r2 }) => {
                let off = p2 - p;
                axis.dot(a2).abs() > 1.0 - 1e-6 && (r - r2).abs() < tol && (off - axis * off.dot(axis)).len() < tol
            }
            (Surf::Sphere { c, r }, Surf::Sphere { c: c2, r: r2 }) => c.dist(c2) < tol && (r - r2).abs() < tol,
            (Surf::Cone { axis, apex, cos }, Surf::Cone { axis: a2, apex: p2, cos: c2 }) => {
                axis.dot(a2) > 1.0 - 1e-5 && (cos - c2).abs() < 1e-4 && apex.dist(p2) < tol * 10.0
            }
            _ => false,
        }
    }
}

/// Least-squares circle through 2D points (Kåsa fit): centre and radius.
fn fit_circle(pts: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = pts.len() as f64;
    if pts.len() < 3 {
        return None;
    }
    let (mx, my) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0 / n, a.1 + p.1 / n));
    let (mut suu, mut suv, mut svv, mut suuu, mut svvv, mut suvv, mut svuu) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for (x, y) in pts {
        let (u, v) = (x - mx, y - my);
        suu += u * u;
        suv += u * v;
        svv += v * v;
        suuu += u * u * u;
        svvv += v * v * v;
        suvv += u * v * v;
        svuu += v * u * u;
    }
    let det = suu * svv - suv * suv;
    if det.abs() < 1e-18 {
        return None;
    }
    let b1 = 0.5 * (suuu + suvv);
    let b2 = 0.5 * (svvv + svuu);
    let uc = (b1 * svv - b2 * suv) / det;
    let vc = (suu * b2 - suv * b1) / det;
    let r = (uc * uc + vc * vc + (suu + svv) / n).sqrt();
    Some((uc + mx, vc + my, r))
}

fn classify(pts: &[Vec3], nrm: &[Vec3], tol: f64, id: usize) -> Surf {
    let Some(&n0) = nrm.first() else { return Surf::Other(id) };
    let Some(&p0) = pts.first() else { return Surf::Other(id) };
    if nrm.iter().all(|n| n.dot(n0) > 1.0 - 1e-5) {
        let n = n0.normalized().unwrap_or(Vec3::Z);
        if pts.iter().all(|p| (p.dot(n) - p0.dot(n)).abs() < tol) {
            return Surf::Plane { n, d: p0.dot(n) };
        }
    }
    // Cylinder: all normals perpendicular to one axis.
    let far = nrm.iter().copied().min_by(|a, b| a.dot(n0).abs().total_cmp(&b.dot(n0).abs())).unwrap_or(n0);
    if let Some(axis) = n0.cross(far).normalized()
        && nrm.iter().all(|n| n.dot(axis).abs() < 2e-3)
    {
        let u = axis.any_perp();
        let v = axis.cross(u);
        let pl: Vec<(f64, f64)> = pts.iter().map(|p| (p.dot(u), p.dot(v))).collect();
        if let Some((cu, cv, r)) = fit_circle(&pl)
            && pl.iter().all(|(x, y)| ((x - cu).hypot(y - cv) - r).abs() < tol.max(r * 2e-3))
        {
            let axis = if axis.x + axis.y * 1e-3 + axis.z * 1e-6 < 0.0 { -axis } else { axis };
            return Surf::Cylinder { axis, p: u * cu + v * cv, r };
        }
    }
    // Cone: normals at a constant angle to one axis; the apex lies on every tangent plane.
    if let Some((axis, cos)) = cone_signature(&pts.iter().copied().zip(nrm.iter().copied()).collect::<Vec<_>>())
        && cos.abs() > 1e-3
        && cos.abs() < 1.0 - 1e-3
    {
        // Least squares: n_i · v = n_i · p_i.
        let mut m = [[0.0f64; 3]; 3];
        let mut r = [0.0f64; 3];
        for (p, n) in pts.iter().zip(nrm) {
            let nv = [n.x, n.y, n.z];
            let k = n.dot(*p);
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] += nv[i] * nv[j];
                }
                r[i] += nv[i] * k;
            }
        }
        if let Some(v) = solve3(m, r) {
            let apex = Vec3::new(v[0], v[1], v[2]);
            if pts.iter().zip(nrm).all(|(p, n)| (*p - apex).dot(*n).abs() < tol.max(1e-6) * 10.0) {
                return Surf::Cone { axis, apex, cos };
            }
        }
    }
    // Sphere: algebraic least-squares fit |p|² = 2c·p + k, then every point at the same distance.
    if let Some((c, r)) = fit_sphere(pts) {
        let spread = pts.iter().map(|p| p.dist(p0)).fold(0.0, f64::max);
        if spread > tol && pts.iter().all(|p| (p.dist(c) - r).abs() < tol.max(r * 2e-3)) && r < spread * 1e4 {
            return Surf::Sphere { c, r };
        }
    }
    Surf::Other(id)
}

/// Per-face vertex samples (position, normal) from the tessellation.
fn face_vertices(mesh: &Mesh, nf: usize) -> Vec<Vec<(Vec3, Vec3)>> {
    let mut out: Vec<Vec<(Vec3, Vec3)>> = vec![Vec::new(); nf];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        for &k in t {
            if let (Some(p), Some(n), Some(v)) = (mesh.positions.get(k as usize), mesh.normals.get(k as usize), out.get_mut(*f as usize)) {
                v.push((*p, *n));
            }
        }
    }
    out
}

/// Normal of face `f` nearest to `p`.
fn normal_near(verts: &[(Vec3, Vec3)], p: Vec3) -> Option<Vec3> {
    verts.iter().min_by(|a, b| a.0.dist(p).total_cmp(&b.0.dist(p))).map(|v| v.1)
}

/// Cone signature of a non-analytic face: (axis, cos of the normal/axis angle) when all normals
/// make the same angle with one axis.
fn cone_signature(verts: &[(Vec3, Vec3)]) -> Option<(Vec3, f64)> {
    let n0 = verts.first()?.1;
    let n1 = verts.iter().map(|v| v.1).max_by(|a, b| (*a - n0).len().total_cmp(&(*b - n0).len()))?;
    let n2 = verts.iter().map(|v| v.1).max_by(|a, b| (*a - n0).cross(n1 - n0).len().total_cmp(&(*b - n0).cross(n1 - n0).len()))?;
    let mut a = (n1 - n0).cross(n2 - n0).normalized()?;
    if a.x + a.y * 1e-3 + a.z * 1e-6 < 0.0 {
        a = -a;
    }
    let c = n0.dot(a);
    verts.iter().all(|v| (v.1.dot(a) - c).abs() < 2e-3).then_some((a, c))
}

/// Do adjacent faces `a` and `b` (sharing the edge polyline `edge`) lie on one surface?
/// Analytic classes compare parameters; other faces must join smoothly along the edge and have
/// the same cone signature (or both have none).
fn same_surface(surfs: &[Surf], verts: &[Vec<(Vec3, Vec3)>], edge: Option<&Vec<Vec3>>, a: usize, b: usize, tol: f64) -> bool {
    let (Some(sa), Some(sb)) = (surfs.get(a), surfs.get(b)) else { return false };
    match (sa, sb) {
        (Surf::Other(_), Surf::Other(_)) => {}
        _ => return sa.same(sb, tol),
    }
    let (Some(va), Some(vb), Some(e)) = (verts.get(a), verts.get(b), edge) else { return false };
    let smooth = e.iter().all(|p| match (normal_near(va, *p), normal_near(vb, *p)) {
        (Some(na), Some(nb)) => na.dot(nb) > 1.0 - 2e-3,
        _ => false,
    });
    if !smooth {
        return false;
    }
    match (cone_signature(va), cone_signature(vb)) {
        (Some((xa, ca)), Some((xb, cb))) => xa.dot(xb) > 1.0 - 1e-4 && (ca - cb).abs() < 1e-3,
        (None, None) => true,
        _ => false,
    }
}

/// Solve a 3×3 system (Cramer's rule).
fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-12 {
        return None;
    }
    let mut out = [0.0; 3];
    for (k, o) in out.iter_mut().enumerate() {
        let mut mk = m;
        for (row, rv) in mk.iter_mut().zip(r) {
            row[k] = rv;
        }
        *o = det(mk) / d;
    }
    Some(out)
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

/// Per edge (in `Body::edges` order): is it a seam between two pieces of the same surface?
pub fn seam_flags(b: &Body, mesh: &Mesh) -> Vec<bool> {
    let solid = &*b.solid;
    let nf = solid.face_iter().count();
    let tol = (b.size() * 1e-4).max(1e-6);
    let surfs = classify_faces(mesh, nf, tol);
    let verts = face_vertices(mesh, nf);
    let mut faces_of: HashMap<String, Vec<usize>> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for (fi, f) in solid.face_iter().enumerate() {
        for e in f.edge_iter() {
            let k = format!("{:?}", e.id());
            let v = faces_of.entry(k.clone()).or_insert_with(|| {
                order.push(k);
                Vec::new()
            });
            if !v.contains(&fi) {
                v.push(fi);
            }
        }
    }
    order
        .iter()
        .enumerate()
        .map(|(ei, k)| match faces_of.get(k).map(Vec::as_slice) {
            Some([a, b2]) => {
                !matches!(surfs.get(*a), Some(Surf::Plane { .. })) && same_surface(&surfs, &verts, mesh.edges.get(ei), *a, *b2, tol * 10.0)
            }
            _ => false,
        })
        .collect()
}

/// Least-squares sphere through points: centre and radius.
fn fit_sphere(pts: &[Vec3]) -> Option<(Vec3, f64)> {
    if pts.len() < 4 {
        return None;
    }
    // Centre the data for conditioning.
    let m = pts.iter().fold(Vec3::ZERO, |a, p| a + *p) / pts.len() as f64;
    let mut a = [[0.0f64; 5]; 4];
    for p in pts {
        let q = *p - m;
        let row = [2.0 * q.x, 2.0 * q.y, 2.0 * q.z, 1.0];
        let rhs = q.dot(q);
        for i in 0..4 {
            for j in 0..4 {
                a[i][j] += row[i] * row[j];
            }
            a[i][4] += row[i] * rhs;
        }
    }
    for col in 0..4 {
        let piv = (col..4).max_by(|x, y| a[*x][col].abs().total_cmp(&a[*y][col].abs()))?;
        if a[piv][col].abs() < 1e-18 {
            return None;
        }
        a.swap(col, piv);
        for r in 0..4 {
            if r != col {
                let f = a[r][col] / a[col][col];
                let prow = a[col];
                for (x, v) in a[r].iter_mut().zip(prow) {
                    *x -= f * v;
                }
            }
        }
    }
    let c = Vec3::new(a[0][4] / a[0][0], a[1][4] / a[1][1], a[2][4] / a[2][2]);
    let k = a[3][4] / a[3][3];
    let r2 = k + c.dot(c);
    (r2 > 0.0 && r2.is_finite()).then(|| (c + m, r2.sqrt()))
}

fn classify_faces(mesh: &Mesh, nf: usize, tol: f64) -> Vec<Surf> {
    let mut pts: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    let mut nrm: Vec<Vec<Vec3>> = vec![Vec::new(); nf];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        for &k in t {
            if let (Some(p), Some(n), Some(pv), Some(nv)) =
                (mesh.positions.get(k as usize), mesh.normals.get(k as usize), pts.get_mut(*f as usize), nrm.get_mut(*f as usize))
            {
                pv.push(*p);
                nv.push(*n);
            }
        }
    }
    (0..nf).map(|i| classify(pts.get(i).map(Vec::as_slice).unwrap_or(&[]), nrm.get(i).map(Vec::as_slice).unwrap_or(&[]), tol * 10.0, i)).collect()
}

/// Merged face/edge/vertex counts (see module docs). `mesh` must be `b.tessellate(..)`.
pub fn merged_topology(b: &Body, mesh: &Mesh) -> Result<TopoCounts> {
    let solid = &*b.solid;
    let nf = solid.face_iter().count();
    let tol = (b.size() * 1e-4).max(1e-6);
    let surfs = classify_faces(mesh, nf, tol);
    let verts = face_vertices(mesh, nf);
    // Zero-area faces (a revolved profile edge lying on the axis) don't count.
    let mut area = vec![0.0f64; nf];
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if let (Some([p, q, r]), Some(a)) = (mesh.tri(t), area.get_mut(*f as usize)) {
            *a += (q - p).cross(r - p).len() * 0.5;
        }
    }
    let degenerate: Vec<bool> = area.iter().map(|a| *a < (b.size() * 1e-6).powi(2)).collect();
    let is_deg = |f: usize| degenerate.get(f).copied().unwrap_or(false);

    // Edge → faces, edge → vertices.
    let mut edge_faces: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut edge_verts: HashMap<usize, (usize, usize)> = HashMap::new();
    let mut eid: HashMap<String, usize> = HashMap::new();
    let mut vid: HashMap<String, usize> = HashMap::new();
    let mut vpos: Vec<Vec3> = Vec::new();
    for (fi, f) in solid.face_iter().enumerate() {
        for e in f.edge_iter() {
            let key = format!("{:?}", e.id());
            let n = eid.len();
            let ei = *eid.entry(key).or_insert(n);
            let faces = edge_faces.entry(ei).or_default();
            if !faces.contains(&fi) {
                faces.push(fi);
            }
            let mut vix = |v: &truck_modeling::Vertex| {
                let k = format!("{:?}", v.id());
                let n = vid.len();
                let i = *vid.entry(k).or_insert(n);
                if i == vpos.len() {
                    vpos.push(from_p3(v.point()));
                }
                i
            };
            let (a, bb) = (vix(e.front()), vix(e.back()));
            edge_verts.insert(ei, (a, bb));
        }
    }
    // Merge faces.
    let mut fp: Vec<usize> = (0..nf).collect();
    for (ei, faces) in &edge_faces {
        if let [a, b] = faces[..]
            && same_surface(&surfs, &verts, mesh.edges.get(*ei), a, b, tol * 10.0)
        {
            let (ra, rb) = (find(&mut fp, a), find(&mut fp, b));
            if let Some(s) = fp.get_mut(ra) {
                *s = rb;
            }
        }
    }
    let mut groups: BTreeMap<usize, usize> = BTreeMap::new();
    let mut face_types: BTreeMap<String, usize> = BTreeMap::new();
    for f in 0..nf {
        if is_deg(f) {
            continue;
        }
        let r = find(&mut fp, f);
        if groups.insert(r, f).is_none()
            && let Some(s) = surfs.get(f)
        {
            *face_types.entry(s.name().into()).or_insert(0) += 1;
        }
    }
    // Remaining edges, keyed by merged face pair.
    let mut remaining: Vec<(usize, (usize, usize), (usize, usize))> = Vec::new(); // (edge, verts, face pair)
    let mut eids: Vec<&usize> = edge_faces.keys().collect();
    eids.sort();
    for e in eids {
        let faces = edge_faces.get(e).cloned().unwrap_or_default();
        if faces.iter().any(|f| is_deg(*f)) {
            continue;
        }
        let gs: Vec<usize> = faces.iter().map(|f| find(&mut fp, *f)).collect();
        let pair = match gs[..] {
            [a, b] if a == b => continue,
            [a, b] => (a.min(b), a.max(b)),
            [a] => (a, a),
            _ => (usize::MAX, usize::MAX),
        };
        if let Some(v) = edge_verts.get(e) {
            remaining.push((*e, *v, pair));
        }
    }
    // Vertex degree among remaining edges.
    let mut deg: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, (_, (a, b), _)) in remaining.iter().enumerate() {
        deg.entry(*a).or_default().push(i);
        if b != a {
            deg.entry(*b).or_default().push(i);
        }
    }
    let mut ep: Vec<usize> = (0..remaining.len()).collect();
    let mut merge_vertex: Vec<usize> = Vec::new();
    for (v, es) in &deg {
        if let [i, j] = es[..]
            && let (Some(a), Some(b)) = (remaining.get(i), remaining.get(j))
            && a.2 == b.2
        {
            let (ri, rj) = (find(&mut ep, i), find(&mut ep, j));
            if let Some(s) = ep.get_mut(ri) {
                *s = rj;
            }
            merge_vertex.push(*v);
        }
    }
    let mut edge_groups: BTreeMap<usize, bool> = BTreeMap::new(); // root → has a real end vertex
    for (i, (_, (a, b), _)) in remaining.iter().enumerate() {
        let r = find(&mut ep, i);
        let real = !merge_vertex.contains(a) || !merge_vertex.contains(b);
        let e = edge_groups.entry(r).or_insert(false);
        *e |= real;
    }
    let real_vertices = deg.keys().filter(|v| !merge_vertex.contains(v)).count();
    let closed_loops = edge_groups.values().filter(|r| !**r).count();
    let _ = vpos;
    // A cone that reaches its apex has a degenerate edge and a vertex there (as other kernels
    // count them).
    let mut apexes = 0;
    for (root, f) in &groups {
        if let Some(Surf::Cone { apex, .. }) = surfs.get(*f) {
            let reaches = (0..nf)
                .filter(|g| find(&mut fp.clone(), *g) == *root)
                .any(|g| verts.get(g).is_some_and(|v| v.iter().any(|(p, _)| p.dist(*apex) < tol * 10.0)));
            if reaches {
                apexes += 1;
            }
        }
    }
    Ok(TopoCounts { faces: groups.len(), edges: edge_groups.len() + apexes, vertices: real_vertices + closed_loops + apexes, face_types })
}

/// A cylindrical face of a body near a point: where its axis is, which way, its radius, the
/// extent of the face along the axis (from the axis point), and whether it is a hole (the
/// material outside the cylinder).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct CylinderFace {
    pub axis_point: Vec3,
    pub axis: Vec3,
    pub radius: f64,
    pub start: f64,
    pub end: f64,
    pub internal: bool,
}

pub fn cylinder_face_at(b: &Body, p: Vec3) -> Option<CylinderFace> {
    let size = b.size();
    let mesh = b.tessellate((size * 1e-3).max(1e-3)).ok()?;
    // The face of the triangle nearest the point.
    let mut best: Option<(f64, u32)> = None;
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let Some([a, bb, c]) = mesh.tri(t) else { continue };
        let d = ((a + bb + c) / 3.0).dist(p).min(a.dist(p)).min(bb.dist(p)).min(c.dist(p));
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, *f));
        }
    }
    let (d, face) = best?;
    if d > size * 0.05 {
        return None;
    }
    let nf = b.face_count();
    let tol = (size * 1e-4).max(1e-6);
    let surfs = classify_faces(&mesh, nf, tol);
    let Surf::Cylinder { axis, p: ap, r } = *surfs.get(face as usize)? else { return None };
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut inward = 0.0;
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if *f != face {
            continue;
        }
        for k in t {
            let (Some(q), Some(n)) = (mesh.positions.get(*k as usize), mesh.normals.get(*k as usize)) else { continue };
            let s = (*q - ap).dot(axis);
            lo = lo.min(s);
            hi = hi.max(s);
            let radial = *q - ap - axis * s;
            inward += n.dot(radial);
        }
    }
    Some(CylinderFace { axis_point: ap, axis, radius: r, start: lo, end: hi, internal: inward < 0.0 })
}
