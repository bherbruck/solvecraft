//! Context menus, one look everywhere.
//!
//! The viewport's right-click opens a radial marking menu (eight commands around the cursor,
//! Repeat at the top) with a list underneath of what fits the selection or the thing under the
//! cursor: faces, edges, bodies, sketch entities or empty space. Holding the right button opens
//! it too; releasing over a direction picks that command (a flick). The browser's right-click
//! opens the same list styling for a body, sketch or component.
//!
//! Items are commands (by id, with parameters filled from the target) or `ui.*` actions on view
//! state; [`items`] lists them and [`run_item`] runs one, so the control channel (`ui.menu`,
//! `ui.menuPick`) and tests reach everything a click does.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde::Serialize;
use serde_json::{Value, json};
use solvecraft_engine::Sel;

use crate::theme::Tokens;
use crate::{SolveApp, icons};

/// What a menu is about.
#[derive(Clone, Debug, Default, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Target {
    /// The viewport: the selection, or empty space.
    #[default]
    Viewport,
    Body {
        name: String,
    },
    Sketch {
        id: u64,
    },
    Component {
        id: u64,
    },
    /// A browser group of items.
    Group {
        id: u64,
    },
    /// The Origin folder.
    Origin,
    /// Document Settings › Units.
    Units,
    /// An assembly joint.
    Joint {
        id: u64,
    },
    /// A command (toolbar button, panel drop-down row, S box row).
    ToolbarCommand {
        id: String,
    },
    /// The workspace switcher.
    Workspace,
    /// The Named Views folder, and one view (saved or standard) in it.
    NamedViews,
    NamedView {
        name: String,
    },
    /// A canvas (reference image).
    Canvas {
        id: u64,
    },
    /// A component's Bodies, Sketches or Construction folder.
    Folder {
        component: u64,
        folder: String,
    },
}

/// One menu entry: a command id (run with `params` when given, else started like a toolbar
/// click) or a `ui.*` action. `-` is a separator, `#…` a section heading.
#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Item {
    /// A view action (`ui.*`).
    pub fn action(id: &str, label: &str, icon: &str) -> Item {
        act(id, label, icon)
    }
    pub fn sep() -> Item {
        Item { id: "-".into(), label: String::new(), shortcut: String::new(), enabled: false, icon: String::new(), params: None }
    }
    fn heading(label: &str) -> Item {
        Item { id: format!("#{label}"), label: label.into(), shortcut: String::new(), enabled: false, icon: String::new(), params: None }
    }
    fn is_sep(&self) -> bool {
        self.id == "-"
    }
    fn is_heading(&self) -> bool {
        self.id.starts_with('#')
    }
    pub fn with(mut self, params: Value) -> Item {
        self.params = Some(params);
        self
    }
    pub fn on_if(self, enabled: bool) -> Item {
        self.on(enabled)
    }
    fn on(mut self, enabled: bool) -> Item {
        self.enabled &= enabled;
        self
    }
    pub fn key(mut self, k: &str) -> Item {
        self.shortcut = k.into();
        self
    }
}

/// A command entry: label, icon, shortcut and enablement come from the registry.
fn cmd(app: &SolveApp, id: &str, label: &str) -> Item {
    let info = solvecraft_engine::find_command(id).map(|c| c.info(&app.session));
    let shortcut = match id {
        "edit.undo" => "Ctrl+Z".to_string(),
        "edit.redo" => "Ctrl+Y".to_string(),
        _ => crate::keymap::effective(app, id).unwrap_or_default(),
    };
    Item {
        id: id.into(),
        label: label.into(),
        shortcut,
        enabled: info.as_ref().is_some_and(|i| i.enabled),
        icon: info.map(|i| i.icon.to_string()).unwrap_or_default(),
        params: None,
    }
}

/// A view action (`ui.*`).
fn act(id: &str, label: &str, icon: &str) -> Item {
    Item { id: id.into(), label: label.into(), shortcut: String::new(), enabled: true, icon: icon.into(), params: None }
}

/// Menu state kept between frames. The anchor is `app.viewport.context_menu`.
#[derive(Default)]
pub struct MenuState {
    pub target: Target,
    /// The list row chosen with the arrow keys (Enter runs it).
    pub key_row: Option<usize>,
    /// The radial ring is shown (viewport menus).
    pub radial: bool,
    /// The right button is held through the menu: releasing over a direction picks it.
    pub flick: bool,
    /// A right press in the viewport that may become a press-and-hold: where and when.
    press: Option<(Pos2, f64)>,
    /// `ui.menu` asked to open at a point once the pointer got there.
    pending: Option<(Pos2, Target)>,
    /// An inline rename in progress.
    pub rename: Option<Rename>,
    /// A delete waiting for its confirmation (it takes or breaks other features).
    pub confirm: Option<crate::delete::Pending>,
    /// The Properties window: title and measurements.
    pub props: Option<(String, Value)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenameWhat {
    Body(String),
    Feature(u64),
    Component(u64),
    Group(u64),
    Canvas(u64),
    View(String),
    Joint(u64),
}

#[derive(Clone, Debug)]
pub struct Rename {
    pub what: RenameWhat,
    pub text: String,
    pub at: Pos2,
    focused: bool,
}

/// Radial slots, clockwise from the top.
const DIRS: usize = 8;
/// Half axes of the ring of radial items.
const RX: f32 = 118.0;
const RY: f32 = 84.0;

// ---------------------------------------------------------------------------------------------
// What the menus offer
// ---------------------------------------------------------------------------------------------

fn sel_bodies(app: &SolveApp) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in &app.session.selection {
        if let Sel::Body { name } = s
            && !v.contains(name)
        {
            v.push(name.clone());
        }
    }
    v
}

/// Bodies the selection touches (bodies, and the bodies of selected faces and edges).
fn touched_bodies(app: &SolveApp) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for s in &app.session.selection {
        let b = match s {
            Sel::Body { name } => name,
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => body,
            _ => continue,
        };
        if !v.contains(b) {
            v.push(b.clone());
        }
    }
    v
}

fn body_feature(app: &SolveApp, body: &str) -> Option<u64> {
    app.session.world_state().body(body).map(|b| b.feature)
}

/// The bodies a target stands for.
pub fn target_bodies(app: &SolveApp, target: &Target) -> Vec<String> {
    match target {
        Target::Viewport => touched_bodies(app),
        Target::Body { name } => vec![name.clone()],
        Target::Component { id } => component_bodies(app, *id),
        Target::Group { id } => crate::browser::group_bodies(app, *id),
        Target::Sketch { .. }
        | Target::Folder { .. }
        | Target::Canvas { .. }
        | Target::Origin
        | Target::Units
        | Target::Joint { .. }
        | Target::ToolbarCommand { .. }
        | Target::Workspace
        | Target::NamedViews
        | Target::NamedView { .. } => Vec::new(),
    }
}

/// Bodies in a component and its subcomponents.
pub fn component_bodies(app: &SolveApp, id: u64) -> Vec<String> {
    let doc = &app.session.doc;
    app.session
        .model
        .state()
        .bodies
        .iter()
        .filter(|b| doc.component_within(doc.body_component(&b.name, b.feature), id))
        .map(|b| b.name.clone())
        .collect()
}

/// The eight radial commands, clockwise from the top.
pub fn radial_items(app: &SolveApp) -> Vec<Item> {
    let repeat = match &app.last_command {
        Some((_, label)) => act("ui.repeat", &format!("Repeat {label}"), "redo"),
        None => act("ui.repeat", "Repeat", "redo").on(false),
    };
    let delete = act("ui.delete", "Delete", "delete").key("Del").on(!app.session.selection.is_empty());
    if app.session.active_sketch.is_some() {
        return vec![
            repeat,
            delete,
            cmd(app, "sketch.line", "Line"),
            cmd(app, "sketch.rectangle.two_point", "Rectangle"),
            act("ui.finishSketch", "Finish Sketch", "finish"),
            cmd(app, "sketch.circle.center", "Circle"),
            cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()),
            cmd(app, "sketch.dimension", "Sketch Dimension"),
        ];
    }
    vec![
        repeat,
        delete,
        cmd(app, "solid.press_pull", "Press Pull"),
        cmd(app, "solid.hole", "Hole"),
        cmd(app, "sketch.create", "Create Sketch"),
        cmd(app, "solid.extrude", "Extrude"),
        cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()),
        cmd(app, "solid.move", "Move/Copy"),
    ]
}

