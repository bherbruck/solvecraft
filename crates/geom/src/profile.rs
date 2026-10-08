//! Planar profiles: closed loops of lines, circular arcs, cubic Béziers and conics (rational
//! quadratic Béziers), and regions (an outer loop minus holes). Sketches produce them; the
//! kernel turns them into faces.

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::Vec2;

/// One boundary segment in sketch coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Seg2 {
    Line {
        a: Vec2,
        b: Vec2,
    },
    /// Arc from `start` angle sweeping `sweep` radians (positive = counter-clockwise).
    Arc {
        center: Vec2,
        radius: f64,
        start: f64,
        sweep: f64,
    },
    /// Cubic Bézier.
    Cubic {
        p0: Vec2,
        p1: Vec2,
        p2: Vec2,
        p3: Vec2,
    },
    /// Rational quadratic Bézier from `a` to `b` with control `apex` of weight `w` (end
    /// weights 1): exact conic arcs (ellipses, parabolas, hyperbolas).
    Conic {
        a: Vec2,
        apex: Vec2,
        b: Vec2,
        w: f64,
    },
}

/// Gauss–Legendre nodes and weights on [0, 1] (5 points).
const GL5: [(f64, f64); 5] = [
    (0.046_910_077_030_668, 0.118_463_442_528_095),
    (0.230_765_344_947_158, 0.239_314_335_249_683),
    (0.5, 0.284_444_444_444_444),
    (0.769_234_655_052_842, 0.239_314_335_249_683),
    (0.953_089_922_969_332, 0.118_463_442_528_095),
];

