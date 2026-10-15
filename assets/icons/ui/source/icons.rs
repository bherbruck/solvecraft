//! Toolbar and browser icons, drawn in code (no image assets). Each icon is designed on a
//! 24 × 24 grid and scaled to the target rectangle.

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// `n + 1` points along the cubic Bézier curve with control points `c`.
fn bezier(c: [(f32, f32); 4], n: usize) -> Vec<(f32, f32)> {
    (0..=n)
        .map(|i| i as f32 / n as f32)
        .map(|t| {
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            (w.iter().zip(&c).map(|(w, q)| w * q.0).sum(), w.iter().zip(&c).map(|(w, q)| w * q.1).sum())
        })
        .collect()
}

/// `n + 1` points along an ellipse from angle `a0` to `a1` (y down: angles below the centre
/// first).
fn ellipse(x: f32, y: f32, rx: f32, ry: f32, a0: f32, a1: f32, n: usize) -> Vec<(f32, f32)> {
    (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|a| (x + rx * a.cos(), y + ry * a.sin())).collect()
}

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
    /// A cubic Bézier curve through control points `c`.
    fn curve(&self, c: [(f32, f32); 4], col: Color32) {
        self.line(&bezier(c, 16), col);
    }
    fn dashed(&self, pts: &[(f32, f32)], c: Color32) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.at(*x, *y)).collect();
        self.p.add(Shape::dashed_line(&v, Stroke::new(self.w, c), 2.0 * self.s(), 1.6 * self.s()));
    }
    /// Fill the strip between two polylines that run the same way, such as the front and back
    /// edges of a curved sheet. Unlike `poly`, the strip need not be convex.
    fn band(&self, a: &[(f32, f32)], b: &[(f32, f32)], fill: Color32) {
        let mut m = Mesh::default();
        for (p, q) in a.iter().zip(b) {
            m.colored_vertex(self.at(p.0, p.1), fill);
            m.colored_vertex(self.at(q.0, q.1), fill);
        }
        let n = a.len().min(b.len()) as u32;
        for i in 1..n {
            let k = 2 * i;
            m.add_triangle(k - 2, k - 1, k);
            m.add_triangle(k - 1, k + 1, k);
        }
        self.p.add(Shape::mesh(m));
    }
}

/// The stroke width for the Normal/Construction/Centerline line-type icons. It is heavier than
/// the generic icon weight (`Pen::w`). It is sized from whichever side of `r` is smaller. A
/// short, wide button and a square toolbar slot both get a stroke their gaps can survive next
/// to.
fn line_type_stroke(r: Rect) -> f32 {
    (r.width().min(r.height()) / 24.0 * 2.0).max(1.6)
}

/// The Centerline icon's dot radius. It is sized from `r`. The stroke width plays no part in it.
/// It stays a round mark even next to a heavy stroke.
fn line_type_dot_radius(r: Rect) -> f32 {
    (r.width().min(r.height()) * 0.16).max(2.4)
}

/// Paint the icon `name` into `r`. `accent` highlights the "action" part.
pub fn paint(p: &Painter, r: Rect, name: &str, ink: Color32, fill: Color32, accent: Color32) {
    paint_with(p, r, name, ink, fill, accent, &crate::theme::Tokens::get().icons);
}

