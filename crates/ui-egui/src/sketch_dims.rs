//! Inline dimensions while drawing in a sketch. Once a shape's first point is down, value
//! boxes (line length and angle, rectangle width and height, circle diameter…) sit next to the
//! segment they measure and follow the cursor. Typing goes straight into the first box and
//! locks that value; Tab moves to the next box; Enter places the shape with the typed values,
//! which become dimension constraints. Values are expressions ("25", "1 in", "d1 * 2").

use egui::{Pos2, vec2};
use serde_json::{Value, json};
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::tools::Tool;
use crate::viewport::Proj;

/// One value box.
#[derive(Clone, Debug)]
pub struct DimBox {
    pub label: &'static str,
    pub kind: ValueKind,
    /// The expression shown (the live value while not locked).
    pub value: String,
    /// Typed by the user: the value no longer follows the cursor.
    pub locked: bool,
}

impl DimBox {
    fn new(label: &'static str, kind: ValueKind) -> DimBox {
        DimBox { label, kind, value: String::new(), locked: false }
    }
}

/// The boxes a tool shows after `n` points.
pub fn boxes_for(cmd: &str, n: usize) -> Vec<DimBox> {
    use ValueKind::*;
    let b = DimBox::new;
    match (cmd, n) {
        ("DrawPolyline", n) if n >= 1 => vec![b("Length", Length), b("Angle", Angle)],
        ("ShapeRectangleTwoPoint" | "ShapeRectangleCenter", 1) => vec![b("Width", Length), b("Height", Length)],
        ("CircleCenterRadius", 1) => vec![b("Diameter", Length)],
        ("ArcCenterTwoPoint", 1) => vec![b("Radius", Length)],
        ("ArcCenterTwoPoint", 2) => vec![b("Sweep", Angle)],
        ("ShapePolygonInscribed" | "ShapePolygonCircumscribed", 1) => vec![b("Radius", Length), b("Sides", Unitless)],
        ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 1) => vec![b("Length", Length)],
        ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 2) => vec![b("Width", Length)],
        _ => Vec::new(),
    }
}

/// A locked box's value (mm or radians).
fn locked(app: &SolveApp, t: &Tool, label: &str) -> Option<f64> {
    let b = t.dims.iter().find(|b| b.label == label && b.locked)?;
    app.session.doc.eval(&b.value, b.kind).ok().filter(|v| v.is_finite())
}

fn dir_or_x(v: Vec2) -> Vec2 {
    v.normalized().unwrap_or(Vec2::X)
}

fn sign(x: f64) -> f64 {
    if x < 0.0 { -1.0 } else { 1.0 }
}

/// The next point the tool would place for the cursor at `h`, honouring locked values.
pub fn effective(app: &SolveApp, t: &Tool, h: Vec2) -> Vec2 {
    let Some(a) = t.pts.last().map(|p| p.0) else { return h };
    let lk = |l: &str| locked(app, t, l);
    match (t.cmd, t.pts.len()) {
        ("DrawPolyline", _) | ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 1) => {
            let v = h - a;
            let len = lk("Length").unwrap_or(v.len());
            let ang = lk("Angle").unwrap_or_else(|| snap_axis(v.angle()));
            a + Vec2::from_angle(ang) * len
        }
        ("ShapeRectangleTwoPoint", 1) => {
            let d = h - a;
            Vec2::new(a.x + lk("Width").map_or(d.x, |w| w * sign(d.x)), a.y + lk("Height").map_or(d.y, |hh| hh * sign(d.y)))
        }
        ("ShapeRectangleCenter", 1) => {
            let d = h - a;
            Vec2::new(a.x + lk("Width").map_or(d.x, |w| w * 0.5 * sign(d.x)), a.y + lk("Height").map_or(d.y, |hh| hh * 0.5 * sign(d.y)))
        }
        ("CircleCenterRadius", 1) => a + dir_or_x(h - a) * lk("Diameter").map_or((h - a).len(), |d| d * 0.5),
        ("ArcCenterTwoPoint", 1) => a + dir_or_x(h - a) * lk("Radius").unwrap_or((h - a).len()),
        ("ArcCenterTwoPoint", 2) => {
            let c = t.pts.first().map(|p| p.0).unwrap_or(a);
            let r = (a - c).len();
            let a0 = (a - c).angle();
            let sweep = lk("Sweep").unwrap_or_else(|| ((h - c).angle() - a0).rem_euclid(std::f64::consts::TAU));
            c + Vec2::from_angle(a0 + sweep) * r
        }
        ("ShapePolygonInscribed" | "ShapePolygonCircumscribed", 1) => a + dir_or_x(h - a) * lk("Radius").unwrap_or((h - a).len()),
        ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 2) => match lk("Width") {
            Some(w) => {
                let c0 = t.pts.first().map(|p| p.0).unwrap_or(a);
                let dir = dir_or_x(a - c0);
                let n = dir.perp();
                let side = sign(n.dot(h - c0));
                c0 + (a - c0) * 0.5 + n * (w * 0.5 * side)
            }
            None => h,
        },
        _ => h,
    }
}