/// The list of a menu (under the ring for the viewport).
pub fn items(app: &SolveApp, target: &Target) -> Vec<Item> {
    let mut v = match target {
        Target::Viewport => viewport_items(app),
        Target::Body { name } => body_items(app, std::slice::from_ref(name), true),
        Target::Sketch { id } => sketch_items(app, *id),
        Target::Component { id } => component_items(app, *id),
        Target::Group { id } => crate::browser::group_items(app, *id),
        Target::Folder { component, folder } => crate::browser::folder_items(app, *component, folder),
        Target::Canvas { id } => crate::browser::canvas_items(app, *id),
        Target::Origin => crate::browser::origin_items(app),
        Target::NamedViews => vec![act("ui.newView", "New Named View", "perspective")],
        Target::Joint { id } => joint_items(app, *id),
        Target::ToolbarCommand { id } => crate::toolbar_custom::command_items(app, id),
        Target::Workspace => {
            let mut v = vec![act("ui.workspace", "Design", "box").key("current")];
            for w in ["Generative Design", "Render", "Animation", "Simulation", "Manufacture", "Drawing"] {
                v.push(act("ui.workspace", w, "").key("coming later").on(false));
            }
            v
        }
        Target::Units => crate::prefs::UNITS
            .iter()
            .map(|u| {
                let item = cmd(app, "document.units", u).with(json!({ "units": u }));
                if app.session.doc.units == *u { item.key("current") } else { item }
            })
            .collect(),
        Target::NamedView { name } => crate::browser::view_items(app, name),
    };
    // Several items of the folder selected: they can be grouped.
    if let Some(p) = crate::browser::selection_group(app, target) {
        v.push(Item::sep());
        v.push(act("ui.groupSelected", "Group Selected", "folder").with(p));
    }
    v
}

/// The feature a body was made by, and that feature's sketch if it has one.
fn body_feature_and_sketch(app: &SolveApp, body: &str) -> (Option<u64>, Option<u64>) {
    let f = body_feature(app, body);
    let sketch = f
        .and_then(|f| app.session.doc.feature(f))
        .and_then(|f| serde_json::to_value(&f.kind).ok())
        .and_then(|v| v.get("sketch").and_then(Value::as_u64).or_else(|| v.as_object()?.values().find_map(|x| x.get("sketch")?.as_u64())));
    (f, sketch)
}

/// The canvas menus follow Fusion's (`plan/fusion/ui/NOTES.md`, feature names only): Repeat on
/// top, then what fits the selection, our extras after a separator.
fn viewport_items(app: &SolveApp) -> Vec<Item> {
    let sel = &app.session.selection;
    let has = |f: fn(&Sel) -> bool| sel.iter().any(f);
    let mut v = Vec::new();
    if let Some((_, label)) = &app.last_command {
        v.push(act("ui.repeat", &format!("Repeat {label}"), "redo"));
        v.push(Item::sep());
    }
    if app.session.active_sketch.is_some() {
        v.extend(sketch_entity_items(app));
        v.push(Item::sep());
        v.push(cmd(app, "sketch.trim", "Trim"));
        v.push(cmd(app, "sketch.offset", "Offset"));
        v.push(Item::sep());
        v.push(act("ui.finishSketch", "Finish Sketch", "finish"));
        v.push(Item::sep());
        v.push(cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()));
        v.push(cmd(app, "edit.redo", "Redo").on(!app.session.redo.is_empty()));
        return v;
    }
    if has(|s| matches!(s, Sel::Face { .. })) {
        let face = sel.iter().find_map(|s| if let Sel::Face { body, .. } = s { Some(body.clone()) } else { None }).unwrap_or_default();
        let (feature, sketch) = body_feature_and_sketch(app, &face);
        let hidden = app.ui.hidden_bodies.contains(&face);
        v.push(cmd(app, "sketch.create", "Create Sketch"));
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "construct.plane.offset", "Offset Plane"));
        v.push(cmd(app, "solid.shell", "Shell"));
        v.push(act("ui.editFeature", "Edit Feature", "").with(json!({ "feature": feature })).on(feature.is_some()));
        v.push(act("ui.editSketch", "Edit Profile Sketch", "sketch").with(json!({ "sketch": sketch })).on(sketch.is_some()));
        v.push(Item::sep());
        v.push(act("ui.appearance", "Appearance", "").key("A").on(false));
        v.push(act("ui.properties", "Properties", "measure").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.delete", "Delete", "delete").key("Del"));
        v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": [face] })));
        v.push(Item::sep());
        v.push(act("ui.findBrowser", "Find in Browser", "").with(json!({ "body": face })));
        v.push(act("ui.findTimeline", "Find in Timeline", "").with(json!({ "body": face })));
        v.push(Item::sep());
        v.push(cmd(app, "solid.press_pull", "Press Pull"));
        v.push(cmd(app, "inspect.measure", "Measure"));
    } else if has(|s| matches!(s, Sel::Edge { .. })) {
        let body = sel.iter().find_map(|s| if let Sel::Edge { body, .. } = s { Some(body.clone()) } else { None }).unwrap_or_default();
        let hidden = app.ui.hidden_bodies.contains(&body);
        v.push(cmd(app, "solid.fillet", "Fillet"));
        v.push(cmd(app, "solid.chamfer", "Chamfer"));
        v.push(Item::sep());
        v.push(act("ui.delete", "Delete", "delete").key("Del"));
        v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": [body] })));
        v.push(Item::sep());
        v.push(act("ui.findBrowser", "Find in Browser", "").with(json!({ "body": body })));
        v.push(Item::sep());
        v.push(act("ui.tangentChain", "Select Tangent Chain", ""));
        v.push(cmd(app, "inspect.measure", "Measure"));
    } else if has(|s| matches!(s, Sel::Body { .. })) {
        v.extend(body_items(app, &sel_bodies(app), false));
    } else if has(|s| matches!(s, Sel::Profile { .. })) {
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "solid.revolve", "Revolve"));
    } else {
        // Empty space.
        v.push(act("ui.nav", "Pan", "pan").with(json!({ "mode": "pan" })));
        v.push(act("ui.nav", "Zoom", "zoom").with(json!({ "mode": "zoom" })));
        v.push(act("ui.nav", "Orbit", "orbit").with(json!({ "mode": "orbit" })));
        v.push(act("ui.fit", "Fit", "fit"));
        v.push(act("ui.home", "Home View", "home"));
        v.push(Item::sep());
        let anything_hidden = !app.ui.hidden_bodies.is_empty() || !app.ui.hidden_sketches.is_empty();
        v.push(act("ui.showAll", "Show All", "eye").on(anything_hidden));
        v.push(act("ui.showAll", "Unisolate", "").on(!app.ui.hidden_bodies.is_empty()));
        v.push(Item::sep());
        v.push(cmd(app, "solid.extrude", "Extrude"));
        v.push(cmd(app, "solid.fillet", "Fillet"));
        v.push(Item::sep());
        v.push(cmd(app, "edit.undo", "Undo").on(!app.session.undo.is_empty()));
        v.push(cmd(app, "edit.redo", "Redo").on(!app.session.redo.is_empty()));
        v.push(Item::sep());
        if app.session.section.is_some() {
            v.push(act("ui.unsection", "Remove Section", "section"));
        }
        v.push(act("ui.origin", if app.ui.show_origin { "Hide Origin" } else { "Show Origin" }, "origin"));
        return v;
    }
    if !sel.is_empty() {
        v.push(Item::sep());
        v.push(act("ui.clear", "Clear Selection", "").key("Esc"));
    }
    v
}

