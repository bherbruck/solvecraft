//! Exact-plane booleans for bodies whose faces are all planar (BSP-tree CSG on polygons, in
//! the classic style), used when the B-rep boolean fails — typically on coincident faces, such
//! as a wall flush with the side of its base. The polygons are rebuilt into a B-rep: coplanar
//! pieces are merged into faces, T-junctions are split so neighbouring faces share edges.

use std::collections::HashMap;

use solvecraft_geom::{Mesh, Vec3};
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

/// Polygons of any body: planar faces exactly, curved ones as their triangles (within `tol`).
fn faceted_polys(b: &Body, tol: f64) -> Option<Vec<Poly>> {
    let faces = b.faces(tol).ok()?;
    let mesh = b.tessellate(tol).ok()?;
    let planes: Vec<Option<PlaneEq>> = faces.iter().map(|f| f.plane_normal.map(|n| PlaneEq { n, w: n.dot(f.centroid) })).collect();
    let mut out = Vec::new();
    for (t, fi) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let [a, bb, c] = mesh.tri(t)?;
        let plane = match planes.get(*fi as usize)? {
            Some(pl) => *pl,
            None => {
                let Some(n) = (bb - a).cross(c - a).normalized() else { continue };
                PlaneEq { n, w: n.dot(a) }
            }
        };
        let snap = |p: Vec3| p - plane.n * (plane.n.dot(p) - plane.w);
        let mut v = vec![snap(a), snap(bb), snap(c)];
        if (v[1] - v[0]).cross(v[2] - v[0]).dot(plane.n) < 0.0 {
            v.reverse();
        }
        out.push(Poly { v, plane });
    }
    (out.len() <= MAX_POLYS).then_some(out)
}

