//! Command dialogs (shown at the right of the viewport). A dialog collects inputs and picks,
//! then runs its command; it never changes the design itself.

use std::hash::{Hash, Hasher};

use egui::{Color32, RichText, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::doc::FeatureKind;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::theme::Tokens;
use crate::viewport::Hit;

const OPS: [&str; 4] = ["new", "join", "cut", "intersect"];
const OP_LABELS: [&str; 4] = ["New Body", "Join", "Cut", "Intersect"];
const DIRS: [&str; 3] = ["positive", "negative", "symmetric"];
const DIR_LABELS: [&str; 3] = ["One side", "Flip", "Symmetric"];

#[derive(Clone, Debug)]
pub enum Dialog {
    Sketch { pick_face: bool, picked: Option<Vec3> },
    Extrude { profiles: Vec<(u64, usize)>, distance: String, direction: usize, operation: usize },
    Revolve { profiles: Vec<(u64, usize)>, axis: String, angle: String, operation: usize },
    Fillet { edges: Vec<Vec3>, radius: String, chamfer: bool },
    Primitive { cmd: &'static str, fields: Vec<(&'static str, String)>, operation: usize },
    Combine { target: String, tools: Vec<String>, operation: usize },
    Params { new_name: String, new_expr: String },
    EditParam { name: String, expr: String, error: Option<String> },
}

/// Profiles of the sketch a feature would use (active, else the last), when there is just one.
fn default_profiles(s: &Session) -> Vec<(u64, usize)> {
    let st = s.model.state();
    let sid = s.active_sketch.or_else(|| s.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id));
    match sid.and_then(|id| st.sketch(id).map(|ss| (id, ss.profiles.len()))) {
        Some((id, 1)) => vec![(id, 0)],
        _ => Vec::new(),
    }
}

impl Dialog {
    pub fn for_command(app: &SolveApp, id: &str) -> Option<Dialog> {
        let s = &app.session;
        Some(match id {
            "SketchCreate" => Dialog::Sketch { pick_face: false, picked: None },
            "Extrude" => Dialog::Extrude {
                profiles: default_profiles(s),
                distance: "10 mm".into(),
                direction: 0,
                operation: if s.model.state().bodies.is_empty() { 0 } else { 1 },
            },
            "Revolve" => Dialog::Revolve { profiles: default_profiles(s), axis: "y".into(), angle: "360 deg".into(), operation: 0 },
            "FusionFilletEdgesCommand" => Dialog::Fillet { edges: selected_edges(s), radius: "2 mm".into(), chamfer: false },
            "FusionChamferCommand" => Dialog::Fillet { edges: selected_edges(s), radius: "1 mm".into(), chamfer: true },
            "PrimitiveBox" => Dialog::Primitive {
                cmd: "PrimitiveBox",
                fields: vec![("length", "20".into()), ("width", "20".into()), ("height", "20".into())],
                operation: 0,
            },
            "PrimitiveCylinder" => {
                Dialog::Primitive { cmd: "PrimitiveCylinder", fields: vec![("diameter", "20".into()), ("height", "20".into())], operation: 0 }
            }
            "PrimitiveSphere" => Dialog::Primitive { cmd: "PrimitiveSphere", fields: vec![("diameter", "20".into())], operation: 0 },
            "PrimitiveTorus" => {
                Dialog::Primitive { cmd: "PrimitiveTorus", fields: vec![("major", "20".into()), ("minor", "5".into())], operation: 0 }
            }
            "FusionCombineCommand" => {
                let bodies: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
                Dialog::Combine { target: bodies.first().cloned().unwrap_or_default(), tools: bodies.iter().skip(1).cloned().collect(), operation: 1 }
            }
            "ChangeParameterCommand" => Dialog::Params { new_name: String::new(), new_expr: String::new() },
            _ => return None,
        })
    }

    pub fn edit_param(s: &Session, name: &str) -> Dialog {
        Dialog::EditParam { name: name.into(), expr: s.doc.param(name).map(|p| p.expr.clone()).unwrap_or_default(), error: None }
    }

    pub fn wants_picks(&self) -> bool {
        matches!(self, Dialog::Extrude { .. } | Dialog::Revolve { .. } | Dialog::Fillet { .. } | Dialog::Sketch { pick_face: true, .. })
    }

    pub fn profiles(&self) -> Vec<(u64, usize)> {
        match self {
            Dialog::Extrude { profiles, .. } | Dialog::Revolve { profiles, .. } => profiles.clone(),
            _ => Vec::new(),
        }
    }

    pub fn edges(&self) -> Vec<Vec3> {
        match self {
            Dialog::Fillet { edges, .. } => edges.clone(),
            _ => Vec::new(),
        }
    }

