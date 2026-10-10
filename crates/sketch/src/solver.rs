//! The constraint solver.
//!
//! Unknowns are the coordinates of every non-fixed point and the radius of every circle. Each
//! constraint contributes residuals (zero when satisfied, in millimetres so that all rows have a
//! comparable scale). The system is split into connected components and each is solved with a
//! damped Gauss–Newton (Levenberg–Marquardt) iteration on a dense Jacobian built from local
//! central differences. Small steps from the current positions are preferred, so
//! under-constrained sketches move as little as possible. Afterwards the rank of the Jacobian
//! gives the remaining degrees of freedom and which points are fully determined.

use serde::{Deserialize, Serialize};
use solvecraft_geom::Vec2;

use crate::linalg::{Mat, determined_vars, solve as lin_solve};
use crate::model::{ConstraintKind, CurveKind, Sketch};

const MAX_ITERS: usize = 500;
/// Converged when every residual is below this (mm).
const TOL: f64 = 1e-9;
/// A solution is accepted when every residual is below this (mm).
const ACCEPT: f64 = 1e-6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolveStatus {
    /// All constraints satisfied.
    Solved,
    /// The constraints conflict (or the solver could not satisfy them); geometry was left as it
    /// was before solving.
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SolveReport {
    pub status: SolveStatus,
    /// Remaining degrees of freedom (0 = fully constrained).
    pub dof: usize,
    pub iterations: usize,
    pub max_residual: f64,
    /// Constraints still violated after a failed solve.
    pub failing: Vec<String>,
    /// Per point: fully determined by constraints (fixed points count as determined).
    pub point_determined: Vec<bool>,
    /// Per curve: all of its defining values determined.
    pub curve_determined: Vec<bool>,
}

impl SolveReport {
    pub fn ok(&self) -> bool {
        self.status == SolveStatus::Solved
    }
    pub fn fully_constrained(&self) -> bool {
        self.dof == 0
    }
}

/// Variable layout: point → (x var, y var), circle → radius var.
struct Layout {
    pvar: Vec<Option<usize>>,
    rvar: Vec<Option<usize>>,
    n: usize,
}

/// Geometry held in place for one solve: point indices and curves whose radius is held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hold {
    pub points: Vec<usize>,
    pub radii: Vec<usize>,
}

fn layout(sk: &Sketch, hold: &Hold) -> Layout {
    let mut fixed: Vec<bool> = sk.points.iter().map(|p| p.fixed).collect();
    for p in &hold.points {
        if let Some(f) = fixed.get_mut(*p) {
            *f = true;
        }
    }
    for c in &sk.constraints {
        if let ConstraintKind::Fix { p } = c.kind
            && let Some(f) = fixed.get_mut(p)
        {
            *f = true;
        }
    }
    for c in sk.curves.iter().filter(|c| c.fixed) {
        for p in c.kind.point_ids() {
            if let Some(f) = fixed.get_mut(p) {
                *f = true;
            }
        }
    }
    let mut n = 0;
    let pvar = fixed
        .iter()
        .map(|f| {
            if *f {
                None
            } else {
                n += 2;
                Some(n - 2)
            }
        })
        .collect();
    let rvar = sk
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| match c.kind {
            CurveKind::Circle { .. } | CurveKind::Ellipse { .. } if c.link.is_none() && !c.fixed && !hold.radii.contains(&i) => {
                n += 1;
                Some(n - 1)
            }
            _ => None,
        })
        .collect();
    Layout { pvar, rvar, n }
}

/// Read-only view of the sketch with variable values substituted.
struct State<'a> {
    sk: &'a Sketch,
    lay: &'a Layout,
    x: &'a [f64],
}

