//! Operations on bodies: booleans and rigid transforms.

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3, v3};
use crate::{KernelError, Result, guard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

/// Shifts applied to both operands on retries. The boolean classifies some faces by casting a
/// ray whose direction is derived from point coordinates; moving both solids (and the result
/// back) leaves the geometry unchanged but changes those rays.
const JITTER: [[f64; 3]; 4] = [[0.0, 0.0, 0.0], [0.1373, 0.2719, 0.3911], [-0.5127, 0.0833, 0.2291], [0.6911, -0.3337, -0.6127]];

fn volume(b: &Body) -> f64 {
    b.tessellate(b.size() * 2e-3).map(|m| m.measure().volume).unwrap_or(f64::NAN)
}

/// Halton low-discrepancy value (base `b`) for index `i`.
fn halton(mut i: usize, b: usize) -> f64 {
    let (mut f, mut r) = (1.0, 0.0);
    while i > 0 {
        f /= b as f64;
        r += f * (i % b) as f64;
        i /= b;
    }
    r
}

/// Fraction of sample points where `result` disagrees with `op` applied to `a` and `b`
/// (membership by ray parity). Samples cover the overlap box densely and the union box lightly.
fn mismatch(a: &solvecraft_geom::Mesh, b: &solvecraft_geom::Mesh, result: &solvecraft_geom::Mesh, op: BoolOp) -> Option<f64> {
    if a.triangles.len() + b.triangles.len() + result.triangles.len() > 60_000 {
        return None;
    }
    let (ba, bb) = (a.bounds(), b.bounds());
    let lo = ba.min.max(bb.min);
    let hi = ba.max.min(bb.max);
    let all = ba.union(&bb);
    let mut boxes = vec![(all, 300usize)];
    if lo.x < hi.x && lo.y < hi.y && lo.z < hi.z {
        boxes.push((solvecraft_geom::Aabb3 { min: lo, max: hi }, 900));
    }
    let (ia_idx, ib_idx, ir_idx) = (a.inside_index(), b.inside_index(), result.inside_index());
    let (mut n, mut bad) = (0usize, 0usize);
    for (bx, count) in boxes {
        let s = bx.size();
        for i in 1..=count {
            let p = bx.min + Vec3::new(s.x * halton(i, 2), s.y * halton(i, 3), s.z * halton(i, 5));
            let (ia, ib) = (ia_idx.contains(p), ib_idx.contains(p));
            let want = match op {
                BoolOp::Union => ia || ib,
                BoolOp::Cut => ia && !ib,
                BoolOp::Intersect => ia && ib,
            };
            n += 1;
            if ir_idx.contains(p) != want {
                bad += 1;
            }
        }
    }
    Some(bad as f64 / n.max(1) as f64)
}

/// How one body sits relative to another, from sample points: (fraction of `b`'s samples
/// inside `a`, fraction of `a`'s samples inside `b`).
fn overlap(a: &solvecraft_geom::Mesh, b: &solvecraft_geom::Mesh) -> Option<(f64, f64)> {
    if a.triangles.len() + b.triangles.len() > 80_000 {
        return None;
    }
    let (ia, ib) = (a.inside_index(), b.inside_index());
    let frac = |inner: &solvecraft_geom::Mesh, inner_i: &solvecraft_geom::InsideIndex, outer_i: &solvecraft_geom::InsideIndex| -> Option<f64> {
        let bx = inner.bounds();
        let s = bx.size();
        let (mut n, mut hit) = (0usize, 0usize);
        for i in 1..=2000 {
            let p = bx.min + Vec3::new(s.x * halton(i, 2), s.y * halton(i, 3), s.z * halton(i, 5));
            if !inner_i.contains(p) {
                continue;
            }
            n += 1;
            if outer_i.contains(p) {
                hit += 1;
            }
        }
        (n >= 50).then(|| hit as f64 / n as f64)
    };
    Some((frac(b, &ib, &ia)?, frac(a, &ia, &ib)?))
}

/// Booleans that need no intersection: one body clear of the other, or inside it. These are
/// common with coincident curved faces (a hole drilled where one already is, a body joined
/// with itself), which the intersection can't handle.
fn trivial(a: &Body, b: &Body, op: BoolOp, ma: &solvecraft_geom::Mesh, mb: &solvecraft_geom::Mesh) -> Option<Option<Body>> {
    let (b_in_a, a_in_b) = overlap(ma, mb)?;
    const NONE: f64 = 0.003;
    const ALL: f64 = 0.997;
    let disjoint = b_in_a < NONE && a_in_b < NONE;
    match op {
        BoolOp::Cut if disjoint => Some(Some(a.clone())),
        BoolOp::Cut if a_in_b > ALL => Some(None),
        BoolOp::Intersect if disjoint => Some(None),
        BoolOp::Intersect if b_in_a > ALL => Some(Some(b.clone())),
        BoolOp::Intersect if a_in_b > ALL => Some(Some(a.clone())),
        BoolOp::Union if b_in_a > ALL => Some(Some(a.clone())),
        BoolOp::Union if a_in_b > ALL => Some(Some(b.clone())),
        // The tool wholly inside, clear of the outside: a void.
        BoolOp::Cut if b_in_a > ALL && clear_inside(ma, mb, a.size().max(b.size()) * 1e-6) => void(a, b).ok(),
        _ => None,
    }
}

/// Does every vertex of `inner` lie inside `outer` and farther than `gap` from its surface?
fn clear_inside(outer: &solvecraft_geom::Mesh, inner: &solvecraft_geom::Mesh, gap: f64) -> bool {
    if outer.triangles.len() * inner.positions.len() > 40_000_000 {
        return false;
    }
    let tris: Vec<[Vec3; 3]> = outer.triangles.iter().filter_map(|t| outer.tri(t)).collect();
    inner.positions.iter().all(|p| {
        outer.contains(*p)
            && tris.iter().all(|[a, b, c]| {
                // Distance to the triangle's plane bounds the distance to the triangle from below.
                let n = (*b - *a).cross(*c - *a);
                let Some(nn) = n.normalized() else { return true };
                let h = (*p - *a).dot(nn).abs();
                h > gap || {
                    let q = *p - nn * (*p - *a).dot(nn);
                    let inside = [(*a, *b), (*b, *c), (*c, *a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= 0.0);
                    !inside && [(*a, *b), (*b, *c), (*c, *a)].iter().all(|(u, v)| p.dist_to_segment(*u, *v) > gap)
                }
            })
    })
}

/// `a` with the inside of `b` (wholly within it) taken out: a second, inward-facing shell.
fn void(a: &Body, b: &Body) -> Result<Option<Body>> {
    guard("void", || {
        let mut inner = b.deep_copy();
        inner.not();
        let mut shells = a.solid.boundaries().clone();
        shells.extend(inner.boundaries().iter().cloned());
        let s = crate::body::Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("void: {e}")))?;
        Ok(Some(Body::new(s)?))
    })
}

