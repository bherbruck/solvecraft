//! Edge fillets and chamfers as a local B-rep operation.
//!
//! Supported: a straight, convex edge between two planar faces whose two end vertices each
//! touch exactly one more planar face perpendicular to the edge (the common case for edges of
//! extruded and box-like parts). The two side faces are trimmed back, the end faces get an arc
//! (fillet) or a line (chamfer) at the corner, and a cylindrical (or planar) face is inserted.
//! The result is exact. Edges meeting at an already blended corner, curved edges and other
//! configurations are reported as not supported yet.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::{KernelError, Result, guard};

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Round,
    Flat,
}

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

fn vtx(v: &mt::Vertex) -> Vec3 {
    from_p3(v.point())
}

/// Outward unit normal of a planar face (from its oriented surface), if planar.
fn plane_normal(f: &mt::Face) -> Option<Vec3> {
    match f.oriented_surface() {
        mt::Surface::Plane(p) => {
            let n = p.normal();
            Vec3::new(n.x, n.y, n.z).normalized()
        }
        _ => None,
    }
}

/// Replace edges in a face's oriented boundary (by id, keeping orientation), and insert extra
/// edges where consecutive edges no longer connect.
fn rebuild_face(f: &mt::Face, subst: &HashMap<mt::EdgeID, mt::Edge>, connectors: &[mt::Edge]) -> Result<mt::Face> {
    let mut wires = Vec::new();
    for w in f.boundaries() {
        let mut edges: Vec<mt::Edge> = Vec::new();
        for e in w.edge_iter() {
            let ne = match subst.get(&e.id()) {
                Some(n) => {
                    // `n` was built in the orientation of the stored edge; match this use.
                    let stored_forward = e.front() == e.absolute_front();
                    if stored_forward { n.clone() } else { n.inverse() }
                }
                None => e.clone(),
            };
            edges.push(ne);
        }
        // Close gaps with connector edges.
        let n = edges.len();
        let mut out: Vec<mt::Edge> = Vec::new();
        for i in 0..n {
            let (Some(cur), Some(next)) = (edges.get(i), edges.get((i + 1) % n)) else { continue };
            out.push(cur.clone());
            if cur.back() != next.front() {
                let c = connectors
                    .iter()
                    .find_map(|c| {
                        if c.front() == cur.back() && c.back() == next.front() {
                            Some(c.clone())
                        } else if c.back() == cur.back() && c.front() == next.front() {
                            Some(c.inverse())
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| KernelError::Failed("blend: could not close a face boundary".into()))?;
                out.push(c);
            }
        }
        wires.push(mt::Wire::from(out));
    }
    let surface = f.oriented_surface();
    mt::Face::try_new(wires, surface).map_err(|e| KernelError::Failed(format!("blend: {e}")))
}

fn blend_one(solid: &Solid, edge: &mt::Edge, size: f64, r: f64, shape: Shape) -> Result<Solid> {
    if !matches!(edge.curve(), mt::Curve::Line(_)) {
        return Err(unsupported("only straight edges can be blended"));
    }
    let shells = solid.boundaries();
    let (si, shell) = shells
        .iter()
        .enumerate()
        .find(|(_, sh)| sh.edge_iter().any(|e| e.id() == edge.id()))
        .ok_or_else(|| KernelError::Failed("edge not found".into()))?;
    let faces: Vec<&mt::Face> = shell.face_iter().collect();
    let on_edge: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.edge_iter().any(|e| e.id() == edge.id()))).collect();
    let [i1, i2] = on_edge[..] else { return Err(unsupported("the edge is not between two faces")) };
    let (Some(f1), Some(f2)) = (faces.get(i1), faces.get(i2)) else { return Err(KernelError::Failed("faces".into())) };
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else { return Err(unsupported("both faces next to the edge must be planar")) };
    let (v0, v1) = (edge.absolute_front().clone(), edge.absolute_back().clone());
    let (p0, p1) = (vtx(&v0), vtx(&v1));
    let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
    // In-face directions perpendicular to the edge, pointing away from it.
    let mut t1 = n1.cross(d);
    if t1.dot(n2) > 0.0 {
        t1 = -t1;
    }
    let mut t2 = n2.cross(d);
    if t2.dot(n1) > 0.0 {
        t2 = -t2;
    }
    // Convex edge: the faces' outward normals point away from each other's interior.
    let cos_phi = t1.dot(t2).clamp(-1.0, 1.0);
    let phi = cos_phi.acos();
    if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) || n1.dot(t2) > 1e-9 || n2.dot(t1) > 1e-9 {
        return Err(unsupported("only convex edges can be blended"));
    }
    blend_geometry(solid, si, &faces, (i1, i2), edge, (v0, v1), (p0, p1, d), (n1, t1, t2, phi), size, r, shape)
}

