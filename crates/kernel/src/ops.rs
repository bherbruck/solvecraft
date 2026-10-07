//! Operations on bodies: booleans and rigid transforms.

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;
use truck_modeling as mt;

use crate::body::{Body, p3, v3};
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

/// Boolean of two bodies. The result may be empty (`Ok(None)`) for a cut that removes
/// everything or an intersection of disjoint bodies. Results are checked against volume bounds
/// and retried with shifted copies and other tolerances when they fail or look wrong.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    let size = a.size().max(b.size());
    let (va, vb) = (volume(a), volume(b));
    let slack = 2e-3 * (va + vb) + 1e-9;
    let plausible = |v: f64| match op {
        BoolOp::Union => v >= va.max(vb) - slack && v <= va + vb + slack,
        BoolOp::Cut => v >= va - vb - slack && v <= va + slack,
        BoolOp::Intersect => v <= va.min(vb) + slack,
    };
    let mut last = String::new();
    let mut empty_votes = 0;
    for (attempt, j) in JITTER.iter().enumerate() {
        let shift = mt::Vector3::new(j[0], j[1], j[2]) * (size * 0.01);
        for k in [5e-4, 2e-3] {
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
                Ok(s) => match Body::new(s) {
                    Ok(body) => {
                        let v = volume(&body);
                        if v > 0.0 && plausible(v) {
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
    Err(KernelError::Failed(format!("boolean {op:?}: {last}")))
}

/// Affine transform by a column-major 4×4 matrix (rotations, translations, reflections).
pub fn transform_matrix(body: &Body, m: [[f64; 4]; 4]) -> Result<Body> {
    if m.iter().flatten().any(|x| !x.is_finite()) {
        return Err(KernelError::Invalid("non-finite transform".into()));
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
