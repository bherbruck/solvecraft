//! Assembly dialogs: New Component, Joint, As-built Joint, Joint Origin, Rigid Group, Drive
//! Joints, Motion Link and Interference. Joint origins are picked by hovering geometry: a face
//! snaps to its centre (or a round face to the centre of its nearer end), an edge to its middle
//! (a round edge to its centre), a vertex to itself; the snap shows as a glyph with its z axis
//! before the click. Joints and drives preview on the placed (world) model: the moving
//! component shows where it goes, and with Animate the joint plays its motion.

use egui::{Color32, Pos2, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::Session;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::doc::joints::JointKind;
use solvecraft_engine::geom::{Aabb3, Mesh, Vec3};

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, field, row_label};
use crate::preview::{Built, Colors};
use crate::selection::{BODIES, EDGES, FACES, SelInput, VERTICES};
use crate::theme::Tokens;
use crate::viewport::Proj;

/// Joint types in the order the Type box lists them (command names).
pub const TYPES: [&str; 7] = ["rigid", "revolute", "slider", "cylindrical", "pin_slot", "planar", "ball"];
const TYPE_LABELS: [&str; 7] = ["Rigid", "Revolute", "Slider", "Cylindrical", "Pin-slot", "Planar", "Ball"];
/// What a joint origin input accepts (snaps on faces, edges and vertices).
const SNAPS: u16 = FACES | EDGES | VERTICES;
/// Commands whose effect is where occurrences sit (previewed on the placed model).
const MOVES: [&str; 6] = ["JointAssembleCmdNew", "JointAsBuiltCmd", "FusionMoveJointsCommand", "joint.edit", "joint.limits", "RigidGroupCmd"];

#[derive(Clone, Debug)]
pub enum Asm {
    NewComponent { name: String, activate: bool },
    Joint(Box<JointForm>),
    JointOrigin { name: String, hover: Hover },
    RigidGroup,
    Drive { joint: usize, values: Vec<String> },
    MotionLink { a: usize, b: usize, ia: usize, ib: usize, ratio: String, offset: String },
    Interference { result: Option<Value>, overlaps: Vec<Mesh>, of: Vec<Sel> },
}

/// The snap under the cursor, remembered for the selection it was found for.
pub type Hover = Option<(Sel, Option<SnapPt>)>;

/// The Joint and As-built Joint dialogs.
#[derive(Clone, Debug)]
pub struct JointForm {
    pub as_built: bool,
    /// Editing this joint (its origins stay; type, alignment and limits change).
    pub editing: Option<u64>,
    pub kind: usize,
    pub flip: bool,
    pub angle: String,
    pub offset: [String; 3],
    /// Per joint value: limited, minimum, maximum.
    pub limits: Vec<(bool, String, String)>,
    pub animate: bool,
    /// Animation phase, 0…1.
    pub phase: f64,
    pub hover: Hover,
}

impl JointForm {
    fn new(as_built: bool) -> JointForm {
        JointForm {
            as_built,
            editing: None,
            kind: 0,
            flip: false,
            angle: "0 deg".into(),
            offset: ["0 mm".into(), "0 mm".into(), "0 mm".into()],
            limits: Vec::new(),
            animate: false,
            phase: 0.0,
            hover: None,
        }
    }

    fn joint_kind(&self) -> JointKind {
        TYPES.get(self.kind).and_then(|t| JointKind::parse(t)).unwrap_or(JointKind::Rigid)
    }

    /// Limits rows sized to the joint's values.
    fn fit_limits(&mut self) {
        let k = self.joint_kind();
        let n = k.dofs().len();
        while self.limits.len() < n {
            let i = self.limits.len();
            self.limits.push(if k.is_angle(i) { (false, "-90 deg".into(), "90 deg".into()) } else { (false, "0 mm".into(), "20 mm".into()) });
        }
        self.limits.truncate(n);
    }
}

impl Asm {
    pub fn title(&self) -> &'static str {
        match self {
            Asm::NewComponent { .. } => "NEW COMPONENT",
            Asm::Joint(j) if j.editing.is_some() => "EDIT JOINT",
            Asm::Joint(j) if j.as_built => "AS-BUILT JOINT",
            Asm::Joint(_) => "JOINT",
            Asm::JointOrigin { .. } => "JOINT ORIGIN",
            Asm::RigidGroup => "RIGID GROUP",
            Asm::Drive { .. } => "DRIVE JOINTS",
            Asm::MotionLink { .. } => "MOTION LINK",
            Asm::Interference { .. } => "INTERFERENCE",
        }
    }

    pub fn previews(&self) -> bool {
        matches!(self, Asm::Joint(_) | Asm::Drive { .. } | Asm::RigidGroup)
    }
}

