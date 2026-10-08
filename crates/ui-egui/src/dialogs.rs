//! Command dialogs (shown at the right of the viewport). A dialog collects values and selection
//! inputs (geometry picked in the viewport, never typed), then runs its command; it never
//! changes the design itself. Things selected before the command starts become its input.

use std::hash::{Hash, Hasher};

use egui::{Color32, RichText, Stroke, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Sel;
use solvecraft_engine::Session;
use solvecraft_engine::doc::expr::Kind as ValueKind;
use solvecraft_engine::doc::{AxisRef, Direction, FeatureKind, HoleKind, Operation, PatternKind, PlaneRef, ProfileSel};
use solvecraft_engine::geom::Vec3;

use crate::SolveApp;
use crate::selection::{self, AXES, Accept, BODIES, CURVES, EDGES, FACES, PLANAR_FACES, PLANES, PROFILES, SelInput};
use crate::theme::Tokens;
use crate::viewport::Hit;

const OPS: [&str; 4] = ["new", "join", "cut", "intersect"];
const OP_LABELS: [&str; 4] = ["New Body", "Join", "Cut", "Intersect"];
const DIRS: [&str; 4] = ["positive", "negative", "symmetric", "positive"];
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
        /// Taper angle (empty or zero: none).
        taper: String,
        /// Start offset from the profile plane (empty: start on the profile plane).
        start: String,
        /// Extent type All (through everything) instead of a distance.
        all: bool,
    },
    Revolve {
        angle: String,
        operation: usize,
    },
    Fillet {
        radius: String,
        chamfer: bool,
        chain: bool,
        /// Chamfer type: 0 equal distance, 1 two distances, 2 distance and angle; with the
        /// second distance, the angle and which face the first distance is on.
        ctype: usize,
        distance2: String,
        angle: String,
        flip: bool,
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
    /// A construction plane offset from a plane.
    OffsetPlane {
        offset: String,
    },
    /// A construction plane through an axis at an angle to a plane.
    AnglePlane {
        angle: String,
    },
    /// Split a body by a plane.
    Split,
    /// Scale bodies uniformly.
    Scale {
        factor: String,
    },
    /// Move planar faces along their normals.
    OffsetFaces {
        distance: String,
    },
    /// A cosmetic thread on a cylindrical face.
    Thread {
        designation: String,
        length: String,
    },
    /// A rib (one wall along open curves) or a web (a wall per curve).
    Rib {
        web: bool,
        thickness: String,
        depth: String,
        flip: bool,
    },
    /// Raise or sink profiles of a sketch on a face.
    Emboss {
        depth: String,
        deboss: bool,
    },
    /// Move planar faces onto a target plane or face.
    ReplaceFace,
    /// Move bodies so a face or point meets another.
    Align {
        flip: bool,
    },
    /// Remove bodies from here on in the timeline.
    Remove,
    /// Pattern bodies along a path of sketch curves.
    PathPattern {
        count: String,
        spacing: String,
    },
    /// A pipe (round, optionally hollow) along sketch curves.
    Pipe {
        diameter: String,
        wall: String,
    },
    /// A stock block around bodies.
    Stock {
        margin: String,
    },
    /// Application preferences (applied as they change; kept between runs).
    Preferences,
    /// Physical material (and with it the appearance) of bodies.
    Material {
        index: usize,
    },
    /// Section Analysis: cut the view by a plane, moved along its normal.
    Section {
        offset: String,
        flip: bool,
    },
    /// Measure one or two picked items (vertices, edges, faces): the result of the last
    /// measurement and the items it was for.
    Measure {
        result: Option<Value>,
        of: Vec<Sel>,
    },
    /// Move bodies by a distance along X, Y and Z.
    /// Move/Copy (see `dialogs_move`): the move type, X/Y/Z distances (Point to Position: the
    /// target), X/Y/Z angles (Free Move) and the angle (Rotate).
    Move {
        mode: usize,
        x: String,
        y: String,
        z: String,
        rx: String,
        ry: String,
        rz: String,
        angle: String,
    },
    Hole {
        diameter: String,
        depth: String,
        kind: usize,
        cb_diameter: String,
        cb_depth: String,
        cs_diameter: String,
        cs_angle: String,
        opts: HoleOpts,
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
    /// Joints and components (`dialogs_assembly`).
    Assembly(crate::dialogs_assembly::Asm),
    /// Sheet metal (`dialogs_sheet`).
    Sheet(crate::dialogs_sheet::Sm),
    /// Plastic features (`dialogs_plastic`).
    Plastic(crate::dialogs_plastic::Pl),
    /// Appearance (`dialogs_appearance`).
    Appearance(crate::dialogs_appearance::Ap),
    /// Contact sets, motion studies, exploded views, configurations (`dialogs_motion`).
    Motion(crate::dialogs_motion::Mo),
    /// Insert Part from the standard parts library (`dialogs_parts`).
    Part(crate::dialogs_parts::Pt),
}

/// The rest of the Hole dialog: placement, extents, tap type and drill point.
#[derive(Clone, Debug, PartialEq)]
pub struct HoleOpts {
    /// Placement at several sketch points instead of one face point.
    pub multiple: bool,
    /// Extents: through all instead of a distance.
    pub all: bool,
    /// Drill point: an angled tip instead of a flat bottom.
    pub angled: bool,
    pub tip_angle: String,
    /// Tap type: tapped (a cosmetic thread) instead of simple.
    pub tapped: bool,
    pub thread: String,
}

impl Default for HoleOpts {
    fn default() -> Self {
        HoleOpts { multiple: false, all: false, angled: true, tip_angle: "118 deg".into(), tapped: false, thread: "M5".into() }
    }
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
    /// Folded to its header (the "−" glyph).
    pub collapsed: bool,
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
        Sel::Feature { .. } => a & selection::FEATURES != 0,
        Sel::SketchPoint { .. } | Sel::SketchConstraint { .. } => false,
    }
}

impl Dialog {
    pub(crate) fn new(kind: Kind, inputs: Vec<SelInput>) -> Dialog {
        Dialog { kind, inputs, active: 0, error: None, editing: None, focus: true, collapsed: false, extra: serde_json::Map::new() }
    }