thread_local! {
    /// Set while a boolean runs on bodies whose coincident faces were pushed apart (no
    /// second round of pushing).
    static APART: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Boolean of two bodies. The result may be empty (`Ok(None)`) for a cut that removes
/// everything or an intersection of disjoint bodies. Results are checked against volume bounds
/// and retried with shifted copies and other tolerances when they fail or look wrong.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    a.require_brep("a boolean")?;
    b.require_brep("a boolean")?;
    let size = a.size().max(b.size());
    let tol_m = size * 5e-4;
    let meshes = (a.tessellate(tol_m).ok(), b.tessellate(tol_m).ok());
    let (va, vb) = match &meshes {
        (Some(ma), Some(mb)) => (ma.measure().volume, mb.measure().volume),
        _ => (volume(a), volume(b)),
    };
    let slack = 2e-3 * (va + vb) + 1e-9;
    // Volume bounds, then membership against the operands (catches misclassified pieces), on
    // one mesh of the result. Returns (plausible, volume).
    let check = |r: &Body| -> (bool, f64) {
        let Ok(mr) = r.tessellate(tol_m) else { return (false, f64::NAN) };
        let v = mr.measure().volume;
        let bounds = v > 0.0
            && match op {
                BoolOp::Union => v >= va.max(vb) - slack && v <= va + vb + slack,
                BoolOp::Cut => v >= va - vb - slack && v <= va + slack,
                BoolOp::Intersect => v <= va.min(vb) + slack,
            };
        if !bounds {
            return (false, v);
        }
        let consistent = match &meshes {
            (Some(ma), Some(mb)) => mismatch(ma, mb, &mr, op).is_none_or(|f| f < 0.004),
            _ => true,
        };
        (consistent, v)
    };
    // One body clear of or inside the other: no intersection needed.
    if let (Some(ma), Some(mb)) = &meshes
        && let Some(r) = trivial(a, b, op, ma, mb)
    {
        return Ok(r);
    }
    let mut last = String::new();
    // Coincident faces make the intersection fail after many retries; push them apart first.
    // Prisms along one direction (plates with holes): exact, in 2D (so only the volume
    // bounds are checked).
    if let Some(Ok(r)) = crate::prism::prism_boolean(a, b, op) {
        match &r {
            Some(body) => {
                let v = volume(body);
                let bounds = match op {
                    BoolOp::Union => v >= va.max(vb) - slack && v <= va + vb + slack,
                    BoolOp::Cut => v >= va - vb - slack && v <= va + slack,
                    BoolOp::Intersect => v <= va.min(vb) + slack,
                };
                if v > 0.0 && bounds {
                    return Ok(r);
                }
            }
            None => return Ok(None),
        }
    }
    // Bodies touching along one whole face (a part joined with its mirror image).
    if op == BoolOp::Union
        && a.solid.boundaries().len() == 1
        && b.solid.boundaries().len() == 1
        && let Some(Ok(body)) = crate::coplanar::glue(a, b)
    {
        let (ok, _) = check(&body);
        if ok {
            return Ok(Some(body));
        }
    }
    if !APART.with(|c| c.get()) && crate::coplanar::has_coincident_faces(a, b) {
        APART.with(|c| c.set(true));
        let r = crate::coplanar::boolean_apart(a, b, op);
        APART.with(|c| c.set(false));
        if let Some(Ok(Some(body))) = r {
            let (ok, _) = check(&body);
            if ok {
                return Ok(Some(body));
            }
        }
    }
    let mut empty_votes = 0;
    for (attempt, j) in JITTER.iter().enumerate() {
        let shift = mt::Vector3::new(j[0], j[1], j[2]) * (size * 0.01);
        for k in [2e-3, 5e-4] {
            let tol = (size * k).max(1e-5);
            let r = guard("boolean", || {
                let (mut sa, mut sb) = (a.deep_copy(), b.deep_copy());
                if attempt > 0 {
                    sa = mt::builder::translated(&sa, shift);
                    sb = mt::builder::translated(&sb, shift);
                }
                let res = match op {
                    BoolOp::Union => truck_shapeops::or(&sa, &sb, tol),
                    BoolOp::Cut => {
                        sb.not();
                        truck_shapeops::and(&sa, &sb, tol)
                    }
                    BoolOp::Intersect => truck_shapeops::and(&sa, &sb, tol),
                };
                let s = res.ok_or_else(|| KernelError::Failed("no result".into()))?;
                Ok(if attempt > 0 { mt::builder::translated(&s, -shift) } else { s })
            });
            match r {
                Ok(s) if s.boundaries().is_empty() || s.face_iter().next().is_none() => {
                    empty_votes += 1;
                    last = "empty result".into();
                }
                Ok(s) => match Body::new(crate::heal::heal(s, size)) {
                    Ok(body) => {
                        let (ok, v) = check(&body);
                        if ok {
                            return Ok(Some(body));
                        }
                        last = format!("implausible result volume {v:.4}");
                    }
                    Err(e) => last = e.to_string(),
                },
                Err(e) => last = e.to_string(),
            }
        }
        if empty_votes >= 2 && op != BoolOp::Union {
            return Ok(None);
        }
    }
    // Coincident planar faces: push them apart (exactly) and try again.
    if !APART.with(|c| c.get()) {
        APART.with(|c| c.set(true));
        let r = crate::coplanar::boolean_apart(a, b, op);
        APART.with(|c| c.set(false));
        match r {
            Some(Ok(Some(body))) => {
                let (ok, v) = check(&body);
                if ok {
                    return Ok(Some(body));
                }
                last = format!("{last}; faces pushed apart gave an implausible volume {v:.4}");
            }
            Some(Ok(None)) => {}
            Some(Err(e)) => last = format!("{last}; {e}"),
            None => {}
        }
    }
    // Coincident faces defeat the B-rep boolean; bodies with only planar faces have an exact
    // polygon fallback.
    match crate::polybool::planar_boolean(a, b, op) {
        Ok(Some(body)) => {
            let (ok, v) = check(&body);
            if ok {
                return Ok(Some(body));
            }
            last = format!("planar fallback gave an implausible volume {v:.4}");
        }
        Ok(None) => return Ok(None),
        Err(e) => last = format!("{last}; {e}"),
    }
    // Last resort: nudge the tool by a micron-scale screw motion (turned 2e-6 rad about a
    // skew axis and moved 2e-6 of the size), which breaks tangencies and seams lying exactly on
    // the other body. The result moves by no more than that.
    if !APART.with(|c| c.get()) {
        APART.with(|c| c.set(true));
        let c = b.solid.vertex_iter().fold(Vec3::ZERO, |acc, v| acc + from_p3(v.point())) * (1.0 / b.solid.vertex_iter().count().max(1) as f64);
        let r = transform(b, Vec3::new(0.61, -0.37, 0.71) * (size * 2e-6), c, Vec3::new(0.27, 0.83, -0.49), 2e-6).and_then(|nb| boolean(a, &nb, op));
        APART.with(|c| c.set(false));
        match r {
            Ok(Some(body)) => {
                let (ok, v) = check(&body);
                if ok {
                    return Ok(Some(body));
                }
                last = format!("{last}; the nudged tool gave an implausible volume {v:.4}");
            }
            Ok(None) => return Ok(None),
            Err(e) => last = format!("{last}; nudged: {e}"),
        }
    }
    Err(KernelError::Failed(format!("boolean {op:?}: {last}")))
}

