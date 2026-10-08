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
    // Samples by how far the curve bows from its chord (small text curves need few).
    let chord = b - a;
    let bow = chord.cross(apex - a).abs() / chord.len().max(1e-12);
    let n = ((bow / 2e-3).sqrt().ceil() as usize).clamp(2, SPAN_SAMPLES * 2);
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

/// Clamped uniform B-spline of `degree` over control points `p` at `t` in [0, 1] (de Boor).
fn bspline_at(p: &[Vec2], degree: usize, t: f64) -> Vec2 {
    let n = p.len();
    let k = degree.min(n.saturating_sub(1)).max(1);
    let spans = n - k;
    let mut knots = vec![0.0; k + 1];
    for i in 1..spans {
        knots.push(i as f64 / spans as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, k + 1));
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
}

/// Clamped uniform B-spline of `degree` over control points `p`, sampled.
fn bspline_polyline(p: &[Vec2], degree: usize) -> Vec<Vec2> {
    let n = p.len();
    let k = degree.min(n.saturating_sub(1)).max(1);
    let total = SPAN_SAMPLES * (n - k);
    (0..=total).map(|i| bspline_at(p, degree, i as f64 / total as f64)).collect()
}

/// Point of a spline at global parameter `t` in [0, 1].
pub fn spline_point(p: &[Vec2], control: bool, degree: u8, t: f64) -> Vec2 {
    let t = t.clamp(0.0, 1.0);
    if p.len() < 2 {
        return p.first().copied().unwrap_or_default();
    }
    if control {
        return bspline_at(p, degree.clamp(1, 7) as usize, t);
    }
    let spans = fit_spans(p);
    let n = spans.len().max(1);
    let x = t * n as f64;
    let i = (x.floor() as usize).min(n - 1);
    match spans.get(i) {
        Some((p0, p1, t0, t1)) => hermite(*p0, *p1, *t0, *t1, x - i as f64),
        None => p[0],
    }
}

/// Curvature vector (towards the centre of curvature, magnitude 1/radius) of a parametric
/// curve at an end (`at_end`), by one-sided second-order differences.
pub fn end_curvature(f: &dyn Fn(f64) -> Vec2, at_end: bool) -> Vec2 {
    let h = 1e-3;
    let (t0, sgn) = if at_end { (1.0, -1.0) } else { (0.0, 1.0) };
    let (f0, f1, f2, f3) = (f(t0), f(t0 + sgn * h), f(t0 + sgn * 2.0 * h), f(t0 + sgn * 3.0 * h));
    let d1 = (f0 * -3.0 + f1 * 4.0 - f2) / (2.0 * h) * sgn;
    let d2 = (f0 * 2.0 - f1 * 5.0 + f2 * 4.0 - f3) / (h * h);
    let l2 = d1.len2();
    if l2 < 1e-24 {
        return Vec2::ZERO;
    }
    let t = d1 / l2.sqrt();
    (d2 - t * t.dot(d2)) / l2
}

/// The cubic Bézier pieces of a fit spline (exact: each span is a cubic Hermite).
pub fn fit_beziers(p: &[Vec2]) -> Vec<[Vec2; 4]> {
    fit_spans(p).into_iter().map(|(p0, p1, t0, t1)| [p0, p0 + t0 / 3.0, p1 - t1 / 3.0, p1]).collect()
}

