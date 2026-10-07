//! Resolving sketch links against the model: projecting edges, faces, body silhouettes, other
//! sketches and the origin onto a sketch plane, and intersecting bodies with it.
//!
//! Model geometry is found again on every recompute from the reference point stored in the
//! link (like fillet edges). Edge polylines from the tessellation lie exactly on their curves,
//! so lines and circular arcs are recovered exactly by fitting; anything else (an ellipse from
//! a tilted circle, a spline edge) becomes a chain of lines.

use std::collections::BTreeMap;

use solvecraft_geom::{Mesh, Plane, Vec2, Vec3};
use solvecraft_sketch::{CurveKind, LinkGeom, LinkKind, LinkSource, Sketch};

use crate::eval::ModelState;
use crate::expr::Value;
use crate::{DocError, Document, PlaneRef, Result};

/// Half length of the line an axis or plane projects to.
const AXIS_HALF: f64 = 100.0;
/// Points in one fitted chain at most (longer chains are thinned).
const MAX_CHAIN: usize = 4000;

/// A resolved link: its geometry in sketch coordinates and the (possibly moved) reference.
#[derive(Clone, Debug)]
pub struct Resolved {
    pub geom: Vec<LinkGeom>,
    pub source: LinkSource,
    /// The reference was not exactly where it was and was re-found on the nearest geometry.
    pub moved: bool,
}

/// Resolve a link source against the model state before the sketch.
pub fn resolve(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, plane: &Plane, kind: LinkKind, src: &LinkSource) -> Result<Resolved> {
    let lost = |what: &str| DocError::Invalid(format!("{what} no longer exists"));
    let mut moved = false;
    let mut source = src.clone();
    let geom = match src {
        LinkSource::Edge { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (e, q, d) = nearest_edge(&mesh, *at).ok_or_else(|| lost("the edge"))?;
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the edge"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Edge { body: name, at: q };
            let pts = mesh.edges.get(e).cloned().unwrap_or_default();
            match kind {
                LinkKind::Intersect => section_of_polyline(plane, &pts),
                _ => fit_world(plane, &pts),
            }
        }
        LinkSource::Face { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let (f, q, d) = nearest_face(&mesh, *at).ok_or_else(|| lost("the face"))?;
            let size = mesh.bounds().diagonal().max(1e-9);
            if d > size * 0.25 {
                return Err(lost("the face"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Face { body: name, at: q };
            match kind {
                LinkKind::Intersect => section(plane, &mesh, Some(f)),
                _ => {
                    let mut g = Vec::new();
                    for e in mesh.face_edges(f) {
                        if mesh.seams.get(e).copied().unwrap_or(false) {
                            continue;
                        }
                        if let Some(pts) = mesh.edges.get(e) {
                            g.extend(fit_world(plane, pts));
                        }
                    }
                    g
                }
            }
        }
        LinkSource::Body { body } => {
            let b = st.body(body).ok_or_else(|| lost(&format!("body {body}")))?;
            let mesh = b.mesh();
            match kind {
                LinkKind::Intersect => section(plane, &mesh, None),
                _ => silhouette(plane, &mesh),
            }
        }
        LinkSource::Vertex { body, at } => {
            let (bi, mesh) = body_mesh(st, body, *at)?;
            let size = mesh.bounds().diagonal().max(1e-9);
            let v = mesh
                .edges
                .iter()
                .flat_map(|e| [e.first().copied(), e.last().copied()])
                .flatten()
                .min_by(|a, b| a.dist(*at).total_cmp(&b.dist(*at)))
                .ok_or_else(|| lost("the vertex"))?;
            let d = v.dist(*at);
            if d > size * 0.25 {
                return Err(lost("the vertex"));
            }
            moved = d > (size * 2e-3).max(1e-3);
            let name = st.bodies.get(bi).map(|b| b.name.clone()).unwrap_or_else(|| body.clone());
            source = LinkSource::Vertex { body: name, at: v };
            vec![LinkGeom::Point(plane.to_local(v))]
        }
        LinkSource::SketchCurve { sketch, curve } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| lost(&format!("sketch curve {curve}")))?;
            match kind {
                LinkKind::Intersect => {
                    let pts: Vec<Vec3> = ss.sketch.segs(ci).iter().flat_map(|s| s.polyline(1e-3)).map(|p| ss.plane.to_world(p)).collect();
                    section_of_polyline(plane, &pts)
                }
                _ => project_sketch_curve(&ss.sketch, &ss.plane, ci, plane),
            }
        }
        LinkSource::SketchPoint { sketch, point } => {
            let ss = st.sketch(*sketch).ok_or_else(|| lost(&format!("sketch {sketch}")))?;
            let pi = ss.sketch.resolve_point(point).ok_or_else(|| lost(&format!("sketch point {point}")))?;
            let p = ss.sketch.point(pi).unwrap_or_default();
            vec![LinkGeom::Point(plane.to_local(ss.plane.to_world(p)))]
        }
        LinkSource::Origin => vec![LinkGeom::Point(plane.to_local(Vec3::ZERO))],
        LinkSource::Axis { name } => {
            let d = match name.to_ascii_uppercase().as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                "Z" => Vec3::Z,
                _ => return Err(DocError::Unknown(format!("axis `{name}`"))),
            };
            let o = plane.to_local(Vec3::ZERO);
            let d2 = Vec2::new(d.dot(plane.x), d.dot(plane.y));
            match d2.normalized() {
                Some(u) if d2.len() > 1e-9 => vec![LinkGeom::Line(o - u * AXIS_HALF, o + u * AXIS_HALF)],
                _ => vec![LinkGeom::Point(o)],
            }
        }
        LinkSource::Plane { name } => {
            let pr =
                if Plane::named(name).is_some() { PlaneRef::Origin { name: name.clone() } } else { PlaneRef::Construction { name: name.clone() } };
            let other = doc.resolve_plane(vals, &pr, 0).map_err(|_| lost(&format!("plane {name}")))?;
            plane_trace(plane, &other).into_iter().collect()
        }
    };
    if geom.is_empty() {
        return Err(DocError::Invalid(format!("the {} projects to nothing on this sketch plane", src.describe())));
    }
    Ok(Resolved { geom, source, moved })
}

