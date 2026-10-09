//! Raising a band of a planar face (a plastic part's lip): the band along one of the face's
//! boundary loops, a set width into the face, is pushed out along the face's normal. Built
//! directly from the body's faces (the band's walls continue the walls below, which a boolean
//! union can't join: their faces would coincide).

use solvecraft_geom::Vec3;
use truck_modeling::{self as mt, builder};

use crate::body::{Body, Solid, from_p3, p3, v3};
use crate::{KernelError, Result, guard};

fn fail(m: &str) -> KernelError {
    KernelError::Failed(format!("raising a band: {m}"))
}

/// A boundary edge of the face, as its ends and (for an arc) a point halfway.
#[derive(Clone, Copy)]
struct Piece {
    a: Vec3,
    b: Vec3,
    mid: Option<Vec3>,
}

/// The planar face of `body` at `at`: its index and outward normal.
fn face_at(body: &Body, at: Vec3) -> Result<(usize, Vec3)> {
    let size = body.size();
    let mesh = body.tessellate((size * 1e-3).max(1e-3))?;
    let near = mesh
        .triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter_map(|(t, f)| {
            let [a, b, c] = mesh.tri(t)?;
            let n = (b - a).cross(c - a).normalized()?;
            let q = at - n * (at - a).dot(n);
            let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= -1e-9);
            inside.then_some(((at - a).dot(n).abs(), *f as usize))
        })
        .min_by(|x, y| x.0.total_cmp(&y.0));
    let (d, f) = near.ok_or_else(|| fail("no face there"))?;
    if d > size * 1e-3 {
        return Err(fail("no face there"));
    }
    let face = body.solid.face_iter().nth(f).ok_or_else(|| fail("face"))?;
    let mt::Surface::Plane(pl) = face.oriented_surface() else { return Err(fail("the face must be planar")) };
    let n = pl.normal();
    Ok((f, Vec3::new(n.x, n.y, n.z).normalized().ok_or_else(|| fail("normal"))?))
}

/// The face's loop moved `w` into the face (to the left of its edges, as a face's boundary has
/// its inside on the left), corners mitred where the offsets don't already meet.
fn offset_loop(pieces: &[Piece], n: Vec3, w: f64, tol: f64) -> Result<Vec<Piece>> {
    let left = |a: Vec3, b: Vec3| n.cross(b - a).normalized();
    let mut out: Vec<Piece> = Vec::new();
    for p in pieces {
        out.push(match p.mid {
            None => {
                let l = left(p.a, p.b).ok_or_else(|| fail("zero edge"))?;
                Piece { a: p.a + l * w, b: p.b + l * w, mid: None }
            }
            Some(m) => {
                // Each point moves square to the arc (along its radius), to the left.
                let (la, lm, lb) =
                    (left(p.a, m).ok_or_else(|| fail("arc"))?, left(p.a, p.b).ok_or_else(|| fail("arc"))?, left(m, p.b).ok_or_else(|| fail("arc"))?);
                let c = circle_centre(p.a, m, p.b).ok_or_else(|| fail("arc"))?;
                let radial = |q: Vec3, hint: Vec3| {
                    let r = (q - c).normalized().unwrap_or(hint);
                    if r.dot(hint) >= 0.0 { r } else { -r }
                };
                let _ = (la, lb);
                Piece { a: p.a + radial(p.a, lm) * w, b: p.b + radial(p.b, lm) * w, mid: Some(m + radial(m, lm) * w) }
            }
        });
    }
    // Joints: where the offsets don't meet (sharp corners of lines), the lines are extended or
    // cut back to their crossing.
    let k = out.len();
    for i in 0..k {
        let j = (i + 1) % k;
        let (Some(pi), Some(pj)) = (out.get(i).copied(), out.get(j).copied()) else { continue };
        if pi.b.dist(pj.a) < tol {
            if let Some(x) = out.get_mut(j) {
                x.a = pi.b;
            }
            continue;
        }
        match (pi.mid, pj.mid) {
            (None, None) => {
                let (d1, d2) = (pi.b - pi.a, pj.b - pj.a);
                let w0 = pj.a - pi.a;
                let cr = d1.cross(d2);
                let den = cr.len2();
                if den < 1e-24 {
                    return Err(fail("parallel edges at a corner"));
                }
                let t = w0.cross(d2).dot(cr) / den;
                let x = pi.a + d1 * t;
                if let Some(e) = out.get_mut(i) {
                    e.b = x;
                }
                if let Some(e) = out.get_mut(j) {
                    e.a = x;
                }
            }
            _ => return Err(fail("an arc meeting another edge at a corner")),
        }
    }
    Ok(out)
}

fn circle_centre(a: Vec3, m: Vec3, b: Vec3) -> Option<Vec3> {
    let (u, v) = (b - a, m - a);
    let w = u.cross(v);
    let d = 2.0 * w.len2();
    if d < 1e-300 {
        return None;
    }
    Some(a + (v.cross(w) * u.len2() + w.cross(u) * v.len2()) * (1.0 / d))
}

