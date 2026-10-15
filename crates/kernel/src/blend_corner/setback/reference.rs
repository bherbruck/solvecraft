//! A bounded reference height made from convex combinations of boundary points.
//! The final surface still follows the exact boundary positions and tangent planes.
use super::*;

pub(super) struct Reference {
    center: Vec3,
    x: Vec3,
    y: Vec3,
    ring: Vec<(f64, Vec3)>,
}

impl Reference {
    pub(super) fn new(samples: &[(Vec3, Vec3)], x: Vec3, y: Vec3) -> Result<Self> {
        let mut points: Vec<_> = samples.iter().map(|(p, _)| *p).collect();
        points.sort_by(|a, b| a.dot(x).total_cmp(&b.dot(x)).then(a.dot(y).total_cmp(&b.dot(y))));
        points.dedup_by(|a, b| (a.dot(x) - b.dot(x)).abs() < 1e-10 && (a.dot(y) - b.dot(y)).abs() < 1e-10);
        let cross = |a: Vec3, b: Vec3, c: Vec3| {
            let u = b - a;
            let v = c - a;
            u.dot(x) * v.dot(y) - u.dot(y) * v.dot(x)
        };
        let mut lower = Vec::new();
        for p in &points {
            while lower.len() >= 2 {
                let (Some(a), Some(b)) = (lower.get(lower.len() - 2), lower.last()) else { break };
                if cross(*a, *b, *p) > 0.0 {
                    break;
                }
                lower.pop();
            }
            lower.push(*p);
        }
        let mut upper = Vec::new();
        for p in points.iter().rev() {
            while upper.len() >= 2 {
                let (Some(a), Some(b)) = (upper.get(upper.len() - 2), upper.last()) else { break };
                if cross(*a, *b, *p) > 0.0 {
                    break;
                }
                upper.pop();
            }
            upper.push(*p);
        }
        lower.pop();
        upper.pop();
        lower.extend(upper);
        if lower.len() < 3 {
            return Err(unsupported("degenerate transition reference"));
        }
        let center = lower.iter().fold(Vec3::ZERO, |s, p| s + *p) / lower.len() as f64;
        let mut ring: Vec<_> = lower
            .into_iter()
            .map(|p| {
                let d = p - center;
                (d.dot(y).atan2(d.dot(x)), p)
            })
            .collect();
        ring.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (a, p) = ring.first().copied().ok_or_else(|| unsupported("empty transition reference"))?;
        ring.push((a + std::f64::consts::TAU, p));
        Ok(Self { center, x, y, ring })
    }

    pub(super) fn height(&self, base: Vec3, z: Vec3) -> Option<f64> {
        let q = base - self.center;
        let mut angle = q.dot(self.y).atan2(q.dot(self.x));
        if angle < self.ring.first()?.0 {
            angle += std::f64::consts::TAU;
        }
        let i = self.ring.partition_point(|(a, _)| *a <= angle).clamp(1, self.ring.len() - 1);
        let a = self.ring.get(i - 1)?.1 - self.center;
        let b = self.ring.get(i)?.1 - self.center;
        let cross = |a: Vec3, b: Vec3| a.dot(self.x) * b.dot(self.y) - a.dot(self.y) * b.dot(self.x);
        let den = cross(a, b);
        if den.abs() < 1e-20 {
            return None;
        }
        let u = cross(q, b) / den;
        let v = cross(a, q) / den;
        // Continue the same affine triangle outside the convex hull. Returning a
        // constant there introduces a jump in the guide at the trimming boundary,
        // making the unused part of the tensor-product surface needlessly steep.
        // The caller clamps this continuation to the supporting halfspaces; only
        // the trimmed domain is accepted by the separate control-hull certificate.
        Some((self.center + a * u + b * v - base).dot(z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_continues_an_affine_boundary_outside_the_trim() {
        let points: Vec<_> =
            [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].into_iter().map(|(x, y)| (Vec3::new(x, y, 2.0 + x - y), Vec3::ZERO)).collect();
        let reference = Reference::new(&points, Vec3::X, Vec3::Y).unwrap();
        for x in [-0.2, 0.0, 0.5, 1.0, 1.2] {
            for y in [-0.2, 0.0, 0.5, 1.0, 1.2] {
                let height = reference.height(Vec3::new(x, y, 0.0), Vec3::Z).unwrap();
                assert!((height - (2.0 + x - y)).abs() < 1e-12);
            }
        }
    }
}
