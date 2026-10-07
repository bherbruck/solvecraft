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