/// The dialog for an assembly command: its state and selection inputs.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let s = &app.session;
    let (asm, inputs) = match id {
        "FusionCreateNewComponentCommand" => {
            (Asm::NewComponent { name: String::new(), activate: true }, vec![SelInput::new("Bodies (optional)", BODIES, true)])
        }
        "JointAssembleCmdNew" => (
            Asm::Joint(Box::new(JointForm::new(false))),
            vec![SelInput::new("Component 1", SNAPS, false), SelInput::new("Component 2", SNAPS, false)],
        ),
        "JointAsBuiltCmd" => {
            (Asm::Joint(Box::new(JointForm::new(true))), vec![SelInput::new("Components", BODIES, true), SelInput::new("Position", SNAPS, false)])
        }
        "JointOrigin" => (Asm::JointOrigin { name: String::new(), hover: None }, vec![SelInput::new("Snap", SNAPS, false)]),
        "RigidGroupCmd" => (Asm::RigidGroup, vec![SelInput::new("Components", BODIES, true)]),
        "FusionMoveJointsCommand" => {
            let joint = movable(s).first().copied().unwrap_or(0);
            (Asm::Drive { joint, values: current_values(s, joint) }, vec![])
        }
        "FusionMotionRelationshipCommand" => {
            let b = usize::from(movable(s).len() > 1);
            (Asm::MotionLink { a: 0, b, ia: 0, ib: 0, ratio: "1".into(), offset: "0".into() }, vec![])
        }
        "InterferenceCheckCommand" => {
            (Asm::Interference { result: None, overlaps: Vec::new(), of: Vec::new() }, vec![SelInput::new("Bodies (all if none)", BODIES, true)])
        }
        _ => return None,
    };
    Some((Kind::Assembly(asm), inputs))
}

/// Indices (into the design's joints) of joints that move (not rigid).
fn movable(s: &Session) -> Vec<usize> {
    s.doc.assembly.joints.iter().enumerate().filter(|(_, j)| j.kind != JointKind::Rigid).map(|(i, _)| i).collect()
}

/// A joint's values as the Drive dialog shows them (degrees, millimetres).
fn current_values(s: &Session, index: usize) -> Vec<String> {
    let Some(j) = s.doc.assembly.joints.get(index) else { return Vec::new() };
    (0..j.kind.dofs().len())
        .map(|i| {
            let v = j.values.get(i).copied().unwrap_or(0.0);
            if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { num(v, "mm") }
        })
        .collect()
}

/// A value with its unit, without trailing zeros ("12 mm", "37.5 deg").
fn num(v: f64, unit: &str) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{} {unit}", if s == "-0" { "0" } else { s })
}

/// The Edit Joint dialog for a joint (by id).
pub fn edit_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let j = app.session.doc.assembly.joints.iter().find(|j| j.id == id)?;
    let mut f = JointForm::new(false);
    f.editing = Some(id);
    f.kind = TYPES.iter().position(|t| JointKind::parse(t) == Some(j.kind)).unwrap_or(0);
    f.flip = j.flip;
    f.angle = num(j.angle.to_degrees(), "deg");
    f.offset[2] = num(j.offset, "mm");
    f.fit_limits();
    for (i, l) in f.limits.iter_mut().enumerate() {
        if let Some(Some((lo, hi))) = j.limits.get(i) {
            let fmt = |v: f64| if j.kind.is_angle(i) { num(v.to_degrees(), "deg") } else { num(v, "mm") };
            *l = (true, fmt(*lo), fmt(*hi));
        }
    }
    Some(Dialog::new(Kind::Assembly(Asm::Joint(Box::new(f))), vec![]))
}

/// The Drive Joints dialog on a joint (by id).
pub fn drive_joint(app: &SolveApp, id: u64) -> Option<Dialog> {
    let joint = app.session.doc.assembly.joints.iter().position(|j| j.id == id)?;
    Some(Dialog::new(Kind::Assembly(Asm::Drive { joint, values: current_values(&app.session, joint) }), vec![]))
}

// ---- snaps ----

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind {
    FaceCenter,
    CircleCenter,
    EdgeMid,
    Vertex,
}

/// A joint origin snap (world): where it sits, its z axis, and the command's origin parameter.
#[derive(Clone, Debug)]
pub struct SnapPt {
    pub at: Vec3,
    pub z: Vec3,
    pub kind: SnapKind,
    pub param: Value,
}