/// Re-resolve every link of a sketch (in place). Returns warnings for lost, moved or rebuilt
/// links; lost links keep their last geometry.
pub fn refresh_links(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, plane: &Plane, sk: &mut Sketch) -> Vec<String> {
    let mut warn = Vec::new();
    let links = sk.links.clone();
    for l in &links {
        match resolve(doc, vals, st, plane, l.kind, &l.source) {
            Ok(r) => {
                sk.set_link_source(&l.id, r.source);
                match sk.update_link(&l.id, &r.geom) {
                    Ok(true) => warn.push(format!("projected {} changed shape and was rebuilt", l.source.describe())),
                    Ok(false) if r.moved => warn.push(format!("projected {} was re-found on the nearest geometry", l.source.describe())),
                    Ok(false) => {}
                    Err(e) => {
                        sk.set_link_lost(&l.id, true);
                        warn.push(format!("projected {}: {e}", l.source.describe()));
                    }
                }
            }
            Err(e) => {
                sk.set_link_lost(&l.id, true);
                warn.push(format!("projected reference lost: {e}"));
            }
        }
    }
    warn
}

// ---------------------------------------------------------------------------------------------
// Model lookups

/// The body by name (or, when it is gone or unnamed, the one with geometry nearest `at`) and
/// its display mesh.
fn body_mesh(st: &ModelState, name: &str, at: Vec3) -> Result<(usize, std::sync::Arc<Mesh>)> {
    if let Some(i) = st.bodies.iter().position(|b| b.name == name)
        && let Some(b) = st.bodies.get(i)
    {
        return Ok((i, b.mesh()));
    }
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        let m = b.mesh();
        if let Some((_, _, d)) = nearest_face(&m, at)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((i, d));
        }
    }
    let (i, _) = best.ok_or_else(|| DocError::Invalid(format!("body {name} no longer exists")))?;
    let b = st.bodies.get(i).ok_or_else(|| DocError::Invalid(format!("body {name} no longer exists")))?;
    Ok((i, b.mesh()))
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let l2 = ab.len2();
    let t = if l2 > 0.0 { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) } else { 0.0 };
    a + ab * t
}

