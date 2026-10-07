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
    Other(usize),
}

impl Surf {
    fn name(&self) -> &'static str {
        match self {
            Surf::Plane { .. } => "plane",
            Surf::Cylinder { .. } => "cylinder",
            Surf::Sphere { .. } => "sphere",
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
    // Sphere: points equidistant from the point where normals meet.
    let c_est = {
        let mut acc = Vec3::ZERO;
        let mut k = 0.0;
        for (p, n) in pts.iter().zip(nrm) {
            // Assume radius from the first pair.
            let r0 = (p0 - *p).len() / ((n0 - *n).len().max(1e-12));
            if r0.is_finite() && r0 > 0.0 && (n0 - *n).len() > 0.1 {
                acc += *p - *n * r0;
                k += 1.0;
            }
        }
        (k > 0.0).then(|| acc / k)
    };
    if let Some(c) = c_est {
        let r = pts.iter().map(|p| p.dist(c)).sum::<f64>() / pts.len().max(1) as f64;
        if pts.iter().all(|p| (p.dist(c) - r).abs() < tol.max(r * 2e-3)) {
            return Surf::Sphere { c, r };
        }
    }
    Surf::Other(id)
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

/// Merged face/edge/vertex counts (see module docs). `mesh` must be `b.tessellate(..)`.
pub fn merged_topology(b: &Body, mesh: &Mesh) -> Result<TopoCounts> {
    let solid = &*b.solid;
    let nf = solid.face_iter().count();
    let tol = (b.size() * 1e-4).max(1e-6);
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
    let surfs: Vec<Surf> = (0..nf)
        .map(|i| classify(pts.get(i).map(Vec::as_slice).unwrap_or(&[]), nrm.get(i).map(Vec::as_slice).unwrap_or(&[]), tol * 10.0, i))
        .collect();

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
    for faces in edge_faces.values() {
        if let [a, b] = faces[..]
            && let (Some(sa), Some(sb)) = (surfs.get(a), surfs.get(b))
            && sa.same(sb, tol * 10.0)
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
    Ok(TopoCounts { faces: groups.len(), edges: edge_groups.len(), vertices: real_vertices + closed_loops, face_types })
}
