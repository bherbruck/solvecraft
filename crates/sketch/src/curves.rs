//! Free-form sketch curves: ellipses, splines (through fit points, or a clamped B-spline over
//! control points) and conics (rational quadratic Béziers). Evaluation works on positions so
//! the solver can use it with trial values.

use std::f64::consts::TAU;

use solvecraft_geom::Vec2;

/// Polyline samples per spline span / conic / ellipse quarter.
const SPAN_SAMPLES: usize = 24;
/// Most points a spline may have.
pub const MAX_SPLINE_POINTS: usize = 500;

/// Point on an ellipse with centre `c`, major-axis end `m` and minor radius `r` at angle `t`.
pub fn ellipse_point(c: Vec2, m: Vec2, r: f64, t: f64) -> Vec2 {
    let u = m - c;
    let a = u.len().max(1e-12);
    let v = u.perp() / a * r;
    c + u * t.cos() + v * t.sin()
}

pub fn ellipse_polyline(c: Vec2, m: Vec2, r: f64) -> Vec<Vec2> {
    let n = SPAN_SAMPLES * 4;
    (0..=n).map(|i| ellipse_point(c, m, r, TAU * i as f64 / n as f64)).collect()
}

/// Rational quadratic Bézier from `a` to `b` with control `apex` and shape `rho` in (0, 1)
/// (0.5 = parabola, below = ellipse, above = hyperbola).
pub fn conic_point(a: Vec2, apex: Vec2, b: Vec2, rho: f64, t: f64) -> Vec2 {
    let w = rho / (1.0 - rho).max(1e-9);
    let (b0, b1, b2) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t) * w, t * t);
    let den = b0 + b1 + b2;
    (a * b0 + apex * b1 + b * b2) / den.max(1e-12)
}

pub fn conic_polyline(a: Vec2, apex: Vec2, b: Vec2, rho: f64) -> Vec<Vec2> {
    let n = SPAN_SAMPLES * 2;
    (0..=n).map(|i| conic_point(a, apex, b, rho, i as f64 / n as f64)).collect()
}

/// Natural cubic interpolation through `p` (chord-length parameters): per span the cubic
/// Hermite data (start, end, start tangent, end tangent) in span-parameter units.
fn fit_spans(p: &[Vec2]) -> Vec<(Vec2, Vec2, Vec2, Vec2)> {
    let n = p.len();
    if n < 2 {
        return Vec::new();
    }
    let h: Vec<f64> = p.windows(2).map(|w| w[0].dist(w[1]).max(1e-9)).collect();
    if n == 2 {
        let d = p[1] - p[0];
        return vec![(p[0], p[1], d, d)];
    }
    // Second derivatives M (natural ends: M0 = Mn = 0) by the tridiagonal (Thomas) solve.
    let m = n - 2;
    let mut diag = vec![0.0; m];
    let mut up = vec![0.0; m];
    let mut low = vec![0.0; m];
    let mut rhs = vec![Vec2::ZERO; m];
    for i in 0..m {
        let (h0, h1) = (h[i], h[i + 1]);
        diag[i] = 2.0 * (h0 + h1);
        low[i] = h0;
        up[i] = h1;
        rhs[i] = ((p[i + 2] - p[i + 1]) / h1 - (p[i + 1] - p[i]) / h0) * 6.0;
    }
    for i in 1..m {
        let w = low[i] / diag[i - 1];
        diag[i] -= w * up[i - 1];
        rhs[i] = rhs[i] - rhs[i - 1] * w;
    }
    let mut sol = vec![Vec2::ZERO; m];
    for i in (0..m).rev() {
        let next = if i + 1 < m { sol[i + 1] * up[i] } else { Vec2::ZERO };
        sol[i] = (rhs[i] - next) / diag[i];
    }
    let mm = |i: usize| if i == 0 || i == n - 1 { Vec2::ZERO } else { sol[i - 1] };
    (0..n - 1)
        .map(|i| {
            let hi = h[i];
            let slope = (p[i + 1] - p[i]) / hi;
            // First derivatives (per unit chord parameter) at both ends of the span.
            let d0 = slope - (mm(i) * 2.0 + mm(i + 1)) * (hi / 6.0);
            let d1 = slope + (mm(i) + mm(i + 1) * 2.0) * (hi / 6.0);
            (p[i], p[i + 1], d0 * hi, d1 * hi)
        })
        .collect()
}

fn hermite(p0: Vec2, p1: Vec2, t0: Vec2, t1: Vec2, s: f64) -> Vec2 {
    let (s2, s3) = (s * s, s * s * s);
    p0 * (2.0 * s3 - 3.0 * s2 + 1.0) + t0 * (s3 - 2.0 * s2 + s) + p1 * (-2.0 * s3 + 3.0 * s2) + t1 * (s3 - s2)
}