impl State<'_> {
    fn p(&self, i: usize) -> Vec2 {
        match self.lay.pvar.get(i).copied().flatten() {
            Some(v) => Vec2::new(self.x.get(v).copied().unwrap_or(0.0), self.x.get(v + 1).copied().unwrap_or(0.0)),
            None => self.sk.points.get(i).map(|p| p.pos).unwrap_or_default(),
        }
    }
    fn line(&self, l: usize) -> (Vec2, Vec2) {
        match self.sk.curves.get(l).map(|c| &c.kind) {
            Some(CurveKind::Line { a, b }) => (self.p(*a), self.p(*b)),
            _ => (Vec2::ZERO, Vec2::X),
        }
    }
    /// Centre and radius of a circle or arc.
    fn round(&self, c: usize) -> (Vec2, f64) {
        match self.sk.curves.get(c).map(|c| &c.kind) {
            Some(CurveKind::Circle { c: ci, r }) => {
                let r = match self.lay.rvar.get(c).copied().flatten() {
                    Some(v) => self.x.get(v).copied().unwrap_or(*r),
                    None => *r,
                };
                (self.p(*ci), r)
            }
            Some(CurveKind::Arc { c: ci, a, .. }) => {
                let cc = self.p(*ci);
                (cc, cc.dist(self.p(*a)))
            }
            _ => (Vec2::ZERO, 0.0),
        }
    }
    fn is_line(&self, c: usize) -> bool {
        matches!(self.sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Line { .. }))
    }
    fn is_freeform(&self, c: usize) -> bool {
        self.sk.curves.get(c).is_some_and(|c| c.kind.is_freeform())
    }
    /// Polyline of a free-form curve with trial values.
    fn poly(&self, c: usize) -> Vec<Vec2> {
        match self.sk.curves.get(c).map(|c| &c.kind) {
            Some(CurveKind::Ellipse { c: ci, m, r }) => {
                let r = match self.lay.rvar.get(c).copied().flatten() {
                    Some(v) => self.x.get(v).copied().unwrap_or(*r),
                    None => *r,
                };
                crate::curves::ellipse_polyline(self.p(*ci), self.p(*m), r)
            }
            Some(CurveKind::Spline { pts, control, degree }) => {
                let p: Vec<Vec2> = pts.iter().map(|q| self.p(*q)).collect();
                crate::curves::spline_polyline(&p, *control, *degree)
            }
            Some(CurveKind::Conic { a, b, apex, rho }) => crate::curves::conic_polyline(self.p(*a), self.p(*apex), self.p(*b), *rho),
            _ => Vec::new(),
        }
    }
    /// Radius of a circle or arc; an ellipse's minor radius.
    fn radius_of(&self, c: usize) -> f64 {
        match self.sk.curves.get(c).map(|x| &x.kind) {
            Some(CurveKind::Ellipse { r, .. }) => match self.lay.rvar.get(c).copied().flatten() {
                Some(v) => self.x.get(v).copied().unwrap_or(*r),
                None => *r,
            },
            _ => self.round(c).1,
        }
    }
    fn ends(&self, c: usize) -> Option<(usize, usize)> {
        self.sk.curves.get(c)?.kind.ends()
    }
    /// Length along an arc (counter-clockwise from its start).
    fn arc_length(&self, c: usize) -> f64 {
        match self.sk.curves.get(c).map(|c| &c.kind) {
            Some(CurveKind::Arc { c: ci, a, b }) => {
                let (cc, pa, pb) = (self.p(*ci), self.p(*a), self.p(*b));
                let mut sw = (pb - cc).angle() - (pa - cc).angle();
                while sw <= 1e-12 {
                    sw += std::f64::consts::TAU;
                }
                cc.dist(pa) * sw
            }
            _ => 0.0,
        }
    }
    /// Curvature vector of a curve at its point `p` (towards the centre of curvature).
    fn curvature_at(&self, c: usize, p: usize) -> Vec2 {
        let Some(kind) = self.sk.curves.get(c).map(|c| &c.kind) else { return Vec2::ZERO };
        match kind {
            CurveKind::Line { .. } | CurveKind::Ellipse { .. } => Vec2::ZERO,
            CurveKind::Circle { .. } | CurveKind::Arc { .. } => {
                let (cc, r) = self.round(c);
                (cc - self.p(p)) / (r * r).max(1e-24)
            }
            CurveKind::Spline { pts, control, degree } => {
                let q: Vec<Vec2> = pts.iter().map(|i| self.p(*i)).collect();
                let at_end = pts.last() == Some(&p) && pts.first() != Some(&p);
                crate::curves::end_curvature(&|t| crate::curves::spline_point(&q, *control, *degree, t), at_end)
            }
            CurveKind::Conic { a, b, apex, rho } => {
                let (pa, pb, px) = (self.p(*a), self.p(*b), self.p(*apex));
                crate::curves::end_curvature(&|t| crate::curves::conic_point(pa, px, pb, *rho, t), p == *b)
            }
        }
    }
    /// Unit tangent of any curve at its point `p` (an end point for open free-form curves).
    fn tangent_at(&self, c: usize, p: usize) -> Option<Vec2> {
        match self.sk.curves.get(c).map(|c| &c.kind)? {
            CurveKind::Line { a, b } => (self.p(*b) - self.p(*a)).normalized(),
            CurveKind::Circle { .. } | CurveKind::Arc { .. } => (self.p(p) - self.round(c).0).perp().normalized(),
            CurveKind::Spline { pts, control, .. } => {
                let q: Vec<Vec2> = pts.iter().map(|i| self.p(*i)).collect();
                crate::curves::spline_end_tangent(&q, *control, pts.last() == Some(&p) && pts.first() != Some(&p))
            }
            CurveKind::Conic { a, b, apex, .. } => {
                if p == *a {
                    (self.p(*apex) - self.p(*a)).normalized()
                } else if p == *b {
                    (self.p(*b) - self.p(*apex)).normalized()
                } else {
                    None
                }
            }
            CurveKind::Ellipse { .. } => {
                let poly = self.poly(c);
                let q = self.p(p);
                let k = (0..poly.len().saturating_sub(1)).min_by(|i, j| poly[*i].dist(q).total_cmp(&poly[*j].dist(q)))?;
                (*poly.get(k + 1)? - *poly.get(k)?).normalized()
            }
        }
    }
}

