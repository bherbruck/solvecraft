//! Booleans of bodies with coincident planar faces, by moving faces apart first.
//!
//! The mesh-based intersection cannot handle faces that lie in the same plane, nor the edges
//! lying in them. A face whose neighbours stand perpendicular to it (any extruded or
//! box-like part) can be pushed along its normal exactly, by rebuilding it and stretching or
//! shrinking its neighbours. Then:
//! - cut and intersect: the tool's coincident face moves out of the way (outward when the
//!   bodies are on the same side of the plane, away from the target when they touch back to
//!   back), which leaves the result unchanged;
//! - union of faces facing the same way: both faces move outward by different amounts, the
//!   union is taken, and the slab beyond the plane is cut off (now a transversal cut);
//! - union of faces back to back: the tool's face moves into the target, which overlaps it.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::{BoolOp, KernelError, Result, guard};

#[derive(Clone, Copy, Debug)]
struct PlaneFace {
    n: Vec3,
    d: f64,
}

fn planes(b: &Body) -> Vec<(usize, PlaneFace)> {
    b.solid
        .face_iter()
        .enumerate()
        .filter_map(|(i, f)| match f.oriented_surface() {
            mt::Surface::Plane(p) => {
                let n = p.normal();
                let n = Vec3::new(n.x, n.y, n.z).normalized()?;
                Some((i, PlaneFace { n, d: n.dot(from_p3(p.origin())) }))
            }
            _ => None,
        })
        .collect()
}

/// Push the planar faces of `b` lying in the plane (n, d) — n their outward normal — along n
/// by `delta` (negative: inward). Their neighbours must stand perpendicular to the plane.
fn push_faces(b: &Body, n: Vec3, d: f64, delta: f64) -> Result<Body> {
    let size = b.size();
    let tol = (size * 1e-7).max(1e-9);
    let fail = |m: &str| KernelError::Failed(format!("pushing a face: {m}"));
    guard("push face", || {
        let solid = b.deep_copy();
        let shift = n * delta;
        let mat = mt::Matrix4::from_translation(v3(shift));
        let mut shells = Vec::new();
        for sh in solid.boundaries() {
            let faces: Vec<mt::Face> = sh.face_iter().cloned().collect();
            let on_plane = |f: &mt::Face| match f.oriented_surface() {
                mt::Surface::Plane(p) => {
                    let m = p.normal();
                    let m = Vec3::new(m.x, m.y, m.z);
                    m.dot(n) > 1.0 - 1e-9 && (n.dot(from_p3(p.origin())) - d).abs() < tol * 10.0
                }
                _ => false,
            };
            let moving: Vec<usize> = (0..faces.len()).filter(|i| faces.get(*i).is_some_and(on_plane)).collect();
            if moving.is_empty() {
                shells.push(sh.clone());
                continue;
            }
            // Vertices and edges of the moving faces.
            let mut vmap: HashMap<mt::VertexID, mt::Vertex> = HashMap::new();
            let mut emap: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
            for i in &moving {
                let f = faces.get(*i).ok_or_else(|| fail("face"))?;
                for v in f.vertex_iter() {
                    vmap.entry(v.id()).or_insert_with(|| builder::vertex(p3(from_p3(v.point()) + shift)));
                }
            }
            for i in &moving {
                let f = faces.get(*i).ok_or_else(|| fail("face"))?;
                for e in f.edge_iter() {
                    if emap.contains_key(&e.id()) {
                        continue;
                    }
                    let (a, bb) = (
                        vmap.get(&e.absolute_front().id()).ok_or_else(|| fail("vertex"))?,
                        vmap.get(&e.absolute_back().id()).ok_or_else(|| fail("vertex"))?,
                    );
                    let curve = mt::Transformed::transformed(&e.curve(), mat);
                    emap.insert(e.id(), mt::Edge::new(a, bb, curve));
                }
            }
            // Edges leaving the plane at moved vertices: straight, along n.
            for f in &faces {
                for e in f.edge_iter() {
                    if emap.contains_key(&e.id()) {
                        continue;
                    }
                    let (fa, fb) = (e.absolute_front(), e.absolute_back());
                    let (ma, mb) = (vmap.get(&fa.id()), vmap.get(&fb.id()));
                    if ma.is_none() && mb.is_none() {
                        continue;
                    }
                    if !matches!(e.curve(), mt::Curve::Line(_)) {
                        return Err(fail("a neighbouring edge is not straight"));
                    }
                    let dir = (from_p3(fb.point()) - from_p3(fa.point())).normalized().ok_or_else(|| fail("zero edge"))?;
                    if dir.cross(n).len() > 1e-9 {
                        return Err(fail("a neighbouring face is not perpendicular"));
                    }
                    let (na, nb) = (ma.cloned().unwrap_or_else(|| fa.clone()), mb.cloned().unwrap_or_else(|| fb.clone()));
                    if (from_p3(nb.point()) - from_p3(na.point())).dot(dir) <= tol {
                        return Err(fail("the face would pass its opposite side"));
                    }
                    emap.insert(e.id(), builder::line(&na, &nb));
                }
            }
            let rebuild_wires = |f: &mt::Face| -> Vec<mt::Wire> {
                f.absolute_boundaries()
                    .iter()
                    .map(|w| {
                        w.edge_iter()
                            .map(|e| match emap.get(&e.id()) {
                                Some(ne) => {
                                    if e.front() == e.absolute_front() {
                                        ne.clone()
                                    } else {
                                        ne.inverse()
                                    }
                                }
                                None => e.clone(),
                            })
                            .collect::<Vec<_>>()
                            .into()
                    })
                    .collect()
            };
            use mt::{ParametricSurface3D, SearchNearestParameter};
            let mut out: Vec<mt::Face> = Vec::new();
            for (i, f) in faces.iter().enumerate() {
                let touches = f.edge_iter().any(|e| emap.contains_key(&e.id()));
                if !touches {
                    out.push(f.clone());
                    continue;
                }
                let wires = rebuild_wires(f);
                let surface = if moving.contains(&i) {
                    mt::Transformed::transformed(&f.surface(), mat)
                } else if matches!(f.surface(), mt::Surface::Plane(_)) {
                    f.surface()
                } else {
                    // A curved side: sweep its edge in the plane along the new height.
                    let base = f.edge_iter().find_map(|e| {
                        emap.get(&e.id()).filter(|_| {
                            let (a, bb) = (from_p3(e.absolute_front().point()), from_p3(e.absolute_back().point()));
                            (n.dot(a) - d).abs() < tol * 10.0 && (n.dot(bb) - d).abs() < tol * 10.0
                        })
                    });
                    let base = base.ok_or_else(|| fail("a curved side without an edge in the plane"))?;
                    let height = f.vertex_iter().map(|v| n.dot(from_p3(v.point())) - d).fold(0.0f64, |m, x| if x.abs() > m.abs() { x } else { m });
                    let swept: mt::Face = builder::tsweep(base, v3(n * (height - delta)));
                    let mut s = swept.surface();
                    // Same normal as the old side at a point both share.
                    let probe = f.vertex_iter().map(|v| from_p3(v.point())).fold(Vec3::ZERO, |a, x| a + x) / f.vertex_iter().count().max(1) as f64;
                    let old = f.surface();
                    if let (Some((u0, v0)), Some((u1, v1))) =
                        (old.search_nearest_parameter(p3(probe), None, 100), s.search_nearest_parameter(p3(probe), None, 100))
                    {
                        let (a, bb) = (old.normal(u0, v0), s.normal(u1, v1));
                        if mt::InnerSpace::dot(a, bb) < 0.0 {
                            s = mt::Invertible::inverse(&s);
                        }
                    }
                    s
                };
                let mut nf = mt::Face::try_new(wires, surface).map_err(|e| fail(&e.to_string()))?;
                if !f.orientation() {
                    nf.invert();
                }
                out.push(nf);
            }
            shells.push(out.into());
        }
        let s = Solid::try_new(shells).map_err(|e| fail(&e.to_string()))?;
        Body::new(s)
    })
}