/// Boolean of a body, its curved faces faceted to `tol`, with a closed triangle mesh (wound
/// outward); the result has planar faces only. For tools the B-rep boolean cannot take, such as
/// the helical groove of a thread.
pub fn faceted_boolean(a: &Body, b: &Mesh, op: BoolOp, tol: f64) -> Result<Option<Body>> {
    a.require_brep("a boolean")?;
    if !(tol.is_finite() && tol > 0.0) {
        return Err(KernelError::Invalid("facet tolerance".into()));
    }
    let Some(pa) = faceted_polys(a, tol) else {
        return Err(KernelError::Failed("the body is too detailed to facet".into()));
    };
    let mut pb = Vec::with_capacity(b.triangles.len());
    for t in &b.triangles {
        let Some([p, q, r]) = b.tri(t) else { continue };
        let Some(n) = (q - p).cross(r - p).normalized() else { continue };
        pb.push(Poly { v: vec![p, q, r], plane: PlaneEq { n, w: n.dot(p) } });
    }
    if pa.len() + pb.len() > MAX_POLYS {
        return Err(KernelError::Failed("the bodies are too detailed to facet".into()));
    }
    let (lo, hi) = b
        .positions
        .iter()
        .fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(l, h), p| (l.min(*p), h.max(*p)));
    let size = if b.positions.is_empty() { a.size() } else { a.size().max((hi - lo).len()) };
    // Fine facets of curved faces meet at shallow angles, where the coplanar test decides
    // whether the pieces' boundaries come out consistent: try a few tolerances, and take the
    // first whose result is watertight and agrees with the complementary boolean (A − B and
    // A ∩ B add up to A; A ∪ B is A plus B less A ∩ B).
    let (va, vb) = (polys_volume(&pa), polys_volume(&pb));
    let mut last = KernelError::Failed("faceted boolean".into());
    for rel in [1e-8, 1e-7, 3e-8, 1e-9, 3e-7] {
        let eps = size * rel;
        let out = csg(pa.clone(), pb.clone(), op, eps)?;
        let both = csg(pa.clone(), pb.clone(), BoolOp::Intersect, eps)?;
        let (vo, vi) = (polys_volume(&out), polys_volume(&both));
        let expect = match op {
            BoolOp::Cut => va - vi,
            BoolOp::Union => va + vb - vi,
            BoolOp::Intersect => vi,
        };
        if (vo - expect).abs() > 1e-6 * (va.abs() + vb.abs()) || vi < -1e-9 * (va.abs() + vb.abs()) {
            last = KernelError::Failed(format!("faceted boolean: inconsistent volumes ({vo} vs {expect})"));
            continue;
        }
        if out.is_empty() {
            return Ok(None);
        }
        match rebuild(out, size) {
            Ok(b) => return Ok(Some(b)),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Boolean of two all-planar bodies.
pub fn planar_boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    a.require_brep("a boolean")?;
    b.require_brep("a boolean")?;
    let (Some(pa), Some(pb)) = (planar_polys(a), planar_polys(b)) else {
        if let Some(r) = sliced_planar_boolean(a, b, op) {
            return r;
        }
        return Err(KernelError::Failed("not supported yet: coincident faces on curved bodies".into()));
    };
    let size = a.size().max(b.size());
    polys_boolean(pa, pb, size, op, size * 1e-9)
}

/// [`planar_boolean`] of a body with curved faces and an all-planar one, when a slab between
/// two parallel planes holds `b` and none of `a`'s curved faces (a pocket cut beside a round
/// hole): `a` is split across the slab, the middle piece takes the exact boolean, and the
/// pieces outside are joined back on. `None` when no such slab exists.
fn sliced_planar_boolean(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    let pb = planar_polys(b)?;
    let size = a.size().max(b.size());
    let tol = (a.size() * 1e-3).max(1e-3);
    let infos = a.faces(tol).ok()?;
    let mesh = a.tessellate(tol).ok()?;
    let mut curved: HashMap<u32, solvecraft_geom::Aabb3> = HashMap::new();
    for (t, fi) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if infos.get(*fi as usize)?.plane_normal.is_some() {
            continue;
        }
        let bx = curved.entry(*fi).or_insert(solvecraft_geom::Aabb3::EMPTY);
        for p in mesh.tri(t)? {
            bx.add(p);
        }
    }
    let mut tool = solvecraft_geom::Aabb3::EMPTY;
    for p in pb.iter().flat_map(|p| &p.v) {
        tool.add(*p);
    }
    let at = |v: Vec3, k: usize| [v.x, v.y, v.z].get(k).copied().unwrap_or(0.0);
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    // Planar faces of `a` square to an axis: a cut must not land in one of their planes.
    let flat: Vec<(Vec3, Vec3)> = infos.iter().filter_map(|f| Some((f.plane_normal?, f.centroid))).collect();
    let gap = size * 1e-4;
    let place = |k: usize, near: f64, far: f64| -> Option<f64> {
        let axis = *axes.get(k)?;
        [0.5, 0.37, 0.63, 0.25, 0.75]
            .iter()
            .map(|t| near + (far - near) * t)
            .find(|x| !flat.iter().any(|(n, c)| n.dot(axis).abs() > 1.0 - 1e-9 && (at(*c, k) - x).abs() < gap))
    };
    // An axis along which every curved face lies clear of the tool: the cuts either side.
    let (k, lo, hi) = (0..3).find_map(|k| {
        let (t0, t1) = (at(tool.min, k), at(tool.max, k));
        let (mut below, mut above) = (None::<f64>, None::<f64>);
        for bx in curved.values() {
            let (c0, c1) = (at(bx.min, k), at(bx.max, k));
            if c1 < t0 - gap {
                below = Some(below.map_or(c1, |x| x.max(c1)));
            } else if c0 > t1 + gap {
                above = Some(above.map_or(c0, |x| x.min(c0)));
            } else {
                return None;
            }
        }
        let lo = match below {
            Some(c) => Some(place(k, t0, c)?),
            None => None,
        };
        let hi = match above {
            Some(c) => Some(place(k, t1, c)?),
            None => None,
        };
        Some((k, lo, hi))
    })?;
    let (u, w) = match k {
        0 => (Vec3::Y, Vec3::Z),
        1 => (Vec3::Z, Vec3::X),
        _ => (Vec3::X, Vec3::Y),
    };
    let axis = *axes.get(k)?;
    let run = || -> Result<Option<Body>> {
        let fail = |m: &str| KernelError::Failed(format!("planar boolean beside curved faces: {m}"));
        let mut middle = a.clone();
        let mut outside = Vec::new();
        for (cut, keep_positive) in [(lo, true), (hi, false)] {
            let Some(x) = cut else { continue };
            let plane = solvecraft_geom::Plane::new(axis * x, u, w).ok_or_else(|| fail("plane"))?;
            outside.extend(crate::ops::half_space(&middle, &plane, !keep_positive)?);
            middle = crate::ops::half_space(&middle, &plane, keep_positive)?.ok_or_else(|| fail("nothing beside the tool"))?;
        }
        let pm = planar_polys(&middle).ok_or_else(|| fail("the middle piece is not planar"))?;
        let r = polys_boolean(pm, pb, size, op, size * 1e-9)?;
        if op == BoolOp::Intersect {
            return Ok(r);
        }
        // The rebuild leaves the cut faces' edges split where other polygons were cut; healed,
        // they match the pieces outside edge for edge.
        let mut pieces: Vec<Body> = match r {
            Some(m) => vec![Body::new(crate::heal::heal_keep(m.deep_copy(), size, a.split_keep()))?],
            None => Vec::new(),
        };
        // Each piece outside meets the middle along its cut face (glued one shell to another).
        if pieces.iter().chain(&outside).any(|x| x.solid.boundaries().len() != 1) {
            return Err(fail("a piece in several parts"));
        }
        for o in outside {
            let joined = pieces.iter().enumerate().find_map(|(i, m)| Some((i, crate::coplanar::glue(m, &o)?.ok()?)));
            let (i, j) = joined.ok_or_else(|| fail("a piece does not join back"))?;
            if let Some(slot) = pieces.get_mut(i) {
                *slot = j;
            }
        }
        crate::ops::join_pieces(pieces)
    };
    Some(run())
}

