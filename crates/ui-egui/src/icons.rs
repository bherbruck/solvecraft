//! Toolbar and browser icons, drawn in code (no image assets). Each icon is designed on a
//! 24 × 24 grid and scaled to the target rectangle.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    ink: Color32,
    fill: Color32,
    accent: Color32,
    w: f32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        pos2(self.r.left() + x / 24.0 * self.r.width(), self.r.top() + y / 24.0 * self.r.height())
    }
    fn s(&self) -> f32 {
        self.r.width() / 24.0
    }
    fn line(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::line(v, Stroke::new(self.w, c)));
    }
    fn poly(&self, pts: &[(f32, f32)], fill: Color32, c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::convex_polygon(v, fill, Stroke::new(self.w, c)));
    }
    fn closed(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::closed_line(v, Stroke::new(self.w, c)));
    }
    fn circle(&self, x: f32, y: f32, r: f32, fill: Color32, c: Color32) {
        self.p.circle(self.at(x, y), r * self.s(), fill, Stroke::new(self.w, c));
    }
    fn dot(&self, x: f32, y: f32, c: Color32) {
        self.p.circle_filled(self.at(x, y), 1.6 * self.s(), c);
    }
    fn arc(&self, x: f32, y: f32, r: f32, a0: f32, a1: f32, c: Color32) {
        let n = 24;
        let pts: Vec<(f32, f32)> = (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| (x + r * a.cos(), y - r * a.sin())).collect();
        self.line(&pts, c);
    }
    fn arrow(&self, from: (f32, f32), to: (f32, f32), c: Color32) {
        self.line(&[from, to], c);
        let d = vec2(to.0 - from.0, to.1 - from.1).normalized();
        let n = vec2(-d.y, d.x);
        let tip = vec2(to.0, to.1);
        let a = tip - d * 3.5 + n * 2.2;
        let b = tip - d * 3.5 - n * 2.2;
        self.poly(&[to, (a.x, a.y), (b.x, b.y)], c, c);
    }
    /// Isometric box from a 2D footprint height.
    fn iso_box(&self, x0: f32, y0: f32, w: f32, d: f32, h: f32) {
        let top = [(x0, y0), (x0 + w, y0 - d * 0.5), (x0 + w + d, y0), (x0 + d, y0 + d * 0.5)];
        let front = [(x0, y0), (x0 + d, y0 + d * 0.5), (x0 + d, y0 + d * 0.5 + h), (x0, y0 + h)];
        let side = [(x0 + d, y0 + d * 0.5), (x0 + w + d, y0), (x0 + w + d, y0 + h), (x0 + d, y0 + d * 0.5 + h)];
        self.poly(&front, self.fill.gamma_multiply(0.85), self.ink);
        self.poly(&side, self.fill.gamma_multiply(0.7), self.ink);
        self.poly(&top, self.fill, self.ink);
    }
}