fn body_items(app: &SolveApp, bodies: &[String], browser: bool) -> Vec<Item> {
    let names = json!(bodies);
    let all_hidden = bodies.iter().all(|b| app.ui.hidden_bodies.contains(b));
    let one = bodies.len() == 1;
    let locked = bodies.iter().all(|b| app.ui.locked_bodies.contains(b));
    let items = json!(bodies.iter().map(|b| json!({"type": "body", "name": b})).collect::<Vec<_>>());
    let mut v = vec![
        act("ui.moveBodies", "Move/Copy", "move").key("M").with(json!({ "bodies": names })).on(!locked),
        cmd(app, "component.from_bodies", "Create Components from Bodies").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.material", "Physical Material", "").with(json!({ "bodies": names })),
        act("ui.appearance", "Appearance", "").key("A").on(false),
        act("ui.properties", "Properties", "measure").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": names })),
        act("ui.export", "Export…", "export").with(json!({ "bodies": names })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": items })).on(!locked),
        cmd(app, "solid.remove", "Remove").with(json!({ "bodies": names })).on(!locked),
        act("ui.rename", "Rename", "").with(json!({ "body": bodies.first() })).on(one),
        Item::sep(),
        act(if all_hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": names })),
        act("ui.lock", if locked { "Unlock" } else { "Lock" }, "").with(json!({ "bodies": names })),
        act("ui.isolate", "Isolate", "").with(json!({ "bodies": names })),
    ];
    if one {
        let b = bodies.first().cloned().unwrap_or_default();
        v.push(Item::sep());
        if !browser {
            v.push(act("ui.findBrowser", "Find in Browser", "").with(json!({ "body": b })));
        }
        v.push(act("ui.findTimeline", "Find in Timeline", "").with(json!({ "body": b })));
    }
    v
}

fn sketch_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let visible = crate::browser::sketch_visible(app, id);
    let profiles = !app.ui.hidden_profiles.contains(&id);
    let dims = app.ui.shown_dims.contains(&id);
    let editing = app.session.active_sketch == Some(id);
    let has_profiles = app.session.world_state().sketch(id).is_some_and(|s| !s.profiles.is_empty());
    vec![
        act("ui.extrudeSketch", "Extrude", "extrude").key("E").with(json!({ "sketch": id })).on(has_profiles && !editing),
        Item::sep(),
        act("ui.editSketch", "Edit Sketch", "sketch").with(json!({ "sketch": id })).on(!editing),
        act("ui.redefineSketch", "Redefine Sketch Plane", "plane").with(json!({ "sketch": id })),
        act("ui.exportDxf", "Export DXF…", "export").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.delete", "Delete", "delete").key("Del").with(json!({ "items": [{"type": "feature", "id": id}] })),
        act("ui.rename", "Rename", "").with(json!({ "feature": id })),
        Item::sep(),
        act("ui.lookAt", "Look At", "").with(json!({ "sketch": id })),
        act("ui.sketchProfile", if profiles { "Hide Profile" } else { "Show Profile" }, "").with(json!({ "sketch": id })),
        act("ui.sketchDims", if dims { "Hide Dimension" } else { "Show Dimension" }, "dimension").with(json!({ "sketch": id })),
        act(if visible { "ui.hideSketch" } else { "ui.showSketch" }, "Show/Hide", "eye").key("V").with(json!({ "sketch": id })),
        Item::sep(),
        act("ui.findTimeline", "Find in Timeline", "").with(json!({ "feature": id })),
    ]
}

/// An assembly joint: edit, drive, suppress, delete.
fn joint_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let Some(j) = app.session.doc.assembly.joints.iter().find(|j| j.id == id) else { return Vec::new() };
    let rigid = j.kind == solvecraft_engine::doc::joints::JointKind::Rigid;
    vec![
        act("ui.editJoint", "Edit Joint", "joint").with(json!({ "joint": id })),
        act("ui.driveJoint", "Drive Joint", "").with(json!({ "joint": id })).on(!rigid),
        cmd(app, "joint.edit", if j.suppressed { "Unsuppress" } else { "Suppress" }).with(json!({ "joint": id, "suppressed": !j.suppressed })),
        Item::sep(),
        cmd(app, "joint.delete", "Delete").key("Del").with(json!({ "joint": id })),
        act("ui.rename", "Rename", "").with(json!({ "joint": id })),
    ]
}