/// Angles within a few degrees of horizontal or vertical snap to it (the line is drawn
/// horizontal or vertical, and the command infers the constraint).
pub fn snap_axis(ang: f64) -> f64 {
    let q = std::f64::consts::FRAC_PI_2;
    let k = (ang / q).round();
    if (ang - k * q).abs() < 2.5f64.to_radians() { k * q } else { ang }
}

/// Is the segment exactly horizontal or vertical (an inferred constraint)?
pub fn axis_aligned(a: Vec2, b: Vec2) -> Option<bool> {
    let d = b - a;
    if d.len() < 1e-9 {
        return None;
    }
    if d.y.abs() < 1e-9 * d.len() {
        Some(true)
    } else if d.x.abs() < 1e-9 * d.len() {
        Some(false)
    } else {
        None
    }
}

/// The value each box shows while it follows the cursor, and where it sits (sketch coordinates).
/// `px` is the size of a screen pixel in sketch units: boxes sit a little off the segment they
/// measure so they don't cover it.
fn live(t: &Tool, e: Vec2, px: f64) -> Vec<(f64, Vec2)> {
    let Some(a) = t.pts.last().map(|p| p.0) else { return Vec::new() };
    let first = t.pts.first().map(|p| p.0).unwrap_or(a);
    let off = 20.0 * px;
    match (t.cmd, t.pts.len()) {
        ("DrawPolyline", _) | ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 1) => {
            let v = e - a;
            let d = dir_or_x(v);
            let n = d.perp();
            vec![(v.len(), (a + e) * 0.5 + n * off), (v.angle(), a + d * (v.len() * 0.3).min(60.0 * px) - n * off)]
        }
        ("ShapeRectangleTwoPoint", 1) => {
            let (sx, sy) = (sign(e.x - a.x), sign(e.y - a.y));
            vec![
                ((e.x - a.x).abs(), Vec2::new((a.x + e.x) * 0.5, a.y - sy * off)),
                ((e.y - a.y).abs(), Vec2::new(e.x + sx * 2.0 * off, (a.y + e.y) * 0.5)),
            ]
        }
        ("ShapeRectangleCenter", 1) => {
            let d = e - a;
            let (sx, sy) = (sign(d.x), sign(d.y));
            vec![(2.0 * d.x.abs(), Vec2::new(a.x, a.y - d.y - sy * off)), (2.0 * d.y.abs(), Vec2::new(a.x + d.x + sx * 2.0 * off, a.y))]
        }
        ("CircleCenterRadius", 1) => vec![(2.0 * (e - a).len(), (a + e) * 0.5 + dir_or_x(e - a).perp() * off)],
        ("ArcCenterTwoPoint", 1) | ("ShapePolygonInscribed" | "ShapePolygonCircumscribed", 1) => {
            vec![((e - a).len(), (a + e) * 0.5), (6.0, a + (e - a) * 0.5 + (e - a).perp() * 0.3)]
        }
        ("ArcCenterTwoPoint", 2) => {
            let a0 = (a - first).angle();
            vec![(((e - first).angle() - a0).rem_euclid(std::f64::consts::TAU), e)]
        }
        ("ShapeSlotCenterToCenter" | "ShapeSlotOverall", 2) => {
            let dir = dir_or_x(a - first);
            vec![(2.0 * dir.cross(e - first).abs(), e)]
        }
        _ => Vec::new(),
    }
}

fn fmt(kind: ValueKind, v: f64) -> String {
    match kind {
        ValueKind::Angle => format!("{:.1} deg", v.to_degrees()),
        ValueKind::Unitless => format!("{}", v.round()),
        ValueKind::Length => format!("{v:.2}"),
    }
}

/// Make the tool's boxes match its stage (new boxes when a point was placed).
pub fn sync(t: &mut Tool) {
    if t.dims_stage != t.pts.len() {
        t.dims = boxes_for(t.cmd, t.pts.len());
        t.dims_stage = t.pts.len();
        t.dims_focus = !t.dims.is_empty();
    }
}

