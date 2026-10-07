//! Convex polyhedra from planes, and the operations that use them on convex planar bodies:
//! shell (hollow out with open faces) and draft (tilt faces about a neutral plane). The solid
//! is built directly from its planes (vertices are triple-plane intersections), so it is exact
//! and needs no booleans except the final shell subtraction.

use std::collections::HashMap;

use solvecraft_geom::{Plane, Vec3};
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3};
use crate::{KernelError, Result, guard};

/// A half-space `n · x <= d` (n is the outward unit normal).
#[derive(Clone, Copy, Debug)]
pub struct HalfSpace {
    pub n: Vec3,
    pub d: f64,
}

fn det3(a: Vec3, b: Vec3, c: Vec3) -> f64 {
    a.dot(b.cross(c))
}

/// The convex polyhedron bounded by the half-spaces.
pub fn convex_polyhedron(hs: &[HalfSpace]) -> Result<Body> {
    if hs.len() < 4 || hs.len() > 200 {
        return Err(KernelError::Invalid("a polyhedron needs 4…200 planes".into()));
    }
    let scale = hs.iter().map(|h| h.d.abs()).fold(1.0, f64::max);
    let eps = scale * 1e-9;
    // Vertices: intersections of three planes inside all others.
    let mut verts: Vec<Vec3> = Vec::new();
    for i in 0..hs.len() {
        for j in (i + 1)..hs.len() {
            for k in (j + 1)..hs.len() {
                let (a, b, c) = (hs[i], hs[j], hs[k]);
                let den = det3(a.n, b.n, c.n);
                if den.abs() < 1e-12 {
                    continue;
                }
                let x = (b.n.cross(c.n) * a.d + c.n.cross(a.n) * b.d + a.n.cross(b.n) * c.d) / den;
                if !x.is_finite() || hs.iter().any(|h| h.n.dot(x) > h.d + eps * 10.0) {
                    continue;
                }
                if !verts.iter().any(|v| v.dist(x) < scale * 1e-8) {
                    verts.push(x);
                }
            }
        }
    }
    if verts.len() < 4 {
        return Err(KernelError::Invalid("the planes don't enclose a solid".into()));
    }
    // Faces: the vertices on each plane, ordered counter-clockwise about its outward normal.
    let mut faces_idx: Vec<Vec<usize>> = Vec::new();
    for h in hs {
        let on: Vec<usize> = (0..verts.len()).filter(|i| verts.get(*i).is_some_and(|v| (h.n.dot(*v) - h.d).abs() < scale * 1e-7)).collect();
        if on.len() < 3 {
            continue;
        }
        let c = on.iter().filter_map(|i| verts.get(*i)).fold(Vec3::ZERO, |a, v| a + *v) / on.len() as f64;
        let u = h.n.any_perp();
        let w = h.n.cross(u);
        let mut ordered = on.clone();
        ordered.sort_by(|a, b| {
            let ang = |i: usize| verts.get(i).map(|v| (*v - c).dot(w).atan2((*v - c).dot(u))).unwrap_or(0.0);
            ang(*a).total_cmp(&ang(*b))
        });
        faces_idx.push(ordered);
    }
    guard("polyhedron", || {
        let tv: Vec<mt::Vertex> = verts.iter().map(|v| builder::vertex(p3(*v))).collect();
        let mut edges: HashMap<(usize, usize), mt::Edge> = HashMap::new();
        let mut faces: Vec<mt::Face> = Vec::new();
        for f in &faces_idx {
            let n = f.len();
            let mut wire: Vec<mt::Edge> = Vec::with_capacity(n);
            for k in 0..n {
                let (Some(&a), Some(&b)) = (f.get(k), f.get((k + 1) % n)) else { continue };
                let key = (a.min(b), a.max(b));
                let e = match edges.get(&key) {
                    Some(e) => e.clone(),
                    None => {
                        let (Some(va), Some(vb)) = (tv.get(key.0), tv.get(key.1)) else { continue };
                        let e = builder::line(va, vb);
                        edges.insert(key, e.clone());
                        e
                    }
                };
                wire.push(if a < b { e } else { e.inverse() });
            }
            let face = builder::try_attach_plane(&[wire.into()]).map_err(|e| KernelError::Failed(format!("polyhedron face: {e}")))?;
            faces.push(face);
        }
        let shell: mt::Shell = faces.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Failed(format!("polyhedron: {e}")))?;
        Body::new(solid)
    })
}