impl Seg2 {
    /// Derivative with respect to the parameter at `t` (Béziers and conics; lines and arcs too).
    pub fn derivative(&self, t: f64) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => b - a,
            Seg2::Arc { radius, start, sweep, .. } => Vec2::from_angle(start + sweep * t).perp() * (radius * sweep),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let u = 1.0 - t;
                (p1 - p0) * (3.0 * u * u) + (p2 - p1) * (6.0 * u * t) + (p3 - p2) * (3.0 * t * t)
            }
            Seg2::Conic { a, apex, b, w } => {
                let u = 1.0 - t;
                let (n, d) = (a * (u * u) + apex * (2.0 * w * u * t) + b * (t * t), u * u + 2.0 * w * u * t + t * t);
                let (dn, dd) = (a * (-2.0 * u) + apex * (2.0 * w * (1.0 - 2.0 * t)) + b * (2.0 * t), -2.0 * u + 2.0 * w * (1.0 - 2.0 * t) + 2.0 * t);
                (dn * d - n * dd) / (d * d).max(1e-300)
            }
        }
    }
    pub fn start(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, .. } | Seg2::Conic { a, .. } => a,
            Seg2::Cubic { p0, .. } => p0,
            Seg2::Arc { center, radius, start, .. } => center + Vec2::from_angle(start) * radius,
        }
    }
    pub fn end(&self) -> Vec2 {
        match *self {
            Seg2::Line { b, .. } | Seg2::Conic { b, .. } => b,
            Seg2::Cubic { p3, .. } => p3,
            Seg2::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep) * radius,
        }
    }
    pub fn point_at(&self, t: f64) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => a.lerp(b, t),
            Seg2::Arc { center, radius, start, sweep } => center + Vec2::from_angle(start + sweep * t) * radius,
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let u = 1.0 - t;
                p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
            }
            Seg2::Conic { a, apex, b, w } => {
                let u = 1.0 - t;
                let d = u * u + 2.0 * w * u * t + t * t;
                (a * (u * u) + apex * (2.0 * w * u * t) + b * (t * t)) / d.max(1e-300)
            }
        }
    }
    pub fn mid(&self) -> Vec2 {
        self.point_at(0.5)
    }
    /// Unit tangent at the start (direction of travel).
    pub fn start_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::X),
            Seg2::Arc { start, sweep, .. } => Vec2::from_angle(start).perp() * sweep.signum(),
            Seg2::Cubic { p0, p1, p2, .. } => (p1 - p0).normalized().or_else(|| (p2 - p0).normalized()).unwrap_or(Vec2::X),
            Seg2::Conic { a, apex, b, .. } => (apex - a).normalized().or_else(|| (b - a).normalized()).unwrap_or(Vec2::X),
        }
    }
    /// Unit tangent at the end (direction of travel).
    pub fn end_tangent(&self) -> Vec2 {
        match *self {
            Seg2::Line { a, b } => (b - a).normalized().unwrap_or(Vec2::X),
            Seg2::Arc { start, sweep, .. } => Vec2::from_angle(start + sweep).perp() * sweep.signum(),
            Seg2::Cubic { p1, p2, p3, .. } => (p3 - p2).normalized().or_else(|| (p3 - p1).normalized()).unwrap_or(Vec2::X),
            Seg2::Conic { a, apex, b, .. } => (b - apex).normalized().or_else(|| (b - a).normalized()).unwrap_or(Vec2::X),
        }
    }
    pub fn reversed(&self) -> Seg2 {
        match *self {
            Seg2::Line { a, b } => Seg2::Line { a: b, b: a },
            Seg2::Arc { center, radius, start, sweep } => Seg2::Arc { center, radius, start: start + sweep, sweep: -sweep },
            Seg2::Cubic { p0, p1, p2, p3 } => Seg2::Cubic { p0: p3, p1: p2, p2: p1, p3: p0 },
            Seg2::Conic { a, apex, b, w } => Seg2::Conic { a: b, apex, b: a, w },
        }
    }
    pub fn length(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => a.dist(b),
            Seg2::Arc { radius, sweep, .. } => (radius * sweep).abs(),
            Seg2::Cubic { .. } | Seg2::Conic { .. } => self.integrate(|s, t| s.derivative(t).len()),
        }
    }
    /// The two halves of the segment (same kind, exact).
    pub fn split_half(&self) -> (Seg2, Seg2) {
        match *self {
            Seg2::Line { a, b } => {
                let m = a.lerp(b, 0.5);
                (Seg2::Line { a, b: m }, Seg2::Line { a: m, b })
            }
            Seg2::Arc { center, radius, start, sweep } => (
                Seg2::Arc { center, radius, start, sweep: sweep / 2.0 },
                Seg2::Arc { center, radius, start: start + sweep / 2.0, sweep: sweep / 2.0 },
            ),
            Seg2::Cubic { p0, p1, p2, p3 } => {
                let (a, b, c) = (p0.lerp(p1, 0.5), p1.lerp(p2, 0.5), p2.lerp(p3, 0.5));
                let (d, e) = (a.lerp(b, 0.5), b.lerp(c, 0.5));
                let m = d.lerp(e, 0.5);
                (Seg2::Cubic { p0, p1: a, p2: d, p3: m }, Seg2::Cubic { p0: m, p1: e, p2: c, p3 })
            }
            Seg2::Conic { a, apex, b, w } => {
                // Rational de Casteljau at the middle, weights normalised to 1 at the ends.
                let m = self.point_at(0.5);
                let w2 = ((1.0 + w) / 2.0).max(0.0).sqrt();
                let (l, r) = ((a + apex * w) / (1.0 + w), (apex * w + b) / (1.0 + w));
                (Seg2::Conic { a, apex: l, b: m, w: w2 }, Seg2::Conic { a: m, apex: r, b, w: w2 })
            }
        }
    }
    /// ∫₀¹ f(t) dt by composite Gauss–Legendre (16 pieces of 5 points).
    fn integrate(&self, f: impl Fn(&Seg2, f64) -> f64) -> f64 {
        let n = 16;
        let mut sum = 0.0;
        for k in 0..n {
            let (t0, h) = (k as f64 / n as f64, 1.0 / n as f64);
            for (x, w) in GL5 {
                sum += w * h * f(self, t0 + x * h);
            }
        }
        sum
    }
    /// Contribution to the loop's signed area (shoelace integral ½∮(x dy − y dx)).
    pub fn area_term(&self) -> f64 {
        match *self {
            Seg2::Line { a, b } => 0.5 * a.cross(b),
            Seg2::Arc { radius, sweep, .. } => {
                let (a, b) = (self.start(), self.end());
                // Chord term plus the circular segment between chord and arc.
                0.5 * a.cross(b) + 0.5 * radius * radius * (sweep - sweep.sin())
            }
            Seg2::Cubic { .. } | Seg2::Conic { .. } => self.integrate(|s, t| 0.5 * s.point_at(t).cross(s.derivative(t))),
        }
    }
    /// Polyline approximation (including both end points) with at most `tol` chord error.
    pub fn polyline(&self, tol: f64) -> Vec<Vec2> {
        match *self {
            Seg2::Line { a, b } => vec![a, b],
            Seg2::Arc { radius, sweep, .. } => {
                let tol = tol.max(radius.abs() * 1e-6).max(1e-9);
                let step = if tol < radius { 2.0 * (1.0 - tol / radius).clamp(-1.0, 1.0).acos() } else { TAU / 8.0 };
                let n = ((sweep.abs() / step.max(1e-6)).ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
            Seg2::Cubic { p0, p1, p2, p3 } => {
                // Chord error of n pieces ≈ max second difference / (8 n²).
                let bow = (p0 - p1 * 2.0 + p2).len().max((p1 - p2 * 2.0 + p3).len()) * 6.0;
                let n = ((bow / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
            Seg2::Conic { a, apex, b, w } => {
                let bow = (a - apex * 2.0 + b).len() * 2.0 * w.max(1.0);
                let n = ((bow / (8.0 * tol.max(1e-9))).sqrt().ceil() as usize).clamp(2, 4096);
                (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
            }
        }
    }
}

/// A closed loop of segments, each starting where the previous one ends.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loop2 {
    pub segs: Vec<Seg2>,
}

impl Loop2 {
    /// Full circle as two half arcs (counter-clockwise).
    pub fn circle(center: Vec2, radius: f64) -> Loop2 {
        Loop2 {
            segs: vec![
                Seg2::Arc { center, radius, start: 0.0, sweep: std::f64::consts::PI },
                Seg2::Arc { center, radius, start: std::f64::consts::PI, sweep: std::f64::consts::PI },
            ],
        }
    }
    /// Full circle starting (and split) at angle `start`.
    pub fn circle_from(center: Vec2, radius: f64, start: f64) -> Loop2 {
        use std::f64::consts::PI;
        Loop2 { segs: vec![Seg2::Arc { center, radius, start, sweep: PI }, Seg2::Arc { center, radius, start: start + PI, sweep: PI }] }
    }
    pub fn polygon(pts: &[Vec2]) -> Loop2 {
        let n = pts.len();
        Loop2 { segs: (0..n).filter_map(|i| Some(Seg2::Line { a: *pts.get(i)?, b: *pts.get((i + 1) % n)? })).collect() }
    }
    /// Signed area: positive when counter-clockwise.
    pub fn signed_area(&self) -> f64 {
        self.segs.iter().map(Seg2::area_term).sum()
    }
    pub fn reversed(&self) -> Loop2 {
        Loop2 { segs: self.segs.iter().rev().map(Seg2::reversed).collect() }
    }
    /// Counter-clockwise copy.
    pub fn ccw(&self) -> Loop2 {
        if self.signed_area() < 0.0 { self.reversed() } else { self.clone() }
    }
    pub fn polyline(&self, tol: f64) -> Vec<Vec2> {
        let mut out = Vec::new();
        for s in &self.segs {
            let p = s.polyline(tol);
            out.extend(p.iter().take(p.len().saturating_sub(1)));
        }
        out
    }
    /// Even-odd point containment against the polyline approximation.
    pub fn contains(&self, p: Vec2) -> bool {
        let poly = self.polyline(1e-3);
        let n = poly.len();
        let mut inside = false;
        for i in 0..n {
            let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            if (a.y > p.y) != (b.y > p.y) {
                let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                if p.x < x {
                    inside = !inside;
                }
            }
        }
        inside
    }
    /// Area-weighted centroid (of the polyline approximation).
    pub fn centroid(&self) -> Vec2 {
        let poly = self.polyline(1e-3);
        let n = poly.len();
        let (mut a2, mut c) = (0.0, Vec2::ZERO);
        for i in 0..n {
            let (Some(p), Some(q)) = (poly.get(i), poly.get((i + 1) % n)) else { continue };
            let k = p.cross(*q);
            a2 += k;
            c += (*p + *q) * k;
        }
        if a2.abs() < 1e-15 { poly.first().copied().unwrap_or_default() } else { c / (3.0 * a2) }
    }
    /// Closed within `tol`?
    pub fn is_closed(&self, tol: f64) -> bool {
        let n = self.segs.len();
        n > 0
            && (0..n).all(|i| match (self.segs.get(i), self.segs.get((i + 1) % n)) {
                (Some(a), Some(b)) => a.end().dist(b.start()) <= tol,
                _ => false,
            })
    }
}

/// A planar region: outer loop (counter-clockwise) minus holes (clockwise).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Region2 {
    pub outer: Loop2,
    pub holes: Vec<Loop2>,
}

impl Region2 {
    pub fn area(&self) -> f64 {
        self.outer.signed_area().abs() - self.holes.iter().map(|h| h.signed_area().abs()).sum::<f64>()
    }
    /// A point strictly inside the region (for picking and labels).
    pub fn interior_point(&self) -> Vec2 {
        let c = self.centroid();
        if self.contains(c) {
            return c;
        }
        // Scan a few horizontal lines for the midpoint of the widest inside span.
        let poly = self.outer.polyline(1e-2);
        let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
        for p in &poly {
            y0 = y0.min(p.y);
            y1 = y1.max(p.y);
        }
        for k in 1..16 {
            let y = y0 + (y1 - y0) * k as f64 / 16.0;
            let mut xs: Vec<f64> = Vec::new();
            for l in std::iter::once(&self.outer).chain(&self.holes) {
                let pl = l.polyline(1e-2);
                let n = pl.len();
                for i in 0..n {
                    let (Some(a), Some(b)) = (pl.get(i), pl.get((i + 1) % n)) else { continue };
                    if (a.y > y) != (b.y > y) {
                        xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                    }
                }
            }
            xs.sort_by(f64::total_cmp);
            for w in xs.chunks(2) {
                if let [a, b] = w {
                    let p = Vec2::new((a + b) * 0.5, y);
                    if self.contains(p) {
                        return p;
                    }
                }
            }
        }
        c
    }
    pub fn contains(&self, p: Vec2) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
    /// Triangles covering the region (for filled display), by slabs between the vertex
    /// heights: inside each slab the boundary edges don't cross, so consecutive crossings pair
    /// up into trapezoids.
    pub fn triangulate(&self, tol: f64) -> Vec<[Vec2; 3]> {
        let mut edges: Vec<(Vec2, Vec2)> = Vec::new();
        for l in std::iter::once(&self.outer).chain(&self.holes) {
            let pl = l.polyline(tol);
            let n = pl.len();
            for i in 0..n {
                let (Some(a), Some(b)) = (pl.get(i), pl.get((i + 1) % n)) else { continue };
                if (a.y - b.y).abs() > 1e-12 {
                    edges.push(if a.y < b.y { (*a, *b) } else { (*b, *a) });
                }
            }
        }
        if edges.len() > 20_000 {
            return Vec::new();
        }
        let mut ys: Vec<f64> = edges.iter().flat_map(|(a, b)| [a.y, b.y]).collect();
        ys.sort_by(f64::total_cmp);
        ys.dedup_by(|a, b| (*a - *b).abs() < 1e-12);
        let x_at = |e: &(Vec2, Vec2), y: f64| e.0.x + (y - e.0.y) / (e.1.y - e.0.y) * (e.1.x - e.0.x);
        let mut out = Vec::new();
        for w in ys.windows(2) {
            let (y0, y1) = (w[0], w[1]);
            let ym = (y0 + y1) * 0.5;
            let mut span: Vec<&(Vec2, Vec2)> = edges.iter().filter(|e| e.0.y <= ym && e.1.y >= ym).collect();
            span.sort_by(|a, b| x_at(a, ym).total_cmp(&x_at(b, ym)));
            for pair in span.chunks(2) {
                let [l, r] = pair else { continue };
                let (a, b) = (Vec2::new(x_at(l, y0), y0), Vec2::new(x_at(r, y0), y0));
                let (c, d) = (Vec2::new(x_at(r, y1), y1), Vec2::new(x_at(l, y1), y1));
                out.push([a, b, c]);
                out.push([a, c, d]);
            }
        }
        out
    }
    pub fn centroid(&self) -> Vec2 {
        let ao = self.outer.signed_area().abs();
        let mut c = self.outer.centroid() * ao;
        let mut a = ao;
        for h in &self.holes {
            let ah = h.signed_area().abs();
            c -= h.centroid() * ah;
            a -= ah;
        }
        if a.abs() < 1e-15 { self.outer.centroid() } else { c / a }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulated_area() {
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 8.0), Vec2::new(5.0, 3.0), Vec2::new(0.0, 8.0)]);
        let r = Region2 { outer: sq, holes: vec![Loop2::circle(Vec2::new(3.0, 1.5), 1.0).reversed()] };
        let tris = r.triangulate(1e-3);
        let a: f64 = tris.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() * 0.5).sum();
        // The circle is a polyline within the tolerance, so the areas agree to about that.
        assert!((a - r.area()).abs() < 1e-3 * r.area(), "{a} vs {}", r.area());
        for [a, b, c] in &tris {
            let m = (*a + *b + *c) / 3.0;
            assert!((*b - *a).cross(*c - *a).abs() < 1e-12 || r.contains(m), "{m:?}");
        }
    }

    #[test]
    fn areas() {
        let sq = Loop2::polygon(&[Vec2::new(0.0, 0.0), Vec2::new(4.0, 0.0), Vec2::new(4.0, 3.0), Vec2::new(0.0, 3.0)]);
        assert!((sq.signed_area() - 12.0).abs() < 1e-12);
        assert!((sq.reversed().signed_area() + 12.0).abs() < 1e-12);
        let c = Loop2::circle(Vec2::new(5.0, 5.0), 2.0);
        assert!((c.signed_area() - std::f64::consts::PI * 4.0).abs() < 1e-9);
        assert!(c.is_closed(1e-9));
        let r = Region2 { outer: Loop2::circle(Vec2::ZERO, 10.0), holes: vec![Loop2::circle(Vec2::ZERO, 5.0).reversed()] };
        assert!((r.area() - std::f64::consts::PI * 75.0).abs() < 1e-9);
        let ip = r.interior_point();
        assert!(r.contains(ip), "{ip:?}");
        assert!(sq.contains(Vec2::new(1.0, 1.0)) && !sq.contains(Vec2::new(5.0, 1.0)));
        assert!(sq.centroid().dist(Vec2::new(2.0, 1.5)) < 1e-9);
    }
}
