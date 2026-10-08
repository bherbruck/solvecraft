//! STEP topology (ISO 10303-42 topology entities) to kernel solids: breps, shells, faces, loops,
//! edges and vertices. A face that cannot be built is left out and reported; the rest of the
//! body is still imported (as an open body when a face is missing).

use std::collections::HashMap;

use mt::{BoundedCurve, EuclideanSpace, InnerSpace, MetricSpace, ParametricCurve};
use truck_modeling as mt;

use super::Ctx;
use super::geom::{self, R, entity};
use super::p21::Param;
use crate::guard;

/// Most faces in one body.
const MAX_FACES: usize = 500_000;
/// Most edges in one loop.
const MAX_LOOP: usize = 100_000;

/// A body read from a brep entity.
pub(crate) struct BrepOut {
    pub solid: mt::Solid,
    /// Faces left out (with the reason).
    pub failed: Vec<String>,
    /// The shells are closed (a solid) rather than open (missing faces, or a surface model).
    pub closed: bool,
    /// Faces read from the file (before band faces are split for the kernel).
    pub file_faces: usize,
    /// The file face of each kernel face, in face order.
    pub face_origin: Vec<u64>,
}

#[derive(Default)]
struct Builder {
    vertices: HashMap<u64, mt::Vertex>,
    edges: HashMap<u64, Vec<mt::Edge>>,
    poly_edges: HashMap<(u64, u64), mt::Edge>,
    faces: usize,
}

fn refs(p: Option<&Param>) -> Vec<u64> {
    p.and_then(Param::as_list).map(|v| v.iter().filter_map(Param::as_ref_id).collect()).unwrap_or_default()
}

fn rid(p: Option<&Param>, what: &str) -> R<u64> {
    p.and_then(Param::as_ref_id).ok_or_else(|| format!("{what}: expected a reference"))
}

impl Builder {
    fn vertex(&mut self, cx: &Ctx, id: u64) -> R<mt::Vertex> {
        if let Some(v) = self.vertices.get(&id) {
            return Ok(v.clone());
        }
        let e = entity(cx, id)?;
        if e.name() != "VERTEX_POINT" {
            return Err(format!("#{id}: expected VERTEX_POINT, found {}", e.name()));
        }
        let p = geom::point(cx, rid(e.params().get(1), "VERTEX_POINT")?)?;
        let v = mt::Vertex::new(p);
        self.vertices.insert(id, v.clone());
        Ok(v)
    }

    /// Kernel edges (start to end) of an EDGE_CURVE.
    fn edge_curve(&mut self, cx: &Ctx, id: u64) -> R<Vec<mt::Edge>> {
        if let Some(e) = self.edges.get(&id) {
            return Ok(e.clone());
        }
        let e = entity(cx, id)?;
        if e.name() != "EDGE_CURVE" {
            return Err(format!("#{id}: unsupported edge {}", e.name()));
        }
        let p = e.params();
        let (vs_id, ve_id) = (rid(p.get(1), "edge start")?, rid(p.get(2), "edge end")?);
        let vs = self.vertex(cx, vs_id)?;
        let ve = self.vertex(cx, ve_id)?;
        let sense = p.get(4).and_then(Param::as_bool).unwrap_or(true);
        let g = geom::curve(cx, rid(p.get(3), "edge geometry")?, 0)?;
        let closed = vs_id == ve_id;
        let pieces = guard("step edge", || geom::edge_pieces(&g, vs.point(), ve.point(), sense, closed, cx.tol).map_err(crate::KernelError::Failed))
            .map_err(|e| e.to_string())?;
        let n = pieces.len();
        let mut out = Vec::with_capacity(n);
        let mut cur = vs.clone();
        for (i, c) in pieces.into_iter().enumerate() {
            let next = if i + 1 == n { ve.clone() } else { mt::Vertex::new(c.back()) };
            out.push(mt::Edge::new_unchecked(&cur, &next, c));
            cur = next;
        }
        self.edges.insert(id, out.clone());
        Ok(out)
    }

    /// Edges of an ORIENTED_EDGE (or a bare EDGE_CURVE) in loop order.
    fn oriented_edge(&mut self, cx: &Ctx, id: u64, depth: usize) -> R<Vec<mt::Edge>> {
        if depth > 4 {
            return Err("oriented edges nested too deeply".into());
        }
        let e = entity(cx, id)?;
        match e.name() {
            "ORIENTED_EDGE" => {
                let p = e.params();
                let inner = self.oriented_edge(cx, rid(p.get(3), "ORIENTED_EDGE")?, depth + 1)?;
                if p.get(4).and_then(Param::as_bool).unwrap_or(true) { Ok(inner) } else { Ok(inner.iter().rev().map(mt::Edge::inverse).collect()) }
            }
            _ => self.edge_curve(cx, id),
        }
    }

