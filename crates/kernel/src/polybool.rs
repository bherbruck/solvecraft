//! Exact-plane booleans for bodies whose faces are all planar (BSP-tree CSG on polygons, in
//! the classic style), used when the B-rep boolean fails — typically on coincident faces, such
//! as a wall flush with the side of its base. The polygons are rebuilt into a B-rep: coplanar
//! pieces are merged into faces, T-junctions are split so neighbouring faces share edges.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, p3};
use crate::{BoolOp, KernelError, Result, guard};

const MAX_DEPTH: usize = 4000;
const MAX_POLYS: usize = 200_000;

#[derive(Clone, Copy, Debug)]
struct PlaneEq {
    n: Vec3,
    w: f64,
}

impl PlaneEq {
    fn flipped(self) -> PlaneEq {
        PlaneEq { n: -self.n, w: -self.w }
    }
}

#[derive(Clone, Debug)]
struct Poly {
    v: Vec<Vec3>,
    plane: PlaneEq,
}

impl Poly {
    fn flip(&mut self) {
        self.v.reverse();
        self.plane = self.plane.flipped();
    }
}

const COPLANAR: u8 = 0;
const FRONT: u8 = 1;
const BACK: u8 = 2;
const SPANNING: u8 = 3;

/// Split `p` by `pl` into the four buckets.
fn split(pl: PlaneEq, p: &Poly, eps: f64, cf: &mut Vec<Poly>, cb: &mut Vec<Poly>, f: &mut Vec<Poly>, b: &mut Vec<Poly>) {
    let mut ptype = 0u8;
    let types: Vec<u8> =
        p.v.iter()
            .map(|v| {
                let t = pl.n.dot(*v) - pl.w;
                let ty = if t < -eps {
                    BACK
                } else if t > eps {
                    FRONT
                } else {
                    COPLANAR
                };
                ptype |= ty;
                ty
            })
            .collect();
    match ptype {
        COPLANAR => {
            if pl.n.dot(p.plane.n) > 0.0 {
                cf.push(p.clone());
            } else {
                cb.push(p.clone());
            }
        }
        FRONT => f.push(p.clone()),
        BACK => b.push(p.clone()),
        _ => {
            let (mut fv, mut bv) = (Vec::new(), Vec::new());
            let n = p.v.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (Some(&ti), Some(&tj), Some(&vi), Some(&vj)) = (types.get(i), types.get(j), p.v.get(i), p.v.get(j)) else { continue };
                if ti != BACK {
                    fv.push(vi);
                }
                if ti != FRONT {
                    bv.push(vi);
                }
                if (ti | tj) == SPANNING {
                    let den = pl.n.dot(vj - vi);
                    if den.abs() > 1e-300 {
                        let t = (pl.w - pl.n.dot(vi)) / den;
                        let x = vi.lerp(vj, t);
                        fv.push(x);
                        bv.push(x);
                    }
                }
            }
            if fv.len() >= 3 {
                f.push(Poly { v: fv, plane: p.plane });
            }
            if bv.len() >= 3 {
                b.push(Poly { v: bv, plane: p.plane });
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<PlaneEq>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<Poly>,
}

impl Node {
    fn build(&mut self, polys: Vec<Poly>, eps: f64, depth: usize) -> Result<()> {
        if polys.is_empty() {
            return Ok(());
        }
        if depth > MAX_DEPTH {
            return Err(KernelError::Failed("planar boolean: tree too deep".into()));
        }
        let pl = match self.plane {
            Some(p) => p,
            None => {
                let p = polys.first().map(|p| p.plane).ok_or_else(|| KernelError::Failed("bsp".into()))?;
                self.plane = Some(p);
                p
            }
        };
        let (mut f, mut b) = (Vec::new(), Vec::new());
        let mut cf = Vec::new();
        let mut cb = Vec::new();
        for p in &polys {
            split(pl, p, eps, &mut cf, &mut cb, &mut f, &mut b);
        }
        self.polys.extend(cf);
        self.polys.extend(cb);
        if !f.is_empty() {
            self.front.get_or_insert_with(Default::default).build(f, eps, depth + 1)?;
        }
        if !b.is_empty() {
            self.back.get_or_insert_with(Default::default).build(b, eps, depth + 1)?;
        }
        Ok(())
    }
    fn invert(&mut self) {
        for p in &mut self.polys {
            p.flip();
        }
        self.plane = self.plane.map(PlaneEq::flipped);
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }
    fn clip_polys(&self, polys: Vec<Poly>, eps: f64) -> Vec<Poly> {
        let Some(pl) = self.plane else { return polys };
        let (mut f, mut b) = (Vec::new(), Vec::new());
        let (mut cf, mut cb) = (Vec::new(), Vec::new());
        for p in &polys {
            split(pl, p, eps, &mut cf, &mut cb, &mut f, &mut b);
        }
        f.extend(cf);
        b.extend(cb);
        let f = match &self.front {
            Some(n) => n.clip_polys(f, eps),
            None => f,
        };
        let b = match &self.back {
            Some(n) => n.clip_polys(b, eps),
            None => Vec::new(),
        };
        let mut out = f;
        out.extend(b);
        out
    }
    fn clip_to(&mut self, other: &Node, eps: f64) {
        let polys = std::mem::take(&mut self.polys);
        self.polys = other.clip_polys(polys, eps);
        if let Some(f) = &mut self.front {
            f.clip_to(other, eps);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other, eps);
        }
    }
    fn all(&self, out: &mut Vec<Poly>) {
        out.extend(self.polys.iter().cloned());
        if let Some(f) = &self.front {
            f.all(out);
        }
        if let Some(b) = &self.back {
            b.all(out);
        }
    }
}

/// Polygons of a body whose faces are all planar (None otherwise).
fn planar_polys(b: &Body) -> Option<Vec<Poly>> {
    let tol = (b.size() * 1e-3).max(1e-3);
    let faces = b.faces(tol).ok()?;
    let mesh = b.tessellate(tol).ok()?;
    let planes: Vec<PlaneEq> = faces.iter().map(|f| f.plane_normal.map(|n| PlaneEq { n, w: n.dot(f.centroid) })).collect::<Option<_>>()?;
    let mut out = Vec::new();
    for (t, fi) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let [a, bb, c] = mesh.tri(t)?;
        let plane = *planes.get(*fi as usize)?;
        // Snap the vertices onto the exact face plane.
        let snap = |p: Vec3| p - plane.n * (plane.n.dot(p) - plane.w);
        let mut v = vec![snap(a), snap(bb), snap(c)];
        if (v[1] - v[0]).cross(v[2] - v[0]).dot(plane.n) < 0.0 {
            v.reverse();
        }
        out.push(Poly { v, plane });
    }
    (out.len() <= MAX_POLYS).then_some(out)
}

