//! Fillets with a radius per edge, and the corners where filleted edges meet.
//!
//! The edges are straight and convex, and every face at their end vertices is planar with three
//! edges meeting at each such vertex (box-like corners). The faces there are rebuilt around the
//! blends, faces away from them are kept. At a vertex:
//! - one filleted edge ends on the third face: the blend ends in that face's section of the
//!   cylinder (an exact arc or ellipse);
//! - two filleted edges, the third sharp: the cylinders trim each other (rolling ball); the
//!   crease runs from where their contact lines cross to the third edge (equal radii) or to the
//!   smaller blend's contact line on the face that ends the larger one;
//! - three filleted edges: each cylinder stops at a plane section through the points where its
//!   contact lines meet the others', and a corner patch spans the three sections (a sphere for
//!   equal radii, else a rational patch through the exact sections).
//!
//! A fillet that ends on an earlier round (of any radius) takes the round off
//! ([`crate::delete_faces`]) and blends both edges together.

use std::collections::{HashMap, HashSet};

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::blend::{plane_normal, unsupported, vtx};
use crate::body::{Body, Solid, p3, v3};
use crate::{KernelError, Result, guard};

/// Fillet each edge nearest a point with its own radius.
pub fn fillet_radii(body: &Body, edges: &[(Vec3, f64)]) -> Result<Body> {
    body.require_brep("fillet")?;
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    if edges.iter().any(|(p, r)| !(p.is_finite() && r.is_finite() && *r > 1e-6 && *r < 1e6)) {
        return Err(KernelError::Invalid("fillet size must be positive".into()));
    }
    let mut cur = Body::new(crate::heal::heal(body.deep_copy(), body.size()))?;
    let mut sel = edges.to_vec();
    let mut first: Option<KernelError> = None;
    // Earlier rounds at the edges' ends come off and are blended again with the new edges.
    for _ in 0..4 {
        let err = match build(&cur, &sel) {
            Ok(b) => return Ok(b),
            Err(e) => e,
        };
        let rounds = rounds_at(&cur, &sel);
        if rounds.is_empty() {
            return Err(first.unwrap_or(err));
        }
        first.get_or_insert(err);
        let idx: Vec<usize> = rounds.iter().map(|r| r.0).collect();
        cur = crate::delete_faces(&cur, &idx)?;
        sel.extend(rounds.iter().map(|r| (r.1, r.2)));
    }
    Err(first.unwrap_or_else(|| unsupported("these fillets")))
}

/// A face of the shell being rebuilt, or a new face.
#[derive(Clone, Copy, PartialEq)]
enum Owner {
    Face(usize),
    Cyl(mt::EdgeID),
    Patch,
}

/// A filleted edge.
struct Sel {
    r: f64,
    /// Setback: distance along each face from the edge to the contact line.
    s: f64,
    /// Middle weight of the cross-section arc, sin(α/2) for the faces' interior angle α.
    w: f64,
    fa: usize,
    fb: usize,
    na: Vec3,
    nb: Vec3,
    /// In-face directions away from the edge.
    ta: Vec3,
    tb: Vec3,
    p0: Vec3,
    d: Vec3,
    surf: mt::Surface,
}

impl Sel {
    fn t(&self, f: usize) -> Vec3 {
        if f == self.fa { self.ta } else { self.tb }
    }
    fn axis_point(&self) -> Vec3 {
        self.p0 + self.ta * self.s - self.na * self.r
    }
}

/// The edges at a vertex: id, the edge, unit direction away from the vertex.
struct Around {
    edges: Vec<(mt::EdgeID, Vec3)>,
}

fn h(p: Vec3, w: f64) -> mt::Vector4 {
    mt::Vector4::new(p.x * w, p.y * w, p.z * w, w)
}

/// Rational quadratic from `a` to `b` with middle control point `m` of weight `w`.
fn conic(a: Vec3, m: Vec3, b: Vec3, w: f64) -> mt::NurbsCurve<mt::Vector4> {
    mt::NurbsCurve::new(mt::BSplineCurve::new(mt::KnotVec::bezier_knot(2), vec![h(a, 1.0), h(m, w), h(b, 1.0)]))
}

fn edge(a: &mt::Vertex, b: &mt::Vertex, c: mt::Curve) -> Result<mt::Edge> {
    mt::Edge::try_new(a, b, c).map_err(|e| KernelError::Failed(format!("corner edge: {e}")))
}

/// Point where the contact line at setback `s` (in-face direction `t`) crosses the line from `v`
/// along `u`.
fn hit(v: Vec3, u: Vec3, t: Vec3, s: f64) -> Result<Vec3> {
    let k = u.dot(t);
    if k < 1e-6 {
        return Err(unsupported("a fillet that runs along its neighbouring edge"));
    }
    Ok(v + u * (s / k))
}

/// Point in a face where the contact lines of two edges from `v` (directions `u1`, `u2`, in-face
/// directions `t1`, `t2`, setbacks `s1`, `s2`) cross.
fn meet(v: Vec3, (u1, t1, s1): (Vec3, Vec3, f64), (u2, t2, s2): (Vec3, Vec3, f64)) -> Result<Vec3> {
    let (k1, k2) = (u1.dot(t2), u2.dot(t1));
    if k1 < 1e-6 || k2 < 1e-6 {
        return Err(unsupported("fillets of edges that meet at a reflex corner"));
    }
    Ok(v + u1 * (s2 / k1) + u2 * (s1 / k2))
}

/// Intersection of the three planes n·x = n·p.
fn planes3(a: (Vec3, Vec3), b: (Vec3, Vec3), c: (Vec3, Vec3)) -> Result<Vec3> {
    let row = |n: Vec3| [n.x, n.y, n.z];
    crate::polyhedron::solve3_pub([row(a.0), row(b.0), row(c.0)], [a.0.dot(a.1), b.0.dot(b.1), c.0.dot(c.1)])
        .map(|x| Vec3::new(x[0], x[1], x[2]))
        .ok_or_else(|| unsupported("a corner whose planes do not meet in a point"))
}

/// Flip a surface so that its normal near `at` points along `outward`.
fn orient(mut surf: mt::Surface, at: Vec3, outward: Vec3) -> mt::Surface {
    use mt::{ParametricSurface3D, SearchNearestParameter};
    if let Some((u, v)) = surf.search_nearest_parameter(p3(at), None, 100) {
        let n = surf.normal(u, v);
        if Vec3::new(n.x, n.y, n.z).dot(outward) < 0.0 {
            surf = mt::Invertible::inverse(&surf);
        }
    }
    surf
}

/// Edges from `from` to `to` through the pool edges of `owner`.
fn connect(from: &mt::Vertex, to: &mt::Vertex, pool: &[(mt::Edge, Vec<Owner>)], owner: Owner) -> Result<Vec<mt::Edge>> {
    let mut out = Vec::new();
    let mut used: HashSet<mt::EdgeID> = HashSet::new();
    let mut cur = from.clone();
    for _ in 0..=pool.len() {
        if &cur == to {
            return Ok(out);
        }
        let next = pool.iter().filter(|(e, o)| o.contains(&owner) && !used.contains(&e.id())).find_map(|(e, _)| {
            if e.front() == &cur {
                Some(e.clone())
            } else if e.back() == &cur {
                Some(e.inverse())
            } else {
                None
            }
        });
        let Some(e) = next else { break };
        used.insert(e.id());
        cur = e.back().clone();
        out.push(e);
    }
    Err(KernelError::Failed("fillet corner: a boundary does not close".into()))
}