/// Nearest non-seam edge: (index, closest point, distance).
pub(crate) fn nearest_edge(m: &Mesh, p: Vec3) -> Option<(usize, Vec3, f64)> {
    let mut best: Option<(usize, Vec3, f64)> = None;
    for (i, e) in m.edges.iter().enumerate() {
        if m.seams.get(i).copied().unwrap_or(false) {
            continue;
        }
        for w in e.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let q = closest_on_segment(p, *a, *b);
            let d = q.dist(p);
            if best.is_none_or(|(_, _, bd)| d < bd) {
                best = Some((i, q, d));
            }
        }
    }
    best
}

fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let n = (b - a).cross(c - a);
    let Some(nn) = n.normalized() else { return closest_on_segment(p, a, b) };
    let q = p - nn * (p - a).dot(nn);
    let inside = (b - a).cross(q - a).dot(n) >= 0.0 && (c - b).cross(q - b).dot(n) >= 0.0 && (a - c).cross(q - c).dot(n) >= 0.0;
    if inside {
        return q;
    }
    [closest_on_segment(p, a, b), closest_on_segment(p, b, c), closest_on_segment(p, c, a)]
        .into_iter()
        .min_by(|x, y| x.dist(p).total_cmp(&y.dist(p)))
        .unwrap_or(a)
}

/// Nearest face: (B-rep face index, closest point, distance).
pub(crate) fn nearest_face(m: &Mesh, p: Vec3) -> Option<(u32, Vec3, f64)> {
    let mut best: Option<(u32, Vec3, f64)> = None;
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let q = closest_on_triangle(p, a, b, c);
        let d = q.dist(p);
        if best.is_none_or(|(_, _, bd)| d < bd) {
            best = Some((*f, q, d));
        }
    }
    best
}

// ---------------------------------------------------------------------------------------------
// Projection

fn project_sketch_curve(sk: &Sketch, from: &Plane, ci: usize, to: &Plane) -> Vec<LinkGeom> {
    let map = |p: Vec2| to.to_local(from.to_world(p));
    let parallel = from.normal().cross(to.normal()).len() < 1e-9;
    let flip = from.normal().dot(to.normal()) < 0.0;
    match sk.curves.get(ci).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => match (sk.point(*a), sk.point(*b)) {
            (Some(a), Some(b)) => {
                let (a, b) = (map(a), map(b));
                if a.dist(b) > 1e-9 { vec![LinkGeom::Line(a, b)] } else { vec![LinkGeom::Point(a)] }
            }
            _ => Vec::new(),
        },
        Some(CurveKind::Circle { c, r }) if parallel => sk.point(*c).map(|c| vec![LinkGeom::Circle(map(c), *r)]).unwrap_or_default(),
        Some(CurveKind::Arc { c, a, b }) if parallel => match (sk.point(*c), sk.point(*a), sk.point(*b)) {
            (Some(c), Some(a), Some(b)) => {
                let (c, a, b) = (map(c), map(a), map(b));
                vec![if flip { LinkGeom::Arc { c, a: b, b: a } } else { LinkGeom::Arc { c, a, b } }]
            }
            _ => Vec::new(),
        },
        Some(_) => {
            let pts: Vec<Vec3> = sk.segs(ci).iter().flat_map(|s| s.polyline(1e-3)).map(|p| from.to_world(p)).collect();
            fit_world(to, &pts)
        }
        None => Vec::new(),
    }
}

/// The line where another plane crosses the sketch plane (None when parallel).
fn plane_trace(sk: &Plane, other: &Plane) -> Option<LinkGeom> {
    let m = other.normal();
    let (a, b) = (sk.x.dot(m), sk.y.dot(m));
    let n2 = a * a + b * b;
    if n2 < 1e-12 {
        return None;
    }
    let c = (other.origin - sk.origin).dot(m);
    let p0 = Vec2::new(a, b) * (c / n2);
    let u = Vec2::new(-b, a).normalized()?;
    Some(LinkGeom::Line(p0 - u * AXIS_HALF, p0 + u * AXIS_HALF))
}

/// Project a world polyline and fit lines and arcs to it.
fn fit_world(plane: &Plane, pts: &[Vec3]) -> Vec<LinkGeom> {
    let p2: Vec<Vec2> = pts.iter().map(|p| plane.to_local(*p)).collect();
    let mut out = Vec::new();
    fit_chain(&p2, None, &mut out);
    out
}

