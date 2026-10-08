//! Convex polyhedra from planes, and the operations that use them on convex planar bodies:
//! shell (hollow out with open faces) and draft (tilt faces about a neutral plane). The solid
//! is built directly from its planes (vertices are triple-plane intersections), so it is exact
//! and needs no booleans except the final shell subtraction.

use std::collections::HashMap;

use solvecraft_geom::{Plane, Vec3};
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
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

/// Vertices, and per half-space with a face its vertex loop (counter-clockwise about the
/// outward normal), of the convex polyhedron bounded by the half-spaces.
fn combinatorics(hs: &[HalfSpace]) -> Result<(Vec<Vec3>, Vec<(usize, Vec<usize>)>)> {
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
    let mut faces_idx: Vec<(usize, Vec<usize>)> = Vec::new();
    for (hi, h) in hs.iter().enumerate() {
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
        faces_idx.push((hi, ordered));
    }
    Ok((verts, faces_idx))
}

/// The convex polyhedron bounded by the half-spaces.
pub fn convex_polyhedron(hs: &[HalfSpace]) -> Result<Body> {
    let (verts, faces_idx) = combinatorics(hs)?;
    guard("polyhedron", || {
        let tv: Vec<mt::Vertex> = verts.iter().map(|v| builder::vertex(p3(*v))).collect();
        let mut edges: HashMap<(usize, usize), mt::Edge> = HashMap::new();
        let mut faces: Vec<mt::Face> = Vec::new();
        for (_, f) in &faces_idx {
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
    b.require_brep("this operation")?;
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
    b.require_brep("shell")?;
    if !(thickness.is_finite() && thickness > 1e-6) {
        return Err(KernelError::Invalid("shell thickness must be positive".into()));
    }
    let hs = match planar_convex(b) {
        Ok(h) => h,
        Err(_) => return shell_planar(b, open, thickness),
    };
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
    b.require_brep("draft")?;
    if !angle.is_finite() || angle.abs() >= std::f64::consts::FRAC_PI_2 - 1e-3 {
        return Err(KernelError::Invalid("draft angle must be between −90° and 90°".into()));
    }
    let pull = pull.normalized().ok_or_else(|| KernelError::Invalid("pull direction".into()))?;
    let hs = match planar_convex(b) {
        Ok(h) => h,
        Err(_) => return draft_walls(b, faces, neutral, pull, angle),
    };
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

/// Draft of a non-convex body: walls (planes, cylinders) next to caps square to `pull`.
fn draft_walls(b: &Body, at: &[Vec3], neutral: &Plane, pull: Vec3, angle: f64) -> Result<Body> {
    let healed = Body::new(crate::heal::heal(b.deep_copy(), b.size()))?;
    let size = healed.size();
    let mesh = healed.tessellate((size * 1e-3).max(1e-3))?;
    let mut chosen: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (point_tri(*p, x, y, z), *f as usize)))
            .min_by(|a, c| a.0.total_cmp(&c.0));
        match near {
            Some((d, f)) if d < size * 1e-3 + 1e-6 => {
                if !chosen.contains(&f) {
                    chosen.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {:?}", [p.x, p.y, p.z]))),
        }
    }
    crate::offset::draft_walls(&healed, &chosen, neutral, pull, angle)
}

/// Round every edge of a convex body with planar faces by `r`: faces shrink, each edge becomes a
/// cylinder and each corner a sphere patch (the body is the inner polyhedron grown by a ball).
pub fn round_all_edges(b: &Body, r: f64) -> Result<Body> {
    let hs: Vec<HalfSpace> = planar_convex(b)?.into_iter().map(|(h, _)| h).collect();
    let inner: Vec<HalfSpace> = hs.iter().map(|h| HalfSpace { n: h.n, d: h.d - r }).collect();
    let (q, faces) = combinatorics(&inner)?;
    if faces.len() != hs.len() {
        return Err(KernelError::Failed("not supported yet: the rounding is larger than a face".into()));
    }
    let normal = |hi: usize| hs.get(hi).map(|h| h.n).unwrap_or(Vec3::Z);
    // Edges of the inner polyhedron: (vertex a, vertex b) → (face left, face right).
    let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (hi, lp) in &faces {
        let n = lp.len();
        for k in 0..n {
            let (Some(&a), Some(&bb)) = (lp.get(k), lp.get((k + 1) % n)) else { continue };
            edge_faces.entry((a.min(bb), a.max(bb))).or_default().push(*hi);
        }
    }
    if edge_faces.values().any(|f| f.len() != 2) {
        return Err(KernelError::Failed("not supported yet: rounding this shape".into()));
    }
    guard("round edges", || {
        // Vertex of face `hi` at inner vertex `qi`.
        let mut verts: HashMap<(usize, usize), mt::Vertex> = HashMap::new();
        let mut vert = |hi: usize, qi: usize| -> mt::Vertex {
            verts.entry((hi, qi)).or_insert_with(|| builder::vertex(p3(q.get(qi).copied().unwrap_or_default() + normal(hi) * r))).clone()
        };
        let mut lines: HashMap<(usize, usize, usize), mt::Edge> = HashMap::new();
        let mut arcs: HashMap<(usize, usize, usize), mt::Edge> = HashMap::new();
        // Straight edge of face hi between inner vertices a and b (stored low → high).
        let mut line = |hi: usize, a: usize, bb: usize, vert: &mut dyn FnMut(usize, usize) -> mt::Vertex| -> mt::Edge {
            let (lo, hi_v) = (a.min(bb), a.max(bb));
            let e = lines.entry((hi, lo, hi_v)).or_insert_with(|| builder::line(&vert(hi, lo), &vert(hi, hi_v))).clone();
            if a < bb { e } else { e.inverse() }
        };
        // Arc at inner vertex qi from face f to face g (stored with f < g).
        let mut arc = |qi: usize, f: usize, g: usize, vert: &mut dyn FnMut(usize, usize) -> mt::Vertex| -> mt::Edge {
            let (lo, hi_f) = (f.min(g), f.max(g));
            let c = q.get(qi).copied().unwrap_or_default();
            let e = arcs
                .entry((qi, lo, hi_f))
                .or_insert_with(|| {
                    let mid = c + (normal(lo) + normal(hi_f)).normalized().unwrap_or(Vec3::Z) * r;
                    builder::circle_arc(&vert(lo, qi), &vert(hi_f, qi), p3(mid))
                })
                .clone();
            if f < g { e } else { e.inverse() }
        };
        let mut out: Vec<mt::Face> = Vec::new();
        // Planar faces.
        for (hi, lp) in &faces {
            let n = lp.len();
            let mut w: Vec<mt::Edge> = Vec::new();
            for k in 0..n {
                let (Some(&a), Some(&bb)) = (lp.get(k), lp.get((k + 1) % n)) else { continue };
                w.push(line(*hi, a, bb, &mut vert));
            }
            out.push(builder::try_attach_plane(&[w.into()]).map_err(|e| KernelError::Failed(format!("rounded face: {e}")))?);
        }
        use mt::{ParametricSurface3D, SearchNearestParameter};
        let orient = |mut surf: mt::Surface, at: Vec3, outward: Vec3| -> mt::Surface {
            if let Some((u, v)) = surf.search_nearest_parameter(p3(at), None, 100) {
                let nn = surf.normal(u, v);
                if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
                    surf = mt::Invertible::inverse(&surf);
                }
            }
            surf
        };
        // Edge cylinders: face f's loop runs a → b, so the cylinder (to f's right) runs b → a
        // along f, then across to g.
        for ((a, bb), fs) in &edge_faces {
            let (Some(&f0), Some(&f1)) = (fs.first(), fs.get(1)) else { continue };
            // Which face runs a → b?
            let runs = |hi: usize| {
                faces.iter().find(|(h, _)| *h == hi).is_some_and(|(_, lp)| {
                    let n = lp.len();
                    (0..n).any(|k| lp.get(k) == Some(a) && lp.get((k + 1) % n) == Some(bb))
                })
            };
            let (f, g) = if runs(f0) { (f0, f1) } else { (f1, f0) };
            let w: Vec<mt::Edge> =
                vec![line(f, *bb, *a, &mut vert), arc(*a, f, g, &mut vert), line(g, *a, *bb, &mut vert), arc(*bb, g, f, &mut vert)];
            let (qa, qb) = (q.get(*a).copied().unwrap_or_default(), q.get(*bb).copied().unwrap_or_default());
            let profile = arc(*a, f, g, &mut vert);
            let swept = builder::tsweep(&profile, v3(qb - qa));
            let out_dir = (normal(f) + normal(g)).normalized().unwrap_or(Vec3::Z);
            let mid = (qa + qb) * 0.5 + out_dir * r;
            let surf = orient(swept.oriented_surface(), mid, out_dir);
            out.push(mt::Face::try_new(vec![w.into()], surf).map_err(|e| KernelError::Failed(format!("rounded edge: {e}")))?);
        }
        // Corner sphere patches: the faces around each inner vertex, counter-clockwise.
        for (qi, c) in q.iter().enumerate() {
            let mut around: Vec<usize> = faces.iter().filter(|(_, lp)| lp.contains(&qi)).map(|(h, _)| *h).collect();
            let axis = around.iter().fold(Vec3::ZERO, |acc, h| acc + normal(*h)).normalized().unwrap_or(Vec3::Z);
            let u = axis.any_perp();
            let w2 = axis.cross(u);
            around.sort_by(|x, y| {
                let ang = |h: usize| normal(h).dot(w2).atan2(normal(h).dot(u));
                ang(*x).total_cmp(&ang(*y))
            });
            let k = around.len();
            let w: Vec<mt::Edge> = (0..k).filter_map(|i| Some(arc(qi, *around.get(i)?, *around.get((i + 1) % k)?, &mut vert))).collect();
            // A sphere surface whose poles and seam are away from this patch.
            let side = axis.any_perp();
            let (np, sp, back) = (*c + side * r, *c - side * r, *c - axis * r);
            let meridian = builder::circle_arc(&builder::vertex(p3(np)), &builder::vertex(p3(sp)), p3(back));
            let surf =
                mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(meridian.oriented_curve(), p3(*c), v3(side))));
            let surf = orient(surf, *c + axis * r, axis);
            out.push(mt::Face::try_new(vec![w.into()], surf).map_err(|e| KernelError::Failed(format!("rounded corner: {e}")))?);
        }
        let shell: mt::Shell = out.into();
        let solid = Solid::try_new(vec![shell]).map_err(|e| KernelError::Failed(format!("rounded body: {e}")))?;
        Body::new(solid)
    })
}

