//! Command dialogs (shown at the right of the viewport). A dialog collects values and selection
//! inputs (geometry picked in the viewport, never typed), then runs its command; it never
//! changes the design itself. Things selected before the command starts become its input.

use std::hash::{Hash, Hasher};

use egui::{Color32, RichText, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::Session;
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::selection::{self, AXES, Accept, BODIES, EDGES, FACES, PLANAR_FACES, PLANES, PROFILES, SelInput};
use crate::theme::Tokens;
use crate::viewport::Hit;

const OPS: [&str; 4] = ["new", "join", "cut", "intersect"];
const OP_LABELS: [&str; 4] = ["New Body", "Join", "Cut", "Intersect"];
const DIRS: [&str; 3] = ["positive", "negative", "symmetric"];
const DIR_LABELS: [&str; 3] = ["One side", "Flip", "Symmetric"];
const HOLE_TYPES: [&str; 3] = ["simple", "counterbore", "countersink"];
const HOLE_LABELS: [&str; 3] = ["Simple", "Counterbore", "Countersink"];

#[derive(Clone, Debug)]
pub enum Kind {
    /// Pick a plane or planar face; the sketch starts on it at once.
    Sketch,
    Extrude {
        distance: String,
        direction: usize,
        operation: usize,
    },
    Revolve {
        angle: String,
        operation: usize,
    },
    Fillet {
        radius: String,
        chamfer: bool,
        chain: bool,
    },
    Shell {
        thickness: String,
    },
    Draft {
        angle: String,
    },
    Mirror,
    Hole {
        diameter: String,
        depth: String,
        kind: usize,
    },
    Primitive {
        cmd: &'static str,
        fields: Vec<(&'static str, String)>,
        operation: usize,
    },
    Combine {
        operation: usize,
        keep_tools: bool,
    },
    Params {
        new_name: String,
        new_expr: String,
    },
    EditParam {
        name: String,
        expr: String,
    },
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub kind: Kind,
    /// Selection inputs, in order; clicks in the viewport go to the active one.
    pub inputs: Vec<SelInput>,
    pub active: usize,
    pub error: Option<String>,
}

/// Can a selection go into an input that accepts `a`? (Planar-ness is checked when picking.)
fn fits(a: Accept, s: &Sel) -> bool {
    match s {
        Sel::Profile { .. } => a & PROFILES != 0,
        Sel::Edge { .. } => a & EDGES != 0,
        Sel::Face { .. } => a & (FACES | PLANAR_FACES) != 0,
        Sel::Body { .. } => a & BODIES != 0,
        Sel::Plane { .. } => a & PLANES != 0,
        Sel::Axis { .. } | Sel::SketchCurve { .. } => a & AXES != 0,
        Sel::Vertex { .. } => a & selection::VERTICES != 0,
        Sel::Feature { .. } | Sel::SketchPoint { .. } => false,
    }
}

impl Dialog {
    fn new(kind: Kind, inputs: Vec<SelInput>) -> Dialog {
        Dialog { kind, inputs, active: 0, error: None }
    }

    pub fn for_command(app: &SolveApp, id: &str) -> Option<Dialog> {
        let s = &app.session;
        let has_bodies = !s.model.state().bodies.is_empty();
        let mut d = match id {
            "SketchCreate" => Dialog::new(Kind::Sketch, vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)]),
            "Extrude" => Dialog::new(
                Kind::Extrude { distance: "10 mm".into(), direction: 0, operation: usize::from(has_bodies) },
                vec![SelInput::new("Profiles", PROFILES, true)],
            ),
            "Revolve" => Dialog::new(
                Kind::Revolve { angle: "360 deg".into(), operation: 0 },
                vec![SelInput::new("Profiles", PROFILES, true), SelInput::new("Axis", AXES, false)],
            ),
            "FusionFilletEdgesCommand" => {
                Dialog::new(Kind::Fillet { radius: "2 mm".into(), chamfer: false, chain: true }, vec![SelInput::new("Edges", EDGES | FACES, true)])
            }
            "FusionChamferCommand" => {
                Dialog::new(Kind::Fillet { radius: "1 mm".into(), chamfer: true, chain: true }, vec![SelInput::new("Edges", EDGES | FACES, true)])
            }
            "FusionShellBodyCommand" => Dialog::new(Kind::Shell { thickness: "2 mm".into() }, vec![SelInput::new("Faces", FACES, true)]),
            "FusionDraftCommand" => Dialog::new(
                Kind::Draft { angle: "5 deg".into() },
                vec![SelInput::new("Faces", FACES, true), SelInput::new("Neutral plane", PLANES | PLANAR_FACES, false)],
            ),
            "MirrorCommand" => {
                Dialog::new(Kind::Mirror, vec![SelInput::new("Bodies", BODIES, true), SelInput::new("Mirror plane", PLANES | PLANAR_FACES, false)])
            }
            "FusionHoleCommand" => {
                Dialog::new(Kind::Hole { diameter: "5 mm".into(), depth: String::new(), kind: 0 }, vec![SelInput::new("Position", FACES, true)])
            }
            "PrimitiveBox" => Dialog::new(
                Kind::Primitive {
                    cmd: "PrimitiveBox",
                    fields: vec![("length", "20".into()), ("width", "20".into()), ("height", "20".into())],
                    operation: 0,
                },
                vec![],
            ),
            "PrimitiveCylinder" => Dialog::new(
                Kind::Primitive { cmd: "PrimitiveCylinder", fields: vec![("diameter", "20".into()), ("height", "20".into())], operation: 0 },
                vec![],
            ),
            "PrimitiveSphere" => {
                Dialog::new(Kind::Primitive { cmd: "PrimitiveSphere", fields: vec![("diameter", "20".into())], operation: 0 }, vec![])
            }
            "PrimitiveTorus" => Dialog::new(
                Kind::Primitive { cmd: "PrimitiveTorus", fields: vec![("major", "20".into()), ("minor", "5".into())], operation: 0 },
                vec![],
            ),
            "FusionCombineCommand" => Dialog::new(
                Kind::Combine { operation: 1, keep_tools: false },
                vec![SelInput::new("Target body", BODIES, false), SelInput::new("Tool bodies", BODIES, true)],
            ),
            "ChangeParameterCommand" => Dialog::new(Kind::Params { new_name: String::new(), new_expr: String::new() }, vec![]),
            _ => return None,
        };
        // Pre-selection: what is selected now becomes the input (first input that takes it).
        for sel in &s.selection {
            if let Some(inp) = d.inputs.iter_mut().find(|i| fits(i.accept, sel) && (i.multi || i.items.is_empty())) {
                inp.items.push(sel.clone());
            }
        }
        // With nothing pre-selected, a single profile in the sketch being used is taken.
        if matches!(d.kind, Kind::Extrude { .. } | Kind::Revolve { .. })
            && let Some(inp) = d.inputs.first_mut()
            && inp.items.is_empty()
        {
            inp.items = default_profiles(s);
        }
        d.advance();
        Some(d)
    }

    pub fn edit_param(s: &Session, name: &str) -> Dialog {
        Dialog::new(Kind::EditParam { name: name.into(), expr: s.doc.param(name).map(|p| p.expr.clone()).unwrap_or_default() }, vec![])
    }

    /// Make the first single input still empty (after the filled ones) active.
    fn advance(&mut self) {
        if let Some(i) = self.inputs.iter().position(|i| i.items.is_empty()) {
            self.active = i;
        } else {
            self.active = self.active.min(self.inputs.len().saturating_sub(1));
        }
    }

    pub fn wants_picks(&self) -> bool {
        !self.inputs.is_empty()
    }

    pub fn active_input(&self) -> Option<&SelInput> {
        self.inputs.get(self.active)
    }

    /// Everything the inputs hold (for highlighting).
    pub fn items(&self) -> Vec<Sel> {
        self.inputs.iter().flat_map(|i| i.items.iter().cloned()).collect()
    }

    pub fn highlight_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&self.items()).unwrap_or_default().hash(&mut h);
        self.active.hash(&mut h);
        h.finish()
    }

    /// The selection a hit would add to the active input.
    pub fn candidate(&self, s: &Session, h: &Hit) -> Option<Sel> {
        self.active_input()?.accepts(h, |body, face| planar_face(s, body, face).is_some())
    }

    /// A pick in the viewport (already accepted by the active input).
    pub fn pick(&mut self, s: &Session, sel: Sel) {
        let chain = matches!(self.kind, Kind::Fillet { chain: true, .. });
        let mut picked = vec![sel.clone()];
        // Tangent chain: an edge brings the edges that continue it smoothly.
        if chain
            && let Sel::Edge { body, index, .. } = &sel
            && let Some(b) = s.model.state().body(body)
        {
            let m = b.mesh();
            picked = m
                .tangent_chain(*index, 2f64.to_radians())
                .into_iter()
                .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                .collect();
        }
        // Extrude and revolve take profiles of one sketch.
        if let Sel::Profile { sketch, .. } = &sel
            && let Some(inp) = self.inputs.get_mut(self.active)
        {
            inp.items.retain(|x| matches!(x, Sel::Profile { sketch: s2, .. } if s2 == sketch));
        }
        let Some(inp) = self.inputs.get_mut(self.active) else { return };
        inp.toggle(picked);
        if !inp.multi && !inp.items.is_empty() && self.active + 1 < self.inputs.len() {
            self.advance();
        }
        self.error = None;
    }

    /// Box selection result: replace (or add to) the active input.
    pub fn take_box(&mut self, sels: Vec<Sel>, add: bool) {
        let Some(inp) = self.inputs.get_mut(self.active) else { return };
        if !add {
            inp.items.clear();
        }
        for x in sels {
            if fits(inp.accept, &x) && !inp.items.contains(&x) {
                inp.items.push(x);
            }
        }
    }
}