fn pt(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// The occurrence placing a body (0: the root design), when the body is its primary instance.
fn occurrence_of_body(s: &Session, body: &str) -> Option<u64> {
    let st = s.model.state();
    let b = st.body(body)?;
    let comp = s.doc.body_component(&b.name, b.feature);
    if comp == 0 { Some(0) } else { s.doc.occurrence_of(comp).map(|o| o.id) }
}

/// Centre and unit normal of the circle through a polyline's points, if it lies on one.
pub fn circle_of(pts: &[Vec3]) -> Option<(Vec3, Vec3)> {
    let n = pts.len();
    if n < 4 {
        return None;
    }
    let (a, b, c) = (*pts.first()?, *pts.get(n / 3)?, *pts.get(2 * n / 3)?);
    let (ab, ac) = (b - a, c - a);
    let nn = ab.cross(ac);
    let l2 = nn.len2();
    if l2 < 1e-18 {
        return None;
    }
    let center = a + (nn.cross(ab) * ac.len2() + ac.cross(nn) * ab.len2()) / (2.0 * l2);
    let r = center.dist(a);
    if !r.is_finite() || r > 1e6 {
        return None;
    }
    let tol = (r * 2e-3).max(1e-6);
    let normal = nn.normalized()?;
    pts.iter().all(|p| (p.dist(center) - r).abs() < tol && (*p - center).dot(normal).abs() < tol).then_some((center, normal))
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

/// Normal of the mesh face `f` near `p`.
fn face_normal_near(m: &Mesh, f: u32, p: Vec3) -> Option<Vec3> {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(_, tf)| **tf == f)
        .filter_map(|(t, _)| m.tri(t))
        .min_by(|a, b| ((a[0] + a[1] + a[2]) / 3.0).dist(p).total_cmp(&((b[0] + b[1] + b[2]) / 3.0).dist(p)))
        .and_then(|[a, b, c]| (b - a).cross(c - a).normalized())
}

/// The snap a picked item stands for (world coordinates, as the viewport shows them).
pub fn snap_of(s: &Session, sel: &Sel) -> Option<SnapPt> {
    let st = s.world_state();
    let (body, mut snap) = match sel {
        Sel::Face { body, index, point } => {
            let b = st.body(body)?;
            let tol = (b.body.size() * 2e-3).max(1e-3);
            let f = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == *index)?;
            match f.plane_normal {
                Some(n) => (body, SnapPt { at: f.centroid, z: n, kind: SnapKind::FaceCenter, param: json!({"face": pt(*point)}) }),
                None => {
                    let c = solvecraft_engine::kernel::cylinder_face_at(&b.body, *point)?;
                    let sv = (*point - c.axis_point).dot(c.axis);
                    let (at, dir) = if (sv - c.start).abs() <= (c.end - sv).abs() { (c.start, -1.0) } else { (c.end, 1.0) };
                    (
                        body,
                        SnapPt {
                            at: c.axis_point + c.axis * at,
                            z: c.axis * dir,
                            kind: SnapKind::CircleCenter,
                            param: json!({"circle": pt(*point)}),
                        },
                    )
                }
            }
        }
        Sel::Edge { body, index, point } => {
            let b = st.body(body)?;
            let m = b.mesh();
            let e = m.edges.get(*index)?;
            match circle_of(e) {
                Some((c, n)) => {
                    // The axis points out of the material: away from the faces' side.
                    let faces = m.edge_faces.get(*index).cloned().unwrap_or_default();
                    let out = faces.iter().filter_map(|f| face_normal_near(&m, *f, *point)).fold(Vec3::ZERO, |a, x| a + x);
                    let z = if out.dot(n) < 0.0 { -n } else { n };
                    (body, SnapPt { at: c, z, kind: SnapKind::CircleCenter, param: json!({"point": pt(c), "z": pt(z)}) })
                }
                None => {
                    let mid = polyline_mid(e);
                    let z = m.edge_faces.get(*index).and_then(|fs| fs.first()).and_then(|f| face_normal_near(&m, *f, mid)).unwrap_or(Vec3::Z);
                    (body, SnapPt { at: mid, z, kind: SnapKind::EdgeMid, param: json!({"point": pt(mid), "z": pt(z)}) })
                }
            }
        }
        Sel::Vertex { body, point } => (body, SnapPt { at: *point, z: Vec3::Z, kind: SnapKind::Vertex, param: json!({"point": pt(*point)}) }),
        _ => return None,
    };
    if let (Some(occ), Value::Object(m)) = (occurrence_of_body(s, body), &mut snap.param) {
        m.insert("occurrence".into(), json!(occ));
    }
    Some(snap)
}

/// A snap moved across its own plane by (x, y) (the joint's offset), as an explicit frame.
fn shifted(s: &SnapPt, x: f64, y: f64) -> Value {
    let ax = s.z.any_perp();
    let ay = s.z.cross(ax);
    let mut p = json!({"frame": {"origin": pt(s.at + ax * x + ay * y), "z": pt(s.z), "x": pt(ax)}});
    if let Some(o) = s.param.get("occurrence") {
        p["occurrence"] = o.clone();
    }
    p
}

// ---- rows ----