fn build(cur: &Body, sel_pts: &[(Vec3, f64)]) -> Result<Body> {
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let solid = cur.deep_copy();
    let all = Body::unique_edges(&solid);
    let mut radius: HashMap<mt::EdgeID, f64> = HashMap::new();
    let mut order: Vec<mt::EdgeID> = Vec::new();
    for (p, r) in sel_pts {
        let Some((idx, dist)) = cur.nearest_edge(*p, tol)? else { return Err(KernelError::Invalid("the body has no edges".into())) };
        if dist > size * 0.05 + 1e-3 {
            return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
        }
        let e = all.get(idx).ok_or_else(|| KernelError::Invalid("edge index".into()))?;
        if radius.insert(e.id(), *r).is_none() {
            order.push(e.id());
        }
    }
    let shells = solid.boundaries();
    let Some(first) = order.first() else { return Err(KernelError::Invalid("no edges selected".into())) };
    let si = shells.iter().position(|sh| sh.edge_iter().any(|e| e.id() == *first)).ok_or_else(|| KernelError::Failed("edge not found".into()))?;
    let shell = shells.get(si).ok_or_else(|| KernelError::Failed("shell".into()))?;
    if order.iter().any(|id| !shell.edge_iter().any(|e| e.id() == *id)) {
        return Err(unsupported("fillets on separate lumps in one go"));
    }
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    let edges: HashMap<mt::EdgeID, mt::Edge> = shell.edge_iter().map(|e| (e.id(), e)).collect();
    // Faces on each edge, and the faces and edges at each vertex.
    let mut on_edge: HashMap<mt::EdgeID, Vec<usize>> = HashMap::new();
    let mut at_vertex: HashMap<mt::VertexID, (HashSet<mt::EdgeID>, Vec<usize>)> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for e in f.edge_iter() {
            let ids = on_edge.entry(e.id()).or_default();
            if !ids.contains(&fi) {
                ids.push(fi);
            }
            for v in [e.front(), e.back()] {
                let a = at_vertex.entry(v.id()).or_default();
                a.0.insert(e.id());
                if !a.1.contains(&fi) {
                    a.1.push(fi);
                }
            }
        }
    }
    let planes: Vec<Option<Vec3>> = faces.iter().map(plane_normal).collect();
    let normal = |fi: usize| planes.get(fi).copied().flatten();

    // The filleted edges.
    let mut sels: HashMap<mt::EdgeID, Sel> = HashMap::new();
    let mut touched: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
    for id in &order {
        let e = edges.get(id).ok_or_else(|| KernelError::Failed("edge".into()))?;
        if !matches!(e.curve(), mt::Curve::Line(_)) {
            return Err(unsupported("only straight edges"));
        }
        let fs = on_edge.get(id).cloned().unwrap_or_default();
        let [fa, fb] = fs[..] else { return Err(unsupported("the edge is not between two faces")) };
        let (Some(na), Some(nb)) = (normal(fa), normal(fb)) else { return Err(unsupported("both faces next to the edge must be planar")) };
        let (v0, v1) = (e.absolute_front().clone(), e.absolute_back().clone());
        let (p0, p1) = (vtx(&v0), vtx(&v1));
        let d = (p1 - p0).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
        let ta = (nb - na * nb.dot(na)) * -1.0;
        let tb = (na - nb * na.dot(nb)) * -1.0;
        let (Some(ta), Some(tb)) = (ta.normalized(), tb.normalized()) else { return Err(unsupported("the faces at the edge are tangent")) };
        // Convex: each face lies away from the other's outward side (its loop's left side).
        let inward = |fi: usize, n: Vec3| -> Option<Vec3> {
            let fe = faces.get(fi)?.boundaries().into_iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).find(|x| x.id() == *id)?;
            n.cross(vtx(fe.back()) - vtx(fe.front())).normalized()
        };
        let (Some(ia), Some(ib)) = (inward(fa, na), inward(fb, nb)) else { return Err(KernelError::Failed("fillet: edge direction".into())) };
        if ia.dot(ta) < 1e-6 || ib.dot(tb) < 1e-6 {
            return Err(unsupported("fillets of concave edges with other radii"));
        }
        let alpha = ta.dot(tb).clamp(-1.0, 1.0).acos();
        if !(alpha > 1e-3 && alpha < std::f64::consts::PI - 1e-3) {
            return Err(unsupported("the faces at the edge are tangent"));
        }
        let r = radius.get(id).copied().unwrap_or(1.0);
        let s = r / (alpha / 2.0).tan();
        let w = (alpha / 2.0).sin();
        // The cylinder, well past both ends (sections on oblique end faces reach beyond them).
        let m = 4.0 * (s + r);
        let (q0, q1) = (p0 - d * m, p1 + d * m);
        let ctrl = vec![vec![h(q0 + ta * s, 1.0), h(q1 + ta * s, 1.0)], vec![h(q0, w), h(q1, w)], vec![h(q0 + tb * s, 1.0), h(q1 + tb * s, 1.0)]];
        let surf = mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new(
            (mt::KnotVec::bezier_knot(2), mt::KnotVec::bezier_knot(1)),
            ctrl,
        )));
        let mid = (p0 + p1) * 0.5;
        let axis = mid + ta * s - na * r;
        let out = ((ta + tb) * -1.0).normalized().unwrap_or(na);
        let surf = orient(surf, axis + (mid - axis).normalized().unwrap_or(out) * r, mid - axis);
        touched.insert(v0.id(), v0);
        touched.insert(v1.id(), v1);
        sels.insert(*id, Sel { r, s, w, fa, fb, na, nb, ta, tb, p0, d, surf });
    }

    // The vertices at the ends of filleted edges: three planar faces, three straight edges.
    let mut around: HashMap<mt::VertexID, Around> = HashMap::new();
    for (vid, v) in &touched {
        let (eids, fids) = at_vertex.get(vid).ok_or_else(|| KernelError::Failed("vertex".into()))?;
        if eids.len() != 3 || fids.len() != 3 {
            return Err(unsupported("fillets ending where more than three edges meet"));
        }
        if fids.iter().any(|f| normal(*f).is_none()) {
            return Err(unsupported("fillets ending on curved faces"));
        }
        let pv = vtx(v);
        let mut list = Vec::new();
        for id in eids {
            let e = edges.get(id).ok_or_else(|| KernelError::Failed("edge".into()))?;
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(unsupported("fillets ending next to curved edges"));
            }
            let other = if e.absolute_front() == v { e.absolute_back() } else { e.absolute_front() };
            let u = (vtx(other) - pv).normalized().ok_or_else(|| KernelError::Failed("zero-length edge".into()))?;
            list.push((*id, u));
        }
        around.insert(*vid, Around { edges: list });
    }

    guard("fillet corners", || {
        // New end points: (edge, vertex, face for a filleted edge's contact line).
        let mut endpt: HashMap<(mt::EdgeID, mt::VertexID, Option<usize>), mt::Vertex> = HashMap::new();
        let mut pool: HashMap<mt::VertexID, Vec<(mt::Edge, Vec<Owner>)>> = HashMap::new();
        let mut patches: Vec<mt::Face> = Vec::new();
        let face_with = |a: mt::EdgeID, b: mt::EdgeID| -> Result<usize> {
            let fa = on_edge.get(&a).cloned().unwrap_or_default();
            let fb = on_edge.get(&b).cloned().unwrap_or_default();
            fa.into_iter().find(|f| fb.contains(f)).ok_or_else(|| KernelError::Failed("fillet corner: faces".into()))
        };
        for (vid, ar) in &around {
            let v = vtx(touched.get(vid).ok_or_else(|| KernelError::Failed("vertex".into()))?);
            let mut chosen: Vec<(mt::EdgeID, Vec3)> = ar.edges.iter().filter(|(id, _)| sels.contains_key(id)).copied().collect();
            let rest: Vec<(mt::EdgeID, Vec3)> = ar.edges.iter().filter(|(id, _)| !sels.contains_key(id)).copied().collect();
            chosen.sort_by_key(|(id, _)| order.iter().position(|x| x == id));
            let sel = |id: mt::EdgeID| sels.get(&id).ok_or_else(|| KernelError::Failed("fillet".into()));
            let list = pool.entry(*vid).or_default();
            match (&chosen[..], &rest[..]) {
                ([(e1, _)], [(e2, u2), (e3, u3)]) => {
                    let s1 = sel(*e1)?;
                    let (f12, f31) = (face_with(*e1, *e2)?, face_with(*e1, *e3)?);
                    let f23 = face_with(*e2, *e3)?;
                    let p2 = hit(v, *u2, s1.t(f12), s1.s)?;
                    let p3_ = hit(v, *u3, s1.t(f31), s1.s)?;
                    let (a, b) = (builder::vertex(p3(p2)), builder::vertex(p3(p3_)));
                    endpt.insert((*e2, *vid, None), a.clone());
                    endpt.insert((*e1, *vid, Some(f12)), a.clone());
                    endpt.insert((*e3, *vid, None), b.clone());
                    endpt.insert((*e1, *vid, Some(f31)), b.clone());
                    let c = edge(&a, &b, mt::Curve::NurbsCurve(conic(p2, v, p3_, s1.w)))?;
                    list.push((c, vec![Owner::Face(f23), Owner::Cyl(*e1)]));
                }
                ([(ea, ua), (eb, ub)], [(e3, u3)]) => {
                    let (sa, sb) = (sel(*ea)?, sel(*eb)?);
                    let fab = face_with(*ea, *eb)?;
                    let (fb3, fa3) = (face_with(*eb, *e3)?, face_with(*ea, *e3)?);
                    let q = meet(v, (*ua, sa.t(fab), sa.s), (*ub, sb.t(fab), sb.s))?;
                    // Where each cylinder touches the third edge.
                    let xb = hit(v, *u3, sb.t(fb3), sb.s)?;
                    let xa = hit(v, *u3, sa.t(fa3), sa.s)?;
                    let qv = builder::vertex(p3(q));
                    endpt.insert((*ea, *vid, Some(fab)), qv.clone());
                    endpt.insert((*eb, *vid, Some(fab)), qv.clone());
                    let (la, lb) = ((xa - v).len(), (xb - v).len());
                    let crease_to = |end: Vec3| -> Result<mt::Curve> { crease(sa, sb, q, end, size) };
                    if (la - lb).abs() <= size * 1e-9 {
                        let xv = builder::vertex(p3(xb));
                        endpt.insert((*e3, *vid, None), xv.clone());
                        endpt.insert((*ea, *vid, Some(fa3)), xv.clone());
                        endpt.insert((*eb, *vid, Some(fb3)), xv.clone());
                        let c = edge(&qv, &xv, crease_to(xb)?)?;
                        list.push((c, vec![Owner::Cyl(*ea), Owner::Cyl(*eb)]));
                    } else {
                        // The larger blend (reaching further down the third edge) ends on the
                        // face it shares with the smaller one's other side.
                        let (big, small, us, fbig3, fsmall3, eb_, es, x) =
                            if lb > la { (sb, sa, *ua, fb3, fa3, *eb, *ea, xb) } else { (sa, sb, *ub, fa3, fb3, *ea, *eb, xa) };
                        // The big cylinder's section by the small one's end face: from the third
                        // edge to the small edge's line, where it crosses the small contact line.
                        let z = hit(v, us, big.t(fab), big.s)?;
                        let mut sec = conic(x, v, z, big.w);
                        let tt = small.t(fsmall3);
                        let c0 = (x - v).dot(tt) - small.s;
                        let c1 = big.w * (0.0 - small.s);
                        let c2 = (z - v).dot(tt) - small.s;
                        let t = bernstein_root(c0, c1, c2).ok_or_else(|| unsupported("fillets this different in size at one corner"))?;
                        let y = {
                            use mt::ParametricCurve;
                            crate::body::from_p3(sec.subs(t))
                        };
                        {
                            use mt::Cut;
                            let _ = sec.cut(t);
                        }
                        let xv = builder::vertex(p3(x));
                        let yv = builder::vertex(p3(y));
                        endpt.insert((*e3, *vid, None), xv.clone());
                        endpt.insert((eb_, *vid, Some(fbig3)), xv.clone());
                        endpt.insert((es, *vid, Some(fsmall3)), yv.clone());
                        let piece = edge(&xv, &yv, mt::Curve::NurbsCurve(sec))?;
                        list.push((piece, vec![Owner::Face(fsmall3), Owner::Cyl(eb_)]));
                        let c = edge(&qv, &yv, crease_to(y)?)?;
                        list.push((c, vec![Owner::Cyl(*ea), Owner::Cyl(*eb)]));
                    }
                }
                ([(e1, u1), (e2, u2), (e3, u3)], []) => {
                    let (s1, s2, s3) = (sel(*e1)?, sel(*e2)?, sel(*e3)?);
                    let (f12, f23, f31) = (face_with(*e1, *e2)?, face_with(*e2, *e3)?, face_with(*e3, *e1)?);
                    let q12 = meet(v, (*u1, s1.t(f12), s1.s), (*u2, s2.t(f12), s2.s))?;
                    let q23 = meet(v, (*u2, s2.t(f23), s2.s), (*u3, s3.t(f23), s3.s))?;
                    let q31 = meet(v, (*u3, s3.t(f31), s3.s), (*u1, s1.t(f31), s1.s))?;
                    let (v12, v23, v31) = (builder::vertex(p3(q12)), builder::vertex(p3(q23)), builder::vertex(p3(q31)));
                    for (e, f, x) in [(e1, f12, &v12), (e2, f12, &v12), (e2, f23, &v23), (e3, f23, &v23), (e3, f31, &v31), (e1, f31, &v31)] {
                        endpt.insert((*e, *vid, Some(f)), x.clone());
                    }
                    let out = (normal(f12).unwrap_or(Vec3::Z) + normal(f23).unwrap_or(Vec3::Z) + normal(f31).unwrap_or(Vec3::Z))
                        .normalized()
                        .unwrap_or(Vec3::Z);
                    let n = |f: usize| normal(f).unwrap_or(Vec3::Z);
                    let cp = corner_patch([s1, s2, s3], [f12, f23, f31], [q12, q23, q31], [n(f12), n(f23), n(f31)], out, size)?;
                    let [t1, t2, t3] = cp.sections;
                    let c1 = edge(&v12, &v31, mt::Curve::NurbsCurve(t1))?;
                    let c2 = edge(&v12, &v23, mt::Curve::NurbsCurve(t2))?;
                    let c3 = edge(&v23, &v31, mt::Curve::NurbsCurve(t3))?;
                    let surf = cp.surf;
                    let wire: mt::Wire = vec![c2.clone(), c3.clone(), c1.inverse()].into();
                    patches.push(mt::Face::try_new(vec![wire], surf).map_err(|e| KernelError::Failed(format!("corner patch: {e}")))?);
                    list.push((c1, vec![Owner::Cyl(*e1), Owner::Patch]));
                    list.push((c2, vec![Owner::Cyl(*e2), Owner::Patch]));
                    list.push((c3, vec![Owner::Cyl(*e3), Owner::Patch]));
                }
                _ => return Err(unsupported("this corner")),
            }
        }

        // Rebuilt edges: contact lines per (edge, face), shortened edges per edge.
        let end_of =
            |id: mt::EdgeID, v: &mt::Vertex, f: Option<usize>| -> mt::Vertex { endpt.get(&(id, v.id(), f)).cloned().unwrap_or_else(|| v.clone()) };
        let mut lines: HashMap<(mt::EdgeID, Option<usize>), mt::Edge> = HashMap::new();
        let mut line_of = |e: &mt::Edge, f: Option<usize>| -> Result<mt::Edge> {
            if let Some(x) = lines.get(&(e.id(), f)) {
                return Ok(x.clone());
            }
            let (a, b) = (end_of(e.id(), e.absolute_front(), f), end_of(e.id(), e.absolute_back(), f));
            let (pa, pb) = (vtx(&a), vtx(&b));
            let orig = vtx(e.absolute_back()) - vtx(e.absolute_front());
            if (pb - pa).dot(orig) <= size * 1e-9 * orig.len() {
                return Err(unsupported("a fillet larger than the faces next to it"));
            }
            let x = builder::line(&a, &b);
            lines.insert((e.id(), f), x.clone());
            Ok(x)
        };
        let moved = |e: &mt::Edge| touched.contains_key(&e.absolute_front().id()) || touched.contains_key(&e.absolute_back().id());
        let mut new_faces: Vec<mt::Face> = Vec::with_capacity(faces.len() + sels.len() + patches.len());
        for (fi, f) in faces.iter().enumerate() {
            if !f.vertex_iter().any(|v| touched.contains_key(&v.id())) {
                new_faces.push(f.clone());
                continue;
            }
            let mut wires = Vec::new();
            for w in f.boundaries() {
                let olds: Vec<mt::Edge> = w.edge_iter().cloned().collect();
                let mut news = Vec::with_capacity(olds.len());
                for e in &olds {
                    let ne = if sels.contains_key(&e.id()) {
                        line_of(e, Some(fi))?
                    } else if moved(e) {
                        line_of(e, None)?
                    } else {
                        news.push(e.clone());
                        continue;
                    };
                    news.push(if e.front() == e.absolute_front() { ne } else { ne.inverse() });
                }
                let n = news.len();
                let mut out = Vec::new();
                for i in 0..n {
                    let (Some(a), Some(b), Some(old)) = (news.get(i), news.get((i + 1) % n), olds.get(i)) else { continue };
                    out.push(a.clone());
                    if a.back() != b.front() {
                        let p = pool.get(&old.back().id()).map(|x| &x[..]).unwrap_or(&[]);
                        out.extend(connect(a.back(), b.front(), p, Owner::Face(fi))?);
                    }
                }
                wires.push(mt::Wire::from(out));
            }
            new_faces.push(mt::Face::try_new(wires, f.oriented_surface()).map_err(|e| KernelError::Failed(format!("fillet: face: {e}")))?);
        }
        // The blends: back along the first face's contact line, round the corner, along the
        // second's, round the other corner.
        for id in &order {
            let (Some(s), Some(e)) = (sels.get(id), edges.get(id)) else { continue };
            let (va, vb) = (e.absolute_front(), e.absolute_back());
            let a_forward = faces.get(s.fa).is_some_and(|f| f.boundaries().iter().any(|w| w.edge_iter().any(|x| x.id() == *id && x.front() == va)));
            let la = line_of(e, Some(s.fa))?;
            let lb = line_of(e, Some(s.fb))?;
            let (first, second, c1, c2) = if a_forward { (la.inverse(), lb, va, vb) } else { (lb.inverse(), la, va, vb) };
            let p1 = pool.get(&c1.id()).map(|x| &x[..]).unwrap_or(&[]);
            let p2 = pool.get(&c2.id()).map(|x| &x[..]).unwrap_or(&[]);
            let mut w = vec![first.clone()];
            w.extend(connect(first.back(), second.front(), p1, Owner::Cyl(*id))?);
            w.push(second.clone());
            w.extend(connect(second.back(), first.front(), p2, Owner::Cyl(*id))?);
            new_faces.push(mt::Face::try_new(vec![w.into()], s.surf.clone()).map_err(|e| KernelError::Failed(format!("fillet face: {e}")))?);
        }
        // Corner patches run each section opposite to its blend.
        for mut p in patches {
            let uses = |f: &mt::Face, id: mt::EdgeID| {
                f.boundaries().iter().flat_map(|w| w.edge_iter().cloned().collect::<Vec<_>>()).find(|x| x.id() == id).map(|x| x.front().id())
            };
            let flip = p
                .boundaries()
                .first()
                .and_then(|w| w.edge_iter().next())
                .and_then(|pe| new_faces.iter().find_map(|f| uses(f, pe.id())).map(|front| front == pe.front().id()));
            if flip == Some(true) {
                let surf = p.oriented_surface();
                let wires: Vec<mt::Wire> = p.boundaries().iter().map(|w| w.inverse()).collect();
                p = mt::Face::try_new(wires, surf).map_err(|e| KernelError::Failed(format!("corner patch: {e}")))?;
            }
            new_faces.push(p);
        }
        let mut shells = solid.boundaries().clone();
        if let Some(sh) = shells.get_mut(si) {
            *sh = new_faces.into();
        }
        let out = Body::new(Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("fillet produced an invalid solid: {e}")))?)?;
        Ok(out)
    })
}