/// The Bézier pieces (each `degree + 1` points) of the clamped uniform B-spline over `ctrl`
/// (Boehm knot insertion until every interior knot has multiplicity `degree`).
pub fn bspline_beziers(ctrl: &[Vec2], degree: usize) -> Vec<Vec<Vec2>> {
    let n = ctrl.len();
    let k = degree.min(n.saturating_sub(1)).max(1);
    let spans = n - k;
    let mut knots = vec![0.0; k + 1];
    for i in 1..spans {
        knots.push(i as f64 / spans as f64);
    }
    knots.extend(std::iter::repeat_n(1.0, k + 1));
    let mut pts = ctrl.to_vec();
    for s in 1..spans {
        let u = s as f64 / spans as f64;
        for _ in 1..k {
            // Span holding u (the last knot ≤ u).
            let Some(span) = knots.iter().rposition(|x| *x <= u + 1e-15) else { break };
            let mut q = Vec::with_capacity(pts.len() + 1);
            for i in 0..=pts.len() {
                let p = if i + k <= span {
                    pts.get(i).copied()
                } else if i > span {
                    pts.get(i - 1).copied()
                } else {
                    let (lo, hi) = (knots.get(i).copied().unwrap_or(0.0), knots.get(i + k).copied().unwrap_or(1.0));
                    let a = if hi - lo > 1e-15 { (u - lo) / (hi - lo) } else { 0.0 };
                    match (pts.get(i.wrapping_sub(1)), pts.get(i)) {
                        (Some(x), Some(y)) => Some(*x * (1.0 - a) + *y * a),
                        _ => None,
                    }
                };
                if let Some(p) = p {
                    q.push(p);
                }
            }
            knots.insert(span + 1, u);
            pts = q;
        }
    }
    pts.windows(k + 1).step_by(k).map(|w| w.to_vec()).collect()
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

/// Signed distance from `q` to a polyline (positive on the left of the nearest segment).
pub fn polyline_signed_dist(poly: &[Vec2], q: Vec2) -> f64 {
    let mut best = (f64::INFINITY, 1.0);
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let d = b - a;
        let l2 = d.len2();
        let t = if l2 > 0.0 { ((q - a).dot(d) / l2).clamp(0.0, 1.0) } else { 0.0 };
        let dist = q.dist(a + d * t);
        if dist < best.0 {
            best = (dist, if d.cross(q - a) >= 0.0 { 1.0 } else { -1.0 });
        }
    }
    best.0 * best.1
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
    fn end_curvature_of_a_circle_quarter() {
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let rho = w / (1.0 + w);
        let (a, x, b) = (Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0));
        let k = end_curvature(&|t| conic_point(a, x, b, rho, t), false);
        assert!(k.dist(Vec2::new(-0.1, 0.0)) < 1e-5, "{k:?}");
        let k1 = end_curvature(&|t| conic_point(a, x, b, rho, t), true);
        assert!(k1.dist(Vec2::new(0.0, -0.1)) < 1e-5, "{k1:?}");
        // Fit splines have natural (straight) ends.
        let p = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(20.0, 0.0)];
        assert!(end_curvature(&|t| spline_point(&p, false, 3, t), false).len() < 1e-3);
    }

    #[test]
    fn spline_bezier_pieces_match_the_spline() {
        let p = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 5.0), Vec2::new(20.0, -3.0), Vec2::new(30.0, 8.0), Vec2::new(35.0, 0.0)];
        let bez = |c: &[Vec2], t: f64| -> Vec2 {
            // de Casteljau.
            let mut v = c.to_vec();
            while v.len() > 1 {
                v = v.windows(2).map(|w| w[0] * (1.0 - t) + w[1] * t).collect();
            }
            v[0]
        };
        for degree in [2usize, 3] {
            let pieces = bspline_beziers(&p, degree);
            assert_eq!(pieces.len(), p.len() - degree, "degree {degree}");
            let spans = pieces.len();
            for (i, c) in pieces.iter().enumerate() {
                for t in [0.0, 0.3, 0.7, 1.0] {
                    let g = (i as f64 + t) / spans as f64;
                    let want = spline_point(&p, true, degree as u8, g);
                    assert!(bez(c, t).dist(want) < 1e-9, "degree {degree} piece {i} t {t}: {:?} vs {want:?}", bez(c, t));
                }
            }
        }
        for (i, c) in fit_beziers(&p).iter().enumerate() {
            let g = (i as f64 + 0.4) / 4.0;
            assert!(bez(c, 0.4).dist(spline_point(&p, false, 3, g)) < 1e-9);
        }
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
