//! Additive: New Setup from G-Code + Simulate Additive Toolpath dialogs.

use egui::RichText;
use serde_json::json;
use solidcraft_manufacture::{AdditiveKind, AdditiveOperation, Operation};

use crate::SolveApp;

#[derive(Clone, Debug)]
pub struct AdditiveDialogState {
    pub setup_name: String,
    pub op_index: usize,
    pub draft: AdditiveOperation,
}

#[derive(Clone, Debug, Default)]
pub struct FromGcodeState {
    pub source_path: String,
    pub gcode: String,
    pub setup_name: String,
}

pub fn open(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(Operation::Additive(op)) = setup.operations.get(op_index).cloned() else {
        return;
    };
    clear_other_dialogs(app);
    app.manufacture_workspace.selected_op = Some(op_index);
    app.manufacture_workspace.additive_dialog = Some(AdditiveDialogState { setup_name: setup_name.to_string(), op_index, draft: op });
    app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Operations;
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
}

pub fn open_from_gcode(app: &mut SolveApp) {
    clear_other_dialogs(app);
    app.manufacture_workspace.from_gcode_dialog = Some(FromGcodeState::default());
    app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
}

fn clear_other_dialogs(app: &mut SolveApp) {
    app.manufacture_workspace.drill_dialog = None;
    app.manufacture_workspace.contour_dialog = None;
    app.manufacture_workspace.twod_dialog = None;
    app.manufacture_workspace.threed_dialog = None;
    app.manufacture_workspace.hole_rec_dialog = None;
    app.manufacture_workspace.multiaxis_dialog = None;
    app.manufacture_workspace.turning_dialog = None;
    app.manufacture_workspace.part_handling_dialog = None;
    app.manufacture_workspace.inspection_dialog = None;
    app.manufacture_workspace.manual_inspection_dialog = None;
    app.manufacture_workspace.fabrication_dialog = None;
    app.manufacture_workspace.nest_dialog = None;
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    let _ = show_op_dialog(app, ui);
    show_from_gcode(app, ui);
}

