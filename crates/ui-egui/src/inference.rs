//! Drawing-time inference for the Line tool, like Fusion's: while the next point is placed the
//! cursor snaps to directions and places taken from the existing geometry (parallel or
//! perpendicular to a line, on the extension of a line, tangent to a circle or arc). A dashed
//! guide and a label show it; placing the point adds the matching constraint.

use std::cell::RefCell;

use egui::{Shape, Stroke};
use serde_json::json;
use solvecraft_engine::geom::Vec2;
use solvecraft_engine::sketch::{CurveKind, Sketch};

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::Proj;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Parallel,
    Perpendicular,
    Extension,
    Tangent,
}

/// A snap taken from existing geometry: what, to which curve, where the point goes, how far the
/// cursor was moved, and the guide to draw (sketch coordinates).
#[derive(Clone, Debug, PartialEq)]
pub struct Inference {
    pub kind: Kind,
    pub curve: String,
    pub at: Vec2,
    pub dist: f64,
    pub guide: (Vec2, Vec2),
}

impl Inference {
    pub fn label(&self) -> &'static str {
        match self.kind {
            Kind::Parallel => "//",
            Kind::Perpendicular => "\u{22a5}",
            Kind::Extension => "Ext",
            Kind::Tangent => "T",
        }
    }
}

thread_local! {
    static PENDING: RefCell<Option<Inference>> = const { RefCell::new(None) };
}

/// Forget the current inference (another snap won, or the tool ended).
pub fn clear() {
    PENDING.with(|p| *p.borrow_mut() = None);
}

pub fn current() -> Option<Inference> {
    PENDING.with(|p| p.borrow().clone())
}

fn keep(i: Option<Inference>) -> Option<Inference> {
    PENDING.with(|p| *p.borrow_mut() = i.clone());
    i
}

fn line_ends(sk: &Sketch, c: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(c)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

/// Tangent from the previous point `prev` to a circle or arc, when the cursor `lp` is near the
/// tangent point (within `tol`).
pub fn tangent(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<Inference> {
    let mut best: Option<Inference> = None;
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction || !matches!(c.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }) {
            continue;
        }
        let (Some(o), Some(r)) = (sk.center(i), sk.radius(i)) else { continue };
        let d = prev.dist(o);
        if d <= r + 1e-9 {
            continue;
        }
        // The two tangent points seen from `prev`.
        let base = (prev - o).angle();
        let half = (r / d).acos();
        for s in [-1.0, 1.0] {
            let t = o + Vec2::from_angle(base + s * half) * r;
            let on_arc = sk.shape(i).is_none_or(|sh| sh.dist(t) < 1e-6);
            let e = t.dist(lp);
            if on_arc && e < tol && best.as_ref().is_none_or(|b| e < b.dist) {
                best = Some(Inference { kind: Kind::Tangent, curve: c.id.clone(), at: t, dist: e, guide: (prev, t) });
            }
        }
    }
    keep(best)
}

/// Directions and places from lines: parallel or perpendicular to a line (from `prev`), or on
/// the extension of a line beyond its ends.
pub fn lines(sk: &Sketch, prev: Vec2, lp: Vec2, tol: f64) -> Option<Inference> {
    let mut best: Option<Inference> = None;
    let mut offer = |i: Inference| {
        if best.as_ref().is_none_or(|b| i.dist < b.dist) {
            best = Some(i);
        }
    };
    let v = lp - prev;
    if v.len() < tol * 2.0 {
        return keep(None);
    }
    for (i, c) in sk.curves.iter().enumerate() {
        if c.construction && c.link.is_none() {
            continue;
        }
        let Some((a, b)) = line_ends(sk, i) else { continue };
        let Some(dir) = (b - a).normalized() else { continue };
        // Axis-aligned directions are the H/V snaps' business.
        let axis = dir.x.abs() < 1e-9 || dir.y.abs() < 1e-9;
        let touches = a.dist(prev) < 1e-9 || b.dist(prev) < 1e-9;
        for (kind, d) in [(Kind::Parallel, dir), (Kind::Perpendicular, dir.perp())] {
            if axis || (kind == Kind::Parallel && touches) {
                continue;
            }
            let t = v.dot(d);
            let at = prev + d * t;
            let e = at.dist(lp);
            if e < tol {
                offer(Inference {
                    kind,
                    curve: c.id.clone(),
                    at,
                    dist: e,
                    guide: (prev - d * (t.signum() * tol * 6.0), at + d * (t.signum() * tol * 6.0)),
                });
            }
        }
        // The extension: beyond the ends, on the line's own (infinite) line.
        let s = (lp - a).dot(dir);
        let len = a.dist(b);
        if !(-tol..=len + tol).contains(&s) {
            let at = a + dir * s;
            let e = at.dist(lp);
            if e < tol * 0.8 && !touches {
                let from = if s < 0.0 { a } else { b };
                offer(Inference { kind: Kind::Extension, curve: c.id.clone(), at, dist: e * 0.9, guide: (from, at) });
            }
        }
    }
    keep(best)
}