/// The root in (0, 1) of the quadratic with Bernstein coefficients c0, c1, c2.
fn bernstein_root(c0: f64, c1: f64, c2: f64) -> Option<f64> {
    let (a, b, c) = (c0 - 2.0 * c1 + c2, 2.0 * (c1 - c0), c0);
    let roots: Vec<f64> = if a.abs() < 1e-14 {
        if b.abs() < 1e-14 { vec![] } else { vec![-c / b] }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc < 0.0 {
            vec![]
        } else {
            let sq = disc.sqrt();
            vec![(-b - sq) / (2.0 * a), (-b + sq) / (2.0 * a)]
        }
    };
    roots.into_iter().filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9).min_by(|x, y| x.total_cmp(y))
}

/// The crease where the blends of two edges cross, from `q` (their contact lines' crossing) to
/// `end`: rulings of the first cylinder cut by the second, through a cubic.
fn crease(a: &Sel, b: &Sel, q: Vec3, end: Vec3, size: f64) -> Result<mt::Curve> {
    let (ca, da) = (a.axis_point(), a.d);
    let (cb, db) = (b.axis_point(), b.d);
    let g = a.na;
    let hh = (a.nb - g * a.nb.dot(g)).normalized().ok_or_else(|| unsupported("the faces at the edge are tangent"))?;
    let angle = |p: Vec3| {
        let x = p - ca;
        x.dot(hh).atan2(x.dot(g))
    };
    let (th0, th1) = (angle(q), angle(end));
    let (l0, l1) = ((q - ca).dot(da), (end - ca).dot(da));
    let dd = da - db * da.dot(db);
    let k2 = dd.dot(dd);
    if k2 < 1e-12 {
        return Err(unsupported("fillets of parallel edges meeting"));
    }
    let n = 48;
    let mut pts = vec![p3(q)];
    for i in 1..n {
        let f = i as f64 / n as f64;
        let th = th0 + (th1 - th0) * f;
        let target = l0 + (l1 - l0) * f;
        let base = ca + (g * th.cos() + hh * th.sin()) * a.r;
        let b0 = (base - cb) - db * (base - cb).dot(db);
        let (k1, k0) = (b0.dot(dd), b0.dot(b0) - b.r * b.r);
        let disc = (k1 * k1 - k2 * k0).max(0.0);
        let sq = disc.sqrt();
        let (r1, r2) = ((-k1 - sq) / k2, (-k1 + sq) / k2);
        let lam = if (r1 - target).abs() < (r2 - target).abs() { r1 } else { r2 };
        let p = base + da * lam;
        // Still on both cylinders.
        let off = (p - cb) - db * (p - cb).dot(db);
        if (off.len() - b.r).abs() > size * 1e-6 + b.r * 1e-6 {
            return Err(unsupported("fillets this different in size at one corner"));
        }
        pts.push(p3(p));
    }
    pts.push(p3(end));
    crate::splitface::fit(&pts, size)
}