/// The half-spaces of a convex body with only planar faces, or an error.
pub fn planar_convex(b: &Body) -> Result<Vec<(HalfSpace, Vec3)>> {
    let tol = (b.size() * 1e-3).max(1e-3);
    let faces = b.faces(tol)?;
    let mut out: Vec<(HalfSpace, Vec3)> = Vec::new();
    for f in &faces {
        let n = f.plane_normal.ok_or_else(|| KernelError::Failed("not supported yet: bodies with curved faces".into()))?;
        let d = n.dot(f.centroid);
        if !out.iter().any(|(h, _)| h.n.dot(n) > 1.0 - 1e-9 && (h.d - d).abs() < tol) {
            out.push((HalfSpace { n, d }, f.centroid));
        }
    }
    let verts: Vec<Vec3> = b.solid.vertex_iter().map(|v| from_p3(v.point())).collect();
    for (h, _) in &out {
        if verts.iter().any(|v| h.n.dot(*v) > h.d + tol) {
            return Err(KernelError::Failed("not supported yet: shell and draft of non-convex bodies".into()));
        }
    }
    Ok(out)
}

/// Hollow a convex planar body: walls of `thickness` inside, faces near `open` removed.
pub fn shell(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    if !(thickness.is_finite() && thickness > 1e-6) {
        return Err(KernelError::Invalid("shell thickness must be positive".into()));
    }
    let hs = planar_convex(b)?;
    let size = b.size();
    let inner: Vec<HalfSpace> = hs
        .iter()
        .map(|(h, c)| {
            let removed = open.iter().any(|p| (h.n.dot(*p) - h.d).abs() < size * 1e-4 + 1e-6 && p.dist(*c) < size * 2.0);
            if removed { HalfSpace { n: h.n, d: h.d + thickness + size * 0.05 } } else { HalfSpace { n: h.n, d: h.d - thickness } }
        })
        .collect();
    if !hs.iter().zip(&inner).any(|((h, _), i)| i.d > h.d) {
        return Err(KernelError::Invalid("select at least one face to remove".into()));
    }
    let cavity = convex_polyhedron(&inner)?;
    crate::ops::boolean(b, &cavity, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("the shell removed everything".into()))
}

/// Tilt the faces near `faces` by `angle` about their line on the neutral plane, so that they
/// lean in (positive angle) going along `pull`.
pub fn draft(b: &Body, faces: &[Vec3], neutral: &Plane, pull: Vec3, angle: f64) -> Result<Body> {
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("draft angle must be between −90° and 90°".into()));
    }
    let pull = pull.normalized().ok_or_else(|| KernelError::Invalid("pull direction".into()))?;
    let hs = planar_convex(b)?;
    let size = b.size();
    let mut out = Vec::new();
    let mut moved = 0;
    for (h, c) in &hs {
        let chosen = faces.iter().any(|p| (h.n.dot(*p) - h.d).abs() < size * 1e-4 + 1e-6 && p.dist(*c) < size * 2.0);
        if !chosen {
            out.push(*h);
            continue;
        }
        if h.n.dot(pull).abs() > 1e-6 {
            return Err(KernelError::Failed("not supported yet: drafting faces that aren't parallel to the pull direction".into()));
        }
        // Hinge: the face's line on the neutral plane; rotate the normal toward the pull.
        let n2 = (h.n * angle.cos() + pull * angle.sin()).normalized().ok_or_else(|| KernelError::Failed("draft".into()))?;
        let hinge_pt =
            neutral.intersect_ray(*c, pull).ok_or_else(|| KernelError::Failed("the neutral plane is parallel to the pull direction".into()))?;
        out.push(HalfSpace { n: n2, d: n2.dot(hinge_pt) });
        moved += 1;
    }
    if moved == 0 {
        return Err(KernelError::Invalid("no faces to draft".into()));
    }
    convex_polyhedron(&out)
}