fn component_items(app: &SolveApp, id: u64) -> Vec<Item> {
    let doc = &app.session.doc;
    let occ = doc.occurrence_of(id);
    let active = app.session.active_component == id;
    let bodies = component_bodies(app, id);
    let hidden = !bodies.is_empty() && bodies.iter().all(|b| app.ui.hidden_bodies.contains(b));
    let names = json!(bodies);
    let mut v = Vec::new();
    if id != 0 {
        let grounded = occ.is_some_and(|o| o.grounded);
        let oid = occ.map(|o| o.id).unwrap_or(0);
        v.push(act("ui.activate", "Activate", "component").with(json!({ "component": id })).on(!active));
        v.push(cmd(app, "occurrence.ground", if grounded { "Unground" } else { "Ground" }).with(json!({ "occurrence": oid, "grounded": !grounded })));
        v.push(act("ui.moveOccurrence", "Move/Copy", "move").key("M").with(json!({ "occurrence": oid })).on(!grounded && occ.is_some()));
        v.push(cmd(app, "component.capture_position", "Capture Position").on(!app.session.pending_moves.is_empty()));
        v.push(cmd(app, "component.revert_position", "Revert Position").on(!app.session.pending_moves.is_empty()));
        v.push(Item::sep());
    } else {
        v.push(act("ui.activate", "Activate", "component").with(json!({ "component": id })).on(!active));
    }
    v.push(cmd(app, "component.create", "New Component").with(json!({ "parent": id })));
    if id == 0 {
        v.push(cmd(app, "component.from_bodies", "Create Components from Bodies").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    }
    v.push(Item::sep());
    v.push(act("ui.material", "Physical Material", "").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.appearance", "Appearance", "").key("A").on(false));
    v.push(act("ui.properties", "Properties", "measure").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(Item::sep());
    v.push(act("ui.export", "Export…", "export").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.saveMesh", "Save As Mesh", "export").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(Item::sep());
    if id != 0 {
        v.push(cmd(app, "occurrence.copy", "Paste (Instance)").with(json!({ "component": id, "translate": [10, 10, 0] })));
        v.push(cmd(app, "component.paste_new", "Paste New").with(json!({ "component": id, "translate": [10, 10, 0] })));
        v.push(act("ui.delete", "Delete", "delete").key("Del").with(json!({ "occurrences": [occ.map(|o| o.id)] })).on(occ.is_some()));
    }
    v.push(act("ui.rename", "Rename", "").with(json!({ "component": id })));
    v.push(Item::sep());
    v.push(act(if hidden { "ui.show" } else { "ui.hide" }, "Show/Hide", "eye").key("V").with(json!({ "bodies": names })).on(!bodies.is_empty()));
    v.push(act("ui.showAll", "Show All Bodies", "eye"));
    v.push(act("ui.isolate", "Isolate", "").with(json!({ "bodies": names })).on(!bodies.is_empty() && id != 0));
    v.push(act("ui.newGroup", "New Group", "folder").with(json!({ "component": id })));
    v
}

/// Sketch entity items: construction, fix, delete, dimension and the constraints the selection
/// can take.
fn sketch_entity_items(app: &SolveApp) -> Vec<Item> {
    use solvecraft_engine::sketch::CurveKind;
    let Some(sid) = app.session.active_sketch else { return Vec::new() };
    let st = app.session.world_state();
    let Some(ss) = st.sketch(sid) else { return Vec::new() };
    let mut lines = Vec::new();
    let mut rounds = Vec::new();
    let mut curves = Vec::new();
    let mut points = Vec::new();
    // Selection order: a two-entity constraint keeps the first and moves the second.
    let mut picked: Vec<String> = Vec::new();
    for s in &app.session.selection {
        match s {
            Sel::SketchCurve { id } => {
                picked.push(id.clone());
                curves.push(id.clone());
                match ss.sketch.curve_index(id).and_then(|i| ss.sketch.curves.get(i)).map(|c| &c.kind) {
                    Some(CurveKind::Line { .. }) => lines.push(id.clone()),
                    Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }) => rounds.push(id.clone()),
                    _ => {}
                }
            }
            Sel::SketchPoint { id } => {
                picked.push(id.clone());
                points.push(id.clone());
            }
            _ => {}
        }
    }
    let mut v = Vec::new();
    if curves.is_empty() && points.is_empty() {
        v.push(cmd(app, "sketch.line", "Line"));
        v.push(cmd(app, "sketch.rectangle.two_point", "Rectangle"));
        v.push(cmd(app, "sketch.circle.center", "Circle"));
        v.push(cmd(app, "sketch.dimension", "Sketch Dimension"));
        return v;
    }
    let ents: Vec<String> = curves.iter().chain(points.iter()).cloned().collect();
    v.push(cmd(app, "sketch.move", "Move/Copy").key("M"));
    if !curves.is_empty() {
        v.push(cmd(app, "sketch.construction", "Normal / Construction").with(json!({ "curves": curves })));
    }
    v.push(act("ui.fix", "Fix/Unfix", "c_fix").with(json!({ "entities": ents })));
    v.push(act("ui.delete", "Delete", "delete").key("Del"));
    v.push(cmd(app, "sketch.dimension", "Sketch Dimension").with(json!({ "entities": ents })).on(ents.len() <= 2));
    let mut c = Vec::new();
    let n = ents.len();
    let (l, r, p) = (lines.len(), rounds.len(), points.len());
    if l == 1 && n == 1 {
        c.push(cmd(app, "sketch.constraint.horizontal_vertical", "Horizontal/Vertical").with(json!({ "line": lines[0] })));
    }
    if p == 2 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": points[0], "b": points[1] })));
        c.push(cmd(app, "sketch.constraint.horizontal_vertical", "Horizontal/Vertical").with(json!({ "points": points })));
    }
    if p == 1 && curves.len() == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.coincident", "Coincident").with(json!({ "a": picked[0], "b": picked[1] })));
        if l == 1 {
            c.push(cmd(app, "sketch.constraint.midpoint", "MidPoint").with(json!({ "point": points[0], "line": lines[0] })));
        }
    }
    if l == 2 && n == 2 {
        let ab = json!({ "a": lines[0], "b": lines[1] });
        c.push(cmd(app, "sketch.constraint.parallel", "Parallel").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.perpendicular", "Perpendicular").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.collinear", "Collinear").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.equal", "Equal").with(ab));
    }
    if r == 2 && n == 2 {
        let ab = json!({ "a": rounds[0], "b": rounds[1] });
        c.push(cmd(app, "sketch.constraint.concentric", "Concentric").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.equal", "Equal").with(ab.clone()));
        c.push(cmd(app, "sketch.constraint.tangent", "Tangent").with(ab));
    }
    if l == 1 && r == 1 && n == 2 {
        c.push(cmd(app, "sketch.constraint.tangent", "Tangent").with(json!({ "a": picked[0], "b": picked[1] })));
    }
    if p == 2 && l == 1 && n == 3 {
        c.push(cmd(app, "sketch.constraint.symmetry", "Symmetry").with(json!({ "a": points[0], "b": points[1], "line": lines[0] })));
    }
    if !c.is_empty() {
        v.push(Item::heading("Constraints"));
        v.extend(c);
    }
    v
}

// ---------------------------------------------------------------------------------------------
// Running an item
// ---------------------------------------------------------------------------------------------

fn strs(p: &Value, k: &str) -> Vec<String> {
    p.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn id_of(p: &Value, k: &str) -> Option<u64> {
    p.get(k).and_then(Value::as_u64)
}

/// Run a menu item (what a click on it does). `at` places a rename box.
pub fn run_item(app: &mut SolveApp, item: &Item, at: Pos2) {
    if !item.enabled || item.is_sep() || item.is_heading() {
        return;
    }
    let p = item.params.clone().unwrap_or(Value::Null);
    let bodies = strs(&p, "bodies");
    match item.id.as_str() {
        "ui.repeat" => {
            if let Some((id, _)) = app.last_command.clone() {
                app.start(&id);
            }
        }
        "ui.delete" if item.params.is_some() => crate::delete::request(app, p),
        "ui.delete" => delete(app),
        "ui.finishSketch" => app.finish_sketch(),
        "ui.hide" => {
            for b in bodies {
                if !app.ui.hidden_bodies.contains(&b) {
                    app.ui.hidden_bodies.push(b);
                }
            }
        }
        "ui.show" => app.ui.hidden_bodies.retain(|b| !bodies.contains(b)),
        "ui.showAll" => {
            app.ui.hidden_bodies.clear();
            app.ui.hidden_sketches.clear();
        }
        "ui.isolate" => {
            app.ui.hidden_bodies = app.session.world_state().bodies.iter().map(|b| b.name.clone()).filter(|b| !bodies.contains(b)).collect();
        }
        "ui.lock" => {
            let locked = bodies.iter().all(|b| app.ui.locked_bodies.contains(b));
            if locked {
                app.ui.locked_bodies.retain(|b| !bodies.contains(b));
            } else {
                app.ui.locked_bodies.extend(bodies.into_iter().filter(|b| !app.ui.locked_bodies.contains(b)).collect::<Vec<_>>());
            }
        }
        "ui.moveBodies" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start("solid.move");
        }
        "ui.rename" => {
            let what = if let Some(b) = p.get("body").and_then(Value::as_str) {
                RenameWhat::Body(b.to_string())
            } else if let Some(f) = id_of(&p, "feature") {
                RenameWhat::Feature(f)
            } else if let Some(c) = id_of(&p, "component") {
                RenameWhat::Component(c)
            } else if let Some(g) = id_of(&p, "group") {
                RenameWhat::Group(g)
            } else if let Some(c) = id_of(&p, "canvas") {
                RenameWhat::Canvas(c)
            } else if let Some(v) = p.get("view").and_then(Value::as_str) {
                RenameWhat::View(v.to_string())
            } else if let Some(j) = id_of(&p, "joint") {
                RenameWhat::Joint(j)
            } else {
                return;
            };
            start_rename(app, what, at);
        }
        "ui.properties" => match app.session.execute("inspect.measure", &json!({ "bodies": bodies })) {
            Ok(v) => app.menu.props = Some((if bodies.len() == 1 { bodies.join("") } else { format!("{} bodies", bodies.len()) }, v)),
            Err(e) => app.set_status(e.to_string(), true),
        },
        "ui.export" => {
            let name = bodies.first().cloned().unwrap_or_else(|| app.session.doc.name.clone());
            if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f(&format!("{name}.step"), &["step", "stp", "igs", "stl", "3mf", "obj"]))
            {
                let _ = app.run("file.export", json!({ "path": path, "bodies": bodies }));
            }
        }
        "ui.findBrowser" => {
            if let Some(b) = p.get("body").and_then(Value::as_str) {
                app.ui.show_browser = true;
                app.tree.reveal = Some(b.to_string());
                let _ = app.run("select.set", json!({"items": [{"type": "body", "name": b}]}));
            }
        }
        "ui.findTimeline" => {
            let f = id_of(&p, "feature").or_else(|| p.get("body").and_then(Value::as_str).and_then(|b| body_feature(app, b)));
            if let Some(f) = f {
                app.ui.show_timeline = true;
                let _ = app.run("select.set", json!({"items": [{"type": "feature", "id": f}]}));
            }
        }
        "ui.tangentChain" => tangent_chain(app),
        "ui.origin" => app.ui.show_origin = !app.ui.show_origin,
        "ui.clear" => drop(app.run("select.clear", json!({}))),
        "ui.unsection" => drop(app.run("inspect.section", json!({"clear": true}))),
        "ui.fit" => app.animate_view("fit"),
        "ui.home" => app.animate_view("home"),
        // One command for the whole selection: all fixed unless all already are.
        "ui.fix" => drop(app.run("sketch.constraint.fix", json!({ "entities": strs(&p, "entities") }))),
        "ui.editSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                app.edit_sketch(id);
            }
        }
        "ui.redefineSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                crate::browser::redefine_sketch(app, id);
            }
        }
        "ui.hideSketch" | "ui.showSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                crate::browser::set_sketch_visible(app, id, item.id == "ui.showSketch");
            }
        }
        "ui.sketchDims" => {
            if let Some(id) = id_of(&p, "sketch") {
                if app.ui.shown_dims.contains(&id) {
                    app.ui.shown_dims.retain(|x| *x != id);
                } else {
                    app.ui.shown_dims.push(id);
                }
            }
        }
        "ui.sketchProfile" => {
            if let Some(id) = id_of(&p, "sketch") {
                if app.ui.hidden_profiles.contains(&id) {
                    app.ui.hidden_profiles.retain(|x| *x != id);
                } else {
                    app.ui.hidden_profiles.push(id);
                }
            }
        }
        "ui.activate" => {
            if let Some(id) = id_of(&p, "component") {
                let _ = app.run("component.activate", json!({ "component": id }));
            }
        }
        "ui.moveOccurrence" => {
            if let Some(o) = id_of(&p, "occurrence") {
                app.tree.occurrence_move = Some(crate::browser::OccMove::new(o));
            }
        }
        "ui.canvasVisible" => drop(app.run("canvas.edit", p)),
        "ui.toolbarPin" | "ui.toolbarRemove" | "ui.toolbarReset" | "ui.shortcutPin" => crate::toolbar_custom::run(app, &item.id, &p),
        "ui.workspace" => {}
        "ui.editJoint" | "ui.driveJoint" => {
            if let Some(id) = id_of(&p, "joint") {
                let d = if item.id == "ui.editJoint" {
                    crate::dialogs_assembly::edit_joint(app, id)
                } else {
                    crate::dialogs_assembly::drive_joint(app, id)
                };
                if let Some(d) = d {
                    app.tool = None;
                    app.dialog = Some(d);
                }
            }
        }
        "ui.newView" => drop(crate::browser::save_view(app, None)),
        "ui.restoreView" => {
            if let Some(v) = p.get("view").and_then(Value::as_str) {
                crate::browser::restore_view(app, v);
            }
        }
        "ui.updateView" => {
            if let Some(v) = p.get("view").and_then(Value::as_str) {
                crate::browser::save_view(app, Some(v));
            }
        }
        "ui.folderVisible" => {
            if let Some(f) = p.get("folder").and_then(Value::as_str) {
                crate::browser::toggle_folder(app, id_of(&p, "component").unwrap_or(0), f);
            }
        }
        "ui.originAll" => {
            app.ui.show_origin = true;
            app.ui.hidden_origin.retain(|h| !["O", "X", "Y", "Z", "XY", "XZ", "YZ"].contains(&h.as_str()));
        }
        "ui.originPart" => {
            let show = p.get("show").and_then(Value::as_bool).unwrap_or(true);
            for k in strs(&p, "keys") {
                app.ui.hidden_origin.retain(|h| *h != k);
                if !show {
                    app.ui.hidden_origin.push(k);
                }
            }
        }
        "ui.editFeature" => {
            if let Some(f) = id_of(&p, "feature") {
                app.edit_feature(f);
            }
        }
        "ui.material" => {
            let items: Vec<Value> = bodies.iter().map(|b| json!({"type": "body", "name": b})).collect();
            let _ = app.run("select.set", json!({ "items": items }));
            app.start("material.assign");
        }
        "ui.saveMesh" => {
            let name = bodies.first().cloned().unwrap_or_else(|| app.session.doc.name.clone());
            if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f(&format!("{name}.stl"), &["stl"])) {
                let _ = app.run("file.save_mesh", json!({ "path": path, "bodies": bodies }));
            }
        }
        "ui.exportDxf" => {
            if let Some(id) = id_of(&p, "sketch") {
                let name = app.session.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
                if let Some(path) = app.services.pick_save.as_ref().and_then(|f| f(&format!("{name}.dxf"), &["dxf"])) {
                    let _ = app.run("sketch.export_dxf", json!({ "sketch": id, "path": path }));
                }
            }
        }
        "ui.extrudeSketch" => {
            if let Some(id) = id_of(&p, "sketch") {
                let n = app.session.world_state().sketch(id).map_or(0, |s| s.profiles.len());
                let items: Vec<Value> = (0..n).map(|i| json!({"type": "profile", "sketch": id, "index": i})).collect();
                let _ = app.run("select.set", json!({ "items": items }));
                app.start("solid.extrude");
            }
        }
        "ui.lookAt" => {
            let st = app.session.world_state();
            if let Some(ss) = id_of(&p, "sketch").and_then(|id| st.sketch(id)) {
                let mut to = app.cam.looking_from(ss.plane.normal());
                to.target = ss.plane.origin;
                app.animate_to(to);
            }
        }
        "ui.nav" => {
            app.viewport.nav = match p.get("mode").and_then(Value::as_str) {
                Some("pan") => Some(crate::viewport::NavMode::Pan),
                Some("zoom") => Some(crate::viewport::NavMode::Zoom),
                Some("orbit") => Some(crate::viewport::NavMode::Orbit),
                _ => None,
            };
        }
        "ui.editCanvas" | "ui.calibrateCanvas" => {
            if let Some(c) = id_of(&p, "canvas") {
                app.tree.canvas_panel = Some(crate::browser::CanvasPanel::new(app, c, item.id == "ui.calibrateCanvas"));
            }
        }
        "ui.newGroup" | "ui.ungroup" | "ui.groupSelected" => crate::browser::group_action(app, &item.id, &p),
        id if item.params.is_some() => {
            let _ = app.run(id, p);
        }
        id => app.start(id),
    }
}