/// Faces at the filleted edges' ends that are rounds of a straight edge between planar faces:
/// (face index, a point on the sharp edge it replaced, its radius).
fn rounds_at(cur: &Body, sel: &[(Vec3, f64)]) -> Vec<(usize, Vec3, f64)> {
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let solid = &*cur.solid;
    let all = Body::unique_edges(solid);
    let mut ends: Vec<Vec3> = Vec::new();
    for (p, _) in sel {
        if let Ok(Some((idx, _))) = cur.nearest_edge(*p, tol)
            && let Some(e) = all.get(idx)
        {
            ends.push(vtx(e.absolute_front()));
            ends.push(vtx(e.absolute_back()));
        }
    }
    let faces: Vec<&mt::Face> = solid.face_iter().collect();
    let mut out = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
        if plane_normal(f).is_some() || !f.vertex_iter().any(|v| ends.iter().any(|p| p.dist(vtx(&v)) <= size * 1e-9)) {
            continue;
        }
        if let Some((p, r)) = round_of(&faces, f, size) {
            out.push((fi, p, r));
        }
    }
    out
}

/// A round between two planes: two parallel straight boundary edges on planar faces, the
/// surface a cylinder tangent to both. The sharp edge's midpoint and the radius.
fn round_of(faces: &[&mt::Face], f: &mt::Face, size: f64) -> Option<(Vec3, f64)> {
    use mt::{ParametricSurface, SearchNearestParameter};
    let mut lines: Vec<(Vec3, Vec3, Vec3)> = Vec::new();
    for e in f.edge_iter() {
        if !matches!(e.curve(), mt::Curve::Line(_)) {
            continue;
        }
        let other = faces.iter().find(|g| !std::ptr::eq(**g, f) && g.edge_iter().any(|x| x.id() == e.id()))?;
        let n = plane_normal(other)?;
        lines.push((vtx(e.absolute_front()), vtx(e.absolute_back()), n));
    }
    for (i, (a0, a1, na)) in lines.iter().enumerate() {
        for (b0, b1, nb) in lines.iter().skip(i + 1) {
            let d = (*a1 - *a0).normalized()?;
            if (*b1 - *b0).normalized()?.cross(d).len() > 1e-9 || na.cross(*nb).len() < 1e-6 {
                continue;
            }
            // The cross-section through a's midpoint.
            let a = (*a0 + *a1) * 0.5;
            let b = *b0 + d * (a - *b0).dot(d);
            let dn = *nb - *na;
            let r = (b - a).len() / dn.len();
            if (b - a - dn * r).len() > size * 1e-6 {
                continue;
            }
            // The surface is the cylinder about a − r·na.
            let c = a - *na * r;
            let mid = c + (*na + *nb).normalized()? * r;
            let surf = f.surface();
            let (u, v) = surf.search_nearest_parameter(p3(mid), None, 100)?;
            if crate::body::from_p3(surf.subs(u, v)).dist(mid) > size * 1e-6 {
                continue;
            }
            let p = planes3((*na, a), (*nb, b), (d, a)).ok()?;
            return Some((p, r));
        }
    }
    None
}