/// Shell of a body whose faces are all planar (convex or not): every face plane moves inward
/// by the thickness (open faces outward, past the body) keeping the body's topology, which
/// gives the cavity; the shell is the body minus the cavity.
fn shell_planar(b: &Body, open: &[Vec3], thickness: f64) -> Result<Body> {
    let healed = Body::new(crate::heal::heal(b.deep_copy(), b.size()))?;
    let size = healed.size();
    let mesh = healed.tessellate((size * 1e-3).max(1e-3))?;
    // Faces to open: the face of the triangle nearest each point.
    let mut opened: Vec<usize> = Vec::new();
    for p in open {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (point_tri(*p, x, y, z), *f as usize)))
            .min_by(|a, c| a.0.total_cmp(&c.0));
        if let Some((d, f)) = near
            && d < size * 1e-3 + 1e-6
            && !opened.contains(&f)
        {
            opened.push(f);
        }
    }
    if opened.is_empty() {
        return Err(KernelError::Invalid("select at least one face to remove".into()));
    }
    let margin = thickness + size * 0.05;
    let cavity = offset_planar(&healed, |fi, _| if opened.contains(&fi) { margin } else { -thickness })?;
    let r = crate::ops::boolean(&healed, &cavity, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("the shell removed everything".into()))?;
    Ok(r)
}