    pub fn for_command(app: &SolveApp, id: &str) -> Option<Dialog> {
        let s = &app.session;
        let has_bodies = !s.world_state().bodies.is_empty();
        let mut d = match id {
            "SketchCreate" => Dialog::new(Kind::Sketch, vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)]),
            "Extrude" => Dialog::new(
                Kind::Extrude {
                    distance: "10 mm".into(),
                    direction: 0,
                    operation: usize::from(has_bodies),
                    auto_op: true,
                    distance2: "10 mm".into(),
                    taper: "0 deg".into(),
                    start: String::new(),
                    all: false,
                },
                vec![SelInput::new("Profiles", PROFILES | PLANAR_FACES, true)],
            ),
            "Revolve" => Dialog::new(
                Kind::Revolve { angle: "360 deg".into(), operation: 0 },
                vec![SelInput::new("Profile", PROFILES, true), SelInput::new("Axis", AXES, false)],
            ),
            "FusionFilletEdgesCommand" => Dialog::new(fillet_kind("2 mm", false), vec![SelInput::new("Edges", EDGES | FACES, true)]),
            "FusionChamferCommand" => Dialog::new(fillet_kind("1 mm", true), vec![SelInput::new("Edges", EDGES | FACES, true)]),
            "FusionShellBodyCommand" => Dialog::new(Kind::Shell { thickness: "2 mm".into() }, vec![SelInput::new("Faces/Body", FACES, true)]),
            "FusionDraftCommand" => Dialog::new(
                Kind::Draft { angle: "5 deg".into() },
                vec![SelInput::new("Faces", FACES, true), SelInput::new("Neutral plane", PLANES | PLANAR_FACES, false)],
            ),
            "MirrorCommand" => {
                Dialog::new(Kind::Mirror, vec![SelInput::new("Objects", BODIES, true), SelInput::new("Mirror Plane", PLANES | PLANAR_FACES, false)])
            }
            "FusionRibCommand" | "FusionWebCommand" => Dialog::new(
                Kind::Rib { web: id == "FusionWebCommand", thickness: "2 mm".into(), depth: String::new(), flip: false },
                vec![SelInput::new("Curves", CURVES, true)],
            ),
            "EmbossCmd" => Dialog::new(Kind::Emboss { depth: "1 mm".into(), deboss: false }, vec![SelInput::new("Profiles", PROFILES, true)]),
            "FusionReplaceFaceCommand" => Dialog::new(
                Kind::ReplaceFace,
                vec![SelInput::new("Faces", PLANAR_FACES, true), SelInput::new("Target", PLANES | PLANAR_FACES, false)],
            ),
            "AlignCmd" => Dialog::new(
                Kind::Align { flip: false },
                vec![
                    SelInput::new("Bodies", BODIES, true),
                    SelInput::new("From", FACES | selection::VERTICES, false),
                    SelInput::new("To", FACES | selection::VERTICES, false),
                ],
            ),
            "SoftDeleteCommand" => Dialog::new(Kind::Remove, vec![SelInput::new("Bodies", BODIES, true)]),
            "PatternOnPath" => Dialog::new(
                Kind::PathPattern { count: "4".into(), spacing: "20 mm".into() },
                vec![SelInput::new("Objects", BODIES, true), SelInput::new("Path", CURVES, true)],
            ),
            "PrimitivePipe" => Dialog::new(Kind::Pipe { diameter: "5 mm".into(), wall: String::new() }, vec![SelInput::new("Path", CURVES, true)]),
            "StockModelCommand" => Dialog::new(Kind::Stock { margin: "2 mm".into() }, vec![SelInput::new("Bodies (all if none)", BODIES, true)]),
            "ConstructionPlaneOffsetFromPlaneCommand" => {
                Dialog::new(Kind::OffsetPlane { offset: "10 mm".into() }, vec![SelInput::new("Plane", PLANES, false)])
            }
            "ConstructionPlaneAtAngleCommand" => Dialog::new(
                Kind::AnglePlane { angle: "45 deg".into() },
                vec![SelInput::new("Plane", PLANES, false), SelInput::new("Axis", AXES, false)],
            ),
            "FusionSplitBodyCommand" => Dialog::new(
                Kind::Split,
                vec![SelInput::new("Body to split", BODIES, false), SelInput::new("Splitting plane", PLANES | PLANAR_FACES, false)],
            ),
            "ModifyScale" => Dialog::new(Kind::Scale { factor: "2".into() }, vec![SelInput::new("Bodies", BODIES, true)]),
            "FusionOffsetFacesCommand" => {
                Dialog::new(Kind::OffsetFaces { distance: "2 mm".into() }, vec![SelInput::new("Faces", PLANAR_FACES, true)])
            }
            "FusionThreadCommand" => {
                Dialog::new(Kind::Thread { designation: String::new(), length: String::new() }, vec![SelInput::new("Cylindrical face", FACES, false)])
            }
            "PhysicalMaterialCommand" => Dialog::new(Kind::Material { index: 1 }, vec![SelInput::new("Bodies", BODIES, true)]),
            "FusionHalfSectionViewCommand" => {
                Dialog::new(Kind::Section { offset: "0 mm".into(), flip: false }, vec![SelInput::new("Plane", PLANES | PLANAR_FACES, false)])
            }
            "MeasureCommand" => {
                Dialog::new(Kind::Measure { result: None, of: Vec::new() }, vec![SelInput::new("Items", FACES | EDGES | selection::VERTICES, true)])
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
            "FusionMoveCommand" => Dialog::new(crate::dialogs_move::kind(0), crate::dialogs_move::inputs(0)),
            "FusionHoleCommand" => Dialog::new(hole_defaults(), vec![SelInput::new("Face", PLANAR_FACES, true)]),
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
            "PrimitiveCoil" => Dialog::new(
                Kind::Primitive {
                    cmd: "PrimitiveCoil",
                    fields: vec![("diameter", "40".into()), ("revolutions", "4".into()), ("pitch", "8".into()), ("section_size", "3".into())],
                    operation: 0,
                },
                vec![],
            ),
            "FusionCombineCommand" => Dialog::new(
                Kind::Combine { operation: 1, keep_tools: false },
                vec![SelInput::new("Target Body", BODIES, false), SelInput::new("Tool Bodies", BODIES, true)],
            ),
            "ChangeParameterCommand" => Dialog::new(Kind::Params { new_name: String::new(), new_expr: String::new() }, vec![]),
            _ => {
                let (kind, inputs) = crate::dialogs_assembly::start(app, id)
                    .or_else(|| crate::dialogs_sheet::start(app, id))
                    .or_else(|| crate::dialogs_plastic::start(app, id))
                    .or_else(|| crate::dialogs_appearance::start(app, id))
                    .or_else(|| crate::dialogs_motion::start(app, id))
                    .or_else(|| crate::dialogs_parts::start(app, id))?;
                Dialog::new(kind, inputs)
            }
        };
        // Patterns of features picked in the timeline: the objects are features.
        if is_pattern(&d.kind)
            && s.selection.iter().any(|x| matches!(x, Sel::Feature { .. }))
            && let Some(inp) = d.inputs.first_mut()
        {
            inp.accept = selection::FEATURES;
            inp.label = objects_label(inp.accept);
        }
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
            // Planar-only inputs take planar faces only.
            let planar_ok = match sel {
                Sel::Face { body, index, .. } => planar_face(s, body, *index).is_some(),
                _ => true,
            };
            let takes = |i: &SelInput| {
                fits(i.accept, sel) && (i.multi || i.items.is_empty()) && (planar_ok || i.accept & FACES != 0 || !matches!(sel, Sel::Face { .. }))
            };
            if let Some(inp) = d.inputs.iter_mut().find(|i| takes(i)) {
                // Fillets and chamfers take an edge with its tangent chain, as a pick does.
                let picked = match (&d.kind, sel) {
                    (Kind::Fillet { chain: true, .. }, Sel::Edge { body, index, .. }) => s
                        .model
                        .state()
                        .body(body)
                        .map(|b| {
                            let m = b.mesh();
                            m.tangent_chain(*index, 2f64.to_radians())
                                .into_iter()
                                .filter_map(|i| m.edges.get(i).map(|e| Sel::Edge { body: body.clone(), index: i, point: polyline_mid(e) }))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_else(|| vec![sel.clone()]),
                    _ => vec![sel.clone()],
                };
                for x in picked {
                    if !inp.items.contains(&x) {
                        inp.items.push(x);
                    }
                }
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

    pub fn preferences() -> Dialog {
        Dialog::new(Kind::Preferences, vec![])
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
        match &self.kind {
            Kind::Assembly(k) => return k.previews(),
            Kind::Sheet(k) => return k.previews(),
            Kind::Plastic(k) => return k.previews(),
            Kind::Part(_) => return true,
            _ => {}
        }
        matches!(
            self.kind,
            Kind::Extrude { .. }
                | Kind::Revolve { .. }
                | Kind::Fillet { .. }
                | Kind::Shell { .. }
                | Kind::Draft { .. }
                | Kind::Mirror
                | Kind::Split
                | Kind::Rib { .. }
                | Kind::Emboss { .. }
                | Kind::ReplaceFace
                | Kind::Align { .. }
                | Kind::Remove
                | Kind::PathPattern { .. }
                | Kind::Pipe { .. }
                | Kind::Stock { .. }
                | Kind::Scale { .. }
                | Kind::OffsetFaces { .. }
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
            Kind::Move { x, .. } => ("X", ValueKind::Length, x),
            Kind::PatternRect { spacing, .. } => ("Spacing", ValueKind::Length, spacing),
            Kind::PatternCirc { angle, .. } => ("Angle", ValueKind::Angle, angle),
            Kind::Section { offset, .. } => ("Distance", ValueKind::Length, offset),
            Kind::OffsetPlane { offset } => ("Distance", ValueKind::Length, offset),
            Kind::Rib { thickness, .. } => ("Thickness", ValueKind::Length, thickness),
            Kind::Emboss { depth, .. } => ("Depth", ValueKind::Length, depth),
            Kind::PathPattern { spacing, .. } => ("Spacing", ValueKind::Length, spacing),
            Kind::Pipe { diameter, .. } => ("Diameter", ValueKind::Length, diameter),
            Kind::Stock { margin } => ("Margin", ValueKind::Length, margin),
            Kind::AnglePlane { angle } => ("Angle", ValueKind::Angle, angle),
            Kind::Scale { factor } => ("Scale", ValueKind::Unitless, factor),
            Kind::OffsetFaces { distance } => ("Distance", ValueKind::Length, distance),
            Kind::Sheet(k) => return k.primary(&self.inputs),
            Kind::Plastic(k) => return k.primary(),
            _ => return None,
        })
    }

    /// Dialogs holding a table get the wide limit.
    pub fn is_wide(&self) -> bool {
        matches!(self.kind, Kind::Params { .. }) || matches!(&self.kind, Kind::Motion(k) if k.wide())
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
        let inp = self.active_input()?;
        // Features are also picked on the canvas: a face stands for the feature that made it.
        if inp.accept == selection::FEATURES {
            return match h {
                Hit::Face { body, index, point } => feature_of_face(s, body, *index, *point).map(|id| Sel::Feature { id }),
                _ => None,
            };
        }
        inp.accepts(h, |body, face| planar_face(s, body, face).is_some())
    }

    /// A pick in the viewport (already accepted by the active input).
    pub fn pick(&mut self, s: &Session, sel: Sel) {
        let chain = matches!(self.kind, Kind::Fillet { chain: true, .. });
        let mut picked = vec![sel.clone()];
        // Tangent chain: an edge brings the edges that continue it smoothly.
        if chain
            && let Sel::Edge { body, index, .. } = &sel
            && let Some(b) = s.world_state().body(body)
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
        // A measurement is between two items: a third pick starts over from the last one.
        if matches!(self.kind, Kind::Measure { .. }) && inp.items.len() > 2 {
            let keep = inp.items.split_off(inp.items.len() - 1);
            inp.items = keep;
        }
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
    let st = s.world_state();
    let sid = s.active_sketch.or_else(|| s.doc.features.iter().rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id));
    match sid.and_then(|id| st.sketch(id).map(|ss| (id, ss.profiles.len()))) {
        Some((id, 1)) => vec![Sel::Profile { sketch: id, index: 0 }],
        _ => Vec::new(),
    }
}

/// The plane of a planar body face: (point on it, unit normal).
pub fn planar_face(s: &Session, body: &str, face: usize) -> Option<(Vec3, Vec3)> {
    let st = s.world_state();
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
pub(crate) fn field(ui: &mut egui::Ui, s: &mut String) -> bool {
    let r = ui.text_edit_singleline(s);
    crate::params_dialog::complete(ui, &r, s);
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

pub(crate) fn combo(ui: &mut egui::Ui, id: &str, labels: &[&str], sel: &mut usize) {
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
        Kind::Measure { .. } => "MEASURE",
        Kind::Material { .. } => "PHYSICAL MATERIAL",
        Kind::Preferences => "PREFERENCES",
        Kind::OffsetPlane { .. } => "OFFSET PLANE",
        Kind::Rib { web: false, .. } => "RIB",
        Kind::Rib { web: true, .. } => "WEB",
        Kind::Emboss { .. } => "EMBOSS",
        Kind::ReplaceFace => "REPLACE FACE",
        Kind::Align { .. } => "ALIGN",
        Kind::Remove => "REMOVE",
        Kind::PathPattern { .. } => "PATTERN ON PATH",
        Kind::Pipe { .. } => "PIPE",
        Kind::Stock { .. } => "STOCK",
        Kind::AnglePlane { .. } => "PLANE AT ANGLE",
        Kind::Split => "SPLIT BODY",
        Kind::Scale { .. } => "SCALE",
        Kind::OffsetFaces { .. } => "OFFSET FACE",
        Kind::Thread { .. } => "THREAD",
        Kind::Section { .. } => "SECTION ANALYSIS",
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
            "PrimitiveCoil" => "COIL",
            _ => "TORUS",
        },
        Kind::Combine { .. } => "COMBINE",
        Kind::Params { .. } => "PARAMETERS",
        Kind::EditParam { .. } => "EDIT DIMENSION",
        Kind::Rename { .. } => "RENAME",
        Kind::ConfirmDelete { .. } => "DELETE FEATURE",
        Kind::Assembly(k) => k.title(),
        Kind::Sheet(k) => k.title(),
        Kind::Plastic(k) => k.title(),
        Kind::Appearance(_) => "APPEARANCE",
        Kind::Motion(k) => k.title(),
        Kind::Part(_) => "INSERT PART",
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

/// Width of the label column; longer labels are cut with "…".
const LABEL_W: f32 = 92.0;

/// A row label in the dialog's label column.
pub(crate) fn row_label(ui: &mut egui::Ui, text: &str) {
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(LABEL_W);
        ui.add(egui::Label::new(text).truncate()).on_hover_text(text);
    });
}

thread_local! {
    /// Faces already traced to their feature, by (revision, body, face).
    static FACE_FEATURE: std::cell::RefCell<(u64, std::collections::HashMap<(String, usize), Option<u64>>)> = Default::default();
}

/// The feature that made a face: the latest feature before which the face's point lay on no
/// face of the model.
fn feature_of_face(s: &Session, body: &str, index: usize, p: Vec3) -> Option<u64> {
    let key = (body.to_string(), index);
    if let Some(hit) = FACE_FEATURE.with(|c| {
        let c = c.borrow();
        if c.0 == s.revision { c.1.get(&key).copied() } else { None }
    }) {
        return hit;
    }
    let on_model = |st: &solvecraft_engine::doc::ModelState| {
        st.bodies.iter().any(|b| {
            let m = b.mesh();
            let bb = m.bounds();
            let tol = 1e-4 * (1.0 + bb.diagonal());
            p.x >= bb.min.x - tol
                && p.y >= bb.min.y - tol
                && p.z >= bb.min.z - tol
                && p.x <= bb.max.x + tol
                && p.y <= bb.max.y + tol
                && p.z <= bb.max.z + tol
                && m.triangles.iter().filter_map(|t| m.tri(t)).any(|t| point_tri_dist(p, t) < tol)
        })
    };
    let found = s
        .doc
        .features
        .iter()
        .rev()
        .filter(|f| !matches!(f.kind, FeatureKind::Sketch { .. }) && s.model.result(f.id).is_some_and(|r| r.error.is_none()))
        .find(|f| !on_model(&s.model.state_before(f.id)))
        .map(|f| f.id);
    FACE_FEATURE.with(|c| {
        let mut c = c.borrow_mut();
        if c.0 != s.revision {
            *c = (s.revision, std::collections::HashMap::new());
        }
        c.1.insert(key, found);
    });
    found
}

/// Distance from a point to a triangle.
pub(crate) fn point_tri_dist(p: Vec3, [a, b, c]: [Vec3; 3]) -> f64 {
    let n = (b - a).cross(c - a);
    let nn = n.dot(n);
    if nn > 1e-24 {
        let q = p - n * ((p - a).dot(n) / nn);
        let inside = [(a, b), (b, c), (c, a)].iter().all(|(u, v)| (*v - *u).cross(q - *u).dot(n) >= 0.0);
        if inside {
            return (p - q).len();
        }
    }
    p.dist_to_segment(a, b).min(p.dist_to_segment(b, c)).min(p.dist_to_segment(c, a))
}

/// The pattern objects' label: features are picked in the timeline.
fn objects_label(accept: Accept) -> &'static str {
    if accept == selection::FEATURES { "Features" } else { "Objects" }
}

fn is_pattern(k: &Kind) -> bool {
    matches!(k, Kind::PatternRect { .. } | Kind::PatternCirc { .. } | Kind::PathPattern { .. } | Kind::Mirror)
}

/// The Object Type row of a pattern: bodies (picked in the view) or features (picked in the
/// timeline, e.g. a hole).
fn object_type_row(d: &mut Dialog, ui: &mut egui::Ui) {
    let Some(inp) = d.inputs.first_mut() else { return };
    row_label(ui, "Object Type");
    let cur = usize::from(inp.accept == selection::FEATURES);
    let mut ty = cur;
    combo(ui, "pat_obj", &["Bodies", "Features"], &mut ty);
    ui.end_row();
    if ty != cur {
        inp.accept = if ty == 1 { selection::FEATURES } else { BODIES };
        inp.label = objects_label(inp.accept);
        inp.items.clear();
        d.active = 0;
    }
}

/// Rows above the selection inputs: the pattern type and object type, the Hole placement (one
/// face point, or sketch points).
fn placement_row(d: &mut Dialog, ui: &mut egui::Ui) {
    if matches!(d.kind, Kind::Mirror) {
        object_type_row(d, ui);
        return;
    }
    if matches!(d.kind, Kind::Move { .. }) {
        crate::dialogs_move::type_row(d, ui);
        return;
    }
    let current = match d.kind {
        Kind::PatternRect { .. } => Some(0),
        Kind::PatternCirc { .. } => Some(1),
        Kind::PathPattern { .. } => Some(2),
        _ => None,
    };
    if let Some(cur) = current {
        row_label(ui, "Type");
        let mut ty = cur;
        combo(ui, "pat_type", &["Rectangular", "Circular", "Path"], &mut ty);
        ui.end_row();
        if ty != cur {
            // Same objects, the new pattern's other inputs.
            let objects = d.inputs.first().map(|i| i.items.clone()).unwrap_or_default();
            let (kind, mut inputs) = match ty {
                0 => (
                    Kind::PatternRect { count: "3".into(), spacing: "20 mm".into(), count2: "1".into(), spacing2: "20 mm".into() },
                    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Direction", AXES, false), SelInput::new("Direction 2", AXES, false)],
                ),
                1 => (
                    Kind::PatternCirc { count: "6".into(), angle: "360 deg".into() },
                    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Axis", AXES, false)],
                ),
                _ => (
                    Kind::PathPattern { count: "4".into(), spacing: "20 mm".into() },
                    vec![SelInput::new("Objects", BODIES, true), SelInput::new("Path", CURVES, true)],
                ),
            };
            let accept = d.inputs.first().map(|i| i.accept).unwrap_or(BODIES);
            if let Some(i) = inputs.first_mut() {
                i.items = objects;
                i.accept = accept;
                i.label = objects_label(accept);
            }
            d.kind = kind;
            d.inputs = inputs;
            d.extra.clear();
            d.advance();
        }
        object_type_row(d, ui);
        return;
    }
    let Kind::Hole { opts, .. } = &mut d.kind else { return };
    row_label(ui, "Placement");
    let mut m = usize::from(opts.multiple);
    combo(ui, "hole_place", &["Single", "Multiple"], &mut m);
    ui.end_row();
    if (m == 1) != opts.multiple {
        opts.multiple = m == 1;
        d.inputs = vec![if opts.multiple { SelInput::new("Points", selection::POINTS, true) } else { SelInput::new("Face", PLANAR_FACES, true) }];
        d.active = 0;
        d.extra.remove("direction");
    }
}

/// Selection input rows: label, "N selected" (or a hint) and a clear button; clicking a row
/// makes it the active input.
fn input_rows(d: &mut Dialog, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut clear: Option<usize> = None;
    let mut activate: Option<usize> = None;
    for (i, inp) in d.inputs.iter().enumerate() {
        row_label(ui, inp.label);
        ui.horizontal(|ui| {
            let active = i == d.active;
            // A chip: "↖ Select" (outlined) while empty, "N selected" (filled) once picked.
            let filled = !inp.items.is_empty();
            let text = if filled {
                RichText::new(format!("↖ {} selected", inp.items.len())).color(Color32::WHITE)
            } else {
                RichText::new("↖ Select").color(t.text)
            };
            let stroke = if active { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.dialog_border.gamma_multiply(1.6)) };
            // Not in the Tab order: Tab moves between values.
            let b = egui::Button::new(text)
                .fill(if filled { t.chip } else { Color32::TRANSPARENT })
                .stroke(stroke)
                .min_size(vec2(0.0, 22.0))
                .sense(egui::Sense::CLICK);
            if ui.add(b).on_hover_text(format!("{} (click to pick into this input)", hint(inp))).clicked() {
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
use crate::frame::{DIALOG_MAX_W, DIALOG_MIN_W, DIALOG_WIDE_MAX_W};
/// Width of value fields and choice boxes in a dialog.
const FIELD_W: f32 = 124.0;

/// Measure again when the picked items changed.
fn update_measure(app: &SolveApp, d: &mut Dialog) {
    let items: Vec<Sel> = sels(d, 0).to_vec();
    if let Kind::Measure { result, of } = &mut d.kind
        && *of != items
    {
        *result = if items.is_empty() { None } else { Some(app.session.measure_items(&items)) };
        *of = items;
    }
}

/// The preferences, applied as they change.
fn preferences_rows(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui.label("Theme");
    let mut theme = usize::from(!app.ui.dark);
    combo(ui, "pref_theme", &["Dark", "Light"], &mut theme);
    app.ui.dark = theme == 0;
    ui.end_row();
    ui.label("Grid");
    ui.checkbox(&mut app.ui.show_grid, "");
    ui.end_row();
    ui.label("Perspective view");
    ui.checkbox(&mut app.ui.perspective, "");
    ui.end_row();
    ui.label("Project face edges into new sketches");
    let mut auto = app.session.auto_project;
    if ui.checkbox(&mut auto, "").changed() {
        let _ = app.run("sketch.auto_project", json!({ "value": auto }));
    }
    ui.end_row();
    ui.label("Click selects whole bodies");
    ui.checkbox(&mut app.ui.pick_bodies, "");
    ui.end_row();
}

fn fmt_mm(v: &Value) -> String {
    v.as_f64().map(|x| format!("{x:.3} mm")).unwrap_or_default()
}

/// The measurement in the dialog.
fn measure_rows(ui: &mut egui::Ui, r: Option<&Value>) {
    let t = Tokens::get();
    let Some(r) = r else {
        ui.label("");
        ui.label(RichText::new("pick one or two vertices, edges or faces").color(t.text_dim));
        ui.end_row();
        return;
    };
    if let Some(e) = r.get("error").and_then(Value::as_str) {
        ui.label("");
        ui.label(RichText::new(e).color(t.error));
        ui.end_row();
        return;
    }
    for (i, it) in r["items"].as_array().into_iter().flatten().enumerate() {
        let what = match it["type"].as_str() {
            Some("edge") => format!("Length  {}", fmt_mm(&it["length_mm"])),
            Some("face") => format!("Area  {:.3} mm²", it["area_mm2"].as_f64().unwrap_or(0.0)),
            Some("vertex") => {
                let p = &it["position"];
                format!("At  {:.3}, {:.3}, {:.3}", p[0].as_f64().unwrap_or(0.0), p[1].as_f64().unwrap_or(0.0), p[2].as_f64().unwrap_or(0.0))
            }
            _ => String::new(),
        };
        ui.label(format!("Item {}", i + 1));
        ui.label(what);
        ui.end_row();
    }
    if r.get("distance_mm").is_some() {
        ui.label(RichText::new("Distance").strong());
        ui.label(RichText::new(fmt_mm(&r["distance_mm"])).strong());
        ui.end_row();
        let d = &r["delta"];
        for (k, l) in ["ΔX", "ΔY", "ΔZ"].iter().enumerate() {
            ui.label(*l);
            ui.label(format!("{:.3} mm", d[k].as_f64().unwrap_or(0.0).abs()));
            ui.end_row();
        }
    }
    if let Some(a) = r["angle_deg"].as_f64() {
        ui.label(RichText::new("Angle").strong());
        ui.label(RichText::new(format!("{a:.3}°")).strong());
        ui.end_row();
    }
}

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
    update_measure(app, &mut d);
    let t = Tokens::get();
    let vp = app.viewport.rect.unwrap_or_else(|| ctx.content_rect());
    let anchor = egui::pos2(vp.right(), vp.top() + crate::viewport::VIEW_CUBE_CLEARANCE);
    let mut keep = true;
    let mut ok = false;
    let mut applied = false;
    let mut cancel = false;
    let mut enter = false;
    let wide = d.is_wide();
    let heading = if d.editing.is_some() { format!("EDIT {}", title(&d.kind)) } else { title(&d.kind).to_string() };
    let frame = egui::Frame::window(&ctx.global_style())
        .fill(t.dialog_bg)
        .stroke(Stroke::new(1.0, t.dialog_border))
        .corner_radius(egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 })
        .shadow(egui::Shadow { offset: [-2, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(40) });
    // OK is offered once the inputs make a feature.
    let has_ok = !matches!(d.kind, Kind::Sketch | Kind::Params { .. } | Kind::Measure { .. } | Kind::Preferences);
    let valid = !has_ok || !d.previews() || (apply_commands(app, &d).is_ok() && app.preview.error.is_none());
    let width = if wide { crate::frame::Width::Wide } else { crate::frame::Width::Normal };
    let wkey = egui::Id::new(("sc_dialog_w", wide));
    crate::frame::docked(
        RichText::new(heading.clone()).strong().size(13.0),
        egui::Id::new(("sc_dialog", title(&d.kind))),
        anchor,
        vp.bottom() - anchor.y - 8.0,
        width,
    )
    .frame(frame)
    .scroll([false, true])
    .open(&mut keep)
    .show(ctx, |ui| {
        ui.spacing_mut().text_edit_width = FIELD_W;
        // Long texts (errors, notes) wrap instead of widening the dialog.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        let margin = 2.0 * ui.style().spacing.window_margin.leftf();
        let min_w = if wide { 480.0 } else { DIALOG_MIN_W } - margin;
        ui.set_min_width(min_w);
        ui.set_max_width(if wide { DIALOG_WIDE_MAX_W } else { DIALOG_MAX_W } - margin);
        // Rows that end at the right edge line up with last frame's content width.
        let total = crate::frame::content_width(ui, wkey, min_w);
        // Header: "−" folds the dialog, the command name, "»" at the right.
        ui.horizontal(|ui| {
            let fold = if d.collapsed { "+" } else { "−" };
            if ui
                .add(egui::Button::new(RichText::new(fold).size(14.0)).frame(false))
                .on_hover_text(if d.collapsed { "Expand" } else { "Collapse" })
                .clicked()
            {
                d.collapsed = !d.collapsed;
            }
            ui.label(RichText::new(heading.as_str()).strong().size(12.5));
            crate::frame::right_aligned(ui, total, 8.0, |ui| ui.label(RichText::new("»").color(t.text_dim)));
        });
        if d.collapsed {
            return;
        }
        ui.add_space(2.0);
        egui::Grid::new("sc_dialog_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
            placement_row(&mut d, ui);
            input_rows(&mut d, ui);
            match &mut d.kind {
                Kind::Sketch => {}
                Kind::Extrude { distance, direction, operation, auto_op, distance2, taper, start, all } => {
                    row_label(ui, "Start");
                    let mut st = usize::from(!start.is_empty());
                    combo(ui, "ex_start", &["Profile Plane", "Offset"], &mut st);
                    if st == 1 && start.is_empty() {
                        *start = "5 mm".into();
                    } else if st == 0 {
                        start.clear();
                    }
                    ui.end_row();
                    if !start.is_empty() {
                        row_label(ui, "Offset");
                        enter |= field(ui, start);
                        ui.end_row();
                    }
                    row_label(ui, "Direction");
                    // One Side, Two Sides, Symmetric (a flip is a negative distance).
                    let order = [0usize, TWO_SIDES, 2];
                    let mut pos = order.iter().position(|o| o == direction).unwrap_or(0);
                    combo(ui, "ex_dir", &["One Side", "Two Sides", "Symmetric"], &mut pos);
                    *direction = order.get(pos).copied().unwrap_or(0);
                    ui.end_row();
                    row_label(ui, "Extent Type");
                    let mut ext = usize::from(*all);
                    combo(ui, "ex_ext", &["Distance", "All"], &mut ext);
                    *all = ext == 1;
                    ui.end_row();
                    if !*all {
                        row_label(ui, "Distance");
                        enter |= field(ui, distance);
                        ui.end_row();
                        if *direction == TWO_SIDES {
                            row_label(ui, "Distance 2");
                            enter |= field(ui, distance2);
                            ui.end_row();
                        }
                    }
                    if *direction < 2 {
                        row_label(ui, "Taper Angle");
                        enter |= field(ui, taper);
                        ui.end_row();
                    }
                    row_label(ui, "Operation");
                    let before = *operation;
                    combo(ui, "ex_op", &OP_LABELS, operation);
                    // A choice made by hand sticks.
                    if *operation != before {
                        *auto_op = false;
                    }
                    ui.end_row();
                }
                Kind::Revolve { angle, operation } => {
                    row_label(ui, "Angle");
                    enter |= field(ui, angle);
                    ui.end_row();
                    row_label(ui, "Operation");
                    combo(ui, "rv_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Kind::Fillet { radius, chamfer, chain, ctype, distance2, angle, flip } => {
                    if *chamfer {
                        row_label(ui, "Chamfer Type");
                        combo(ui, "ch_type", &CHAMFER_TYPES, ctype);
                        ui.end_row();
                    }
                    row_label(
                        ui,
                        if !*chamfer {
                            "Radius"
                        } else if *ctype == 0 {
                            "Distance"
                        } else {
                            "Distance 1"
                        },
                    );
                    enter |= field(ui, radius);
                    ui.end_row();
                    if *chamfer && *ctype == 1 {
                        row_label(ui, "Distance 2");
                        enter |= field(ui, distance2);
                        ui.end_row();
                    }
                    if *chamfer && *ctype == 2 {
                        row_label(ui, "Angle");
                        enter |= field(ui, angle);
                        ui.end_row();
                    }
                    if *chamfer && *ctype > 0 {
                        row_label(ui, "Flip");
                        ui.checkbox(flip, "");
                        ui.end_row();
                    }
                    row_label(ui, "Tangent Chain");
                    ui.checkbox(chain, "");
                    ui.end_row();
                }
                Kind::Shell { thickness } => {
                    row_label(ui, "Inside Thickness");
                    enter |= field(ui, thickness);
                    ui.end_row();
                }
                Kind::Draft { angle } => {
                    row_label(ui, "Angle");
                    enter |= field(ui, angle);
                    ui.end_row();
                }
                Kind::Mirror => {
                    row_label(ui, "Operation");
                    let mut op = usize::from(d.extra.get("combine").and_then(Value::as_bool).unwrap_or(false));
                    combo(ui, "mi_op", &["New Body", "Join"], &mut op);
                    d.extra.insert("combine".into(), json!(op == 1));
                    ui.end_row();
                }
                Kind::Measure { result, .. } => measure_rows(ui, result.as_ref()),
                Kind::Preferences => preferences_rows(app, ui),
                Kind::OffsetPlane { offset } => {
                    row_label(ui, "Distance");
                    enter |= field(ui, offset);
                    ui.end_row();
                }
                Kind::AnglePlane { angle } => {
                    row_label(ui, "Angle");
                    enter |= field(ui, angle);
                    ui.end_row();
                }
                Kind::Split | Kind::ReplaceFace | Kind::Remove => {}
                Kind::Rib { thickness, depth, flip, .. } => {
                    row_label(ui, "Thickness");
                    enter |= field(ui, thickness);
                    ui.end_row();
                    row_label(ui, "Depth");
                    let r = ui.add(egui::TextEdit::singleline(depth).hint_text("to the next face"));
                    crate::params_dialog::complete(ui, &r, depth);
                    enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.end_row();
                    row_label(ui, "Flip");
                    ui.checkbox(flip, "");
                    ui.end_row();
                }
                Kind::Emboss { depth, deboss } => {
                    row_label(ui, "Depth");
                    enter |= field(ui, depth);
                    ui.end_row();
                    row_label(ui, "Type");
                    let mut i = usize::from(*deboss);
                    combo(ui, "emb_mode", &["Emboss", "Deboss"], &mut i);
                    *deboss = i == 1;
                    ui.end_row();
                }
                Kind::Align { flip } => {
                    row_label(ui, "Flip");
                    ui.checkbox(flip, "");
                    ui.end_row();
                }
                Kind::PathPattern { count, spacing } => {
                    for (l, v) in [("Quantity", count), ("Spacing", spacing)] {
                        row_label(ui, l);
                        enter |= field(ui, v);
                        ui.end_row();
                    }
                }
                Kind::Pipe { diameter, wall } => {
                    row_label(ui, "Diameter");
                    enter |= field(ui, diameter);
                    ui.end_row();
                    row_label(ui, "Wall");
                    let r = ui.add(egui::TextEdit::singleline(wall).hint_text("solid"));
                    crate::params_dialog::complete(ui, &r, wall);
                    enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.end_row();
                }
                Kind::Stock { margin } => {
                    row_label(ui, "Margin");
                    enter |= field(ui, margin);
                    ui.end_row();
                }
                Kind::Scale { factor } => {
                    row_label(ui, "Scale factor");
                    enter |= field(ui, factor);
                    ui.end_row();
                }
                Kind::OffsetFaces { distance } => {
                    row_label(ui, "Distance");
                    enter |= field(ui, distance);
                    ui.end_row();
                }
                Kind::Thread { designation, length } => {
                    // Fusion's order: Full Length, then the size; a length only when not full.
                    row_label(ui, "Full Length");
                    let mut full = length.trim().is_empty();
                    if ui.checkbox(&mut full, "").changed() {
                        *length = if full { String::new() } else { "10 mm".into() };
                    }
                    ui.end_row();
                    row_label(ui, "Size");
                    let r = ui.add(egui::TextEdit::singleline(designation).hint_text("fits the face, e.g. M8"));
                    enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.end_row();
                    if !full {
                        row_label(ui, "Length");
                        enter |= field(ui, length);
                        ui.end_row();
                    }
                }
                Kind::Material { index } => {
                    let names: Vec<&str> = solvecraft_engine::doc::MATERIALS.iter().map(|(n, _)| *n).collect();
                    row_label(ui, "Material");
                    combo(ui, "mat", &names, index);
                    ui.end_row();
                }
                Kind::Section { offset, flip } => {
                    row_label(ui, "Distance");
                    enter |= field(ui, offset);
                    ui.end_row();
                    row_label(ui, "Flip");
                    ui.checkbox(flip, "");
                    ui.end_row();
                }
                Kind::PatternRect { count, spacing, count2, spacing2 } => {
                    for (l, v) in [("Quantity", count), ("Spacing", spacing), ("Quantity 2", count2), ("Spacing 2", spacing2)] {
                        row_label(ui, l);
                        enter |= field(ui, v);
                        ui.end_row();
                    }
                }
                Kind::PatternCirc { count, angle } => {
                    for (l, v) in [("Quantity", count), ("Total angle", angle)] {
                        row_label(ui, l);
                        enter |= field(ui, v);
                        ui.end_row();
                    }
                }
                Kind::Loft { operation } => {
                    row_label(ui, "Operation");
                    combo(ui, "lf_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Kind::Sweep { operation } => {
                    row_label(ui, "Operation");
                    combo(ui, "sw_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                k @ Kind::Move { .. } => enter |= crate::dialogs_move::rows(ui, k),
                Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } => {
                    row_label(ui, "Extents");
                    let mut ext = usize::from(opts.all);
                    combo(ui, "hole_ext", &["Distance", "All"], &mut ext);
                    opts.all = ext == 1;
                    ui.end_row();
                    row_label(ui, "Hole Type");
                    combo(ui, "hole_kind", &HOLE_LABELS, kind);
                    ui.end_row();
                    row_label(ui, "Hole Tap Type");
                    let mut tap = usize::from(opts.tapped);
                    combo(ui, "hole_tap", &["Simple", "Tapped"], &mut tap);
                    opts.tapped = tap == 1;
                    ui.end_row();
                    if opts.tapped {
                        row_label(ui, "Size");
                        let r = ui.add(egui::TextEdit::singleline(&mut opts.thread).hint_text("M5"));
                        enter |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.end_row();
                    }
                    if *kind == 0 {
                        row_label(ui, "Drill Point");
                        let mut tip = usize::from(opts.angled);
                        combo(ui, "hole_tip", &["Flat", "Angle"], &mut tip);
                        opts.angled = tip == 1;
                        ui.end_row();
                    }
                    if !opts.all {
                        row_label(ui, "Depth");
                        enter |= field(ui, depth);
                        ui.end_row();
                    }
                    if *kind == 0 && opts.angled {
                        row_label(ui, "Tip Angle");
                        enter |= field(ui, &mut opts.tip_angle);
                        ui.end_row();
                    }
                    row_label(ui, "Diameter");
                    enter |= field(ui, diameter);
                    ui.end_row();
                    if *kind == 1 {
                        row_label(ui, "Counterbore Ø");
                        enter |= field(ui, cb_diameter);
                        ui.end_row();
                        row_label(ui, "Counterbore depth");
                        enter |= field(ui, cb_depth);
                        ui.end_row();
                    }
                    if *kind == 2 {
                        row_label(ui, "Countersink Ø");
                        enter |= field(ui, cs_diameter);
                        ui.end_row();
                        row_label(ui, "Countersink angle");
                        enter |= field(ui, cs_angle);
                        ui.end_row();
                    }
                }
                Kind::Primitive { fields, operation, .. } => {
                    for (k, v) in fields.iter_mut() {
                        row_label(ui, k);
                        enter |= field(ui, v);
                        ui.end_row();
                    }
                    row_label(ui, "Operation");
                    combo(ui, "pr_op", &OP_LABELS, operation);
                    ui.end_row();
                }
                Kind::Combine { operation, keep_tools } => {
                    row_label(ui, "Operation");
                    let mut op = operation.saturating_sub(1);
                    combo(ui, "cb_op", &OP_LABELS[1..], &mut op);
                    *operation = op + 1;
                    ui.end_row();
                    row_label(ui, "Keep Tools");
                    ui.checkbox(keep_tools, "");
                    ui.end_row();
                }
                Kind::Params { new_name, new_expr } => params_table(app, ui, new_name, new_expr),
                Kind::EditParam { name, expr } => {
                    ui.label(name.as_str());
                    let r = ui.text_edit_singleline(expr);
                    crate::params_dialog::complete(ui, &r, expr);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    r.request_focus();
                    ui.end_row();
                }
                Kind::Rename { name, .. } => {
                    row_label(ui, "Name");
                    let r = ui.text_edit_singleline(name);
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                    r.request_focus();
                    ui.end_row();
                }
                Kind::Assembly(k) => enter |= crate::dialogs_assembly::rows(app, ui, k, &mut d.inputs, &mut d.active),
                Kind::Sheet(k) => enter |= crate::dialogs_sheet::rows(app, ui, k, &mut d.inputs),
                Kind::Plastic(k) => enter |= crate::dialogs_plastic::rows(app, ui, k, &d.inputs),
                Kind::Appearance(k) => enter |= crate::dialogs_appearance::rows(app, ui, k, &mut d.inputs),
                Kind::Motion(k) => enter |= crate::dialogs_motion::rows(app, ui, k, &mut d.inputs),
                Kind::Part(k) => enter |= crate::dialogs_parts::rows(app, ui, k, &d.inputs),
                Kind::ConfirmDelete { with, fail, .. } => {
                    if !with.is_empty() {
                        row_label(ui, "Also deletes");
                        ui.label(with.join(", "));
                        ui.end_row();
                    }
                    if !fail.is_empty() {
                        row_label(ui, "Will fail");
                        ui.label(RichText::new(fail.join(", ")).color(t.warning));
                        ui.end_row();
                    }
                }
            }
            if let Some(e) = d.error.as_ref().or(app.preview.error.as_ref()) {
                row_label(ui, "");
                ui.add(egui::Label::new(RichText::new(e.as_str()).color(t.error)).wrap());
                ui.end_row();
            }
        });
        ui.add_space(6.0);
        // Footer: an info glyph at the left, OK and Cancel at the right.
        ui.horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
            ui.painter().circle_stroke(r.center(), 7.0, Stroke::new(1.2, t.text_dim));
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "i", egui::FontId::proportional(11.0), t.text_dim);
            resp.on_hover_text(heading.as_str());
            // The spinner's place is always taken: a widget that comes and goes would shift
            // the automatic ids of OK and Cancel, and a click pressed in one frame and
            // released in the next would be lost (an animating preview toggles it often).
            let (sr, sresp) = ui.allocate_exact_size(vec2(14.0, 14.0), egui::Sense::hover());
            if app.preview.busy {
                egui::Spinner::new().size(14.0).paint_at(ui, sr);
                sresp.on_hover_text("Updating the preview");
            }
            let buttons = if has_ok { 64.0 * 2.0 + ui.spacing().item_spacing.x } else { 64.0 };
            crate::frame::right_aligned(ui, total, buttons, |ui| {
                if has_ok {
                    let label = if matches!(d.kind, Kind::ConfirmDelete { .. }) { "Delete" } else { "OK" };
                    let b = egui::Button::new(RichText::new(label).color(if valid { Color32::WHITE } else { t.text_dim }))
                        .fill(if valid { t.accent } else { Color32::TRANSPARENT })
                        .min_size(vec2(64.0, 24.0));
                    if ui.add_enabled(valid, b).clicked() {
                        ok = true;
                    }
                }
                let close = if matches!(d.kind, Kind::Params { .. } | Kind::Measure { .. } | Kind::Preferences) { "Close" } else { "Cancel" };
                if ui.add(egui::Button::new(close).min_size(vec2(64.0, 24.0))).clicked() {
                    cancel = true;
                }
            });
        });
        crate::frame::remember_width(ui, wkey);
    });
    // Keyboard: Enter applies (from a value field, or with nothing focused), Esc cancels.
    let (enter_free, esc) = ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
    let nothing_focused = ctx.memory(|m| m.focused().is_none());
    let canvas_enter = enter_free && ctx.memory(|m| m.had_focus_last_frame(egui::Id::new("sc_canvas_value")));
    // Tables (configurations, a study's keys) take Enter for their cells.
    let enter_applies = !matches!(d.kind, Kind::Sketch | Kind::Params { .. } | Kind::Measure { .. } | Kind::ConfirmDelete { .. } | Kind::Preferences)
        && !matches!(&d.kind, Kind::Motion(k) if k.wide());
    if enter_applies && (enter || canvas_enter || (enter_free && nothing_focused)) {
        ok = true;
    }
    if esc && !app.esc_handled {
        app.esc_handled = true;
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
            Ok(()) => {
                cancel = true;
                applied = true;
            }
            Err(e) => d.error = Some(e),
        }
    }
    if !(keep && !cancel) {
        crate::dialogs_motion::closed(app, &mut d, applied);
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
    if let Some(mut d) = app.dialog.take() {
        crate::dialogs_motion::closed(app, &mut d, false);
        app.dialog = Some(d);
    }
    if let Some(d) = app.dialog.take()
        && let Some((_, marker)) = d.editing
    {
        let _ = app.run("timeline.rollTo", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
    }
}

const CHAMFER_TYPES: [&str; 3] = ["Equal Distance", "Two Distances", "Distance and Angle"];

/// A new Fillet or Chamfer dialog's values.
fn fillet_kind(size: &str, chamfer: bool) -> Kind {
    Kind::Fillet { radius: size.into(), chamfer, chain: true, ctype: 0, distance2: size.into(), angle: "45 deg".into(), flip: false }
}

fn hole_defaults() -> Kind {
    Kind::Hole {
        diameter: "5 mm".into(),
        depth: "10 mm".into(),
        kind: 0,
        cb_diameter: "9 mm".into(),
        cb_depth: "3 mm".into(),
        cs_diameter: "10 mm".into(),
        cs_angle: "90 deg".into(),
        opts: HoleOpts::default(),
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
        // On a face: look at the picked point, not the plane's origin (a corner of the face),
        // which left the face filling the view off-centre like a big square.
        if let Sel::Face { point, .. } = sel
            && let Some(mut to) = app.cam_anim.as_ref().map(|a| a.to)
        {
            to.target = *point;
            app.animate_to(to);
        }
    }
}

/// Animate the camera to look straight at the active sketch plane.
pub fn look_at_sketch(app: &mut SolveApp) {
    let st = app.session.world_state();
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

/// The commands the live preview runs: OK's, with a joint's motion while it animates.
pub fn preview_commands(app: &SolveApp, d: &Dialog) -> Result<Vec<(String, Value)>, String> {
    let mut cmds = apply_commands(app, d)?;
    if let Kind::Assembly(k) = &d.kind {
        crate::dialogs_assembly::animate(app, k, &mut cmds);
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
        Kind::Extrude { distance, direction, operation, distance2, taper, start, all, .. } => {
            need(0, "profiles or a planar face")?;
            let mut common = json!({"distance": distance, "direction": DIRS.get(*direction).copied().unwrap_or("positive"), "operation": OPS.get(*operation).copied().unwrap_or("new")});
            let tapered = s.doc.eval(taper, ValueKind::Angle).map(|v| v.abs() > 1e-12).unwrap_or(!taper.trim().is_empty());
            if *direction == TWO_SIDES {
                common["distance2"] = json!(distance2);
            } else if *direction < 2 && tapered {
                common["taper"] = json!(taper);
            }
            if !start.trim().is_empty() {
                common["start_offset"] = json!(start);
            }
            if *all {
                common["through_all"] = json!(true);
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
        Kind::Fillet { radius, chamfer, ctype, distance2, angle, flip, .. } => {
            need(0, "edges")?;
            // Faces stand for all their edges.
            let st = s.world_state();
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
                let mut p = json!({"edges": edges, "distance": radius});
                match *ctype {
                    1 => p["distance2"] = json!(distance2),
                    2 => p["angle"] = json!(angle),
                    _ => {}
                }
                if *ctype > 0 && *flip {
                    p["flip"] = json!(true);
                }
                ("FusionChamferCommand", p)
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
            need(0, "objects")?;
            need(1, "the mirror plane")?;
            let features = pattern_features(s, sels(d, 0));
            let plane = plane_value(s, sels(d, 1).first()).ok_or("the mirror plane must be a plane or a planar face")?;
            ("MirrorCommand", json!({"features": features, "plane": plane}))
        }
        Kind::PatternRect { count, spacing, count2, spacing2 } => {
            need(0, "objects")?;
            let kept = d.extra.get("dir1").and_then(|v| serde_json::from_value::<[f64; 3]>(v.clone()).ok()).map(|a| Vec3::new(a[0], a[1], a[2]));
            let d1 = match (sels(d, 1).first(), kept) {
                (Some(x), _) => axis_of(app, x).ok_or("the direction must be an axis or a sketch line")?.1,
                (None, Some(k)) => k,
                (None, None) => return Err("select a direction (an axis or a sketch line) first".into()),
            };
            let mut p = json!({"features": pattern_features(s, sels(d, 0)), "dir1": pt(d1), "count1": count, "spacing1": spacing});
            if let Some((_, d2)) = sels(d, 2).first().and_then(|x| axis_of(app, x)) {
                p["dir2"] = pt(d2);
                p["count2"] = json!(count2);
                p["spacing2"] = json!(spacing2);
            }
            ("PatternRectangular", p)
        }
        Kind::PatternCirc { count, angle } => {
            need(0, "objects")?;
            let axis = match sels(d, 1).first() {
                None if d.extra.contains_key("axis") => d.extra.get("axis").cloned().unwrap_or(Value::Null),
                Some(Sel::Axis { name }) => json!(name),
                Some(x) => {
                    let (o, dir) = axis_of(app, x).ok_or("the axis must be an origin axis or a sketch line")?;
                    json!({"origin": pt(o), "dir": pt(dir)})
                }
                None => return Err("select an axis".into()),
            };
            ("PatternCircular", json!({"features": pattern_features(s, sels(d, 0)), "axis": axis, "count": count, "angle": angle}))
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
        Kind::OffsetPlane { offset } => {
            need(0, "a plane")?;
            let base = match sels(d, 0).first() {
                Some(Sel::Plane { name }) => name.clone(),
                _ => return Err("pick an origin or construction plane".into()),
            };
            ("ConstructionPlaneOffsetFromPlaneCommand", json!({"base": base, "offset": offset}))
        }
        Kind::AnglePlane { angle } => {
            need(0, "a plane")?;
            let base = match sels(d, 0).first() {
                Some(Sel::Plane { name }) => name.clone(),
                _ => return Err("pick an origin or construction plane".into()),
            };
            let axis = match sels(d, 1).first() {
                Some(Sel::Axis { name }) => json!(name),
                Some(x) => {
                    let (o, dir) = axis_of(app, x).ok_or("the axis must be an origin axis or a sketch line")?;
                    json!({"origin": pt(o), "dir": pt(dir)})
                }
                None => d.extra.get("axis").cloned().ok_or("pick an axis")?,
            };
            ("ConstructionPlaneAtAngleCommand", json!({"base": base, "axis": axis, "angle": angle}))
        }
        Kind::Rib { web, thickness, depth, flip } => {
            need(0, "open sketch curves")?;
            let curves = curve_ids(d, 0);
            let sketch = curves_sketch(app, &curves, None).ok_or("the curves must be in one sketch")?;
            let mut p = json!({"sketch": sketch, "curves": curves, "thickness": thickness, "flip": flip});
            if !depth.trim().is_empty() {
                p["depth"] = json!(depth);
            }
            (if *web { "FusionWebCommand" } else { "FusionRibCommand" }, p)
        }
        Kind::Emboss { depth, deboss } => {
            need(0, "profiles")?;
            let (sketch, idx) = profiles();
            ("EmbossCmd", json!({"sketch": sketch, "profiles": idx, "depth": depth, "mode": if *deboss { "deboss" } else { "emboss" }}))
        }
        Kind::ReplaceFace => {
            need(0, "faces to move")?;
            need(1, "the target")?;
            let mut p = json!({"faces": face_points(0)});
            match sels(d, 1).first() {
                Some(Sel::Plane { name }) => p["target"] = json!(name),
                Some(Sel::Face { point, .. }) => p["target_face"] = pt(*point),
                _ => return Err("pick a plane or planar face".into()),
            }
            ("FusionReplaceFaceCommand", p)
        }
        Kind::Align { flip } => {
            need(0, "bodies")?;
            need(1, "what to move from")?;
            need(2, "where to move it to")?;
            let mut p = json!({"bodies": body_names(0), "flip": flip});
            for (i, key) in [(1, "from"), (2, "to")] {
                match sels(d, i).first() {
                    Some(Sel::Face { point, .. }) => p[format!("{key}_face")] = pt(*point),
                    Some(Sel::Vertex { point, .. }) => p[key] = pt(*point),
                    _ => return Err("pick faces or vertices".into()),
                }
            }
            ("AlignCmd", p)
        }
        Kind::Remove => {
            need(0, "bodies")?;
            ("SoftDeleteCommand", json!({"bodies": body_names(0)}))
        }
        Kind::PathPattern { count, spacing } => {
            need(0, "objects")?;
            need(1, "the path")?;
            let path = curve_ids(d, 1);
            let sketch = curves_sketch(app, &path, None).ok_or("the path curves must be in one sketch")?;
            let mut p = json!({"path_sketch": sketch, "path": path, "count": count, "spacing": spacing});
            if sels(d, 0).iter().any(|x| matches!(x, Sel::Feature { .. })) {
                p["features"] = json!(pattern_features(s, sels(d, 0)));
            } else {
                p["bodies"] = json!(body_names(0));
            }
            ("PatternOnPath", p)
        }
        Kind::Pipe { diameter, wall } => {
            need(0, "the path")?;
            let path = curve_ids(d, 0);
            let sketch = curves_sketch(app, &path, None).ok_or("the path curves must be in one sketch")?;
            let mut p = json!({"path_sketch": sketch, "path": path, "diameter": diameter});
            if !wall.trim().is_empty() {
                p["wall"] = json!(wall);
            }
            ("PrimitivePipe", p)
        }
        Kind::Stock { margin } => {
            let mut p = json!({"margin": margin});
            if !sels(d, 0).is_empty() {
                p["bodies"] = json!(body_names(0));
            }
            ("StockModelCommand", p)
        }
        Kind::Split => {
            need(0, "the body to split")?;
            need(1, "the splitting plane")?;
            let body = body_names(0).into_iter().next().unwrap_or_default();
            let plane = match sels(d, 1).first() {
                Some(Sel::Plane { name }) => json!(name),
                Some(Sel::Face { body, index, point }) => {
                    let (_, n) = planar_face(s, body, *index).ok_or("the face must be planar")?;
                    json!({"origin": pt(*point), "normal": pt(n)})
                }
                _ => return Err("pick a plane or a planar face".into()),
            };
            ("FusionSplitBodyCommand", json!({"body": body, "plane": plane}))
        }
        Kind::Scale { factor } => {
            need(0, "bodies")?;
            ("ModifyScale", json!({"bodies": body_names(0), "factor": factor}))
        }
        Kind::OffsetFaces { distance } => {
            need(0, "planar faces")?;
            ("FusionOffsetFacesCommand", json!({"faces": face_points(0), "distance": distance}))
        }
        Kind::Thread { designation, length } => {
            need(0, "a cylindrical face")?;
            let mut p = json!({"face": face_points(0).into_iter().next().unwrap_or(Value::Null)});
            if !designation.trim().is_empty() {
                p["designation"] = json!(designation.trim());
            }
            if !length.trim().is_empty() {
                p["length"] = json!(length);
            }
            ("FusionThreadCommand", p)
        }
        Kind::Material { index } => {
            need(0, "bodies")?;
            let name = solvecraft_engine::doc::MATERIALS.get(*index).map(|(n, _)| *n).unwrap_or("Default");
            ("PhysicalMaterialCommand", json!({"bodies": body_names(0), "material": name}))
        }
        Kind::Section { offset, flip } => {
            need(0, "a plane or planar face")?;
            let plane = match sels(d, 0).first() {
                Some(Sel::Plane { name }) if matches!(name.as_str(), "XY" | "XZ" | "YZ") => json!(name),
                Some(Sel::Face { body, index, point }) => {
                    let (_, n) = planar_face(s, body, *index).ok_or("the face must be planar")?;
                    json!({"origin": pt(*point), "normal": pt(n)})
                }
                Some(Sel::Plane { name }) => {
                    let pl = solvecraft_engine::view::construction_planes(s)
                        .into_iter()
                        .find(|(_, n, _)| n == name)
                        .map(|(_, _, pl)| pl)
                        .ok_or("unknown plane")?;
                    json!({"origin": pt(pl.origin), "normal": pt(pl.normal())})
                }
                _ => return Err("pick a plane or a planar face".into()),
            };
            ("FusionHalfSectionViewCommand", json!({"plane": plane, "offset": offset, "flip": flip}))
        }
        Kind::Move { .. } => return Ok(vec![("FusionMoveCommand".into(), crate::dialogs_move::params(app, d)?)]),
        Kind::Hole { diameter, depth, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } => {
            need(0, if opts.multiple { "sketch points" } else { "a face position" })?;
            let ty = match *kind {
                0 if opts.angled => "drilled",
                k => HOLE_TYPES.get(k).copied().unwrap_or("simple"),
            };
            // One hole per picked face position, or one Hole feature per sketch for sketch points.
            let mut places: Vec<Value> = Vec::new();
            if opts.multiple {
                let mut by_sketch: Vec<(u64, Vec<String>)> = Vec::new();
                for x in sels(d, 0) {
                    if let Sel::SketchPoint { id } = x
                        && let Some((sk, pid)) = id.split_once(':')
                        && let Ok(sk) = sk.parse::<u64>()
                    {
                        match by_sketch.iter_mut().find(|(s, _)| *s == sk) {
                            Some((_, v)) => v.push(pid.to_string()),
                            None => by_sketch.push((sk, vec![pid.to_string()])),
                        }
                    }
                }
                places.extend(by_sketch.into_iter().map(|(sk, ids)| json!({"sketch": sk, "points": ids})));
            } else {
                places.extend(face_points(0).into_iter().map(|p| json!({ "position": p })));
            }
            let mut out = Vec::new();
            for place in places {
                let mut params = place;
                params["diameter"] = json!(diameter);
                params["type"] = json!(ty);
                if !opts.all {
                    params["depth"] = json!(depth);
                }
                if ty == "drilled" {
                    params["tip_angle"] = json!(opts.tip_angle);
                }
                if opts.tapped && !opts.thread.trim().is_empty() {
                    params["thread"] = json!(opts.thread.trim());
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
        Kind::Sketch | Kind::Params { .. } | Kind::Measure { .. } | Kind::Preferences => return Ok(Vec::new()),
        Kind::Assembly(k) => return crate::dialogs_assembly::commands(app, k, &d.inputs),
        Kind::Sheet(k) => return crate::dialogs_sheet::commands(app, k, &d.inputs, &d.extra),
        Kind::Plastic(k) => return crate::dialogs_plastic::commands(app, k, &d.inputs, &d.extra),
        Kind::Appearance(k) => return Ok(crate::dialogs_appearance::commands(k, &d.inputs)),
        Kind::Motion(k) => return crate::dialogs_motion::commands(app, k, &d.inputs),
        Kind::Part(k) => return crate::dialogs_parts::commands(app, k, &d.inputs),
    };
    let mut params = params;
    if let Value::Object(m) = &mut params {
        for (k, v) in &d.extra {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Ok(vec![(cmd.to_string(), params)])
}

/// Sketch curve ids picked in an input.
fn curve_ids(d: &Dialog, i: usize) -> Vec<String> {
    sels(d, i).iter().filter_map(|x| if let Sel::SketchCurve { id } = x { Some(id.clone()) } else { None }).collect()
}

/// The features that made these bodies (what patterns and mirrors copy).
/// The features a pattern repeats: picked features as they are, picked bodies by the features
/// that made them.
fn pattern_features(s: &Session, objects: &[Sel]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bodies: Vec<String> = objects.iter().filter_map(|x| if let Sel::Body { name } = x { Some(name.clone()) } else { None }).collect();
    for x in objects {
        if let Sel::Feature { id } = x
            && let Some(f) = s.doc.feature(*id)
            && !out.contains(&f.name)
        {
            out.push(f.name.clone());
        }
    }
    for f in source_features(s, &bodies) {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

fn source_features(s: &Session, bodies: &[String]) -> Vec<String> {
    let st = s.world_state();
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
    let st = app.session.world_state();
    let has =
        |sid: u64| st.sketch(sid).is_some_and(|ss| ids.iter().all(|id| ss.sketch.curve_index(id).is_some() || ss.sketch.wire_index(id).is_some()));
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
            let st = app.session.world_state();
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
pub(crate) fn profile_indices(ss: &solvecraft_engine::doc::SolvedSketch, sel: &ProfileSel) -> Vec<usize> {
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
pub(crate) fn edge_sel(s: &Session, p: Vec3) -> Option<Sel> {
    let st = s.world_state();
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
pub(crate) fn face_sel(s: &Session, p: Vec3) -> Option<Sel> {
    let st = s.world_state();
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
        PlaneRef::Custom { plane } | PlaneRef::Face { plane, .. } => face_sel(s, plane.origin),
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

/// The origin axis a unit direction runs along (either way), if any.
fn world_axis(d: Vec3) -> Option<String> {
    [("X", Vec3::X), ("Y", Vec3::Y), ("Z", Vec3::Z)]
        .into_iter()
        .find(|(_, a)| d.cross(*a).len() < 1e-9 && d.dot(*a) > 0.0)
        .map(|(n, _)| n.to_string())
}

fn pt3(v: Vec3) -> Value {
    json!([v.x, v.y, v.z])
}

/// A dialog that edits an existing feature, filled from it. The timeline is rolled back to just
/// before the feature, so its references show on the geometry they refer to.
pub fn for_feature(app: &SolveApp, id: u64, marker: Option<usize>) -> Option<Dialog> {
    let s = &app.session;
    let f = s.doc.feature(id)?.clone();
    let st = s.world_state();
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
                taper: extent.taper.clone().unwrap_or_else(|| "0 deg".into()),
                start: extent.start_offset.clone().unwrap_or_default(),
                all: extent.through_all,
            };
            // The dialog has no "flip": a flipped extrude is a negative distance.
            if let Kind::Extrude { direction, distance, .. } = &mut d.kind
                && *direction == 1
            {
                *direction = 0;
                *distance = format!("-({distance})");
            }
            let items = st.sketch(*sketch).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
            }
            for (k, v) in [("targets", (!targets.is_empty()).then(|| json!(targets)))] {
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
            d.kind = fillet_kind(radius, chamfer);
            if let Kind::Fillet { chain, ctype, distance2, angle, flip, .. } = &mut d.kind {
                *chain = false;
                // An unequal chamfer: two distances, or a distance and an angle.
                if let FeatureKind::Chamfer { distance2: d2, angle: a, flip: f, .. } = &f.kind {
                    if let Some(v) = d2 {
                        *ctype = 1;
                        *distance2 = v.clone();
                    } else if let Some(v) = a {
                        *ctype = 2;
                        *angle = v.clone();
                    }
                    *flip = *f;
                }
            }
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
        FeatureKind::Hole { position, direction, diameter, depth, hole, points, thread } => {
            let mut d = start("FusionHoleCommand")?;
            let mut k = hole_defaults();
            if let Kind::Hole { diameter: dia, depth: dep, kind, cb_diameter, cb_depth, cs_diameter, cs_angle, opts } = &mut k {
                *dia = diameter.clone();
                opts.all = depth.is_none();
                opts.angled = matches!(hole, HoleKind::Drilled { .. });
                opts.multiple = points.is_some();
                if let Some(th) = thread {
                    opts.tapped = true;
                    opts.thread = th.clone();
                }
                if let Some(dd) = depth {
                    *dep = dd.clone();
                }
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
                        opts.tip_angle = tip_angle.clone();
                    }
                    HoleKind::Simple => {}
                }
            }
            d.kind = k;
            match points {
                Some(sp) => {
                    d.inputs = vec![SelInput::new("Points", selection::POINTS, true)];
                    if let Some(inp) = d.inputs.first_mut() {
                        inp.items = sp.ids.iter().map(|id| Sel::SketchPoint { id: format!("{}:{id}", sp.sketch) }).collect();
                    }
                }
                None => {
                    if let Some(inp) = d.inputs.first_mut() {
                        inp.items = vec![face_sel(s, *position).unwrap_or(Sel::Face { body: String::new(), index: 0, point: *position })];
                    }
                    d.extra.insert("direction".into(), pt3(*direction));
                }
            }
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
        FeatureKind::Mirror { features, plane, .. } => {
            let mut d = start("MirrorCommand")?;
            let ids: Vec<u64> = features.iter().filter_map(|n| s.doc.find_feature(n).map(|f| f.id)).collect();
            // Features that made no body of their own (a hole) were picked as features.
            let as_features = ids.iter().any(|id| !st.bodies.iter().any(|b| b.feature == *id));
            if let Some(inp) = d.inputs.get_mut(0) {
                if as_features {
                    inp.accept = selection::FEATURES;
                    inp.label = objects_label(inp.accept);
                    inp.items = ids.iter().map(|id| Sel::Feature { id: *id }).collect();
                } else {
                    inp.items = st.bodies.iter().filter(|b| ids.contains(&b.feature)).map(|b| Sel::Body { name: b.name.clone() }).collect();
                }
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, plane).into_iter().collect();
            }
            d
        }
        FeatureKind::Pattern { features, pattern, .. } => {
            let ids: Vec<u64> = features.iter().filter_map(|n| s.doc.find_feature(n).map(|f| f.id)).collect();
            let bodies: Vec<Sel> = st.bodies.iter().filter(|b| ids.contains(&b.feature)).map(|b| Sel::Body { name: b.name.clone() }).collect();
            let mut d = match pattern {
                PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2 } => {
                    let mut d = start("PatternRectangular")?;
                    d.kind = Kind::PatternRect {
                        count: count1.clone(),
                        spacing: spacing1.clone(),
                        count2: count2.clone().unwrap_or_else(|| "1".into()),
                        spacing2: spacing2.clone().unwrap_or_else(|| "20 mm".into()),
                    };
                    // Directions along the origin axes show as those axes; others are kept as is.
                    for (i, (key, dir)) in [("dir1", Some(*dir1)), ("dir2", *dir2)].into_iter().enumerate() {
                        let Some(dir) = dir else { continue };
                        match world_axis(dir) {
                            Some(name) => {
                                if let Some(inp) = d.inputs.get_mut(i + 1) {
                                    inp.items = vec![Sel::Axis { name }];
                                }
                            }
                            None => {
                                d.extra.insert(key.into(), pt3(dir));
                            }
                        }
                    }
                    d
                }
                PatternKind::Circular { origin, axis, count, angle } => {
                    let mut d = start("PatternCircular")?;
                    d.kind = Kind::PatternCirc { count: count.clone(), angle: angle.clone() };
                    match world_axis(*axis).filter(|_| origin.len() < 1e-9) {
                        Some(name) => {
                            if let Some(inp) = d.inputs.get_mut(1) {
                                inp.items = vec![Sel::Axis { name }];
                            }
                        }
                        None => {
                            d.extra.insert("axis".into(), json!({"origin": pt3(*origin), "dir": pt3(*axis)}));
                        }
                    }
                    d
                }
                // No dialog for a path pattern yet: edit it with timeline.redefine.
                PatternKind::Path { .. } => return None,
            };
            // Features that made no body of their own (a hole, a fillet) were picked as features.
            let as_features = ids.iter().any(|id| !st.bodies.iter().any(|b| b.feature == *id));
            if let Some(inp) = d.inputs.first_mut() {
                if as_features {
                    inp.accept = selection::FEATURES;
                    inp.label = objects_label(inp.accept);
                    inp.items = ids.iter().map(|id| Sel::Feature { id: *id }).collect();
                } else {
                    inp.items = bodies;
                }
            }
            d
        }
        FeatureKind::Loft { sections, operation, targets } => {
            let mut d = start("SolidLoft")?;
            d.kind = Kind::Loft { operation: op_index(operation) };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = sections
                    .iter()
                    .flat_map(|sec| {
                        let idx = st.sketch(sec.sketch).map(|ss| profile_indices(ss, &sec.profiles)).unwrap_or_default();
                        idx.into_iter().map(move |index| Sel::Profile { sketch: sec.sketch, index })
                    })
                    .collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Sweep { sketch, profiles, path, operation, targets, .. } => {
            let mut d = start("Sweep")?;
            d.kind = Kind::Sweep { operation: op_index(operation) };
            let items = st.sketch(*sketch).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.get_mut(0) {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = path.iter().map(|id| Sel::SketchCurve { id: id.clone() }).collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::Offset { base, distance } } => {
            let mut d = start("ConstructionPlaneOffsetFromPlaneCommand")?;
            d.kind = Kind::OffsetPlane { offset: distance.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = plane_sel(s, base).into_iter().collect();
            }
            d
        }
        FeatureKind::ConstructionPlane { plane: PlaneRef::AtAngle { base, axis_origin, axis_dir, angle } } => {
            let mut d = start("ConstructionPlaneAtAngleCommand")?;
            d.kind = Kind::AnglePlane { angle: angle.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = plane_sel(s, base).into_iter().collect();
            }
            match world_axis(*axis_dir).filter(|_| axis_origin.len() < 1e-9) {
                Some(name) => {
                    if let Some(inp) = d.inputs.get_mut(1) {
                        inp.items = vec![Sel::Axis { name }];
                    }
                }
                None => {
                    d.extra.insert("axis".into(), json!({"origin": pt3(*axis_origin), "dir": pt3(*axis_dir)}));
                }
            }
            d
        }
        FeatureKind::Split { body, plane, .. } => {
            let mut d = start("FusionSplitBodyCommand")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = vec![Sel::Body { name: body.clone() }];
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, plane).into_iter().collect();
            }
            d
        }
        FeatureKind::Scale { bodies, origin, factor, factors } => {
            let mut d = start("ModifyScale")?;
            d.kind = Kind::Scale { factor: factor.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect();
            }
            d.extra.insert("origin".into(), pt3(*origin));
            if let Some(f) = factors {
                d.extra.insert("factors".into(), json!(f));
            }
            d
        }
        FeatureKind::OffsetFace { faces, distance, body } => {
            let mut d = start("FusionOffsetFacesCommand")?;
            d.kind = Kind::OffsetFaces { distance: distance.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, *p)).collect();
            }
            if let Some(b) = body {
                d.extra.insert("body".into(), json!(b));
            }
            d
        }
        FeatureKind::Thread { face, designation, length } => {
            let mut d = start("FusionThreadCommand")?;
            d.kind = Kind::Thread { designation: designation.clone().unwrap_or_default(), length: length.clone().unwrap_or_default() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = face_sel(s, *face).into_iter().collect();
            }
            d
        }
        FeatureKind::BoundingSolid { bodies, margin } => {
            let mut d = start("StockModelCommand")?;
            d.kind = Kind::Stock { margin: margin.clone() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect();
            }
            d
        }
        FeatureKind::Pipe { path, diameter, wall, operation, targets, .. } => {
            let mut d = start("PrimitivePipe")?;
            d.kind = Kind::Pipe { diameter: diameter.clone(), wall: wall.clone().unwrap_or_default() };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = path.iter().map(|id| Sel::SketchCurve { id: id.clone() }).collect();
            }
            d.extra.insert("operation".into(), json!(OPS.get(op_index(operation)).copied().unwrap_or("new")));
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Emboss { sketch, profiles, depth, deboss, targets } => {
            let mut d = start("EmbossCmd")?;
            d.kind = Kind::Emboss { depth: depth.clone(), deboss: *deboss };
            let items = st.sketch(*sketch).map(|ss| profile_indices(ss, profiles)).unwrap_or_default();
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = items.into_iter().map(|index| Sel::Profile { sketch: *sketch, index }).collect();
            }
            if !targets.is_empty() {
                d.extra.insert("targets".into(), json!(targets));
            }
            d
        }
        FeatureKind::Rib { curves, thickness, depth, flip, web, .. } => {
            let mut d = start(if *web { "FusionWebCommand" } else { "FusionRibCommand" })?;
            d.kind = Kind::Rib { web: *web, thickness: thickness.clone(), depth: depth.clone().unwrap_or_default(), flip: *flip };
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = curves.iter().map(|id| Sel::SketchCurve { id: id.clone() }).collect();
            }
            d
        }
        FeatureKind::ReplaceFace { faces, target, body } => {
            let mut d = start("FusionReplaceFaceCommand")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = faces.iter().filter_map(|p| face_sel(s, *p)).collect();
            }
            if let Some(inp) = d.inputs.get_mut(1) {
                inp.items = plane_sel(s, target).into_iter().collect();
            }
            if let Some(b) = body {
                d.extra.insert("body".into(), json!(b));
            }
            d
        }
        FeatureKind::Remove { bodies } => {
            let mut d = start("SoftDeleteCommand")?;
            if let Some(inp) = d.inputs.first_mut() {
                inp.items = bodies.iter().map(|n| Sel::Body { name: n.clone() }).collect();
            }
            d
        }
        FeatureKind::Move { bodies, translate, rotate_axis, angle } => {
            let mut d = start("FusionMoveCommand")?;
            crate::dialogs_move::for_feature(&mut d, bodies, translate, *rotate_axis, angle.as_ref());
            d
        }
        other => {
            let (kind, inputs, extra) = crate::dialogs_sheet::for_feature(app, other).or_else(|| crate::dialogs_plastic::for_feature(app, other))?;
            let mut d = Dialog::new(kind, inputs);
            d.extra = extra;
            d
        }
    };
    d.editing = Some((id, marker));
    d.active = 0;
    d.error = None;
    Some(d)
}