fn show_from_gcode(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.from_gcode_dialog.clone() else {
        return;
    };
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    crate::frame::window(ui.ctx(), "New Setup from G-Code", crate::frame::Width::Normal)
        .id(egui::Id::new("mfg_additive_from_gcode"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .show(ui.ctx(), |ui| {
            ui.label("Import an FFF print file to create an Additive setup.");
            ui.horizontal(|ui| {
                ui.label("Setup name");
                ui.text_edit_singleline(&mut st.setup_name);
            });
            ui.horizontal(|ui| {
                ui.label("Source path");
                ui.text_edit_singleline(&mut st.source_path);
            });
            ui.label(RichText::new("G-code").strong());
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                ui.add(egui::TextEdit::multiline(&mut st.gcode).desired_width(f32::INFINITY).desired_rows(12).code_editor());
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
    if !open || cancel {
        app.manufacture_workspace.from_gcode_dialog = None;
        return;
    }
    if commit {
        let mut params = json!({ "gcode": st.gcode });
        if !st.setup_name.trim().is_empty() {
            params["name"] = json!(st.setup_name.trim());
        }
        if !st.source_path.trim().is_empty() {
            params["source_path"] = json!(st.source_path.trim());
        }
        match app.run("manufacture.setup.from_gcode", params) {
            Ok(v) => {
                if let Some(name) = v.get("setup").and_then(|s| s.as_str()) {
                    app.manufacture_workspace.active_setup = name.to_string();
                    app.manufacture_workspace.selected_op = Some(0);
                    self::open(app, name, 0);
                }
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.from_gcode_dialog = None;
                app.set_status("Additive setup imported from G-code", false);
            }
            Err(e) => app.set_status(e, true),
        }
        return;
    }
    app.manufacture_workspace.from_gcode_dialog = Some(st);
}

fn show_op_dialog(app: &mut SolveApp, ui: &mut egui::Ui) -> bool {
    let Some(state) = app.manufacture_workspace.additive_dialog.clone() else {
        return false;
    };
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    let mut draft = state.draft.clone();
    crate::frame::window(ui.ctx(), draft.kind.label(), crate::frame::Width::Normal)
        .id(egui::Id::new("mfg_additive_dialog"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(380.0)
        .show(ui.ctx(), |ui| {
            ui.label(RichText::new(draft.kind.label()).strong());
            match draft.kind {
                AdditiveKind::ImportedGcode => {
                    if !draft.source_path.is_empty() {
                        ui.label(format!("Source: {}", draft.source_path));
                    }
                    ui.label(format!("G-code bytes: {}", draft.gcode.len()));
                }
                AdditiveKind::GeneratedToolpath => {
                    ui.label("Generated layered FFF toolpath for simulation / post.");
                }
            }
            ui.separator();
            drag(ui, "Layer height", &mut draft.layer_height_mm, 0.01, 0.05..=2.0, " mm");
            ui.horizontal(|ui| {
                ui.label("Layers");
                ui.add(egui::DragValue::new(&mut draft.layer_count).speed(1).range(1..=512));
            });
            drag(ui, "Nozzle ⌀", &mut draft.nozzle_diameter_mm, 0.01, 0.1..=2.0, " mm");
            drag(ui, "Build size XY", &mut draft.size_xy_mm, 0.5, 1.0..=500.0, " mm");
            ui.horizontal(|ui| {
                ui.label("Print temp");
                ui.add(egui::DragValue::new(&mut draft.print_temp_c).suffix(" °C").range(0..=400));
            });
            ui.horizontal(|ui| {
                ui.label("Bed temp");
                ui.add(egui::DragValue::new(&mut draft.bed_temp_c).suffix(" °C").range(0..=200));
            });
            drag(ui, "Print feedrate", &mut draft.feedrate_mmpm, 1.0, 1.0..=30_000.0, " mm/min");
            drag(ui, "Travel feedrate", &mut draft.travel_mmpm, 1.0, 1.0..=50_000.0, " mm/min");
            ui.checkbox(&mut draft.relative_extrusion, "Relative extrusion mode");
            ui.separator();
            if ui.button("Simulate Additive Toolpath").clicked() {
                run_simulate(app, &state.setup_name);
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("OK").clicked() {
                        commit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
    if !open || cancel {
        app.manufacture_workspace.additive_dialog = None;
        return false;
    }
    if commit && save(app, &state.setup_name, state.op_index, draft.clone()) {
        app.manufacture_workspace.additive_dialog = None;
        return false;
    }
    if let Some(d) = &mut app.manufacture_workspace.additive_dialog {
        d.draft = draft;
    }
    true
}

pub fn run_simulate(app: &mut SolveApp, setup_name: &str) {
    let params = if setup_name.is_empty() { json!({}) } else { json!({"setup": setup_name}) };
    match app.run("manufacture.additive.simulate", params) {
        Ok(v) => {
            if let Some(segs) = v.get("segments").cloned()
                && let Ok(segments) = serde_json::from_value::<Vec<solidcraft_manufacture::PathSegment>>(segs)
            {
                app.manufacture_workspace.toolpath_segments = segments;
                app.manufacture_workspace.simulate_step = 0;
            }
            app.manufacture_workspace.tab = crate::manufacture_workspace::MfgTab::Simulate;
            app.manufacture_workspace.cam_mode = crate::manufacture_workspace::CamMode::Additive;
            let layers = v.get("layer_count").and_then(|n| n.as_u64()).unwrap_or(0);
            app.set_status(format!("Additive simulation ready ({layers} layers)"), false);
        }
        Err(e) => app.set_status(e, true),
    }
}

fn drag(ui: &mut egui::Ui, label: &str, v: &mut f64, speed: f64, range: std::ops::RangeInclusive<f64>, suffix: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(speed).range(range).suffix(suffix));
    });
}

fn save(app: &mut SolveApp, setup_name: &str, op_index: usize, draft: AdditiveOperation) -> bool {
    let Some(mut setup) = app.session.doc.manufacture.setups.get(setup_name).cloned() else {
        return false;
    };
    if op_index >= setup.operations.len() {
        return false;
    }
    setup.operations[op_index] = Operation::Additive(draft);
    match serde_json::to_value(&setup.operations) {
        Ok(ops) => match app.run("manufacture.settings.set", json!({"name": setup_name, "set": {"operations": ops}})) {
            Ok(_) => {
                app.manufacture_workspace.revision = u64::MAX;
                app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                true
            }
            Err(e) => {
                app.set_status(e, true);
                false
            }
        },
        Err(e) => {
            app.set_status(e.to_string(), true);
            false
        }
    }
}