struct CornerPatch {
    /// Sections of the first blend (q12 → q31), the second (q12 → q23), the third (q23 → q31).
    sections: [mt::NurbsCurve<mt::Vector4>; 3],
    surf: mt::Surface,
}

/// Plane section of a blend's cylinder from `a` to `b` (points on its contact lines): the plane
/// holds the chord and the faces' bisector turned by `psi` about the chord. Its middle control
/// point and the curve.
fn section(s: &Sel, a: Vec3, b: Vec3, psi: f64) -> Result<(Vec3, mt::NurbsCurve<mt::Vector4>)> {
    let c = (b - a).normalized().ok_or_else(|| unsupported("a corner this small"))?;
    let bis = s.na + s.nb;
    let bis = (bis - c * bis.dot(c)).normalized().ok_or_else(|| unsupported("the faces at the edge are tangent"))?;
    let dir = bis * psi.cos() + c.cross(bis) * psi.sin();
    let cut_n = c.cross(dir).normalized().ok_or_else(|| unsupported("a corner this small"))?;
    let m = planes3((s.na, s.p0), (s.nb, s.p0), (cut_n, a))?;
    if m.dist(a) > 10.0 * (s.s + s.r) {
        return Err(unsupported("a corner this skewed"));
    }
    Ok((m, conic(a, m, b, s.w)))
}