fn wrap_angle(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    let mut a = a % t;
    if a > std::f64::consts::PI {
        a -= t;
    } else if a <= -std::f64::consts::PI {
        a += t;
    }
    a
}

/// Distance from `p` to the infinite line through `a`, `b` (signed by side).
fn line_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l = d.len().max(1e-12);
    d.cross(p - a) / l
}

/// Residual rows of one constraint.
fn residuals(s: &State, k: &ConstraintKind, out: &mut Vec<f64>) {
    use ConstraintKind::*;
    match *k {
        Coincident { p, q } => {
            let d = s.p(p) - s.p(q);
            out.extend([d.x, d.y]);
        }
        PointOnCurve { p, c } => {
            let pp = s.p(p);
            if s.is_freeform(c) {
                out.push(crate::curves::polyline_signed_dist(&s.poly(c), pp));
            } else if s.is_line(c) {
                let (a, b) = s.line(c);
                out.push(line_dist(pp, a, b));
            } else {
                let (cc, r) = s.round(c);
                out.push(pp.dist(cc) - r);
            }
        }
        Horizontal { l } => {
            let (a, b) = s.line(l);
            out.push(b.y - a.y);
        }
        Vertical { l } => {
            let (a, b) = s.line(l);
            out.push(b.x - a.x);
        }
        HorizontalPoints { p, q } => out.push(s.p(q).y - s.p(p).y),
        VerticalPoints { p, q } => out.push(s.p(q).x - s.p(p).x),
        Parallel { a, b } | Perpendicular { a, b } => {
            let (a0, a1) = s.line(a);
            let (b0, b1) = s.line(b);
            let (da, db) = (a1 - a0, b1 - b0);
            // The sine (parallel) or cosine (perpendicular) of the angle between the lines:
            // independent of their lengths, so the solver can't satisfy it by shrinking a line
            // to nothing (scaling by the summed lengths did, collapsing dragged rectangles).
            let scale = (da.len() * db.len()).max(1e-12);
            let v = if matches!(k, Parallel { .. }) { da.cross(db) } else { da.dot(db) };
            out.push(v / scale);
        }
        Collinear { a, b } => {
            let (a0, a1) = s.line(a);
            let (b0, b1) = s.line(b);
            out.push(line_dist(b0, a0, a1));
            out.push(line_dist(b1, a0, a1));
        }
        Tangent { a, b } => {
            let (la, lb) = (s.is_line(a), s.is_line(b));
            if s.is_freeform(a) || s.is_freeform(b) {
                // Without a shared end point: the curves touch (closest approach zero).
                let (f, o) = if s.is_freeform(a) { (a, b) } else { (b, a) };
                let poly = s.poly(f);
                // Signed gap at the closest approach (smooth through zero, unlike its size).
                let closest = |v: &mut dyn Iterator<Item = f64>| v.fold(f64::INFINITY, |m: f64, x| if x.abs() < m.abs() { x } else { m });
                let d = if s.is_line(o) {
                    let (p0, p1) = s.line(o);
                    closest(&mut poly.iter().map(|q| line_dist(*q, p0, p1)))
                } else if s.is_freeform(o) {
                    let other = s.poly(o);
                    closest(&mut poly.iter().map(|q| crate::curves::polyline_signed_dist(&other, *q)))
                } else {
                    let (cc, r) = s.round(o);
                    closest(&mut poly.iter().map(|q| q.dist(cc) - r))
                };
                out.push(if d.is_finite() { d } else { 0.0 });
            } else if la || lb {
                let (l, c) = if la { (a, b) } else { (b, a) };
                let (p0, p1) = s.line(l);
                let (cc, r) = s.round(c);
                out.push(line_dist(cc, p0, p1).abs() - r);
            } else {
                let (c1, r1) = s.round(a);
                let (c2, r2) = s.round(b);
                let d = c1.dist(c2);
                let ext = d - (r1 + r2);
                let int = d - (r1 - r2).abs();
                out.push(if ext.abs() <= int.abs() { ext } else { int });
            }
        }
        Equal { a, b } => {
            if s.is_line(a) {
                let (a0, a1) = s.line(a);
                let (b0, b1) = s.line(b);
                out.push(a0.dist(a1) - b0.dist(b1));
            } else {
                out.push(s.round(a).1 - s.round(b).1);
            }
        }
        Concentric { a, b } => {
            let d = s.round(a).0 - s.round(b).0;
            out.extend([d.x, d.y]);
        }
        Midpoint { p, l } => {
            let (a, b) = s.line(l);
            let d = s.p(p) - (a + b) * 0.5;
            out.extend([d.x, d.y]);
        }
        Symmetric { p, q, l } => {
            let (a, b) = s.line(l);
            let (pp, qq) = (s.p(p), s.p(q));
            let dir = b - a;
            let len = dir.len().max(1e-12);
            out.push(line_dist((pp + qq) * 0.5, a, b));
            out.push((qq - pp).dot(dir) / len);
        }
        Fix { .. } => {}
        Distance { p, q, value } => out.push(s.p(p).dist(s.p(q)) - value),
        DistanceX { p, q, value } => out.push((s.p(q).x - s.p(p).x).abs() - value),
        DistanceY { p, q, value } => out.push((s.p(q).y - s.p(p).y).abs() - value),
        PointLineDistance { p, l, value } => {
            let (a, b) = s.line(l);
            out.push(line_dist(s.p(p), a, b).abs() - value);
        }
        Length { l, value } => {
            let (a, b) = s.line(l);
            out.push(a.dist(b) - value);
        }
        Radius { c, value } => out.push(s.radius_of(c) - value),
        Diameter { c, value } => out.push(2.0 * s.round(c).1 - value),
        Angle { a, b, value, flip } => {
            let (a0, a1) = s.line(a);
            let (b0, b1) = s.line(b);
            let (da, db) = (a1 - a0, b1 - b0);
            let ang = da.cross(db).atan2(da.dot(db));
            let scale = 0.5 * (da.len() + db.len());
            let target = if flip { value + std::f64::consts::PI } else { value };
            out.push(wrap_angle(ang - target) * scale.max(1e-6));
        }
        Smooth { a, b } => {
            let shared = match (s.ends(a), s.ends(b)) {
                (Some((a0, a1)), Some((b0, b1))) => [a0, a1].into_iter().find(|x| *x == b0 || *x == b1),
                _ => None,
            };
            let Some(p) = shared else {
                out.extend([0.0, 0.0, 0.0]);
                return;
            };
            match (s.tangent_at(a, p), s.tangent_at(b, p)) {
                (Some(u), Some(v)) => out.push(u.cross(v) * 10.0),
                _ => out.push(0.0),
            }
            let k = s.curvature_at(a, p) - s.curvature_at(b, p);
            out.extend([k.x * 100.0, k.y * 100.0]);
        }
        ArcLength { c, value } => out.push(s.arc_length(c) - value),
        LinearDiameter { p, l, value } => {
            let (a, b) = s.line(l);
            out.push(2.0 * line_dist(s.p(p), a, b).abs() - value);
        }
    }
}

