//! Edge fillets and chamfers as a local B-rep operation.
//!
//! Supported locally: a straight edge between two planar faces whose two end vertices each
//! touch exactly one more planar face. Fillets require perpendicular end faces; chamfers
//! intersect their offset rails with the end planes, including sloped walls. The end faces get an arc
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

pub(crate) fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

pub(crate) fn vtx(v: &mt::Vertex) -> Vec3 {
    from_p3(v.point())
}

/// Outward unit normal of a planar face (from its oriented surface), if planar.
pub(crate) fn plane_normal(f: &mt::Face) -> Option<Vec3> {
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
pub(crate) fn rebuild_face(f: &mt::Face, subst: &HashMap<mt::EdgeID, mt::Edge>, connectors: &[mt::Edge]) -> Result<mt::Face> {
    let mut wires = Vec::new();
    for w in f.absolute_boundaries() {
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
    crate::heal::absolute_face(f, wires).ok_or_else(|| KernelError::Failed("blend: a face could not be rebuilt".into()))
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
    // In-face directions perpendicular to the edge, into each face: a face's boundary runs
    // counter-clockwise about its outward normal, so its interior is to the left.
    let into = |f: &mt::Face, n: Vec3| -> Option<Vec3> {
        let e = f.boundary_iters().into_iter().flatten().find(|e| e.id() == edge.id())?;
        let dir = (vtx(e.back()) - vtx(e.front())).normalized()?;
        n.cross(dir).normalized()
    };
    let (Some(t1), Some(t2)) = (into(f1, n1), into(f2, n2)) else { return Err(KernelError::Failed("blend: edge direction".into())) };
    // Convex edge: each face's outward normal points away from the other face; concave: toward.
    let cos_phi = t1.dot(t2).clamp(-1.0, 1.0);
    let phi = cos_phi.acos();
    let convex = n1.dot(t2) < -1e-9 && n2.dot(t1) < -1e-9;
    let concave = n1.dot(t2) > 1e-9 && n2.dot(t1) > 1e-9;
    if !(phi > 1e-3 && phi < std::f64::consts::PI - 1e-3) || !(convex || concave) {
        return Err(unsupported("the faces at the edge are tangent or the edge is ambiguous"));
    }
    // The blend's centre line is inside the material for a convex edge, outside for a concave one.
    let side = if convex { -1.0 } else { 1.0 };
    blend_geometry(solid, si, &faces, (i1, i2), edge, (v0, v1), (p0, p1, d), (n1, t1, t2, phi, side), size, r, shape)
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
    (n1, t1, t2, phi, side): (Vec3, Vec3, Vec3, f64, f64),
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
        a: Vec3,
        b: Vec3,
    }
    let mut ends: Vec<End> = Vec::new();
    for (v, p) in [(&v0, p0), (&v1, p1)] {
        let at_v: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.vertex_iter().any(|x| &x == v))).collect();
        let others: Vec<usize> = at_v.iter().copied().filter(|i| *i != i1 && *i != i2).collect();
        let [g] = others[..] else { return Err(unsupported("each end of the edge must touch exactly one more face")) };
        let gf = faces.get(g).ok_or_else(|| KernelError::Failed("face".into()))?;
        let ng = plane_normal(gf).ok_or_else(|| unsupported("the faces at the edge ends must be planar"))?;
        if shape == Shape::Round && ng.dot(d).abs() < 1.0 - 1e-9 {
            return Err(unsupported("the faces at the edge ends must be perpendicular to the edge"));
        }
        if ng.dot(d).abs() < 1e-9 {
            return Err(unsupported("an end face parallel to the edge"));
        }
        // Each rail is parallel to the original edge and set back by s in its side
        // face. Slide its endpoint along d until it lies on the actual end plane.
        let endpoint = |t: Vec3| {
            let q = p + t * s;
            if shape == Shape::Flat { q - d * (ng.dot(t) * s / ng.dot(d)) } else { q }
        };
        let (a, b) = (endpoint(t1), endpoint(t2));
        // Edges at v shared by g with f1 and with f2.
        let shared = |fi: usize| -> Result<mt::Edge> {
            let f = faces.get(fi).ok_or_else(|| KernelError::Failed("face".into()))?;
            f.edge_iter()
                .find(|e| e.id() != edge.id() && (e.front() == v || e.back() == v) && gf.edge_iter().any(|x| x.id() == e.id()))
                .ok_or_else(|| unsupported("unexpected corner topology"))
        };
        let (e1, e2) = (shared(i1)?, shared(i2)?);
        for (e, q) in [(&e1, a), (&e2, b)] {
            if !matches!(e.curve(), mt::Curve::Line(_)) {
                return Err(unsupported("the edges at the corners must be straight"));
            }
            let other = if e.front() == v { vtx(e.back()) } else { vtx(e.front()) };
            let delta = other - p;
            let len = delta.len();
            let dir = delta.normalized().ok_or_else(|| unsupported("zero-length corner edge"))?;
            let along = (q - p).dot(dir);
            // An inner rim extends the neighbouring rim edge away from its other
            // vertex. This is valid; passing that other vertex is not.
            if (q - p - dir * along).len() > tol * 100.0 + 1e-7 || along >= len - tol || (shape == Shape::Round && along <= tol) {
                return Err(unsupported("the blend is larger than the neighbouring faces"));
            }
        }
        ends.push(End { v: v.clone(), g, e1, e2, a, b });
    }
    let [end0, end1] = &ends[..] else { return Err(KernelError::Failed("ends".into())) };
    if end0.g == end1.g {
        return Err(unsupported("both ends touch the same face"));
    }
    if (end1.a - end0.a).dot(d) <= tol || (end1.b - end0.b).dot(d) <= tol {
        return Err(unsupported("the blend is larger than the neighbouring faces"));
    }
    let a0 = builder::vertex(p3(end0.a));
    let b0 = builder::vertex(p3(end0.b));
    let a1 = builder::vertex(p3(end1.a));
    let b1 = builder::vertex(p3(end1.b));
    // Corner curves on the end faces (A → B) and the blend surface. The arc centre is r from
    // both faces (inside the material for a convex edge, outside for a concave one); the arc's
    // middle is the point nearest the old edge.
    let centre = |p: Vec3| p + t1 * s + n1 * (r * side);
    let arc_mid = |p: Vec3| {
        let c = centre(p);
        c + (p - c).normalized().unwrap_or(-n1) * r
    };
    let corner = |a: &mt::Vertex, b: &mt::Vertex, p: Vec3| -> mt::Edge {
        match shape {
            Shape::Round => builder::circle_arc(a, b, p3(arc_mid(p))),
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
            // Outward normal at the arc middle: away from the centre on a convex edge (the
            // centre is in the material), toward it on a concave one.
            let (c, mid) = (centre(p0), arc_mid(p0));
            let outward = (mid - c) * (-side);
            use mt::{ParametricSurface3D, SearchParameter};
            if let Some((u, v)) = surf.search_parameter(p3(mid), None, 100) {
                let nn = surf.normal(u, v);
                if Vec3::new(nn.x, nn.y, nn.z).dot(outward) < 0.0 {
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
    // Coplanar neighbours as one face (as after a join), so corners look as they should.
    let mut cur = Body::new(crate::heal::heal_keep(body.deep_copy(), body.size(), body.split_keep()))?;
    // Every edge of a convex planar body: rounded as a whole (sphere corners).
    if shape == Shape::Round && edges.len() >= 6 {
        let solid = cur.deep_copy();
        let all = Body::unique_edges(&solid);
        if all.len() == edges.len() {
            let size = cur.size();
            let tol = (size * 2e-3).max(1e-3);
            let mut hit = std::collections::HashSet::new();
            for p in edges {
                if let Some((idx, _)) = cur.nearest_edge(*p, tol)? {
                    hit.insert(idx);
                }
            }
            if hit.len() == all.len()
                && let Ok(b) = crate::polyhedron::round_all_edges(&cur, r)
            {
                return Ok(b);
            }
        }
    }
    // Whole smooth loops of planar faces are blended in one go, loop by loop.
    // Loops and other edges together (every edge of an extruded part): the other edges
    // first, which leaves the loops smooth, then the loops.
    if let Some((groups, rest)) = loop_split(&cur, edges)?
        && !rest.is_empty()
        && rest.len() < edges.len()
    {
        let _ = groups;
        let loop_pts: Vec<Vec3> = edges.iter().filter(|p| !rest.iter().any(|q| q.dist(**p) < 1e-12)).copied().collect();
        cur = blend(&cur, &rest, r, shape, what)?;
        return blend(&cur, &loop_pts, r, shape, what);
    }
    if let Some(groups) = loop_groups(&cur, edges)? {
        for g in groups {
            let size = cur.size();
            let solid = cur.deep_copy();
            let ids = edge_ids(&cur, &solid, &g)?;
            match crate::loopblend::loop_blend(&solid, &ids, size, r, shape == Shape::Round) {
                Some(res) => cur = Body::new(guard(what, || res)?)?,
                None => return Err(unsupported("the edges are not a whole loop of a planar face")),
            }
        }
        return Ok(cur);
    }
    // One at a time, in whatever order works: a run along a planar face's loop (bridging arcs
    // that earlier blends left between selected edges, so corners round off), a single edge,
    // or a chain of curved edges.
    let mut todo: Vec<Vec3> = edges.to_vec();
    for _ in 0..(2 * edges.len() + 4) {
        if todo.is_empty() {
            return Ok(cur);
        }
        let size = cur.size();
        let tol = (size * 2e-3).max(1e-3);
        let solid = cur.deep_copy();
        if todo.len() >= 2
            && let Some((ids, covered)) = run_on_loop(&cur, &solid, &todo)?
            && let Some(Ok(res)) = crate::loopblend::loop_blend(&solid, &ids, size, r, shape == Shape::Round)
            && let Ok(b) = guard(what, || Ok(res)).and_then(Body::new)
        {
            cur = b;
            todo.retain(|p| !covered.iter().any(|q| q.dist(*p) < 1e-12));
            continue;
        }
        let mut first_err: Option<KernelError> = None;
        let mut done: Option<(Body, Vec<usize>)> = None;
        for (j, p) in todo.iter().enumerate() {
            let Some((idx, dist)) = cur.nearest_edge(*p, tol)? else { return Err(KernelError::Invalid("the body has no edges".into())) };
            if dist > size * 0.05 + 1e-3 {
                return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
            }
            let edge = Body::unique_edges(&solid).into_iter().nth(idx).ok_or_else(|| KernelError::Invalid("edge index".into()))?;
            let attempt = match guard(what, || blend_one(&solid, &edge, size, r, shape)) {
                Ok(o) => Ok((o, vec![j])),
                // A curved edge, or one between curved faces: roll a ball along its smooth chain.
                Err(e) if !curved_case(&solid, &edge) => Err(e),
                Err(e) => match smooth_chain(&solid, &edge) {
                    Some(ids) => {
                        let on_chain: Vec<usize> = todo
                            .iter()
                            .enumerate()
                            .filter(|(_, q)| {
                                cur.nearest_edge(**q, tol)
                                    .ok()
                                    .flatten()
                                    .and_then(|(i, _)| Body::unique_edges(&solid).into_iter().nth(i))
                                    .is_some_and(|x| ids.contains(&x.id()))
                            })
                            .map(|(i, _)| i)
                            .collect();
                        guard(what, || crate::curveblend::curve_blend(&solid, &ids, size, r, shape == Shape::Round))
                            .map(|o| (o, if on_chain.is_empty() { vec![j] } else { on_chain }))
                            .map_err(|e2| KernelError::Failed(format!("{e2} ({e})")))
                    }
                    None => Err(e),
                },
            };
            match attempt.and_then(|(o, js)| Body::new(o).map(|b| (b, js))) {
                Ok(x) => {
                    done = Some(x);
                    break;
                }
                Err(e) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        match done {
            Some((b, js)) => {
                cur = b;
                let mut k = 0;
                todo.retain(|_| {
                    let keep = !js.contains(&k);
                    k += 1;
                    keep
                });
            }
            None => return Err(first_err.unwrap_or_else(|| unsupported("these edges"))),
        }
    }
    if todo.is_empty() { Ok(cur) } else { Err(unsupported("blending these edges together")) }
}

/// Neighbouring edges along one boundary loop of a planar face that between them hold at least
/// two of the points' edges, joined by arcs that meet them smoothly (left by earlier blends):
/// the run's edge ids and the points it covers.
#[allow(clippy::type_complexity)]
fn run_on_loop(cur: &Body, solid: &Solid, pts: &[Vec3]) -> Result<Option<(Vec<mt::EdgeID>, Vec<Vec3>)>> {
    use mt::{BoundedCurve, ParametricCurve};
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let all = Body::unique_edges(solid);
    let mut per: Vec<(Vec3, mt::EdgeID)> = Vec::new();
    for p in pts {
        if let Some((idx, dist)) = cur.nearest_edge(*p, tol)?
            && dist <= size * 0.05 + 1e-3
            && let Some(e) = all.get(idx)
        {
            per.push((*p, e.id()));
        }
    }
    let dir = |e: &mt::Edge, at_end: bool| -> Option<Vec3> {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let d = c.der(if at_end { t1 } else { t0 });
        Vec3::new(d.x, d.y, d.z).normalized()
    };
    let mut best: Option<(Vec<mt::EdgeID>, Vec<Vec3>)> = None;
    for f in solid.face_iter() {
        if !matches!(f.oriented_surface(), mt::Surface::Plane(_)) {
            continue;
        }
        for w in f.boundaries() {
            let es: Vec<mt::Edge> = w.edge_iter().cloned().collect();
            let m = es.len();
            let sel = |i: usize| es.get(i % m).is_some_and(|e| per.iter().any(|(_, id)| *id == e.id()));
            // A filler: an unselected arc meeting both neighbours smoothly.
            let smooth = |a: &mt::Edge, b: &mt::Edge| match (dir(a, true), dir(b, false)) {
                (Some(x), Some(y)) => x.dot(y) > 1.0 - 1e-6,
                _ => false,
            };
            let filler = |i: usize| {
                let (Some(prev), Some(e), Some(next)) = (es.get((i + m - 1) % m), es.get(i % m), es.get((i + 1) % m)) else { return false };
                !matches!(e.curve(), mt::Curve::Line(_)) && smooth(prev, e) && smooth(e, next)
            };
            let usable = |i: usize| sel(i) || filler(i);
            if (0..m).all(usable) {
                // The whole loop.
                if (0..m).filter(|i| sel(*i)).count() >= 2 {
                    let ids: Vec<mt::EdgeID> = es.iter().map(|e| e.id()).collect();
                    let covered: Vec<Vec3> = per.iter().filter(|(_, id)| ids.contains(id)).map(|(p, _)| *p).collect();
                    if best.as_ref().is_none_or(|b| covered.len() > b.1.len()) {
                        best = Some((ids, covered));
                    }
                }
                continue;
            }
            // Runs: start after an unusable edge; trim fillers off both ends.
            for s0 in 0..m {
                if usable(s0) && !usable(s0 + m - 1) {
                    let mut idx: Vec<usize> = (0..m).map(|j| s0 + j).take_while(|i| usable(*i)).collect();
                    while idx.first().is_some_and(|i| !sel(*i)) {
                        idx.remove(0);
                    }
                    while idx.last().is_some_and(|i| !sel(*i)) {
                        idx.pop();
                    }
                    if idx.iter().filter(|i| sel(**i)).count() < 2 {
                        continue;
                    }
                    let ids: Vec<mt::EdgeID> = idx.iter().filter_map(|i| es.get(i % m).map(|e| e.id())).collect();
                    // Not yet when another selected edge meets one of the run's inner corners:
                    // that edge goes first, and the run then rounds the corner.
                    let inner: Vec<mt::Vertex> = idx.iter().skip(1).filter_map(|i| es.get(i % m).map(|e| e.front().clone())).collect();
                    let blocked = per
                        .iter()
                        .filter(|(_, id)| !ids.contains(id))
                        .any(|(_, id)| all.iter().find(|e| e.id() == *id).is_some_and(|e| inner.iter().any(|v| v == e.front() || v == e.back())));
                    if blocked {
                        continue;
                    }
                    let covered: Vec<Vec3> = per.iter().filter(|(_, id)| ids.contains(id)).map(|(p, _)| *p).collect();
                    if best.as_ref().is_none_or(|b| covered.len() > b.1.len()) {
                        best = Some((ids, covered));
                    }
                }
            }
        }
    }
    Ok(best)
}

/// Is the edge curved, or does it lie between faces that aren't both planar?
fn curved_case(solid: &Solid, edge: &mt::Edge) -> bool {
    if !matches!(edge.curve(), mt::Curve::Line(_)) {
        return true;
    }
    solid.face_iter().filter(|f| f.edge_iter().any(|e| e.id() == edge.id())).any(|f| !matches!(f.oriented_surface(), mt::Surface::Plane(_)))
}

/// The chain of edges continuing `edge` smoothly at its ends (its ids): closed, or open where
/// the next edge turns sharply.
fn smooth_chain(solid: &Solid, edge: &mt::Edge) -> Option<Vec<mt::EdgeID>> {
    use mt::{BoundedCurve, ParametricCurve};
    let all = Body::unique_edges(solid);
    let dir = |e: &mt::Edge, at_end: bool| -> Option<Vec3> {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        let d = c.der(if at_end { t1 } else { t0 });
        Vec3::new(d.x, d.y, d.z).normalized()
    };
    let mut ids = vec![edge.id()];
    // Walk on from one end of the edge (leaving vertex `v` in direction `d`).
    let walk = |mut v: mt::Vertex, mut d: Vec3, ids: &mut Vec<mt::EdgeID>, stop: &mt::Vertex| -> Option<bool> {
        for _ in 0..200 {
            if v == *stop {
                return Some(true);
            }
            let next = all.iter().filter(|e| !ids.contains(&e.id()) && (*e.front() == v || *e.back() == v)).find_map(|e| {
                let fwd = *e.front() == v;
                let t = if fwd { dir(e, false)? } else { -dir(e, true)? };
                (t.dot(d) > 1.0 - 1e-4).then(|| (e.clone(), fwd))
            });
            let Some((e, fwd)) = next else { return Some(false) };
            ids.push(e.id());
            if fwd {
                v = e.back().clone();
                d = dir(&e, true)?;
            } else {
                v = e.front().clone();
                d = -dir(&e, false)?;
            }
        }
        None
    };
    let closed = walk(edge.back().clone(), dir(edge, true)?, &mut ids, edge.front())?;
    if !closed {
        walk(edge.front().clone(), -dir(edge, false)?, &mut ids, edge.back())?;
    }
    Some(ids)
}

fn edge_mid(e: &mt::Edge) -> Option<Vec3> {
    use mt::{BoundedCurve, ParametricCurve};
    let c = e.curve();
    let (t0, t1) = c.range_tuple();
    Some(from_p3(c.subs((t0 + t1) * 0.5)))
}

/// Do the loop's edges meet tangentially everywhere?
fn smooth_loop(w: &mt::Wire) -> bool {
    use mt::{BoundedCurve, ParametricCurve};
    let es: Vec<mt::Edge> = w.edge_iter().cloned().collect();
    let n = es.len();
    let dir = |e: &mt::Edge, at_end: bool| -> Option<Vec3> {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        let t = if at_end { t1 } else { t0 };
        let d = c.der(t);
        Vec3::new(d.x, d.y, d.z).normalized()
    };
    (0..n).all(|i| match (es.get(i), es.get((i + 1) % n)) {
        (Some(a), Some(b)) => match (dir(a, true), dir(b, false)) {
            (Some(x), Some(y)) => x.dot(y) > 1.0 - 1e-6,
            _ => false,
        },
        _ => false,
    })
}

/// The B-rep edges nearest the points (each once).
fn edge_ids(cur: &Body, solid: &Solid, pts: &[Vec3]) -> Result<Vec<mt::EdgeID>> {
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let all = Body::unique_edges(solid);
    let mut ids = Vec::new();
    for p in pts {
        if let Some((idx, dist)) = cur.nearest_edge(*p, tol)?
            && dist <= size * 0.05 + 1e-3
            && let Some(e) = all.get(idx)
            && !ids.contains(&e.id())
        {
            ids.push(e.id());
        }
    }
    Ok(ids)
}

/// When the edges make up whole boundary loops of planar faces: the edge points per loop.
fn loop_groups(cur: &Body, pts: &[Vec3]) -> Result<Option<Vec<Vec<Vec3>>>> {
    Ok(loop_split(cur, pts)?.and_then(|(g, rest)| rest.is_empty().then_some(g)))
}

/// Edges in whole loops of planar faces (points per loop), and the other edges' points.
fn loop_split(cur: &Body, pts: &[Vec3]) -> Result<Option<(Vec<Vec<Vec3>>, Vec<Vec3>)>> {
    let solid = cur.deep_copy();
    let size = cur.size();
    let tol = (size * 2e-3).max(1e-3);
    let all = Body::unique_edges(&solid);
    // Each point's edge. A point on a vertex is as near to the edges meeting there (a circle
    // and a cylinder's seam line): prefer one on a planar face's boundary.
    let polys = cur.edges(tol)?;
    let planar_edge: std::collections::HashSet<mt::EdgeID> = solid
        .face_iter()
        .filter(|f| matches!(f.oriented_surface(), mt::Surface::Plane(_)))
        .flat_map(|f| f.edge_iter().map(|e| e.id()).collect::<Vec<_>>())
        .collect();
    let mut per: Vec<(Vec3, mt::EdgeID)> = Vec::new();
    for p in pts {
        let dists: Vec<(usize, f64)> =
            polys.iter().map(|e| (e.index, e.points.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min))).collect();
        let best = dists.iter().map(|x| x.1).fold(f64::INFINITY, f64::min);
        if best > size * 0.05 + 1e-3 {
            return Ok(None);
        }
        let near: Vec<&mt::Edge> = dists.iter().filter(|x| x.1 <= best + tol).filter_map(|x| all.get(x.0)).collect();
        let Some(e) = near.iter().find(|e| planar_edge.contains(&e.id())).or(near.first()) else { return Ok(None) };
        per.push((*p, e.id()));
    }
    let ids: Vec<mt::EdgeID> = per.iter().map(|x| x.1).collect();
    let mut covered: Vec<mt::EdgeID> = Vec::new();
    let mut groups = Vec::new();
    for f in solid.face_iter() {
        if !matches!(f.oriented_surface(), mt::Surface::Plane(_)) {
            continue;
        }
        for w in f.boundaries() {
            let wids: Vec<mt::EdgeID> = w.edge_iter().map(|e| e.id()).collect();
            if wids.iter().any(|x| covered.contains(x)) {
                continue;
            }
            // The whole loop is selected, or one of its edges and the loop is smooth (tangent
            // chain, which also covers a circle that a boolean left in pieces).
            let all_sel = wids.iter().all(|x| ids.contains(x));
            let some_sel = wids.iter().any(|x| ids.contains(x));
            if all_sel || (some_sel && smooth_loop(&w)) {
                covered.extend(wids.iter().copied());
                // One point per loop edge, on it.
                let pts: Vec<Vec3> = w.edge_iter().filter_map(edge_mid).collect();
                groups.push(pts);
            }
        }
    }
    let rest: Vec<Vec3> = per.iter().filter(|(_, id)| !covered.contains(id)).map(|(p, _)| *p).collect();
    Ok((!groups.is_empty()).then_some((groups, rest)))
}

/// Constant-radius fillet of the edges nearest to the given points.
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    // Corners the local blends refuse (an earlier round at an edge's end): radius per edge.
    blend(body, edges, radius, Shape::Round, "fillet").or_else(|e| {
        let sel: Vec<(Vec3, f64)> = edges.iter().map(|p| (*p, radius)).collect();
        crate::blend_corner::fillet_radii(body, &sel).map_err(|_| e)
    })
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
    // Coplanar neighbours as one face (as after a join), so corners look as they should.
    let mut cur = Body::new(crate::heal::heal_keep(body.deep_copy(), body.size(), body.split_keep()))?;
    if let Some(groups) = loop_groups(&cur, edges)? {
        for g in groups {
            let size = cur.size();
            let solid = cur.deep_copy();
            let ids = edge_ids(&cur, &solid, &g)?;
            match crate::loopblend::loop_blend(&solid, &ids, size, distance, false) {
                Some(res) => cur = Body::new(guard("chamfer", || res)?)?,
                None => return Err(unsupported("the edges are not a whole loop of a planar face")),
            }
        }
        return Ok(cur);
    }
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
/// The second side of an unequal chamfer: a distance along the other face, or the angle the
/// chamfer makes with the first face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSide {
    Distance(f64),
    Angle(f64),
}

/// A chamfer with different setbacks on the two faces of each (straight) edge: `d1` along the
/// first face (the one facing up most, the other with `flip`), the second side by distance or
/// angle.
pub fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    body.require_brep("chamfer")?;
    let ok = |x: f64| x.is_finite() && x > 1e-6 && x < 1e6;
    let valid = ok(d1)
        && match side {
            ChamferSide::Distance(d) => ok(d),
            ChamferSide::Angle(a) => a.is_finite() && a > 1e-3 && a < std::f64::consts::PI - 1e-3,
        };
    if !valid {
        return Err(KernelError::Invalid("chamfer sizes must be positive (the angle within 0°…180°)".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    let mut cur = body.clone();
    for (k, p) in edges.iter().enumerate() {
        let t = chamfer_tool_sides(&cur, *p, d1, Some((side, flip)), k)?;
        cur = crate::ops::boolean(&cur, &t, crate::BoolOp::Cut)?.ok_or_else(|| KernelError::Failed("the chamfer removed everything".into()))?;
    }
    Ok(cur)
}

pub(crate) fn chamfer_tool(cur: &Body, p: Vec3, s: f64, k: usize) -> Result<Body> {
    chamfer_tool_sides(cur, p, s, None, k)
}

fn chamfer_tool_sides(cur: &Body, p: Vec3, s: f64, sides: Option<(ChamferSide, bool)>, k: usize) -> Result<Body> {
    let mut f = edge_frame(cur, p)?;
    // Setbacks along each face: equal, or by the second side's rule.
    let (s1, s2) = match sides {
        None => (s, s),
        Some((side, flip)) => {
            // Side 1: the face facing up most (then +y, +x), unless flipped.
            let up = Vec3::new(1e-6, 1e-3, 1.0);
            if (f.n2.dot(up) > f.n1.dot(up)) != flip {
                std::mem::swap(&mut f.n1, &mut f.n2);
                std::mem::swap(&mut f.t1, &mut f.t2);
            }
            let s2 = match side {
                ChamferSide::Distance(d) => d,
                ChamferSide::Angle(a) => {
                    // The triangle edge–A–B: angle φ at the edge, `a` at A (on face 1).
                    let phi = f.t1.dot(f.t2).clamp(-1.0, 1.0).acos();
                    let den = (phi + a).sin();
                    if den <= 1e-9 {
                        return Err(KernelError::Invalid("that chamfer angle runs past the other face".into()));
                    }
                    s * a.sin() / den
                }
            };
            (s, s2)
        }
    };
    let s = s1.max(s2);
    let size = cur.size();
    let len = f.p0.dist(f.p1);
    let ext = (s * 2.0).max(size * 0.01) * (1.0 + 0.093 * (k % 5) as f64);
    // Cross-section at p0 - d*ext, in a plane with normal d.
    let base = f.p0 - f.d * ext;
    let plane = solvecraft_geom::Plane::from_normal(base, f.d).ok_or_else(|| KernelError::Failed("chamfer plane".into()))?;
    let a = base + f.t1 * s1;
    let b = base + f.t2 * s2;
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

#[cfg(test)]
mod tests {
    use super::*;
    use solvecraft_geom::{Loop2, Plane, Region2, Vec2};

    #[test]
    fn local_chamfer_intersects_sloped_end_planes() {
        for slope in [-0.5, 0.5] {
            let region = Region2 {
                outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0 - slope * 20.0, 20.0), Vec2::new(slope * 20.0, 20.0)]),
                holes: vec![],
            };
            let body = crate::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
            let pick = [Vec3::new(20.0, 0.0, 20.0)];
            let result = blend(&body, &pick, 2.0, Shape::Flat, "chamfer").unwrap();
            let removed = 2.0 * (40.0 - 2.0 * slope * 2.0 / 3.0);
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&result).unwrap().volume;
            assert!((actual - removed).abs() < 1e-6, "{actual} vs {removed}");
            assert!(result.validity().is_empty());
            // The new corner vertices lie on both original end planes.
            for point in [Vec3::new(slope * 2.0, 2.0, 20.0), Vec3::new(40.0 - slope * 2.0, 2.0, 20.0)] {
                assert!(result.solid.vertex_iter().any(|v| vtx(&v).dist(point) < 1e-8));
            }
            assert!(blend(&body, &pick, 25.0, Shape::Flat, "chamfer").is_err());
            // Oblique fillet caps need elliptical trims, which this local path does not build.
            assert!(blend(&body, &pick, 2.0, Shape::Round, "fillet").is_err());
        }
    }
}
