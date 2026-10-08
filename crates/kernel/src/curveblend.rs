//! Rolling-ball blends along a closed chain of smooth edges between two smooth groups of faces
//! of any surfaces (a branch pipe meeting a main pipe, a boss on a cylinder). A ball of the
//! blend's radius rolls along the edge touching both sides; its centre traces the spine and its
//! contacts the curves where the blend meets each side. The blend face is a rational B-spline
//! through the circular cross-sections (exact arcs at the samples), the side faces are trimmed
//! back to the contact curves, and seams that met the edge are shortened to the contacts.

use std::collections::HashMap;

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Solid, from_p3, p3};
use crate::{KernelError, Result};

fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

/// Samples per edge for the contact curves and the blend surface.
const SAMPLES: usize = 24;

/// One cross-section of the blend: ball centre and its contacts with side 1 and side 2.
#[derive(Clone, Copy, Debug)]
struct Section {
    c: Vec3,
    q1: Vec3,
    q2: Vec3,
}

/// Nearest point of a face's surface and the outward normal there.
fn project(f: &mt::Face, p: Vec3, hint: Option<(f64, f64)>) -> Option<(Vec3, Vec3, (f64, f64))> {
    use mt::{ParametricSurface, ParametricSurface3D, SearchNearestParameter};
    let s = f.oriented_surface();
    let uv = s.search_nearest_parameter(p3(p), hint, 100).or_else(|| s.search_nearest_parameter(p3(p), None, 100))?;
    let q = from_p3(s.subs(uv.0, uv.1));
    let n = s.normal(uv.0, uv.1);
    Some((q, Vec3::new(n.x, n.y, n.z).normalized()?, uv))
}

/// Solve for the ball touching both sides near `p`. `extra` adds a third condition (a plane the
/// side-k contact must lie in); without it the centre stays in the plane through `p` square to
/// `t`. `sigma` is +1 when the ball sits outside the material (concave edge), −1 inside.
#[allow(clippy::too_many_arguments)]
fn solve(f1: &[&mt::Face], f2: &[&mt::Face], p: Vec3, t: Vec3, r: f64, sigma: f64, extra: Option<(usize, Vec3, Vec3)>, tol: f64) -> Option<Section> {
    // The nearest face of a side (several faces on one smooth surface group).
    let near = |fs: &[&mt::Face], c: Vec3| -> Option<(Vec3, Vec3)> {
        fs.iter().filter_map(|f| project(f, c, None).map(|(q, n, _)| (q, n))).min_by(|a, b| a.0.dist(c).total_cmp(&b.0.dist(c)))
    };
    let (q1, n1) = near(f1, p)?;
    let (q2, n2) = near(f2, p)?;
    let _ = (q1, q2);
    let den = 1.0 + n1.dot(n2);
    if den.abs() < 1e-9 {
        return None;
    }
    let mut c = p + (n1 + n2) * (sigma * r / den);
    for _ in 0..60 {
        let (q1, n1) = near(f1, c)?;
        let (q2, n2) = near(f2, c)?;
        let g1 = (c - q1).dot(n1) - sigma * r;
        let g2 = (c - q2).dot(n2) - sigma * r;
        // Third row: stay in the section plane, or put a contact on the seam plane.
        let (row3, g3) = match extra {
            None => (t, (c - p).dot(t)),
            Some((k, o, m)) => {
                let q = if k == 1 { q1 } else { q2 };
                (m, (q - o).dot(m))
            }
        };
        let a = [[n1.x, n1.y, n1.z], [n2.x, n2.y, n2.z], [row3.x, row3.y, row3.z]];
        let b = [-g1, -g2, -g3];
        let x = crate::polyhedron::solve3_pub(a, b)?;
        let dc = Vec3::new(x[0], x[1], x[2]);
        c += dc;
        if dc.len() < tol * 1e-3 && g1.abs() < tol && g2.abs() < tol && g3.abs() < tol {
            let (q1, _) = near(f1, c)?;
            let (q2, _) = near(f2, c)?;
            return Some(Section { c, q1, q2 });
        }
    }
    None
}

