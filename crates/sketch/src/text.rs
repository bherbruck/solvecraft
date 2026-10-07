//! Sketch text: glyph outlines of an embedded open font (Liberation Sans, SIL OFL 1.1; see
//! `fonts/OFL.txt` and ATTRIBUTION.md) as lines and conics (TrueType quadratic Béziers are
//! conics with rho = 0.5), so text closes into profiles that can be extruded.

use solvecraft_geom::Vec2;

use crate::link::LinkGeom;
use crate::model::{Result, SketchError};

static FONT: &[u8] = include_bytes!("../fonts/LiberationSans-Regular.ttf");

/// Most characters in one text.
pub const MAX_TEXT_CHARS: usize = 1000;

struct Outline {
    out: Vec<LinkGeom>,
    start: Vec2,
    cur: Vec2,
    map: Box<dyn Fn(f32, f32) -> Vec2>,
}

impl Outline {
    fn line(&mut self, to: Vec2) {
        if self.cur.dist(to) > 1e-9 {
            self.out.push(LinkGeom::Line(self.cur, to));
        }
        self.cur = to;
    }
    fn quad(&mut self, apex: Vec2, to: Vec2) {
        let (a, b) = (self.cur, to);
        let chord = b - a;
        let off = chord.cross(apex - a).abs() / chord.len().max(1e-12);
        if off < 1e-6 || a.dist(b) < 1e-9 {
            self.line(to);
            return;
        }
        self.out.push(LinkGeom::Conic { a, apex, b, rho: 0.5 });
        self.cur = to;
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.start = p;
        self.cur = p;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = (self.map)(x, y);
        self.line(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = ((self.map)(x1, y1), (self.map)(x, y));
        self.quad(c, p);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        // A cubic as two quadratics (split at the middle).
        let (p0, c1, c2, p3) = (self.cur, (self.map)(x1, y1), (self.map)(x2, y2), (self.map)(x, y));
        let mid = (p0 + c1 * 3.0 + c2 * 3.0 + p3) / 8.0;
        let q1 = p0 + (c1 - p0) * 0.75;
        let q2 = p3 + (c2 - p3) * 0.75;
        self.quad(q1, mid);
        self.quad(q2, p3);
    }
    fn close(&mut self) {
        let s = self.start;
        self.line(s);
    }
}

/// Outline geometry of `text` with its baseline starting at `at`, capital height `height`
/// (mm), rotated by `angle` (radians) about `at`. Lines break at `\n`.
pub fn text_geometry(text: &str, at: Vec2, height: f64, angle: f64) -> Result<Vec<LinkGeom>> {
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(SketchError::TooLarge);
    }
    if !(height.is_finite() && height > 1e-6 && height < 1e6) || !angle.is_finite() || !at.is_finite() {
        return Err(SketchError::Invalid("text height must be positive".into()));
    }
    let face = ttf_parser::Face::parse(FONT, 0).map_err(|e| SketchError::Invalid(format!("font: {e}")))?;
    let cap = face.capital_height().map(f64::from).filter(|c| *c > 0.0).unwrap_or(f64::from(face.units_per_em()) * 0.7);
    let k = height / cap;
    let line_gap = f64::from(face.ascender() - face.descender() + face.line_gap()) * k;
    let (s, c) = angle.sin_cos();
    let mut out = Vec::new();
    for (row, line) in text.split('\n').enumerate() {
        let mut pen = 0.0f64;
        let base_y = -(row as f64) * line_gap;
        for ch in line.chars() {
            let Some(gid) = face.glyph_index(ch).or_else(|| face.glyph_index('?')) else { continue };
            let x0 = pen;
            let map = Box::new(move |x: f32, y: f32| {
                let (lx, ly) = (x0 + f64::from(x) * k, base_y + f64::from(y) * k);
                at + Vec2::new(lx * c - ly * s, lx * s + ly * c)
            });
            let mut ob = Outline { out: Vec::new(), start: at, cur: at, map };
            let _ = face.outline_glyph(gid, &mut ob);
            out.extend(ob.out);
            pen += f64::from(face.glyph_hor_advance(gid).unwrap_or(0)) * k;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_have_closed_outlines_at_the_cap_height() {
        let g = text_geometry("HO", Vec2::new(5.0, 0.0), 10.0, 0.0).unwrap();
        assert!(!g.is_empty());
        let ys: Vec<f64> = g
            .iter()
            .flat_map(|x| match *x {
                LinkGeom::Line(a, b) => vec![a.y, b.y],
                LinkGeom::Conic { a, b, .. } => vec![a.y, b.y],
                _ => vec![],
            })
            .collect();
        let top = ys.iter().cloned().fold(f64::MIN, f64::max);
        assert!((top - 10.0).abs() < 0.3, "{top}");
        assert!(g.iter().any(|x| matches!(x, LinkGeom::Conic { .. })), "O has curves");
        assert!(text_geometry("x", Vec2::ZERO, -1.0, 0.0).is_err());
        assert!(text_geometry(&"x".repeat(2000), Vec2::ZERO, 1.0, 0.0).is_err());
        // Empty text makes nothing (not an error here; the command rejects it).
        assert!(text_geometry("", Vec2::ZERO, 1.0, 0.0).unwrap().is_empty());
    }
}
