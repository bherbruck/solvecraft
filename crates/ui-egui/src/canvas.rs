//! On-canvas command input: while a feature dialog is open, its main value (extrude distance,
//! fillet radius…) gets a value box in the viewport next to the geometry, and an arrow
//! (extrude, fillet, shell, move) that can be dragged: values snap to round steps (hold Alt or
//! Ctrl for a free drag) and the dialog follows. The box takes the keyboard as soon as the
//! command has its geometry, so typing a number sets the value at once; Enter applies, Tab
//! moves on, Esc cancels.

use egui::{Color32, Pos2, Stroke, vec2};
use solvecraft_engine::Sel;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::geom::{Mesh, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::theme::Tokens;
use crate::viewport::Proj;

/// Where the dialog's geometry is: a base point and, for values that can be dragged, the
/// direction a growing value points.
pub fn anchor(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Option<Vec3>)> {
    let st = app.session.model.state();
    let first = d.inputs.first()?.items.first()?;
    let (at, normal) = match first {
        Sel::Profile { sketch, index } => {
            let ss = st.sketch(*sketch)?;
            let p = ss.profiles.get(*index)?;
            (ss.plane.to_world(p.region.interior_point()), Some(ss.plane.normal()))
        }
        Sel::Face { body, index, point } => (*point, crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| n)),
        Sel::Edge { body, index, point } => (*point, st.body(body).and_then(|b| edge_bisector(&b.mesh(), *index, *point))),
        Sel::Vertex { point, .. } => (*point, None),
        Sel::Body { name } => (st.body(name)?.mesh().bounds().center(), None),
        _ => return None,
    };
    Some(match d.kind {
        Kind::Extrude { .. } | Kind::Fillet { .. } => (at, normal),
        // Shell thickness grows into the body.
        Kind::Shell { .. } => (at, normal.map(|n| -n)),
        Kind::Move { .. } => (at, Some(Vec3::Z)),
        Kind::PatternRect { .. } => (at, d.inputs.get(1).and_then(|i| i.items.first()).and_then(|x| crate::dialogs::axis_of(app, x)).map(|a| a.1)),
        _ => (at, None),
    })
}

/// The outward direction between the two faces at an edge (where a fillet's radius arrow
/// points).
fn edge_bisector(m: &Mesh, index: usize, p: Vec3) -> Option<Vec3> {
    let faces = m.edge_faces.get(index)?;
    let mut sum = Vec3::ZERO;
    for f in faces {
        let n = m
            .triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| *tf == f)
            .filter_map(|(t, _)| m.tri(t))
            .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(p).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(p)))
            .and_then(|[a, b, c]| (b - a).cross(c - a).normalized())?;
        sum += n;
    }
    sum.normalized()
}

/// Draw the manipulator and the value box (called by the viewport after the model is drawn).
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(mut d) = app.dialog.take() else { return };
    if let Some((base, dir)) = anchor(app, &d)
        && let Some(bs) = proj.to_screen(base)
    {
        draw(app, ui, painter, proj, &mut d, base, bs, dir);
    }
    app.dialog = Some(d);
}

/// A round step for dragged values at this zoom (1, 2 or 5 times a power of ten).
pub fn snap_step(half_height: f64) -> f64 {
    let raw = (half_height / 40.0).max(1e-4);
    let p = 10f64.powf(raw.log10().floor());
    if raw / p < 2.0 {
        p
    } else if raw / p < 5.0 {
        2.0 * p
    } else {
        5.0 * p
    }
}

/// Parameter along the line `base + dir * t` closest to the view ray through `pos`.
fn along(proj: &Proj, pos: Pos2, base: Vec3, dir: Vec3) -> Option<f64> {
    let (o, r) = proj.ray(pos);
    let w0 = base - o;
    let (b, d, e) = (dir.dot(r), dir.dot(w0), r.dot(w0));
    let den = 1.0 - b * b;
    // Looking straight along the line: nothing to drag.
    if den.abs() < 1e-6 {
        return None;
    }
    Some((b * e - d) / den)
}

fn format_len(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} mm", if s == "-0" { "0" } else { s })
}