/// Distance along each face from the edge to the blend boundary.
fn setback(r: f64, phi: f64, shape: Shape) -> f64 {
    match shape {
        Shape::Round => r / (phi / 2.0).tan(),
        Shape::Flat => r,
    }
}

#[allow(clippy::too_many_arguments)]
fn blend_geometry(
    solid: &Solid,
    si: usize,
    faces: &[&mt::Face],
    (i1, i2): (usize, usize),
    edge: &mt::Edge,
    (v0, v1): (mt::Vertex, mt::Vertex),
    (p0, p1, d): (Vec3, Vec3, Vec3),
    (n1, t1, t2, phi): (Vec3, Vec3, Vec3, f64),
    size: f64,
    r: f64,
    shape: Shape,
) -> Result<Solid> {
    let s = setback(r, phi, shape);
    let tol = (size * 1e-7).max(1e-9);
    // End faces and the side edges at each end.
    struct End {
        v: mt::Vertex,
        g: usize,
        e1: mt::Edge,
        e2: mt::Edge,
    }
    let mut ends: Vec<End> = Vec::new();
    for (v, p) in [(&v0, p0), (&v1, p1)] {
        let at_v: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.vertex_iter().any(|x| &x == v))).collect();
        let others: Vec<usize> = at_v.iter().copied().filter(|i| *i != i1 && *i != i2).collect();
        let [g] = others[..] else { return Err(unsupported("each end of the edge must touch exactly one more face")) };
        let gf = faces.get(g).ok_or_else(|| KernelError::Failed("face".into()))?;
        let ng = plane_normal(gf).ok_or_else(|| unsupported("the faces at the edge ends must be planar"))?;
        if ng.dot(d).abs() < 1.0 - 1e-9 {
            return Err(unsupported("the faces at the edge ends must be perpendicular to the edge"));
        }
        // Edges at v shared by g with f1 and with f2.
        let shared = |fi: usize| -> Result<mt::Edge> {
            let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
            f.edge_iter()
                .find(|e| e.id() != edge.id() && (e.front() == v || e.back() == v) && gf.edge_iter().any(|x| x.id() == e.id()))
                .ok_or_else(|| unsupported("unexpected corner topology"))
        };
        let (e1, e2) = (shared(i1)?, shared(i2)?);
        for (e, t) in [(&e1, t1), (&e2, t2)] {
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(unsupported("the edges at the corners must be straight"));
            }
            let other = if e.front() == v { vtx(e.back()) } else { vtx(e.front()) };
            let along = (other - p).dot(t);
            if (other - p - t * along).len() > tol * 100.0 + 1e-7 || along <= s + tol {
                return Err(unsupported("the blend is larger than the neighbouring faces"));
            }
        }
        ends.push(End { v: v.clone(), g, e1, e2 });
    }
    let [end0, end1] = &ends[..] else { return Err(KernelError::Failed("ends".into())) };
    if end0.g == end1.g {
        return Err(unsupported("both ends touch the same face"));
    }
    let a0 = builder::vertex(p3(p0 + t1 * s));
    let b0 = builder::vertex(p3(p0 + t2 * s));
    let a1 = builder::vertex(p3(p1 + t1 * s));
    let b1 = builder::vertex(p3(p1 + t2 * s));
    // Corner curves on the end faces (A → B) and the blend surface.
    let bisector = (t1 + t2).normalized().ok_or_else(|| KernelError::Failed("bisector".into()))?;
    let corner = |a: &mt::Vertex, b: &mt::Vertex, p: Vec3| -> mt::Edge {
        match shape {
            Shape::Round => {
                // Arc centre is r inside both faces; its mid point is on the bisector, toward the edge.
                let c = p + t1 * s - n1 * r;
                let mid = c - bisector * r;
                builder::circle_arc(a, b, p3(mid))
            }
            Shape::Flat => builder::line(a, b),
        }
    };
    let c0 = corner(&a0, &b0, p0);
    let c1 = corner(&a1, &b1, p1);
    let la = builder::line(&a0, &a1);
    let lb = builder::line(&b0, &b1);

    // Substitutions: side edges at the ends now stop at A/B; the blended edge moves to A or B.
    let mut subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
    for (end, a, b) in [(end0, &a0, &b0), (end1, &a1, &b1)] {
        for (e, nv) in [(&end.e1, a), (&end.e2, b)] {
            let (f, bk) = (e.absolute_front(), e.absolute_back());
            let ne = if f == &end.v { builder::line(nv, bk) } else { builder::line(f, nv) };
            subst.insert(e.id(), ne);
        }
    }
    let mut new_faces: Vec<mt::Face> = Vec::with_capacity(faces.len() + 1);
    for (i, f) in faces.iter().enumerate() {
        let nf = if i == i1 {
            let mut m = subst.clone();
            m.insert(edge.id(), la.clone());
            rebuild_face(f, &m, &[])?
        } else if i == i2 {
            let mut m = subst.clone();
            m.insert(edge.id(), lb.clone());
            rebuild_face(f, &m, &[])?
        } else if i == end0.g {
            rebuild_face(f, &subst, std::slice::from_ref(&c0))?
        } else if i == end1.g {
            rebuild_face(f, &subst, std::slice::from_ref(&c1))?
        } else {
            (*f).clone()
        };
        new_faces.push(nf);
    }
    // The blend face uses each new edge opposite to its neighbour.
    let f1_forward = faces.get(i1).is_some_and(|f| f.boundaries().iter().any(|w| w.edge_iter().any(|e| e.id() == edge.id() && e.front() == &v0)));
    let wire: mt::Wire = if f1_forward {
        vec![c0.clone(), lb.clone(), c1.inverse(), la.inverse()].into()
    } else {
        vec![la.clone(), c1.clone(), lb.inverse(), c0.inverse()].into()
    };
    let blend_face = match shape {
        Shape::Flat => builder::try_attach_plane(&[wire]).map_err(|e| KernelError::Failed(format!("chamfer face: {e}")))?,
        Shape::Round => {
            let swept: mt::Face = builder::tsweep(&c0, v3(p1 - p0));
            let mut surf = swept.oriented_surface();
            // Outward normal at the arc middle points away from the arc centre.
            let c = p0 + t1 * s - n1 * r;
            let mid = c - bisector * r;
            use mt::{ParametricSurface3D, SearchParameter};
            if let Some((u, v)) = surf.search_parameter(p3(mid), None, 100) {
                let nn = surf.normal(u, v);
                if Vec3::new(nn.x, nn.y, nn.z).dot(bisector) > 0.0 {
                    surf = mt::Invertible::inverse(&surf);
                }
            }
            mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("fillet face: {e}")))?
        }
    };
    new_faces.push(blend_face);
    let mut shells = solid.boundaries().clone();
    let new_shell: mt::Shell = new_faces.into();
    if let Some(sh) = shells.get_mut(si) {
        *sh = new_shell;
    }
    Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("blend produced an invalid solid: {e}")))
}

