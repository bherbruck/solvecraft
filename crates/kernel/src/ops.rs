//! Operations on bodies: booleans, fillets, chamfers, rigid transforms.

use monstertruck_modeling as mt;
use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec3;

use crate::body::{Body, Solid, v3};
use crate::{KernelError, Result, guard};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoolOp {
    Union,
    Cut,
    Intersect,
}

/// Boolean of two bodies. Retries with a few tolerances; the result may be empty (`Ok(None)`)
/// for a cut that removes everything or an intersection of disjoint bodies.
pub fn boolean(a: &Body, b: &Body, op: BoolOp) -> Result<Option<Body>> {
    let size = a.size().max(b.size());
    let mut last = String::new();
    for k in [1e-4, 1e-3, 5e-5, 5e-3] {
        let tol = (size * k).max(1e-6);
        let r = guard("boolean", || {
            let (sa, sb) = (a.deep_copy(), b.deep_copy());
            let res = match op {
                BoolOp::Union => monstertruck_solid::or(&sa, &sb, tol),
                BoolOp::Cut => monstertruck_solid::difference(&sa, &sb, tol),
                BoolOp::Intersect => monstertruck_solid::and(&sa, &sb, tol),
            };
            res.map_err(|e| KernelError::Failed(format!("{e:?}")))
        });
        match r {
            Ok(s) if s.is_empty() => return Ok(None),
            Ok(s) => {
                let body = Body::new(s)?;
                if body.tessellate(size * 1e-2).map(|m| m.measure().volume > 0.0).unwrap_or(false) {
                    return Ok(Some(body));
                }
                last = "degenerate result".into();
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(KernelError::Failed(format!("boolean {op:?}: {last}")))
}

fn blend(body: &Body, edges: &[Vec3], radius: f64, profile: mt::FilletProfile, what: &str) -> Result<Body> {
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid(format!("{what} size must be positive")));
    }
    if edges.is_empty() {
        return Err(KernelError::Invalid("no edges selected".into()));
    }
    let tol = (body.size() * 2e-3).max(1e-3);
    let mut cur = body.clone();
    // One edge at a time: re-find each edge on the current body by its reference point.
    for p in edges {
        let Some((idx, d)) = cur.nearest_edge(*p, tol)? else { return Err(KernelError::Invalid("body has no edges".into())) };
        if d > cur.size() * 0.05 + 1e-3 {
            return Err(KernelError::Invalid(format!("no edge near {:?}", [p.x, p.y, p.z])));
        }
        let mut solid = cur.deep_copy();
        let shape = profile.clone();
        let res = guard(what, || {
            let edge = Body::unique_edges(&solid).into_iter().nth(idx).ok_or_else(|| KernelError::Invalid("edge index".into()))?;
            let shell_i = solid
                .boundaries()
                .iter()
                .position(|sh| sh.edge_iter().any(|e| e.id() == edge.id()))
                .ok_or_else(|| KernelError::Failed("edge not in a shell".into()))?;
            let mut shells = solid.boundaries().clone();
            let shell = shells.get_mut(shell_i).ok_or_else(|| KernelError::Failed("shell".into()))?;
            let opts = mt::FilletOptions { radius: mt::RadiusSpec::Constant(radius), profile: shape, ..Default::default() };
            mt::fillet_edges(shell, &[edge], Some(&opts)).map_err(|e| KernelError::Failed(format!("{what}: {e:?}")))?;
            solid = Solid::try_new(shells).map_err(|e| KernelError::Failed(format!("{what} left an open shell: {e:?}")))?;
            Ok(())
        });
        res?;
        cur = Body::new(solid)?;
    }
    Ok(cur)
}

/// Constant-radius fillet of the edges nearest to the given points.
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    blend(body, edges, radius, mt::FilletProfile::Round, "fillet")
}

/// Equal-distance chamfer of the edges nearest to the given points.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    blend(body, edges, distance, mt::FilletProfile::Chamfer, "chamfer")
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
            s = mt::builder::rotated(&s, crate::body::p3(origin), v3(ax), mt::Rad(angle));
        }
        if translate.len() > 0.0 {
            s = mt::builder::translated(&s, v3(translate));
        }
        Body::new(s)
    })
}