/// Profiles of the sketch a feature would use (active, else the last), when there is just one.
fn default_profiles(s: &Session) -> Vec<Sel> {
    use solvecraft_engine::doc::FeatureKind;
    let st = s.model.state();
    let sid = s.active_sketch.or_else(|| s.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id));
    match sid.and_then(|id| st.sketch(id).map(|ss| (id, ss.profiles.len()))) {
        Some((id, 1)) => vec![Sel::Profile { sketch: id, index: 0 }],
        _ => Vec::new(),
    }
}

/// The plane of a planar body face: (point on it, unit normal).
pub fn planar_face(s: &Session, body: &str, face: usize) -> Option<(Vec3, Vec3)> {
    let st = s.model.state();
    let b = st.body(body)?;
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let f = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == face)?;
    Some((f.centroid, f.plane_normal?))
}

fn polyline_mid(pts: &[Vec3]) -> Vec3 {
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= len * 0.5 && l > 0.0 {
            return w[0].lerp(w[1], (len * 0.5 - acc) / l);
        }
        acc += l;
    }
    pts.first().copied().unwrap_or_default()
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A plane input as a command parameter: an origin/construction plane name or a face plane.
fn plane_value(s: &Session, sel: Option<&Sel>) -> Option<Value> {
    match sel? {
        Sel::Plane { name } => Some(json!(name)),
        Sel::Face { body, index, point } => {
            let (_, n) = planar_face(s, body, *index)?;
            Some(json!({"origin": pt(*point), "normal": pt(n)}))
        }
        _ => None,
    }
}