/// The dimension constraints the locked values ask for after the shape made `curves`:
/// (entities, type, value).
pub fn constraints(t: &Tool, curves: &[String]) -> Vec<(Vec<String>, &'static str, String)> {
    let mut out = Vec::new();
    for b in t.dims.iter().filter(|b| b.locked) {
        let target = match (t.cmd, b.label) {
            ("DrawPolyline", "Length") | ("ShapeRectangleTwoPoint" | "ShapeRectangleCenter", "Width") => curves.first().map(|c| (c, "length")),
            ("ShapeRectangleTwoPoint" | "ShapeRectangleCenter", "Height") => curves.get(1).map(|c| (c, "length")),
            ("CircleCenterRadius", "Diameter") => curves.first().map(|c| (c, "diameter")),
            ("ArcCenterTwoPoint", "Radius") => curves.first().map(|c| (c, "radius")),
            _ => None,
        };
        if let Some((c, ty)) = target {
            out.push((vec![c.clone()], ty, b.value.clone()));
        }
    }
    out
}

/// A locked box value by label (for the polygon side count).
pub fn locked_value(app: &SolveApp, t: &Tool, label: &str) -> Option<f64> {
    locked(app, t, label)
}

/// Draw the boxes of the active tool next to what they measure. Enter places the point.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, proj: &Proj) {
    // Esc in a box ends the shape (the box had the keyboard, so the shortcut didn't see it).
    let in_box =
        app.tool.as_ref().is_some_and(|t| (0..t.dims.len()).any(|i| ui.ctx().memory(|m| m.had_focus_last_frame(egui::Id::new(("sc_dim", i))))));
    if in_box && !app.esc_handled && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.esc_handled = true;
        crate::tools::finish(app);
        return;
    }
    let Some(mut t) = app.tool.take() else { return };
    sync(&mut t);
    let place = if t.dims.is_empty() { None } else { boxes(app, ui, proj, &mut t) };
    app.tool = Some(t);
    if let Some(p) = place {
        crate::tools::place(app, (p, None));
    }
}

fn boxes(app: &SolveApp, ui: &mut egui::Ui, proj: &Proj, t: &mut Tool) -> Option<Vec2> {
    let tk = Tokens::get();
    let sid = app.session.active_sketch?;
    let st = app.session.model.state();
    let plane = st.sketch(sid)?.plane;
    let h = t.hover.as_ref().map(|x| x.0)?;
    let e = effective(app, t, h);
    let px = 2.0 * app.cam.half_height() / f64::from(proj.rect.height().max(1.0));
    let lv = live(t, e, px);
    let focus = std::mem::take(&mut t.dims_focus);
    let mut any_focused = false;
    for (i, b) in t.dims.iter_mut().enumerate() {
        let Some((v, at)) = lv.get(i).copied() else { continue };
        if !b.locked {
            b.value = fmt(b.kind, v);
        }
        let Some(sp) = proj.to_screen(plane.to_world(at)) else { continue };
        let id = egui::Id::new(("sc_dim", i));
        let has_focus = ui.ctx().memory(|m| m.has_focus(id));
        // While a box follows the cursor its whole text is selected, so typing replaces it.
        if (has_focus || (focus && i == 0)) && !b.locked {
            let mut s = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            let n = b.value.chars().count();
            s.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
            s.store(ui.ctx(), id);
        }
        let pos: Pos2 = sp + vec2(-34.0, -11.0);
        egui::Area::new(egui::Id::new(("sc_dim_area", i))).fixed_pos(pos).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            let fill = if b.locked { tk.accent_soft } else { tk.panel };
            egui::Frame::popup(ui.style()).fill(fill).inner_margin(egui::Margin::symmetric(3, 1)).show(ui, |ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut b.value).id(id).desired_width(66.0)).on_hover_text(b.label);
                crate::params_dialog::complete(ui, &r, &mut b.value);
                if r.changed() {
                    b.locked = true;
                }
                if focus && i == 0 {
                    r.request_focus();
                }
                any_focused |= r.has_focus() || r.lost_focus();
            });
        });
    }
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let nothing = ui.ctx().memory(|m| m.focused().is_none());
    (enter && (any_focused || nothing)).then_some(e)
}

/// JSON for a typed dimension.
pub fn dimension_params(entities: &[String], ty: &str, value: &str) -> Value {
    json!({"entities": entities, "type": ty, "value": value})
}
