//! The Sketch Palette: a panel docked at the right while a sketch is being edited, like a
//! command dialog. Options: the line type of the selected curves (normal, construction,
//! centerline), Look At, and the display toggles kept with the sketch (grid, snap, slice,
//! profiles, points, dimensions, constraints, projected geometry, 3D sketch); the sketch's
//! degrees of freedom; Finish Sketch. Toggles run `sketch.options`, line types
//! `sketch.construction` / `sketch.centerline`.

use std::cell::Cell;

use egui::{Color32, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;

use crate::SolveApp;
use crate::theme::Tokens;

thread_local! {
    static FOLDED: Cell<bool> = const { Cell::new(false) };
    static OPTIONS_FOLDED: Cell<bool> = const { Cell::new(false) };
}

/// Palette width (logical px).
const WIDTH: f32 = 236.0;

/// The display toggles: option key and label.
pub const TOGGLES: [(&str, &str); 9] = [
    ("grid", "Sketch Grid"),
    ("snap", "Snap"),
    ("slice", "Slice"),
    ("show_profile", "Show Profile"),
    ("show_points", "Show Points"),
    ("show_dimensions", "Show Dimensions"),
    ("show_constraints", "Show Constraints"),
    ("show_projected", "Show Projected Geometries"),
    ("sketch_3d", "3D Sketch"),
];

/// The degrees-of-freedom line: "Fully constrained" or "N degrees of freedom".
pub fn dof_text(dof: usize, ok: bool) -> String {
    if !ok {
        "Constraints conflict".into()
    } else if dof == 0 {
        "Fully constrained".into()
    } else if dof == 1 {
        "1 degree of freedom".into()
    } else {
        format!("{dof} degrees of freedom")
    }
}

/// Selected curves of the active sketch.
fn selected_curves(app: &SolveApp) -> Vec<String> {
    app.session.selection.iter().filter_map(|s| if let Sel::SketchCurve { id } = s { Some(id.clone()) } else { None }).collect()
}

/// Set the line type of the selected curves: 0 normal, 1 construction, 2 centerline (lines).
pub fn set_linetype(app: &mut SolveApp, kind: u8) {
    let curves = selected_curves(app);
    if curves.is_empty() {
        app.set_status("Select sketch curves first", false);
        return;
    }
    let lines: Vec<String> = {
        let st = app.session.world_state();
        let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
        curves
            .iter()
            .filter(|id| {
                ss.sketch
                    .curve_index(id)
                    .and_then(|i| ss.sketch.curves.get(i))
                    .is_some_and(|c| matches!(c.kind, solvecraft_engine::sketch::CurveKind::Line { .. }))
            })
            .cloned()
            .collect()
    };
    let r = match kind {
        0 => {
            if !lines.is_empty() {
                let _ = app.run("sketch.centerline", json!({ "curves": lines, "value": false }));
            }
            app.run("sketch.construction", json!({ "curves": curves, "value": false }))
        }
        1 => app.run("sketch.construction", json!({ "curves": curves, "value": true })),
        _ if lines.is_empty() => Err("a centerline must be a line".to_string()),
        _ => app.run("sketch.centerline", json!({ "curves": lines, "value": true })),
    };
    if let Err(e) = r {
        app.set_status(e, true);
    }
}

/// Draw the palette (while sketching and no command dialog is open).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if app.dialog.is_some() {
        return;
    }
    let (opts, dof_line, ok) = {
        let st = app.session.world_state();
        let Some(ss) = app.session.active_sketch.and_then(|s| st.sketch(s)) else { return };
        (ss.sketch.view.clone(), dof_text(ss.report.dof, ss.report.ok()), ss.report.ok() && ss.report.dof == 0)
    };
    let t = Tokens::get();
    let vp = app.viewport.rect.unwrap_or_else(|| ctx.content_rect());
    let anchor = egui::pos2(vp.right(), vp.top() + crate::viewport::VIEW_CUBE_CLEARANCE);
    let frame = egui::Frame::window(&ctx.global_style())
        .fill(t.dialog_bg)
        .stroke(Stroke::new(1.0, t.dialog_border))
        .corner_radius(egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 })
        .shadow(egui::Shadow { offset: [-2, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(40) });
    let shown = |k: &str| match k {
        "grid" => !opts.hide_grid,
        "snap" => !opts.no_snap,
        "slice" => opts.slice,
        "show_profile" => !opts.hide_profile,
        "show_points" => !opts.hide_points,
        "show_dimensions" => !opts.hide_dimensions,
        "show_constraints" => !opts.hide_constraints,
        "show_projected" => !opts.hide_projected,
        "sketch_3d" => opts.three_d,
        _ => false,
    };
    let mut set: Option<(String, bool)> = None;
    let mut linetype: Option<u8> = None;
    let mut look = false;
    let mut finish = false;
    let has_curves = !selected_curves(app).is_empty();
    egui::Window::new("SKETCH PALETTE")
        .id(egui::Id::new("sc_sketch_palette"))
        .title_bar(false)
        .frame(frame)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(anchor)
        .constrain(false)
        .auto_sized()
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_width(WIDTH);
            ui.horizontal(|ui| {
                let folded = FOLDED.with(Cell::get);
                if ui.add(egui::Button::new(RichText::new(if folded { "+" } else { "−" }).size(14.0)).frame(false)).clicked() {
                    FOLDED.with(|f| f.set(!folded));
                }
                ui.label(RichText::new("SKETCH PALETTE").strong().size(12.5));
            });
            if FOLDED.with(Cell::get) {
                return;
            }
            let ofold = OPTIONS_FOLDED.with(Cell::get);
            if ui.add(egui::Button::new(RichText::new(format!("{} Options", if ofold { "+" } else { "−" })).size(12.0)).frame(false)).clicked() {
                OPTIONS_FOLDED.with(|f| f.set(!ofold));
            }
            if !ofold {
                egui::Grid::new("sc_palette_grid").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label("Linetype");
                    ui.horizontal(|ui| {
                        for (i, (label, tip)) in [("—", "Normal"), ("- -", "Construction"), ("-·-", "Centerline")].iter().enumerate() {
                            if ui.add_enabled(has_curves, egui::Button::new(*label).min_size(vec2(30.0, 20.0))).on_hover_text(*tip).clicked() {
                                linetype = Some(i as u8);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Look At");
                    if ui.button("Look At").clicked() {
                        look = true;
                    }
                    ui.end_row();
                    for (k, label) in TOGGLES {
                        // The label is part of the checkbox: clicking it toggles too.
                        let mut on = shown(k);
                        if ui.checkbox(&mut on, label).changed() {
                            set = Some((k.to_string(), on));
                        }
                        ui.end_row();
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(RichText::new(dof_line.as_str()).color(if ok { t.sketch_accent } else { t.text_dim }));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(WIDTH - 100.0);
                if ui.add(egui::Button::new(RichText::new("Finish Sketch").color(Color32::WHITE)).fill(t.accent)).clicked() {
                    finish = true;
                }
            });
        });
    if let Some((k, on)) = set {
        let mut p = serde_json::Map::new();
        p.insert(k, Value::Bool(on));
        if let Err(e) = app.run("sketch.options", Value::Object(p)) {
            app.set_status(e, true);
        }
    }
    if let Some(k) = linetype {
        set_linetype(app, k);
    }
    if look {
        crate::dialogs::look_at_sketch(app);
    }
    if finish {
        app.finish_sketch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dof_readout() {
        assert_eq!(dof_text(0, true), "Fully constrained");
        assert_eq!(dof_text(1, true), "1 degree of freedom");
        assert_eq!(dof_text(4, true), "4 degrees of freedom");
        assert_eq!(dof_text(4, false), "Constraints conflict");
    }
}