/// The corner where three blends meet. Equal radii: a sphere through cross-sections of the
/// cylinders. Else the sections are turned so that the blends' curvatures agree where two of
/// them meet on a face (else no smooth patch can meet both there), and a rational patch of
/// degree 4 through them, closed at q31, is fitted tangent to the three cylinders and to the
/// face at q31.
fn corner_patch(s: [&Sel; 3], f: [usize; 3], q: [Vec3; 3], n: [Vec3; 3], out: Vec3, size: f64) -> Result<CornerPatch> {
    let [s1, s2, s3] = s;
    let [f12, f23, _] = f;
    let [q12, q23, q31] = q;
    let [n12, n23, n31] = n;
    let equal = (s1.r - s2.r).abs() <= size * 1e-9 && (s1.r - s3.r).abs() <= size * 1e-9;
    let mk = |psi: [f64; 3]| -> Result<[(Vec3, mt::NurbsCurve<mt::Vector4>); 3]> {
        Ok([section(s1, q12, q31, psi[0])?, section(s2, q12, q23, psi[1])?, section(s3, q23, q31, psi[2])?])
    };
    if equal {
        // A sphere about the point a radius inside all three faces, its poles and seam away
        // from the patch.
        let [(_, t1), (_, t2), (_, t3)] = mk([0.0; 3])?;
        let r = s1.r;
        let c = planes3((n12, q12 - n12 * r), (n23, q23 - n23 * r), (n31, q31 - n31 * r))?;
        let side = out.any_perp();
        let (np, sp, back) = (c + side * r, c - side * r, c - out * r);
        let meridian = builder::circle_arc(&builder::vertex(p3(np)), &builder::vertex(p3(sp)), p3(back));
        let surf = mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(meridian.oriented_curve(), p3(c), v3(side))));
        return Ok(CornerPatch { sections: [t1, t2, t3], surf: orient(surf, c + out * r, out) });
    }
    // Curvature agreement at q12 (blends 1, 2) and q23 (blends 2, 3): along unit tangents x, y
    // the normal curvatures (x·e)(y·e)/r of the two cylinders match (e: in the face, square to
    // the cylinder's axis).
    let rmin = s1.r.min(s2.r).min(s3.r);
    let resid = |psi: [f64; 3]| -> Option<[f64; 5]> {
        let [(m1, _), (m2, _), (m3, _)] = mk(psi).ok()?;
        let ii = |x: Vec3, y: Vec3, e: Vec3, r: f64| x.dot(e) * y.dot(e) / r;
        let (a, b) = ((m1 - q12).normalized()?, (m2 - q12).normalized()?);
        let c12 = ii(a, b, s1.t(f12), s1.r) - ii(a, b, s2.t(f12), s2.r);
        let (a, b) = ((m2 - q23).normalized()?, (m3 - q23).normalized()?);
        let c23 = ii(a, b, s2.t(f23), s2.r) - ii(a, b, s3.t(f23), s3.r);
        let mu = 1e-2;
        Some([c12 * rmin, c23 * rmin, psi[0] * mu, psi[1] * mu, psi[2] * mu])
    };
    let cost = |p: [f64; 3]| resid(p).map(|r| r.iter().map(|x| x * x).sum::<f64>()).unwrap_or(f64::INFINITY);
    let mut best = [0.0; 3];
    for start in [[0.0; 3], [0.3, 0.3, 0.3], [-0.3, -0.3, -0.3], [0.3, -0.3, 0.3], [-0.3, 0.3, -0.3]] {
        let mut p = start;
        let mut lambda = 1e-3;
        for _ in 0..100 {
            let Some(r0) = resid(p) else { break };
            // Gauss-Newton with damping on the 3 unknowns.
            let mut jac = [[0.0; 3]; 5];
            for k in 0..3 {
                let mut q = p;
                if let Some(x) = q.get_mut(k) {
                    *x += 1e-7;
                }
                let Some(rk) = resid(q) else { continue };
                for (i, row) in jac.iter_mut().enumerate() {
                    if let (Some(slot), Some(a), Some(b)) = (row.get_mut(k), rk.get(i), r0.get(i)) {
                        *slot = (a - b) / 1e-7;
                    }
                }
            }
            let mut m = [[0.0; 3]; 3];
            let mut g = [0.0; 3];
            for (i, row) in jac.iter().enumerate() {
                for a in 0..3 {
                    if let (Some(ga), Some(ja), Some(ri)) = (g.get_mut(a), row.get(a), r0.get(i)) {
                        *ga -= ja * ri;
                    }
                    for b in 0..3 {
                        if let (Some(mab), Some(ja), Some(jb)) = (m.get_mut(a).and_then(|x| x.get_mut(b)), row.get(a), row.get(b)) {
                            *mab += ja * jb;
                        }
                    }
                }
            }
            for (a, row) in m.iter_mut().enumerate() {
                if let Some(x) = row.get_mut(a) {
                    *x *= 1.0 + lambda;
                }
            }
            let Some(dp) = crate::polyhedron::solve3_pub(m, g) else { break };
            let next = [p[0] + dp[0], p[1] + dp[1], p[2] + dp[2]];
            if cost(next) < cost(p) {
                p = next;
                lambda *= 0.3;
            } else {
                lambda *= 10.0;
            }
            if dp.iter().all(|x| x.abs() < 1e-12) {
                break;
            }
        }
        if cost(p) < cost(best) {
            best = p;
        }
    }
    let [(m1, t1), (m2, t2), (m3, t3)] = mk(best)?;
    // Degree-2 patch through the sections (rows along the second section, columns along the
    // first and third), raised to degree 4.
    let w11 = s2.w * (s1.w + s3.w) * 0.5;
    let p11 = ((m1 + m2 + m3) * 2.0 - (q12 + q23 + q31)) * (1.0 / 3.0);
    let c2: [[mt::Vector4; 3]; 3] =
        [[h(q12, 1.0), h(m1, s1.w), h(q31, 1.0)], [h(m2, s2.w), h(p11, w11), h(q31, s2.w)], [h(q23, 1.0), h(m3, s3.w), h(q31, 1.0)]];
    const RAISE: [[f64; 3]; 5] = [[1.0, 0.0, 0.0], [0.5, 0.5, 0.0], [1.0 / 6.0, 2.0 / 3.0, 1.0 / 6.0], [0.0, 0.5, 0.5], [0.0, 0.0, 1.0]];
    let zero = mt::Vector4::new(0.0, 0.0, 0.0, 0.0);
    let mut hp = [[zero; 5]; 5];
    for (i, ri) in RAISE.iter().enumerate() {
        for (j, rj) in RAISE.iter().enumerate() {
            let mut acc = zero;
            for (a, ca) in ri.iter().enumerate() {
                for (b, cb) in rj.iter().enumerate() {
                    if let Some(x) = c2.get(a).and_then(|r| r.get(b)) {
                        acc += *x * (ca * cb);
                    }
                }
            }
            if let Some(slot) = hp.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot = acc;
            }
        }
    }
    fit_tangent(&mut hp, [s1, s2, s3], q31, n31)?;
    let ctrl: Vec<Vec<mt::Vector4>> = hp.iter().map(|r| r.to_vec()).collect();
    let surf =
        mt::Surface::NurbsSurface(mt::NurbsSurface::new(mt::BSplineSurface::new((mt::KnotVec::bezier_knot(4), mt::KnotVec::bezier_knot(4)), ctrl)));
    let rs = s1.r + s2.r + s3.r;
    let surf = orient(surf, (q12 + q23 + q31) * (1.0 / 3.0) + out * rs * 0.05, out);
    Ok(CornerPatch { sections: [t1, t2, t3], surf })
}