/// Boolean of two all-planar bodies.
pub fn planar_boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    a.require_brep("a boolean")?;
    b.require_brep("a boolean")?;
    let (Some(pa), Some(pb)) = (planar_polys(a), planar_polys(b)) else {
        return Err(KernelError::Failed("not supported yet: coincident faces on curved bodies".into()));
    };
    let size = a.size().max(b.size());
    let eps = size * 1e-9;
    let (mut na, mut nb) = (Node::default(), Node::default());
    na.build(pa, eps, 0)?;
    nb.build(pb, eps, 0)?;
    match op {
        BoolOp::Union => {
            na.clip_to(&nb, eps);
            nb.clip_to(&na, eps);
            nb.invert();
            nb.clip_to(&na, eps);
            nb.invert();
            let mut extra = Vec::new();
            nb.all(&mut extra);
            na.build(extra, eps, 0)?;
        }
        BoolOp::Cut => {
            na.invert();
            na.clip_to(&nb, eps);
            nb.clip_to(&na, eps);
            nb.invert();
            nb.clip_to(&na, eps);
            nb.invert();
            let mut extra = Vec::new();
            nb.all(&mut extra);
            na.build(extra, eps, 0)?;
            na.invert();
        }
        BoolOp::Intersect => {
            na.invert();
            nb.clip_to(&na, eps);
            nb.invert();
            na.clip_to(&nb, eps);
            nb.clip_to(&na, eps);
            let mut extra = Vec::new();
            nb.all(&mut extra);
            na.build(extra, eps, 0)?;
            na.invert();
        }
    }
    let mut polys = Vec::new();
    na.all(&mut polys);
    if polys.is_empty() {
        return Ok(None);
    }
    rebuild(polys, size).map(Some)
}