/// Measured value of a dimension (for driven dimensions).
fn measure(s: &State, k: &ConstraintKind) -> Option<f64> {
    use ConstraintKind::*;
    Some(match *k {
        Distance { p, q, .. } => s.p(p).dist(s.p(q)),
        DistanceX { p, q, .. } => (s.p(q).x - s.p(p).x).abs(),
        DistanceY { p, q, .. } => (s.p(q).y - s.p(p).y).abs(),
        PointLineDistance { p, l, .. } => {
            let (a, b) = s.line(l);
            line_dist(s.p(p), a, b).abs()
        }
        Length { l, .. } => {
            let (a, b) = s.line(l);
            a.dist(b)
        }
        Radius { c, .. } => s.radius_of(c),
        Diameter { c, .. } => 2.0 * s.round(c).1,
        Angle { a, b, flip, .. } => {
            let (a0, a1) = s.line(a);
            let (b0, b1) = s.line(b);
            let (da, db) = (a1 - a0, b1 - b0);
            let ang = da.cross(db).atan2(da.dot(db)) - if flip { std::f64::consts::PI } else { 0.0 };
            ang.rem_euclid(std::f64::consts::TAU)
        }
        ArcLength { c, .. } => s.arc_length(c),
        LinearDiameter { p, l, .. } => {
            let (a, b) = s.line(l);
            2.0 * line_dist(s.p(p), a, b).abs()
        }
        _ => return None,
    })
}

/// What a dimension measures on the sketch as it stands (mm, radians for angles); `None` for a
/// constraint that is not a dimension.
pub fn measure_dimension(sk: &Sketch, k: &ConstraintKind) -> Option<f64> {
    let lay = Layout { pvar: vec![None; sk.points.len()], rvar: vec![None; sk.curves.len()], n: 0 };
    measure(&State { sk, lay: &lay, x: &[] }, k).filter(|v| v.is_finite())
}

/// One residual block: implicit arc radius equality, a user constraint, or a tangency at a known
/// touch point (better conditioned than the distance form when an end point is on the curve).
enum Block<'a> {
    Arc { c: usize, a: usize, b: usize },
    User(usize, &'a ConstraintKind),
    TangentAt(usize, &'a ConstraintKind, usize),
}

fn copy_block<'a>(b: &Block<'a>) -> Block<'a> {
    match b {
        Block::Arc { c, a, b } => Block::Arc { c: *c, a: *a, b: *b },
        Block::User(i, k) => Block::User(*i, k),
        Block::TangentAt(i, k, p) => Block::TangentAt(*i, k, *p),
    }
}