/// Delete what is selected: sketch entities, features, and bodies (a Remove feature).
pub fn delete(app: &mut SolveApp) {
    crate::delete::delete_selection(app);
}

/// Show/Hide (the V key): what is selected in the viewport or picked in the browser flips its
/// visibility (bodies, faces' and edges' bodies, sketches, construction and origin planes and
/// axes, components' bodies, canvases). When some are shown and some hidden, all are hidden.
pub fn toggle_visibility(app: &mut SolveApp) {
    use solvecraft_engine::doc::FeatureKind;
    let mut bodies: Vec<String> = touched_bodies(app);
    for c in app.tree.picked_components.clone() {
        bodies.extend(component_bodies(app, c));
    }
    let sketches: Vec<u64> = app
        .session
        .selection
        .iter()
        .filter_map(|s| if let Sel::Feature { id } = s { Some(*id) } else { None })
        .filter(|id| app.session.doc.feature(*id).is_some_and(|f| matches!(f.kind, FeatureKind::Sketch { .. })))
        .collect();
    let origin: Vec<String> = app
        .session
        .selection
        .iter()
        .filter_map(|s| match s {
            Sel::Plane { name } | Sel::Axis { name } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let canvases = app.tree.picked_canvases.clone();
    let any_visible = bodies.iter().any(|b| !app.ui.hidden_bodies.contains(b))
        || sketches.iter().any(|s| crate::browser::sketch_visible(app, *s))
        || origin.iter().any(|o| !app.ui.hidden_origin.contains(o))
        || app.session.doc.canvases.iter().any(|c| canvases.contains(&c.id) && c.visible);
    let hide = any_visible;
    app.ui.hidden_bodies.retain(|b| !bodies.contains(b));
    app.ui.hidden_origin.retain(|o| !origin.contains(o));
    if hide {
        app.ui.hidden_bodies.extend(bodies);
        app.ui.hidden_origin.extend(origin);
    }
    for s in sketches {
        crate::browser::set_sketch_visible(app, s, !hide);
    }
    for c in canvases {
        let _ = app.run("canvas.edit", json!({ "canvas": c, "visible": !hide }));
    }
}

/// Rename what is selected (F2): one body, sketch or feature, construction plane, component
/// picked in the browser, or canvas.
pub fn rename_selection(app: &mut SolveApp, at: Pos2) -> bool {
    let what = match (app.session.selection.as_slice(), app.tree.picked_components.as_slice(), app.tree.picked_canvases.as_slice()) {
        ([Sel::Body { name }], [], []) => RenameWhat::Body(name.clone()),
        ([Sel::Feature { id }], [], []) => RenameWhat::Feature(*id),
        ([Sel::Plane { name }], [], []) => match app.session.doc.find_feature(name) {
            Some(f) => RenameWhat::Feature(f.id),
            None => return false,
        },
        ([], [c], []) => RenameWhat::Component(*c),
        ([], [], [c]) => RenameWhat::Canvas(*c),
        _ => return false,
    };
    start_rename(app, what, at);
    true
}

/// Add the edges that continue the selected edges smoothly to the selection.
fn tangent_chain(app: &mut SolveApp) {
    let st = app.session.world_state();
    let mut out: Vec<Sel> = Vec::new();
    for s in &app.session.selection {
        let Sel::Edge { body, index, .. } = s else { continue };
        let Some(b) = st.body(body) else { continue };
        let m = b.mesh();
        for i in m.tangent_chain(*index, 2f64.to_radians()) {
            let Some(e) = m.edges.get(i) else { continue };
            let Some(mid) = e.get(e.len() / 2).copied() else { continue };
            let x = Sel::Edge { body: body.clone(), index: i, point: mid };
            if !out.iter().any(|y| matches!(y, Sel::Edge { body: bb, index: ii, .. } if bb == body && *ii == i)) {
                out.push(x);
            }
        }
    }
    if !out.is_empty() {
        let _ = app.run("select.set", json!({ "items": out }));
    }
}

pub fn start_rename(app: &mut SolveApp, what: RenameWhat, at: Pos2) {
    let doc = &app.session.doc;
    let text = match &what {
        RenameWhat::Body(b) => b.clone(),
        RenameWhat::Feature(f) => doc.feature(*f).map(|f| f.name.clone()).unwrap_or_default(),
        RenameWhat::Component(0) => doc.name.clone(),
        RenameWhat::Component(c) => doc.components.iter().find(|x| x.id == *c).map(|x| x.name.clone()).unwrap_or_default(),
        RenameWhat::Group(g) => crate::browser::group_name(app, *g).unwrap_or_default(),
        RenameWhat::Canvas(c) => doc.canvases.iter().find(|x| x.id == *c).map(|x| x.name.clone()).unwrap_or_default(),
        RenameWhat::View(v) => v.clone(),
        RenameWhat::Joint(j) => doc.assembly.joints.iter().find(|x| x.id == *j).map(|x| x.name.clone()).unwrap_or_default(),
    };
    app.menu.rename = Some(Rename { what, text, at, focused: false });
}

fn commit_rename(app: &mut SolveApp, r: &Rename) {
    let name = r.text.trim().to_string();
    if name.is_empty() {
        return;
    }
    match &r.what {
        RenameWhat::Body(old) => {
            if app.run("body.rename", json!({ "body": old, "name": name })).is_ok() {
                for list in [&mut app.ui.hidden_bodies, &mut app.ui.locked_bodies] {
                    for b in list.iter_mut().filter(|b| *b == old) {
                        *b = name.clone();
                    }
                }
            }
        }
        RenameWhat::Feature(f) => drop(app.run("timeline.rename", json!({ "feature": f, "name": name }))),
        RenameWhat::Component(c) => drop(app.run("component.rename", json!({ "component": c, "name": name }))),
        RenameWhat::Group(g) => crate::browser::group_action(app, "ui.renameGroup", &json!({ "group": g, "name": name })),
        RenameWhat::Canvas(c) => drop(app.run("canvas.edit", json!({ "canvas": c, "name": name }))),
        RenameWhat::View(v) => drop(app.run("view.rename", json!({ "view": v, "name": name }))),
        RenameWhat::Joint(j) => drop(app.run("joint.edit", json!({ "joint": j, "name": name }))),
    }
}

// ---------------------------------------------------------------------------------------------
// Opening and showing
// ---------------------------------------------------------------------------------------------

/// Open the viewport menu at a screen position. A right-click on something not selected
/// selects it first, so the menu is about what is under the cursor.
pub fn open(app: &mut SolveApp, at: Pos2) {
    if app.session.active_sketch.is_none()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && !app.session.selection.contains(&sel)
        && !matches!(sel, Sel::Plane { .. } | Sel::Axis { .. })
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
    } else if app.session.active_sketch.is_some()
        && let Some(h) = app.viewport.hover.clone()
        && let Some((_, sel)) = crate::viewport::candidate(app, &[h])
        && matches!(sel, Sel::SketchCurve { .. } | Sel::SketchPoint { .. })
        && !app.session.selection.contains(&sel)
    {
        let _ = app.run("select.set", json!({ "items": [sel] }));
    }
    open_for(app, at, Target::Viewport);
}

/// Open a menu for a target (the radial ring only for the viewport).
pub fn open_for(app: &mut SolveApp, at: Pos2, target: Target) {
    app.menu.key_row = None;
    app.menu.radial = target == Target::Viewport;
    app.menu.target = target;
    app.menu.flick = false;
    app.viewport.context_menu = Some(at);
}

pub fn close(app: &mut SolveApp) {
    app.viewport.context_menu = None;
    app.menu.flick = false;
}

/// Open the menu at a point for automation: the pointer moves there first so what is under it
/// counts, then the menu opens on a later frame.
pub fn request_open(app: &mut SolveApp, at: Pos2, target: Target) {
    app.synthetic.push(egui::Event::PointerMoved(at));
    app.menu.pending = Some((at, target));
}

/// The open menu as JSON (for `ui.menu`).
pub fn describe(app: &SolveApp) -> Value {
    let Some(at) = app.viewport.context_menu else {
        return json!({ "open": false, "pending": app.menu.pending.is_some() });
    };
    let radial = if app.menu.radial { radial_items(app) } else { Vec::new() };
    json!({
        "open": true,
        "at": [at.x, at.y],
        "target": app.menu.target,
        "radial": radial,
        "items": items(app, &app.menu.target),
        "rename": app.menu.rename.as_ref().map(|r| r.text.clone()),
    })
}

/// Pick an item of the open menu by id or label (for `ui.menuPick`).
pub fn pick(app: &mut SolveApp, key: &str) -> Result<Value, String> {
    let Some(at) = app.viewport.context_menu else { return Err("no menu is open".into()) };
    let target = app.menu.target.clone();
    let mut all = if app.menu.radial { radial_items(app) } else { Vec::new() };
    all.extend(items(app, &target));
    let item =
        all.into_iter().find(|i| !i.is_sep() && !i.is_heading() && (i.id == key || i.label == key)).ok_or_else(|| format!("no menu item `{key}`"))?;
    if !item.enabled {
        return Err(format!("`{}` is disabled", item.label));
    }
    close(app);
    run_item(app, &item, at);
    Ok(json!({ "ran": item.id, "label": item.label }))
}

/// Where the radial menu goes so its ring stays on screen.
fn radial_center(at: Pos2, screen: Rect) -> Pos2 {
    let mx = RX + 165.0;
    let my = RY + 34.0;
    let x = if screen.width() > 2.0 * mx { at.x.clamp(screen.left() + mx, screen.right() - mx) } else { screen.center().x };
    let y = if screen.height() > 2.0 * my { at.y.clamp(screen.top() + my, screen.bottom() - my) } else { screen.center().y };
    pos2(x, y)
}

/// Height of a list of items as [`list_row`] draws them.
fn list_height(items: &[Item]) -> f32 {
    8.0 + items
        .iter()
        .map(|i| {
            if i.is_sep() {
                7.0
            } else if i.is_heading() {
                20.0
            } else {
                24.0
            }
        })
        .sum::<f32>()
}

/// Where the list goes next to a ring centred at `c`: below it when it fits, otherwise beside
/// it (right, else left), kept on screen.
fn list_place(c: Pos2, h: f32, screen: Rect) -> Pos2 {
    let below = c.y + RY + 26.0;
    if below + h <= screen.bottom() - 4.0 {
        return pos2((c.x - LIST_W / 2.0).clamp(screen.left() + 4.0, (screen.right() - LIST_W - 4.0).max(screen.left())), below);
    }
    let y = (c.y - h / 2.0).clamp(screen.top() + 4.0, (screen.bottom() - h - 4.0).max(screen.top()));
    let right = c.x + RX + 170.0;
    if right + LIST_W <= screen.right() - 4.0 {
        return pos2(right, y);
    }
    pos2((c.x - RX - 170.0 - LIST_W).max(screen.left() + 4.0), y)
}

/// Width of list menus.
pub const LIST_W: f32 = 250.0;

/// The radial slot a pointer offset points at (none near the centre).
fn sector(d: Vec2) -> Option<usize> {
    if d.length() < 22.0 {
        return None;
    }
    // Clockwise from the top (screen y points down).
    let a = d.x.atan2(-d.y).to_degrees();
    let k = ((a + 360.0 + 22.5) / 45.0).floor() as i64;
    Some((k.rem_euclid(DIRS as i64)) as usize)
}

/// The rectangle of radial slot `k` with a label of `size`.
fn slot_rect(c: Pos2, k: usize, size: Vec2) -> Rect {
    let a = (k as f32 * 45.0).to_radians();
    let p = c + vec2(a.sin() * RX, -a.cos() * RY);
    let (sx, sy) = (a.sin(), -a.cos());
    let ax = if sx > 0.3 {
        0.0
    } else if sx < -0.3 {
        1.0
    } else {
        0.5
    };
    let ay = if sy > 0.9 {
        0.0
    } else if sy < -0.9 {
        1.0
    } else {
        0.5
    };
    Rect::from_min_size(p - vec2(size.x * ax, size.y * ay), size)
}

/// Press-and-hold of the right button in the viewport opens the menu; releasing over a
/// direction picks it.
fn track_hold(app: &mut SolveApp, ctx: &egui::Context) {
    let (pressed, down, released, pos, time) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.pointer.secondary_down(),
            i.pointer.button_released(egui::PointerButton::Secondary),
            i.pointer.latest_pos(),
            i.time,
        )
    });
    let free = app.tool.is_none() && app.dialog.is_none();
    if pressed && free && app.viewport.context_menu.is_none() && app.viewport.mouse.is_some() {
        app.menu.press = pos.map(|p| (p, time));
    }
    if let Some((p0, t0)) = app.menu.press {
        let moved = pos.is_some_and(|p| p.distance(p0) > 5.0);
        if !down || moved {
            app.menu.press = None;
        } else if time - t0 > 0.35 {
            app.menu.press = None;
            open(app, p0);
            app.menu.flick = true;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    if released && app.menu.flick {
        app.menu.flick = false;
        if let (Some(at), Some(p)) = (app.viewport.context_menu, pos) {
            let c = radial_center(at, ctx.content_rect());
            if app.menu.radial
                && let Some(k) = sector(p - c)
                && p.distance(c) > 36.0
                && let Some(item) = radial_items(app).into_iter().nth(k).filter(|i| i.enabled)
            {
                close(app);
                run_item(app, &item, at);
            }
        }
    }
}

/// Show the open menu, the rename box and the Properties window.
pub fn show(app: &mut SolveApp, ctx: &egui::Context) {
    if let Some((at, target)) = app.menu.pending.clone()
        && app.synthetic.is_empty()
    {
        app.menu.pending = None;
        if target == Target::Viewport { open(app, at) } else { open_for(app, at, target) }
    }
    crate::browser::dims_overlay(app, ctx);
    track_hold(app, ctx);
    show_rename(app, ctx);
    show_props(app, ctx);
    let Some(at) = app.viewport.context_menu else { return };
    let t = Tokens::get();
    let screen = ctx.content_rect();
    let target = app.menu.target.clone();
    let list = items(app, &target);
    let mut picked: Option<Item> = None;
    let mut inside = false;
    let pointer = ctx.input(|i| i.pointer.latest_pos());

    let list_at = if app.menu.radial {
        let c = radial_center(at, screen);
        let radial = radial_items(app);
        let area = Rect::from_center_size(c, vec2(2.0 * (RX + 165.0), 2.0 * (RY + 34.0)));
        inside |= pointer.is_some_and(|p| area.contains(p));
        let hot = pointer.and_then(|p| sector(p - c));
        let resp = egui::Area::new(egui::Id::new("sc_marking_menu")).fixed_pos(area.min).order(egui::Order::Foreground).show(ctx, |ui| {
            let (r, _) = ui.allocate_exact_size(area.size(), Sense::hover());
            let painter = ui.painter_at(r.expand(4.0));
            let font = FontId::proportional(13.0);
            // Centre: a ring with a pointer toward the hot direction.
            painter.circle(c, 15.0, t.panel.gamma_multiply(0.92), Stroke::new(1.0, t.border));
            if let (Some(k), Some(p)) = (hot, pointer) {
                let d = (p - c).normalized();
                painter.line_segment(
                    [c + d * 6.0, c + d * 15.0],
                    Stroke::new(3.0, if radial.get(k).is_some_and(|i| i.enabled) { t.accent } else { t.border }),
                );
            } else {
                painter.circle_filled(c, 3.0, t.text_dim);
            }
            for (k, item) in radial.iter().enumerate() {
                let galley = painter.layout_no_wrap(item.label.clone(), font.clone(), if item.enabled { t.text } else { t.text_dim });
                let size = vec2(galley.size().x + 40.0, 28.0);
                let rr = slot_rect(c, k, size);
                let resp = ui.interact(rr, ui.id().with(("radial", k)), Sense::click());
                let lit = item.enabled && (resp.hovered() || hot == Some(k));
                painter.rect(
                    rr,
                    14.0,
                    if lit { t.accent_soft } else { t.panel },
                    Stroke::new(1.0, if lit { t.accent } else { t.border }),
                    egui::StrokeKind::Inside,
                );
                let ir = Rect::from_center_size(pos2(rr.left() + 16.0, rr.center().y), vec2(16.0, 16.0));
                let ink = if item.enabled { t.icon } else { t.border };
                if !item.icon.is_empty() {
                    icons::paint(&painter, ir, &item.icon, ink, t.icon_fill, t.accent);
                }
                painter.galley(pos2(rr.left() + 30.0, rr.center().y - galley.size().y / 2.0), galley, t.text);
                if resp.clicked() && item.enabled {
                    picked = Some(item.clone());
                }
                if item.enabled {
                    resp.on_hover_text(item.shortcut.clone());
                }
            }
        });
        inside |= resp.response.contains_pointer();
        list_place(c, list_height(&list), screen)
    } else {
        at
    };

    // Up/Down choose a row of the list, Enter runs it.
    let (up, down, enter) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter)));
    let usable: Vec<usize> = list.iter().enumerate().filter(|(_, i)| i.enabled && !i.is_sep() && !i.is_heading()).map(|(k, _)| k).collect();
    if up || down {
        let at = app.menu.key_row.and_then(|r| usable.iter().position(|u| *u == r));
        let next = match (at, down) {
            (None, true) => Some(0),
            (None, false) => usable.len().checked_sub(1),
            (Some(k), true) => Some((k + 1) % usable.len().max(1)),
            (Some(k), false) => Some((k + usable.len().max(1) - 1) % usable.len().max(1)),
        };
        app.menu.key_row = next.and_then(|k| usable.get(k).copied());
    }
    if enter && let Some(item) = app.menu.key_row.and_then(|r| list.get(r)) {
        picked = Some(item.clone());
    }
    let key_row = app.menu.key_row;
    let resp = egui::Area::new(egui::Id::new("sc_context_menu")).fixed_pos(list_at).constrain(true).order(egui::Order::Foreground).show(ctx, |ui| {
        menu_frame(ui, |ui| {
            for (k, item) in list.iter().enumerate() {
                if let Some(p) = list_row_lit(ui, item, key_row == Some(k)) {
                    picked = Some(p);
                }
            }
        });
    });
    inside |= resp.response.contains_pointer() || resp.response.rect.contains(pointer.unwrap_or(Pos2::ZERO));

    let (primary, secondary, esc) = ctx.input(|i| {
        (
            i.pointer.button_pressed(egui::PointerButton::Primary),
            i.pointer.button_pressed(egui::PointerButton::Secondary),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if secondary && inside && app.menu.radial {
        // A right press on the ring: drag toward a command and release to pick it.
        app.menu.flick = true;
    } else if (primary || secondary) && !inside || esc {
        close(app);
    }
    if let Some(item) = picked {
        close(app);
        run_item(app, &item, at);
    }
}

/// The frame every menu uses.
pub fn menu_frame<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let t = Tokens::get();
    egui::Frame::new()
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(4, 4))
        .shadow(egui::Shadow {
            offset: [0, 3],
            blur: 12,
            spread: 0,
            color: Color32::from_black_alpha(if crate::theme::is_dark() { 110 } else { 50 }),
        })
        .show(ui, |ui| {
            ui.set_width(LIST_W - 10.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// One list row; the item when it was clicked.
pub fn list_row(ui: &mut egui::Ui, item: &Item) -> Option<Item> {
    list_row_lit(ui, item, false)
}

/// A list row, drawn highlighted when `lit` (the keyboard's choice).
pub fn list_row_lit(ui: &mut egui::Ui, item: &Item, lit: bool) -> Option<Item> {
    let t = Tokens::get();
    let w = LIST_W - 10.0;
    if item.is_sep() {
        let (r, _) = ui.allocate_exact_size(vec2(w, 7.0), Sense::hover());
        ui.painter().hline(r.x_range().shrink(6.0), r.center().y, Stroke::new(1.0, t.border));
        return None;
    }
    if item.is_heading() {
        let (r, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
        ui.painter().text(
            pos2(r.left() + 8.0, r.center().y + 2.0),
            Align2::LEFT_CENTER,
            item.label.to_uppercase(),
            FontId::proportional(10.5),
            t.text_dim,
        );
        return None;
    }
    let (r, resp) = ui.allocate_exact_size(vec2(w, 24.0), if item.enabled { Sense::click() } else { Sense::hover() });
    if item.enabled && (resp.hovered() || lit) {
        ui.painter().rect_filled(r.shrink2(vec2(2.0, 1.0)), 4.0, t.hover);
    }
    let ink = if item.enabled { t.icon } else { t.border };
    if !item.icon.is_empty() {
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(16.0, 16.0)),
            &item.icon,
            ink,
            t.icon_fill,
            t.accent,
        );
    }
    let text = if item.enabled { t.text } else { t.text_dim.gamma_multiply(0.75) };
    ui.painter().text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, &item.label, FontId::proportional(13.0), text);
    if !item.shortcut.is_empty() {
        ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, &item.shortcut, FontId::proportional(11.5), t.text_dim);
    }
    (item.enabled && resp.clicked()).then(|| item.clone())
}

fn show_rename(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut r) = app.menu.rename.take() else { return };
    let t = Tokens::get();
    let mut done: Option<bool> = None;
    let resp = egui::Area::new(egui::Id::new("sc_rename")).fixed_pos(r.at).constrain(true).order(egui::Order::Foreground).show(ctx, |ui| {
        menu_frame(ui, |ui| {
            ui.add_space(2.0);
            ui.label(egui::RichText::new("Rename").size(11.0).color(t.text_dim));
            let te = ui.add(egui::TextEdit::singleline(&mut r.text).desired_width(200.0).id(egui::Id::new("sc_rename_text")));
            if !r.focused {
                // Start with the whole name selected, ready to type over.
                let id = egui::Id::new("sc_rename_text");
                if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), id) {
                    let n = r.text.chars().count();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(n))));
                    st.store(ui.ctx(), id);
                }
                te.request_focus();
                r.focused = true;
            }
            if te.lost_focus() {
                done = Some(ui.input(|i| i.key_pressed(egui::Key::Enter)));
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        })
    });
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() && r.focused {
        done = Some(true);
    }
    match done {
        Some(true) => commit_rename(app, &r),
        Some(false) => {}
        None => app.menu.rename = Some(r),
    }
}