    pub fn highlight_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.profiles().hash(&mut h);
        for e in self.edges() {
            for v in [e.x, e.y, e.z] {
                v.to_bits().hash(&mut h);
            }
        }
        h.finish()
    }

    pub fn on_pick(&mut self, hits: &[Hit]) {
        match self {
            Dialog::Extrude { profiles, .. } | Dialog::Revolve { profiles, .. } => {
                if let Some(Hit::Profile { sketch, index }) = hits.iter().find(|h| matches!(h, Hit::Profile { .. })) {
                    let k = (*sketch, *index);
                    if let Some(i) = profiles.iter().position(|p| *p == k) {
                        profiles.remove(i);
                    } else {
                        // Profiles from one sketch only.
                        profiles.retain(|p| p.0 == k.0);
                        profiles.push(k);
                    }
                }
            }
            Dialog::Fillet { edges, .. } => {
                if let Some(Hit::Edge { mid, .. }) = hits.iter().find(|h| matches!(h, Hit::Edge { .. })) {
                    if let Some(i) = edges.iter().position(|e| e.dist(*mid) < 1e-6) {
                        edges.remove(i);
                    } else {
                        edges.push(*mid);
                    }
                }
            }
            Dialog::Sketch { picked, .. } => {
                if let Some(Hit::Face { point, .. }) = hits.iter().find(|h| matches!(h, Hit::Face { .. })) {
                    *picked = Some(*point);
                }
            }
            _ => {}
        }
    }
}

fn selected_edges(s: &Session) -> Vec<Vec3> {
    s.selection.iter().filter_map(|x| if let solvecraft_engine::Sel::Edge { point, .. } = x { Some(*point) } else { None }).collect()
}

fn combo(ui: &mut egui::Ui, id: &str, labels: &[&str], sel: &mut usize) {
    egui::ComboBox::from_id_salt(id).selected_text(labels.get(*sel).copied().unwrap_or("")).width(150.0).show_ui(ui, |ui| {
        for (i, l) in labels.iter().enumerate() {
            ui.selectable_value(sel, i, *l);
        }
    });
}

fn profile_param(p: &[(u64, usize)]) -> (Value, Value) {
    let sketch = p.first().map(|x| json!(x.0)).unwrap_or(Value::Null);
    (sketch, json!(p.iter().map(|x| x.1).collect::<Vec<_>>()))
}