fn block_residuals(s: &State, b: &Block, out: &mut Vec<f64>) {
    match b {
        Block::Arc { c, a, b } => {
            let cc = s.p(*c);
            out.push(cc.dist(s.p(*b)) - cc.dist(s.p(*a)));
        }
        Block::User(_, k) => residuals(s, k, out),
        Block::TangentAt(_, k, p) => {
            let ConstraintKind::Tangent { a, b } = **k else { return };
            let pt = s.p(*p);
            if s.is_freeform(a) || s.is_freeform(b) {
                // Directions at the shared end parallel.
                match (s.tangent_at(a, *p), s.tangent_at(b, *p)) {
                    (Some(u), Some(v)) => out.push(u.cross(v) * 10.0),
                    _ => out.push(0.0),
                }
            } else if s.is_line(a) || s.is_line(b) {
                let (l, c) = if s.is_line(a) { (a, b) } else { (b, a) };
                let (p0, p1) = s.line(l);
                let (cc, _) = s.round(c);
                let d = p1 - p0;
                // The radius to the touch point is perpendicular to the line.
                out.push((pt - cc).dot(d) / d.len().max(1e-12));
            } else {
                let (c1, _) = s.round(a);
                let (c2, _) = s.round(b);
                let (u, v) = (pt - c1, pt - c2);
                // Both centres on one line through the touch point.
                out.push(u.cross(v) / (u.len() + v.len()).max(1e-12));
            }
        }
    }
}

/// A point where a tangency must happen: an end point shared by the two curves, or a line end
/// point constrained onto the round curve.
fn touch_point(sk: &Sketch, k: &ConstraintKind) -> Option<usize> {
    let ConstraintKind::Tangent { a, b } = *k else { return None };
    let ends = |c: usize| -> Vec<usize> {
        match sk.curves.get(c).map(|c| &c.kind) {
            Some(k) => k.ends().map(|(a, b)| vec![a, b]).unwrap_or_default(),
            None => Vec::new(),
        }
    };
    let (ea, eb) = (ends(a), ends(b));
    if let Some(p) = ea.iter().find(|p| eb.contains(p)) {
        return Some(*p);
    }
    let on = |p: usize, c: usize| sk.constraints.iter().any(|x| x.kind == ConstraintKind::PointOnCurve { p, c });
    ea.iter().find(|p| on(**p, b)).or_else(|| eb.iter().find(|p| on(**p, a))).copied()
}

/// Variables a block depends on.
fn block_vars(sk: &Sketch, lay: &Layout, b: &Block) -> Vec<usize> {
    let mut v = Vec::new();
    let pt = |v: &mut Vec<usize>, p: usize| {
        if let Some(x) = lay.pvar.get(p).copied().flatten() {
            v.extend([x, x + 1]);
        }
    };
    let curve = |v: &mut Vec<usize>, c: usize| {
        if let Some(k) = sk.curves.get(c).map(|c| &c.kind) {
            for q in k.point_ids() {
                pt(v, q);
            }
            if let Some(r) = lay.rvar.get(c).copied().flatten() {
                v.push(r);
            }
        }
    };
    use ConstraintKind::*;
    match b {
        Block::Arc { c, a, b } => {
            pt(&mut v, *c);
            pt(&mut v, *a);
            pt(&mut v, *b);
        }
        Block::User(_, k) => match **k {
            Coincident { p, q } | HorizontalPoints { p, q } | VerticalPoints { p, q } => {
                pt(&mut v, p);
                pt(&mut v, q);
            }
            Distance { p, q, .. } | DistanceX { p, q, .. } | DistanceY { p, q, .. } => {
                pt(&mut v, p);
                pt(&mut v, q);
            }
            PointOnCurve { p, c } | Midpoint { p, l: c } | PointLineDistance { p, l: c, .. } => {
                pt(&mut v, p);
                curve(&mut v, c);
            }
            Horizontal { l } | Vertical { l } | Length { l, .. } | Radius { c: l, .. } | Diameter { c: l, .. } => curve(&mut v, l),
            Parallel { a, b }
            | Perpendicular { a, b }
            | Collinear { a, b }
            | Tangent { a, b }
            | Equal { a, b }
            | Concentric { a, b }
            | Angle { a, b, .. } => {
                curve(&mut v, a);
                curve(&mut v, b);
            }
            Symmetric { p, q, l } => {
                pt(&mut v, p);
                pt(&mut v, q);
                curve(&mut v, l);
            }
            Fix { .. } => {}
            Smooth { a, b } => {
                curve(&mut v, a);
                curve(&mut v, b);
            }
            ArcLength { c, .. } => curve(&mut v, c),
            LinearDiameter { p, l, .. } => {
                pt(&mut v, p);
                curve(&mut v, l);
            }
        },
        Block::TangentAt(_, k, p) => {
            pt(&mut v, *p);
            if let ConstraintKind::Tangent { a, b } = **k {
                curve(&mut v, a);
                curve(&mut v, b);
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p == i {
            break;
        }
        let gp = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = gp;
        }
        i = p;
    }
    i
}

/// Residual vector and Jacobian (rows × all n vars) for the given blocks.
fn system(sk: &Sketch, lay: &Layout, blocks: &[(Block, Vec<usize>)], x: &mut [f64], with_j: bool) -> (Vec<f64>, Option<Mat>, Vec<usize>) {
    let mut r = Vec::new();
    let mut owner = Vec::new();
    for (bi, (b, _)) in blocks.iter().enumerate() {
        let before = r.len();
        block_residuals(&State { sk, lay, x }, b, &mut r);
        owner.extend(std::iter::repeat_n(bi, r.len() - before));
    }
    if !with_j {
        return (r, None, owner);
    }
    let mut j = Mat::zeros(r.len(), lay.n);
    let mut row = 0;
    let mut plus = Vec::new();
    let mut minus = Vec::new();
    for (b, vars) in blocks {
        plus.clear();
        block_residuals(&State { sk, lay, x }, b, &mut plus);
        let rows = plus.len();
        for &v in vars {
            let Some(x0) = x.get(v).copied() else { continue };
            let h = 1e-7 * x0.abs().max(1.0);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0 + h;
            }
            plus.clear();
            block_residuals(&State { sk, lay, x }, b, &mut plus);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0 - h;
            }
            minus.clear();
            block_residuals(&State { sk, lay, x }, b, &mut minus);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0;
            }
            for k in 0..rows {
                let d = (plus.get(k).copied().unwrap_or(0.0) - minus.get(k).copied().unwrap_or(0.0)) / (2.0 * h);
                j.set(row + k, v, d);
            }
        }
        row += rows;
    }
    (r, Some(j), owner)
}