fn blend(body: &Body, edges: &[Vec3], r: f64, shape: Shape, what: &str) -> Result<Body> {
    if !(r.is_finite() && r > 1e-6 && r < 1e6) {
        return Err(KernelError::Invalid(format!("{what} size must be positive")));
    }
    if edges.is_empty() {
        return Err(KernelError::Invalid("no edges selected".into()));
    }
    if edges.len() > 1000 {
        return Err(KernelError::Invalid("too many edges".into()));
    }
    let mut cur = body.clone();
    for p in edges {
        let size = cur.size();
        let tol = (size * 2e-3).max(1e-3);
        let Some((idx, dist)) = cur.nearest_edge(*p, tol)? else { return Err(KernelError::Invalid("the body has no edges".into())) };
        if dist > size * 0.05 + 1e-3 {
            return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
        }
        let solid = cur.deep_copy();
        let edge = Body::unique_edges(&solid).into_iter().nth(idx).ok_or_else(|| KernelError::Invalid("edge index".into()))?;
        let out = guard(what, || blend_one(&solid, &edge, size, r, shape))?;
        cur = Body::new(out)?;
    }
    Ok(cur)
}

/// Constant-radius fillet of the edges nearest to the given points.
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    blend(body, edges, radius, Shape::Round, "fillet")
}