fn polys_boolean(pa: Vec<Poly>, pb: Vec<Poly>, size: f64, op: BoolOp, eps: f64) -> Result<Option<Body>> {
    let polys = csg(pa, pb, op, eps)?;
    if polys.is_empty() {
        return Ok(None);
    }
    rebuild(polys, size).map(Some)
}

/// Enclosed volume of a closed set of polygons (divergence theorem).
fn polys_volume(ps: &[Poly]) -> f64 {
    let mut v6 = 0.0;
    for p in ps {
        let Some(&o) = p.v.first() else { continue };
        for w in p.v.windows(2).skip(1) {
            v6 += o.dot(w[0].cross(w[1]));
        }
    }
    v6 / 6.0
}

/// The polygons of a BSP boolean (csg.js style).
fn csg(pa: Vec<Poly>, pb: Vec<Poly>, op: BoolOp, eps: f64) -> Result<Vec<Poly>> {
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
    Ok(polys)
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
    // Points within `q` share a vertex, also across the rounding grid's cell boundaries.
    let mut id = |p: Vec3, verts: &mut Vec<Vec3>| -> usize {
        let k = key(p, q);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(&i) = index.get(&(k.0 + dx, k.1 + dy, k.2 + dz))
                        && verts.get(i).is_some_and(|v| (*v - p).len() <= q)
                    {
                        return i;
                    }
                }
            }
        }
        verts.push(p);
        index.insert(k, verts.len() - 1);
        verts.len() - 1
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
    // Split every polygon edge at vertices lying on it (T-junctions). Vertices are binned in a
    // grid sized to the typical edge, so each edge only looks at its neighbourhood.
    let tol = size * 1e-7;
    let edge_len: Vec<f64> = loops
        .iter()
        .flat_map(|(_, l)| (0..l.len()).filter_map(move |i| Some((*l.get(i)?, *l.get((i + 1) % l.len())?))))
        .filter_map(|(a, b)| Some((*verts.get(b)? - *verts.get(a)?).len()))
        .collect();
    let mean = edge_len.iter().sum::<f64>() / edge_len.len().max(1) as f64;
    let cell = mean.max(size * 1e-4).max(1e-9);
    let ckey = |p: Vec3| ((p.x / cell).floor() as i64, (p.y / cell).floor() as i64, (p.z / cell).floor() as i64);
    let mut grid: HashMap<(i64, i64, i64), Vec<usize>> = HashMap::new();
    for (k, v) in verts.iter().enumerate() {
        grid.entry(ckey(*v)).or_default().push(k);
    }
    let mut directed: Vec<(usize, usize, usize)> = Vec::new(); // (plane group, from, to)
    let mut groups: Vec<PlaneEq> = Vec::new();
    let mut group_of: HashMap<(i64, i64, i64, i64), usize> = HashMap::new();
    for (pl, l) in &loops {
        let gk = ((pl.n.x * 1e8).round() as i64, (pl.n.y * 1e8).round() as i64, (pl.n.z * 1e8).round() as i64, (pl.w / (tol * 10.0)).round() as i64);
        let g = *group_of.entry(gk).or_insert_with(|| {
            groups.push(*pl);
            groups.len() - 1
        });
        let n = l.len();
        for i in 0..n {
            let (Some(&a), Some(&b)) = (l.get(i), l.get((i + 1) % n)) else { continue };
            let (Some(&pa), Some(&pb)) = (verts.get(a), verts.get(b)) else { continue };
            let d = pb - pa;
            let len2 = d.len2();
            let (lo, hi) = (ckey(pa.min(pb) - Vec3::new(tol, tol, tol)), ckey(pa.max(pb) + Vec3::new(tol, tol, tol)));
            let cells = (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) * (hi.2 - lo.2 + 1);
            let candidates: Vec<usize> = if cells > 4096 {
                (0..verts.len()).collect()
            } else {
                let mut c = Vec::new();
                for x in lo.0..=hi.0 {
                    for y in lo.1..=hi.1 {
                        for z in lo.2..=hi.2 {
                            if let Some(ks) = grid.get(&(x, y, z)) {
                                c.extend(ks.iter().copied());
                            }
                        }
                    }
                }
                c
            };
            let mut on: Vec<(f64, usize)> = candidates
                .into_iter()
                .filter(|k| *k != a && *k != b)
                .filter_map(|k| {
                    let v = verts.get(k)?;
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
                    // A walk through a vertex twice (two loops touching there) is split into
                    // simple loops at that vertex.
                    let mut stack: Vec<usize> = Vec::new();
                    for v in lp {
                        if let Some(at) = stack.iter().position(|x| *x == v) {
                            let sub: Vec<usize> = stack.split_off(at);
                            if sub.len() >= 3 {
                                loops_g.push(sub);
                            }
                        }
                        stack.push(v);
                    }
                    if stack.len() >= 3 {
                        loops_g.push(stack);
                    }
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
                // A loop a hair off flat (fine facets of a curved face) takes the group's plane.
                let face = match builder::try_attach_plane(&ws) {
                    Ok(f) => f,
                    Err(e) => {
                        let o = plane.n * plane.w;
                        let surface = mt::Plane::new(p3(o), p3(o + u), p3(o + w));
                        mt::Face::try_new(ws, mt::Surface::Plane(surface))
                            .map_err(|e2| KernelError::Failed(format!("planar boolean face: {e}; {e2}")))?
                    }
                };
                faces.push(face);
            }
        }
        let shell: mt::Shell = faces.into();
        let parts = shell.connected_components();
        let solid = Solid::try_new(parts).map_err(|e| KernelError::Failed(format!("planar boolean: {e}")))?;
        Body::new(solid)
    })
}