/// Affine transform by a column-major 4×4 matrix (rotations, translations, reflections).
pub fn transform_matrix(body: &Body, m: [[f64; 4]; 4]) -> Result<Body> {
    if m.iter().flatten().any(|x| !x.is_finite()) {
        return Err(KernelError::Invalid("non-finite transform".into()));
    }
    if let Some(b) = body.mesh_transformed(&m) {
        return Ok(b);
    }
    guard("transform", || {
        let mat = mt::Matrix4::new(
            m[0][0], m[0][1], m[0][2], m[0][3], m[1][0], m[1][1], m[1][2], m[1][3], m[2][0], m[2][1], m[2][2], m[2][3], m[3][0], m[3][1], m[3][2],
            m[3][3],
        );
        Body::new(mt::builder::transformed(&body.deep_copy(), mat))
    })
}

/// Rigid transform: rotate about `axis` through `origin` by `angle` radians, then translate.
pub fn transform(body: &Body, translate: Vec3, origin: Vec3, axis: Vec3, angle: f64) -> Result<Body> {
    if !(translate.is_finite() && origin.is_finite() && angle.is_finite()) {
        return Err(KernelError::Invalid("non-finite transform".into()));
    }
    if body.is_mesh() {
        // Same motion as a matrix: rotate about the line through `origin`, then translate.
        let ax = if angle.abs() > 1e-12 { axis.normalized().ok_or_else(|| KernelError::Invalid("rotation axis".into()))? } else { Vec3::Z };
        let (c, s) = (angle.cos(), angle.sin());
        let t = 1.0 - c;
        let r = [
            [t * ax.x * ax.x + c, t * ax.x * ax.y + s * ax.z, t * ax.x * ax.z - s * ax.y],
            [t * ax.x * ax.y - s * ax.z, t * ax.y * ax.y + c, t * ax.y * ax.z + s * ax.x],
            [t * ax.x * ax.z + s * ax.y, t * ax.y * ax.z - s * ax.x, t * ax.z * ax.z + c],
        ];
        let ro = Vec3::new(
            r[0][0] * origin.x + r[1][0] * origin.y + r[2][0] * origin.z,
            r[0][1] * origin.x + r[1][1] * origin.y + r[2][1] * origin.z,
            r[0][2] * origin.x + r[1][2] * origin.y + r[2][2] * origin.z,
        );
        let tr = origin - ro + translate;
        let m = [[r[0][0], r[0][1], r[0][2], 0.0], [r[1][0], r[1][1], r[1][2], 0.0], [r[2][0], r[2][1], r[2][2], 0.0], [tr.x, tr.y, tr.z, 1.0]];
        return body.mesh_transformed(&m).ok_or_else(|| KernelError::Failed("mesh transform".into()));
    }
    guard("transform", || {
        let mut s = body.deep_copy();
        if angle.abs() > 1e-12 {
            let ax = axis.normalized().ok_or_else(|| KernelError::Invalid("rotation axis".into()))?;
            s = mt::builder::rotated(&s, p3(origin), v3(ax), mt::Rad(angle));
        }
        if translate.len() > 0.0 {
            s = mt::builder::translated(&s, v3(translate));
        }
        Body::new(s)
    })
}