/// Equal-distance chamfer of the edges nearest to the given points. Each edge is cut with a
/// prism whose only face inside the body is the chamfer plane (so neighbouring chamfers meet in
/// a mitre); the local operation is the fallback.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    body.require_brep("chamfer")?;
    if !(distance.is_finite() && distance > 1e-6 && distance < 1e6) {
        return Err(KernelError::Invalid("chamfer size must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    // All cutters at once (their union mitres the corners), else one edge at a time.
    let tools: Result<Vec<Body>> = edges.iter().enumerate().map(|(i, p)| chamfer_tool(body, *p, distance, i)).collect();
    if let Ok(tools) = tools {
        let mut acc: Option<Body> = None;
        let mut ok = true;
        for t in tools {
            let next = match &acc {
                None => Some(t),
                Some(a) => crate::ops::boolean(a, &t, crate::BoolOp::Union).ok().flatten(),
            };
            if next.is_none() {
                ok = false;
                break;
            }
            acc = next;
        }
        if ok
            && let Some(tool) = acc
            && let Ok(Some(r)) = crate::ops::boolean(body, &tool, crate::BoolOp::Cut)
        {
            return Ok(r);
        }
    }
    let mut cur = body.clone();
    for p in edges {
        cur = match chamfer_tool(&cur, *p, distance, 0).and_then(|t| crate::ops::boolean(&cur, &t, crate::BoolOp::Cut)) {
            Ok(Some(b)) => b,
            _ => blend(&cur, std::slice::from_ref(p), distance, Shape::Flat, "chamfer")?,
        };
    }
    Ok(cur)
}

/// Geometry of a straight convex edge between two planar faces.
struct EdgeFrame {
    p0: Vec3,
    p1: Vec3,
    d: Vec3,
    n1: Vec3,
    n2: Vec3,
    t1: Vec3,
    t2: Vec3,
}

fn edge_frame(cur: &Body, p: Vec3) -> Result<EdgeFrame> {
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let Some((idx, dist)) = cur.nearest_edge(p, tol)? else { return Err(KernelError::Invalid("the body has no edges".into())) };
    if dist > size * 0.05 + 1e-3 {
        return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
    }
    let solid = &*cur.solid;
    let edge = Body::unique_edges(solid).into_iter().nth(idx).ok_or_else(|| KernelError::Invalid("edge index".into()))?;
    if !matches!(edge.curve(), mt::Curve::Line(_)) {
        return Err(unsupported("only straight edges"));
    }
    let faces: Vec<&mt::Face> = solid.face_iter().filter(|f| f.edge_iter().any(|e| e.id() == edge.id())).collect();
    let [f1, f2] = faces[..] else { return Err(unsupported("the edge is not between two faces")) };
    let (Some(n1), Some(n2)) = (plane_normal(f1), plane_normal(f2)) else { return Err(unsupported("both faces next to the edge must be planar")) };
    let (p0, p1) = (vtx(edge.absolute_front()), vtx(edge.absolute_back()));
    let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
    let mut t1 = n1.cross(d);
    if t1.dot(n2) > 0.0 {
        t1 = -t1;
    }
    let mut t2 = n2.cross(d);
    if t2.dot(n1) > 0.0 {
        t2 = -t2;
    }
    if n1.dot(t2) > 1e-9 || n2.dot(t1) > 1e-9 || t1.dot(t2) < -1.0 + 1e-6 {
        return Err(unsupported("only convex edges"));
    }
    Ok(EdgeFrame { p0, p1, d, n1, n2, t1, t2 })
}

/// The prism that cuts a chamfer of size `s` on the edge nearest `p`. `k` varies the parts that
/// stay outside the body so that neighbouring cutters don't share edges.
pub(crate) fn chamfer_tool(cur: &Body, p: Vec3, s: f64, k: usize) -> Result<Body> {
    let f = edge_frame(cur, p)?;
    let size = cur.size();
    let len = f.p0.dist(f.p1);
    let ext = (s * 2.0).max(size * 0.01) * (1.0 + 0.093 * (k % 5) as f64);
    // Cross-section at p0 - d*ext, in a plane with normal d.
    let base = f.p0 - f.d * ext;
    let plane = solvecraft_geom::Plane::from_normal(base, f.d).ok_or_else(|| KernelError::Failed("chamfer plane".into()))?;
    let a = base + f.t1 * s;
    let b = base + f.t2 * s;
    let ab = (a - b).normalized().ok_or_else(|| KernelError::Failed("chamfer".into()))?;
    let out = (f.n1 + f.n2).normalized().ok_or_else(|| KernelError::Failed("chamfer".into()))?;
    let vary = 1.0 + 0.137 * (k % 7) as f64;
    let reach = (s * 4.0 + size * 0.02) * vary;
    let a_ext = a + ab * (s * 0.5 + reach * 0.05);
    let b_ext = b - ab * (s * 0.5 + reach * 0.07);
    let e_far = base + out * reach;
    // The material beyond both ends must be empty (the cutter runs past them).
    let mid = (a + b + base) / 3.0;
    for probe in [mid + f.d * (ext * 0.5), mid + f.d * (len + ext * 1.5)] {
        if cur.contains(probe) {
            return Err(unsupported("chamfers ending at a concave corner"));
        }
    }
    let loc: Vec<solvecraft_geom::Vec2> = [a_ext, b_ext, e_far].iter().map(|q| plane.to_local(*q)).collect();
    let mut lp = solvecraft_geom::Loop2::polygon(&loc);
    if lp.signed_area() < 0.0 {
        lp = lp.reversed();
    }
    let region = solvecraft_geom::Region2 { outer: lp, holes: vec![] };
    crate::build::extrude(&plane, &[region], 0.0, len + 2.0 * ext)?.pop().ok_or_else(|| KernelError::Failed("chamfer tool".into()))
}