fn bern4(i: usize, t: f64) -> f64 {
    let c = [1.0, 4.0, 6.0, 4.0, 1.0];
    c.get(i).copied().unwrap_or(0.0) * t.powi(i as i32) * (1.0 - t).powi(4 - i as i32)
}

/// Move the inner control points of the degree-4 patch (boundary rows fixed) as little as
/// possible so that across each boundary the patch is tangent to that blend's cylinder, and at
/// the closed corner q31 to the face there (normal `n31`): conditions linear in the homogeneous
/// points, sampled along each boundary and solved by least squares.
fn fit_tangent(hp: &mut [[mt::Vector4; 5]; 5], s: [&Sel; 3], q31: Vec3, n31: Vec3) -> Result<()> {
    const N: usize = 36;
    let unknown = |i: usize, j: usize| -> Option<usize> { ((1..4).contains(&i) && (1..4).contains(&j)).then(|| ((i - 1) * 3 + (j - 1)) * 4) };
    let at =
        |hp: &[[mt::Vector4; 5]; 5], i: usize, j: usize| hp.get(i).and_then(|r| r.get(j)).copied().unwrap_or(mt::Vector4::new(0.0, 0.0, 0.0, 1.0));
    let eval = |hp: &[[mt::Vector4; 5]; 5], u: f64, v: f64| -> Vec3 {
        let mut acc = mt::Vector4::new(0.0, 0.0, 0.0, 0.0);
        for i in 0..5 {
            for j in 0..5 {
                acc += at(hp, i, j) * (bern4(i, u) * bern4(j, v));
            }
        }
        if acc.w.abs() < 1e-300 { Vec3::ZERO } else { Vec3::new(acc.x / acc.w, acc.y / acc.w, acc.z / acc.w) }
    };
    let cyl_normal = |s: &Sel, p: Vec3| -> Vec3 {
        let x = p - s.axis_point();
        (x - s.d * x.dot(s.d)).normalized().unwrap_or(Vec3::Z)
    };
    let mut rows: Vec<([f64; N], f64)> = Vec::new();
    let big = 1e3;
    let samples = 13;
    for k in 1..=samples {
        let t = k as f64 / (samples + 1) as f64;
        // Boundary u = 0 (first blend), v = 0 (second), u = 1 (third): the row next to it.
        for bd in 0..3 {
            let mut row = [0.0; N];
            let mut cst = 0.0;
            let (p, sel) = match bd {
                0 => (eval(hp, 0.0, t), s[0]),
                1 => (eval(hp, t, 0.0), s[1]),
                _ => (eval(hp, 1.0, t), s[2]),
            };
            let n = cyl_normal(sel, p);
            for m in 0..5 {
                let (i, j) = match bd {
                    0 => (1, m),
                    1 => (m, 1),
                    _ => (3, m),
                };
                let b = bern4(m, t);
                let coef = [n.x * b, n.y * b, n.z * b, -p.dot(n) * b];
                match unknown(i, j) {
                    Some(o) => {
                        for (c, x) in coef.iter().enumerate() {
                            if let Some(slot) = row.get_mut(o + c) {
                                *slot += x * big;
                            }
                        }
                    }
                    None => {
                        let hv = at(hp, i, j);
                        cst += coef[0] * hv.x + coef[1] * hv.y + coef[2] * hv.z + coef[3] * hv.w;
                    }
                }
            }
            rows.push((row, -cst * big));
        }
    }
    // At the closed corner every direction leaving it lies in the face's plane.
    for i in 1..4 {
        let mut row = [0.0; N];
        if let Some(o) = unknown(i, 3) {
            for (c, x) in [n31.x, n31.y, n31.z, -q31.dot(n31)].iter().enumerate() {
                if let Some(slot) = row.get_mut(o + c) {
                    *slot = x * big;
                }
            }
        }
        rows.push((row, 0.0));
    }
    // Stay near the raised patch.
    for i in 1..4 {
        for j in 1..4 {
            let hv = at(hp, i, j);
            if let Some(o) = unknown(i, j) {
                for (c, x) in [hv.x, hv.y, hv.z, hv.w].iter().enumerate() {
                    let mut row = [0.0; N];
                    if let Some(slot) = row.get_mut(o + c) {
                        *slot = 1.0;
                    }
                    rows.push((row, *x));
                }
            }
        }
    }
    // Normal equations, Gaussian elimination with partial pivoting.
    let mut a = vec![[0.0; N]; N];
    let mut y = [0.0; N];
    for (row, rhs) in &rows {
        for (p, rp) in row.iter().enumerate() {
            if *rp == 0.0 {
                continue;
            }
            if let Some(yp) = y.get_mut(p) {
                *yp += rp * rhs;
            }
            if let Some(ap) = a.get_mut(p) {
                for (q, rq) in row.iter().enumerate() {
                    if let Some(x) = ap.get_mut(q) {
                        *x += rp * rq;
                    }
                }
            }
        }
    }
    for c in 0..N {
        let piv = (c..N).max_by(|x, z| {
            let g = |r: usize| a.get(r).and_then(|row| row.get(c)).map(|v| v.abs()).unwrap_or(0.0);
            g(*x).total_cmp(&g(*z))
        });
        let Some(piv) = piv else { return Err(KernelError::Failed("corner patch".into())) };
        a.swap(c, piv);
        y.swap(c, piv);
        let (Some(arow), Some(&yc)) = (a.get(c).copied(), y.get(c)) else { continue };
        let d = arow.get(c).copied().unwrap_or(0.0);
        if d.abs() < 1e-300 {
            return Err(unsupported("this corner patch"));
        }
        for r in 0..N {
            if r == c {
                continue;
            }
            let Some(row) = a.get_mut(r) else { continue };
            let f = row.get(c).copied().unwrap_or(0.0) / d;
            if f == 0.0 {
                continue;
            }
            for (x, ac) in row.iter_mut().zip(arow.iter()) {
                *x -= f * ac;
            }
            if let Some(yr) = y.get_mut(r) {
                *yr -= f * yc;
            }
        }
    }
    for i in 1..4 {
        for j in 1..4 {
            let Some(o) = unknown(i, j) else { continue };
            let val = |k: usize| y.get(o + k).copied().unwrap_or(0.0) / a.get(o + k).and_then(|r| r.get(o + k)).copied().unwrap_or(1.0);
            let v = mt::Vector4::new(val(0), val(1), val(2), val(3));
            if !(v.w > 1e-6 && v.x.is_finite() && v.y.is_finite() && v.z.is_finite()) {
                return Err(unsupported("this corner patch"));
            }
            if let Some(slot) = hp.get_mut(i).and_then(|r| r.get_mut(j)) {
                *slot = v;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, fillet, measure};

    /// The 20 mm box and the three edges at its corner (20, 0, 20): `a` along y, `b` along x,
    /// `c` up.
    fn setup() -> (Body, Vec3, Vec3, Vec3) {
        let b = box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        (b, Vec3::new(20.0, 10.0, 20.0), Vec3::new(10.0, 0.0, 20.0), Vec3::new(20.0, 0.0, 10.0))
    }

    fn vol(b: &Body) -> f64 {
        measure(b).unwrap().volume
    }

    /// The body minus the material outside each round (the strips between the faces and the
    /// cylinders), for rounds `ra`, `rb`, `rc` on edges a, b, c: the strips integrated exactly
    /// and their overlap at the corner numerically.
    fn strips(ra: f64, rb: f64, rc: f64) -> f64 {
        let len = |d: f64, r: f64| if d < r { r - (r * r - (r - d) * (r - d)).max(0.0).sqrt() } else { 0.0 };
        let (n, l) = (800, 5.0);
        let hh = l / n as f64;
        let (mut union, mut sum) = (0.0, 0.0);
        for i in 0..n {
            let dx = (i as f64 + 0.5) * hh;
            for j in 0..n {
                let dy = (j as f64 + 0.5) * hh;
                let tc = rc > 0.0 && dx < rc && dy < rc && (rc - dx).powi(2) + (rc - dy).powi(2) > rc * rc;
                let (a, b) = (len(dx, ra).min(l), len(dy, rb).min(l));
                union += if tc { l } else { a.max(b) };
                sum += a + b + if tc { l } else { 0.0 };
            }
        }
        let full = (1.0 - std::f64::consts::FRAC_PI_4) * (ra * ra + rb * rb + rc * rc) * 20.0;
        8000.0 - full + (sum - union) * hh * hh
    }

    /// Largest angle (degrees) between the two faces' normals along edges between curved faces.
    fn kink(b: &Body) -> f64 {
        use mt::{BoundedCurve, InnerSpace, ParametricCurve, ParametricSurface3D, SearchNearestParameter};
        let faces: Vec<&mt::Face> = b.solid.face_iter().collect();
        let mut worst: f64 = 0.0;
        for (i, f) in faces.iter().enumerate() {
            for g in faces.iter().skip(i + 1) {
                if plane_normal(f).is_some() || plane_normal(g).is_some() {
                    continue;
                }
                for e in f.edge_iter().filter(|e| g.edge_iter().any(|x| x.id() == e.id())) {
                    let c = e.curve();
                    let (t0, t1) = c.range_tuple();
                    for k in 1..10 {
                        let p = c.subs(t0 + (t1 - t0) * k as f64 / 10.0);
                        let n = |s: mt::Surface| {
                            let (u, v) = s.search_nearest_parameter(p, None, 100).unwrap();
                            s.normal(u, v)
                        };
                        let (a, bb) = (n(f.oriented_surface()), n(g.oriented_surface()));
                        worst = worst.max(a.dot(bb).abs().min(1.0).acos().to_degrees());
                    }
                }
            }
        }
        worst
    }

    #[test]
    fn analytic_volumes_of_the_reference() {
        // One round and the sphere corner of three equal ones.
        let one = 8000.0 - (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 20.0;
        assert!((strips(2.0, 0.0, 0.0) - one).abs() < 1e-6);
        // Rounds 2 and 3 on a and b: the overlap is ∫ (2 − √(4 − (z − 18)²))(3 − √(9 − (z − 17)²)) dz
        // over z from 18 to 20 (Simpson, 200000 steps).
        assert!((strips(2.0, 3.0, 0.0) - 7945.579002).abs() < 1e-3);
    }

    #[test]
    fn a_round_next_to_a_round_of_another_radius() {
        let (bx, a, b, _) = setup();
        let ra = fillet(&bx, &[a], 2.0).unwrap();
        // Today's local blend refuses the edge that ends on the round; the corner path
        // takes the round off and blends both edges: their cylinders trim each other.
        let r = fillet(&ra, &[b], 3.0).unwrap();
        assert_eq!(r.face_count(), 8);
        assert_eq!(r.unmeshed_faces(), 0);
        assert!((vol(&r) - strips(2.0, 3.0, 0.0)).abs() < 0.05, "{} vs {}", vol(&r), strips(2.0, 3.0, 0.0));
        // The same in one go with a radius per edge, either way round.
        for sel in [[(a, 2.0), (b, 3.0)], [(b, 3.0), (a, 2.0)]] {
            let s = fillet_radii(&bx, &sel).unwrap();
            assert!((vol(&s) - vol(&r)).abs() < 1e-3);
        }
        let s = fillet_radii(&bx, &[(a, 3.0), (b, 2.0)]).unwrap();
        assert!((vol(&s) - strips(3.0, 2.0, 0.0)).abs() < 0.05);
        // Equal radii: the mitre, as the local blend makes it.
        let e = fillet(&ra, &[b], 2.0).unwrap();
        assert!((vol(&e) - strips(2.0, 2.0, 0.0)).abs() < 0.05);
    }

    #[test]
    fn three_rounds_at_a_corner() {
        let (bx, a, b, c) = setup();
        // Two rounds meeting in a mitre, then the third edge: a sphere octant.
        let ab = fillet(&bx, &[a, b], 2.0).unwrap();
        let r = fillet(&ab, &[c], 2.0).unwrap();
        let sphere = 8000.0 - 3.0 * (1.0 - std::f64::consts::FRAC_PI_4) * 4.0 * 18.0 - 8.0 * (1.0 - std::f64::consts::PI / 6.0);
        assert_eq!(r.face_count(), 10);
        assert!((vol(&r) - sphere).abs() < 0.05, "{} vs {sphere}", vol(&r));
        assert!(kink(&r) < 0.01);
        // Different radii: a smooth patch over the corner, slightly more than the strips take.
        for (rs, sel) in [((2.0, 2.0, 3.0), fillet(&ab, &[c], 3.0)), ((2.0, 3.0, 4.0), fillet_radii(&bx, &[(a, 2.0), (b, 3.0), (c, 4.0)]))] {
            let r = sel.unwrap();
            assert_eq!(r.face_count(), 10);
            assert_eq!(r.unmeshed_faces(), 0);
            let k = kink(&r);
            assert!(k < 0.05, "kink {k}°");
            let (v, s) = (vol(&r), strips(rs.0, rs.1, rs.2));
            assert!(v < s + 0.05 && v > s - 3.0, "{v} vs {s}");
        }
    }

    #[test]
    fn corners_refuse_what_they_cannot_build() {
        let (bx, a, b, c) = setup();
        // Larger than the faces, or not a size: errors, no panic. Large but fitting is fine.
        assert!(fillet_radii(&bx, &[(a, 15.0), (b, 2.0)]).is_ok_and(|r| r.unmeshed_faces() == 0));
        assert!(fillet_radii(&bx, &[(a, 25.0), (b, 2.0)]).is_err());
        assert!(fillet_radii(&bx, &[(a, f64::NAN)]).is_err());
        assert!(fillet_radii(&bx, &[(a, 0.0)]).is_err());
        assert!(fillet_radii(&bx, &[]).is_err());
        assert!(fillet_radii(&bx, &[(Vec3::new(500.0, 0.0, 0.0), 1.0)]).is_err());
        // A concave edge is left to the other blends.
        let l = crate::ops::boolean(&bx, &box_solid(Vec3::new(10.0, -1.0, 10.0), Vec3::new(21.0, 21.0, 21.0)).unwrap(), crate::BoolOp::Cut)
            .unwrap()
            .unwrap();
        assert!(fillet_radii(&l, &[(Vec3::new(10.0, 10.0, 10.0), 1.0), (c, 2.0)]).is_err());
    }
}
