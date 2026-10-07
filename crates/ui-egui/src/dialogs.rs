//! Command dialogs (shown at the right of the viewport). A dialog collects values and selection
//! inputs (geometry picked in the viewport, never typed), then runs its command; it never
//! changes the design itself. Things selected before the command starts become its input.

use std::hash::{Hash, Hasher};

use egui::{Color32, RichText, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::Session;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::doc::{AxisRef, Direction, FeatureKind, HoleKind, Operation, PlaneRef, ProfileSel};
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::selection::{self, AXES, Accept, BODIES, CURVES, EDGES, FACES, PLANAR_FACES, PLANES, PROFILES, SelInput};
use crate::theme::Tokens;
use crate::viewport::Hit;

const OPS: [&str; 4] = ["new", "join", "cut", "intersect"];
const OP_LABELS: [&str; 4] = ["New Body", "Join", "Cut", "Intersect"];
const DIRS: [&str; 4] = ["positive", "negative", "symmetric", "positive"];
const DIR_LABELS: [&str; 4] = ["One side", "Flip", "Symmetric", "Two sides"];
/// The "Two sides" direction (a second distance the other way).
pub const TWO_SIDES: usize = 3;
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
        /// The operation follows the geometry (into a body: cut, out of one: join, else new)
        /// until the user picks one.
        auto_op: bool,
        /// The other side's distance (two sides).
        distance2: String,
        /// Taper angle (empty: none).
        taper: String,
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
    /// Rectangular pattern of bodies: along one direction, optionally a second.
    PatternRect {
        count: String,
        spacing: String,
        count2: String,
        spacing2: String,
    },
    /// Circular pattern of bodies around an axis.
    PatternCirc {
        count: String,
        angle: String,
    },
    /// Loft through profiles of different sketches, in pick order.
    Loft {
        operation: usize,
    },
    /// Sweep a profile along a path of sketch curves.
    Sweep {
        operation: usize,
    },
    /// Move bodies by a distance along X, Y and Z.
    Move {
        x: String,
        y: String,
        z: String,
    },
    Hole {
        diameter: String,
        depth: String,
        kind: usize,
        cb_diameter: String,
        cb_depth: String,
        cs_diameter: String,
        cs_angle: String,
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
    Rename {
        feature: u64,
        name: String,
    },
    /// Delete a feature that others depend on.
    ConfirmDelete {
        feature: u64,
        with: Vec<String>,
        fail: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub struct Dialog {
    pub kind: Kind,
    /// Selection inputs, in order; clicks in the viewport go to the active one.
    pub inputs: Vec<SelInput>,
    pub active: usize,
    pub error: Option<String>,
    /// Editing an existing feature (its id, and the timeline marker to restore afterwards).
    pub editing: Option<(u64, Option<usize>)>,
    /// Give the on-canvas value box the keyboard (and select its text) on the next frame.
    pub focus: bool,
    /// Parameters the dialog doesn't show but the command needs (kept when editing).
    pub extra: serde_json::Map<String, Value>,
}

/// Can a selection go into an input that accepts `a`? (Planar-ness is checked when picking.)
fn fits(a: Accept, s: &Sel) -> bool {
    match s {
        Sel::Profile { .. } => a & PROFILES != 0,
        Sel::Edge { .. } => a & EDGES != 0,
        Sel::Face { .. } => a & (FACES | PLANAR_FACES) != 0,
        Sel::Body { .. } => a & BODIES != 0,
        Sel::Plane { .. } => a & PLANES != 0,
        Sel::Axis { .. } => a & AXES != 0,
        Sel::SketchCurve { .. } => a & (AXES | CURVES) != 0,
        Sel::Vertex { .. } => a & selection::VERTICES != 0,
        Sel::Feature { .. } | Sel::SketchPoint { .. } => false,
    }
}

impl Dialog {
    fn new(kind: Kind, inputs: Vec<SelInput>) -> Dialog {
        Dialog { kind, inputs, active: 0, error: None, editing: None, focus: true, extra: serde_json::Map::new() }
    }

    pub fn for_command(app: &SolveApp, id: &str) -> Option<Dialog> {
        let s = &app.session;
        let has_bodies = !s.model.state().bodies.is_empty();
        let mut d = match id {
            "SketchCreate" => Dialog::new(Kind::Sketch, vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)]),
            "Extrude" => Dialog::new(
                Kind::Extrude {
                    distance: "10 mm".into(),
                    direction: 0,
                    operation: usize::from(has_bodies),
                    auto_op: true,
                    distance2: "10 mm".into(),
                    taper: String::new(),
                },
                vec![SelInput::new("Profiles", PROFILES | PLANAR_FACES, true)],
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
            "PatternRectangular" => Dialog::new(
                Kind::PatternRect { count: "3".into(), spacing: "20 mm".into(), count2: "1".into(), spacing2: "20 mm".into() },
                vec![SelInput::new("Objects", BODIES, true), SelInput::new("Direction", AXES, false), SelInput::new("Direction 2", AXES, false)],
            ),
            "PatternCircular" => Dialog::new(
                Kind::PatternCirc { count: "6".into(), angle: "360 deg".into() },
                vec![SelInput::new("Objects", BODIES, true), SelInput::new("Axis", AXES, false)],
            ),
            "SolidLoft" => Dialog::new(Kind::Loft { operation: usize::from(has_bodies) }, vec![SelInput::new("Profiles", PROFILES, true)]),
            "Sweep" => Dialog::new(
                Kind::Sweep { operation: usize::from(has_bodies) },
                vec![SelInput::new("Profile", PROFILES, true), SelInput::new("Path", CURVES, true)],
            ),
            "FusionMoveCommand" => {
                Dialog::new(Kind::Move { x: "0 mm".into(), y: "0 mm".into(), z: "10 mm".into() }, vec![SelInput::new("Bodies", BODIES, true)])
            }
            "FusionHoleCommand" => Dialog::new(hole_defaults(), vec![SelInput::new("Position", FACES, true)]),
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
        // Pre-selection: what is selected now becomes the input (first input that takes it); a
        // face or edge stands for its body where bodies are wanted.
        for sel in &s.selection {
            let as_body = match sel {
                Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => Some(Sel::Body { name: body.clone() }),
                _ => None,
            };
            if let Some(b) = as_body
                && let Some(inp) = d.inputs.iter_mut().find(|i| i.accept == BODIES && (i.multi || i.items.is_empty()))
            {
                if !inp.items.contains(&b) {
                    inp.items.push(b);
                }
                continue;
            }
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

    pub fn rename(feature: u64, name: &str) -> Dialog {
        Dialog::new(Kind::Rename { feature, name: name.to_string() }, vec![])
    }

    pub fn confirm_delete(feature: u64, with: Vec<String>, fail: Vec<String>) -> Dialog {
        Dialog::new(Kind::ConfirmDelete { feature, with, fail }, vec![])
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

    /// Does this dialog make a feature that can be previewed live?
    pub fn previews(&self) -> bool {
        matches!(
            self.kind,
            Kind::Extrude { .. }
                | Kind::Revolve { .. }
                | Kind::Fillet { .. }
                | Kind::Shell { .. }
                | Kind::Draft { .. }
                | Kind::Mirror
                | Kind::PatternRect { .. }
                | Kind::PatternCirc { .. }
                | Kind::Loft { .. }
                | Kind::Sweep { .. }
                | Kind::Move { .. }
                | Kind::Hole { .. }
                | Kind::Primitive { .. }
                | Kind::Combine { .. }
        )
    }

    /// The main value (shown on the canvas too): label, kind and the expression.
    pub fn primary(&mut self) -> Option<(&'static str, ValueKind, &mut String)> {
        Some(match &mut self.kind {
            Kind::Extrude { distance, .. } => ("Distance", ValueKind::Length, distance),
            Kind::Revolve { angle, .. } => ("Angle", ValueKind::Angle, angle),
            Kind::Fillet { radius, chamfer, .. } => (if *chamfer { "Distance" } else { "Radius" }, ValueKind::Length, radius),
            Kind::Shell { thickness } => ("Thickness", ValueKind::Length, thickness),
            Kind::Draft { angle } => ("Angle", ValueKind::Angle, angle),
            Kind::Hole { diameter, .. } => ("Diameter", ValueKind::Length, diameter),
            Kind::Move { z, .. } => ("Z", ValueKind::Length, z),
            Kind::PatternRect { spacing, .. } => ("Spacing", ValueKind::Length, spacing),
            Kind::PatternCirc { angle, .. } => ("Angle", ValueKind::Angle, angle),
            _ => return None,
        })
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
            && !matches!(self.kind, Kind::Loft { .. })
            && let Some(inp) = self.inputs.get_mut(self.active)
        {
            inp.items.retain(|x| !matches!(x, Sel::Profile { sketch: s2, .. } if s2 != sketch));
        }
        let Some(inp) = self.inputs.get_mut(self.active) else { return };
        inp.toggle(picked);
        self.focus = true;
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

/// A value field; true when Enter was pressed in it.
fn field(ui: &mut egui::Ui, s: &mut String) -> bool {
    let r = ui.text_edit_singleline(s);
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

fn combo(ui: &mut egui::Ui, id: &str, labels: &[&str], sel: &mut usize) {
    egui::ComboBox::from_id_salt(id).selected_text(labels.get(*sel).copied().unwrap_or("")).width(FIELD_W).show_ui(ui, |ui| {
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
        Kind::PatternRect { .. } => "RECTANGULAR PATTERN",
        Kind::PatternCirc { .. } => "CIRCULAR PATTERN",
        Kind::Loft { .. } => "LOFT",
        Kind::Sweep { .. } => "SWEEP",
        Kind::Move { .. } => "MOVE",
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
        Kind::Rename { .. } => "RENAME",
        Kind::ConfirmDelete { .. } => "DELETE FEATURE",
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
    } else if a & CURVES != 0 {
        "click sketch curves"
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
            // Not in the Tab order: Tab moves between values.
            let b = egui::Button::new(text)
                .fill(if active { t.accent_soft } else { Color32::TRANSPARENT })
                .min_size(vec2(FIELD_W, 22.0))
                .sense(egui::Sense::CLICK);
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

/// Width limits of a docked dialog: shrink-wrapped to its content within these.
const DIALOG_MIN_W: f32 = 260.0;
const DIALOG_MAX_W: f32 = 380.0;
/// Width of value fields and choice boxes in a dialog.
const FIELD_W: f32 = 140.0;

/// Keep an automatic extrude operation in step with the geometry.
fn auto_operation(app: &SolveApp, d: &mut Dialog) {
    if !matches!(d.kind, Kind::Extrude { auto_op: true, .. }) {
        return;
    }
    let Some(op) =
        dialog_commands(app, d).ok().and_then(|c| c.into_iter().next()).and_then(|(_, p)| solvecraft_engine::auto_operation(&app.session, &p))
    else {
        return;
    };
    if let Kind::Extrude { operation, .. } = &mut d.kind
        && let Some(i) = OPS.iter().position(|o| *o == op)
    {
        *operation = i;
    }
}

/// Show the active dialog (if any), docked flush to the right edge of the viewport just below
/// the view cube, sized to its content.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else { return };
    auto_operation(app, &mut d);
    let t = Tokens::get();
    let vp = app.viewport.rect.unwrap_or_else(|| ctx.content_rect());
    let anchor = egui::pos2(vp.right(), vp.top() + crate::viewport::VIEW_CUBE_CLEARANCE);
    let mut keep = true;
    let mut ok = false;
    let mut cancel = false;
    let mut enter = false;
    let wide = matches!(d.kind, Kind::Params { .. });
    let heading = if d.editing.is_some() { format!("EDIT {}", title(&d.kind)) } else { title(&d.kind).to_string() };
    let frame = egui::Frame::window(&ctx.global_style()).corner_radius(egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }).shadow(egui::Shadow {
        offset: [-2, 2],
        blur: 8,
        spread: 0,
        color: Color32::from_black_alpha(40),
    });
    egui::Window::new(RichText::new(heading).strong().size(13.0))
        .id(egui::Id::new("sc_dialog"))
        .frame(frame)
        .pivot(egui::Align2::RIGHT_TOP)
        .fixed_pos(anchor)
        .constrain(false)
        .auto_sized()
        .scroll([false, true])
        .collapsible(false)
        .max_height((vp.bottom() - anchor.y - 8.0).max(120.0))
        .open(&mut keep)
        .show(ctx, |ui| {
            ui.spacing_mut().text_edit_width = FIELD_W;
            let margin = 2.0 * ui.style().spacing.window_margin.leftf();
            ui.set_min_width(if wide { 480.0 } else { DIALOG_MIN_W } - margin);
            ui.set_max_width(if wide { 560.0 } else { DIALOG_MAX_W } - margin);
            egui::Grid::new("sc_dialog_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                input_rows(&mut d, ui);
                match &mut d.kind {
                    Kind::Sketch => {}
                    Kind::Extrude { distance, direction, operation, auto_op, distance2, taper } => {
                        ui.label("Direction");
                        combo(ui, "ex_dir", &DIR_LABELS, direction);
                        ui.end_row();
                        ui.label("Distance");
                        enter |= field(ui, distance);
                        ui.end_row();
                        if *direction == TWO_SIDES {
                            ui.label("Distance 2");
                            enter |= field(ui, distance2);
                            ui.end_row();
                        } else if *direction < 2 {
                            ui.label("Taper angle");
                            let r = ui.add(egui::TextEdit::singleline(taper).hint_text("0 deg"));
                            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            ui.end_row();
                        }
                        ui.label("Operation");
                        let before = *operation;
                        combo(ui, "ex_op", &OP_LABELS, operation);
                        // A choice made by hand sticks.
                        if *operation != before {
                            *auto_op = false;
                        }
                        ui.end_row();
                    }
                    Kind::Revolve { angle, operation } => {
                        ui.label("Angle");
                        enter |= field(ui, angle);
                        ui.end_row();
                        ui.label("Operation");
                        combo(ui, "rv_op", &OP_LABELS, operation);
                        ui.end_row();
                    }
                    Kind::Fillet { radius, chamfer, chain } => {
                        ui.label(if *chamfer { "Distance" } else { "Radius" });
                        enter |= field(ui, radius);
                        ui.end_row();
                        ui.label("Tangent chain");
                        ui.checkbox(chain, "");
                        ui.end_row();
                    }
                    Kind::Shell { thickness } => {
                        ui.label("Inside thickness");
                        enter |= field(ui, thickness);
                        ui.end_row();
                    }
                    Kind::Draft { angle } => {
                        ui.label("Angle");
                        enter |= field(ui, angle);
                        ui.end_row();
                    }
                    Kind::Mirror => {}
                    Kind::PatternRect { count, spacing, count2, spacing2 } => {
                        for (l, v) in [("Quantity", count), ("Spacing", spacing), ("Quantity 2", count2), ("Spacing 2", spacing2)] {
                            ui.label(l);
                            enter |= field(ui, v);
                            ui.end_row();
                        }
                    }
                    Kind::PatternCirc { count, angle } => {
                        for (l, v) in [("Quantity", count), ("Total angle", angle)] {
                            ui.label(l);
                            enter |= field(ui, v);
                            ui.end_row();
                        }
                    }
                    Kind::Loft { operation } => {
                        ui.label("Operation");
                        combo(ui, "lf_op", &OP_LABELS, operation);
                        ui.end_row();
                    }
                    Kind::Sweep { operation } => {
                        ui.label("Operation");
                        combo(ui, "sw_op", &OP_LABELS, operation);
                        ui.end_row();
                    }
                    Kind::Move { x, y, z } => {
                        for (l, v) in [("X distance", x), ("Y distance", y), ("Z distance", z)] {
                            ui.label(l);
                            enter |= field(ui, v);
                            ui.end_row();
                        }
                    }
                    Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle } => {
                        ui.label("Type");
                        combo(ui, "hole_kind", &HOLE_LABELS, kind);
                        ui.end_row();
                        ui.label("Diameter");
                        enter |= field(ui, diameter);
                        ui.end_row();
                        ui.label("Depth");
                        let r = ui.add(egui::TextEdit::singleline(depth).hint_text("through all"));
                        enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.end_row();
                        if *kind == 1 {
                            ui.label("Counterbore Ø");
                            enter |= field(ui, cb_diameter);
                            ui.end_row();
                            ui.label("Counterbore depth");
                            enter |= field(ui, cb_depth);
                            ui.end_row();
                        }
                        if *kind == 2 {
                            ui.label("Countersink Ø");
                            enter |= field(ui, cs_diameter);
                            ui.end_row();
                            ui.label("Countersink angle");
                            enter |= field(ui, cs_angle);
                            ui.end_row();
                        }
                    }
                    Kind::Primitive { fields, operation, .. } => {
                        for (k, v) in fields.iter_mut() {
                            ui.label(*k);
                            enter |= field(ui, v);
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
                    Kind::Rename { name, .. } => {
                        ui.label("Name");
                        let r = ui.text_edit_singleline(name);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            ok = true;
                        }
                        r.request_focus();
                        ui.end_row();
                    }
                    Kind::ConfirmDelete { with, fail, .. } => {
                        if !with.is_empty() {
                            ui.label("Also deletes");
                            ui.label(with.join(", "));
                            ui.end_row();
                        }
                        if !fail.is_empty() {
                            ui.label("Will fail");
                            ui.label(RichText::new(fail.join(", ")).color(t.warning));
                            ui.end_row();
                        }
                    }
                }
                if let Some(e) = d.error.as_ref().or(app.preview.error.as_ref()) {
                    ui.label("");
                    ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.error)).wrap());
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if !matches!(d.kind, Kind::Sketch | Kind::Params { .. })
                    && ui
                        .add(
                            egui::Button::new(
                                RichText::new(if matches!(d.kind, Kind::ConfirmDelete { .. }) { "Delete" } else { "OK" }).color(Color32::WHITE),
                            )
                            .fill(t.accent)
                            .min_size(vec2(70.0, 24.0)),
                        )
                        .clicked()
                {
                    ok = true;
                }
                let close = if matches!(d.kind, Kind::Params { .. }) { "Close" } else { "Cancel" };
                if ui.add(egui::Button::new(close).min_size(vec2(70.0, 24.0))).clicked() {
                    cancel = true;
                }
                if app.preview.busy {
                    ui.add(egui::Spinner::new().size(14.0)).on_hover_text("Updating the preview");
                }
            });
        });
    // Keyboard: Enter applies (from a value field, or with nothing focused), Esc cancels.
    let (enter_free, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let nothing_focused = ctx.memory(|m| m.focused().is_none());
    let canvas_enter = enter_free && ctx.memory(|m| m.had_focus_last_frame(egui::Id::new("sc_canvas_value")));
    if d.previews() && (enter || canvas_enter || (enter_free && nothing_focused)) {
        ok = true;
    }
    if esc {
        cancel = true;
    }
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
    } else if let Some((_, marker)) = d.editing {
        // Editing done: put the timeline marker back where it was.
        let _ = app.run("timeline.rollTo", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
    }
}

/// Close the dialog without applying it (Esc, Cancel). An edit puts the timeline marker back.
pub fn cancel(app: &mut SolveApp) {
    if let Some(d) = app.dialog.take()
        && let Some((_, marker)) = d.editing
    {
        let _ = app.run("timeline.rollTo", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
    }
}

fn hole_defaults() -> Kind {
    Kind::Hole {
        diameter: "5 mm".into(),
        depth: String::new(),
        kind: 0,
        cb_diameter: "9 mm".into(),
        cb_depth: "3 mm".into(),
        cs_diameter: "10 mm".into(),
        cs_angle: "90 deg".into(),
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
    for (cmd, params) in apply_commands(app, d)? {
        app.run(&cmd, params)?;
    }
    Ok(())
}

/// The commands OK runs (an edit rebuilds its feature in place); the live preview runs the same.
pub fn apply_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let cmds = dialog_commands(app, d)?;
    if let Some((id, _)) = d.editing {
        let (cmd, params) = cmds.into_iter().next().ok_or("nothing to apply")?;
        return Ok(vec![("timeline.redefine".into(), json!({"feature": id, "command": cmd, "params": params}))]);
    }
    Ok(cmds)
}

/// The commands (id, parameters) a dialog's OK runs.
fn dialog_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
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
        Kind::Extrude { distance, direction, operation, distance2, taper, .. } => {
            need(0, "profiles or a planar face")?;
            let mut common = json!({"distance": distance, "direction": DIRS.get(*direction).copied().unwrap_or("positive"), "operation": OPS.get(*operation).copied().unwrap_or("new")});
            if *direction == TWO_SIDES {
                common["distance2"] = json!(distance2);
            } else if *direction < 2 && !taper.trim().is_empty() {
                common["taper"] = json!(taper);
            }
            let with = |extra: Value| -> Value {
                let mut p = common.clone();
                if let (Value::Object(m), Value::Object(e)) = (&mut p, extra) {
                    m.extend(e);
                    for (k, v) in &d.extra {
                        m.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                p
            };
            let mut out = Vec::new();
            let (sketch, idx) = profiles();
            if !sketch.is_null() {
                out.push(("Extrude".to_string(), with(json!({"sketch": sketch, "profiles": idx}))));
            }
            // Each planar body face extrudes on its own.
            for p in face_points(0) {
                out.push(("Extrude".to_string(), with(json!({ "face": p }))));
            }
            return Ok(out);
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
            let features = source_features(s, &body_names(0));
            let plane = plane_value(s, sels(d, 1).first()).ok_or("the mirror plane must be a plane or a planar face")?;
            ("MirrorCommand", json!({"features": features, "plane": plane}))
        }
        Kind::PatternRect { count, spacing, count2, spacing2 } => {
            need(0, "objects")?;
            need(1, "a direction (an axis or a sketch line)")?;
            let (_, d1) = sels(d, 1).first().and_then(|x| axis_of(app, x)).ok_or("the direction must be an axis or a sketch line")?;
            let mut p = json!({"features": source_features(s, &body_names(0)), "dir1": pt(d1), "count1": count, "spacing1": spacing});
            if let Some((_, d2)) = sels(d, 2).first().and_then(|x| axis_of(app, x)) {
                p["dir2"] = pt(d2);
                p["count2"] = json!(count2);
                p["spacing2"] = json!(spacing2);
            }
            ("PatternRectangular", p)
        }
        Kind::PatternCirc { count, angle } => {
            need(0, "objects")?;
            need(1, "an axis")?;
            let axis = match sels(d, 1).first() {
                Some(Sel::Axis { name }) => json!(name),
                Some(x) => {
                    let (o, dir) = axis_of(app, x).ok_or("the axis must be an origin axis or a sketch line")?;
                    json!({"origin": pt(o), "dir": pt(dir)})
                }
                None => return Err("select an axis".into()),
            };
            ("PatternCircular", json!({"features": source_features(s, &body_names(0)), "axis": axis, "count": count, "angle": angle}))
        }
        Kind::Loft { operation } => {
            need(0, "profiles of two or more sketches")?;
            // One section per sketch, in the order the sketches were first picked.
            let mut sections: Vec<(u64, Vec<usize>)> = Vec::new();
            for x in sels(d, 0) {
                if let Sel::Profile { sketch, index } = x {
                    match sections.iter_mut().find(|(sk, _)| sk == sketch) {
                        Some((_, v)) => v.push(*index),
                        None => sections.push((*sketch, vec![*index])),
                    }
                }
            }
            if sections.len() < 2 {
                return Err("pick profiles in two or more sketches".into());
            }
            let sections: Vec<Value> = sections.into_iter().map(|(sk, v)| json!({"sketch": sk, "profiles": v})).collect();
            ("SolidLoft", json!({"sections": sections, "operation": OPS.get(*operation).copied().unwrap_or("new")}))
        }
        Kind::Sweep { operation } => {
            need(0, "a profile")?;
            need(1, "the path")?;
            let (sketch, idx) = profiles();
            let path: Vec<String> = sels(d, 1).iter().filter_map(|x| if let Sel::SketchCurve { id } = x { Some(id.clone()) } else { None }).collect();
            let path_sketch = curves_sketch(app, &path, sketch.as_u64()).ok_or("the path curves must be in one sketch")?;
            (
                "Sweep",
                json!({"sketch": sketch, "profiles": idx, "path_sketch": path_sketch, "path": path, "operation": OPS.get(*operation).copied().unwrap_or("new")}),
            )
        }
        Kind::Move { x, y, z } => {
            need(0, "bodies")?;
            ("FusionMoveCommand", json!({"bodies": body_names(0), "translate": [x, y, z]}))
        }
        Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle } => {
            need(0, "a face position")?;
            let ty = HOLE_TYPES.get(*kind).copied().unwrap_or("simple");
            // One hole per picked position.
            let mut out = Vec::new();
            for p in face_points(0) {
                let mut params = json!({"position": p, "diameter": diameter, "type": ty});
                if !depth.trim().is_empty() {
                    params["depth"] = json!(depth);
                }
                match *kind {
                    1 => {
                        params["cb_diameter"] = json!(cb_diameter);
                        params["cb_depth"] = json!(cb_depth);
                    }
                    2 => {
                        params["cs_diameter"] = json!(cs_diameter);
                        params["cs_angle"] = json!(cs_angle);
                    }
                    _ => {}
                }
                out.push(("FusionHoleCommand".to_string(), params));
            }
            return Ok(out);
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
        Kind::Rename { feature, name } => ("FusionRenameTimelineEntryCommand", json!({"feature": feature, "name": name})),
        Kind::ConfirmDelete { feature, .. } => ("FusionDeleteCommand", json!({ "features": [feature.to_string()] })),
        Kind::Sketch | Kind::Params { .. } => return Ok(Vec::new()),
    };
    let mut params = params;
    if let Value::Object(m) = &mut params {
        for (k, v) in &d.extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), params)])
}

/// The features that made these bodies (what patterns and mirrors copy).
fn source_features(s: &Session, bodies: &[String]) -> Vec<String> {
    let st = s.model.state();
    let mut features: Vec<String> = Vec::new();
    for n in bodies {
        if let Some(f) = st.body(n).and_then(|b| s.doc.feature(b.feature)).map(|f| f.name.clone())
            && !features.contains(&f)
        {
            features.push(f);
        }
    }
    features
}

/// The sketch holding all these curve ids (the active sketch first, never `not`).
fn curves_sketch(app: &SolveApp, ids: &[String], not: Option<u64>) -> Option<u64> {
    let st = app.session.model.state();
    let has = |sid: u64| st.sketch(sid).is_some_and(|ss| ids.iter().all(|id| ss.sketch.curve_index(id).is_some()));
    if let Some(a) = app.session.active_sketch
        && Some(a) != not
        && has(a)
    {
        return Some(a);
    }
    st.sketches.iter().rev().map(|ss| ss.feature).find(|sid| Some(*sid) != not && has(*sid))
}

/// An axis selection as a line: a point on it and its unit direction (origin axes, or a straight
/// sketch line).
pub fn axis_of(app: &SolveApp, sel: &Sel) -> Option<(Vec3, Vec3)> {
    match sel {
        Sel::Axis { name } => Some((
            Vec3::ZERO,
            match name.as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                _ => Vec3::Z,
            },
        )),
        Sel::SketchCurve { id } => {
            let st = app.session.model.state();
            let sid = curves_sketch(app, std::slice::from_ref(id), None)?;
            let ss = st.sketch(sid)?;
            let c = ss.sketch.curves.get(ss.sketch.curve_index(id)?)?;
            let solvecraft_engine::sketch::CurveKind::Line { a, b } = c.kind else { return None };
            let (pa, pb) = (ss.plane.to_world(ss.sketch.point(a)?), ss.plane.to_world(ss.sketch.point(b)?));
            Some((pa, (pb - pa).normalized()?))
        }
        _ => None,
    }
}

/// Which profiles of a sketch a profile selection means.
fn profile_indices(ss: &solvecraft_engine::doc::SolvedSketch, sel: &ProfileSel) -> Vec<usize> {
    let ps = &ss.profiles;
    match sel {
        ProfileSel::All => (0..ps.len()).collect(),
        ProfileSel::Indices { indices } => indices.iter().copied().filter(|i| *i < ps.len()).collect(),
        ProfileSel::Curves { loops } => loops
            .iter()
            .filter_map(|l| {
                let mut want: Vec<&String> = l.iter().collect();
                want.sort();
                ps.iter().position(|p| {
                    let mut have: Vec<&String> = p.outer_curves.iter().collect();
                    have.sort();
                    have == want
                })
            })
            .collect(),
        ProfileSel::Points { points } => points.iter().filter_map(|q| ps.iter().position(|p| p.region.contains(*q))).collect(),
    }
}

/// The edge of a visible body through (or nearest) a point.
fn edge_sel(s: &Session, p: Vec3) -> Option<Sel> {
    let st = s.model.state();
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        for (i, e) in m.edges.iter().enumerate() {
            let d = e.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Edge { body: b.name.clone(), index: i, point: p }));
            }
        }
    }
    best.map(|x| x.1)
}

/// The body face containing a point (nearest triangle).
fn face_sel(s: &Session, p: Vec3) -> Option<Sel> {
    let st = s.model.state();
    let mut best: Option<(f64, Sel)> = None;
    for b in &st.bodies {
        let m = b.mesh();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = crate::preview::point_triangle_dist(p, a, bb, c);
            if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, Sel::Face { body: b.name.clone(), index: *f as usize, point: p }));
            }
        }
    }
    best.filter(|(d, _)| *d < 1e-3 + 1e-6 * p.len()).map(|x| x.1)
}