/// Position key for vertex merging.
fn key(p: Vec3, q: f64) -> (i64, i64, i64) {
    ((p.x / q).round() as i64, (p.y / q).round() as i64, (p.z / q).round() as i64)
}

/// Turn polygons into a B-rep solid: merge by plane, split T-junctions, chain boundary loops.
fn rebuild(polys: Vec<Poly>, size: f64) -> Result<Body> {
    let q = size * 1e-7;
    // Global vertex table.
    let mut index: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut verts: Vec<Vec3> = Vec::new();
    let mut id = |p: Vec3, verts: &mut Vec<Vec3>| -> usize {
        let k = key(p, q);
        *index.entry(k).or_insert_with(|| {
            verts.push(p);
            verts.len() - 1
        })
    };
    let mut loops: Vec<(PlaneEq, Vec<usize>)> = Vec::new();
    for p in &polys {
        let mut l: Vec<usize> = p.v.iter().map(|v| id(*v, &mut verts)).collect();
        l.dedup();
        if l.first() == l.last() && l.len() > 1 {
            l.pop();
        }
        if l.len() >= 3 {
            loops.push((p.plane, l));
        }
    }
    // Split every polygon edge at vertices lying on it (T-junctions).
    let tol = size * 1e-7;
    let mut directed: Vec<(usize, usize, usize)> = Vec::new(); // (plane group, from, to)
    let mut groups: Vec<PlaneEq> = Vec::new();
    for (pl, l) in &loops {
        let g = match groups.iter().position(|g| g.n.dot(pl.n) > 1.0 - 1e-9 && (g.w - pl.w).abs() < tol * 10.0) {
            Some(g) => g,
            None => {
                groups.push(*pl);
                groups.len() - 1
            }
        };
        let n = l.len();
        for i in 0..n {
            let (Some(&a), Some(&b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
            let (Some(&pa), Some(&pb)) = (verts.get(a), verts.get(b)) else { continue };
            let d = pb - pa;
            let len2 = d.len2();
            let mut on: Vec<(f64, usize)> = verts
                .iter()
                .enumerate()
                .filter(|(k, _)| *k != a && *k != b)
                .filter_map(|(k, v)| {
                    let t = (*v - pa).dot(d) / len2.max(1e-300);
                    (t > 1e-9 && t < 1.0 - 1e-9 && v.dist_to_segment(pa, pb) < tol).then_some((t, k))
                })
                .collect();
            on.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut prev = a;
            for (_, k) in on {
                directed.push((g, prev, k));
                prev = k;
            }
            directed.push((g, prev, b));
        }
    }
    // Interior edges cancel against their reverse within the same plane group.
    let mut count: HashMap<(usize, usize, usize), i64> = HashMap::new();
    for (g, a, b) in &directed {
        *count.entry((*g, *a, *b)).or_insert(0) += 1;
    }
    let mut boundary: Vec<(usize, usize, usize)> = Vec::new();
    for (&(g, a, b), &c) in &count {
        let rev = count.get(&(g, b, a)).copied().unwrap_or(0);
        for _ in 0..(c - rev).max(0) {
            boundary.push((g, a, b));
        }
    }
    boundary.sort_unstable();
    guard("planar boolean rebuild", || {
        let tv: Vec<mt::Vertex> = verts.iter().map(|v| builder::vertex(p3(*v))).collect();
        let mut edges: HashMap<(usize, usize), mt::Edge> = HashMap::new();
        let mut faces: Vec<mt::Face> = Vec::new();
        for (g, plane) in groups.iter().enumerate() {
            let mut segs: Vec<(usize, usize)> = boundary.iter().filter(|e| e.0 == g).map(|e| (e.1, e.2)).collect();
            let mut wires: Vec<mt::Wire> = Vec::new();
            let mut loops_g: Vec<Vec<usize>> = Vec::new();
            while let Some((s, t)) = segs.pop() {
                let mut lp = vec![s, t];
                let mut cur = t;
                let mut guard_n = 0;
                while cur != s && guard_n < 100_000 {
                    guard_n += 1;
                    let Some(k) = segs.iter().position(|(a, _)| *a == cur) else { break };
                    let (_, nb) = segs.remove(k);
                    lp.push(nb);
                    cur = nb;
                }
                if cur == s {
                    lp.pop();
                    loops_g.push(lp);
                }
            }
            for lp in &loops_g {
                let n = lp.len();
                let mut es = Vec::with_capacity(n);
                for i in 0..n {
                    let (Some(&a), Some(&b)) = (lp.get(i), lp.get((i + 1) % n)) else { continue };
                    let k = (a.min(b), a.max(b));
                    let e = match edges.get(&k) {
                        Some(e) => e.clone(),
                        None => {
                            let (Some(va), Some(vb)) = (tv.get(k.0), tv.get(k.1)) else { continue };
                            let e = builder::line(va, vb);
                            edges.insert(k, e.clone());
                            e
                        }
                    };
                    es.push(if a < b { e } else { e.inverse() });
                }
                wires.push(es.into());
            }
            if wires.is_empty() {
                continue;
            }
            // Outer loops are counter-clockwise about the plane normal; holes go with the
            // outer loop that contains them.
            let area = |lp: &Vec<usize>| -> f64 {
                let n = lp.len();
                let mut s = Vec3::ZERO;
                for i in 0..n {
                    if let (Some(a), Some(b)) = (lp.get(i).and_then(|k| verts.get(*k)), lp.get((i + 1) % n).and_then(|k| verts.get(*k))) {
                        s += a.cross(*b);
                    }
                }
                s.dot(plane.n) * 0.5
            };
            let (outer, holes): (Vec<usize>, Vec<usize>) = (0..loops_g.len()).partition(|i| loops_g.get(*i).is_some_and(|l| area(l) > 0.0));
            let u = plane.n.any_perp();
            let w = plane.n.cross(u);
            let to2 = |p: Vec3| (p.dot(u), p.dot(w));
            let inside = |pt: Vec3, lp: &Vec<usize>| {
                let (px, py) = to2(pt);
                let n = lp.len();
                let mut c = false;
                for i in 0..n {
                    let (Some(a), Some(b)) = (lp.get(i).and_then(|k| verts.get(*k)), lp.get((i + 1) % n).and_then(|k| verts.get(*k))) else {
                        continue;
                    };
                    let ((ax, ay), (bx, by)) = (to2(*a), to2(*b));
                    if (ay > py) != (by > py) && px < ax + (py - ay) / (by - ay) * (bx - ax) {
                        c = !c;
                    }
                }
                c
            };
            for o in &outer {
                let Some(ol) = loops_g.get(*o) else { continue };
                let mut ws = vec![wires.get(*o).cloned().ok_or_else(|| KernelError::Failed("wire".into()))?];
                for h in &holes {
                    let Some(hl) = loops_g.get(*h) else { continue };
                    let probe = hl.first().and_then(|k| verts.get(*k)).copied().unwrap_or_default();
                    // The innermost outer loop containing the hole.
                    let owner = outer.iter().filter(|x| loops_g.get(**x).is_some_and(|l| inside(probe, l))).min_by(|x, y| {
                        let ax = loops_g.get(**x).map(area).unwrap_or(0.0);
                        let ay = loops_g.get(**y).map(area).unwrap_or(0.0);
                        ax.total_cmp(&ay)
                    });
                    if owner == Some(o) {
                        ws.push(wires.get(*h).cloned().ok_or_else(|| KernelError::Failed("wire".into()))?);
                    }
                }
                let _ = ol;
                let face = builder::try_attach_plane(&ws).map_err(|e| KernelError::Failed(format!("planar boolean face: {e}")))?;
                faces.push(face);
            }
        }
        let shell: mt::Shell = faces.into();
        let parts = shell.connected_components();
        let solid = Solid::try_new(parts).map_err(|e| KernelError::Failed(format!("planar boolean: {e}")))?;
        Body::new(solid)
    })
}