    fn poly_loop(&mut self, cx: &Ctx, pts: &[u64]) -> R<Vec<mt::Edge>> {
        let n = pts.len();
        if !(3..=MAX_LOOP).contains(&n) {
            return Err(format!("polygon loop with {n} points"));
        }
        let mut verts = Vec::with_capacity(n);
        for &id in pts {
            if let Some(v) = self.vertices.get(&id) {
                verts.push(v.clone());
            } else {
                let v = mt::Vertex::new(geom::point(cx, id)?);
                self.vertices.insert(id, v.clone());
                verts.push(v);
            }
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let (Some(&a), Some(&b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
            if let Some(e) = self.poly_edges.get(&(b, a)) {
                out.push(e.inverse());
                continue;
            }
            let (Some(va), Some(vb)) = (verts.get(i), verts.get((i + 1) % n)) else { continue };
            if va.point().distance(vb.point()) < 1e-12 {
                return Err("polygon loop with repeated points".into());
            }
            let e = mt::Edge::try_new(va, vb, mt::Curve::Line(mt::Line(va.point(), vb.point()))).map_err(|e| format!("polygon edge: {e}"))?;
            self.poly_edges.insert((a, b), e.clone());
            out.push(e);
        }
        Ok(out)
    }

    /// A face bound as a wire (oriented by the bound's orientation), or `None` for a vertex loop.
    fn bound(&mut self, cx: &Ctx, id: u64) -> R<Option<mt::Wire>> {
        let b = entity(cx, id)?;
        if !matches!(b.name(), "FACE_BOUND" | "FACE_OUTER_BOUND") {
            return Err(format!("#{id}: unsupported face bound {}", b.name()));
        }
        let lp_id = rid(b.params().get(1), "face bound loop")?;
        let orient = b.params().get(2).and_then(Param::as_bool).unwrap_or(true);
        let lp = entity(cx, lp_id)?;
        let edges = match lp.name() {
            "EDGE_LOOP" => {
                let ids = refs(lp.params().get(1));
                if ids.is_empty() || ids.len() > MAX_LOOP {
                    return Err(format!("edge loop with {} edges", ids.len()));
                }
                let mut v = Vec::new();
                for oe in ids {
                    v.extend(self.oriented_edge(cx, oe, 0)?);
                }
                v
            }
            "POLY_LOOP" => self.poly_loop(cx, &refs(lp.params().get(1)))?,
            "VERTEX_LOOP" => return Ok(None),
            n => return Err(format!("unsupported loop {n}")),
        };
        let mut w: mt::Wire = edges.into();
        if !w.is_closed() {
            return Err("face boundary is not a closed loop".into());
        }
        if !orient {
            w.invert();
        }
        Ok(Some(w))
    }

    /// A face (ADVANCED_FACE, FACE_SURFACE, ORIENTED_FACE or a polygonal FACE).
    fn face(&mut self, cx: &Ctx, id: u64, depth: usize) -> R<mt::Face> {
        if depth > 4 {
            return Err("oriented faces nested too deeply".into());
        }
        self.faces += 1;
        if self.faces > MAX_FACES {
            return Err(format!("more than {MAX_FACES} faces"));
        }
        let e = entity(cx, id)?;
        let p = e.params();
        if e.name() == "ORIENTED_FACE" {
            let mut f = self.face(cx, rid(p.get(2), "ORIENTED_FACE")?, depth + 1)?;
            if !p.get(3).and_then(Param::as_bool).unwrap_or(true) {
                f.invert();
            }
            return Ok(f);
        }
        if !matches!(e.name(), "ADVANCED_FACE" | "FACE_SURFACE" | "FACE") {
            return Err(format!("unsupported face {}", e.name()));
        }
        let mut wires = Vec::new();
        for b in refs(p.get(1)) {
            if let Some(w) = self.bound(cx, b)? {
                wires.push(w);
            }
        }
        if wires.is_empty() {
            return Err("face without boundary edges".into());
        }
        let extent: Vec<mt::Point3> = wires
            .iter()
            .flat_map(|w| w.edge_iter())
            .flat_map(|e| {
                let c = e.curve();
                let (t0, t1) = c.range_tuple();
                (0..=8).map(move |i| c.subs(t0 + (t1 - t0) * i as f64 / 8.0)).collect::<Vec<_>>()
            })
            .collect();
        // Seam edges: used twice by the face's loops.
        let mut uses: HashMap<mt::EdgeID, usize> = HashMap::new();
        for e in wires.iter().flat_map(|w| w.edge_iter()) {
            *uses.entry(e.id()).or_default() += 1;
        }
        let mut seen = std::collections::HashSet::new();
        let seam: Vec<mt::Point3> = wires
            .iter()
            .flat_map(|w| w.edge_iter())
            .filter(|e| uses.get(&e.id()).copied().unwrap_or(0) > 1 && seen.insert(e.id()))
            .flat_map(|e| {
                let c = e.curve();
                let (t0, t1) = c.range_tuple();
                (0..=8).map(move |i| c.subs(t0 + (t1 - t0) * i as f64 / 8.0)).collect::<Vec<_>>()
            })
            .collect();
        let surface = if e.name() == "FACE" {
            plane_through(&extent).ok_or("polygonal face without a plane")?
        } else {
            let sid = rid(p.get(2), "face geometry")?;
            let sense = p.get(3).and_then(Param::as_bool).unwrap_or(true);
            guard("step surface", || geom::surface(cx, sid, &extent, &seam, sense, 0).map_err(crate::KernelError::Failed))
                .map_err(|e| e.to_string())?
        };
        // Loops on a surface of revolution start away from the axis: a pole (cone apex, sphere
        // pole) has no definite rotation parameter, and the mesher takes the first point's
        // parameter as the anchor for the rest of the loop.
        let wires: Vec<mt::Wire> = match &surface {
            mt::Surface::RevolutedCurve(rc) => {
                let (o, a) = (rc.entity().origin(), rc.entity().axis());
                let dist = |e: &mt::Edge| {
                    let v = e.front().point() - o;
                    (v - a * v.dot(a)).magnitude()
                };
                wires
                    .into_iter()
                    .map(|w| {
                        let edges: Vec<mt::Edge> = w.edge_iter().cloned().collect();
                        let start = edges.iter().enumerate().max_by(|x, y| dist(x.1).total_cmp(&dist(y.1))).map(|(i, _)| i).unwrap_or(0);
                        edges.iter().skip(start).chain(edges.iter().take(start)).cloned().collect::<Vec<_>>().into()
                    })
                    .collect()
            }
            // A loop passing a vertex twice (a figure eight through a pole, as written for a
            // torus whose tube touches its axis) starts at a vertex it passes once.
            _ => wires
                .into_iter()
                .map(|w| {
                    let edges: Vec<mt::Edge> = w.edge_iter().cloned().collect();
                    let passes = |e: &mt::Edge| edges.iter().filter(|x| x.front().id() == e.front().id()).count();
                    match edges.first().map(passes) {
                        Some(n) if n > 1 => {
                            let start = edges.iter().position(|e| passes(e) == 1).unwrap_or(0);
                            edges.iter().skip(start).chain(edges.iter().take(start)).cloned().collect::<Vec<_>>().into()
                        }
                        _ => w,
                    }
                })
                .collect(),
        };
        match mt::Face::try_new(wires.clone(), surface.clone()) {
            Ok(f) => Ok(f),
            // Faces on periodic surfaces run along a seam edge twice: valid in STEP.
            Err(truck_topology::errors::Error::NotSimpleWire) => Ok(mt::Face::new_unchecked(wires, surface)),
            Err(e) => Err(format!("face: {e}")),
        }
    }

    fn shell(&mut self, cx: &Ctx, id: u64, failed: &mut Vec<String>) -> R<(mt::Shell, bool, usize, Vec<u64>)> {
        let e = entity(cx, id)?;
        let p = e.params();
        let (faces, flip, closed) = match e.name() {
            "CLOSED_SHELL" => (refs(p.get(1)), false, true),
            "OPEN_SHELL" => (refs(p.get(1)), false, false),
            "ORIENTED_CLOSED_SHELL" | "ORIENTED_OPEN_SHELL" => {
                let inner = entity(cx, rid(p.get(2), "oriented shell")?)?;
                (refs(inner.params().get(1)), !p.get(3).and_then(Param::as_bool).unwrap_or(true), e.name() == "ORIENTED_CLOSED_SHELL")
            }
            n => return Err(format!("unsupported shell {n}")),
        };
        if faces.len() > MAX_FACES {
            return Err(format!("shell with {} faces", faces.len()));
        }
        let mut out = Vec::with_capacity(faces.len());
        let mut origin = Vec::with_capacity(faces.len());
        for f in faces {
            match self.face(cx, f, 0) {
                Ok(mut face) => {
                    if flip {
                        face.invert();
                    }
                    out.push(face);
                    origin.push(f);
                }
                Err(err) => {
                    if self.faces > MAX_FACES {
                        return Err(err);
                    }
                    failed.push(format!("face #{f}: {err}"));
                }
            }
        }
        if out.is_empty() {
            return Err("no face could be imported".into());
        }
        let read = out.len();
        let out = super::split::split_bands(out, &mut origin, failed);
        Ok((out.into(), closed, read, origin))
    }
}

/// Plane through a polygon (Newell's normal).
fn plane_through(pts: &[mt::Point3]) -> Option<mt::Surface> {
    let n = pts.len();
    let mut nrm = mt::Vector3::new(0.0, 0.0, 0.0);
    let mut c = mt::Vector3::new(0.0, 0.0, 0.0);
    for i in 0..n {
        let (a, b) = (pts.get(i)?, pts.get((i + 1) % n)?);
        nrm.x += (a.y - b.y) * (a.z + b.z);
        nrm.y += (a.z - b.z) * (a.x + b.x);
        nrm.z += (a.x - b.x) * (a.y + b.y);
        c += a.to_homogeneous().truncate();
    }
    let len = nrm.magnitude();
    if !(len > 1e-12) || n == 0 {
        return None;
    }
    let z = nrm / len;
    let o = mt::Point3::from_vec(c / n as f64);
    let a = if z.x.abs() < 0.9 { mt::Vector3::unit_x() } else { mt::Vector3::unit_y() };
    let x = (a - z * a.dot(z)).normalize();
    let y = z.cross(x);
    Some(mt::Surface::Plane(mt::Plane::new(o, o + x, o + y)))
}

/// Read a solid-like representation item: MANIFOLD_SOLID_BREP, BREP_WITH_VOIDS, FACETED_BREP,
/// or a SHELL_BASED_SURFACE_MODEL (closed shells give solids, open ones open bodies).
pub(crate) fn read_item(cx: &Ctx, id: u64) -> R<BrepOut> {
    let e = entity(cx, id)?;
    let p = e.params();
    let mut b = Builder::default();
    let mut failed = Vec::new();
    let mut shells = Vec::new();
    let mut closed = true;
    let mut file_faces = 0;
    // The file face each kernel face comes from (in face order).
    let mut face_origin = Vec::new();
    match e.name() {
        "MANIFOLD_SOLID_BREP" | "FACETED_BREP" | "BREP_WITH_VOIDS" => {
            let (s, c, n, o) = b.shell(cx, rid(p.get(1), "brep outer shell")?, &mut failed)?;
            closed &= c;
            file_faces += n;
            shells.push(s);
            face_origin.extend(o);
            if e.name() == "BREP_WITH_VOIDS" {
                for v in refs(p.get(2)) {
                    match b.shell(cx, v, &mut failed) {
                        Ok((s, c, n, o)) => {
                            closed &= c;
                            file_faces += n;
                            shells.push(s);
                            face_origin.extend(o);
                        }
                        Err(err) => failed.push(format!("void shell #{v}: {err}")),
                    }
                }
            }
        }
        "SHELL_BASED_SURFACE_MODEL" => {
            for s in refs(p.get(1)) {
                match b.shell(cx, s, &mut failed) {
                    Ok((s, c, n, o)) => {
                        closed &= c;
                        file_faces += n;
                        shells.push(s);
                        face_origin.extend(o);
                    }
                    Err(err) => failed.push(format!("shell #{s}: {err}")),
                }
            }
        }
        n => return Err(format!("unsupported item {n}")),
    }
    if shells.is_empty() {
        return Err(failed.first().cloned().unwrap_or_else(|| "no shells".into()));
    }
    let solid = match guard("step solid", || mt::Solid::try_new(shells.clone()).map_err(|e| crate::KernelError::Failed(e.to_string()))) {
        Ok(s) => s,
        Err(_) => {
            closed = false;
            mt::Solid::new_unchecked(shells)
        }
    };
    Ok(BrepOut { solid, failed, closed, file_faces, face_origin })
}