/// The dialog's own rows (after the selection inputs); true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Asm, inputs: &mut [SelInput], active: &mut usize) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    match k {
        Asm::NewComponent { name, activate } => {
            row_label(ui, "Name");
            let r = ui.add(egui::TextEdit::singleline(name).hint_text("Component1"));
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.end_row();
            row_label(ui, "Activate");
            ui.checkbox(activate, "");
            ui.end_row();
        }
        Asm::Joint(f) => {
            // As-built: two components picked, the position comes next.
            if f.as_built && *active == 0 && inputs.first().is_some_and(|i| i.items.len() >= 2) {
                *active = 1;
            }
            row_label(ui, "Motion");
            ui.label("");
            ui.end_row();
            row_label(ui, "Type");
            combo(ui, "jt_type", &TYPE_LABELS, &mut f.kind);
            ui.end_row();
            f.fit_limits();
            if !f.as_built {
                row_label(ui, "Angle");
                enter |= field(ui, &mut f.angle);
                ui.end_row();
                for (l, v) in ["Offset X", "Offset Y", "Offset Z"].iter().zip(f.offset.iter_mut()) {
                    if f.editing.is_some() && *l != "Offset Z" {
                        continue;
                    }
                    row_label(ui, l);
                    enter |= field(ui, v);
                    ui.end_row();
                }
                row_label(ui, "Flip");
                ui.checkbox(&mut f.flip, "");
                ui.end_row();
            }
            let kind = f.joint_kind();
            if kind != JointKind::Rigid {
                row_label(ui, "Animate");
                let label = if f.animate { "■ Stop" } else { "▶ Play" };
                if ui.button(label).on_hover_text("Play the joint's motion in the view").clicked() {
                    f.animate = !f.animate;
                }
                ui.end_row();
                for (i, (on, lo, hi)) in f.limits.iter_mut().enumerate() {
                    let name = kind.dofs().get(i).copied().unwrap_or("value");
                    row_label(ui, &format!("{} limits", capital(name)));
                    ui.checkbox(on, "");
                    ui.end_row();
                    if *on {
                        row_label(ui, "  Minimum");
                        enter |= field(ui, lo);
                        ui.end_row();
                        row_label(ui, "  Maximum");
                        enter |= field(ui, hi);
                        ui.end_row();
                    }
                }
            } else {
                f.animate = false;
            }
            if f.animate {
                let now = ui.input(|i| i.time);
                // 48 steps over 3 s: each one a preview.
                f.phase = ((now / 3.0).fract() * 48.0).floor() / 48.0;
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
            }
        }
        Asm::JointOrigin { name, .. } => {
            row_label(ui, "Name");
            let r = ui.add(egui::TextEdit::singleline(name).hint_text("Joint Origin1"));
            enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.end_row();
        }
        Asm::RigidGroup => {}
        Asm::Drive { joint, values } => {
            let s = &app.session;
            let list = movable(s);
            if list.is_empty() {
                row_label(ui, "");
                ui.label(RichText::new("no joint that moves yet").color(t.text_dim));
                ui.end_row();
                return false;
            }
            let labels: Vec<String> =
                list.iter().filter_map(|i| s.doc.assembly.joints.get(*i)).map(|j| format!("{} ({:?})", j.name, j.kind)).collect();
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let mut pos = list.iter().position(|i| i == joint).unwrap_or(0);
            row_label(ui, "Joint");
            combo(ui, "drv_joint", &refs, &mut pos);
            ui.end_row();
            let pick = list.get(pos).copied().unwrap_or(0);
            if pick != *joint || values.is_empty() {
                *joint = pick;
                *values = current_values(s, pick);
            }
            let Some(j) = s.doc.assembly.joints.get(*joint).cloned() else { return false };
            for (i, v) in values.iter_mut().enumerate() {
                let angle = j.kind.is_angle(i);
                let unit_kind = if angle { ValueKind::Angle } else { ValueKind::Length };
                let name = j.kind.dofs().get(i).copied().unwrap_or("value");
                row_label(ui, &capital(name));
                enter |= field(ui, v);
                ui.end_row();
                // A slider over the limits (or a turn, or ±100 mm).
                let (lo, hi) = match j.limits.get(i).copied().flatten() {
                    Some((a, b)) => (a.min(b), a.max(b)),
                    None if angle => (-std::f64::consts::PI, std::f64::consts::PI),
                    None => (-100.0, 100.0),
                };
                let cur = s.doc.eval(v, unit_kind).ok().filter(|x| x.is_finite()).unwrap_or(0.0);
                let (mut shown, scale) = if angle { (cur.to_degrees(), 1f64.to_degrees()) } else { (cur, 1.0) };
                row_label(ui, "");
                ui.spacing_mut().slider_width = 124.0;
                let r = ui.add(egui::Slider::new(&mut shown, lo * scale..=hi * scale).show_value(false));
                if r.changed() {
                    *v = if angle { format!("{shown:.1} deg") } else { format!("{shown:.2} mm") };
                }
                ui.end_row();
            }
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset } => {
            let s = &app.session;
            let joints = &s.doc.assembly.joints;
            if joints.len() < 2 {
                row_label(ui, "");
                ui.label(RichText::new("needs two joints").color(t.text_dim));
                ui.end_row();
                return false;
            }
            let labels: Vec<&str> = joints.iter().map(|j| j.name.as_str()).collect();
            for (l, sel, idx, id) in [("Joint 1", &mut *a, &mut *ia, "ml_a"), ("Joint 2", &mut *b, &mut *ib, "ml_b")] {
                row_label(ui, l);
                combo(ui, id, &labels, sel);
                ui.end_row();
                let dofs = joints.get(*sel).map(|j| j.kind.dofs()).unwrap_or(&[]);
                if dofs.len() > 1 {
                    row_label(ui, "  Value");
                    let caps: Vec<String> = dofs.iter().map(|d| capital(d)).collect();
                    let refs: Vec<&str> = caps.iter().map(String::as_str).collect();
                    combo(ui, &format!("{id}_i"), &refs, idx);
                    ui.end_row();
                } else {
                    *idx = 0;
                }
            }
            row_label(ui, "Ratio");
            enter |= field(ui, ratio);
            ui.end_row();
            row_label(ui, "Offset");
            enter |= field(ui, offset);
            ui.end_row();
        }
        Asm::Interference { result, overlaps, of } => {
            let picked: Vec<Sel> = inputs.first().map(|i| i.items.clone()).unwrap_or_default();
            row_label(ui, "");
            let stale = result.is_some() && *of != picked;
            if ui.button(if stale { "Compute again" } else { "Compute" }).clicked() {
                let (r, o) = interference(app, &picked);
                *result = Some(r);
                *overlaps = o;
                *of = picked;
            }
            ui.end_row();
            if let Some(r) = result.as_ref() {
                interference_rows(ui, r);
            }
        }
    }
    enter
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Run the check and find the overlapping volumes (world placement) to show in red.
fn interference(app: &mut SolveApp, picked: &[Sel]) -> (Value, Vec<Mesh>) {
    let names: Vec<String> = picked.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
    let r = match app.run("InterferenceCheckCommand", json!({ "bodies": names })) {
        Ok(v) => v,
        Err(e) => return (json!({ "error": e }), Vec::new()),
    };
    let st = app.session.world_state();
    let mut meshes = Vec::new();
    for hit in r["interferences"].as_array().into_iter().flatten() {
        let (Some(a), Some(b)) = (hit["a"].as_str().and_then(|n| st.body(n)), hit["b"].as_str().and_then(|n| st.body(n))) else { continue };
        if let Ok(Some(o)) = solvecraft_engine::kernel::boolean(&a.body, &b.body, solvecraft_engine::kernel::BoolOp::Intersect)
            && let Ok(m) = o.tessellate((o.size() * 2e-3).max(1e-3))
        {
            meshes.push(m);
        }
    }
    (r, meshes)
}

