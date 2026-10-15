//! Projected trim validation for graph patches. Positive-weight Bezier hulls
//! certify separation between boundaries, including between neighboring patches.

use super::*;
use std::collections::{HashMap, HashSet};

type H = mt::Vector3;
type P = (f64, f64);

fn point(p: H) -> P {
    (p.x / p.z, p.y / p.z)
}
fn split(mut row: Vec<H>) -> Result<(Vec<H>, Vec<H>)> {
    let mut left = vec![*row.first().ok_or_else(|| unsupported("empty patch boundary"))?];
    let mut right = vec![*row.last().ok_or_else(|| unsupported("empty patch boundary"))?];
    while row.len() > 1 {
        row = row
            .windows(2)
            .filter_map(|p| match p {
                [a, b] => Some((*a + *b) * 0.5),
                _ => None,
            })
            .collect();
        left.push(*row.first().ok_or_else(|| unsupported("patch boundary split"))?);
        right.push(*row.last().ok_or_else(|| unsupported("patch boundary split"))?);
    }
    right.reverse();
    Ok((left, right))
}
fn bounds(c: &[H]) -> [f64; 4] {
    c.iter()
        .map(|p| point(*p))
        .fold([f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY], |[a, b, c, d], (x, y)| [a.min(x), b.max(x), c.min(y), d.max(y)])
}
fn near(bounds: [f64; 4], p: P, tol: f64) -> bool {
    let [a, b, c, d] = bounds;
    (a - p.0).abs().max((b - p.0).abs()).hypot((c - p.1).abs().max((d - p.1).abs())) <= tol
}

// Axis-aligned boxes are inconclusive near two nearly tangent boundaries.
// A strict separating axis of their positive-weight control hulls is also a
// certificate, regardless of the direction of the trimming curves.
fn separated_hulls(a: &[H], b: &[H]) -> bool {
    let (a, b): (Vec<_>, Vec<_>) = (a.iter().map(|p| point(*p)).collect(), b.iter().map(|p| point(*p)).collect());
    for polygon in [&a, &b] {
        for (p, q) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            let (nx, ny) = (q.1 - p.1, p.0 - q.0);
            if nx == 0.0 && ny == 0.0 {
                continue;
            }
            let interval = |points: &[P]| {
                points
                    .iter()
                    .map(|r| (r.0 - p.0) * nx + (r.1 - p.1) * ny)
                    .try_fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| v.is_finite().then_some((lo.min(v), hi.max(v))))
            };
            let (Some((al, ah)), Some((bl, bh))) = (interval(&a), interval(&b)) else { continue };
            if ah < bl || bh < al {
                return true;
            }
        }
    }
    false
}

fn separate(a: Vec<H>, b: Vec<H>, shared: &[P], tol: f64, budget: &mut usize) -> Result<()> {
    let mut stack = vec![(a, b, 0)];
    while let Some((a, b, depth)) = stack.pop() {
        *budget += 1;
        if *budget > 262144 || depth > 56 {
            return Err(unsupported("patch boundary separation exceeds its budget"));
        }
        let (aa, bb) = (bounds(&a), bounds(&b));
        if aa[1] < bb[0] || bb[1] < aa[0] || aa[3] < bb[2] || bb[3] < aa[2] || separated_hulls(&a, &b) {
            continue;
        }
        if shared.iter().any(|p| near(aa, *p, tol) && near(bb, *p, tol)) {
            continue;
        }
        let da = (aa[1] - aa[0]).hypot(aa[3] - aa[2]);
        let db = (bb[1] - bb[0]).hypot(bb[3] - bb[2]);
        if da.max(db) < tol * 0.1 {
            return Err(unsupported("transition boundaries cross in projection"));
        }
        if da >= db {
            let (left, right) = split(a)?;
            stack.push((left, b.clone(), depth + 1));
            stack.push((right, b, depth + 1));
        } else {
            let (left, right) = split(b)?;
            stack.push((a.clone(), left, depth + 1));
            stack.push((a, right, depth + 1));
        }
    }
    Ok(())
}