/// Homogeneous control points of the circular (or straight) cross-section from q1 to q2.
fn section_controls(s: &Section, round: bool) -> Option<[mt::Vector4; 3]> {
    let h = |p: Vec3, w: f64| mt::Vector4::new(p.x * w, p.y * w, p.z * w, w);
    if !round {
        let m = (s.q1 + s.q2) * 0.5;
        return Some([h(s.q1, 1.0), h(m, 1.0), h(s.q2, 1.0)]);
    }
    let (a, b) = (s.q1 - s.c, s.q2 - s.c);
    let theta = a.normalized()?.dot(b.normalized()?).clamp(-1.0, 1.0).acos();
    if !(theta > 1e-6 && theta < std::f64::consts::PI - 1e-6) {
        return None;
    }
    let half = (theta / 2.0).cos();
    let bis = (a + b).normalized()?;
    let m = s.c + bis * (a.len() / half);
    Some([h(s.q1, 1.0), h(m, half), h(s.q2, 1.0)])
}

/// Blend the closed chain of edges (ids in any order) with radius `r`.
pub(crate) fn curve_blend(solid: &Solid, ids: &[mt::EdgeID], size: f64, r: f64, round: bool) -> Result<Solid> {
    let tol = (size * 1e-7).max(1e-9);
    let shells = solid.boundaries();
    let (si, shell) = shells
        .iter()
        .enumerate()
        .find(|(_, sh)| sh.edge_iter().any(|e| ids.contains(&e.id())))
        .ok_or_else(|| KernelError::Failed("edge not found".into()))?;
    let faces: Vec<mt::Face> = shell.face_iter().cloned().collect();
    // Order the chain: edges as stored (absolute direction), each starting where the last ended.
    let mut chain: Vec<mt::Edge> = Vec::new();
    let all: Vec<mt::Edge> = {
        let mut seen = std::collections::HashSet::new();
        shell.edge_iter().filter(|e| ids.contains(&e.id()) && seen.insert(e.id())).map(|e| e.absolute_clone()).collect()
    };
    let first = all.first().cloned().ok_or_else(|| KernelError::Failed("no edges".into()))?;
    chain.push(first.clone());
    while chain.len() < all.len() {
        let end = chain.last().map(|e| e.back().clone()).ok_or_else(|| KernelError::Failed("chain".into()))?;
        let next = all
            .iter()
            .find(|e| !chain.iter().any(|c| c.id() == e.id()) && (*e.front() == end || *e.back() == end))
            .ok_or_else(|| unsupported("the edges don't form one closed chain"))?;
        chain.push(if *next.front() == end { next.clone() } else { next.inverse() });
    }
    if chain.first().map(|e| e.front().clone()) != chain.last().map(|e| e.back().clone()) {
        return Err(unsupported("blending an open chain of curved edges"));
    }
    let n = chain.len();
    // Faces of each edge, split into the two sides.
    let faces_of = |e: &mt::Edge| -> Vec<usize> {
        (0..faces.len()).filter(|i| faces.get(*i).is_some_and(|f| f.edge_iter().any(|x| x.id() == e.id()))).collect()
    };
    let mut side1: Vec<usize> = Vec::new();
    let mut side2: Vec<usize> = Vec::new();
    for (k, e) in chain.iter().enumerate() {
        let fs = faces_of(e);
        let [a, b] = fs[..] else { return Err(unsupported("an edge without two faces")) };
        if k == 0 {
            side1.push(a);
            side2.push(b);
            continue;
        }
        // The side-1 face of this edge is the one whose normal matches side 1 at the joint.
        let p = from_p3(e.front().point());
        let nrm = |fi: usize| faces.get(fi).and_then(|f| project(f, p, None)).map(|x| x.1);
        let prev1 = side1.last().copied().and_then(nrm);
        let (na, nb) = (nrm(a), nrm(b));
        let a_is_1 = match (prev1, na, nb) {
            (Some(x), Some(y), Some(z)) => x.dot(y) >= x.dot(z),
            _ => return Err(KernelError::Failed("blend: normals".into())),
        };
        if a_is_1 {
            side1.push(a);
            side2.push(b);
        } else {
            side1.push(b);
            side2.push(a);
        }
    }
    let s1: Vec<&mt::Face> = side1.iter().filter_map(|i| faces.get(*i)).collect();
    let s2: Vec<&mt::Face> = side2.iter().filter_map(|i| faces.get(*i)).collect();
    // Convex or concave, from the first edge as its side-1 face runs it.
    let e0 = chain.first().ok_or_else(|| KernelError::Failed("chain".into()))?;
    let f1 = s1.first().ok_or_else(|| KernelError::Failed("side".into()))?;
    let in_f1 = f1.boundary_iters().into_iter().flatten().find(|e| e.id() == e0.id()).ok_or_else(|| KernelError::Failed("edge use".into()))?;
    let (pm, t1) = {
        use mt::{BoundedCurve, ParametricCurve};
        let c = in_f1.oriented_curve();
        let (a, b) = c.range_tuple();
        let m = (a + b) * 0.5;
        let d = c.der(m);
        (from_p3(c.subs(m)), Vec3::new(d.x, d.y, d.z).normalized().ok_or_else(|| KernelError::Failed("tangent".into()))?)
    };
    let (n1, n2) = match (project(f1, pm, None), s2.first().and_then(|f| project(f, pm, None))) {
        (Some(x), Some(y)) => (x.1, y.1),
        _ => return Err(KernelError::Failed("blend: normals".into())),
    };
    let convex = t1.dot(n1.cross(n2)) > 0.0;
    let sigma = if convex { -1.0 } else { 1.0 };
    if n1.dot(n2) > 1.0 - 1e-6 {
        return Err(unsupported("blending a smooth edge"));
    }
    // Seams at the chain's vertices: edges there that aren't in the chain, and their side.
    let chain_ids: Vec<mt::EdgeID> = chain.iter().map(|e| e.id()).collect();
    let mut seams: Vec<Vec<(usize, mt::Edge)>> = Vec::with_capacity(n);
    for e in &chain {
        let v = e.front().clone();
        let mut found: Vec<(usize, mt::Edge)> = Vec::new();
        for (fi, f) in faces.iter().enumerate() {
            for x in f.edge_iter() {
                if chain_ids.contains(&x.id()) || !(*x.front() == v || *x.back() == v) || found.iter().any(|(_, y)| y.id() == x.id()) {
                    continue;
                }
                let side = if side1.contains(&fi) {
                    1
                } else if side2.contains(&fi) {
                    2
                } else {
                    return Err(unsupported("a vertex of the edge chain where other faces meet"));
                };
                found.push((side, x.absolute_clone()));
            }
        }
        // At most one seam per side.
        if found.iter().filter(|x| x.0 == 1).count() > 1 || found.iter().filter(|x| x.0 == 2).count() > 1 {
            return Err(unsupported("a vertex of the edge chain where several seams meet"));
        }
        seams.push(found);
    }
    // Sections: at each vertex (a contact on the seam there), then along each edge.
    let tangent_at = |e: &mt::Edge, s: f64| -> Option<(Vec3, Vec3)> {
        use mt::{BoundedCurve, ParametricCurve};
        let c = e.oriented_curve();
        let (a, b) = c.range_tuple();
        let t = a + (b - a) * s;
        let d = c.der(t);
        Some((from_p3(c.subs(t)), Vec3::new(d.x, d.y, d.z).normalized()?))
    };
    let mut vsec: Vec<Section> = Vec::with_capacity(n);
    for (k, e) in chain.iter().enumerate() {
        let (p, t) = tangent_at(e, 0.0).ok_or_else(|| KernelError::Failed("tangent".into()))?;
        let here = seams.get(k).cloned().unwrap_or_default();
        let extra = match here.first().cloned() {
            None => None,
            Some((side, seam)) => {
                // The plane through the seam square to that side's surface.
                let o = from_p3(seam.front().point());
                let d = (from_p3(seam.back().point()) - o).normalized().ok_or_else(|| KernelError::Failed("seam".into()))?;
                if !matches!(seam.curve(), mt::Curve::Line(_)) {
                    return Err(unsupported("a curved seam where the edge chain turns"));
                }
                let fs = if side == 1 { &s1 } else { &s2 };
                let nn =
                    fs.iter().filter_map(|f| project(f, p, None)).map(|x| x.1).next().ok_or_else(|| KernelError::Failed("seam normal".into()))?;
                Some((side, o, d.cross(nn).normalized().ok_or_else(|| KernelError::Failed("seam plane".into()))?))
            }
        };
        let s = solve(&s1, &s2, p, t, r, sigma, extra, tol * 100.0).ok_or_else(|| unsupported("the ball doesn't fit along the edge"))?;
        // A seam on the other side too must pass through that side's contact.
        if let Some((side, seam)) = here.get(1) {
            let q = if *side == 1 { s.q1 } else { s.q2 };
            let (a, b) = (from_p3(seam.front().point()), from_p3(seam.back().point()));
            if !matches!(seam.curve(), mt::Curve::Line(_)) || q.dist_to_segment(a, b) > tol * 1e3 {
                return Err(unsupported("seams on both sides of a corner of the edge chain"));
            }
        }
        vsec.push(s);
    }
    let mut esec: Vec<Vec<Section>> = Vec::with_capacity(n);
    for (k, e) in chain.iter().enumerate() {
        let mut list = vec![*vsec.get(k).ok_or_else(|| KernelError::Failed("section".into()))?];
        for j in 1..SAMPLES - 1 {
            let (p, t) = tangent_at(e, j as f64 / (SAMPLES - 1) as f64).ok_or_else(|| KernelError::Failed("tangent".into()))?;
            list.push(solve(&s1, &s2, p, t, r, sigma, None, tol * 100.0).ok_or_else(|| unsupported("the ball doesn't fit along the edge"))?);
        }
        list.push(*vsec.get((k + 1) % n).ok_or_else(|| KernelError::Failed("section".into()))?);
        esec.push(list);
    }
    crate::guard("curve blend", || {
        let fail = |m: &str| KernelError::Failed(format!("curve blend: {m}"));
        // Vertices and arcs at the chain's vertices.
        let v1: Vec<mt::Vertex> = vsec.iter().map(|s| builder::vertex(p3(s.q1))).collect();
        let v2: Vec<mt::Vertex> = vsec.iter().map(|s| builder::vertex(p3(s.q2))).collect();
        let mut arcs = Vec::with_capacity(n);
        for (k, s) in vsec.iter().enumerate() {
            let (a, b) = (v1.get(k).ok_or_else(|| fail("vertex"))?, v2.get(k).ok_or_else(|| fail("vertex"))?);
            let e = if round {
                let m = s.c + ((s.q1 - s.c) + (s.q2 - s.c)).normalized().ok_or_else(|| fail("arc"))? * r;
                builder::circle_arc(a, b, p3(m))
            } else {
                builder::line(a, b)
            };
            arcs.push(e);
        }
        // Contact curves and blend faces per edge.
        let mut c1: Vec<mt::Edge> = Vec::with_capacity(n);
        let mut c2: Vec<mt::Edge> = Vec::with_capacity(n);
        let mut blends: Vec<mt::Surface> = Vec::with_capacity(n);
        for (k, list) in esec.iter().enumerate() {
            let q1: Vec<Vec3> = list.iter().map(|s| s.q1).collect();
            let q2: Vec<Vec3> = list.iter().map(|s| s.q2).collect();
            let k1 = (k + 1) % n;
            let (a1, b1, a2, b2) = (v1.get(k), v1.get(k1), v2.get(k), v2.get(k1));
            let (Some(a1), Some(b1), Some(a2), Some(b2)) = (a1, b1, a2, b2) else { return Err(fail("vertex")) };
            let cv1 = crate::build::interpolate_cubic(&q1).ok_or_else(|| fail("contact curve"))?;
            let cv2 = crate::build::interpolate_cubic(&q2).ok_or_else(|| fail("contact curve"))?;
            let e1 = mt::Edge::new(a1, b1, mt::Curve::BSplineCurve(cv1));
            let e2 = mt::Edge::new(a2, b2, mt::Curve::BSplineCurve(cv2));
            // The blend surface: each control row interpolated along the edge (homogeneous),
            // through two sections of each neighbouring edge too, so that neighbouring blend
            // faces meet smoothly.
            let prev = esec.get((k + n - 1) % n).ok_or_else(|| fail("section"))?;
            let next = esec.get(k1).ok_or_else(|| fail("section"))?;
            let mut ext: Vec<Section> = Vec::with_capacity(list.len() + 4);
            if n > 1 || prev.len() > 3 {
                ext.extend(prev.iter().rev().skip(1).take(2).rev().copied());
            }
            ext.extend(list.iter().copied());
            if n > 1 || next.len() > 3 {
                ext.extend(next.iter().skip(1).take(2).copied());
            }
            let ctrl: Vec<[mt::Vector4; 3]> = ext.iter().map(|s| section_controls(s, round)).collect::<Option<_>>().ok_or_else(|| fail("section"))?;
            let mut rows: Vec<Vec<mt::Vector4>> = Vec::with_capacity(3);
            let mut vknots = None;
            for j in 0..3 {
                let xyz: Vec<Vec3> = ctrl.iter().map(|c| Vec3::new(c[j].x, c[j].y, c[j].z)).collect();
                let ww: Vec<Vec3> = ctrl.iter().map(|c| Vec3::new(c[j].w, 0.0, 0.0)).collect();
                let bx = crate::build::interpolate_cubic(&xyz).ok_or_else(|| fail("blend surface"))?;
                let bw = crate::build::interpolate_cubic(&ww).ok_or_else(|| fail("blend surface"))?;
                vknots = Some(bx.knot_vec().clone());
                rows.push(bx.control_points().iter().zip(bw.control_points()).map(|(p, w)| mt::Vector4::new(p.x, p.y, p.z, w.x)).collect());
            }
            let vk = vknots.ok_or_else(|| fail("knots"))?;
            let uk = mt::KnotVec::bezier_knot(2);
            let bs = mt::BSplineSurface::try_new((uk, vk), rows).map_err(|e| fail(&e.to_string()))?;
            let mut surface = mt::Surface::NurbsSurface(mt::NurbsSurface::new(bs));
            // Outward: toward the ball centre on a concave edge, away on a convex one.
            {
                use mt::{ParametricSurface3D, SearchNearestParameter};
                let s = list.get(SAMPLES / 2).ok_or_else(|| fail("section"))?;
                let mid = s.c + ((s.q1 - s.c) + (s.q2 - s.c)).normalized().ok_or_else(|| fail("mid"))? * r;
                if let Some((u, v)) = surface.search_nearest_parameter(p3(mid), None, 100) {
                    let nn = surface.normal(u, v);
                    let want = (s.c - mid) * sigma;
                    if Vec3::new(nn.x, nn.y, nn.z).dot(want) < 0.0 {
                        surface = mt::Invertible::inverse(&surface);
                    }
                }
            }
            c1.push(e1);
            c2.push(e2);
            blends.push(surface);
        }
        // Rebuild the side faces: chain edges become contact curves, seams start at contacts.
        let mut subst: HashMap<mt::EdgeID, (mt::Edge, mt::Edge)> = HashMap::new();
        for (k, e) in chain.iter().enumerate() {
            let (Some(x), Some(y)) = (c1.get(k), c2.get(k)) else { return Err(fail("contact")) };
            subst.insert(e.id(), (x.clone(), y.clone()));
        }
        let mut seam_subst: HashMap<mt::EdgeID, mt::Edge> = HashMap::new();
        for (k, sm) in seams.iter().enumerate().flat_map(|(k, l)| l.iter().map(move |x| (k, x))) {
            let (side, seam) = sm;
            let v = chain.get(k).map(|e| e.front().clone()).ok_or_else(|| fail("vertex"))?;
            let nv = if *side == 1 { v1.get(k) } else { v2.get(k) }.ok_or_else(|| fail("vertex"))?;
            let ne = if *seam.front() == v { builder::line(nv, seam.back()) } else { builder::line(seam.front(), nv) };
            seam_subst.insert(seam.id(), ne);
        }
        let mut out: Vec<mt::Face> = Vec::new();
        for (fi, f) in faces.iter().enumerate() {
            let on1 = side1.contains(&fi);
            let on2 = side2.contains(&fi);
            if !on1 && !on2 && !f.edge_iter().any(|e| seam_subst.contains_key(&e.id())) {
                out.push(f.clone());
                continue;
            }
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut es = Vec::new();
                for e in w.edge_iter() {
                    let ne = if let Some((x, y)) = subst.get(&e.id()) {
                        let base = if on1 { x } else { y };
                        // Same direction as the chain edge, or reversed.
                        let chain_e = chain.iter().find(|c| c.id() == e.id()).ok_or_else(|| fail("edge"))?;
                        if e.front() == chain_e.front() { base.clone() } else { base.inverse() }
                    } else if let Some(s) = seam_subst.get(&e.id()) {
                        if e.front() == e.absolute_front() { s.clone() } else { s.inverse() }
                    } else {
                        e.clone()
                    };
                    es.push(ne);
                }
                wires.push(mt::Wire::from(es));
            }
            out.push(crate::heal::absolute_face(f, wires).ok_or_else(|| fail("a side face could not be rebuilt"))?);
        }
        // Blend faces: each runs its side-1 contact the other way from side 1's face.
        for (k, bf) in blends.iter().enumerate() {
            let (Some(e1), Some(e2), Some(ak), Some(ak1)) = (c1.get(k), c2.get(k), arcs.get(k), arcs.get((k + 1) % n)) else {
                return Err(fail("blend"));
            };
            let f1i = side1.get(k).ok_or_else(|| fail("side"))?;
            let f1 = faces.get(*f1i).ok_or_else(|| fail("side"))?;
            let chain_e = chain.get(k).ok_or_else(|| fail("edge"))?;
            let used_fwd = f1
                .boundary_iters()
                .into_iter()
                .flatten()
                .find(|e| e.id() == chain_e.id())
                .map(|e| e.front() == chain_e.front())
                .ok_or_else(|| fail("edge use"))?;
            // Side 1 runs the contact forward (as the chain) when it ran the edge forward.
            let wire: mt::Wire = if used_fwd {
                vec![e1.inverse(), ak.clone(), e2.clone(), ak1.inverse()].into()
            } else {
                vec![e1.clone(), ak1.clone(), e2.inverse(), ak.inverse()].into()
            };
            let face = mt::Face::try_new(vec![wire], bf.clone()).map_err(|e| fail(&e.to_string()))?;
            out.push(face);
        }
        let mut shells = solid.boundaries().clone();
        if let Some(sh) = shells.get_mut(si) {
            *sh = out.into();
        }
        Solid::try_new(shells).map_err(|e| fail(&format!("invalid solid: {e}")))
    })
}