/// The Line tool placed a segment ending at `end` (the new line `line`): add the constraint of
/// the inference it was snapped by.
pub fn commit(app: &mut SolveApp, line: &str, end: Vec2) {
    let Some(i) = current().filter(|i| i.at.dist(end) < 1e-9) else { return };
    clear();
    let (cmd, params) = match i.kind {
        Kind::Parallel => ("sketch.constraint.parallel", json!({"a": i.curve, "b": line})),
        Kind::Perpendicular => ("sketch.constraint.perpendicular", json!({"a": i.curve, "b": line})),
        Kind::Tangent => ("sketch.constraint.tangent", json!({"a": i.curve, "b": line})),
        Kind::Extension => ("sketch.constraint.coincident", json!({"a": format!("{line}.end"), "b": i.curve})),
    };
    // An inference that would over-constrain the sketch is simply not kept.
    let _ = app.session.execute(cmd, &params);
}

/// The dashed guide of the current inference (its label is the snap hint) (while the Line tool is active).
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().is_none_or(|t| t.cmd != "sketch.line") {
        return;
    }
    let Some(i) = current() else { return };
    let st = app.session.world_state();
    let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = Tokens::get();
    if let (Some(a), Some(b)) = (to(i.guide.0), to(i.guide.1)) {
        painter.add(Shape::dashed_line(&[a, b], Stroke::new(1.0, tk.rubber_band), 5.0, 4.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk() -> Sketch {
        let mut s = Sketch::new();
        let _ = s.add_line(Vec2::new(0.0, 0.0), Vec2::new(10.0, 10.0), None, None, Some("l1"));
        let _ = s.add_circle(Vec2::new(30.0, 0.0), 5.0, None, Some("c1"));
        s
    }

    #[test]
    fn parallel_and_perpendicular_from_the_previous_point() {
        let s = sk();
        let i = lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.2, 29.9), 0.5).expect("parallel");
        assert_eq!(i.kind, Kind::Parallel);
        assert!((i.at - Vec2::new(0.0, 20.0)).cross(Vec2::new(1.0, 1.0)).abs() < 1e-9);
        let i = lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.1, 10.0), 0.5).expect("perpendicular");
        assert_eq!(i.kind, Kind::Perpendicular);
        assert!(lines(&s, Vec2::new(0.0, 20.0), Vec2::new(10.0, 25.0), 0.5).is_none());
    }

    #[test]
    fn extension_beyond_the_end() {
        let s = sk();
        let i = lines(&s, Vec2::new(30.0, 15.0), Vec2::new(20.1, 19.9), 0.5).expect("extension");
        assert_eq!(i.kind, Kind::Extension);
        assert!((i.at.x - i.at.y).abs() < 1e-9 && i.at.x > 10.0);
    }

    #[test]
    fn tangent_point_on_a_circle() {
        let s = sk();
        let prev = Vec2::new(30.0, 20.0);
        // Tangent points from (30, 20) to the circle at (30, 0) r 5.
        let half = (5.0f64 / 20.0).acos();
        let t = Vec2::new(30.0, 0.0) + Vec2::from_angle(std::f64::consts::FRAC_PI_2 + half) * 5.0;
        let i = tangent(&s, prev, t + Vec2::new(0.2, 0.1), 0.5).expect("tangent");
        assert_eq!(i.kind, Kind::Tangent);
        assert!(i.at.dist(t) < 1e-9);
        assert!((i.at - Vec2::new(30.0, 0.0)).dot(i.at - prev).abs() < 1e-9);
    }
}
