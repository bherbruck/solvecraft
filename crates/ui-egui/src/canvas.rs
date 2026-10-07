//! On-canvas command input: while a feature dialog is open, its main value (extrude distance,
//! fillet radius…) gets a value box in the viewport next to the geometry, with a direction
//! arrow for extrudes. The box takes the keyboard as soon as the command has its geometry, so
//! typing a number sets the value at once; Enter applies, Tab moves on, Esc cancels.

use egui::{Color32, Pos2, Stroke, vec2};
use solvecraft_engine::Sel;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::theme::Tokens;
use crate::viewport::Proj;

/// Where the dialog's geometry is: a base point and, for directional values, the direction a
/// positive value points.
pub fn anchor(app: &SolveApp, d: &Dialog) -> Option<(Vec3, Option<Vec3>)> {
    let st = app.session.model.state();
    let first = d.inputs.first()?.items.first()?;
    let at = match first {
        Sel::Profile { sketch, index } => {
            let ss = st.sketch(*sketch)?;
            let p = ss.profiles.get(*index)?;
            (ss.plane.to_world(p.region.interior_point()), Some(ss.plane.normal()))
        }
        Sel::Face { body, index, point } => (*point, crate::dialogs::planar_face(&app.session, body, *index).map(|(_, n)| n)),
        Sel::Edge { point, .. } | Sel::Vertex { point, .. } => (*point, None),
        Sel::Body { name } => (st.body(name)?.mesh().bounds().center(), None),
        _ => return None,
    };
    // Only extrudes point along the normal.
    Some(match d.kind {
        Kind::Extrude { .. } => at,
        _ => (at.0, None),
    })
}

/// Draw the arrow and the value box (called by the viewport after the model is drawn).
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj) {
    let Some(mut d) = app.dialog.take() else { return };
    if let Some((base, dir)) = anchor(app, &d)
        && let Some(bs) = proj.to_screen(base)
    {
        draw(app, ui, painter, proj, &mut d, base, bs, dir);
    }
    app.dialog = Some(d);
}

#[allow(clippy::too_many_arguments)]
fn draw(app: &SolveApp, ui: &mut egui::Ui, painter: &egui::Painter, proj: &Proj, d: &mut Dialog, base: Vec3, bs: Pos2, dir: Option<Vec3>) {
    let t = Tokens::get();
    let sign = match d.kind {
        Kind::Extrude { direction: 1, .. } => -1.0,
        _ => 1.0,
    };
    let symmetric = matches!(d.kind, Kind::Extrude { direction: 2, .. });
    let focus = std::mem::take(&mut d.focus);
    let Some((label, kind, value)) = d.primary() else { return };
    let len = app.session.doc.eval(value, kind).ok().filter(|v| v.is_finite());
    let mut box_at = bs + vec2(16.0, -30.0);
    if let (Some(n), Some(l)) = (dir, len.filter(|_| kind == ValueKind::Length)) {
        let n = n * sign;
        let tip = base + n * l;
        let back = if symmetric { base - n * l } else { base };
        if let (Some(ts), Some(b0)) = (proj.to_screen(tip), proj.to_screen(back)) {
            arrow(painter, b0, ts, t.manipulator);
            if symmetric && let Some(bt) = proj.to_screen(base - n * l) {
                arrow(painter, bs, bt, t.manipulator);
            }
            painter.circle(bs, 3.5, t.manipulator, Stroke::new(1.0, Color32::WHITE));
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