/// Paint the icon `name` into `r`. `accent` highlights the "action" part.
pub fn paint(p: &Painter, r: Rect, name: &str, ink: Color32, fill: Color32, accent: Color32) {
    let pen = Pen { p, r, ink, fill, accent, w: (r.width() / 24.0 * 1.4).max(1.0) };
    let a = pen.accent;
    match name {
        "sketch" => {
            pen.poly(&[(3.0, 14.0), (13.0, 9.0), (21.0, 13.0), (11.0, 18.0)], pen.fill, ink);
            pen.line(&[(7.0, 13.0), (16.0, 4.0), (18.0, 6.0), (9.0, 15.0), (6.5, 15.5), (7.0, 13.0)], a);
        }
        "finish" => {
            pen.circle(12.0, 12.0, 9.0, Color32::TRANSPARENT, pen.accent);
            pen.line(&[(7.0, 12.5), (10.5, 16.0), (17.0, 8.5)], a);
        }
        "extrude" => {
            pen.iso_box(4.0, 11.0, 9.0, 7.0, 7.0);
            pen.arrow((12.0, 9.0), (12.0, 2.5), a);
        }
        "revolve" => {
            pen.line(&[(12.0, 2.0), (12.0, 22.0)], ink);
            pen.poly(&[(13.0, 7.0), (19.0, 7.0), (19.0, 17.0), (13.0, 17.0)], pen.fill, ink);
            pen.arc(12.0, 12.0, 9.0, 3.6, 5.8, a);
            pen.arrow((15.0, 20.4), (18.5, 19.0), a);
        }
        "box" => pen.iso_box(3.0, 9.0, 10.0, 8.0, 9.0),
        "cylinder" => {
            pen.poly(&[(5.0, 6.0), (19.0, 6.0), (19.0, 18.0), (5.0, 18.0)], pen.fill.gamma_multiply(0.8), Color32::TRANSPARENT);
            pen.line(&[(5.0, 6.0), (5.0, 18.0)], ink);
            pen.line(&[(19.0, 6.0), (19.0, 18.0)], ink);
            let e = |y: f32, fillc: Color32| {
                let pts: Vec<Pos2> =
                    (0..32).map(|i| i as f32 / 32.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 7.0 * t.cos(), y + 2.5 * t.sin())).collect();
                p.add(Shape::convex_polygon(pts, fillc, Stroke::new(pen.w, ink)));
            };
            pen.arc(12.0, 18.0, 7.0, std::f32::consts::PI, std::f32::consts::TAU, ink);
            e(6.0, pen.fill);
        }
        "sphere" => {
            pen.circle(12.0, 12.0, 8.5, pen.fill, ink);
            pen.arc(12.0, 12.0, 8.5, 3.3, 6.1, ink.gamma_multiply(0.5));
            p.circle_filled(pen.at(9.0, 8.5), 2.0 * pen.s(), Color32::from_white_alpha(160));
        }
        "torus" => {
            let pts: Vec<Pos2> =
                (0..40).map(|i| i as f32 / 40.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 9.0 * t.cos(), 12.0 + 5.5 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, pen.fill, Stroke::new(pen.w, ink)));
            let pts: Vec<Pos2> =
                (0..40).map(|i| i as f32 / 40.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 3.5 * t.cos(), 11.0 + 1.8 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::new(pen.w, ink)));
        }
        "fillet" => {
            pen.poly(&[(4.0, 20.0), (4.0, 4.0), (10.0, 4.0), (10.0, 14.0), (20.0, 14.0), (20.0, 20.0)], pen.fill, Color32::TRANSPARENT);
            pen.line(&[(4.0, 20.0), (4.0, 4.0)], ink);
            pen.line(&[(4.0, 20.0), (20.0, 20.0)], ink);
            pen.arc(4.0, 4.0, 16.0, -std::f32::consts::FRAC_PI_2, 0.0, ink);
            pen.arc(20.0, 4.0, 10.0, std::f32::consts::PI, std::f32::consts::PI * 1.5, a);
        }
        "chamfer" => {
            pen.poly(&[(4.0, 20.0), (4.0, 4.0), (12.0, 4.0), (20.0, 12.0), (20.0, 20.0)], pen.fill, ink);
            pen.line(&[(12.0, 4.0), (20.0, 12.0)], a);
        }
        "combine" => {
            pen.poly(&[(3.0, 8.0), (14.0, 8.0), (14.0, 19.0), (3.0, 19.0)], pen.fill, ink);
            pen.poly(&[(9.0, 4.0), (21.0, 4.0), (21.0, 15.0), (9.0, 15.0)], pen.fill.gamma_multiply(0.75), a);
        }
        "pattern_rect" => {
            for (x, y) in [(4.0, 4.0), (14.0, 4.0), (4.0, 14.0), (14.0, 14.0)] {
                pen.poly(
                    &[(x, y), (x + 6.0, y), (x + 6.0, y + 6.0), (x, y + 6.0)],
                    if x == 4.0 && y == 4.0 { pen.fill } else { Color32::TRANSPARENT },
                    if x == 4.0 && y == 4.0 { ink } else { a },
                );
            }
        }
        "pattern_circ" => {
            for i in 0..6 {
                let t = i as f32 / 6.0 * std::f32::consts::TAU;
                pen.circle(
                    12.0 + 7.5 * t.cos(),
                    12.0 + 7.5 * t.sin(),
                    2.4,
                    if i == 0 { pen.fill } else { Color32::TRANSPARENT },
                    if i == 0 { ink } else { a },
                );
            }
        }
        "hole" => {
            pen.poly(&[(3.0, 8.0), (21.0, 8.0), (21.0, 20.0), (3.0, 20.0)], pen.fill, ink);
            let pts: Vec<Pos2> =
                (0..32).map(|i| i as f32 / 32.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 5.0 * t.cos(), 8.0 + 2.0 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, Color32::WHITE, Stroke::new(pen.w, a)));
            pen.line(&[(7.0, 8.0), (7.0, 16.0)], a);
            pen.line(&[(17.0, 8.0), (17.0, 16.0)], a);
        }
        "sweep" => {
            pen.arc(20.0, 20.0, 14.0, std::f32::consts::FRAC_PI_2, std::f32::consts::PI, ink);
            pen.circle(6.0, 20.0, 3.0, pen.fill, a);
            pen.circle(20.0, 6.0, 3.0, pen.fill, a);
        }
        "loft" => {
            pen.poly(&[(3.0, 20.0), (15.0, 20.0), (21.0, 15.0), (9.0, 15.0)], pen.fill, ink);
            pen.circle(12.0, 6.0, 3.5, Color32::TRANSPARENT, a);
            pen.line(&[(3.0, 20.0), (8.5, 6.0)], a);
            pen.line(&[(21.0, 15.0), (15.5, 6.0)], a);
        }
        "shell" => {
            pen.iso_box(3.0, 9.0, 10.0, 8.0, 9.0);
            pen.poly(&[(6.0, 9.0), (13.0, 5.5), (18.0, 8.0), (11.0, 11.5)], Color32::WHITE, a);
        }
        "draft" => {
            pen.poly(&[(4.0, 20.0), (20.0, 20.0), (17.0, 5.0), (7.0, 5.0)], pen.fill, ink);
            pen.line(&[(20.0, 20.0), (20.0, 5.0)], a);
        }
        "split" => {
            pen.poly(&[(3.0, 6.0), (21.0, 6.0), (21.0, 18.0), (3.0, 18.0)], pen.fill, ink);
            pen.line(&[(8.0, 2.0), (16.0, 22.0)], a);
        }
        "mirror" => {
            pen.line(&[(12.0, 2.0), (12.0, 22.0)], ink);
            pen.poly(&[(3.0, 7.0), (9.0, 10.0), (9.0, 17.0), (3.0, 17.0)], pen.fill, ink);
            pen.poly(&[(21.0, 7.0), (15.0, 10.0), (15.0, 17.0), (21.0, 17.0)], Color32::TRANSPARENT, a);
        }
        "lookat" => {
            pen.poly(&[(4.0, 6.0), (16.0, 6.0), (16.0, 18.0), (4.0, 18.0)], pen.fill, ink);
            pen.arrow((22.0, 12.0), (13.0, 12.0), a);
        }
        "style" => {
            pen.poly(&[(12.0, 3.0), (20.0, 7.5), (12.0, 12.0), (4.0, 7.5)], pen.fill, ink);
            pen.poly(&[(4.0, 7.5), (12.0, 12.0), (12.0, 21.0), (4.0, 16.5)], a, ink);
            pen.poly(&[(12.0, 12.0), (20.0, 7.5), (20.0, 16.5), (12.0, 21.0)], Color32::TRANSPARENT, ink);
        }
        "sun" => {
            pen.circle(12.0, 12.0, 4.5, a, ink);
            for i in 0..8 {
                let t = i as f32 / 8.0 * std::f32::consts::TAU;
                pen.line(&[(12.0 + 7.0 * t.cos(), 12.0 + 7.0 * t.sin()), (12.0 + 9.5 * t.cos(), 12.0 + 9.5 * t.sin())], ink);
            }
        }
        "moon" => {
            let outer: Vec<Pos2> =
                (0..=24).map(|i| -0.6 + i as f32 / 24.0 * 4.4).map(|t| pen.at(12.0 + 8.0 * t.cos(), 12.0 + 8.0 * t.sin())).collect();
            let inner: Vec<Pos2> =
                (0..=24).rev().map(|i| -0.9 + i as f32 / 24.0 * 3.9).map(|t| pen.at(15.5 + 6.5 * t.cos(), 9.5 + 6.5 * t.sin())).collect();
            let mut pts = outer;
            pts.extend(inner);
            p.add(Shape::closed_line(pts, Stroke::new(pen.w, ink)));
        }
        "section" => {
            pen.poly(&[(4.0, 8.0), (14.0, 4.0), (20.0, 8.0), (10.0, 12.0)], pen.fill, ink);
            pen.poly(&[(4.0, 8.0), (10.0, 12.0), (10.0, 20.0), (4.0, 16.0)], pen.fill, ink);
            pen.poly(&[(10.0, 12.0), (20.0, 8.0), (20.0, 16.0), (10.0, 20.0)], a, ink);
        }
        "presspull" => {
            pen.poly(&[(3.0, 15.0), (12.0, 11.0), (21.0, 15.0), (12.0, 19.0)], pen.fill, ink);
            pen.arrow((12.0, 15.0), (12.0, 3.0), a);
        }
        "move" => {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0, 12.0), (12.0 + 9.0 * dx, 12.0 + 9.0 * dy), a);
            }
        }
        "params" => {
            pen.line(&[(8.0, 20.0), (9.0, 8.0), (10.0, 5.0), (13.0, 4.0)], ink);
            pen.line(&[(6.0, 11.0), (12.0, 11.0)], ink);
            pen.line(&[(14.0, 13.0), (20.0, 20.0)], a);
            pen.line(&[(20.0, 13.0), (14.0, 20.0)], a);
        }
        "compute" => {
            pen.arc(12.0, 12.0, 7.5, 0.4, 3.0, ink);
            pen.arc(12.0, 12.0, 7.5, 3.5, 6.0, a);
            pen.arrow((5.0, 9.0), (4.6, 13.0), ink);
            pen.arrow((19.0, 15.0), (19.4, 11.0), a);
        }
        "delete" => {
            pen.line(&[(6.0, 6.0), (18.0, 18.0)], pen.accent);
            pen.line(&[(18.0, 6.0), (6.0, 18.0)], pen.accent);
        }
        "measure" => {
            pen.poly(&[(2.0, 15.0), (15.0, 2.0), (22.0, 9.0), (9.0, 22.0)], pen.fill, ink);
            for i in 0..5 {
                let t = 4.0 + i as f32 * 2.6;
                pen.line(&[(t, 15.0 - (t - 2.0)), (t + 2.0, 15.0 - (t - 2.0) + 2.0)], ink);
            }
        }
        "line" => {
            pen.line(&[(4.0, 19.0), (20.0, 5.0)], a);
            pen.dot(4.0, 19.0, ink);
            pen.dot(20.0, 5.0, ink);
        }
        "rect" | "rect3" | "rect_center" => {
            pen.closed(&[(4.0, 6.0), (20.0, 6.0), (20.0, 18.0), (4.0, 18.0)], a);
            match name {
                "rect" => {
                    pen.dot(4.0, 18.0, ink);
                    pen.dot(20.0, 6.0, ink);
                }
                "rect3" => {
                    pen.dot(4.0, 18.0, ink);
                    pen.dot(20.0, 18.0, ink);
                    pen.dot(20.0, 6.0, ink);
                }
                _ => {
                    pen.dot(12.0, 12.0, ink);
                    pen.dot(20.0, 6.0, ink);
                }
            }
        }
        "circle" | "circle2" | "circle3" => {
            pen.circle(12.0, 12.0, 8.0, Color32::TRANSPARENT, a);
            match name {
                "circle" => {
                    pen.dot(12.0, 12.0, ink);
                    pen.line(&[(12.0, 12.0), (17.6, 6.4)], ink);
                }
                "circle2" => {
                    pen.dot(4.0, 12.0, ink);
                    pen.dot(20.0, 12.0, ink);
                }
                _ => {
                    pen.dot(4.0, 12.0, ink);
                    pen.dot(12.0, 4.0, ink);
                    pen.dot(20.0, 12.0, ink);
                }
            }
        }
        "arc3" | "arc_center" => {
            pen.arc(12.0, 16.0, 9.0, 0.0, std::f32::consts::PI, a);
            pen.dot(3.0, 16.0, ink);
            pen.dot(21.0, 16.0, ink);
            if name == "arc3" {
                pen.dot(12.0, 7.0, ink);
            } else {
                pen.dot(12.0, 16.0, ink);
            }
        }
        "polygon" => {
            let pts: Vec<(f32, f32)> =
                (0..6).map(|i| i as f32 / 6.0 * std::f32::consts::TAU + 0.52).map(|t| (12.0 + 9.0 * t.cos(), 12.0 + 9.0 * t.sin())).collect();
            pen.closed(&pts, a);
            pen.dot(12.0, 12.0, ink);
        }
        "slot" => {
            pen.arc(8.0, 12.0, 5.0, std::f32::consts::FRAC_PI_2, std::f32::consts::PI * 1.5, a);
            pen.arc(16.0, 12.0, 5.0, -std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2, a);
            pen.line(&[(8.0, 7.0), (16.0, 7.0)], a);
            pen.line(&[(8.0, 17.0), (16.0, 17.0)], a);
            pen.dot(8.0, 12.0, ink);
            pen.dot(16.0, 12.0, ink);
        }
        "point" => {
            pen.dot(12.0, 12.0, a);
            pen.circle(12.0, 12.0, 4.0, Color32::TRANSPARENT, ink);
        }
        "dimension" => {
            pen.line(&[(4.0, 18.0), (4.0, 8.0)], ink);
            pen.line(&[(20.0, 18.0), (20.0, 8.0)], ink);
            pen.arrow((12.0, 11.0), (4.5, 11.0), a);
            pen.arrow((12.0, 11.0), (19.5, 11.0), a);
            pen.line(&[(9.0, 15.0), (15.0, 15.0)], ink);
        }
        "construction" => {
            for i in 0..4 {
                let t = i as f32 * 4.5;
                pen.line(&[(3.0 + t, 19.0 - t), (5.5 + t, 16.5 - t)], pen.accent);
            }
        }
        "c_hv" => {
            pen.line(&[(4.0, 18.0), (20.0, 18.0)], a);
            pen.line(&[(6.0, 4.0), (6.0, 14.0)], a);
        }
        "c_coincident" => {
            pen.line(&[(3.0, 18.0), (12.0, 12.0)], ink);
            pen.line(&[(12.0, 12.0), (21.0, 16.0)], ink);
            pen.circle(12.0, 12.0, 2.6, Color32::WHITE, a);
        }
        "c_tangent" => {
            pen.circle(12.0, 14.0, 6.0, Color32::TRANSPARENT, ink);
            pen.line(&[(3.0, 8.0), (21.0, 8.0)], a);
        }
        "c_equal" => {
            pen.line(&[(5.0, 9.0), (19.0, 9.0)], a);
            pen.line(&[(5.0, 15.0), (19.0, 15.0)], a);
        }
        "c_parallel" => {
            pen.line(&[(5.0, 19.0), (13.0, 5.0)], a);
            pen.line(&[(11.0, 19.0), (19.0, 5.0)], a);
        }
        "c_perpendicular" => {
            pen.line(&[(4.0, 19.0), (20.0, 19.0)], a);
            pen.line(&[(12.0, 19.0), (12.0, 5.0)], a);
            pen.line(&[(12.0, 15.0), (16.0, 15.0), (16.0, 19.0)], ink);
        }
        "c_fix" => {
            pen.circle(12.0, 9.0, 4.0, Color32::TRANSPARENT, a);
            pen.line(&[(12.0, 13.0), (12.0, 21.0)], a);
            pen.line(&[(9.0, 21.0), (15.0, 21.0)], ink);
        }
        "c_midpoint" => {
            pen.line(&[(3.0, 15.0), (21.0, 9.0)], ink);
            pen.poly(&[(12.0, 9.0), (15.0, 12.0), (12.0, 15.0), (9.0, 12.0)], a, a);
        }
        "c_concentric" => {
            pen.circle(12.0, 12.0, 8.5, Color32::TRANSPARENT, a);
            pen.circle(12.0, 12.0, 4.0, Color32::TRANSPARENT, a);
        }
        "c_collinear" => {
            pen.line(&[(3.0, 19.0), (10.0, 12.0)], a);
            pen.line(&[(14.0, 8.0), (21.0, 1.0)], a);
        }
        "c_symmetry" => {
            pen.line(&[(12.0, 3.0), (12.0, 21.0)], ink);
            pen.poly(&[(4.0, 8.0), (9.0, 12.0), (4.0, 16.0)], a, a);
            pen.poly(&[(20.0, 8.0), (15.0, 12.0), (20.0, 16.0)], a, a);
        }
        "undo" | "redo" => {
            let (x0, x1) = if name == "undo" { (19.0, 5.0) } else { (5.0, 19.0) };
            pen.line(&[(x0, 18.0), (x0, 12.0), (12.0, 9.0), (x1, 9.0)], ink);
            pen.arrow((x1 + (x0 - x1) * 0.15, 9.0), (x1, 9.0), ink);
        }
        "new" => {
            pen.poly(&[(6.0, 3.0), (15.0, 3.0), (19.0, 7.0), (19.0, 21.0), (6.0, 21.0)], Color32::WHITE, ink);
            pen.line(&[(15.0, 3.0), (15.0, 7.0), (19.0, 7.0)], ink);
        }
        "open" => {
            pen.poly(&[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (20.0, 9.0), (20.0, 19.0), (3.0, 19.0)], pen.fill, ink);
        }
        "save" => {
            pen.poly(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)], pen.fill, ink);
            pen.poly(&[(8.0, 4.0), (16.0, 4.0), (16.0, 9.0), (8.0, 9.0)], Color32::WHITE, ink);
        }
        "export" => {
            pen.poly(&[(4.0, 10.0), (4.0, 20.0), (16.0, 20.0), (16.0, 10.0)], pen.fill, ink);
            pen.arrow((10.0, 14.0), (20.0, 4.0), a);
        }
        "import" => {
            pen.iso_box(4.0, 12.0, 12.0, 7.0, 7.0);
            pen.arrow((20.0, 3.0), (12.0, 11.0), a);
        }
        "home" => {
            pen.poly(&[(4.0, 12.0), (12.0, 4.0), (20.0, 12.0)], pen.fill, ink);
            pen.poly(&[(6.0, 12.0), (18.0, 12.0), (18.0, 20.0), (6.0, 20.0)], pen.fill, ink);
        }
        "fit" => {
            for (cx, cy, dx, dy) in [(4.0, 4.0, 1.0, 1.0), (20.0, 4.0, -1.0, 1.0), (4.0, 20.0, 1.0, -1.0), (20.0, 20.0, -1.0, -1.0)] {
                pen.line(&[(cx + dx * 5.0, cy), (cx, cy), (cx, cy + dy * 5.0)], ink);
            }
            pen.poly(&[(9.0, 9.0), (15.0, 9.0), (15.0, 15.0), (9.0, 15.0)], pen.fill, ink);
        }
        "orbit" => {
            pen.arc(12.0, 12.0, 8.0, 0.3, 5.5, ink);
            pen.arrow((17.0, 17.5), (19.6, 14.5), a);
            pen.dot(12.0, 12.0, ink);
        }
        "pan" => {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0 + 3.0 * dx, 12.0 + 3.0 * dy), (12.0 + 9.0 * dx, 12.0 + 9.0 * dy), ink);
            }
        }
        "zoom" => {
            pen.circle(10.0, 10.0, 6.0, Color32::WHITE, ink);
            pen.line(&[(14.5, 14.5), (20.0, 20.0)], ink);
        }
        "perspective" => {
            pen.closed(&[(4.0, 6.0), (20.0, 4.0), (20.0, 20.0), (4.0, 18.0)], ink);
            pen.closed(&[(9.0, 9.0), (15.0, 8.5), (15.0, 15.5), (9.0, 15.0)], ink);
        }
        "eye" => {
            pen.arc(12.0, 18.0, 10.0, 0.6, 2.54, ink);
            pen.arc(12.0, 6.0, 10.0, 3.74, 5.68, ink);
            pen.circle(12.0, 12.0, 2.6, ink, ink);
        }
        "body" => pen.iso_box(4.0, 10.0, 8.0, 6.0, 6.0),
        // Component: a box with a smaller one stacked on it.
        "component" => {
            pen.iso_box(2.0, 13.0, 9.0, 6.0, 6.0);
            pen.iso_box(9.0, 6.0, 5.0, 4.0, 4.0);
        }
        "folder" => pen.poly(&[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (21.0, 9.0), (21.0, 19.0), (3.0, 19.0)], Color32::from_rgb(236, 200, 110), ink),
        "origin" => {
            pen.arrow((6.0, 18.0), (20.0, 18.0), Color32::from_rgb(210, 60, 60));
            pen.arrow((6.0, 18.0), (6.0, 4.0), Color32::from_rgb(60, 100, 220));
            pen.arrow((6.0, 18.0), (15.0, 9.0), Color32::from_rgb(60, 160, 60));
        }
        "plane" => pen.poly(&[(3.0, 15.0), (12.0, 8.0), (21.0, 11.0), (12.0, 18.0)], Color32::from_rgb(244, 214, 150), ink),
        "axis" => {
            pen.line(&[(4.0, 20.0), (20.0, 4.0)], ink);
            pen.circle(4.0, 20.0, 2.0, ink, ink);
        }
        "settings" => {
            pen.circle(12.0, 12.0, 6.5, pen.fill, ink);
            pen.circle(12.0, 12.0, 2.5, Color32::WHITE, ink);
            for i in 0..8 {
                let t = i as f32 / 8.0 * std::f32::consts::TAU;
                pen.line(&[(12.0 + 6.5 * t.cos(), 12.0 + 6.5 * t.sin()), (12.0 + 9.0 * t.cos(), 12.0 + 9.0 * t.sin())], ink);
            }
        }
        // Sketch tools (sketch-style: thin outlines, accent for what the tool makes).
        "canvas" | "decal" => {
            if name == "canvas" {
                pen.poly(&[(3.0, 5.0), (21.0, 5.0), (21.0, 19.0), (3.0, 19.0)], pen.fill, ink);
            } else {
                pen.poly(&[(3.0, 5.0), (21.0, 5.0), (21.0, 13.0), (15.0, 19.0), (3.0, 19.0)], pen.fill, ink);
                pen.line(&[(21.0, 13.0), (15.0, 13.0), (15.0, 19.0)], ink);
            }
            pen.line(&[(5.0, 17.0), (9.5, 10.5), (12.5, 14.5), (14.5, 12.0), (18.0, 16.5)], a);
            pen.circle(16.5, 8.5, 1.6, a, a);
        }
        "dxf" | "svg" => {
            pen.poly(&[(5.0, 2.5), (15.0, 2.5), (19.5, 7.0), (19.5, 21.5), (5.0, 21.5)], pen.fill, ink);
            pen.line(&[(15.0, 2.5), (15.0, 7.0), (19.5, 7.0)], ink);
            let label = if name == "dxf" { "DXF" } else { "SVG" };
            p.text(pen.at(12.2, 15.0), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(6.4 * pen.s()), a);
        }
        "c_smooth" => {
            let pts: Vec<(f32, f32)> =
                (0..=24).map(|i| i as f32 / 24.0).map(|t| (3.0 + 18.0 * t, 12.0 - 7.0 * (t * std::f32::consts::TAU).sin())).collect();
            let (l, r) = pts.split_at(13);
            pen.line(l, ink);
            let mut r2 = vec![pts[12]];
            r2.extend_from_slice(r);
            pen.line(&r2, a);
            pen.dot(12.0, 12.0, ink);
        }
        "c_polygon" => {
            let pts: Vec<(f32, f32)> =
                (0..6).map(|i| i as f32 / 6.0 * std::f32::consts::TAU).map(|t| (12.0 + 8.5 * t.cos(), 12.0 + 8.5 * t.sin())).collect();
            pen.closed(&pts, a);
            pen.line(&[(12.0, 12.0), (20.5, 12.0)], ink);
            pen.dot(12.0, 12.0, ink);
        }
        "auto_constrain" | "constrainer" => {
            pen.closed(&[(3.0, 8.0), (15.0, 8.0), (15.0, 20.0), (3.0, 20.0)], ink);
            if name == "auto_constrain" {
                for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0), (0.7, 0.7), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7)] {
                    let k = if dx * dy == 0.0 { 4.0 } else { 2.5 };
                    pen.line(&[(18.0, 6.0), (18.0 + k * dx, 6.0 + k * dy)], a);
                }
            } else {
                pen.line(&[(13.0, 6.0), (16.0, 9.0), (21.5, 2.5)], a);
            }
            pen.line(&[(6.0, 17.0), (12.0, 17.0)], a);
            pen.line(&[(6.0, 11.0), (6.0, 14.0)], a);
        }
        "project" | "project_surface" => {
            if name == "project" {
                pen.poly(&[(2.0, 18.0), (12.0, 14.0), (22.0, 18.0), (12.0, 22.0)], pen.fill, ink);
                pen.closed(&[(6.0, 18.0), (12.0, 15.6), (18.0, 18.0), (12.0, 20.4)], a);
            } else {
                pen.line(&[(2.0, 19.0), (7.0, 16.0), (12.0, 17.5), (17.0, 15.5), (22.0, 18.0)], ink);
                pen.line(&[(6.0, 19.5), (12.0, 20.5), (18.0, 18.5)], a);
            }
            pen.iso_box(7.0, 5.0, 5.0, 5.0, 4.0);
            pen.arrow((12.0, 10.5), (12.0, 15.0), a);
        }
        "intersect" | "intersection_curve" => {
            pen.iso_box(6.0, 6.0, 7.0, 5.0, 11.0);
            if name == "intersect" {
                pen.line(&[(2.0, 13.0), (14.0, 8.0), (22.0, 12.0)], ink.gamma_multiply(0.6));
                pen.line(&[(6.0, 13.0), (11.0, 15.5), (18.0, 12.0)], a);
            } else {
                pen.arc(12.0, 26.0, 13.0, 0.6, 2.55, ink.gamma_multiply(0.6));
                pen.line(&[(6.0, 14.5), (11.0, 16.5), (18.0, 13.5)], a);
            }
        }
        "include" => {
            pen.iso_box(4.0, 9.0, 10.0, 7.0, 9.0);
            pen.line(&[(4.0, 9.0), (11.0, 12.5), (21.0, 7.5)], a);
            pen.dot(11.0, 12.5, a);
        }
        "spun" => {
            pen.line(&[(12.0, 2.0), (12.0, 22.0)], ink);
            pen.line(&[(14.0, 5.0), (18.0, 9.0), (18.0, 15.0), (14.0, 19.0)], ink);
            pen.line(&[(10.0, 5.0), (6.0, 9.0), (6.0, 15.0), (10.0, 19.0)], a);
            pen.arc(12.0, 12.0, 6.0, 0.3, 2.8, ink.gamma_multiply(0.6));
        }
        "iso_curve" => {
            pen.poly(&[(3.0, 16.0), (10.0, 6.0), (21.0, 8.0), (14.0, 19.0)], pen.fill, ink);
            pen.line(&[(6.5, 11.0), (17.5, 13.5)], ink.gamma_multiply(0.5));
            pen.line(&[(7.7, 17.3), (14.8, 6.8)], a);
        }
        "fit_section" => {
            pen.closed(&[(3.0, 18.0), (8.0, 6.0), (14.0, 15.0), (19.0, 5.0), (21.0, 18.0)], ink.gamma_multiply(0.6));
            pen.line(&[(8.0, 6.0), (3.0, 18.0)], ink.gamma_multiply(0.6));
            pen.line(&[(14.0, 15.0), (3.0, 18.0)], ink.gamma_multiply(0.6));
            pen.line(&[(14.0, 15.0), (21.0, 18.0)], ink.gamma_multiply(0.6));
            pen.line(&[(2.0, 12.0), (7.0, 10.5), (12.0, 12.5), (17.0, 10.0), (22.0, 11.5)], a);
        }
        "sketch_mirror" => {
            p.add(Shape::dashed_line(&[pen.at(12.0, 2.0), pen.at(12.0, 22.0)], Stroke::new(pen.w, ink), 2.5 * pen.s(), 1.8 * pen.s()));
            pen.closed(&[(3.0, 6.0), (9.0, 10.0), (9.0, 18.0), (3.0, 18.0)], ink);
            pen.closed(&[(21.0, 6.0), (15.0, 10.0), (15.0, 18.0), (21.0, 18.0)], a);
        }
        "sketch_circ_pattern" => paint(p, r, "pattern_circ", ink, Color32::TRANSPARENT, a),
        "sketch_rect_pattern" => paint(p, r, "pattern_rect", ink, Color32::TRANSPARENT, a),
        "line_mid" => {
            pen.line(&[(3.0, 19.0), (21.0, 5.0)], a);
            pen.dot(12.0, 12.0, ink);
            pen.dot(21.0, 5.0, ink);
            pen.circle(12.0, 12.0, 3.0, Color32::TRANSPARENT, ink);
        }
        "arc_tangent" => {
            pen.line(&[(2.0, 18.0), (11.0, 18.0)], ink);
            pen.arc(11.0, 10.0, 8.0, -std::f32::consts::FRAC_PI_2, 0.6, a);
            pen.dot(11.0, 18.0, ink);
        }
        "circle_tt" | "circle_ttt" => {
            if name == "circle_ttt" {
                pen.closed(&[(3.0, 20.0), (21.0, 20.0), (12.0, 4.0)], ink);
            } else {
                pen.line(&[(21.0, 20.0), (3.0, 20.0), (12.0, 4.0)], ink);
            }
            pen.circle(12.0, 14.74, 5.26, Color32::TRANSPARENT, a);
        }
        "arc_slot" => {
            let (t0, t1) = (0.75_f32, 2.4_f32);
            pen.arc(12.0, 21.0, 13.0, t0, t1, a);
            pen.arc(12.0, 21.0, 7.0, t0, t1, a);
            pen.arc(12.0 + 10.0 * t0.cos(), 21.0 - 10.0 * t0.sin(), 3.0, t0, t0 - std::f32::consts::PI, a);
            pen.arc(12.0 + 10.0 * t1.cos(), 21.0 - 10.0 * t1.sin(), 3.0, t1, t1 + std::f32::consts::PI, a);
            pen.dot(12.0, 21.0, ink);
        }
        "ellipse" => {
            let pts: Vec<(f32, f32)> =
                (0..40).map(|i| i as f32 / 40.0 * std::f32::consts::TAU).map(|t| (12.0 + 9.5 * t.cos(), 12.0 + 5.5 * t.sin())).collect();
            pen.closed(&pts, a);
            pen.dot(12.0, 12.0, ink);
            pen.line(&[(12.0, 12.0), (21.5, 12.0)], ink);
        }
        "spline" | "spline_cv" => {
            let c = [(3.0_f32, 18.0_f32), (8.0, 3.0), (16.0, 21.0), (21.0, 6.0)];
            let pts: Vec<(f32, f32)> = (0..=24)
                .map(|i| i as f32 / 24.0)
                .map(|t| {
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    (w.iter().zip(&c).map(|(w, q)| w * q.0).sum(), w.iter().zip(&c).map(|(w, q)| w * q.1).sum())
                })
                .collect();
            if name == "spline_cv" {
                pen.line(&c, ink.gamma_multiply(0.6));
                for q in c {
                    pen.dot(q.0, q.1, ink);
                }
            } else {
                for i in [0, 8, 16, 24] {
                    if let Some(q) = pts.get(i) {
                        pen.dot(q.0, q.1, ink);
                    }
                }
            }
            pen.line(&pts, a);
        }
        "conic" => {
            pen.line(&[(3.0, 20.0), (12.0, 3.0), (21.0, 20.0)], ink.gamma_multiply(0.6));
            let pts: Vec<(f32, f32)> =
                (0..=20).map(|i| i as f32 / 20.0).map(|t| (3.0 + 18.0 * t, 20.0 - 4.0 * 0.78 * 17.0 * t * (1.0 - t))).collect();
            pen.line(&pts, a);
            pen.dot(3.0, 20.0, ink);
            pen.dot(21.0, 20.0, ink);
        }
        "text" => {
            p.text(pen.at(12.0, 11.5), egui::Align2::CENTER_CENTER, "A", egui::FontId::proportional(17.0 * pen.s()), a);
            pen.line(&[(4.0, 20.5), (20.0, 20.5)], ink);
        }
        "curvature_comb" => {
            let f = |x: f32| 16.0 - 6.0 * ((x - 3.0) / 18.0 * std::f32::consts::PI).sin();
            let mut env = Vec::new();
            for i in 0..=8 {
                let x = 3.0 + 18.0 * i as f32 / 8.0;
                let k = 1.0 + 5.0 * ((x - 3.0) / 18.0 * std::f32::consts::PI).sin();
                pen.line(&[(x, f(x)), (x, f(x) - k)], a.gamma_multiply(0.7));
                env.push((x, f(x) - k));
            }
            pen.line(&env, a);
            let pts: Vec<(f32, f32)> = (0..=24).map(|i| 3.0 + 18.0 * i as f32 / 24.0).map(|x| (x, f(x))).collect();
            pen.line(&pts, ink);
        }
        "min_radius" => {
            pen.line(&[(3.0, 20.0), (7.0, 9.0), (12.0, 5.5), (17.0, 9.0), (21.0, 20.0)], ink);
            pen.circle(12.0, 10.5, 5.0, Color32::TRANSPARENT, a);
            pen.arrow((12.0, 10.5), (15.5, 7.0), a);
            pen.dot(12.0, 10.5, ink);
        }
        "iso_analysis" => {
            pen.poly(&[(3.0, 17.0), (9.0, 5.0), (21.0, 7.0), (15.0, 19.0)], pen.fill, ink);
            for k in 1..4 {
                let t = k as f32 / 4.0;
                pen.line(&[(3.0 + 6.0 * t, 17.0 - 12.0 * t), (15.0 + 6.0 * t, 19.0 - 12.0 * t)], a);
                pen.line(&[(3.0 + 12.0 * t, 17.0 + 2.0 * t), (9.0 + 12.0 * t, 5.0 + 2.0 * t)], a);
            }
        }
        "center_of_mass" => {
            pen.circle(12.0, 12.0, 8.0, Color32::TRANSPARENT, ink);
            let q = |a0: f32| {
                let pts: Vec<Pos2> = std::iter::once(pen.at(12.0, 12.0))
                    .chain(
                        (0..=8)
                            .map(|i| a0 + i as f32 / 8.0 * std::f32::consts::FRAC_PI_2)
                            .map(|t| pen.at(12.0 + 8.0 * t.cos(), 12.0 - 8.0 * t.sin())),
                    )
                    .collect();
                p.add(Shape::convex_polygon(pts, a, Stroke::NONE));
            };
            q(0.0);
            q(std::f32::consts::PI);
            pen.circle(12.0, 12.0, 8.0, Color32::TRANSPARENT, ink);
        }
        "sketch_fillet" => {
            pen.line(&[(4.0, 21.0), (4.0, 12.0)], ink);
            pen.line(&[(12.0, 4.0), (21.0, 4.0)], ink);
            pen.arc(12.0, 12.0, 8.0, std::f32::consts::FRAC_PI_2, std::f32::consts::PI, a);
            p.add(Shape::dashed_line(
                &[pen.at(4.0, 12.0), pen.at(4.0, 4.0), pen.at(12.0, 4.0)],
                Stroke::new(pen.w * 0.7, ink.gamma_multiply(0.5)),
                2.0 * pen.s(),
                1.5 * pen.s(),
            ));
        }
        "sketch_chamfer" => {
            pen.line(&[(4.0, 21.0), (4.0, 12.0)], ink);
            pen.line(&[(12.0, 4.0), (21.0, 4.0)], ink);
            pen.line(&[(4.0, 12.0), (12.0, 4.0)], a);
            p.add(Shape::dashed_line(
                &[pen.at(4.0, 12.0), pen.at(4.0, 4.0), pen.at(12.0, 4.0)],
                Stroke::new(pen.w * 0.7, ink.gamma_multiply(0.5)),
                2.0 * pen.s(),
                1.5 * pen.s(),
            ));
        }
        "offset" => {
            pen.closed(&[(8.0, 9.0), (16.0, 9.0), (16.0, 15.0), (8.0, 15.0)], ink);
            pen.line(&[(8.0, 4.0), (16.0, 4.0)], a);
            pen.line(&[(21.0, 9.0), (21.0, 15.0)], a);
            pen.line(&[(16.0, 20.0), (8.0, 20.0)], a);
            pen.line(&[(3.0, 15.0), (3.0, 9.0)], a);
            pen.arc(16.0, 9.0, 5.0, 0.0, std::f32::consts::FRAC_PI_2, a);
            pen.arc(16.0, 15.0, 5.0, -std::f32::consts::FRAC_PI_2, 0.0, a);
            pen.arc(8.0, 15.0, 5.0, std::f32::consts::PI, std::f32::consts::PI * 1.5, a);
            pen.arc(8.0, 9.0, 5.0, std::f32::consts::FRAC_PI_2, std::f32::consts::PI, a);
        }
        "trim" => {
            pen.line(&[(12.0, 3.0), (12.0, 21.0)], ink);
            pen.line(&[(3.0, 12.0), (12.0, 12.0)], ink);
            p.add(Shape::dashed_line(&[pen.at(12.0, 12.0), pen.at(21.0, 12.0)], Stroke::new(pen.w, a), 2.0 * pen.s(), 1.6 * pen.s()));
            pen.line(&[(15.0, 15.0), (19.0, 19.0)], a);
            pen.line(&[(19.0, 15.0), (15.0, 19.0)], a);
        }
        "extend" => {
            pen.line(&[(20.0, 3.0), (20.0, 21.0)], ink);
            pen.line(&[(3.0, 12.0), (10.0, 12.0)], ink);
            pen.dot(10.0, 12.0, ink);
            pen.arrow((10.0, 12.0), (19.0, 12.0), a);
        }
        "break" => {
            pen.line(&[(3.0, 17.0), (10.5, 11.0)], ink);
            pen.line(&[(13.5, 10.0), (21.0, 5.0)], a);
            pen.dot(12.0, 10.5, a);
            pen.line(&[(9.0, 5.0), (15.0, 16.0)], ink.gamma_multiply(0.5));
        }
        "scale" => {
            pen.closed(&[(3.0, 13.0), (11.0, 13.0), (11.0, 21.0), (3.0, 21.0)], ink);
            pen.closed(&[(3.0, 3.0), (21.0, 3.0), (21.0, 21.0), (3.0, 21.0)], a);
            pen.dot(3.0, 21.0, ink);
            pen.arrow((11.0, 13.0), (18.0, 6.0), a);
        }
        "blend" => {
            pen.line(&[(2.0, 19.0), (8.0, 15.0)], ink);
            pen.line(&[(16.0, 9.0), (22.0, 5.0)], ink);
            let c = [(8.0_f32, 15.0_f32), (12.5, 12.0), (11.5, 12.0), (16.0, 9.0)];
            let pts: Vec<(f32, f32)> = (0..=16)
                .map(|i| i as f32 / 16.0)
                .map(|t| {
                    let u = 1.0 - t;
                    let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                    (w.iter().zip(&c).map(|(w, q)| w * q.0).sum(), w.iter().zip(&c).map(|(w, q)| w * q.1).sum())
                })
                .collect();
            pen.line(&pts, a);
            pen.dot(8.0, 15.0, ink);
            pen.dot(16.0, 9.0, ink);
        }
        // Assembly: joints (accent: the joint itself).
        "joint" => {
            pen.iso_box(2.0, 13.0, 7.0, 5.0, 5.0);
            pen.iso_box(12.0, 4.0, 7.0, 5.0, 5.0);
            pen.line(&[(9.0, 15.0), (15.0, 9.0)], a);
            pen.circle(12.0, 12.0, 2.6, pen.fill, a);
        }
        "as_built" => {
            pen.iso_box(1.0, 10.0, 8.0, 5.0, 8.0);
            pen.iso_box(10.0, 10.0, 8.0, 5.0, 8.0);
            pen.circle(13.0, 15.0, 2.6, pen.fill, a);
        }
        "joint_origin" => {
            pen.arrow((8.0, 17.0), (20.0, 17.0), ink);
            pen.arrow((8.0, 17.0), (14.0, 11.0), ink);
            pen.arrow((8.0, 17.0), (8.0, 3.0), a);
            pen.circle(8.0, 17.0, 2.4, pen.fill, a);
        }
        "rigid_group" => {
            pen.iso_box(3.0, 13.0, 5.0, 3.0, 4.0);
            pen.iso_box(13.0, 13.0, 5.0, 3.0, 4.0);
            pen.iso_box(8.0, 4.0, 5.0, 3.0, 4.0);
            pen.closed(&[(2.0, 3.0), (22.0, 3.0), (22.0, 21.0), (2.0, 21.0)], a);
        }
        "drive" => {
            pen.circle(12.0, 12.0, 4.0, pen.fill, ink);
            pen.dot(12.0, 12.0, ink);
            pen.arc(12.0, 12.0, 8.5, 0.3, 4.6, a);
            pen.arrow((11.0, 20.4), (14.5, 20.3), a);
        }
        "motion_link" => {
            pen.circle(7.0, 14.0, 4.5, pen.fill, ink);
            pen.circle(17.5, 9.0, 3.0, pen.fill, ink);
            pen.line(&[(7.0, 14.0), (17.5, 9.0)], a);
            pen.dot(7.0, 14.0, a);
            pen.dot(17.5, 9.0, a);
        }
        "interference" => {
            pen.closed(&[(3.0, 5.0), (14.0, 5.0), (14.0, 16.0), (3.0, 16.0)], ink);
            pen.poly(&[(9.0, 10.0), (14.0, 10.0), (14.0, 16.0), (9.0, 16.0)], Color32::from_rgb(225, 70, 60), Color32::from_rgb(225, 70, 60));
            pen.closed(&[(9.0, 10.0), (21.0, 10.0), (21.0, 20.0), (9.0, 20.0)], ink);
        }
        // Sheet metal: thin plates (fill), the made or changed part in the accent.
        "flange" => {
            pen.poly(&[(2.0, 17.0), (12.0, 13.0), (18.0, 16.0), (8.0, 20.0)], pen.fill, ink);
            pen.poly(&[(12.0, 13.0), (12.0, 5.0), (18.0, 8.0), (18.0, 16.0)], pen.fill.gamma_multiply(0.8), a);
            pen.arrow((20.5, 15.0), (20.5, 4.0), a);
        }
        "hem" => {
            pen.line(&[(2.0, 16.0), (16.0, 16.0)], ink);
            pen.line(&[(2.0, 13.0), (16.0, 13.0)], ink);
            pen.arc(16.0, 11.5, 4.5, -1.57, 1.57, a);
            pen.line(&[(16.0, 7.0), (8.0, 7.0)], a);
        }
        "unfold" => {
            pen.line(&[(3.0, 18.0), (12.0, 18.0), (12.0, 7.0)], ink);
            pen.line(&[(12.0, 18.0), (22.0, 18.0)], a);
            pen.arc(12.0, 18.0, 7.0, 0.3, 1.4, a);
            pen.arrow((17.5, 14.0), (19.0, 16.0), a);
        }
        "refold" => {
            pen.line(&[(3.0, 18.0), (12.0, 18.0), (21.0, 18.0)], ink);
            pen.line(&[(12.0, 18.0), (12.0, 6.0)], a);
            pen.arc(12.0, 18.0, 7.0, 0.3, 1.4, a);
            pen.arrow((15.0, 11.5), (13.5, 10.5), a);
        }
        "flat" => {
            pen.poly(&[(3.0, 6.0), (21.0, 6.0), (21.0, 18.0), (3.0, 18.0)], pen.fill, ink);
            for x in [9.0, 15.0] {
                for k in 0..3 {
                    let y = 7.5 + k as f32 * 4.0;
                    pen.line(&[(x, y), (x, y + 2.2)], a);
                }
            }
        }
        "convert_sheet" => {
            pen.iso_box(2.0, 9.0, 6.0, 4.0, 6.0);
            pen.arrow((11.0, 12.0), (15.0, 12.0), a);
            pen.poly(&[(15.0, 15.0), (21.0, 12.0), (23.0, 13.0), (17.0, 16.0)], pen.fill, a);
        }
        "sheet_rules" => {
            pen.poly(&[(2.0, 15.0), (11.0, 11.0), (17.0, 14.0), (8.0, 18.0)], pen.fill, ink);
            for y in [5.0, 9.0, 13.0] {
                pen.line(&[(14.0, y), (22.0, y)], a);
            }
        }
        // Plastic: a wall or plate (fill) with the feature in the accent.
        "boss" => {
            pen.poly(&[(2.0, 16.0), (13.0, 12.0), (22.0, 16.0), (11.0, 20.0)], pen.fill, ink);
            pen.poly(&[(9.0, 7.0), (15.0, 7.0), (15.0, 16.0), (9.0, 16.0)], pen.fill.gamma_multiply(0.85), a);
            pen.circle(12.0, 7.0, 3.0, pen.fill, a);
            pen.circle(12.0, 7.0, 1.2, ink, ink);
        }
        "lip" => {
            pen.poly(&[(4.0, 21.0), (4.0, 9.0), (10.0, 9.0), (10.0, 21.0)], pen.fill, ink);
            pen.poly(&[(4.0, 9.0), (4.0, 4.0), (7.0, 4.0), (7.0, 9.0)], pen.fill, a);
            pen.poly(&[(14.0, 21.0), (14.0, 6.0), (20.0, 6.0), (20.0, 21.0)], pen.fill, ink);
            pen.closed(&[(17.0, 6.0), (17.0, 11.0), (20.0, 11.0), (20.0, 6.0)], a);
        }
        "snap" => {
            pen.poly(&[(2.0, 20.0), (2.0, 16.0), (22.0, 16.0), (22.0, 20.0)], pen.fill, ink);
            pen.poly(&[(9.0, 16.0), (9.0, 4.0), (12.0, 4.0), (12.0, 16.0)], pen.fill, a);
            pen.poly(&[(12.0, 4.0), (16.0, 8.0), (12.0, 8.0)], a, a);
        }
        "rest" => {
            pen.poly(&[(2.0, 16.0), (13.0, 12.0), (22.0, 16.0), (11.0, 20.0)], pen.fill, ink);
            pen.poly(&[(7.0, 14.0), (13.0, 11.5), (17.0, 13.5), (11.0, 16.0)], a, a);
            pen.line(&[(7.0, 14.0), (7.0, 11.5), (13.0, 9.0), (17.0, 11.0), (17.0, 13.5)], a);
        }
        "plastic_rule" => {
            pen.circle(7.0, 8.0, 3.2, pen.fill, a);
            pen.circle(11.0, 13.0, 3.2, pen.fill, a);
            pen.circle(5.5, 16.0, 3.2, pen.fill, a);
            for y in [6.0, 11.0, 16.0] {
                pen.line(&[(15.0, y), (22.0, y)], ink);
            }
        }
        "plastic_assign" => {
            pen.iso_box(2.0, 12.0, 7.0, 5.0, 6.0);
            pen.arrow((19.0, 6.0), (13.0, 11.0), a);
            pen.circle(19.0, 6.0, 3.0, pen.fill, a);
        }
        "warning" => {
            pen.poly(&[(12.0, 3.0), (21.0, 20.0), (3.0, 20.0)], Color32::from_rgb(250, 200, 60), ink);
            pen.line(&[(12.0, 9.0), (12.0, 14.0)], ink);
            pen.dot(12.0, 17.0, ink);
        }
        _ => {
            pen.poly(&[(5.0, 5.0), (19.0, 5.0), (19.0, 19.0), (5.0, 19.0)], pen.fill, ink);
        }
    }
}