/// Clamped uniform B-spline of `degree` over control points `p`, sampled.
fn bspline_polyline(p: &[Vec2], degree: usize) -> Vec<Vec2> {
    let n = p.len();
    let k = degree.min(n.saturating_sub(1)).max(1);
    // Knots: k+1 zeros, uniform interior, k+1 ones.
    let spans = n - k;
    let mut knots = vec![0.0; k + 1];
    for i in 1..spans {
        knots.push(i as f64 / spans as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, k + 1));
    let total = SPAN_SAMPLES * spans;
    (0..=total)
        .map(|i| {
            let t = i as f64 / total as f64;
            // De Boor.
            let mut s = k;
            while s + 1 < n && knots.get(s + 1).is_some_and(|x| *x <= t) {
                s += 1;
            }
            let mut d: Vec<Vec2> = (0..=k).map(|j| p.get(j + s - k).copied().unwrap_or_default()).collect();
            for r in 1..=k {
                for j in (r..=k).rev() {
                    let i0 = j + s - k;
                    let (lo, hi) = (knots.get(i0).copied().unwrap_or(0.0), knots.get(i0 + k + 1 - r).copied().unwrap_or(1.0));
                    let a = if hi - lo > 1e-15 { (t - lo) / (hi - lo) } else { 0.0 };
                    d[j] = d[j - 1] * (1.0 - a) + d[j] * a;
                }
            }
            d[k]
        })
        .collect()
}

/// Polyline of a spline through (`control == false`) or over (`control`) the points.
pub fn spline_polyline(p: &[Vec2], control: bool, degree: u8) -> Vec<Vec2> {
    if p.len() < 2 {
        return p.to_vec();
    }
    if control {
        return bspline_polyline(p, degree.clamp(1, 7) as usize);
    }
    let mut out = Vec::new();
    for (p0, p1, t0, t1) in fit_spans(p) {
        for i in 0..SPAN_SAMPLES {
            out.push(hermite(p0, p1, t0, t1, i as f64 / SPAN_SAMPLES as f64));
        }
    }
    if let Some(l) = p.last() {
        out.push(*l);
    }
    out
}

/// Unit tangent of a spline at its start (`at_end == false`) or end, pointing along the curve.
pub fn spline_end_tangent(p: &[Vec2], control: bool, at_end: bool) -> Option<Vec2> {
    let n = p.len();
    if n < 2 {
        return None;
    }
    if control {
        return if at_end { (p[n - 1] - p[n - 2]).normalized() } else { (p[1] - p[0]).normalized() };
    }
    let spans = fit_spans(p);
    if at_end { spans.last().and_then(|s| s.3.normalized()) } else { spans.first().and_then(|s| s.2.normalized()) }
}

/// Distance from `q` to a polyline.
pub fn polyline_dist(poly: &[Vec2], q: Vec2) -> f64 {
    let mut best = f64::INFINITY;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = b - a;
        let l2 = d.len2();
        let t = if l2 > 0.0 { ((q - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min(q.dist(a + d * t));
    }
    if poly.len() == 1 {
        best = poly[0].dist(q);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_spline_passes_through_its_points_and_is_smooth() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(20.0, 0.0), Vec2::new(30.0, 8.0)];
        let poly = spline_polyline(&p, false, 3);
        for q in p {
            assert!(poly.iter().any(|x| x.dist(q) < 1e-9));
        }
        // A straight row of points stays straight.
        let line = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 1.0), Vec2::new(3.0, 3.0)];
        assert!(spline_polyline(&line, false, 3).iter().all(|q| (q.x - q.y).abs() < 1e-9));
        let t = spline_end_tangent(&line, false, false).unwrap();
        assert!((t.x - t.y).abs() < 1e-9);
    }

    #[test]
    fn control_spline_is_clamped_and_conic_hits_its_ends() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(0.0, 10.0), Vec2::new(10.0, 10.0), Vec2::new(10.0, 0.0)];
        let poly = spline_polyline(&p, true, 3);
        assert!(poly.first().unwrap().dist(p[0]) < 1e-12 && poly.last().unwrap().dist(p[3]) < 1e-12);
        // Cubic Bézier midpoint.
        let mid = poly[poly.len() / 2];
        assert!(mid.dist(Vec2::new(5.0, 7.5)) < 1e-9, "{mid:?}");
        let c = conic_polyline(
            Vec2::new(10.0, 0.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(0.0, 10.0),
            std::f64::consts::FRAC_1_SQRT_2 / (1.0 + std::f64::consts::FRAC_1_SQRT_2),
        );
        // w = cos(45°): a quarter circle of radius 10 about the origin.
        assert!(c.iter().all(|q| (q.len() - 10.0).abs() < 1e-9));
        let e = ellipse_polyline(Vec2::ZERO, Vec2::new(5.0, 0.0), 2.0);
        assert!(e.iter().all(|q| ((q.x / 5.0).powi(2) + (q.y / 2.0).powi(2) - 1.0).abs() < 1e-9));
    }
}
