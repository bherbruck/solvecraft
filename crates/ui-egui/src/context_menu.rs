//! The viewport's right-click menu: repeat the last command, then the commands that fit what is
//! selected (edges: fillet, chamfer; faces: press pull, sketch, hole, shell; bodies: move), then
//! view and selection helpers.

use egui::{Pos2, vec2};
use serde_json::json;
use solvecraft_engine::Sel;

use crate::SolveApp;

/// Commands offered for the current selection: (command id, label).
fn for_selection(app: &SolveApp) -> Vec<(&'static str, &'static str)> {
    let sel = &app.session.selection;
    let has = |f: fn(&Sel) -> bool| sel.iter().any(f);
    let mut v = Vec::new();
    if app.session.active_sketch.is_some() {
        v.push(("DrawPolyline", "Line"));
        v.push(("ShapeRectangleTwoPoint", "Rectangle"));
        v.push(("CircleCenterRadius", "Circle"));
        v.push(("SketchDimension", "Sketch Dimension"));
        if has(|s| matches!(s, Sel::SketchCurve { .. })) {
            v.push(("sketch.construction", "Normal / Construction"));
        }
        v.push(("SketchStop", "Finish Sketch"));
        return v;
    }
    if has(|s| matches!(s, Sel::Edge { .. })) {
        v.push(("FusionFilletEdgesCommand", "Fillet"));
        v.push(("FusionChamferCommand", "Chamfer"));
    }
    if has(|s| matches!(s, Sel::Face { .. } | Sel::Profile { .. })) {
        v.push(("Extrude", "Extrude / Press Pull"));
    }
    if has(|s| matches!(s, Sel::Face { .. })) {
        v.push(("SketchCreate", "Create Sketch"));
        v.push(("FusionHoleCommand", "Hole"));
        v.push(("FusionShellBodyCommand", "Shell"));
    }
    if has(|s| matches!(s, Sel::Face { .. } | Sel::Edge { .. } | Sel::Body { .. })) {
        v.push(("FusionMoveCommand", "Move"));
    }
    if v.is_empty() {
        v.push(("SketchCreate", "Create Sketch"));
        v.push(("Extrude", "Extrude"));
        v.push(("PrimitiveBox", "Box"));
    }
    v
}

/// Open the menu at a screen position.
pub fn open(app: &mut SolveApp, at: Pos2) {
    app.viewport.context_menu = Some(at);
}

/// Show the menu if it is open; a pick or a click elsewhere closes it.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(at) = app.viewport.context_menu else { return };
    let mut pick: Option<String> = None;
    let mut other: Option<&'static str> = None;
    let resp = egui::Area::new(egui::Id::new("sc_context_menu")).fixed_pos(at).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(180.0);
            let item = |ui: &mut egui::Ui, label: &str| ui.add(egui::Button::new(label).frame(false).min_size(vec2(170.0, 22.0))).clicked();
            if let Some((id, label)) = app.last_command.clone() {
                if item(ui, &format!("Repeat {label}")) {
                    pick = Some(id);
                }
                ui.separator();
            }
            for (id, label) in for_selection(app) {
                if item(ui, label) {
                    pick = Some(id.to_string());
                }
            }
            ui.separator();
            if app.session.section.is_some() && item(ui, "Remove Section") {
                other = Some("unsection");
            }
            if !app.session.selection.is_empty() && item(ui, "Clear Selection") {
                other = Some("clear");
            }
            if item(ui, "Fit") {
                other = Some("fit");
            }
            if item(ui, "Home View") {
                other = Some("home");
            }
            if item(ui, "Undo") {
                other = Some("undo");
            }
        });
    });
    let clicked_outside = ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer();
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if pick.is_some() || other.is_some() || clicked_outside || esc {
        app.viewport.context_menu = None;
    }
    if let Some(id) = pick {
        app.start(&id);
    }
    match other {
        Some("clear") => drop(app.run("select.clear", json!({}))),
        Some("unsection") => drop(app.run("FusionHalfSectionViewCommand", json!({"clear": true}))),
        Some("fit") => app.animate_view("fit"),
        Some("home") => app.animate_view("home"),
        Some("undo") => drop(app.run("UndoCommand", json!({}))),
        _ => {}
    }
}
