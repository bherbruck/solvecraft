//! Manufacture → INSPECT: Sustainability Insights dialog, plus a thin host for
//! inspect tools already implemented in the engine (Measure, Section, analyses).

use egui::RichText;
use serde_json::json;

use crate::SolveApp;

const PROCESSES: &[&str] = &["subtractive", "additive", "injection"];
const MATERIALS: &[&str] = &["Aluminum", "Steel", "Titanium", "ABS Plastic", "PLA", "Carbon Fiber Composite", "Unknown"];
const REGIONS: &[&str] = &["North America", "Europe", "China", "South America", "Other"];

#[derive(Clone, Debug)]
pub struct SustainabilityState {
    pub process: String,
    pub material: String,
    pub region: String,
    pub body_filter: String,
    pub result: Option<serde_json::Value>,
}

impl Default for SustainabilityState {
    fn default() -> Self {
        Self { process: "subtractive".into(), material: "Aluminum".into(), region: "North America".into(), body_filter: String::new(), result: None }
    }
}

pub fn open_sustainability(app: &mut SolveApp) {
    let mut st = SustainabilityState::default();
    if let Some(body) = app.session.world_state().bodies.first() {
        let mat = app.session.doc.materials.get(body.definition_name()).cloned().unwrap_or_default();
        if !mat.is_empty() {
            st.material = mat;
        }
    }
    st.result = run_estimate(app, &st);
    app.manufacture_workspace.sustainability_dialog = Some(st);
    if !crate::manufacture_workspace::active(app) {
        crate::manufacture_workspace::enter(app);
    }
}

fn run_estimate(app: &mut SolveApp, st: &SustainabilityState) -> Option<serde_json::Value> {
    let mut params = json!({
        "process": st.process,
        "material": st.material,
        "region": st.region,
    });
    if !st.body_filter.trim().is_empty() {
        let bodies: Vec<String> = st.body_filter.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if !bodies.is_empty() {
            params["bodies"] = json!(bodies);
        }
    }
    match app.run("inspect.sustainability", params) {
        Ok(v) => Some(v),
        Err(e) => {
            app.set_status(e, true);
            None
        }
    }
}

pub fn show_dialogs(app: &mut SolveApp, ui: &mut egui::Ui) {
    show_sustainability(app, ui);
}

fn show_sustainability(app: &mut SolveApp, ui: &mut egui::Ui) {
    let Some(mut st) = app.manufacture_workspace.sustainability_dialog.clone() else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let mut recalc = false;

    crate::frame::window(ui.ctx(), "Sustainability Insights", crate::frame::Width::Normal)
        .id(egui::Id::new("mfg_sustainability"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_width(420.0)
        .show(ui.ctx(), |ui| {
            ui.label(RichText::new("Cradle-to-gate carbon (illustrative)").strong());
            ui.small("Illustrative inputs: process, material, and region. Values are order-of-magnitude only.");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Process");
                egui::ComboBox::from_id_salt("msi_process").selected_text(process_label(&st.process)).show_ui(ui, |ui| {
                    for p in PROCESSES {
                        if ui.selectable_label(st.process == *p, process_label(p)).clicked() {
                            st.process = (*p).into();
                            recalc = true;
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Material");
                egui::ComboBox::from_id_salt("msi_material").selected_text(&st.material).show_ui(ui, |ui| {
                    for m in MATERIALS {
                        if ui.selectable_label(st.material == *m, *m).clicked() {
                            st.material = (*m).into();
                            recalc = true;
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Region");
                egui::ComboBox::from_id_salt("msi_region").selected_text(&st.region).show_ui(ui, |ui| {
                    for r in REGIONS {
                        if ui.selectable_label(st.region == *r, *r).clicked() {
                            st.region = (*r).into();
                            recalc = true;
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Bodies");
                if ui.text_edit_singleline(&mut st.body_filter).on_hover_text("Comma-separated body names; empty = all").changed() {
                    recalc = true;
                }
            });

            if ui.button("Recalculate").clicked() {
                recalc = true;
            }

            ui.separator();
            if let Some(r) = &st.result {
                let co2 = r["co2e_kg"].as_f64().unwrap_or(0.0);
                let vol = r["volume_cm3"].as_f64().unwrap_or(0.0);
                ui.label(RichText::new(format!("Estimated CO₂e: {co2:.3} kg")).strong());
                ui.label(format!("Part volume: {vol:.2} cm³"));
                ui.label(format!("Intensity: {:.4} kg CO₂e / cm³", r["intensity_kg_per_cm3"].as_f64().unwrap_or(0.0)));
                if let Some(note) = r["note"].as_str() {
                    ui.small(note);
                }
            } else {
                ui.label("No estimate yet — open a model with solid bodies.");
            }

            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        });

    if !open || close {
        app.manufacture_workspace.sustainability_dialog = None;
        return;
    }
    if recalc {
        st.result = run_estimate(app, &st);
    }
    app.manufacture_workspace.sustainability_dialog = Some(st);
}

fn process_label(p: &str) -> &'static str {
    match p {
        "subtractive" => "Subtractive (CNC)",
        "additive" => "Additive",
        "injection" => "Injection molding",
        _ => "Unknown",
    }
}