/// Finish a rename from automation: commit (true) or cancel.
pub fn finish_rename(app: &mut SolveApp, text: Option<&str>, commit: bool) -> bool {
    let Some(mut r) = app.menu.rename.take() else { return false };
    if let Some(s) = text {
        r.text = s.to_string();
    }
    if commit {
        commit_rename(app, &r);
    }
    true
}

fn show_props(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((title, v)) = app.menu.props.clone() else { return };
    let mut open = true;
    let f = |x: &Value, k: &str| x.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
    crate::frame::window(ctx, format!("Properties: {title}"), crate::frame::Width::Normal)
        .id(egui::Id::new("sc_props"))
        .open(&mut open)
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::Grid::new("sc_props_grid").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                let total = &v["total"];
                let bodies = v["bodies"].as_array().cloned().unwrap_or_default();
                if let [b] = bodies.as_slice() {
                    ui.label("Material");
                    ui.label(b["material"].as_str().unwrap_or("Default"));
                    ui.end_row();
                    ui.label("Density");
                    ui.label(format!("{:.3} g/cm³", f(b, "density_g_cm3")));
                    ui.end_row();
                }
                ui.label("Mass");
                ui.label(format!("{:.3} g", f(total, "mass_g")));
                ui.end_row();
                ui.label("Volume");
                ui.label(crate::prefs::show_mm(app, f(total, "volume_mm3"), 3));
                ui.end_row();
                ui.label("Area");
                ui.label(crate::prefs::show_mm(app, f(total, "area_mm2"), 2));
                ui.end_row();
                if let [b] = bodies.as_slice() {
                    if let Some(c) = b["center_of_mass"].as_array() {
                        let c: Vec<String> = c.iter().map(|x| crate::prefs::show_mm_bare(app, x.as_f64().unwrap_or(f64::NAN))).collect();
                        ui.label("Center of mass");
                        ui.label(format!("({}) {}", c.join(", "), crate::prefs::unit_scale(&app.session.doc.units).1));
                        ui.end_row();
                    }
                    let (mn, mx) = (&b["bbox"]["min"], &b["bbox"]["max"]);
                    if let (Some(a), Some(z)) = (mn.as_array(), mx.as_array()) {
                        let d: Vec<String> = a
                            .iter()
                            .zip(z)
                            .map(|(a, z)| crate::prefs::show_mm_bare(app, z.as_f64().unwrap_or(0.0) - a.as_f64().unwrap_or(0.0)))
                            .collect();
                        ui.label("Bounding box");
                        ui.label(format!("{} {}", d.join(" × "), crate::prefs::unit_scale(&app.session.doc.units).1));
                        ui.end_row();
                    }
                }
                ui.label("Faces / edges / vertices");
                ui.label(format!("{} / {} / {}", total["faces"], total["edges"], total["vertices"]));
                ui.end_row();
            });
        });
    if !open {
        app.menu.props = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sectors_go_clockwise_from_the_top() {
        assert_eq!(sector(vec2(0.0, -50.0)), Some(0));
        assert_eq!(sector(vec2(50.0, -50.0)), Some(1));
        assert_eq!(sector(vec2(50.0, 0.0)), Some(2));
        assert_eq!(sector(vec2(0.0, 50.0)), Some(4));
        assert_eq!(sector(vec2(-50.0, 0.0)), Some(6));
        assert_eq!(sector(vec2(-50.0, -50.0)), Some(7));
        assert_eq!(sector(vec2(3.0, 3.0)), None);
    }

    #[test]
    fn list_goes_beside_the_ring_near_the_bottom() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        let c = radial_center(pos2(800.0, 300.0), screen);
        assert!(list_place(c, 300.0, screen).y > c.y + RY, "room below");
        for at in [pos2(800.0, 900.0), pos2(1590.0, 900.0), pos2(10.0, 990.0)] {
            let c = radial_center(at, screen);
            let p = list_place(c, 400.0, screen);
            let r = Rect::from_min_size(p, vec2(LIST_W, 400.0));
            assert!(screen.contains_rect(r), "{at:?}: {r:?}");
            let ring = Rect::from_center_size(c, vec2(2.0 * (RX + 160.0), 2.0 * RY));
            assert!(!ring.intersects(r), "{at:?}: list {r:?} over ring {ring:?}");
        }
    }

    #[test]
    fn radial_menu_stays_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        for at in [pos2(2.0, 2.0), pos2(1199.0, 799.0), pos2(600.0, 400.0), pos2(-50.0, 900.0)] {
            let c = radial_center(at, screen);
            for k in 0..DIRS {
                let r = slot_rect(c, k, vec2(160.0, 28.0));
                assert!(screen.contains_rect(r), "{at:?} slot {k}: {r:?}");
            }
        }
    }
}