/// Where a world polyline crosses the plane.
fn section_of_polyline(plane: &Plane, pts: &[Vec3]) -> Vec<LinkGeom> {
    let mut out: Vec<LinkGeom> = Vec::new();
    for w in pts.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let (ha, hb) = (plane.height(*a), plane.height(*b));
        if (ha > 0.0) != (hb > 0.0) || ha == 0.0 {
            let t = if (ha - hb).abs() < 1e-15 { 0.0 } else { ha / (ha - hb) };
            let p = plane.to_local(a.lerp(*b, t));
            if !out.iter().any(|g| matches!(g, LinkGeom::Point(q) if q.dist(p) < 1e-7)) {
                out.push(LinkGeom::Point(p));
            }
        }
    }
    out
}

/// The outline of a body seen along the sketch normal: mesh edges between triangles facing the
/// sketch and triangles facing away (or edge-on), projected and fitted.
fn silhouette(plane: &Plane, m: &Mesh) -> Vec<LinkGeom> {
    let n = plane.normal();
    let size = m.bounds().diagonal().max(1e-9);
    let q = size * 1e-7;
    let key = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    // Undirected mesh edge → facing flags of the triangles using it.
    let mut edges: BTreeMap<((i64, i64, i64), (i64, i64, i64)), (Vec3, Vec3, Vec<bool>)> = BTreeMap::new();
    for t in &m.triangles {
        let Some([a, b, c]) = m.tri(t) else { continue };
        let fnrm = (b - a).cross(c - a);
        let Some(un) = fnrm.normalized() else { continue };
        let front = un.dot(n) > 1e-6;
        for (p, r) in [(a, b), (b, c), (c, a)] {
            let (kp, kr) = (key(p), key(r));
            let k = if kp <= kr { (kp, kr) } else { (kr, kp) };
            edges.entry(k).or_insert_with(|| (p, r, Vec::new())).2.push(front);
        }
    }
    let segs: Vec<(Vec2, Vec2)> = edges
        .values()
        .filter(|(_, _, f)| {
            let fronts = f.iter().filter(|x| **x).count();
            fronts > 0 && fronts < f.len() || (f.len() == 1 && fronts == 1)
        })
        .map(|(a, b, _)| (plane.to_local(*a), plane.to_local(*b)))
        .collect();
    fit_segments(&segs, size)
}