#[allow(clippy::too_many_arguments)]
fn draw(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog, base: Vec3, bs: Pos2, dir: Option<Vec3>) {
    let t = Tokens::get();
    let sign = match d.kind {
        Kind::Extrude { direction: 1, .. } => -1.0,
        _ => 1.0,
    };
    let symmetric = matches!(d.kind, Kind::Extrude { direction: 2, .. });
    // Extrudes and moves go either way; radii and thicknesses stay positive.
    let signed = matches!(d.kind, Kind::Extrude { .. } | Kind::Move { .. });
    let focus = std::mem::take(&mut d.focus);
    let half_height = app.cam.half_height();
    let axis = revolve_axis(app, d);
    let Some((label, kind, value)) = d.primary() else { return };
    let len = app.session.doc.eval(value, kind).ok().filter(|v| v.is_finite());
    let mut box_at = bs + vec2(16.0, -30.0);
    if let (Some(axis), Some(angle)) = (axis, len.filter(|_| kind == ValueKind::Angle))
        && let Some(at) = rotator(ui, painter, proj, base, axis, angle, value)
    {
        box_at = at + vec2(14.0, -12.0);
    }
    if let (Some(n), Some(l)) = (dir, len.filter(|_| kind == ValueKind::Length)) {
        let n = n * sign;
        // A short arrow still shows (and can be grabbed) when the value is near zero.
        let min = 40.0 * 2.0 * half_height / f64::from(proj.rect.height().max(1.0));
        let shown = if l.abs() < min { min * if l < 0.0 { -1.0 } else { 1.0 } } else { l };
        let tip = base + n * shown;
        if let Some(ts) = proj.to_screen(tip) {
            let handle = egui::Rect::from_center_size(ts, vec2(20.0, 20.0));
            let r = ui.interact(handle, egui::Id::new("sc_manipulator"), egui::Sense::drag());
            let hot = r.hovered() || r.dragged();
            let col = if hot { t.accent } else { t.manipulator };
            arrow(painter, bs, ts, col);
            if symmetric && let Some(bt) = proj.to_screen(base - n * shown) {
                arrow(painter, bs, bt, col);
            }
            painter.circle(bs, 3.5, col, Stroke::new(1.0, Color32::WHITE));
            if hot {
                painter.circle_stroke(ts, 8.0, Stroke::new(1.5, col));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
                && let Some(v) = along(proj, p, base, n)
            {
                let free = ui.input(|i| i.modifiers.alt || i.modifiers.ctrl || i.modifiers.command);
                let step = snap_step(half_height);
                let mut v = if free { v } else { (v / step).round() * step };
                if !signed {
                    v = v.max(if free { 1e-3 } else { step });
                }
                *value = format_len(v);
            }
            box_at = ts + vec2(14.0, -12.0);
        }
    }
    let id = egui::Id::new("sc_canvas_value");
    egui::Area::new(egui::Id::new("sc_canvas_area")).fixed_pos(box_at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).color(t.text_dim).size(11.5));
                let r = ui.add(egui::TextEdit::singleline(value).id(id).desired_width(72.0));
                if focus {
                    r.request_focus();
                    let n = value.chars().count();
                    let mut st = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
                    st.cursor.set_char_range(Some(egui::text_selection::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                    st.store(ui.ctx(), id);
                }
            });
        });
    });
}

/// The revolve axis (a point on it and its unit direction).
fn revolve_axis(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Vec3)> {
    if !matches!(d.kind, Kind::Revolve { .. }) {
        return None;
    }
    crate::dialogs::axis_of(app, d.inputs.get(1)?.items.first()?)
}

/// The revolve angle rotator: an arc from the profile around the axis with a handle at its end
/// that drags the angle (5° steps; Alt or Ctrl: free). Returns the handle's screen position.
#[allow(clippy::too_many_arguments)]
fn rotator(ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, p0: Vec3, (o, a): (Vec3, Vec3), angle: f64, value: &mut String) -> Option<Pos2> {
    let t = Tokens::get();
    let c = o + a * (p0 - o).dot(a);
    let r = (p0 - c).len();
    if r < 1e-9 {
        return None;
    }
    let e1 = (p0 - c) / r;
    let e2 = a.cross(e1);
    let at = |th: f64| c + (e1 * th.cos() + e2 * th.sin()) * r;
    let n = 48;
    let pts: Vec<Pos2> = (0..=n).filter_map(|i| proj.to_screen(at(angle * i as f64 / n as f64))).collect();
    let end = proj.to_screen(at(angle))?;
    let handle = egui::Rect::from_center_size(end, vec2(20.0, 20.0));
    let resp = ui.interact(handle, egui::Id::new("sc_rotator"), egui::Sense::drag());
    let hot = resp.hovered() || resp.dragged();
    let col = if hot { t.accent } else { t.manipulator };
    painter.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
    if let (Some(cs), Some(ps)) = (proj.to_screen(c), proj.to_screen(p0)) {
        painter.add(egui::Shape::dashed_line(&[cs, ps], Stroke::new(1.0, col), 4.0, 3.0));
    }
    painter.circle(end, if hot { 7.0 } else { 5.5 }, col, Stroke::new(1.5, Color32::WHITE));
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if resp.dragged()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (ro, rd) = proj.ray(p);
        let den = rd.dot(a);
        if den.abs() > 1e-9 {
            let q = ro + rd * ((c - ro).dot(a) / den) - c;
            let mut th = q.dot(e2).atan2(q.dot(e1)).to_degrees().rem_euclid(360.0);
            let free = ui.input(|i| i.modifiers.alt || i.modifiers.ctrl || i.modifiers.command);
            if !free {
                th = (th / 5.0).round() * 5.0;
            }
            if th < 0.5 {
                th = 360.0;
            }
            let s = format!("{th:.1}");
            *value = format!("{} deg", s.trim_end_matches('0').trim_end_matches('.'));
        }
    }
    Some(end)
}

fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, col: Color32) {
    let v = b - a;
    let l = v.length();
    if l < 1.0 {
        return;
    }
    let u = v / l;
    let n = vec2(-u.y, u.x);
    painter.line_segment([a, b], Stroke::new(2.0, col));
    let head = 11.0f32.min(l * 0.6);
    painter.add(egui::Shape::convex_polygon(vec![b, b - u * head + n * head * 0.45, b - u * head - n * head * 0.45], col, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_steps_and_formatting() {
        assert_eq!(snap_step(40.0), 1.0);
        assert_eq!(snap_step(100.0), 2.0);
        assert_eq!(snap_step(4.0), 0.1);
        assert_eq!(format_len(12.0), "12 mm");
        assert_eq!(format_len(-2.5), "-2.5 mm");
        assert_eq!(format_len(-0.0001), "0 mm");
    }
}