fn simple(c: Vec<H>, tol: f64, budget: &mut usize) -> Result<()> {
    let mut stack = vec![(c, 0)];
    while let Some((c, depth)) = stack.pop() {
        if depth > 24 {
            return Err(unsupported("patch boundary reverses in projection"));
        }
        let a = point(*c.first().ok_or_else(|| unsupported("empty patch curve"))?);
        let b = point(*c.last().ok_or_else(|| unsupported("empty patch curve"))?);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        if dx.hypot(dy) > tol * 0.01
            && c.windows(2).all(|p| {
                let [p, q] = p else { return false };
                let (p, q) = (point(*p), point(*q));
                (q.0 - p.0) * dx + (q.1 - p.1) * dy >= 0.0
            })
        {
            continue;
        }
        let (left, right) = split(c)?;
        let joint = point(*left.last().ok_or_else(|| unsupported("patch curve split"))?);
        separate(left.clone(), right.clone(), &[joint], tol, budget)?;
        stack.push((left, depth + 1));
        stack.push((right, depth + 1));
    }
    Ok(())
}

/// All surfaces fitted to these wires use the same graph direction. Jordan
/// boundaries with the same winding and opposite uses of shared edges partition
/// that projection, so the graph interiors cannot pass through one another.
pub(crate) fn check(wires: &[Vec<mt::Edge>], direction: Vec3, size: f64) -> Result<()> {
    if !size.is_finite()
        || size <= 0.0
        || !direction.is_finite()
        || wires.is_empty()
        || wires.len() > 16
        || wires.iter().any(|w| w.len() < 3 || w.len() > 16)
    {
        return Err(unsupported("invalid patch partition size"));
    }
    let z = direction.normalized().ok_or_else(|| unsupported("patch partition direction"))?;
    let x = z.any_perp();
    let y = z.cross(x);
    let tol = (size * 1e-7).max(1e-7);
    let mut directions = HashMap::new();
    let mut shared_edges = HashSet::new();
    let mut arcs = Vec::new();
    let mut polygons = Vec::new();
    let mut budget = 0;
    for wire in wires {
        let mut polygon = Vec::new();
        for edge in wire {
            let curve = edge.oriented_curve();
            // Degree eight accommodates the exact cylindrical boundary with
            // cubic angular and quadratic axial progression; budgets stay finite.
            let bounded = match &curve {
                mt::Curve::Line(_) => true,
                mt::Curve::BSplineCurve(c) => c.control_points().len() <= 256 && c.degree() <= 8,
                mt::Curve::NurbsCurve(c) => c.control_points().len() <= 256 && c.degree() <= 8,
                _ => false,
            };
            if !bounded {
                return Err(unsupported("patch partition curve budget"));
            }
            let (lo, hi) = curve.range_tuple();
            if !lo.is_finite() || !hi.is_finite() || hi <= lo {
                return Err(unsupported("invalid patch partition range"));
            }
            for i in 0..512 {
                let p = from_p3(curve.subs(lo + (hi - lo) * i as f64 / 512.0));
                if !p.is_finite() {
                    return Err(unsupported("invalid patch boundary point"));
                }
                polygon.push((p.dot(x), p.dot(y)));
            }
            if let Some(forward) = directions.get(&edge.id()) {
                if *forward == edge.orientation() || !shared_edges.insert(edge.id()) {
                    return Err(unsupported("patch domains must use a shared edge on opposite sides"));
                }
                continue;
            }
            directions.insert(edge.id(), edge.orientation());
            let (knots, controls): (mt::KnotVec, Vec<mt::Vector4>) = match edge.curve() {
                mt::Curve::Line(line) => {
                    (mt::KnotVec::bezier_knot(1), [line.0, line.1].into_iter().map(|p| mt::Vector4::new(p.x, p.y, p.z, 1.0)).collect())
                }
                mt::Curve::BSplineCurve(c) => {
                    (c.knot_vec().clone(), c.control_points().iter().map(|p| mt::Vector4::new(p.x, p.y, p.z, 1.0)).collect())
                }
                mt::Curve::NurbsCurve(c) => (c.knot_vec().clone(), c.control_points().clone()),
                _ => return Err(unsupported("unsupported patch partition boundary")),
            };
            let degree = knots.len().checked_sub(controls.len() + 1).ok_or_else(|| unsupported("patch boundary degree"))?;
            if controls.len() > 256
                || !(1..=8).contains(&degree)
                || knots.iter().any(|k| !k.is_finite())
                || knots.windows(2).any(|w| matches!(w,[a,b] if a>b))
                || controls.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() || !p.w.is_finite() || p.w <= 0.0)
            {
                return Err(unsupported("invalid patch partition curve"));
            }
            let spline = mt::BSplineCurve::try_new(knots, controls).map_err(|e| KernelError::Failed(format!("partition spline: {e}")))?;
            for (span, bezier) in spline.bezier_decomposition().into_iter().enumerate() {
                if bezier.control_points().len() != degree + 1 || arcs.len() >= 4096 {
                    return Err(unsupported("patch partition span budget"));
                }
                let mut projected = Vec::new();
                for p in bezier.control_points() {
                    if !p.w.is_finite() || p.w <= 0.0 {
                        return Err(unsupported("invalid patch partition weight"));
                    }
                    let q = Vec3::new(p.x, p.y, p.z);
                    if !q.is_finite() {
                        return Err(unsupported("invalid patch partition point"));
                    }
                    let h = H::new(q.dot(x), q.dot(y), p.w);
                    let (u, v) = point(h);
                    if !u.is_finite() || !v.is_finite() {
                        return Err(unsupported("invalid projected patch point"));
                    }
                    projected.push(h);
                }
                simple(projected.clone(), tol, &mut budget)?;
                arcs.push((edge.clone(), span, projected));
            }
        }
        polygons.push(polygon);
    }
    for (i, (a, ai, ac)) in arcs.iter().enumerate() {
        for (b, bi, bc) in arcs.iter().skip(i + 1) {
            let mut shared = Vec::new();
            if a.id() == b.id() {
                // Adjacent spans share a virtual knot vertex. Nonadjacent spans
                // of the same edge must still be checked for self-intersection.
                if ai + 1 == *bi {
                    let p = point(*ac.last().ok_or_else(|| unsupported("empty partition span"))?);
                    let q = point(*bc.first().ok_or_else(|| unsupported("empty partition span"))?);
                    if (p.0 - q.0).hypot(p.1 - q.1) > tol * 0.1 {
                        return Err(unsupported("discontinuous patch partition boundary"));
                    }
                    shared.push(p);
                }
            } else {
                for v in [a.front(), a.back()] {
                    if v == b.front() || v == b.back() {
                        let p = crate::blend::vtx(v);
                        shared.push((p.dot(x), p.dot(y)));
                    }
                }
            }
            separate(ac.clone(), bc.clone(), &shared, tol, &mut budget)?;
        }
    }
    let mut winding = 0.0_f64;
    for polygon in &polygons {
        let area = polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()).map(|(a, b)| a.0 * b.1 - a.1 * b.0).sum::<f64>();
        if !area.is_finite() || area.abs() <= tol * tol || winding * area < 0.0 {
            return Err(unsupported("transition patch domains overlap or collapse"));
        }
        winding = area;
    }
    let inside = |p: P, polygon: &[P]| {
        let mut inside = false;
        for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let d = dx * dx + dy * dy;
            let t = if d > 0.0 { ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / d } else { 0.0 }.clamp(0.0, 1.0);
            if (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy) <= tol {
                return false;
            }
            if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) * dx / dy {
                inside = !inside;
            }
        }
        inside
    };
    for (i, polygon) in polygons.iter().enumerate() {
        for (j, other) in polygons.iter().enumerate() {
            if i != j && polygon.iter().step_by(64).any(|p| inside(*p, other)) {
                return Err(unsupported("transition patch domains contain one another"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt::builder;
    #[test]
    fn nearly_tangent_hulls_can_be_separated_away_from_a_shared_vertex() {
        let line = vec![H::new(0.0, 0.0, 1.0), H::new(1.0, 1.0, 1.0), H::new(2.0, 2.0, 1.0)];
        let curve = vec![H::new(0.0, 0.0, 1.0), H::new(1.0, 1.0, 1.0), H::new(2.0, 2.0001, 1.0)];
        separate(line, curve, &[(0.0, 0.0)], 1e-7, &mut 0).unwrap();
    }
    fn polygon(points: &[(f64, f64)]) -> Vec<mt::Edge> {
        let vertices: Vec<_> = points.iter().map(|(x, y)| builder::vertex(p3(Vec3::new(*x, *y, 0.0)))).collect();
        vertices.iter().zip(vertices.iter().cycle().skip(1)).take(vertices.len()).map(|(a, b)| builder::line(a, b)).collect()
    }
    #[test]
    fn rejects_crossing_and_nested_graph_domains() {
        let square = polygon(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
        assert!(check(std::slice::from_ref(&square), Vec3::Z, f64::NAN).is_err());
        assert!(check(std::slice::from_ref(&square), Vec3::new(f64::INFINITY, 0.0, 0.0), 4.0).is_err());
        let crossing = polygon(&[(1.0, -1.0), (3.0, -1.0), (3.0, 1.0), (1.0, 1.0)]);
        let nested = polygon(&[(0.5, 0.5), (1.5, 0.5), (1.5, 1.5), (0.5, 1.5)]);
        assert!(check(&[square.clone(), square.clone()], Vec3::Z, 4.0).is_err());
        assert!(check(&[square.clone(), crossing], Vec3::Z, 4.0).is_err());
        assert!(check(&[square, nested], Vec3::Z, 4.0).is_err());
        assert!(check(&[polygon(&[(0.0, 0.0), (2.0, 2.0), (0.0, 2.0), (2.0, 0.0)])], Vec3::Z, 4.0).is_err());
    }
    #[test]
    fn spline_spans_are_checked_against_each_other_and_the_other_edges() {
        let with_spline = |points: &[(f64, f64)]| {
            let mut wire = polygon(&[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]);
            let mut knots = vec![0.0, 0.0];
            knots.extend((1..points.len() - 1).map(|i| i as f64 / (points.len() - 1) as f64));
            knots.extend([1.0, 1.0]);
            let curve = mt::Curve::BSplineCurve(mt::BSplineCurve::new(
                mt::KnotVec::from(knots),
                points.iter().map(|(x, y)| p3(Vec3::new(*x, *y, 0.0))).collect(),
            ));
            wire[0] = mt::Edge::try_new(wire[0].front(), wire[0].back(), curve).unwrap();
            wire
        };
        check(&[with_spline(&[(0.0, 0.0), (0.5, 0.0), (1.5, 0.0), (2.0, 0.0)])], Vec3::Z, 4.0).unwrap();
        assert!(check(&[with_spline(&[(0.0, 0.0), (1.0, 3.0), (1.0, -1.0), (2.0, 0.0)])], Vec3::Z, 4.0).is_err());
        assert!(check(&[with_spline(&[(0.0, 0.0), (1.0, 1.0), (0.0, 1.0), (1.0, 0.0), (2.0, 0.0)])], Vec3::Z, 4.0).is_err());
    }

    #[test]
    fn adjacent_domains_share_only_their_common_edge() {
        let left = polygon(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
        let middle = left[1].clone();
        let a = builder::vertex(p3(Vec3::new(2.0, 0.0, 0.0)));
        let b = builder::vertex(p3(Vec3::new(2.0, 1.0, 0.0)));
        let right = vec![builder::line(middle.front(), &a), builder::line(&a, &b), builder::line(&b, middle.back()), middle.inverse()];
        check(&[left, right], Vec3::Z, 4.0).unwrap();
    }
}