fn point_tri(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f64 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return f64::INFINITY };
    let h = (p - a).dot(nn);
    let q = p - nn * h;
    let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-12);
    if inside { h.abs() } else { [(a, b), (b, c), (c, a)].iter().map(|(u, v)| p.dist_to_segment(*u, *v)).fold(f64::INFINITY, f64::min) }
}

/// The body with every planar face moved along its outward normal by `shift(face index,
/// normal)`, keeping the topology: each vertex goes to where its faces' moved planes meet.
pub(crate) fn offset_planar(b: &Body, shift: impl Fn(usize, Vec3) -> f64) -> Result<Body> {
    crate::offset::offset_body(b, shift)
}

pub(crate) fn solve3_pub(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    solve3(m, r)
}

fn solve3(m: [[f64; 3]; 3], r: [f64; 3]) -> Option<[f64; 3]> {
    let det = |a: [[f64; 3]; 3]| {
        a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1]) - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
            + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0])
    };
    let d = det(m);
    if d.abs() < 1e-9 {
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

/// Move the planar faces at the given points along their outward normals by `distance`
/// (negative: into the body). The body's other faces follow (its topology is kept).
pub fn offset_faces(b: &Body, at: &[Vec3], distance: f64) -> Result<Body> {
    b.require_brep("offset faces")?;
    if !distance.is_finite() || distance.abs() > 1e6 {
        return Err(KernelError::Invalid("offset distance".into()));
    }
    let healed = Body::new(crate::heal::heal(b.deep_copy(), b.size()))?;
    let size = healed.size();
    let mesh = healed.tessellate((size * 1e-3).max(1e-3))?;
    let mut chosen: Vec<usize> = Vec::new();
    for p in at {
        let near = mesh
            .triangles
            .iter()
            .zip(&mesh.tri_face)
            .filter_map(|(t, f)| mesh.tri(t).map(|[x, y, z]| (point_tri(*p, x, y, z), *f as usize)))
            .min_by(|a, c| a.0.total_cmp(&c.0));
        match near {
            Some((d, f)) if d < size * 1e-3 + 1e-6 => {
                if !chosen.contains(&f) {
                    chosen.push(f);
                }
            }
            _ => return Err(KernelError::Invalid(format!("no face at {:?}", [p.x, p.y, p.z]))),
        }
    }
    offset_planar(&healed, |fi, _| if chosen.contains(&fi) { distance } else { 0.0 })
}
