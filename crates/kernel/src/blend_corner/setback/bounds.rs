//! Containment of a trimmed graph patch, using positive-weight curve hulls and
//! spline control hulls. Surface samples alone can miss a narrow interior spike.

use super::*;

type Uv = (f64, f64);

pub(super) struct Trim {
    segments: Vec<(Uv, Uv)>,
    tolerance: f64,
}

impl Trim {
    pub(super) fn new(boundaries: &[Boundary], origin: Vec3, x: Vec3, y: Vec3, du: f64, dv: f64, tolerance: f64) -> Result<Self> {
        let tolerance = tolerance / (du + dv) * 0.125;
        let project = |p: mt::Vector4| -> Result<Uv> {
            if !p.w.is_finite() || p.w <= 0.0 {
                return Err(unsupported("invalid setback trim weight"));
            }
            let p = Vec3::new(p.x / p.w, p.y / p.w, p.z / p.w) - origin;
            if !p.is_finite() {
                return Err(unsupported("invalid setback trim point"));
            }
            Ok((p.dot(x) / du, p.dot(y) / dv))
        };
        let distance = |p: Uv, a: Uv, b: Uv| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let d = dx * dx + dy * dy;
            let t = if d > 0.0 { ((p.0 - a.0) * dx + (p.1 - a.1) * dy) / d } else { 0.0 };
            let t = t.clamp(0.0, 1.0);
            (p.0 - a.0 - dx * t).hypot(p.1 - a.1 - dy * t)
        };
        let mut segments = Vec::new();
        for boundary in boundaries {
            let controls = match &boundary.curve {
                mt::Curve::NurbsCurve(curve) if curve.control_points().len() == curve.degree() + 1 => curve.control_points().clone(),
                mt::Curve::BSplineCurve(curve) if curve.control_points().len() == curve.degree() + 1 => {
                    curve.control_points().iter().map(|p| mt::Vector4::new(p.x, p.y, p.z, 1.0)).collect()
                }
                mt::Curve::Line(line) => {
                    vec![mt::Vector4::new(line.0.x, line.0.y, line.0.z, 1.0), mt::Vector4::new(line.1.x, line.1.y, line.1.z, 1.0)]
                }
                _ => return Err(unsupported("non-Bezier patch trim")),
            };
            let mut stack = vec![(controls, 0)];
            while let Some((controls, depth)) = stack.pop() {
                if segments.len() + stack.len() > 32768 || depth > 24 {
                    return Err(unsupported("setback trim exceeds its budget"));
                }
                let first = controls.first().copied().ok_or_else(|| unsupported("empty setback trim"))?;
                let last = controls.last().copied().ok_or_else(|| unsupported("empty setback trim"))?;
                let (a, b) = (project(first)?, project(last)?);
                let flat =
                    controls.iter().map(|p| project(*p).map(|p| distance(p, a, b))).collect::<Result<Vec<_>>>()?.into_iter().all(|d| d <= tolerance);
                if flat {
                    segments.push((a, b));
                    continue;
                }
                let mut row = controls;
                let mut left = vec![first];
                let mut right = vec![last];
                while row.len() > 1 {
                    row = row
                        .windows(2)
                        .filter_map(|w| match w {
                            [a, b] => Some((*a + *b) * 0.5),
                            _ => None,
                        })
                        .collect();
                    left.push(row.first().copied().ok_or_else(|| unsupported("setback trim split"))?);
                    right.push(row.last().copied().ok_or_else(|| unsupported("setback trim split"))?);
                }
                right.reverse();
                stack.push((right, depth + 1));
                stack.push((left, depth + 1));
            }
        }
        Ok(Self { segments, tolerance })
    }

    fn inside(&self, u: f64, v: f64) -> bool {
        let mut inside = false;
        for ((a, b), (c, d)) in &self.segments {
            if (b > &v) != (d > &v) && u < (c - a) * (v - b) / (d - b) + a {
                inside = !inside;
            }
        }
        inside
    }

    /// A cell is outside only when the curve hull tubes cannot enter it and its
    /// centre is outside the closed polyline. Ambiguous cells are subdivided.
    fn outside(&self, rect: [f64; 4]) -> bool {
        let [u0, u1, v0, v1] = rect;
        let t = self.tolerance;
        if self.segments.iter().any(|(a, b)| {
            let (mut enter, mut leave) = (0.0_f64, 1.0_f64);
            for (p, d, lo, hi) in [(a.0, b.0 - a.0, u0 - t, u1 + t), (a.1, b.1 - a.1, v0 - t, v1 + t)] {
                if d.abs() < 1e-20 {
                    if p < lo || p > hi {
                        return false;
                    }
                } else {
                    let (c, e) = ((lo - p) / d, (hi - p) / d);
                    enter = enter.max(c.min(e));
                    leave = leave.min(c.max(e));
                    if enter > leave {
                        return false;
                    }
                }
            }
            true
        }) {
            return false;
        }
        !self.inside((u0 + u1) * 0.5, (v0 + v1) * 0.5)
    }

    pub(super) fn violations(
        &self,
        surface: &mt::BSplineSurface<mt::Point3>,
        planes: &[(Vec3, Vec3)],
        tolerance: f64,
        refinement_margin: f64,
    ) -> Result<Vec<Uv>> {
        let mut violations = Vec::new();
        let mut stack = vec![(surface.clone(), [0.0, 1.0, 0.0, 1.0], 0)];
        let mut visited = 0;
        while let Some((mut patch, rect, depth)) = stack.pop() {
            visited += 1;
            if visited > 65536 {
                if !violations.is_empty() {
                    return Ok(violations);
                }
                return Err(unsupported("cannot certify setback containment within its budget"));
            }
            if self.outside(rect) {
                continue;
            }
            if patch.control_points().iter().flatten().all(|p| {
                let p = from_p3(*p);
                p.is_finite() && planes.iter().all(|(n, q)| (p - *q).dot(*n) <= tolerance)
            }) {
                continue;
            }
            let [u0, u1, v0, v1] = rect;
            let (u, v) = ((u0 + u1) * 0.5, (v0 + v1) * 0.5);
            if self.inside(u, v) {
                let p = from_p3(patch.subs(u, v));
                // Leave a margin when refining an uncertain hull. Waiting for
                // a sample to exceed the full tolerance can chase a level set
                // indefinitely even though the hull has not yet been certified.
                if !p.is_finite() || planes.iter().any(|(n, q)| (p - *q).dot(*n) > tolerance * refinement_margin) {
                    violations.push((u, v));
                    if violations.len() >= 128 {
                        return Ok(violations);
                    }
                    continue;
                }
            }
            if depth > 36 {
                if !violations.is_empty() {
                    return Ok(violations);
                }
                return Err(unsupported("cannot certify setback containment at this resolution"));
            }
            if u1 - u0 >= v1 - v0 {
                let right = patch.ucut(u);
                let cut = patch.uknot_vec().last().copied().ok_or_else(|| unsupported("setback containment split"))?;
                if !(cut > u0 && cut < u1) {
                    return Err(unsupported("setback containment split is too small"));
                }
                stack.push((right, [cut, u1, v0, v1], depth + 1));
                stack.push((patch, [u0, cut, v0, v1], depth + 1));
            } else {
                let upper = patch.vcut(v);
                let cut = patch.vknot_vec().last().copied().ok_or_else(|| unsupported("setback containment split"))?;
                if !(cut > v0 && cut < v1) {
                    return Err(unsupported("setback containment split is too small"));
                }
                stack.push((upper, [u0, u1, cut, v1], depth + 1));
                stack.push((patch, [u0, u1, v0, cut], depth + 1));
            }
        }
        Ok(violations)
    }

    #[cfg(test)]
    fn contains(&self, surface: &mt::BSplineSurface<mt::Point3>, planes: &[(Vec3, Vec3)], tolerance: f64) -> Result<()> {
        if self.violations(surface, planes, tolerance, 0.5)?.is_empty() {
            Ok(())
        } else {
            Err(unsupported("the setback surface leaves the original material"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(a: f64, b: f64) -> Trim {
        let points = [(a, a), (b, a), (b, b), (a, b)];
        Trim { segments: (0..4).map(|i| (points[i], points[(i + 1) % 4])).collect(), tolerance: 1e-8 }
    }

    #[test]
    fn control_hulls_catch_a_spike_between_boundary_samples() {
        let controls = (0..4)
            .map(|i| {
                (0..4).map(|j| p3(Vec3::new(i as f64 / 3.0, j as f64 / 3.0, if i > 0 && i < 3 && j > 0 && j < 3 { 1.0 } else { 0.0 }))).collect()
            })
            .collect();
        let surface = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(3), mt::KnotVec::bezier_knot(3)), controls);
        let trim = square(0.0, 1.0);
        assert!(trim.contains(&surface, &[(Vec3::Z, Vec3::ZERO)], 1e-6).is_err());
        assert!(trim.contains(&surface, &[(-Vec3::Z, Vec3::ZERO)], 1e-6).is_ok());
    }

    #[test]
    fn ignores_geometry_outside_the_trimmed_domain() {
        // z = (u - 1/2)^2 - .02 is negative on the trimmed square, but
        // positive at both ends of the untrimmed rectangular surface.
        let height = [0.23, 0.23 - 1.0 / 3.0, 0.23 - 1.0 / 3.0, 0.23];
        let controls =
            height.into_iter().enumerate().map(|(i, z)| (0..4).map(|j| p3(Vec3::new(i as f64 / 3.0, j as f64 / 3.0, z))).collect()).collect();
        let surface = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(3), mt::KnotVec::bezier_knot(3)), controls);
        assert!(square(0.4, 0.6).contains(&surface, &[(Vec3::Z, Vec3::ZERO)], 1e-6).is_ok());
        assert!(square(0.0, 1.0).contains(&surface, &[(Vec3::Z, Vec3::ZERO)], 1e-6).is_err());
    }

    #[test]
    fn diagonal_trim_uses_a_curve_tube_not_its_bounding_rectangle() {
        let triangle = Trim { segments: vec![((0.1, 0.1), (0.8, 0.1)), ((0.8, 0.1), (0.1, 0.8)), ((0.1, 0.8), (0.1, 0.1))], tolerance: 1e-8 };
        let controls = (0..=1).map(|i| (0..=1).map(|j| p3(Vec3::new(i as f64, j as f64, (i + j) as f64 - 1.0))).collect()).collect();
        let surface = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(1), mt::KnotVec::bezier_knot(1)), controls);
        assert!(triangle.contains(&surface, &[(Vec3::Z, Vec3::ZERO)], 1e-6).is_ok());
    }
}
