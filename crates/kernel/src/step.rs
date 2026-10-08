use truck_modeling as mt;
use truck_stepio::out;

use crate::body::Body;
use crate::{KernelError, Result, guard};

/// Intersection-curve edges (from booleans) as cubic B-splines through the exact curve: STEP
/// readers get plain 3D geometry that lies on both faces, and the file does not repeat the two
/// surfaces for every such edge.
fn plain_curves(b: &Body) -> mt::Solid {
    let s = b.deep_copy();
    let size = b.size();
    let mut seen = std::collections::HashSet::new();
    for e in s.edge_iter() {
        if !seen.insert(e.id()) {
            continue;
        }
        if let mt::Curve::IntersectionCurve(ic) = e.curve()
            && let Some(c) = hermite_through(&ic, size * 1e-4)
        {
            e.set_curve(mt::Curve::BSplineCurve(c));
        }
    }
    s
}

/// A C¹ cubic B-spline through exact points of `c` spaced so that chords deviate at most `tol`
/// (Catmull–Rom tangents over chord length; the cubic is far closer than the chords).
fn hermite_through<C>(c: &C, tol: f64) -> Option<mt::BSplineCurve<mt::Point3>>
where
    C: mt::ParametricCurve<Point = mt::Point3, Vector = mt::Vector3> + mt::BoundedCurve + mt::ParameterDivision1D<Point = mt::Point3>,
{
    use mt::{InnerSpace, MetricSpace};
    let (_, pts) = c.parameter_division(c.range_tuple(), tol);
    let pts: Vec<mt::Point3> = pts.into_iter().fold(Vec::new(), |mut v: Vec<mt::Point3>, p| {
        if v.last().is_none_or(|q| q.distance(p) > tol * 1e-3) {
            v.push(p);
        }
        v
    });
    let n = pts.len().checked_sub(1).filter(|n| *n >= 1 && *n < 1_000_000)?;
    let mut u = vec![0.0; n + 1];
    for i in 1..=n {
        u[i] = u[i - 1] + pts[i].distance(pts[i - 1]);
    }
    let tangent = |i: usize| {
        let (a, b) = (i.saturating_sub(1), (i + 1).min(n));
        (pts[b] - pts[a]) / (u[b] - u[a])
    };
    let mut ctrl = vec![pts[0]];
    let mut knots = vec![u[0]; 4];
    for i in 0..n {
        let h = u[i + 1] - u[i];
        ctrl.push(pts[i] + tangent(i) * (h / 3.0));
        ctrl.push(pts[i + 1] - tangent(i + 1) * (h / 3.0));
        ctrl.push(pts[i + 1]);
        knots.extend(std::iter::repeat_n(u[i + 1], if i + 1 == n { 4 } else { 3 }));
    }
    if ctrl.iter().any(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) || tangent(0).magnitude() == 0.0 {
        return None;
    }
    mt::BSplineCurve::try_new(mt::KnotVec::from(knots), ctrl).ok()
}

/// truck's STEP text for the given bodies (geometry and topology; product data is ours).
pub(crate) fn truck_step(bodies: &[&Body], system: &str) -> Result<String> {
    if bodies.is_empty() {
        return Err(KernelError::Invalid("nothing to export".into()));
    }
    for b in bodies {
        b.require_brep("STEP export")?;
    }
    guard("step export", || {
        let cs: Vec<_> = bodies.iter().map(|b| plain_curves(b).compress()).collect();
        let models: out::StepModels<_, _, _> = cs.iter().collect();
        let header = out::StepHeaderDescriptor { organization_system: system.to_owned(), ..Default::default() };
        Ok(out::CompleteStepDisplay::new(models, header).to_string())
    })
}

/// STEP (ISO 10303-21, AP242) text for the given bodies as one part (`Body1`…, with the
/// bodies' colours).
pub fn step_export(bodies: &[&Body], system: &str) -> Result<String> {
    let list: Vec<crate::ExportBody> =
        bodies.iter().enumerate().map(|(i, b)| crate::ExportBody { name: format!("Body{}", i + 1), body: b, color: b.color() }).collect();
    let header = crate::StepHeader { organization: system.to_string(), ..Default::default() };
    crate::step_out::step_export_bodies(&list, &header)
}