fn interference_rows(ui: &mut egui::Ui, r: &Value) {
    let t = Tokens::get();
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        row_label(ui, "");
        ui.label(RichText::new(e).color(t.error));
        ui.end_row();
        return;
    }
    let hits = r["interferences"].as_array().cloned().unwrap_or_default();
    row_label(ui, "Checked");
    ui.label(format!("{} bodies", r["checked"].as_u64().unwrap_or(0)));
    ui.end_row();
    if hits.is_empty() {
        row_label(ui, "Result");
        ui.label(RichText::new("no interference").color(t.text_dim));
        ui.end_row();
        return;
    }
    row_label(ui, "Interferes");
    ui.label(RichText::new("Volume").strong());
    ui.end_row();
    for h in hits {
        row_label(ui, &format!("{} · {}", h["a"].as_str().unwrap_or(""), h["b"].as_str().unwrap_or("")));
        ui.label(RichText::new(format!("{:.3} mm³", h["volume_mm3"].as_f64().unwrap_or(0.0))).color(t.error));
        ui.end_row();
    }
}

// ---- commands ----

fn sel_items(inputs: &[SelInput], i: usize) -> &[Sel] {
    inputs.get(i).map(|x| x.items.as_slice()).unwrap_or(&[])
}

/// The occurrences of picked bodies (deduplicated, in pick order).
fn occurrences(s: &Session, items: &[Sel]) -> Vec<u64> {
    let mut out = Vec::new();
    for x in items {
        if let Sel::Body { name } = x
            && let Some(o) = occurrence_of_body(s, name)
            && !out.contains(&o)
        {
            out.push(o);
        }
    }
    out
}