/// Section of a mesh (or one of its faces) with the sketch plane.
fn section(plane: &Plane, m: &Mesh, face: Option<u32>) -> Vec<LinkGeom> {
    let size = m.bounds().diagonal().max(1e-9);
    let q = size * 1e-7;
    let key = |p: Vec3| ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64);
    // Surface normal at each mesh vertex where the surface is smooth (None at creases).
    let mut normals: BTreeMap<(i64, i64, i64), Option<Vec3>> = BTreeMap::new();
    for (t, _) in m.triangles.iter().zip(&m.tri_face) {
        for k in t {
            let (Some(p), Some(n)) = (m.positions.get(*k as usize), m.normals.get(*k as usize)) else { continue };
            let Some(n) = n.normalized() else { continue };
            normals
                .entry(key(*p))
                .and_modify(|e| {
                    if e.is_some_and(|o| o.dot(n) < 1.0 - 1e-6) {
                        *e = None;
                    }
                })
                .or_insert(Some(n));
        }
    }
    // Where the plane crosses a mesh edge: on a cubic through the end points whose end
    // tangents lie in the surface's tangent planes (so sections of curved faces land on the
    // surface, not on the chord), computed once per edge so neighbours agree.
    let exact: std::cell::RefCell<Vec<Vec2>> = std::cell::RefCell::new(Vec::new());
    let crossing = |a: Vec3, b: Vec3| -> Option<Vec3> {
        let (a, b) = if key(a) <= key(b) { (a, b) } else { (b, a) };
        let (ha, hb) = (plane.height(a), plane.height(b));
        if (ha > 0.0) == (hb > 0.0) {
            return None;
        }
        let d = b - a;
        // Tangent length: the arc length of a circle through both ends with these normals.
        let (na, nb) = (normals.get(&key(a)).copied().flatten(), normals.get(&key(b)).copied().flatten());
        let l = match (na, nb) {
            (Some(x), Some(y)) => {
                let half = 0.5 * x.dot(y).clamp(-1.0, 1.0).acos();
                if half > 1e-9 { d.len() * half / half.sin() } else { d.len() }
            }
            _ => d.len(),
        };
        let tangent = |n: Option<Vec3>| -> Vec3 {
            match n {
                Some(n) => (d - n * d.dot(n)).normalized().map(|t| t * l).unwrap_or(d),
                None => d,
            }
        };
        // Along a ruling (same normal at both ends: planes, cylinder and cone generators) the
        // edge lies on the surface and the straight crossing is exact.
        if let (Some(x), Some(y)) = (na, nb)
            && x.dot(y) > 1.0 - 1e-12
        {
            let p = a.lerp(b, ha / (ha - hb));
            exact.borrow_mut().push(plane.to_local(p));
            return Some(p);
        }
        let (ta, tb) = (tangent(na), tangent(nb));
        let h = |t: f64| {
            let (t2, t3) = (t * t, t * t * t);
            a * (2.0 * t3 - 3.0 * t2 + 1.0) + ta * (t3 - 2.0 * t2 + t) + b * (-2.0 * t3 + 3.0 * t2) + tb * (t3 - t2)
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (plane.height(h(mid)) > 0.0) == (ha > 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        Some(h(0.5 * (lo + hi)))
    };
    let mut segs: Vec<(Vec2, Vec2)> = Vec::new();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        if face.is_some_and(|x| x != *f) {
            continue;
        }
        let Some(tri) = m.tri(t) else { continue };
        let mut pts: Vec<Vec3> = Vec::new();
        for (i, j) in [(0usize, 1usize), (1, 2), (2, 0)] {
            let (Some(&pi), Some(&pj)) = (tri.get(i), tri.get(j)) else { continue };
            if let Some(x) = crossing(pi, pj) {
                pts.push(x);
            }
        }
        if let [a, b] = pts[..] {
            let (a, b) = (plane.to_local(a), plane.to_local(b));
            if a.dist(b) > size * 1e-9 {
                segs.push((a, b));
            }
        }
    }
    let exact = exact.into_inner();
    let loose = size * 2e-3;
    fit_segments(&segs, size).into_iter().map(|g| refine_round(g, &exact, loose)).collect()
}

/// Re-fit a circle or arc through the exact section points near it, when there are enough.
fn refine_round(g: LinkGeom, exact: &[Vec2], tol: f64) -> LinkGeom {
    let (c, r) = match g {
        LinkGeom::Circle(c, r) => (c, r),
        LinkGeom::Arc { c, a, .. } => (c, c.dist(a)),
        _ => return g,
    };
    let near: Vec<Vec2> = exact.iter().filter(|p| (p.dist(c) - r).abs() <= tol).copied().collect();
    // Arcs: only points within the arc's sweep.
    let near: Vec<Vec2> = match g {
        LinkGeom::Arc { c, a, b } => {
            let (s0, sw) = ((a - c).angle(), ((b - c).angle() - (a - c).angle()).rem_euclid(std::f64::consts::TAU));
            near.into_iter().filter(|p| ((*p - c).angle() - s0).rem_euclid(std::f64::consts::TAU) <= sw + 1e-9).collect()
        }
        _ => near,
    };
    let Some((c2, r2)) = (near.len() >= 4).then(|| ls_circle(&near)).flatten() else { return g };
    if c2.dist(c) > tol || (r2 - r).abs() > tol || near.iter().any(|p| (p.dist(c2) - r2).abs() > tol * 1e-3) {
        return g;
    }
    let on = |q: Vec2| c2 + (q - c).normalized().unwrap_or(Vec2::X) * r2;
    match g {
        LinkGeom::Arc { a, b, .. } => LinkGeom::Arc { c: c2, a: on(a), b: on(b) },
        _ => LinkGeom::Circle(c2, r2),
    }
}