fn plane_sel(s: &Session, pl: &PlaneRef) -> Option<Sel> {
    match pl {
        PlaneRef::Origin { name } | PlaneRef::Construction { name } => Some(Sel::Plane { name: name.clone() }),
        PlaneRef::Custom { plane } => face_sel(s, plane.origin),
        _ => None,
    }
}

fn op_index(o: &Operation) -> usize {
    match o {
        Operation::NewBody => 0,
        Operation::Join => 1,
        Operation::Cut => 2,
        Operation::Intersect => 3,
    }
}

fn pt3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A dialog that edits an existing feature, filled from it. The timeline is rolled back to just
/// before the feature, so its references show on the geometry they refer to.
pub fn for_feature(app: &SolveApp, id: u64, marker: Option<usize>) -> Option<Dialog> {
    let s = &app.session;
    let f = s.doc.feature(id)?.clone();
    let st = s.model.state();
    let start = |cmd: &str| Dialog::for_command(app, cmd);
    let mut d = match &f.kind {
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets } => {
            let mut d = start("Extrude")?;
            d.kind = Kind::Extrude {
                distance: extent.distance.clone(),
                direction: match (extent.direction, &extent.distance2) {
                    (_, Some(_)) => TWO_SIDES,
                    (Direction::Positive, None) => 0,
                    (Direction::Negative, None) => 1,
                    (Direction::Symmetric, None) => 2,
                },
                operation: op_index(operation),
                auto_op: false,
                distance2: extent.distance2.clone().unwrap_or_else(|| "10 mm".into()),
                taper: extent.taper.clone().unwrap_or_default(),
            };
            let items = st.sketch(*sketch).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
            }
            for (k, v) in [
                ("start_offset", extent.start_offset.as_ref().map(|x| json!(x))),
                ("through_all", extent.through_all.then_some(json!(true))),
                ("targets", (!targets.is_empty()).then(|| json!(targets))),
            ] {
                if let Some(v) = v {
                    d.extra.insert(k.into(), v);
                }
            }
            d
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, operation, targets } => {
            let mut d = start("Revolve")?;
            d.kind = Kind::Revolve { angle: angle.clone(), operation: op_index(operation) };
            let items = st.sketch(*sketch).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
            }
            let ax = match axis {
                AxisRef::World { axis } => Some(Sel::Axis { name: axis.to_ascii_uppercase() }),
                AxisRef::SketchLine { curve } => Some(Sel::SketchCurve { id: curve.clone() }),
                _ => None,
            };
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = ax.into_iter().collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Fillet { edges, radius, .. } | FeatureKind::Chamfer { edges, distance: radius, .. } => {
            let chamfer = matches!(f.kind, FeatureKind::Chamfer { .. });
            let mut d = start(if chamfer { "FusionChamferCommand" } else { "FusionFilletEdgesCommand" })?;
            d.kind = Kind::Fillet { radius: radius.clone(), chamfer, chain: false };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = edges.iter().filter_map(|p| edge_sel(s, *p)).collect();
            }
            d
        }
        FeatureKind::Shell { faces, thickness, .. } => {
            let mut d = start("FusionShellBodyCommand")?;
            d.kind = Kind::Shell { thickness: thickness.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, *p)).collect();
            }
            d
        }
        FeatureKind::Draft { faces, angle, neutral, pull, .. } => {
            let mut d = start("FusionDraftCommand")?;
            d.kind = Kind::Draft { angle: angle.clone() };
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = faces.iter().filter_map(|p| face_sel(s, *p)).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, neutral).into_iter().collect();
            }
            d.extra.insert("pull".into(), pt3(*pull));
            d
        }
        FeatureKind::Hole { position, direction, diameter, depth, hole, .. } => {
            let mut d = start("FusionHoleCommand")?;
            let mut k = hole_defaults();
            if let Kind::Hole { diameter: dia, depth: dep, kind, cb_diameter, cb_depth, cs_diameter, cs_angle } = &mut k {
                *dia = diameter.clone();
                *dep = depth.clone().unwrap_or_default();
                match hole {
                    HoleKind::Counterbore { cb_diameter: a, cb_depth: b } => {
                        *kind = 1;
                        *cb_diameter = a.clone();
                        *cb_depth = b.clone();
                    }
                    HoleKind::Countersink { cs_diameter: a, cs_angle: b } => {
                        *kind = 2;
                        *cs_diameter = a.clone();
                        *cs_angle = b.clone();
                    }
                    HoleKind::Drilled { tip_angle } => {
                        d.extra.insert("tip_angle".into(), json!(tip_angle));
                    }
                    HoleKind::Simple => {}
                }
            }
            d.kind = k;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = vec![face_sel(s, *position).unwrap_or(Sel::Face { body: String::new(), index: 0, point: *position })];
            }
            d.extra.insert("direction".into(), pt3(*direction));
            d
        }
        FeatureKind::Box { corner, length, width, height, operation } => {
            let mut d = start("PrimitiveBox")?;
            d.kind = Kind::Primitive {
                cmd: "PrimitiveBox",
                fields: vec![("length", length.clone()), ("width", width.clone()), ("height", height.clone())],
                operation: op_index(operation),
            };
            d.extra.insert("corner".into(), pt3(*corner));
            d
        }
        FeatureKind::Cylinder { base, axis, radius, height, operation } => {
            let mut d = start("PrimitiveCylinder")?;
            d.kind = Kind::Primitive {
                cmd: "PrimitiveCylinder",
                fields: vec![("radius", radius.clone()), ("height", height.clone())],
                operation: op_index(operation),
            };
            d.extra.insert("base".into(), pt3(*base));
            d.extra.insert("axis".into(), pt3(*axis));
            d
        }
        FeatureKind::Sphere { center, radius, operation } => {
            let mut d = start("PrimitiveSphere")?;
            d.kind = Kind::Primitive { cmd: "PrimitiveSphere", fields: vec![("radius", radius.clone())], operation: op_index(operation) };
            d.extra.insert("center".into(), pt3(*center));
            d
        }
        FeatureKind::Torus { center, major, minor, operation } => {
            let mut d = start("PrimitiveTorus")?;
            d.kind = Kind::Primitive {
                cmd: "PrimitiveTorus",
                fields: vec![("major", major.clone()), ("minor", minor.clone())],
                operation: op_index(operation),
            };
            d.extra.insert("center".into(), pt3(*center));
            d
        }
        FeatureKind::Combine { target, tools, operation, keep_tools } => {
            let mut d = start("FusionCombineCommand")?;
            d.kind = Kind::Combine { operation: op_index(operation).max(1), keep_tools: *keep_tools };
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = vec![Sel::Body { name: target.clone() }];
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = tools.iter().map(|n| Sel::Body { name: n.clone() }).collect();
            }
            d
        }
        FeatureKind::Mirror { features, plane } => {
            let mut d = start("MirrorCommand")?;
            let ids: Vec<u64> = features.iter().filter_map(|n| s.doc.find_feature(n).map(|f| f.id)).collect();
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = st.bodies.iter().filter(|b| ids.contains(&b.feature)).map(|b| Sel::Body { name: b.name.clone() }).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, plane).into_iter().collect();
            }
            d
        }
        _ => return None,
    };
    d.editing = Some((id, marker));
    d.active = 0;
    d.error = None;
    Some(d)
}