/// Split a body with a plane: the parts on the positive and the negative side of the plane
/// (only the non-empty ones).
pub fn split_by_plane(body: &Body, plane: &solvecraft_geom::Plane) -> Result<Vec<Body>> {
    body.require_brep("split")?;
    let size = body.size();
    let mut bb = solvecraft_geom::Aabb3::EMPTY;
    for v in body.solid.vertex_iter() {
        bb.add(crate::body::from_p3(v.point()));
    }
    let reach = (bb.diagonal() + plane.origin.dist(bb.center())) * 2.0 + size;
    let c = plane.to_local(bb.center());
    let sq = solvecraft_geom::Region2 {
        outer: solvecraft_geom::Loop2::polygon(&[
            solvecraft_geom::Vec2::new(c.x - reach, c.y - reach),
            solvecraft_geom::Vec2::new(c.x + reach, c.y - reach),
            solvecraft_geom::Vec2::new(c.x + reach, c.y + reach),
            solvecraft_geom::Vec2::new(c.x - reach, c.y + reach),
        ]),
        holes: vec![],
    };
    let mut out = Vec::new();
    for (lo, hi) in [(0.0, reach), (-reach, 0.0)] {
        let half = crate::build::extrude(plane, std::slice::from_ref(&sq), lo, hi)?.pop().ok_or_else(|| KernelError::Failed("half space".into()))?;
        if let Some(part) = boolean(body, &half, BoolOp::Intersect)? {
            out.push(part);
        }
    }
    Ok(out)
}