/// Residuals and the Jacobian restricted to `vars` (columns in that order; `col` maps a global
/// variable to its column, usize::MAX elsewhere). Blocks only touch their component's variables,
/// so a component's Jacobian is small even in a huge sketch.
fn local_system(
    sk: &Sketch,
    lay: &Layout,
    blocks: &[(Block, Vec<usize>)],
    x: &mut [f64],
    col: &[usize],
    ncols: usize,
) -> (Vec<f64>, Mat, Vec<usize>) {
    let (r, _, owner) = system(sk, lay, blocks, x, false);
    let mut j = Mat::zeros(r.len(), ncols);
    let mut row = 0;
    let mut plus = Vec::new();
    let mut minus = Vec::new();
    for (b, vars) in blocks {
        plus.clear();
        block_residuals(&State { sk, lay, x }, b, &mut plus);
        let rows = plus.len();
        for &v in vars {
            let Some(c) = col.get(v).copied().filter(|c| *c < ncols) else { continue };
            let Some(x0) = x.get(v).copied() else { continue };
            let h = 1e-7 * x0.abs().max(1.0);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0 + h;
            }
            plus.clear();
            block_residuals(&State { sk, lay, x }, b, &mut plus);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0 - h;
            }
            minus.clear();
            block_residuals(&State { sk, lay, x }, b, &mut minus);
            if let Some(xv) = x.get_mut(v) {
                *xv = x0;
            }
            for k in 0..rows {
                let d = (plus.get(k).copied().unwrap_or(0.0) - minus.get(k).copied().unwrap_or(0.0)) / (2.0 * h);
                j.set(row + k, c, d);
            }
        }
        row += rows;
    }
    (r, j, owner)
}

fn max_abs(v: &[f64]) -> f64 {
    v.iter().fold(0.0_f64, |m, x| if x.is_finite() { m.max(x.abs()) } else { f64::INFINITY })
}
fn norm2(v: &[f64]) -> f64 {
    v.iter().map(|x| x * x).sum()
}