fn combo(ui: &mut egui::Ui, id: &str, labels: &[&str], sel: &mut usize) {
    egui::ComboBox::from_id_salt(id).selected_text(labels.get(*sel).copied().unwrap_or("")).width(150.0).show_ui(ui, |ui| {
        for (i, l) in labels.iter().enumerate() {
            ui.selectable_value(sel, i, *l);
        }
    });
}

fn title(k: &Kind) -> &'static str {
    match k {
        Kind::Sketch => "CREATE SKETCH",
        Kind::Extrude { .. } => "EXTRUDE",
        Kind::Revolve { .. } => "REVOLVE",
        Kind::Fillet { chamfer: false, .. } => "FILLET",
        Kind::Fillet { chamfer: true, .. } => "CHAMFER",
        Kind::Shell { .. } => "SHELL",
        Kind::Draft { .. } => "DRAFT",
        Kind::Mirror => "MIRROR",
        Kind::Hole { .. } => "HOLE",
        Kind::Primitive { cmd, .. } => match *cmd {
            "PrimitiveBox" => "BOX",
            "PrimitiveCylinder" => "CYLINDER",
            "PrimitiveSphere" => "SPHERE",
            _ => "TORUS",
        },
        Kind::Combine { .. } => "COMBINE",
        Kind::Params { .. } => "PARAMETERS",
        Kind::EditParam { .. } => "EDIT DIMENSION",
    }
}

fn hint(inp: &SelInput) -> &'static str {
    let a = inp.accept;
    if a & PROFILES != 0 {
        "click profiles in the view"
    } else if a & EDGES != 0 {
        "click edges or faces"
    } else if a & PLANES != 0 {
        "click a plane or planar face"
    } else if a & AXES != 0 {
        "click an axis or sketch line"
    } else if a & BODIES != 0 {
        "click bodies"
    } else {
        "click faces"
    }
}