/// The commands the dialog's OK runs.
pub fn commands(app: &SolveApp, k: &Asm, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    let s = &app.session;
    let snap = |i: usize, what: &str| -> Result<SnapPt, String> {
        sel_items(inputs, i).first().and_then(|x| snap_of(s, x)).ok_or_else(|| format!("select {what} first"))
    };
    let eval = |e: &str, kind: ValueKind| -> Result<f64, String> {
        if e.trim().is_empty() { Ok(0.0) } else { s.doc.eval(e, kind).map_err(|e| e.to_string()) }
    };
    let cmd = |id: &str, p: Value| vec![(id.to_string(), p)];
    Ok(match k {
        Asm::NewComponent { name, activate } => {
            let bodies: Vec<String> =
                sel_items(inputs, 0).iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
            if bodies.is_empty() {
                let mut p = json!({ "activate": activate });
                if !name.trim().is_empty() {
                    p["name"] = json!(name.trim());
                }
                cmd("FusionCreateNewComponentCommand", p)
            } else {
                cmd("FusionCreateComponentsFromBodiesCommand", json!({ "bodies": bodies }))
            }
        }
        Asm::Joint(f) => {
            let kind = TYPES.get(f.kind).copied().unwrap_or("rigid");
            let jk = f.joint_kind();
            let limits: Vec<Value> =
                f.limits.iter().take(jk.dofs().len()).map(|(on, lo, hi)| if *on { json!([lo, hi]) } else { Value::Null }).collect();
            if let Some(id) = f.editing {
                let mut out = cmd("joint.edit", json!({"joint": id, "type": kind, "flip": f.flip, "offset": f.offset[2], "angle": f.angle}));
                for (i, l) in limits.iter().enumerate() {
                    let p = match l.as_array() {
                        Some(a) => json!({"joint": id, "index": i, "min": a.first(), "max": a.get(1)}),
                        None => json!({"joint": id, "index": i}),
                    };
                    out.push(("joint.limits".into(), p));
                }
                return Ok(out);
            }
            if f.as_built {
                let occs = occurrences(s, sel_items(inputs, 0));
                if occs.len() != 2 {
                    return Err("select two components".into());
                }
                let mut p = json!({"type": kind, "a": occs.first(), "b": occs.get(1), "limits": limits});
                if let Some(x) = sel_items(inputs, 1).first().and_then(|x| snap_of(s, x)) {
                    p["at"] = x.param;
                }
                return Ok(cmd("JointAsBuiltCmd", p));
            }
            let (a, b) = (snap(0, "component 1's joint origin")?, snap(1, "component 2's joint origin")?);
            if a.param.get("occurrence").is_some() && a.param.get("occurrence") == b.param.get("occurrence") {
                return Err("the two origins are on the same component".into());
            }
            let (x, y) = (eval(&f.offset[0], ValueKind::Length)?, eval(&f.offset[1], ValueKind::Length)?);
            let a_param = if x.abs() > 1e-12 || y.abs() > 1e-12 { shifted(&a, x, y) } else { a.param };
            let p = json!({"type": kind, "a": a_param, "b": b.param, "flip": f.flip, "offset": f.offset[2], "angle": f.angle, "limits": limits});
            cmd("JointAssembleCmdNew", p)
        }
        Asm::JointOrigin { name, .. } => {
            let mut p = snap(0, "a snap")?.param;
            if !name.trim().is_empty() {
                p["name"] = json!(name.trim());
            }
            cmd("JointOrigin", p)
        }
        Asm::RigidGroup => {
            let occs = occurrences(s, sel_items(inputs, 0));
            if occs.len() < 2 {
                return Err("select two or more components".into());
            }
            cmd("RigidGroupCmd", json!({ "occurrences": occs }))
        }
        Asm::Drive { joint, values } => {
            let j = s.doc.assembly.joints.get(*joint).ok_or("no joint that moves yet")?;
            cmd("FusionMoveJointsCommand", json!({"joint": j.id, "values": values}))
        }
        Asm::MotionLink { a, b, ia, ib, ratio, offset } => {
            let js = &s.doc.assembly.joints;
            let (Some(ja), Some(jb)) = (js.get(*a), js.get(*b)) else { return Err("needs two joints".into()) };
            let num = |e: &str| eval(e, ValueKind::Unitless);
            cmd("FusionMotionRelationshipCommand", json!({"a": ja.id, "b": jb.id, "ia": ia, "ib": ib, "ratio": num(ratio)?, "offset": num(offset)?}))
        }
        // The check runs from its Compute button; OK only closes.
        Asm::Interference { .. } => Vec::new(),
    })
}

/// The preview's commands: OK's, with the joint's values playing its motion while animating.
pub fn animate(app: &SolveApp, k: &Asm, cmds: &mut Vec<(String, Value)>) {
    let Asm::Joint(f) = k else { return };
    if !f.animate {
        return;
    }
    let kind = f.joint_kind();
    let tau = std::f64::consts::TAU;
    let wave = (f.phase * tau).sin();
    let values: Vec<String> = (0..kind.dofs().len())
        .map(|i| {
            let limited = f.limits.get(i).filter(|l| l.0);
            let angle = kind.is_angle(i);
            let vk = if angle { ValueKind::Angle } else { ValueKind::Length };
            match limited.and_then(|(_, lo, hi)| Some((app.session.doc.eval(lo, vk).ok()?, app.session.doc.eval(hi, vk).ok()?))) {
                // Back and forth between the limits.
                Some((lo, hi)) => {
                    let v = lo + (hi - lo) * (0.5 - 0.5 * (f.phase * tau).cos());
                    if angle { format!("{} deg", v.to_degrees()) } else { format!("{v} mm") }
                }
                None if angle && i == 0 => format!("{} deg", f.phase * 360.0),
                None if angle => format!("{} deg", 30.0 * wave),
                None => format!("{} mm", 20.0 * wave),
            }
        })
        .collect();
    let drive = match f.editing {
        Some(id) => Some(id),
        None => match cmds.iter_mut().find(|(c, _)| c == "JointAssembleCmdNew") {
            Some((_, p)) => {
                p["values"] = json!(values);
                None
            }
            // As-built joints start at zero: drive the new one after it is made.
            None => Some(next_joint_id(&app.session)),
        },
    };
    if let Some(id) = drive {
        cmds.push(("FusionMoveJointsCommand".into(), json!({"joint": id, "values": values})));
    }
}

fn next_joint_id(s: &Session) -> u64 {
    s.doc.assembly.joints.iter().map(|j| j.id).max().unwrap_or(0) + 1
}

/// Joint and drive previews: the commands run on a scratch copy and the placed model before and
/// after is compared, so the components that move show where they go (`None`: not such
/// commands).
pub fn world_preview(s: &Session, cmds: &[(String, Value)], colors: Colors) -> Option<Result<Built, String>> {
    if cmds.is_empty() || !cmds.iter().all(|(c, _)| MOVES.contains(&c.as_str())) {
        return None;
    }
    let mut scratch = s.scratch();
    for (id, p) in cmds {
        if let Err(e) = scratch.execute(id, p) {
            return Some(Err(e.to_string()));
        }
    }
    let (before, after) = (s.world_state(), scratch.world_state());
    // A moved component looks like itself (no new surface), with its old place as the ghost.
    let colors = Colors { added: colors.body, ..colors };
    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| crate::preview::build(&before, &after, colors)));
    Some(match built {
        Ok(mut b) => {
            b.bounds = after.bodies.iter().fold(Aabb3::EMPTY, |acc, x| acc.union(&x.mesh().bounds()));
            Ok(b)
        }
        Err(_) => Err("the preview failed".into()),
    })
}