/// A box covering everything beyond the plane (n, d) by up to `depth`.
fn slab_beyond(n: Vec3, d: f64, depth: f64, extent: f64) -> Result<Body> {
    let u = n.any_perp();
    let w = n.cross(u);
    let o = n * d;
    let pl = solvecraft_geom::Plane::new(o, u, w).ok_or_else(|| KernelError::Failed("slab plane".into()))?;
    let sq = solvecraft_geom::Loop2::polygon(&[
        solvecraft_geom::Vec2::new(-extent, -extent),
        solvecraft_geom::Vec2::new(extent, -extent),
        solvecraft_geom::Vec2::new(extent, extent),
        solvecraft_geom::Vec2::new(-extent, extent),
    ]);
    let r = solvecraft_geom::Region2 { outer: sq, holes: vec![] };
    crate::extrude(&pl, &[r], 0.0, depth)?.pop().ok_or_else(|| KernelError::Failed("slab".into()))
}

/// Retry a failed boolean with coincident planar faces pushed apart. `None` when there are no
/// coincident faces or the faces can't be pushed.
pub(crate) fn boolean_apart(a: &Body, b: &Body, op: BoolOp) -> Option<Result<Option<Body>>> {
    let size = a.size().max(b.size());
    let tol = (size * 1e-7).max(1e-9);
    let (pa, pb) = (planes(a), planes(b));
    let mut same: Vec<(Vec3, f64)> = Vec::new();
    let mut back: Vec<(Vec3, f64)> = Vec::new();
    for (_, x) in &pa {
        for (_, y) in &pb {
            if (x.d - y.d * x.n.dot(y.n)).abs() < tol * 10.0 && x.n.dot(y.n).abs() > 1.0 - 1e-9 {
                let list = if x.n.dot(y.n) > 0.0 { &mut same } else { &mut back };
                if !list.iter().any(|(n, d)| n.dot(x.n) > 1.0 - 1e-9 && (d - x.d).abs() < tol * 10.0) {
                    list.push((x.n, x.d));
                }
            }
        }
    }
    if same.is_empty() && back.is_empty() {
        return None;
    }
    let (d1, d2) = (size * 0.0137, size * 0.0229);
    let run = || -> Result<Option<Body>> {
        let (mut a2, mut b2) = (a.clone(), b.clone());
        for (n, d) in &same {
            // b's face points along n too.
            b2 = push_faces(&b2, *n, *d, d2)?;
            if op == BoolOp::Union {
                a2 = push_faces(&a2, *n, *d, d1)?;
            }
        }
        for (n, d) in &back {
            // a's face points along n; b's face points along −n at the same plane (−d).
            let shift = if op == BoolOp::Union { d2 } else { -d2 };
            b2 = push_faces(&b2, -*n, -*d, shift)?;
        }
        let Some(mut r) = crate::ops::boolean(&a2, &b2, op)? else { return Ok(None) };
        if op == BoolOp::Union {
            for (n, d) in &same {
                let slab = slab_beyond(*n, *d, d1.max(d2) * 3.0, size * 4.0)?;
                match crate::ops::boolean(&r, &slab, BoolOp::Cut)? {
                    Some(x) => r = x,
                    None => return Ok(None),
                }
            }
        }
        Ok(Some(r))
    };
    Some(run())
}