/// Chain loose 2D segments into polylines (merging coincident ends) and fit each chain.
fn fit_segments(segs: &[(Vec2, Vec2)], size: f64) -> Vec<LinkGeom> {
    // Mesh chords stray from the surface by up to the display tolerance (1/1000 of the size).
    let loose = Some(size * 2e-3);
    let q = size * 1e-7;
    let key = |p: Vec2| ((p.x / q).round() as i64, (p.y / q).round() as i64);
    let mut nodes: BTreeMap<(i64, i64), (Vec2, Vec<usize>)> = BTreeMap::new();
    let mut uniq: Vec<((i64, i64), (i64, i64))> = Vec::new();
    for (a, b) in segs {
        let (ka, kb) = (key(*a), key(*b));
        if ka == kb {
            continue;
        }
        let k = if ka <= kb { (ka, kb) } else { (kb, ka) };
        if uniq.contains(&k) {
            continue;
        }
        let si = uniq.len();
        uniq.push(k);
        nodes.entry(ka).or_insert_with(|| (*a, Vec::new())).1.push(si);
        nodes.entry(kb).or_insert_with(|| (*b, Vec::new())).1.push(si);
        if uniq.len() > 200_000 {
            break;
        }
    }
    let mut used = vec![false; uniq.len()];
    let mut out = Vec::new();
    let walk = |start: (i64, i64), used: &mut Vec<bool>| -> Vec<Vec2> {
        let mut chain = vec![nodes.get(&start).map(|n| n.0).unwrap_or_default()];
        let mut cur = start;
        while let Some((_, adj)) = nodes.get(&cur) {
            let next = adj.iter().copied().find(|s| !used.get(*s).copied().unwrap_or(true));
            let Some(s) = next else { break };
            if let Some(u) = used.get_mut(s) {
                *u = true;
            }
            let Some(&(ka, kb)) = uniq.get(s) else { break };
            cur = if ka == cur { kb } else { ka };
            chain.push(nodes.get(&cur).map(|n| n.0).unwrap_or_default());
            // Stop at junctions so each chain is a simple path.
            if nodes.get(&cur).is_some_and(|n| n.1.len() != 2) {
                break;
            }
        }
        chain
    };
    // Open chains start at ends and junctions; what is left are closed loops.
    let starts: Vec<(i64, i64)> = nodes.iter().filter(|(_, n)| n.1.len() != 2).map(|(k, _)| *k).collect();
    for s in starts {
        while nodes.get(&s).is_some_and(|n| n.1.iter().any(|e| !used.get(*e).copied().unwrap_or(true))) {
            let c = walk(s, &mut used);
            fit_chain(&c, loose, &mut out);
        }
    }
    let keys: Vec<(i64, i64)> = nodes.keys().copied().collect();
    for s in keys {
        while nodes.get(&s).is_some_and(|n| n.1.iter().any(|e| !used.get(*e).copied().unwrap_or(true))) {
            let c = walk(s, &mut used);
            fit_chain(&c, loose, &mut out);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Fitting

fn circumcircle(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-18 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let o = Vec2::new((a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d, (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d);
    let r = o.dist(a);
    (o.is_finite() && r.is_finite() && r < 1e7).then_some((o, r))
}

/// Least-squares circle (algebraic fit, in coordinates centred on the points' mean).
fn ls_circle(p: &[Vec2]) -> Option<(Vec2, f64)> {
    let n = p.len() as f64;
    if p.len() < 3 {
        return None;
    }
    let m = p.iter().fold(Vec2::ZERO, |a, q| a + *q) / n;
    // x² + y² + D x + E y + F = 0 → normal equations.
    let (mut a, mut r) = ([[0.0f64; 3]; 3], [0.0f64; 3]);
    for q in p {
        let (x, y) = (q.x - m.x, q.y - m.y);
        let row = [x, y, 1.0];
        let rhs = -(x * x + y * y);
        for i in 0..3 {
            for j in 0..3 {
                a[i][j] += row[i] * row[j];
            }
            r[i] += row[i] * rhs;
        }
    }
    let det = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let d0 = det(a);
    if d0.abs() < 1e-30 {
        return None;
    }
    let mut sol = [0.0; 3];
    for (k, s) in sol.iter_mut().enumerate() {
        let mut mk = a;
        for i in 0..3 {
            mk[i][k] = r[i];
        }
        *s = det(mk) / d0;
    }
    let c = Vec2::new(-sol[0] / 2.0, -sol[1] / 2.0);
    let rr = c.len2() - sol[2];
    let c = c + m;
    (rr > 0.0 && c.is_finite() && rr.sqrt() < 1e7).then(|| (c, rr.sqrt()))
}

fn fits_line(p: &[Vec2], tol: f64) -> bool {
    let (Some(a), Some(b)) = (p.first(), p.last()) else { return false };
    let d = *b - *a;
    let l = d.len();
    if l <= tol {
        return false;
    }
    let u = d / l;
    let mut last = -tol;
    p.iter().all(|q| {
        let t = (*q - *a).dot(u);
        let ok = u.cross(*q - *a).abs() <= tol && t >= last - tol && t <= l + tol;
        last = t;
        ok
    })
}

/// Circle through the chain (centre, radius, signed sweep from the first to the last point).
/// `exact`: the points lie on the curve (edge tessellation), else they are approximate
/// (mesh sections and silhouettes) and the circle is a least-squares fit.
fn fits_circle(p: &[Vec2], tol: f64, exact: bool) -> Option<(Vec2, f64, f64)> {
    let n = p.len();
    if n < 3 || (!exact && n < 5) {
        return None;
    }
    let closed = p.first()?.dist(*p.last()?) <= tol;
    let (c, r) = if exact {
        let (i1, i2) = if closed { (n / 3, 2 * n / 3) } else { (n / 2, n - 1) };
        circumcircle(*p.first()?, *p.get(i1)?, *p.get(i2)?)?
    } else {
        ls_circle(p)?
    };
    if r > 1e6 || p.iter().any(|q| (q.dist(c) - r).abs() > tol) {
        return None;
    }
    let mut sweep = 0.0;
    for w in p.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let mut d = (*b - c).angle() - (*a - c).angle();
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        // Chords of a tessellated circle are short; points jumping around the circle (a
        // polygon inscribed in it) are not an arc.
        if d.abs() > 1.0 || (sweep != 0.0 && d * sweep < 0.0) {
            return None;
        }
        sweep += d;
    }
    Some((c, r, sweep))
}

fn arc_geom(p: &[Vec2], c: Vec2, r: f64, sweep: f64, tol: f64) -> Option<LinkGeom> {
    let (a, b) = (*p.first()?, *p.last()?);
    if sweep.abs() >= std::f64::consts::TAU - 1e-3 || a.dist(b) <= tol {
        return Some(LinkGeom::Circle(c, r));
    }
    // Ends exactly on the circle.
    let on = |q: Vec2| c + (q - c).normalized().unwrap_or(Vec2::X) * r;
    let (a, b) = (on(a), on(b));
    Some(if sweep > 0.0 { LinkGeom::Arc { c, a, b } } else { LinkGeom::Arc { c, a: b, b: a } })
}

/// Fit lines and circular arcs to a 2D polyline: the whole chain when it is one line or arc,
/// else greedy maximal pieces. `loose`: the points are approximate (within `loose` mm), and
/// sharp corners always split the chain.
pub(crate) fn fit_chain(raw: &[Vec2], loose: Option<f64>, out: &mut Vec<LinkGeom>) {
    let mut p: Vec<Vec2> = Vec::with_capacity(raw.len());
    let scale = raw.iter().fold(0.0_f64, |m, q| m.max(q.x.abs()).max(q.y.abs())).max(1.0);
    let ext = match (
        raw.iter().map(|q| q.x).reduce(f64::min),
        raw.iter().map(|q| q.x).reduce(f64::max),
        raw.iter().map(|q| q.y).reduce(f64::min),
        raw.iter().map(|q| q.y).reduce(f64::max),
    ) {
        (Some(x0), Some(x1), Some(y0), Some(y1)) => (x1 - x0).max(y1 - y0),
        _ => return,
    };
    let exact = loose.is_none();
    let tol = loose.unwrap_or(1e-6 * scale.max(ext)).max(1e-9);
    for q in raw {
        if !q.is_finite() {
            return;
        }
        if p.last().is_none_or(|l| l.dist(*q) > tol * 1e-3) {
            p.push(*q);
        }
    }
    if p.len() > MAX_CHAIN {
        let step = p.len().div_ceil(MAX_CHAIN);
        let last = p.last().copied();
        p = p.iter().step_by(step).copied().collect();
        if let Some(l) = last
            && p.last() != Some(&l)
        {
            p.push(l);
        }
    }
    if p.len() < 2 || ext <= tol {
        return;
    }
    if !exact {
        // Split at corners (turns sharper than 30 degrees); a closed chain starts at one.
        let n = p.len();
        let closed = n > 3 && p.first().zip(p.last()).is_some_and(|(a, b)| a.dist(*b) <= tol);
        let turn = |i: usize| -> f64 {
            let prev = if i == 0 { if closed { n.saturating_sub(2) } else { return 0.0 } } else { i - 1 };
            let next = if i + 1 >= n { if closed { 1 } else { return 0.0 } } else { i + 1 };
            match (p.get(prev), p.get(i), p.get(next)) {
                (Some(a), Some(b), Some(c)) => match ((*b - *a).normalized(), (*c - *b).normalized()) {
                    (Some(u), Some(v)) => u.cross(v).atan2(u.dot(v)).abs(),
                    _ => 0.0,
                },
                _ => 0.0,
            }
        };
        let corners: Vec<usize> = (0..n).filter(|i| turn(*i) > 30f64.to_radians()).collect();
        let cut = |a: usize, b: usize, out: &mut Vec<LinkGeom>| {
            if let Some(s) = p.get(a..=b) {
                fit_piece(s, tol, exact, out);
            }
        };
        if closed && let Some(&c0) = corners.first() {
            // Rotate so the loop starts and ends at a corner.
            let mut q: Vec<Vec2> = p.get(c0..n.saturating_sub(1)).unwrap_or_default().to_vec();
            q.extend(p.get(..=c0).unwrap_or_default());
            let m = q.len();
            let shift = |i: usize| (i + n - 1 - c0) % (n - 1);
            let mut cs: Vec<usize> = corners.iter().map(|c| shift(*c)).filter(|c| *c > 0 && *c < m - 1).collect();
            cs.sort_unstable();
            let mut start = 0;
            for c in cs.into_iter().chain([m - 1]) {
                if let Some(s) = q.get(start..=c) {
                    fit_piece(s, tol, exact, out);
                }
                start = c;
            }
            return;
        }
        let mut start = 0;
        for c in corners.into_iter().filter(|c| *c > 0 && *c < n - 1).chain([n - 1]) {
            cut(start, c, out);
            start = c;
        }
        return;
    }
    fit_piece(&p, tol, exact, out);
}

fn fit_piece(p: &[Vec2], tol: f64, exact: bool, out: &mut Vec<LinkGeom>) {
    if p.len() < 2 {
        return;
    }
    if fits_line(p, tol) {
        if let (Some(a), Some(b)) = (p.first(), p.last()) {
            out.push(LinkGeom::Line(*a, *b));
        }
        return;
    }
    if let Some((c, r, sw)) = fits_circle(p, tol, exact)
        && let Some(g) = arc_geom(p, c, r, sw, tol)
    {
        out.push(g);
        return;
    }
    // Greedy pieces.
    let mut i = 0;
    let n = p.len();
    let mut guard = 0;
    while i + 1 < n && guard < MAX_CHAIN * 2 {
        guard += 1;
        let mut jl = i + 1;
        while jl + 1 < n && p.get(i..=jl + 1).is_some_and(|s| fits_line(s, tol)) {
            jl += 1;
        }
        let mut jc = i + 1;
        let mut arc = None;
        while jc + 1 < n {
            match p.get(i..=jc + 1).and_then(|s| fits_circle(s, tol, exact)) {
                Some(f) => {
                    arc = Some(f);
                    jc += 1;
                }
                None => break,
            }
        }
        if jc >= jl + 2
            && jc - i >= 3
            && let Some((c, r, sw)) = arc
            && let Some(g) = p.get(i..=jc).and_then(|s| arc_geom(s, c, r, sw, tol))
        {
            out.push(g);
            i = jc;
        } else {
            if let (Some(a), Some(b)) = (p.get(i), p.get(jl)) {
                out.push(LinkGeom::Line(*a, *b));
            }
            i = jl;
        }
    }
}