// ---- overlay ----

/// Snap glyphs under the cursor and at the picked origins; the overlap of an interference.
pub fn overlay(app: &SolveApp, painter: &egui::Painter, proj: &Proj, d: &mut Dialog) {
    let t = Tokens::get();
    let Kind::Assembly(k) = &mut d.kind else { return };
    if let Asm::Interference { overlaps, .. } = k {
        // Front faces only, shaded by how they face the view.
        let back = proj.cam.back();
        for m in overlaps.iter() {
            let mut em = egui::Mesh::default();
            for tri in &m.triangles {
                let Some(ps) = m.tri(tri) else { continue };
                let Some(n) = (ps[1] - ps[0]).cross(ps[2] - ps[0]).normalized() else { continue };
                let facing = n.dot(back);
                if facing <= 0.0 {
                    continue;
                }
                let k = 0.55 + 0.45 * facing;
                let fill = Color32::from_rgba_unmultiplied((235.0 * k) as u8, (45.0 * k) as u8, (40.0 * k) as u8, 210);
                let sp: Vec<Pos2> = ps.iter().filter_map(|p| proj.to_screen(*p)).collect();
                if sp.len() == 3 {
                    let base = em.vertices.len() as u32;
                    for p in sp {
                        em.colored_vertex(p, fill);
                    }
                    em.add_triangle(base, base + 1, base + 2);
                }
            }
            painter.add(egui::Shape::mesh(em));
        }
        return;
    }
    let hover = match k {
        Asm::Joint(f) => Some(&mut f.hover),
        Asm::JointOrigin { hover, .. } => Some(hover),
        _ => None,
    };
    let Some(hover) = hover else { return };
    // The picked origins: Component 1 in the accent colour, Component 2 in orange.
    let picked: Vec<(SnapPt, Color32)> = d
        .inputs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.accept == SNAPS)
        .filter_map(|(n, i)| i.items.first().and_then(|x| snap_of(&app.session, x)).map(|s| (s, if n == 0 { t.accent } else { t.warning })))
        .collect();
    for (s, c) in &picked {
        glyph(painter, proj, s, *c, app.cam.half_height());
    }
    // The snap under the cursor (found again only when the hovered item changes).
    let accepting = d.inputs.get(d.active).is_some_and(|i| i.accept == SNAPS);
    let under = app.viewport.hover.as_ref().and_then(crate::viewport::hit_sel).filter(|_| accepting);
    match under {
        Some(sel) => {
            let fresh = !matches!(hover, Some((s, _)) if *s == sel);
            if fresh {
                *hover = Some((sel.clone(), snap_of(&app.session, &sel)));
            }
            if let Some((_, Some(s))) = hover.as_ref() {
                glyph(painter, proj, s, t.hover_profile_edge, app.cam.half_height());
            }
        }
        None => *hover = None,
    }
}

/// A snap: its kind's marker at the point and the z axis as an arrow.
fn glyph(painter: &egui::Painter, proj: &Proj, s: &SnapPt, col: Color32, half_height: f64) {
    let Some(c) = proj.to_screen(s.at) else { return };
    let len = half_height * 2.0 * 0.06;
    if let Some(tip) = proj.to_screen(s.at + s.z * len) {
        painter.line_segment([c, tip], Stroke::new(2.0, col));
        let d = (tip - c).normalized();
        let n = vec2(-d.y, d.x);
        painter.add(egui::Shape::convex_polygon(vec![tip, tip - d * 8.0 + n * 4.0, tip - d * 8.0 - n * 4.0], col, Stroke::NONE));
    }
    let white = Stroke::new(1.5, Color32::WHITE);
    match s.kind {
        SnapKind::FaceCenter => {
            painter.rect(egui::Rect::from_center_size(c, vec2(10.0, 10.0)), 1.0, col, white, egui::StrokeKind::Middle);
        }
        SnapKind::CircleCenter => {
            painter.circle(c, 6.0, col, white);
            painter.line_segment([c - vec2(4.0, 0.0), c + vec2(4.0, 0.0)], Stroke::new(1.2, Color32::WHITE));
            painter.line_segment([c - vec2(0.0, 4.0), c + vec2(0.0, 4.0)], Stroke::new(1.2, Color32::WHITE));
        }
        SnapKind::EdgeMid => {
            painter.add(egui::Shape::convex_polygon(vec![c + vec2(0.0, -7.0), c + vec2(6.5, 5.0), c + vec2(-6.5, 5.0)], col, white));
        }
        SnapKind::Vertex => {
            painter.add(egui::Shape::convex_polygon(
                vec![c + vec2(0.0, -6.5), c + vec2(6.5, 0.0), c + vec2(0.0, 6.5), c + vec2(-6.5, 0.0)],
                col,
                white,
            ));
        }
    }
}

// ---- timeline and browser ----