/// Selection input rows: label, "N selected" (or a hint) and a clear button; clicking a row
/// makes it the active input.
fn input_rows(d: &mut Dialog, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut clear: Option<usize> = None;
    let mut activate: Option<usize> = None;
    for (i, inp) in d.inputs.iter().enumerate() {
        ui.label(inp.label);
        ui.horizontal(|ui| {
            let active = i == d.active;
            let text = if inp.items.is_empty() {
                RichText::new(hint(inp)).color(if active { t.warning } else { t.text_dim })
            } else {
                RichText::new(format!("{} selected", inp.items.len())).color(t.text)
            };
            let b = egui::Button::new(text).fill(if active { t.accent_soft } else { Color32::TRANSPARENT }).min_size(vec2(170.0, 22.0));
            if ui.add(b).on_hover_text("Click to make this the input that viewport picks go to").clicked() {
                activate = Some(i);
            }
            if !inp.items.is_empty() && ui.small_button("×").on_hover_text("Clear the selection").clicked() {
                clear = Some(i);
            }
        });
        ui.end_row();
    }
    if let Some(i) = activate {
        d.active = i;
    }
    if let Some(i) = clear
        && let Some(inp) = d.inputs.get_mut(i)
    {
        inp.items.clear();
        d.active = i;
    }
}

/// Show the active dialog (if any).
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    let t = Tokens::get();
    let pos = app.viewport.rect.map(|r| egui::pos2(r.right() - 330.0, r.top() + 150.0)).unwrap_or(egui::pos2(900.0, 200.0));
    let mut keep = true;
    let mut ok = false;
    let mut cancel = false;
    let wide = matches!(d.kind, Kind::Params { .. });
    egui::Window::new(RichText::new(title(&d.kind)).strong().size(13.0))
        .id(egui::Id::new("sc_dialog"))
        .default_pos(if wide { egui::pos2(pos.x - 260.0, pos.y - 60.0) } else { pos })
        .resizable(false)
        .collapsible(false)
        .open(&mut keep)
        .show(ctx, |ui| {
            ui.set_min_width(if wide { 520.0 } else { 280.0 });
            egui::Grid::new("sc_dialog_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                input_rows(&mut d, ui);
                match &mut d.kind {
                    Kind::Sketch => {}
                    Kind::Extrude { distance, direction, operation } => {
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
                    Kind::Revolve { angle, operation } => {
                        ui.label("Angle");
                        ui.text_edit_singleline(angle);
                        ui.end_row();
                        ui.label("Operation");
                        combo(ui, "rv_op", &OP_LABELS, operation);
                        ui.end_row();
                    }
                    Kind::Fillet { radius, chamfer, chain } => {
                        ui.label(if *chamfer { "Distance" } else { "Radius" });
                        ui.text_edit_singleline(radius);
                        ui.end_row();
                        ui.label("Tangent chain");
                        ui.checkbox(chain, "");
                        ui.end_row();
                    }
                    Kind::Shell { thickness } => {
                        ui.label("Inside thickness");
                        ui.text_edit_singleline(thickness);
                        ui.end_row();
                    }
                    Kind::Draft { angle } => {
                        ui.label("Angle");
                        ui.text_edit_singleline(angle);
                        ui.end_row();
                    }
                    Kind::Mirror => {}
                    Kind::Hole { diameter, depth, kind } => {
                        ui.label("Type");
                        combo(ui, "hole_kind", &HOLE_LABELS, kind);
                        ui.end_row();
                        ui.label("Diameter");
                        ui.text_edit_singleline(diameter);
                        ui.end_row();
                        ui.label("Depth");
                        ui.add(egui::TextEdit::singleline(depth).hint_text("through all"));
                        ui.end_row();
                    }
                    Kind::Primitive { fields, operation, .. } => {
                        for (k, v) in fields.iter_mut() {
                            ui.label(*k);
                            ui.text_edit_singleline(v);
                            ui.end_row();
                        }
                        ui.label("Operation");
                        combo(ui, "pr_op", &OP_LABELS, operation);
                        ui.end_row();
                    }
                    Kind::Combine { operation, keep_tools } => {
                        ui.label("Operation");
                        let mut op = operation.saturating_sub(1);
                        combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                        *operation = op + 1;
                        ui.end_row();
                        ui.label("Keep tools");
                        ui.checkbox(keep_tools, "");
                        ui.end_row();
                    }
                    Kind::Params { new_name, new_expr } => params_table(app, ui, new_name, new_expr),
                    Kind::EditParam { name, expr } => {
                        ui.label(name.as_str());
                        let r = ui.text_edit_singleline(expr);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                        r.request_focus();
                        ui.end_row();
                    }
                }
                if let Some(e) = &d.error {
                    ui.label("");
                    ui.label(RichText::new(e.as_str()).color(t.error));
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !matches!(d.kind, Kind::Sketch | Kind::Params { .. })
                    && ui.add(egui::Button::new(RichText::new("OK").color(Color32::WHITE)).fill(t.accent).min_size(vec2(70.0, 24.0))).clicked()
                {
                    ok = true;
                }
                let close = if matches!(d.kind, Kind::Params { .. }) { "Close" } else { "Cancel" };
                if ui.add(egui::Button::new(close).min_size(vec2(70.0, 24.0))).clicked() {
                    cancel = true;
                }
            });
        });
    // A plane picked for a new sketch starts it right away.
    if matches!(d.kind, Kind::Sketch)
        && let Some(sel) = d.inputs.first().and_then(|i| i.items.first()).cloned()
    {
        start_sketch(app, &sel);
        cancel = true;
    }
    if ok {
        match run_dialog(app, &d) {
            Ok(()) => cancel = true,
            Err(e) => d.error = Some(e),
        }
    }
    if keep && !cancel {
        app.dialog = Some(d);
    }
}