/// Show the active dialog (if any).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let t = Tokens::get();
    let title = match &d {
        Dialog::Sketch { .. } => "CREATE SKETCH",
        Dialog::Extrude { .. } => "EXTRUDE",
        Dialog::Revolve { .. } => "REVOLVE",
        Dialog::Fillet { chamfer: false, .. } => "FILLET",
        Dialog::Fillet { chamfer: true, .. } => "CHAMFER",
        Dialog::Primitive { cmd, .. } => match *cmd {
            "PrimitiveBox" => "BOX",
            "PrimitiveCylinder" => "CYLINDER",
            "PrimitiveSphere" => "SPHERE",
            _ => "TORUS",
        },
        Dialog::Combine { .. } => "COMBINE",
        Dialog::Params { .. } => "PARAMETERS",
        Dialog::EditParam { .. } => "EDIT DIMENSION",
    };
    let pos = app.viewport.rect.map(|r| egui::pos2(r.right() - 330.0, r.top() + 150.0)).unwrap_or(egui::pos2(900.0, 200.0));
    let mut keep = true;
    let mut ok = false;
    let mut cancel = false;
    let wide = matches!(d, Dialog::Params { .. });
    egui::Window::new(RichText::new(title).strong().size(13.0))
        .id(egui::Id::new("sc_dialog"))
        .default_pos(if wide { egui::pos2(pos.x - 260.0, pos.y - 60.0) } else { pos })
        .resizable(false)
        .collapsible(false)
        .open(&mut keep)
        .show(ctx, |ui| {
            ui.set_min_width(if wide { 520.0 } else { 280.0 });
            egui::Grid::new("sc_dialog_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| match &mut d {
                Dialog::Sketch { pick_face, .. } => {
                    ui.label("Plane");
                    ui.horizontal(|ui| {
                        for p in ["XY", "XZ", "YZ"] {
                            if ui.button(p).clicked() {
                                if app.run("SketchCreate", json!({"plane": p})).is_ok() {
                                    look_at_sketch(app);
                                }
                                cancel = true;
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Face");
                    ui.toggle_value(pick_face, "Pick a planar face");
                    ui.end_row();
                }
                Dialog::Extrude { profiles, distance, direction, operation } => {
                    ui.label("Profiles");
                    ui.label(if profiles.is_empty() {
                        RichText::new("click profiles in the view").color(t.warning)
                    } else {
                        RichText::new(format!("{} selected", profiles.len()))
                    });
                    ui.end_row();
                    ui.label("Direction");
                    combo(ui, "ex_dir", &DIR_LABELS, direction);
                    ui.end_row();
                    ui.label("Distance");
                    ui.text_edit_singleline(distance);
                    ui.end_row();
                    ui.label("Operation");
                    combo(ui, "ex_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Dialog::Revolve { profiles, axis, angle, operation } => {
                    ui.label("Profiles");
                    ui.label(if profiles.is_empty() {
                        RichText::new("click profiles in the view").color(t.warning)
                    } else {
                        RichText::new(format!("{} selected", profiles.len()))
                    });
                    ui.end_row();
                    ui.label("Axis");
                    ui.text_edit_singleline(axis).on_hover_text("sketch line id, x / y (sketch axes) or X / Y / Z");
                    ui.end_row();
                    ui.label("Angle");
                    ui.text_edit_singleline(angle);
                    ui.end_row();
                    ui.label("Operation");
                    combo(ui, "rv_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Dialog::Fillet { edges, radius, chamfer } => {
                    ui.label("Edges");
                    ui.label(if edges.is_empty() {
                        RichText::new("click edges in the view").color(t.warning)
                    } else {
                        RichText::new(format!("{} selected", edges.len()))
                    });
                    ui.end_row();
                    ui.label(if *chamfer { "Distance" } else { "Radius" });
                    ui.text_edit_singleline(radius);
                    ui.end_row();
                }
                Dialog::Primitive { fields, operation, .. } => {
                    for (k, v) in fields.iter_mut() {
                        ui.label(*k);
                        ui.text_edit_singleline(v);
                        ui.end_row();
                    }
                    ui.label("Operation");
                    combo(ui, "pr_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Dialog::Combine { target, tools, operation } => {
                    let bodies: Vec<String> = app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect();
                    ui.label("Target body");
                    egui::ComboBox::from_id_salt("cb_target").selected_text(target.clone()).show_ui(ui, |ui| {
                        for b in &bodies {
                            ui.selectable_value(target, b.clone(), b);
                        }
                    });
                    ui.end_row();
                    ui.label("Tool bodies");
                    ui.vertical(|ui| {
                        for b in bodies.iter().filter(|b| *b != target) {
                            let mut on = tools.contains(b);
                            if ui.checkbox(&mut on, b).changed() {
                                if on {
                                    tools.push(b.clone());
                                } else {
                                    tools.retain(|x| x != b);
                                }
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Operation");
                    let mut op = operation.saturating_sub(1);
                    combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                    *operation = op + 1;
                    ui.end_row();
                }
                Dialog::Params { new_name, new_expr } => {
                    ui.label(RichText::new("Name").strong());
                    ui.label(RichText::new("Expression  ·  value").strong());
                    ui.end_row();
                    let (vals, errs) = app.session.doc.param_values();
                    let params = app.session.doc.params.clone();
                    for p in params {
                        ui.label(if p.model { RichText::new(&p.name).color(t.text_dim) } else { RichText::new(&p.name) });
                        ui.horizontal(|ui| {
                            let id = egui::Id::new(("pexpr", &p.name));
                            let mut e = ui.data(|dd| dd.get_temp::<String>(id)).unwrap_or_else(|| p.expr.clone());
                            let r = ui.add(egui::TextEdit::singleline(&mut e).desired_width(160.0));
                            if r.changed() {
                                ui.data_mut(|dd| dd.insert_temp(id, e.clone()));
                            }
                            if r.lost_focus() && e != p.expr {
                                let _ = app.run("ChangeParameterCommand", json!({"name": p.name, "expression": e}));
                                ui.data_mut(|dd| dd.remove::<String>(id));
                            }
                            let v = vals
                                .get(&p.name)
                                .map(|v| if p.unit == "deg" { format!("{:.3}°", v.v.to_degrees()) } else { format!("{:.4} {}", v.v, p.unit) });
                            match (v, errs.get(&p.name)) {
                                (_, Some(e)) => ui.label(RichText::new(e).color(t.error)),
                                (Some(v), None) => ui.label(RichText::new(v).color(t.text_dim)),
                                _ => ui.label(""),
                            };
                        });
                        ui.end_row();
                    }
                    ui.add(egui::TextEdit::singleline(new_name).hint_text("new name").desired_width(110.0));
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(new_expr).hint_text("expression, e.g. 25 mm").desired_width(160.0));
                        if ui.button("Add").clicked()
                            && !new_name.is_empty()
                            && app.run("ChangeParameterCommand", json!({"name": new_name, "expression": new_expr})).is_ok()
                        {
                            new_name.clear();
                            new_expr.clear();
                        }
                    });
                    ui.end_row();
                }
                Dialog::EditParam { name, expr, error } => {
                    ui.label(name.as_str());
                    let r = ui.text_edit_singleline(expr);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    r.request_focus();
                    ui.end_row();
                    if let Some(e) = error {
                        ui.label("");
                        ui.label(RichText::new(e.as_str()).color(t.error));
                        ui.end_row();
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !matches!(d, Dialog::Sketch { .. } | Dialog::Params { .. })
                    && ui.add(egui::Button::new(RichText::new("OK").color(Color32::WHITE)).fill(t.accent).min_size(vec2(70.0, 24.0))).clicked()
                {
                    ok = true;
                }
                if ui.add(egui::Button::new(if matches!(d, Dialog::Params { .. }) { "Close" } else { "Cancel" }).min_size(vec2(70.0, 24.0))).clicked()
                {
                    cancel = true;
                }
            });
        });
    if ok {
        let r = run_dialog(app, &d);
        match r {
            Ok(()) => cancel = true,
            Err(e) => {
                if let Dialog::EditParam { error, .. } = &mut d {
                    *error = Some(e);
                }
            }
        }
    }
    // A face picked in the view while the sketch dialog waits for one.
    if let Dialog::Sketch { picked: Some(point), .. } = d {
        if app.run("SketchCreate", json!({"plane": {"face": [point.x, point.y, point.z]}})).is_ok() {
            look_at_sketch(app);
        }
        cancel = true;
    }
    if keep && !cancel {
        app.dialog = Some(d);
    }
}

fn look_at_sketch(app: &mut SolveApp) {
    let st = app.session.model.state();
    if let Some(ss) = app.session.active_sketch.and_then(|id| st.sketch(id)) {
        let n = ss.plane.normal();
        let yaw = (-n.x).atan2(-n.y);
        let pitch = n.z.clamp(-1.0, 1.0).asin();
        app.cam.yaw = if n.z.abs() > 0.999 { 0.0 } else { yaw };
        app.cam.pitch = pitch.clamp(-std::f64::consts::FRAC_PI_2 + 1e-4, std::f64::consts::FRAC_PI_2 - 1e-4);
        app.cam.target = ss.plane.origin;
    }
}

fn run_dialog(app: &mut SolveApp, d: &Dialog) -> Result<(), String> {
    match d {
        Dialog::Extrude { profiles, distance, direction, operation } => {
            if profiles.is_empty() {
                return Err("select profiles first".into());
            }
            let (sketch, idx) = profile_param(profiles);
            app.run(
                "Extrude",
                json!({"sketch": sketch, "profiles": idx, "distance": distance, "direction": DIRS.get(*direction).copied().unwrap_or("positive"), "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
            .map(drop)
        }
        Dialog::Revolve { profiles, axis, angle, operation } => {
            if profiles.is_empty() {
                return Err("select profiles first".into());
            }
            let (sketch, idx) = profile_param(profiles);
            app.run(
                "Revolve",
                json!({"sketch": sketch, "profiles": idx, "axis": axis, "angle": angle, "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
            .map(drop)
        }
        Dialog::Fillet { edges, radius, chamfer } => {
            if edges.is_empty() {
                return Err("select edges first".into());
            }
            let pts: Vec<[f64; 3]> = edges.iter().map(|e| [e.x, e.y, e.z]).collect();
            if *chamfer {
                app.run("FusionChamferCommand", json!({"edges": pts, "distance": radius})).map(drop)
            } else {
                app.run("FusionFilletEdgesCommand", json!({"edges": pts, "radius": radius})).map(drop)
            }
        }
        Dialog::Primitive { cmd, fields, operation } => {
            let mut p = serde_json::Map::new();
            for (k, v) in fields {
                p.insert((*k).into(), json!(v));
            }
            p.insert("operation".into(), json!(OPS.get(*operation).copied().unwrap_or("new")));
            app.run(cmd, Value::Object(p)).map(drop)
        }
        Dialog::Combine { target, tools, operation } => app
            .run("FusionCombineCommand", json!({"target": target, "tools": tools, "operation": OPS.get(*operation).copied().unwrap_or("join")}))
            .map(drop),
        Dialog::EditParam { name, expr, .. } => app.run("ChangeParameterCommand", json!({"name": name, "expression": expr})).map(drop),
        Dialog::Sketch { .. } | Dialog::Params { .. } => Ok(()),
    }
}