/// A joint's menu (timeline and browser): Edit, Drive, Suppress, Delete.
fn joint_menu(
    app: &SolveApp,
    ui: &mut egui::Ui,
    (id, kind, suppressed): (u64, JointKind, bool),
    open: &mut Option<Dialog>,
    run: &mut Option<(&'static str, Value)>,
) {
    if ui.button("Edit Joint").clicked() {
        *open = edit_joint(app, id);
        ui.close();
    }
    if kind != JointKind::Rigid && ui.button("Drive Joint").clicked() {
        *open = drive_joint(app, id);
        ui.close();
    }
    if ui.button(if suppressed { "Unsuppress" } else { "Suppress" }).clicked() {
        *run = Some(("joint.edit", json!({"joint": id, "suppressed": !suppressed})));
        ui.close();
    }
    ui.separator();
    if ui.button("Delete").clicked() {
        *run = Some(("joint.delete", json!({ "joint": id })));
        ui.close();
    }
}

/// Joints after the features in the timeline: double-click edits, the menu drives, suppresses
/// or deletes. Returns the x after them.
pub fn timeline_joints(app: &mut SolveApp, ui: &mut egui::Ui, p: &egui::Painter, mut x: f32, r: egui::Rect) -> f32 {
    let t = Tokens::get();
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    if joints.is_empty() {
        return x;
    }
    x += 8.0;
    let mut open: Option<Dialog> = None;
    let mut run: Option<(&'static str, Value)> = None;
    for (id, name, kind, suppressed) in joints {
        let br = egui::Rect::from_min_size(egui::pos2(x, r.top() + 6.0), vec2(28.0, 28.0));
        let resp = ui.interact(br, ui.id().with(("joint", id)), egui::Sense::click());
        let bg = if resp.hovered() { t.hover } else { t.timeline_item };
        p.rect(br, 3.0, bg, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        crate::icons::paint(p, br.shrink(4.0), "joint", if suppressed { t.border } else { t.icon }, t.icon_fill, t.accent);
        if suppressed {
            p.line_segment([br.left_bottom() + vec2(4.0, -4.0), br.right_top() + vec2(-4.0, 4.0)], Stroke::new(1.5, t.text_dim));
        }
        let resp = resp.on_hover_text(format!("{name}  ({kind:?} joint)\nDouble-click to edit · right-click for more"));
        if resp.double_clicked() {
            open = edit_joint(app, id);
        }
        resp.context_menu(|ui| joint_menu(app, ui, (id, kind, suppressed), &mut open, &mut run));
        x += 32.0;
    }
    if let Some((c, prm)) = run {
        let _ = app.run(c, prm);
    }
    if let Some(d) = open {
        app.tool = None;
        app.dialog = Some(d);
    }
    x
}

/// The Joints folder at the end of the Browser: a row per joint (double-click edits, the menu
/// edits, drives, suppresses or deletes).
pub fn browser_joints(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let joints: Vec<(u64, String, JointKind, bool)> =
        app.session.doc.assembly.joints.iter().map(|j| (j.id, j.name.clone(), j.kind, j.suppressed)).collect();
    if joints.is_empty() {
        return;
    }
    let fold_id = egui::Id::new("sc_browser_joints_open");
    let mut open = ui.data(|d| d.get_temp::<bool>(fold_id)).unwrap_or(true);
    let row = |ui: &mut egui::Ui, depth: f32, icon: &str, label: &str, dim: bool, fold: Option<bool>| -> egui::Response {
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 3.0, t.hover);
        }
        let x = r.left() + 2.0 + depth * 14.0;
        let cy = r.center().y;
        if let Some(o) = fold {
            let c = egui::pos2(x + 6.0, cy);
            let pts = if o {
                vec![c + vec2(-4.0, -2.0), c + vec2(4.0, -2.0), c + vec2(0.0, 3.0)]
            } else {
                vec![c + vec2(-2.0, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.0, 4.0)]
            };
            ui.painter().add(egui::Shape::convex_polygon(pts, t.text_dim, Stroke::NONE));
        }
        let ir = egui::Rect::from_center_size(egui::pos2(x + 26.0, cy), vec2(16.0, 16.0));
        crate::icons::paint(ui.painter(), ir, icon, if dim { t.border } else { t.icon }, t.icon_fill, t.accent);
        ui.painter().text(
            egui::pos2(x + 38.0, cy),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(13.0),
            if dim { t.text_dim } else { t.text },
        );
        resp
    };
    if row(ui, 1.0, "folder", "Joints", false, Some(open)).clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(fold_id, open));
    }
    if !open {
        return;
    }
    let mut dialog: Option<Dialog> = None;
    let mut run: Option<(&'static str, Value)> = None;
    for (id, name, kind, suppressed) in joints {
        let resp = row(ui, 2.0, "joint", &name, suppressed, None).on_hover_text(format!("{kind:?} joint"));
        if resp.double_clicked() {
            dialog = edit_joint(app, id);
        }
        resp.context_menu(|ui| joint_menu(app, ui, (id, kind, suppressed), &mut dialog, &mut run));
    }
    if let Some((c, p)) = run {
        let _ = app.run(c, p);
    }
    if let Some(d) = dialog {
        app.tool = None;
        app.dialog = Some(d);
    }
}

#[cfg(test)]
#[path = "dialogs_assembly_tests.rs"]
mod tests;