fn params_table(app: &mut SolveApp, ui: &mut egui::Ui, new_name: &mut String, new_expr: &mut String) {
    let t = Tokens::get();
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
            let v = vals.get(&p.name).map(|v| if p.unit == "deg" { format!("{:.3}°", v.v.to_degrees()) } else { format!("{:.4} {}", v.v, p.unit) });
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

/// Start a sketch on a picked plane or planar face and turn the view to face it.
pub fn start_sketch(app: &mut SolveApp, sel: &Sel) {
    let plane = match sel {
        Sel::Plane { name } => json!(name),
        Sel::Face { point, .. } => json!({"face": pt(*point)}),
        _ => return,
    };
    let before = app.cam;
    if app.run("SketchCreate", json!({ "plane": plane })).is_ok() {
        app.pre_sketch_cam = Some(before);
        look_at_sketch(app);
    }
}

/// Animate the camera to look straight at the active sketch plane.
pub fn look_at_sketch(app: &mut SolveApp) {
    let st = app.session.model.state();
    if let Some(ss) = app.session.active_sketch.and_then(|id| st.sketch(id)) {
        let mut to = app.cam.looking_from(ss.plane.normal());
        // Keep the sketch's own x axis to the right when looking straight down or up.
        if ss.plane.normal().z.abs() > 0.999 {
            let x = ss.plane.x;
            to.yaw = if ss.plane.normal().z > 0.0 { (-x.y).atan2(x.x) } else { x.y.atan2(x.x) };
        }
        to.target = ss.plane.origin;
        app.animate_to(to);
    }
}

fn sels(d: &Dialog, i: usize) -> &[Sel] {
    d.inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

fn run_dialog(app: &mut SolveApp, d: &Dialog) -> Result<(), String> {
    let s = &app.session;
    let need = |i: usize, what: &str| -> Result<(), String> { if sels(d, i).is_empty() { Err(format!("select {what} first")) } else { Ok(()) } };
    let profiles = || -> (Value, Value) {
        let ps: Vec<(u64, usize)> =
            sels(d, 0).iter().filter_map(|x| if let Sel::Profile { sketch, index } = x { Some((*sketch, *index)) } else { None }).collect();
        (ps.first().map(|x| json!(x.0)).unwrap_or(Value::Null), json!(ps.iter().map(|x| x.1).collect::<Vec<_>>()))
    };
    let body_names =
        |i: usize| -> Vec<String> { sels(d, i).iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect() };
    let face_points = |i: usize| -> Vec<Value> {
        sels(d, i).iter().filter_map(|x| if let Sel::Face { point, .. } = x { Some(pt(*point)) } else { None }).collect()
    };
    let (cmd, params): (&str, Value) = match &d.kind {
        Kind::Extrude { distance, direction, operation } => {
            need(0, "profiles")?;
            let (sketch, idx) = profiles();
            (
                "Extrude",
                json!({"sketch": sketch, "profiles": idx, "distance": distance, "direction": DIRS.get(*direction).copied().unwrap_or("positive"), "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
        }
        Kind::Revolve { angle, operation } => {
            need(0, "profiles")?;
            need(1, "an axis")?;
            let axis = match sels(d, 1).first() {
                Some(Sel::Axis { name }) => json!(name),
                Some(Sel::SketchCurve { id }) => json!(id),
                _ => return Err("select an axis".into()),
            };
            let (sketch, idx) = profiles();
            (
                "Revolve",
                json!({"sketch": sketch, "profiles": idx, "axis": axis, "angle": angle, "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
        }
        Kind::Fillet { radius, chamfer, .. } => {
            need(0, "edges")?;
            // Faces stand for all their edges.
            let st = s.model.state();
            let mut pts: Vec<Vec3> = Vec::new();
            for x in sels(d, 0) {
                match x {
                    Sel::Edge { point, .. } => pts.push(*point),
                    Sel::Face { body, index, .. } => {
                        if let Some(b) = st.body(body) {
                            let m = b.mesh();
                            for e in m.face_edges(u32::try_from(*index).unwrap_or(u32::MAX)) {
                                if !m.seams.get(e).copied().unwrap_or(false)
                                    && let Some(p) = m.edges.get(e)
                                {
                                    pts.push(polyline_mid(p));
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            let mut uniq: Vec<Vec3> = Vec::new();
            for p in pts {
                if !uniq.iter().any(|q| q.dist(p) < 1e-9) {
                    uniq.push(p);
                }
            }
            let edges: Vec<Value> = uniq.into_iter().map(pt).collect();
            if *chamfer {
                ("FusionChamferCommand", json!({"edges": edges, "distance": radius}))
            } else {
                ("FusionFilletEdgesCommand", json!({"edges": edges, "radius": radius}))
            }
        }
        Kind::Shell { thickness } => {
            need(0, "faces to remove")?;
            ("FusionShellBodyCommand", json!({"faces": face_points(0), "thickness": thickness}))
        }
        Kind::Draft { angle } => {
            need(0, "faces")?;
            need(1, "the neutral plane")?;
            let neutral = plane_value(s, sels(d, 1).first()).ok_or("the neutral plane must be a plane or a planar face")?;
            ("FusionDraftCommand", json!({"faces": face_points(0), "angle": angle, "neutral": neutral}))
        }
        Kind::Mirror => {
            need(0, "bodies")?;
            need(1, "the mirror plane")?;
            let st = s.model.state();
            let mut features: Vec<String> = Vec::new();
            for n in body_names(0) {
                if let Some(f) = st.body(&n).and_then(|b| s.doc.feature(b.feature)).map(|f| f.name.clone())
                    && !features.contains(&f)
                {
                    features.push(f);
                }
            }
            let plane = plane_value(s, sels(d, 1).first()).ok_or("the mirror plane must be a plane or a planar face")?;
            ("MirrorCommand", json!({"features": features, "plane": plane}))
        }
        Kind::Hole { diameter, depth, kind } => {
            need(0, "a face position")?;
            let ty = HOLE_TYPES.get(*kind).copied().unwrap_or("simple");
            // One hole per picked position.
            for p in face_points(0) {
                let mut params = json!({"position": p, "diameter": diameter, "type": ty});
                if !depth.trim().is_empty() {
                    params["depth"] = json!(depth);
                }
                app.run("FusionHoleCommand", params)?;
            }
            return Ok(());
        }
        Kind::Primitive { cmd, fields, operation } => {
            let mut p = serde_json::Map::new();
            for (k, v) in fields {
                p.insert((*k).into(), json!(v));
            }
            p.insert("operation".into(), json!(OPS.get(*operation).copied().unwrap_or("new")));
            (cmd, Value::Object(p))
        }
        Kind::Combine { operation, keep_tools } => {
            need(0, "the target body")?;
            need(1, "tool bodies")?;
            let target = body_names(0).into_iter().next().unwrap_or_default();
            let tools: Vec<String> = body_names(1).into_iter().filter(|n| *n != target).collect();
            (
                "FusionCombineCommand",
                json!({"target": target, "tools": tools, "operation": OPS.get(*operation).copied().unwrap_or("join"), "keep_tools": keep_tools}),
            )
        }
        Kind::EditParam { name, expr } => ("ChangeParameterCommand", json!({"name": name, "expression": expr})),
        Kind::Sketch | Kind::Params { .. } => return Ok(()),
    };
    app.run(cmd, params).map(drop)
}