/// Raise the band of the planar face at `at` that runs `width` wide along its outer loop
/// (`outside`) or its inner loop, by `height` along the face's normal.
pub fn raise_band(body: &Body, at: Vec3, width: f64, height: f64, outside: bool) -> Result<Body> {
    body.require_brep("a lip")?;
    if !(width > 1e-6 && height > 1e-6 && width.is_finite() && height.is_finite()) {
        return Err(KernelError::Invalid("the band's width and height must be positive".into()));
    }
    if body.solid.boundaries().len() != 1 {
        return Err(fail("a body in one piece"));
    }
    let size = body.size();
    let tol = (size * 1e-7).max(1e-9) * 100.0;
    let (fi, n) = face_at(body, at)?;
    let faces: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let f = faces.get(fi).ok_or_else(|| fail("face"))?;
    let wires = f.boundaries();
    if wires.len() != 2 {
        return Err(fail("the face must have an outer and one inner loop (a rim)"));
    }
    // Outer loop: turning positively about n.
    let turn = |w: &mt::Wire| -> f64 {
        let pts: Vec<Vec3> = w.vertex_iter().map(|v| from_p3(v.point())).collect();
        let mut s = Vec3::ZERO;
        for (k, p) in pts.iter().enumerate() {
            if let Some(q) = pts.get((k + 1) % pts.len()) {
                s += p.cross(*q);
            }
        }
        s.dot(n)
    };
    let (w0, w1) = (wires.first().ok_or_else(|| fail("loop"))?, wires.get(1).ok_or_else(|| fail("loop"))?);
    let (outer, inner) = if turn(w0) >= turn(w1) { (w0, w1) } else { (w1, w0) };
    let (along, other) = if outside { (outer, inner) } else { (inner, outer) };
    let pieces: Vec<Piece> = along
        .edge_iter()
        .map(|e| {
            let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
            match crate::offset::arc_mid(e, size * 1e-6) {
                Some(None) => Ok(Piece { a, b, mid: None }),
                Some(Some(m)) => Ok(Piece { a, b, mid: Some(m) }),
                None => Err(fail("edges must be lines or arcs")),
            }
        })
        .collect::<Result<_>>()?;
    let moved = offset_loop(&pieces, n, width, tol)?;
    guard("lip", || {
        // The band's other edge, on the face.
        let verts: Vec<mt::Vertex> = moved.iter().map(|p| builder::vertex(p3(p.a))).collect();
        let k = verts.len();
        let mut cedges: Vec<mt::Edge> = Vec::with_capacity(k);
        for (i, p) in moved.iter().enumerate() {
            let (Some(a), Some(b)) = (verts.get(i), verts.get((i + 1) % k)) else { return Err(fail("vertex")) };
            cedges.push(match p.mid {
                None => builder::line(a, b),
                Some(m) => builder::circle_arc(a, b, p3(m)),
            });
        }
        let c: mt::Wire = cedges.into();
        // The rest of the face: its other loop, and the band's edge as a loop of its own.
        // (The band's edge runs the face's way round: a hole when the band follows the inner
        // loop, the outer loop when it follows the outer.)
        let rest = mt::Face::try_new(vec![other.clone(), c.clone()], f.oriented_surface()).map_err(|e| fail(&e.to_string()))?;
        // Walls: the face's loop and the band's edge swept out; the band's top.
        let up = v3(n * height);
        let wall_along: mt::Shell = builder::tsweep(along, up);
        let wall_c: mt::Shell = builder::tsweep(&c, up);
        let top_of = |sh: &mt::Shell| -> Vec<mt::Wire> {
            sh.extract_boundaries().into_iter().filter(|w| w.vertex_iter().all(|v| (from_p3(v.point()) - at).dot(n) > height * 0.5)).collect()
        };
        let flipped = |sh: &mt::Shell, yes: bool| -> mt::Shell {
            if yes { sh.face_iter().map(|x| x.inverse()).collect::<Vec<_>>().into() } else { sh.clone() }
        };
        let mut last = String::new();
        for flip in 0..8u32 {
            let (wa, wc) = (flipped(&wall_along, flip & 1 == 1), flipped(&wall_c, flip & 2 == 2));
            let mut tops = top_of(&wa);
            tops.extend(top_of(&wc));
            if tops.len() != 2 {
                return Err(fail("the band's top"));
            }
            // The top closes both walls: its loops run against theirs.
            let tops: Vec<mt::Wire> = tops.iter().map(|w| w.inverse()).collect();
            let Ok(top) = builder::try_attach_plane(&tops) else { continue };
            let top = if flip & 4 == 4 { top.inverse() } else { top };
            let mut all: Vec<mt::Face> = faces.iter().enumerate().filter(|(i, _)| *i != fi).map(|(_, x)| x.clone()).collect();
            all.push(rest.clone());
            all.extend(wa.face_iter().cloned());
            all.extend(wc.face_iter().cloned());
            all.push(top);
            match Solid::try_new(vec![all.into()]) {
                Ok(s) => {
                    let b = Body::new(s)?;
                    if b.validity().is_empty() {
                        return Ok(b);
                    }
                    last = b.validity().join("; ");
                }
                Err(e) => last = e.to_string(),
            }
        }
        Err(fail(&last))
    })
}