/// [`paint`] with the colours an icon draws of its own given (`theme::IconColors`).
pub(crate) fn paint_with(p: &Painter, r: Rect, name: &str, ink: Color32, fill: Color32, accent: Color32, ic: &crate::theme::IconColors) {
    let ic = *ic;
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
            p.circle_filled(pen.at(9.0, 8.5), 2.0 * pen.s(), ic.glint);
        }
        "torus" => {
            let pts: Vec<Pos2> =
                (0..40).map(|i| i as f32 / 40.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 9.0 * t.cos(), 12.0 + 5.5 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, pen.fill, Stroke::new(pen.w, ink)));
            let pts: Vec<Pos2> =
                (0..40).map(|i| i as f32 / 40.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 3.5 * t.cos(), 11.0 + 1.8 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, ic.highlight, Stroke::new(pen.w, ink)));
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
            p.add(Shape::convex_polygon(pts, ic.highlight, Stroke::new(pen.w, a)));
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
            pen.poly(&[(6.0, 9.0), (13.0, 5.5), (18.0, 8.0), (11.0, 11.5)], ic.highlight, a);
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
        // Original bug glyph: rounded body, antennae and three pairs of legs.
        "bug_report" => {
            pen.circle(12.0, 13.5, 5.0, Color32::TRANSPARENT, ink);
            pen.line(&[(12.0, 9.0), (12.0, 18.0)], ink);
            pen.line(&[(9.5, 8.5), (8.0, 5.0)], ink);
            pen.line(&[(14.5, 8.5), (16.0, 5.0)], ink);
            for y in [10.0, 13.5, 17.0] {
                pen.line(&[(3.0, y), (7.0, y)], ink);
                pen.line(&[(17.0, y), (21.0, y)], ink);
            }
        }
        // Update: an arrow down onto a tray (a newer release can be downloaded).
        "update" => {
            pen.line(&[(12.0, 3.5), (12.0, 14.5)], ink);
            pen.line(&[(7.5, 10.0), (12.0, 14.5), (16.5, 10.0)], ink);
            pen.line(&[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)], ink);
        }
        // Help: a question mark in a ring.
        "help" => {
            pen.circle(12.0, 12.0, 9.0, Color32::TRANSPARENT, ink);
            pen.arc(12.0, 9.5, 3.2, -0.6, 3.4, ink);
            pen.line(&[(13.9, 12.0), (12.0, 13.6), (12.0, 15.0)], ink);
            pen.dot(12.0, 17.8, ink);
        }
        // A clock: earlier conversations.
        "history" => {
            pen.circle(12.0, 12.0, 8.5, Color32::TRANSPARENT, ink);
            pen.line(&[(12.0, 6.5), (12.0, 12.0), (16.0, 14.5)], a);
        }
        // A camera: attach the current view.
        "view_attach" => {
            pen.poly(&[(3.5, 8.0), (8.0, 8.0), (9.5, 5.5), (14.5, 5.5), (16.0, 8.0), (20.5, 8.0), (20.5, 19.0), (3.5, 19.0)], pen.fill, ink);
            pen.circle(12.0, 13.2, 3.6, Color32::TRANSPARENT, a);
        }
        // The assistant: a speech bubble with a spark.
        "assistant" => {
            pen.poly(&[(4.0, 5.0), (20.0, 5.0), (20.0, 16.0), (11.5, 16.0), (7.0, 20.0), (7.0, 16.0), (4.0, 16.0)], pen.fill, ink);
            pen.line(&[(12.0, 7.5), (12.0, 13.5)], a);
            pen.line(&[(9.0, 10.5), (15.0, 10.5)], a);
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
        "configuration_table" => {
            pen.poly(&[(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)], pen.fill, ink);
            pen.line(&[(3.0, 9.0), (21.0, 9.0)], a);
            pen.line(&[(3.0, 14.0), (21.0, 14.0)], ink);
            pen.line(&[(9.0, 4.0), (9.0, 20.0)], ink);
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
        "rect_rounded" => {
            // Straight sides between quarter arcs of radius 3 at the corners.
            pen.line(&[(7.0, 6.0), (17.0, 6.0)], a);
            pen.line(&[(20.0, 9.0), (20.0, 15.0)], a);
            pen.line(&[(17.0, 18.0), (7.0, 18.0)], a);
            pen.line(&[(4.0, 15.0), (4.0, 9.0)], a);
            let q = std::f32::consts::FRAC_PI_2;
            pen.arc(17.0, 9.0, 3.0, 0.0, q, a);
            pen.arc(7.0, 9.0, 3.0, q, 2.0 * q, a);
            pen.arc(7.0, 15.0, 3.0, 2.0 * q, 3.0 * q, a);
            pen.arc(17.0, 15.0, 3.0, 3.0 * q, 4.0 * q, a);
            pen.dot(4.0, 18.0, ink);
            pen.dot(20.0, 6.0, ink);
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
        // The Sketch Palette's line type selector (normal, construction, centerline) paints all
        // three on the same diagonal, from (3, 19) to (19, 3). Only the dash pattern changes
        // between them. Normal draws in `ink` and ignores `accent`. It is a plain line. It is
        // never construction geometry. Its colour must stay fixed even if a caller passes the
        // construction orange for it. Construction and centerline draw only in `accent` and
        // ignore `ink`. The caller decides the colour. The palette and the toolbar both pass
        // the theme's construction orange.
        //
        // These three are drawn small. The Sketch Palette draws them down to about 18 px
        // square. At that size a repeating dash pattern loses its gaps. It reads as a solid
        // line. Two patterns that only differ in dash phase read as the same texture too.
        // Normal, Construction and Centerline instead get three different silhouettes. One is
        // an unbroken line. One is a line broken by a single gap. One is a line broken by a
        // single gap with a bold dot sitting in it. A viewer can name the shape on sight. No
        // rhythm needs measuring. The stroke width and the dot radius both come from the rect
        // actually passed in. That keeps the shapes legible in the palette, the toolbar and the
        // command palette, at every size any of them uses.
        "normal" => {
            let w = line_type_stroke(r);
            p.add(Shape::line(vec![pen.at(3.0, 19.0), pen.at(19.0, 3.0)], Stroke::new(w, ink)));
        }
        "construction" => {
            // One gap, no dot. It is an unmistakably broken line. Nothing sits in the break.
            let w = line_type_stroke(r);
            let at = |t: f32| pen.at(3.0 + 16.0 * t, 19.0 - 16.0 * t);
            for (t0, t1) in [(0.0, 0.35), (0.65, 1.0)] {
                p.add(Shape::line(vec![at(t0), at(t1)], Stroke::new(w, a)));
            }
        }
        "centerline" => {
            // The same gap as Construction, with a bold dot centred in it. The silhouette is
            // line, dot, line. The dot's radius comes from the rect. It reads as a distinct
            // round mark on its own, never as a thicker stretch of the stroke. The dashes stop
            // well short of it on both sides. The dot never touches either stroke. It always
            // reads as a separate shape.
            let w = line_type_stroke(r);
            let dot_r = line_type_dot_radius(r);
            let at = |t: f32| pen.at(3.0 + 16.0 * t, 19.0 - 16.0 * t);
            for (t0, t1) in [(0.0, 0.26), (0.74, 1.0)] {
                p.add(Shape::line(vec![at(t0), at(t1)], Stroke::new(w, a)));
            }
            p.circle_filled(at(0.5), dot_r, a);
        }
        "c_hv" => {
            pen.line(&[(4.0, 18.0), (20.0, 18.0)], a);
            pen.line(&[(6.0, 4.0), (6.0, 14.0)], a);
        }
        "c_coincident" => {
            pen.line(&[(3.0, 18.0), (12.0, 12.0)], ink);
            pen.line(&[(12.0, 12.0), (21.0, 16.0)], ink);
            pen.circle(12.0, 12.0, 2.6, ic.highlight, a);
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
            pen.poly(&[(6.0, 3.0), (15.0, 3.0), (19.0, 7.0), (19.0, 21.0), (6.0, 21.0)], ic.highlight, ink);
            pen.line(&[(15.0, 3.0), (15.0, 7.0), (19.0, 7.0)], ink);
        }
        "open" => {
            pen.poly(&[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (20.0, 9.0), (20.0, 19.0), (3.0, 19.0)], pen.fill, ink);
        }
        "save" => {
            pen.poly(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)], pen.fill, ink);
            pen.poly(&[(8.0, 4.0), (16.0, 4.0), (16.0, 9.0), (8.0, 9.0)], ic.highlight, ink);
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
        "spin" => {
            // A turntable: an ellipse with an arrow running round it, a post in the middle.
            let pts: Vec<Pos2> = (0..=28).map(|i| 0.2 + i as f32 / 28.0 * 5.6).map(|t| pen.at(12.0 + 9.0 * t.cos(), 16.0 + 3.5 * t.sin())).collect();
            p.add(Shape::line(pts, Stroke::new(pen.w, ink)));
            pen.arrow((17.0, 19.0), (20.5, 17.0), a);
            pen.line(&[(12.0, 15.5), (12.0, 4.0)], ink);
            pen.poly(&[(9.0, 9.0), (15.0, 9.0), (15.0, 4.0), (9.0, 4.0)], Color32::TRANSPARENT, ink);
        }
        "pan" => {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                pen.arrow((12.0 + 3.0 * dx, 12.0 + 3.0 * dy), (12.0 + 9.0 * dx, 12.0 + 9.0 * dy), ink);
            }
        }
        "zoom" => {
            pen.circle(10.0, 10.0, 6.0, ic.highlight, ink);
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
        "folder" => pen.poly(&[(3.0, 7.0), (9.0, 7.0), (11.0, 9.0), (21.0, 9.0), (21.0, 19.0), (3.0, 19.0)], ic.folder, ink),
        "origin" => {
            pen.arrow((6.0, 18.0), (20.0, 18.0), ic.axes[0]);
            pen.arrow((6.0, 18.0), (6.0, 4.0), ic.axes[2]);
            pen.arrow((6.0, 18.0), (15.0, 9.0), ic.axes[1]);
        }
        "plane" => pen.poly(&[(3.0, 15.0), (12.0, 8.0), (21.0, 11.0), (12.0, 18.0)], ic.plane, ink),
        "axis" => {
            pen.line(&[(4.0, 20.0), (20.0, 4.0)], ink);
            pen.circle(4.0, 20.0, 2.0, ink, ink);
        }
        "settings" => {
            // A gear: a solid ring with six chunky, square-ish teeth and a see-through centre
            // hole (#290; thin rays read as a sun).
            let (teeth, root, tip, hole) = (6usize, 7.0f32, 10.5f32, 3.2f32);
            let pitch = std::f32::consts::TAU / teeth as f32;
            // Half-widths (radians) of a tooth at its root and at its tip: nearly parallel sides.
            let (half_root, half_tip) = (pitch * 0.3, pitch * 0.22);
            let polar = |a: f32, r: f32| (12.0 + r * a.cos(), 12.0 + r * a.sin());
            let start = -std::f32::consts::FRAC_PI_2;
            let mut outline = Vec::new();
            for i in 0..teeth {
                let c = start + i as f32 * pitch;
                // The tooth, filled; it reaches into the ring so no seam shows.
                let quad = [polar(c - half_root, root - 1.0), polar(c - half_tip, tip), polar(c + half_tip, tip), polar(c + half_root, root - 1.0)];
                let v: Vec<Pos2> = quad.iter().map(|(x, y)| pen.at(*x, *y)).collect();
                p.add(Shape::convex_polygon(v, pen.fill, Stroke::NONE));
                outline.extend([polar(c - half_root, root), polar(c - half_tip, tip), polar(c + half_tip, tip), polar(c + half_root, root)]);
                // The root circle up to the next tooth.
                for k in 1..4 {
                    let a = c + half_root + (pitch - 2.0 * half_root) * k as f32 / 4.0;
                    outline.push(polar(a, root));
                }
            }
            // The ring: a thick stroke, so the hole stays transparent on any background.
            p.circle_stroke(pen.at(12.0, 12.0), (root + hole) * 0.5 * pen.s(), Stroke::new((root - hole) * pen.s(), pen.fill));
            pen.closed(&outline, ink);
            pen.circle(12.0, 12.0, hole, Color32::TRANSPARENT, ink);
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
        "accessibility" => {
            pen.poly(&[(2.0, 20.0), (22.0, 20.0), (22.0, 15.0), (2.0, 15.0)], ic.access[0], ink);
            pen.poly(&[(8.0, 15.0), (16.0, 15.0), (16.0, 17.5), (8.0, 17.5)], ic.access[1], Color32::TRANSPARENT);
            pen.poly(&[(6.0, 10.0), (18.0, 10.0), (18.0, 12.5), (6.0, 12.5)], pen.fill, ink);
            pen.arrow((4.0, 2.0), (4.0, 13.0), a);
            pen.arrow((20.0, 2.0), (20.0, 13.0), a);
        }
        "environment_map" => {
            let pts: Vec<Pos2> =
                (0..32).map(|i| i as f32 / 32.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 9.0 * t.cos(), 12.0 + 9.0 * t.sin())).collect();
            p.add(Shape::convex_polygon(pts, ic.sky, Stroke::new(pen.w, ink)));
            pen.poly(&[(3.6, 13.0), (20.4, 13.0), (18.5, 17.5), (5.5, 17.5)], ic.floor, Color32::TRANSPARENT);
            pen.line(&[(3.2, 12.6), (20.8, 12.6)], ic.highlight);
            p.circle_filled(pen.at(9.0, 8.0), 2.2 * pen.s(), ic.highlight);
            pen.circle(12.0, 12.0, 9.0, Color32::TRANSPARENT, ink);
        }
        "zebra" | "draft_analysis" | "curvature_map" => {
            let pts: Vec<Pos2> =
                (0..32).map(|i| i as f32 / 32.0 * std::f32::consts::TAU).map(|t| pen.at(12.0 + 9.0 * t.cos(), 12.0 + 9.0 * t.sin())).collect();
            match name {
                "zebra" => {
                    p.add(Shape::convex_polygon(pts, ic.highlight, Stroke::new(pen.w, ink)));
                    for k in 0..3 {
                        let y = 6.5 + k as f32 * 4.5;
                        let h = (81.0 - (y + 1.0 - 12.0) * (y + 1.0 - 12.0)).max(0.0).sqrt();
                        pen.poly(&[(12.0 - h, y), (12.0 + h, y), (12.0 + h, y + 2.2), (12.0 - h, y + 2.2)], ic.stripe, Color32::TRANSPARENT);
                    }
                    pen.circle(12.0, 12.0, 9.0, Color32::TRANSPARENT, ink);
                }
                "draft_analysis" => {
                    pen.poly(&[(4.0, 20.0), (20.0, 20.0), (17.0, 5.0), (7.0, 5.0)], ic.draft[0], ink);
                    pen.poly(&[(7.0, 5.0), (17.0, 5.0), (16.0, 9.0), (8.0, 9.0)], ic.draft[1], Color32::TRANSPARENT);
                    pen.arrow((12.0, 16.0), (12.0, 1.5), a);
                }
                _ => {
                    p.add(Shape::convex_polygon(pts, ic.curvature[0], Stroke::new(pen.w, ink)));
                    pen.circle(14.0, 10.0, 5.0, ic.curvature[1], Color32::TRANSPARENT);
                    pen.circle(15.5, 8.5, 2.2, ic.curvature[2], Color32::TRANSPARENT);
                }
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
        // The three chamfer types, for the Chamfer Type row of the corner tool window. Each
        // draws the cut it makes: equal legs with a tick on each, two unequal legs, and one leg
        // with the angle the cut leaves it at.
        "sketch_chamfer_equal" => {
            pen.line(&[(4.0, 21.0), (4.0, 12.0)], ink);
            pen.line(&[(12.0, 4.0), (21.0, 4.0)], ink);
            pen.line(&[(4.0, 12.0), (12.0, 4.0)], a);
            pen.line(&[(2.5, 9.5), (5.5, 6.5)], a);
            pen.line(&[(9.5, 2.5), (6.5, 5.5)], a);
            p.add(Shape::dashed_line(
                &[pen.at(4.0, 12.0), pen.at(4.0, 4.0), pen.at(12.0, 4.0)],
                Stroke::new(pen.w * 0.7, ink.gamma_multiply(0.5)),
                2.0 * pen.s(),
                1.5 * pen.s(),
            ));
        }
        "sketch_chamfer_two" => {
            pen.line(&[(4.0, 21.0), (4.0, 10.0)], ink);
            pen.line(&[(18.0, 4.0), (21.0, 4.0)], ink);
            pen.line(&[(4.0, 10.0), (18.0, 4.0)], a);
            p.add(Shape::dashed_line(
                &[pen.at(4.0, 10.0), pen.at(4.0, 4.0), pen.at(18.0, 4.0)],
                Stroke::new(pen.w * 0.7, ink.gamma_multiply(0.5)),
                2.0 * pen.s(),
                1.5 * pen.s(),
            ));
        }
        "sketch_chamfer_angle" => {
            pen.line(&[(4.0, 21.0), (4.0, 13.0)], ink);
            pen.line(&[(14.0, 4.0), (21.0, 4.0)], ink);
            pen.line(&[(4.0, 13.0), (14.0, 4.0)], a);
            pen.arc(4.0, 13.0, 6.0, -std::f32::consts::FRAC_PI_2, 0.73, a);
            p.add(Shape::dashed_line(
                &[pen.at(4.0, 13.0), pen.at(4.0, 4.0), pen.at(14.0, 4.0)],
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
        // Offset mode icons, drawn with the house `dashed` helper. The source is the solid
        // line and each result a dashed one. One Side has a single result; Two Sides has one
        // each way at two different gaps; Symmetric has one each way at the same gap.
        "offset_one_side" => {
            pen.line(&[(8.0, 4.0), (8.0, 20.0)], ink);
            pen.dashed(&[(18.0, 4.0), (18.0, 20.0)], a);
        }
        "offset_two_sides" => {
            pen.line(&[(13.0, 4.0), (13.0, 20.0)], ink);
            pen.dashed(&[(3.0, 4.0), (3.0, 20.0)], a);
            pen.dashed(&[(20.0, 4.0), (20.0, 20.0)], a);
        }
        "offset_symmetric" => {
            pen.line(&[(12.0, 4.0), (12.0, 20.0)], ink);
            pen.dashed(&[(4.0, 4.0), (4.0, 20.0)], a);
            pen.dashed(&[(20.0, 4.0), (20.0, 20.0)], a);
        }
        "trim" => {
            pen.line(&[(12.0, 3.0), (12.0, 21.0)], ink);
            pen.line(&[(3.0, 12.0), (12.0, 12.0)], ink);
            p.add(Shape::dashed_line(&[pen.at(12.0, 12.0), pen.at(21.0, 12.0)], Stroke::new(pen.w, a), 2.0 * pen.s(), 1.6 * pen.s()));
            pen.line(&[(15.0, 15.0), (19.0, 19.0)], a);
            pen.line(&[(19.0, 15.0), (15.0, 19.0)], a);
        }
        "patch" => {
            // A wire loop with a surface spanned across it.
            pen.poly(&[(4.0, 16.0), (9.0, 6.0), (19.0, 5.0), (20.0, 15.0), (12.0, 20.0)], ic.sheet, ink);
            pen.line(&[(4.0, 16.0), (9.0, 6.0), (19.0, 5.0), (20.0, 15.0), (12.0, 20.0), (4.0, 16.0)], a);
        }
        "stitch" => {
            // Two surface patches sewn along their shared edge.
            pen.poly(&[(3.0, 18.0), (11.0, 14.0), (11.0, 4.0), (3.0, 8.0)], ic.sheet, ink);
            pen.poly(&[(13.0, 14.0), (21.0, 18.0), (21.0, 8.0), (13.0, 4.0)], ic.sheet_2, ink);
            for y in [6.0, 9.0, 12.0] {
                pen.line(&[(10.0, y), (14.0, y + 1.5)], a);
            }
        }
        "thicken" => {
            // A wavy sheet (accent edges) grown downward into a slab, shaded like `iso_box`.
            let f = [(2.0, 12.5), (7.0, 9.5), (12.0, 15.5), (17.0, 12.5)];
            let b = [(6.4, 8.9), (11.4, 5.9), (16.4, 11.9), (21.4, 8.9)];
            let d = f.map(|(x, y)| (x, y + 6.0));
            pen.band(&bezier(b, 16), &bezier(f, 16), ic.sheet);
            pen.band(&bezier(f, 16), &bezier(d, 16), pen.fill.gamma_multiply(0.85));
            pen.poly(&[(17.0, 12.5), (21.4, 8.9), (21.4, 14.9), (17.0, 18.5)], pen.fill.gamma_multiply(0.7), ink);
            pen.line(&[(2.0, 12.5), (2.0, 18.5)], ink);
            pen.line(&[(17.0, 12.5), (17.0, 18.5)], ink);
            pen.line(&[(21.4, 8.9), (21.4, 14.9)], ink);
            pen.curve(d, ink);
            pen.line(&[(6.4, 8.9), (2.0, 12.5)], ink);
            pen.line(&[(21.4, 8.9), (17.0, 12.5)], a);
            pen.curve(b, a);
            pen.curve(f, a);
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
            pen.curve([(8.0, 15.0), (12.5, 12.0), (11.5, 12.0), (16.0, 9.0)], a);
            pen.dot(8.0, 15.0, ink);
            pen.dot(16.0, 9.0, ink);
        }
        // Surface modelling: open sheets (no caps) in the sheet colours, the input or the result
        // in the accent.
        "surface_extrude" => {
            // An open profile (accent) pulled up into a sheet.
            let top = [(4.0, 11.0), (8.0, 8.0), (13.0, 14.0), (20.0, 10.0)];
            let bottom = [(4.0, 20.0), (8.0, 17.0), (13.0, 23.0), (20.0, 19.0)];
            pen.band(&bezier(top, 16), &bezier(bottom, 16), ic.sheet);
            pen.line(&[(4.0, 11.0), (4.0, 20.0)], ink);
            pen.line(&[(20.0, 10.0), (20.0, 19.0)], ink);
            pen.curve(top, ink);
            pen.curve(bottom, a);
            pen.arrow((12.0, 8.5), (12.0, 2.5), a);
        }
        "surface_revolve" => {
            // An open profile (accent) turned about the axis into a hollow band.
            let left = [(5.0, 6.0), (7.0, 10.0), (5.0, 14.0), (6.0, 18.0)];
            let right = [(19.0, 6.0), (17.0, 10.0), (19.0, 14.0), (18.0, 18.0)];
            pen.band(&bezier(left, 16), &bezier(right, 16), ic.sheet);
            let front = ellipse(12.0, 18.0, 6.0, 2.2, 0.0, std::f32::consts::PI, 16);
            pen.poly(&front, ic.sheet, Color32::TRANSPARENT);
            // A closed fill takes each point once: a repeated end point spikes `poly`'s edge.
            let mut opening = ellipse(12.0, 6.0, 7.0, 2.2, 0.0, std::f32::consts::TAU, 32);
            opening.pop();
            pen.poly(&opening, ic.sheet_light, ink);
            pen.line(&front, ink);
            pen.curve(left, ink);
            pen.line(&[(12.0, 1.5), (12.0, 3.8)], ink);
            pen.line(&[(12.0, 20.2), (12.0, 22.5)], ink);
            pen.curve(right, a);
        }
        "surface_sweep" => {
            // A straight profile (accent) carried along a curved path: a ribbon.
            let near = [(4.0, 13.0), (8.0, 5.0), (14.0, 5.0), (20.0, 9.0)];
            let far = [(4.0, 20.0), (8.0, 12.0), (14.0, 12.0), (20.0, 16.0)];
            pen.band(&bezier(near, 16), &bezier(far, 16), ic.sheet);
            pen.curve(near, ink);
            pen.curve(far, ink);
            pen.line(&[(4.0, 13.0), (4.0, 20.0)], a);
            pen.line(&[(20.0, 9.0), (20.0, 16.0)], ink);
        }
        "surface_loft" => {
            // Two open sections (accent) joined by a sheet.
            let top = [(5.0, 6.0), (9.0, 3.0), (14.0, 8.0), (19.0, 5.0)];
            let bottom = [(3.0, 19.0), (8.0, 15.0), (14.0, 22.0), (21.0, 18.0)];
            pen.band(&bezier(top, 16), &bezier(bottom, 16), ic.sheet);
            pen.line(&[(5.0, 6.0), (3.0, 19.0)], ink);
            pen.line(&[(19.0, 5.0), (21.0, 18.0)], ink);
            pen.curve(top, a);
            pen.curve(bottom, a);
        }
        "surface_offset" => {
            // A wavy sheet and its offset copy (dashed), the distance shown by the arrow.
            let back = [(6.0, 14.0), (10.0, 11.0), (15.0, 17.0), (21.0, 14.0)];
            let front = [(3.0, 20.0), (7.0, 17.0), (12.0, 23.0), (18.0, 20.0)];
            pen.band(&bezier(back, 16), &bezier(front, 16), ic.sheet);
            pen.curve(back, ink);
            pen.curve(front, ink);
            pen.line(&[(6.0, 14.0), (3.0, 20.0)], ink);
            pen.line(&[(21.0, 14.0), (18.0, 20.0)], ink);
            pen.dashed(&bezier(back.map(|(x, y)| (x, y - 9.0)), 16), a);
            pen.dashed(&bezier(front.map(|(x, y)| (x, y - 9.0)), 16), a);
            pen.dashed(&[(6.0, 5.0), (3.0, 11.0)], a);
            pen.dashed(&[(21.0, 5.0), (18.0, 11.0)], a);
            pen.arrow((12.0, 17.0), (12.0, 8.5), a);
        }
        "ruled" => {
            // A strip hanging off a block's edge (accent), ruled by straight lines.
            pen.poly(&[(2.0, 12.0), (12.0, 12.0), (12.0, 22.0), (2.0, 22.0)], pen.fill, ink);
            pen.poly(&[(12.0, 12.0), (21.0, 4.0), (21.0, 14.0), (12.0, 22.0)], ic.sheet, ink);
            pen.line(&[(15.0, 9.3), (15.0, 19.3)], a);
            pen.line(&[(18.0, 6.7), (18.0, 16.7)], a);
            pen.line(&[(12.0, 12.0), (12.0, 22.0)], a);
        }
        "boundary" => {
            // Four boundary curves (accent) meeting at corners, the patch between them a Coons
            // patch with its iso lines.
            let (c00, c10, c11, c01) = ((4.0, 17.0), (10.0, 5.0), (20.0, 7.0), (17.0, 20.0));
            let left = [c00, (5.0, 12.0), (8.0, 8.0), c10];
            let top = [c10, (13.0, 4.0), (17.0, 4.0), c11];
            let right = [c01, (19.0, 16.0), (20.0, 11.0), c11];
            let bottom = [c00, (8.0, 20.0), (13.0, 21.0), c01];
            let n = 12;
            let lerp = |p: (f32, f32), q: (f32, f32), s: f32| (p.0 + (q.0 - p.0) * s, p.1 + (q.1 - p.1) * s);
            let (b, t) = (bezier(bottom, n), bezier(top, n));
            // Each row runs across the patch (u) at one height (v) up the left and right sides.
            let rows: Vec<Vec<(f32, f32)>> = bezier(left, n)
                .into_iter()
                .zip(bezier(right, n))
                .enumerate()
                .map(|(j, (lv, rv))| {
                    let v = j as f32 / n as f32;
                    b.iter()
                        .zip(&t)
                        .enumerate()
                        .map(|(i, (bu, tu))| {
                            let u = i as f32 / n as f32;
                            let corners = lerp(lerp(c00, c01, u), lerp(c10, c11, u), v);
                            let (ruled_v, ruled_u) = (lerp(*bu, *tu, v), lerp(lv, rv, u));
                            (ruled_v.0 + ruled_u.0 - corners.0, ruled_v.1 + ruled_u.1 - corners.1)
                        })
                        .collect()
                })
                .collect();
            for (lo, hi) in rows.iter().zip(rows.iter().skip(1)) {
                pen.band(lo, hi, ic.sheet);
            }
            for j in [n / 3, 2 * n / 3] {
                if let Some(r) = rows.get(j) {
                    pen.line(r, ic.sheet_line);
                }
                pen.line(&rows.iter().filter_map(|r| r.get(j).copied()).collect::<Vec<_>>(), ic.sheet_line);
            }
            for c in [left, top, right, bottom] {
                pen.curve(c, a);
            }
            for (x, y) in [c00, c10, c11, c01] {
                pen.dot(x, y, ink);
            }
        }
        "surface_blend" => {
            // Two separate sheets at different heights bridged by a smooth strip (accent) that
            // leaves both edges tangent.
            let near = [(13.0, 20.0), (8.5, 20.0), (10.5, 11.0), (6.0, 11.0)];
            let far = near.map(|(x, y)| (x + 4.4, y - 3.6));
            pen.poly(&[(2.0, 11.0), (6.0, 11.0), (10.4, 7.4), (6.4, 7.4)], ic.sheet, ink);
            pen.band(&bezier(near, 16), &bezier(far, 16), ic.sheet_light);
            pen.curve(far, a);
            pen.poly(&[(13.0, 20.0), (17.0, 20.0), (21.4, 16.4), (17.4, 16.4)], ic.sheet, ink);
            pen.curve(near, a);
        }
        "untrim" => {
            // A trimmed sheet whose hole (dashed, accent) is filled back in.
            pen.poly(&[(3.0, 17.0), (9.0, 5.0), (21.0, 7.0), (16.0, 20.0)], ic.sheet, ink);
            let hole = ellipse(12.2, 12.3, 4.2, 3.2, 0.0, std::f32::consts::TAU, 32);
            pen.poly(hole.get(1..).unwrap_or_default(), ic.sheet_light, Color32::TRANSPARENT);
            pen.dashed(&hole, a);
        }
        "unstitch" => {
            // Two sheets pulled apart from their shared edge.
            pen.poly(&[(2.0, 18.0), (9.0, 14.0), (9.0, 4.0), (2.0, 8.0)], ic.sheet, ink);
            pen.poly(&[(15.0, 14.0), (22.0, 18.0), (22.0, 8.0), (15.0, 4.0)], ic.sheet_2, ink);
            pen.arrow((10.8, 11.0), (7.0, 11.0), a);
            pen.arrow((13.2, 11.0), (17.0, 11.0), a);
        }
        "reverse_normal" => {
            // A sheet with its old normal (faded) and the flipped one (accent).
            pen.poly(&[(2.0, 14.0), (14.0, 10.0), (22.0, 13.0), (10.0, 18.0)], ic.sheet, ink);
            pen.arrow((12.0, 14.0), (12.0, 3.5), ink.gamma_multiply(0.5));
            pen.arrow((12.0, 14.0), (12.0, 22.5), a);
        }
        "surface_fillet" => {
            // A wall sheet and a floor sheet joined by a round strip (accent edges).
            let near = [(3.0, 16.0), (3.0, 18.8), (5.2, 21.0), (8.0, 21.0)];
            let far = near.map(|(x, y)| (x + 4.4, y - 3.6));
            pen.poly(&[(3.0, 16.0), (3.0, 8.0), (7.4, 4.4), (7.4, 12.4)], ic.sheet_2, ink);
            pen.poly(&[(8.0, 21.0), (16.0, 21.0), (20.4, 17.4), (12.4, 17.4)], ic.sheet, ink);
            pen.band(&bezier(near, 16), &bezier(far, 16), ic.sheet_light);
            pen.line(&[(4.5, 19.5), (8.9, 15.9)], ic.sheet_line);
            pen.curve(far, a);
            pen.curve(near, a);
        }
        "continuity" => {
            // A straight curve joining a curved one (accent) at a point, with the curved one's
            // curvature comb: teeth that grow as the curve tightens.
            pen.line(&[(2.0, 17.0), (12.0, 13.0)], ink);
            let c = [(12.0, 13.0), (16.5, 11.2), (20.0, 8.0), (21.0, 3.0)];
            let pts = bezier(c, 10);
            let mut tips = Vec::new();
            for (i, ((x, y), (x1, y1))) in pts.iter().zip(pts.iter().skip(1)).enumerate() {
                let len = (x1 - x).hypot(y1 - y).max(1e-6);
                let h = 0.8 + 4.2 * i as f32 / 10.0;
                let tip = (x + (y1 - y) / len * h, y - (x1 - x) / len * h);
                if i % 2 == 0 {
                    pen.line(&[(*x, *y), tip], ic.sheet_line);
                }
                tips.push(tip);
            }
            pen.line(&tips, ic.sheet_line);
            pen.curve(c, a);
            pen.dot(12.0, 13.0, a);
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
            pen.poly(&[(9.0, 10.0), (14.0, 10.0), (14.0, 16.0), (9.0, 16.0)], ic.interference, ic.interference);
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
        // Fastener: a socket head screw seen from the side.
        "fastener" => {
            pen.poly(&[(6.0, 3.0), (18.0, 3.0), (18.0, 9.0), (6.0, 9.0)], pen.fill, ink);
            pen.line(&[(10.0, 3.0), (10.0, 6.0), (14.0, 6.0), (14.0, 3.0)], a);
            pen.poly(&[(9.0, 9.0), (15.0, 9.0), (15.0, 21.0), (9.0, 21.0)], pen.fill.gamma_multiply(0.85), ink);
            for y in [12.0, 15.0, 18.0] {
                pen.line(&[(9.0, y), (15.0, y + 1.5)], ink);
            }
        }
        "fold" => {
            pen.line(&[(2.0, 18.0), (12.0, 18.0)], ink);
            pen.line(&[(12.0, 18.0), (19.0, 6.0)], a);
            pen.line(&[(9.0, 21.0), (15.0, 15.0)], ic.fold);
            pen.arc(12.0, 18.0, 6.0, 0.2, 1.0, a);
            pen.arrow((18.0, 16.0), (17.0, 12.5), a);
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
        // Appearance: a shaded ball with three paint dabs.
        "appearance" => {
            pen.circle(10.0, 13.0, 7.5, pen.fill, ink);
            pen.arc(10.0, 13.0, 4.5, 1.8, 3.2, ic.highlight);
            pen.circle(18.0, 5.0, 2.6, ic.paint[0], ink);
            pen.circle(20.5, 11.0, 2.6, ic.paint[1], ink);
            pen.circle(19.0, 17.5, 2.6, ic.paint[2], ink);
        }
        "warning" => {
            pen.poly(&[(12.0, 3.0), (21.0, 20.0), (3.0, 20.0)], ic.warning, ink);
            pen.line(&[(12.0, 9.0), (12.0, 14.0)], ink);
            pen.dot(12.0, 17.0, ink);
        }
        // A failed feature: a disc with a cross, a different shape from the warning's triangle, so
        // it reads without colour (#972). Drawn in the caller's colours only.
        "error" => {
            pen.circle(12.0, 12.0, 10.0, pen.fill, ink);
            pen.line(&[(8.0, 8.0), (16.0, 16.0)], ink);
            pen.line(&[(16.0, 8.0), (8.0, 16.0)], ink);
        }
        _ => {
            pen.poly(&[(5.0, 5.0), (19.0, 5.0), (19.0, 19.0), (5.0, 19.0)], pen.fill, ink);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A debug dump of everything `paint` draws for `name`, for comparing two colour choices
    /// without depending on epaint's internal shape fields.
    fn drawn(name: &str, ink: Color32, fill: Color32, accent: Color32) -> String {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let out = ctx.run_ui(input, |ui| {
            let r = Rect::from_min_size(Pos2::ZERO, vec2(24.0, 24.0));
            paint(ui.painter(), r, name, ink, fill, accent);
        });
        format!("{:?}", out.shapes)
    }

    /// Command icons not drawn yet (the fallback square) or drawn past their square, from before
    /// this check. Remove a name when its icon is fixed; never add one.
    const KNOWN_GAPS: &[&str] =
        &["cylinder", "pipe", "offset_face", "thread", "boundary_fill", "coil", "rib", "emboss", "capture", "revert", "camera", "material"];

    /// What is wrong with the icon `name`: not drawn (the fallback square), or drawn past its
    /// square by more than a stroke width.
    fn icon_problem(name: &str) -> Option<String> {
        if drawn(name, Color32::BLACK, Color32::WHITE, Color32::BLUE) == drawn("", Color32::BLACK, Color32::WHITE, Color32::BLUE) {
            return Some("no icon drawn".into());
        }
        let side = 48.0;
        let r = Rect::from_min_size(Pos2::ZERO, vec2(side, side));
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0))), ..Default::default() };
        let out = ctx.run_ui(input, |ui| paint(ui.painter(), r, name, Color32::BLACK, Color32::WHITE, Color32::BLUE));
        let used = out.shapes.iter().fold(Rect::NOTHING, |b, s| b.union(s.shape.visual_bounding_rect()));
        (!r.expand(side / 24.0 * 1.4).contains_rect(used)).then(|| format!("drawn over {used:?}"))
    }

    /// Every command's icon is drawn and stays inside its square.
    #[test]
    fn every_command_icon_is_drawn_inside_its_square() {
        let bad: Vec<String> = solidcraft_engine::command_specs()
            .into_iter()
            .filter(|c| !c.icon.is_empty() && !KNOWN_GAPS.contains(&c.icon))
            .filter_map(|c| icon_problem(c.icon).map(|why| format!("{} ({}): {why}", c.id, c.icon)))
            .collect();
        assert!(bad.is_empty(), "{}", bad.join("\n"));
        for gap in KNOWN_GAPS {
            assert!(icon_problem(gap).is_some(), "`{gap}` is fixed now: take it off KNOWN_GAPS");
        }
    }

    /// The Sketch Palette and the toolbar both rely on this contract. Construction and
    /// centerline must track whatever accent colour the caller passes (both callers pass the
    /// theme's construction orange) and must not change with `ink`.
    #[test]
    fn construction_and_centerline_icons_follow_the_callers_accent_only() {
        for name in ["construction", "centerline"] {
            let base = drawn(name, Color32::BLACK, Color32::WHITE, Color32::from_rgb(1, 2, 3));
            let other_accent = drawn(name, Color32::BLACK, Color32::WHITE, Color32::from_rgb(4, 5, 6));
            assert_ne!(base, other_accent, "{name} icon must change with the caller's accent colour");
            let other_ink = drawn(name, Color32::from_rgb(9, 9, 9), Color32::WHITE, Color32::from_rgb(1, 2, 3));
            assert_eq!(base, other_ink, "{name} icon has no ink-coloured parts: ink alone must not change it");
        }
    }

    /// Normal is a plain line type. It is never construction geometry. Its icon must never pick
    /// up the accent colour. It can never render construction orange even if a caller passes it
    /// by mistake.
    #[test]
    fn normal_icon_never_uses_the_accent_colour() {
        let one_accent = drawn("normal", Color32::BLACK, Color32::WHITE, Color32::from_rgb(1, 2, 3));
        let other_accent = drawn("normal", Color32::BLACK, Color32::WHITE, Color32::from_rgb(4, 5, 6));
        assert_eq!(one_accent, other_accent, "the Normal icon must not change with the accent colour");
        let other_ink = drawn("normal", Color32::from_rgb(9, 9, 9), Color32::WHITE, Color32::from_rgb(1, 2, 3));
        assert_ne!(one_accent, other_ink, "the Normal icon must change with ink");
    }

    /// The dashes (as their first and last point) and the dots (as centre and radius) `paint`
    /// actually drew for `name`, at `side` pixels square: the Sketch Palette draws these three
    /// icons down to about 18 px.
    fn dashes_and_dots(name: &str, side: f32) -> (Vec<(Pos2, Pos2)>, Vec<(Pos2, f32)>) {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let r = Rect::from_min_size(Pos2::ZERO, vec2(side, side));
        let out = ctx.run_ui(input, |ui| {
            paint(ui.painter(), r, name, Color32::BLACK, Color32::WHITE, Color32::from_rgb(232, 140, 48));
        });
        let dashes = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                Shape::Path(p) => Some((*p.points.first()?, *p.points.last()?)),
                _ => None,
            })
            .collect();
        let dots = out.shapes.iter().filter_map(|s| if let Shape::Circle(c) = &s.shape { Some((c.center, c.radius)) } else { None }).collect();
        (dashes, dots)
    }

    /// Pins the dash count and the gap. A future tidy-up might add dashes back in. It might
    /// narrow the gap to look more faithful to the ISO convention. Either change would flatten
    /// the icon back into a near-solid line at the size the Sketch Palette actually draws it,
    /// about 18 px square. That is the exact defect this fix addressed.
    #[test]
    fn construction_and_centerline_read_as_two_dashes_with_a_gap_wider_than_the_stroke() {
        for (name, want_dots) in [("construction", 0), ("centerline", 1)] {
            for side in [18.0, 26.0] {
                let (dashes, dots) = dashes_and_dots(name, side);
                assert_eq!(dashes.len(), 2, "{name} at {side}px must be exactly two dashes");
                assert_eq!(dots.len(), want_dots, "{name} at {side}px has the wrong number of dots");
                let (_, gap_start) = dashes[0];
                let (gap_end, _) = dashes[1];
                let gap = gap_start.distance(gap_end);
                let stroke = line_type_stroke(Rect::from_min_size(Pos2::ZERO, vec2(side, side)));
                assert!(
                    gap > stroke * 2.0,
                    "{name} at {side}px: the gap of {gap:.1}px must exceed the {stroke:.1}px stroke by a wide margin, or the dashes read as one solid line"
                );
            }
        }
    }

    /// Pins the shape of the Centerline dot. It must be large enough to read as a round mark
    /// beside the stroke. It must sit clear of both dashes. The silhouette must stay line, dot,
    /// line.
    #[test]
    fn centerline_dot_is_a_bold_mark_clear_of_both_dashes() {
        for side in [18.0, 26.0] {
            let (dashes, dots) = dashes_and_dots("centerline", side);
            let (center, radius) = dots[0];
            let stroke = line_type_stroke(Rect::from_min_size(Pos2::ZERO, vec2(side, side)));
            assert!(radius > stroke * 1.2, "centerline dot at {side}px: radius {radius:.1}px must exceed the {stroke:.1}px stroke by a wide margin");
            for (a, b) in &dashes {
                assert!(center.distance(*a) > radius, "centerline dot at {side}px must not touch a dash end");
                assert!(center.distance(*b) > radius, "centerline dot at {side}px must not touch a dash end");
            }
        }
    }
}