/// Levenberg–Marquardt on one component (variables `vars`, blocks `blocks`). Returns iterations.
fn lm(sk: &Sketch, lay: &Layout, blocks: &[(Block, Vec<usize>)], vars: &[usize], x: &mut Vec<f64>, col: &[usize]) -> usize {
    let n = vars.len();
    let mut lambda = 1e-3;
    let (mut r, _, _) = system(sk, lay, blocks, x, false);
    let mut f = norm2(&r);
    for it in 0..MAX_ITERS {
        if max_abs(&r) < TOL {
            return it;
        }
        let (_, j, _) = local_system(sk, lay, blocks, x, col, n);
        // Normal equations of this component.
        let mut a = Mat::zeros(n, n);
        let mut g = vec![0.0; n];
        for row in 0..j.rows {
            let ri = r.get(row).copied().unwrap_or(0.0);
            let nz: Vec<(usize, f64)> = (0..n).map(|li| (li, j.get(row, li))).filter(|(_, d)| *d != 0.0).collect();
            for &(li, di) in &nz {
                if let Some(gi) = g.get_mut(li) {
                    *gi += di * ri;
                }
                for &(lk, dk) in &nz {
                    a.add(li, lk, di * dk);
                }
            }
        }
        let mut improved = false;
        // Damping floor: variables the residuals barely see must not take huge steps.
        let floor = (0..n).map(|i| a.get(i, i)).fold(0.0_f64, f64::max) * 1e-4;
        for _ in 0..30 {
            let mut m = a.clone();
            for i in 0..n {
                let d = a.get(i, i);
                m.add(i, i, lambda * (d.max(floor) + 1e-9) + 1e-12);
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let Some(step) = lin_solve(m, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let mut trial = x.clone();
            for (li, &v) in vars.iter().enumerate() {
                if let (Some(t), Some(s)) = (trial.get_mut(v), step.get(li)) {
                    *t += s;
                }
            }
            let (rt, _, _) = system(sk, lay, blocks, &mut trial, false);
            let ft = norm2(&rt);
            if ft.is_finite() && ft < f {
                *x = trial;
                r = rt;
                f = ft;
                lambda = (lambda / 3.0).max(1e-12);
                improved = true;
                break;
            }
            lambda *= 4.0;
            if lambda > 1e12 {
                break;
            }
        }
        if !improved {
            return it;
        }
    }
    MAX_ITERS
}

/// Solve the sketch in place. On failure the geometry is restored and the report lists the
/// violated constraints.
pub fn solve(sk: &mut Sketch) -> SolveReport {
    solve_holding(sk, &Hold::default())
}

/// Solve with some geometry held as if it were fixed (for this solve only): applying a
/// constraint between two entities holds the first so only the second moves. Everything else
/// moves as little as it can. On failure the geometry is restored.
pub fn solve_holding(sk: &mut Sketch, hold: &Hold) -> SolveReport {
    let lay = layout(sk, hold);
    let mut x = vec![0.0; lay.n];
    for (i, p) in sk.points.iter().enumerate() {
        if let Some(v) = lay.pvar.get(i).copied().flatten() {
            if let Some(s) = x.get_mut(v) {
                *s = p.pos.x;
            }
            if let Some(s) = x.get_mut(v + 1) {
                *s = p.pos.y;
            }
        }
    }
    for (i, c) in sk.curves.iter().enumerate() {
        if let (CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }, Some(v)) = (&c.kind, lay.rvar.get(i).copied().flatten())
            && let Some(s) = x.get_mut(v)
        {
            *s = *r;
        }
    }
    let x0 = x.clone();

    let mut blocks: Vec<(Block, Vec<usize>)> = Vec::new();
    for c in &sk.curves {
        if let CurveKind::Arc { c, a, b } = c.kind {
            let bl = Block::Arc { c, a, b };
            let vars = block_vars(sk, &lay, &bl);
            blocks.push((bl, vars));
        }
    }
    for (i, c) in sk.constraints.iter().enumerate() {
        if c.driven {
            continue;
        }
        let bl = match touch_point(sk, &c.kind) {
            Some(p) => Block::TangentAt(i, &c.kind, p),
            None => Block::User(i, &c.kind),
        };
        let vars = block_vars(sk, &lay, &bl);
        blocks.push((bl, vars));
    }

    // Connected components over variables.
    let mut parent: Vec<usize> = (0..lay.n).collect();
    for (_, vars) in &blocks {
        if let Some(&first) = vars.first() {
            for &v in vars.iter().skip(1) {
                let (ra, rb) = (find(&mut parent, first), find(&mut parent, v));
                if let Some(p) = parent.get_mut(ra) {
                    *p = rb;
                }
            }
        }
    }
    let mut comps: Vec<(usize, Vec<usize>, Vec<usize>)> = Vec::new(); // (root, vars, block idx)
    let mut comp_of: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for v in 0..lay.n {
        let r = find(&mut parent, v);
        match comp_of.get(&r) {
            Some(&k) => {
                if let Some(c) = comps.get_mut(k) {
                    c.1.push(v);
                }
            }
            None => {
                comp_of.insert(r, comps.len());
                comps.push((r, vec![v], Vec::new()));
            }
        }
    }
    let mut constant_blocks = Vec::new();
    for (bi, (_, vars)) in blocks.iter().enumerate() {
        match vars.first() {
            Some(&v) => {
                let r = find(&mut parent, v);
                if let Some(c) = comp_of.get(&r).and_then(|k| comps.get_mut(*k)) {
                    c.2.push(bi);
                }
            }
            None => constant_blocks.push(bi),
        }
    }
    // Column of each variable within its component.
    let mut col = vec![usize::MAX; lay.n];
    for (_, vars, _) in &comps {
        for (k, v) in vars.iter().enumerate() {
            if let Some(c) = col.get_mut(*v) {
                *c = k;
            }
        }
    }

    let mut iterations = 0;
    // Solve each component; `blocks` is consumed by index so rebuild per component.
    let all_blocks = blocks;
    for (_, vars, bis) in &comps {
        if bis.is_empty() {
            continue;
        }
        let sub: Vec<(Block, Vec<usize>)> = bis.iter().filter_map(|&bi| all_blocks.get(bi).map(|(b, v)| (copy_block(b), v.clone()))).collect();
        iterations += lm(sk, &lay, &sub, vars, &mut x, &col);
    }

    // Final residuals and analysis, component by component (the Jacobian is block diagonal:
    // its rank is the sum of the components' ranks).
    let mut max_residual = 0.0_f64;
    let mut failing: Vec<String> = Vec::new();
    let mut det = vec![false; lay.n];
    let mut rank = 0;
    let mut check = |r: &[f64], owner: &[usize], blocks: &[(Block, Vec<usize>)], failing: &mut Vec<String>| {
        max_residual = max_residual.max(max_abs(r));
        for (row, v) in r.iter().enumerate() {
            if !(v.abs() < ACCEPT)
                && let Some((Block::User(ci, _) | Block::TangentAt(ci, _, _), _)) = owner.get(row).and_then(|bi| blocks.get(*bi))
                && let Some(c) = sk.constraints.get(*ci)
                && !failing.contains(&c.id)
            {
                failing.push(c.id.clone());
            }
        }
    };
    for (_, vars, bis) in &comps {
        let sub: Vec<(Block, Vec<usize>)> = bis.iter().filter_map(|&bi| all_blocks.get(bi).map(|(b, v)| (copy_block(b), v.clone()))).collect();
        if sub.is_empty() {
            continue;
        }
        let (r, j, owner) = local_system(sk, &lay, &sub, &mut x, &col, vars.len());
        check(&r, &owner, &sub, &mut failing);
        if j.rows > 0 {
            let (d, rk) = determined_vars(&j, 1e-7);
            rank += rk;
            for (k, v) in vars.iter().enumerate() {
                if let (Some(slot), Some(flag)) = (det.get_mut(*v), d.get(k)) {
                    *slot = *flag;
                }
            }
        }
    }
    let consts: Vec<(Block, Vec<usize>)> =
        constant_blocks.iter().filter_map(|&bi| all_blocks.get(bi).map(|(b, v)| (copy_block(b), v.clone()))).collect();
    if !consts.is_empty() {
        let (r, _, owner) = system(sk, &lay, &consts, &mut x, false);
        check(&r, &owner, &consts, &mut failing);
    }
    let status = if max_residual < ACCEPT { SolveStatus::Solved } else { SolveStatus::Failed };
    let use_x = if status == SolveStatus::Solved { &x } else { &x0 };
    // Write back.
    for (i, p) in sk.points.iter_mut().enumerate() {
        if let Some(v) = lay.pvar.get(i).copied().flatten() {
            p.pos = Vec2::new(use_x.get(v).copied().unwrap_or(p.pos.x), use_x.get(v + 1).copied().unwrap_or(p.pos.y));
        }
    }
    for (i, c) in sk.curves.iter_mut().enumerate() {
        if let (CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. }, Some(v)) = (&mut c.kind, lay.rvar.get(i).copied().flatten()) {
            *r = use_x.get(v).copied().unwrap_or(*r).abs().max(1e-9);
        }
    }
    let vdet = |v: Option<usize>| v.is_none_or(|v| det.get(v).copied().unwrap_or(false));
    let pdet = |p: usize| match lay.pvar.get(p).copied().flatten() {
        Some(v) => vdet(Some(v)) && vdet(Some(v + 1)),
        None => true,
    };
    // Driven dimensions measure the solved geometry.
    let fixed_lay = Layout { pvar: vec![None; sk.points.len()], rvar: vec![None; sk.curves.len()], n: 0 };
    let measured: Vec<Option<f64>> =
        sk.constraints.iter().map(|c| if c.driven { measure(&State { sk, lay: &fixed_lay, x: &[] }, &c.kind) } else { None }).collect();
    for (c, m) in sk.constraints.iter_mut().zip(measured) {
        if let Some(v) = m
            && v.is_finite()
        {
            c.kind.set_value(v);
        }
    }
    let point_determined: Vec<bool> = (0..sk.points.len()).map(pdet).collect();
    let curve_determined =
        sk.curves.iter().enumerate().map(|(i, c)| c.kind.point_ids().into_iter().all(pdet) && vdet(lay.rvar.get(i).copied().flatten())).collect();
    SolveReport {
        status,
        dof: lay.n.saturating_sub(rank),
        iterations,
        max_residual: if max_residual.is_finite() { max_residual } else { f64::MAX },
        failing,
        point_determined,
        curve_determined,
    }
}
