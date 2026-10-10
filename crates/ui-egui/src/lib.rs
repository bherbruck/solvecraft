//! The SolveCraft egui front end.
//!
//! A thin shell over [`solvecraft_engine::Session`]: panels read engine state and act through
//! commands ([`SolveApp::run`]). Nothing here owns design data. The layout follows the familiar
//! parametric-CAD desktop: application bar, a toolbar of workspace tabs and panels, the browser
//! on the left, the 3D viewport with a view cube and navigation bar, the command dialog on the
//! right of the viewport, the command palette and the timeline at the bottom.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod access;
pub mod agent_cursor;
pub mod browser;
pub mod canvas;
pub mod context_menu;
pub mod control;
pub mod delete;
pub mod dialogs;
pub mod dialogs_appearance;
pub mod dialogs_assembly;
pub mod dialogs_motion;
pub mod dialogs_move;
pub mod dialogs_parts;
pub mod dialogs_plastic;
pub mod dialogs_sheet;
pub mod dialogs_surface;
pub mod dim_view;
pub mod documents;
pub mod drag_snap;
pub mod frame;
pub mod gizmo;
pub mod gpu;
pub mod help;
pub mod home;
pub mod icons;
pub mod inference;
pub mod keymap;
#[cfg(test)]
mod menu_tests;
#[cfg(test)]
mod status_tests {
    #[test]
    fn the_status_bar_names_commands_by_label() {
        assert_eq!(super::command_label("solid.fillet"), "Fillet");
        assert_eq!(
            super::friendly_error("solid.shell", "invalid parameters for `solid.shell`: offset: must be positive"),
            "Shell: offset: must be positive"
        );
        assert_eq!(super::friendly_error("edit.undo", "nothing to undo"), "Undo: nothing to undo");
        let mut app = super::SolveApp::new(solvecraft_engine::Session::default(), Default::default());
        app.run("solid.box", serde_json::json!({"length": 5, "width": 5, "height": 5})).unwrap();
        assert_eq!(app.session.log.last().map(String::as_str), Some("Box done"));
        let _ = app.run("edit.redo", serde_json::json!({}));
        assert!(app.session.log.last().is_some_and(|l| l.starts_with("Redo: ") && !l.contains("edit.redo")), "{:?}", app.session.log.last());
    }
}
pub mod palette;
pub mod params_dialog;
pub mod prefs;
#[cfg(test)]
mod preselect_tests;
pub mod preview;
pub mod recovery_ui;
pub mod ref_images;
pub mod scenario;
pub mod selection;
pub mod shortcut_box;
pub mod sketch3d;
pub mod sketch_dims;
#[cfg(test)]
mod sketch_edit_tests;
pub mod sketch_palette;
pub mod sketch_tools;
pub mod stand_up;
pub mod theme;
pub mod timeline;
pub mod titlebar;
pub mod toolbar;
pub mod toolbar_custom;
pub mod tools;
pub mod viewport;
pub mod workspace;

use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::render::{Camera, CameraAnim, StandardView};

pub use control::{ControlRequest, ControlResponse};

/// UI state that automation can read and set (`ui.set`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    /// Toolbar tab: SOLID, SURFACE, MESH, SHEET METAL, PLASTIC, UTILITIES (SKETCH while sketching).
    pub tab: String,
    pub show_browser: bool,
    pub show_timeline: bool,
    pub show_grid: bool,
    pub show_origin: bool,
    pub perspective: bool,
    pub hidden_bodies: Vec<String>,
    /// Origin items (O, X, Y, Z, XY, XZ, YZ) and construction planes hidden one by one.
    pub hidden_origin: Vec<String>,
    pub show_sketches: bool,
    pub palette_open: bool,
    /// Dark theme (the default); a preference kept between runs.
    pub dark: bool,
    /// Selection filter: kinds a click doesn't pick (`vertices`, `edges`, `faces`, `sketch`).
    pub pick_off: Vec<String>,
    /// A click on a face picks its whole body.
    pub pick_bodies: bool,
    /// 0: shaded with edges, 1: shaded, 2: wireframe, 3: shaded with hidden edges.
    pub visual_style: u8,
    /// A soft shadow on the ground under the model (seen from above).
    pub ground_shadow: bool,
    /// The world's up axis (Z or Y; #3): the camera, view cube, home view and ground follow it.
    pub up_axis: solvecraft_engine::render::UpAxis,
    /// Sketches hidden one by one, and finished sketches shown although a feature uses them.
    pub hidden_sketches: Vec<u64>,
    pub shown_sketches: Vec<u64>,
    /// Sketches whose closed profiles are not shaded.
    pub hidden_profiles: Vec<u64>,
    /// Finished sketches whose dimensions are shown.
    pub shown_dims: Vec<u64>,
    /// Bodies locked in the browser (no move or delete).
    pub locked_bodies: Vec<String>,
    /// The agent cursor: shown, speed, follow camera (#35).
    pub agent_cursor: agent_cursor::Settings,
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            tab: "SOLID".into(),
            show_browser: true,
            show_timeline: true,
            show_grid: true,
            show_origin: true,
            perspective: false,
            hidden_bodies: Vec::new(),
            hidden_origin: Vec::new(),
            show_sketches: true,
            palette_open: false,
            dark: true,
            pick_off: Vec::new(),
            pick_bodies: false,
            visual_style: 0,
            ground_shadow: true,
            up_axis: Default::default(),
            hidden_sketches: Vec::new(),
            shown_sketches: Vec::new(),
            hidden_profiles: Vec::new(),
            shown_dims: Vec::new(),
            locked_bodies: Vec::new(),
            agent_cursor: Default::default(),
        }
    }
}

/// Host services (file pickers) injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str, &[&str]) -> Option<String>>>,
    /// Where the host reads the anti-aliasing preference at start (a small JSON file).
    pub graphics_path: Option<String>,
}

pub struct SolveApp {
    pub session: Session,
    pub ui: UiState,
    pub cam: Camera,
    /// A view change in progress (view cube, Home, Fit); any mouse navigation cancels it.
    pub cam_anim: Option<CameraAnim>,
    /// Seconds since start, as of this frame.
    pub now: f64,
    /// The view before the current sketch started (Finish Sketch returns to it).
    pub pre_sketch_cam: Option<Camera>,
    /// The history marker before Edit Sketch rolled the timeline to the sketch; Finish Sketch
    /// restores it.
    pub pre_sketch_marker: Option<Option<usize>>,
    pub services: Services,
    pub viewport: viewport::ViewportState,
    pub tool: Option<tools::Tool>,
    pub dialog: Option<dialogs::Dialog>,
    /// The Change Parameters window, when open.
    pub params: Option<params_dialog::ParamsDialog>,
    pub palette: palette::Palette,
    /// Context menus, rename box and Properties window.
    pub menu: context_menu::MenuState,
    /// Browser tree state (reveal, groups being edited, occurrence moves).
    pub tree: browser::TreeState,
    /// The S box (command search at the cursor): recent and pinned commands.
    pub sbox: shortcut_box::ShortcutBox,
    /// Keyboard shortcuts the user changed.
    pub keymap: keymap::Keymap,
    /// The Help menu and the About window.
    pub help: help::HelpState,
    /// Toolbar buttons the user pinned, removed or reordered.
    pub toolbar_custom: toolbar_custom::ToolbarCustom,
    /// Preferences kept between runs, and their window.
    pub preferences: prefs::Prefs,
    pub prefs_window: prefs::PrefsWindow,
    pub preview: preview::PreviewState,
    /// The last command started interactively (id, label), for Repeat.
    pub last_command: Option<(String, String)>,
    /// Esc was already acted on this frame (by the shortcuts), so later handlers skip it.
    pub esc_handled: bool,
    pub status: Option<(String, f64, bool)>,
    pub quit_requested: bool,
    /// Autosave into the recovery folder (the desktop app turns it on).
    pub autosave: Option<solvecraft_engine::recovery::Autosaver>,
    /// Designs left by crashed apps, offered for recovery.
    pub recoverable: Vec<solvecraft_engine::recovery::Entry>,
    /// The bodies of a just-opened Y-up file, offered to be stood up (`stand_up`).
    pub stand_up: Option<Vec<String>>,
    /// The last command was a big operation: autosave after it.
    pub big_operation: bool,
    /// Autosave interval (minutes; 0 = only after big operations). A preference.
    pub autosave_minutes: f64,
    /// The application bar is the window's title bar (Windows, Linux: no OS decorations).
    pub custom_titlebar: bool,
    /// macOS: the traffic lights sit over the application bar's left end.
    pub integrated_titlebar: bool,
    pub frame_ms: f64,
    pub synthetic: Vec<egui::Event>,
    /// The modifiers of the synthetic press in progress (a Ctrl-drag keeps Ctrl while it moves).
    pub synthetic_mods: egui::Modifiers,
    control_rx: Option<Receiver<ControlRequest>>,
    /// The agent cursor acting out control requests.
    pub agent: agent_cursor::AgentCursor,
    pending_shots: Vec<(u64, Option<String>, std::sync::mpsc::Sender<Value>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    styled: bool,
    fitted: bool,
    /// The start page.
    pub home: home::HomeState,
    /// The other open designs (tabs).
    pub docs: documents::Documents,
}

impl SolveApp {
    pub fn new(session: Session, services: Services) -> Self {
        SolveApp {
            session,
            ui: UiState::default(),
            cam: Camera::default(),
            cam_anim: None,
            now: 0.0,
            pre_sketch_cam: None,
            pre_sketch_marker: None,
            services,
            viewport: viewport::ViewportState::default(),
            tool: None,
            dialog: None,
            params: None,
            palette: palette::Palette::default(),
            menu: context_menu::MenuState::default(),
            tree: browser::TreeState::default(),
            sbox: shortcut_box::ShortcutBox::default(),
            keymap: keymap::Keymap::default(),
            help: help::HelpState::default(),
            toolbar_custom: toolbar_custom::ToolbarCustom::default(),
            preferences: prefs::Prefs::default(),
            prefs_window: prefs::PrefsWindow::default(),
            preview: preview::PreviewState::default(),
            last_command: None,
            esc_handled: false,
            status: None,
            quit_requested: false,
            autosave: None,
            recoverable: Vec::new(),
            stand_up: None,
            big_operation: false,
            autosave_minutes: 5.0,
            custom_titlebar: false,
            integrated_titlebar: false,
            frame_ms: 0.0,
            synthetic: Vec::new(),
            synthetic_mods: egui::Modifiers::default(),
            control_rx: None,
            agent: Default::default(),
            pending_shots: Vec::new(),
            queued_shots: Vec::new(),
            shot_token: 0,
            styled: false,
            fitted: false,
            home: home::HomeState::default(),
            docs: documents::Documents::default(),
        }
    }

    /// Draw the viewport on the GPU (eframe's wgpu render state, with the app's depth and
    /// stencil buffer bits and MSAA sample count). Without it the viewport renders on the CPU.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState, depth_bits: u8, stencil_bits: u8, samples: u32) {
        self.viewport.gpu = Some(gpu::install(rs, depth_bits, stencil_bits, samples));
        let info = rs.adapter.get_info();
        self.help.gpu = Some(format!("{} ({:?}, {:?})", info.name.trim(), info.backend, info.device_type));
    }

    /// Preferences kept between runs (JSON), for the host to store.
    pub fn prefs(&self) -> String {
        json!({
            "dark": self.ui.dark,
            "grid": self.ui.show_grid,
            "perspective": self.ui.perspective,
            "pick_bodies": self.ui.pick_bodies,
            "auto_project": self.session.auto_project,
            "visual_style": self.ui.visual_style,
            "browser_collapsed": self.tree.collapsed,
            "browser_expanded": self.tree.expanded,
            "recent": self.home.recent,
            "shortcut_box": self.sbox.prefs(),
            "shortcuts": self.keymap.prefs(),
            "toolbar": self.toolbar_custom.prefs(),
            "autosave_minutes": self.autosave_minutes,
            "ground_shadow": self.ui.ground_shadow,
            "agent_cursor": self.ui.agent_cursor,
            "preferences": self.preferences,
        })
        .to_string()
    }

    /// Restore preferences saved with [`SolveApp::prefs`].
    pub fn load_prefs(&mut self, prefs: &str) {
        let Ok(v) = serde_json::from_str::<Value>(prefs) else { return };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool);
        if let Some(d) = flag("dark") {
            self.ui.dark = d;
        }
        if let Some(g) = flag("grid") {
            self.ui.show_grid = g;
        }
        if let Some(p) = flag("perspective") {
            self.ui.perspective = p;
        }
        if let Some(b) = flag("pick_bodies") {
            self.ui.pick_bodies = b;
        }
        if let Some(a) = flag("auto_project") {
            self.session.auto_project = a;
        }
        if let Some(m) = v.get("autosave_minutes").and_then(Value::as_f64).filter(|m| m.is_finite()) {
            self.autosave_minutes = m.clamp(0.0, 600.0);
        }
        if let Some(s) = v.get("visual_style").and_then(Value::as_u64) {
            self.ui.visual_style = u8::try_from(s.min(3)).unwrap_or(0);
        }
        // Browser folders the user opened or closed.
        let keys = |k: &str| -> std::collections::BTreeSet<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().take(10_000).filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        self.tree.collapsed = keys("browser_collapsed");
        self.tree.expanded = keys("browser_expanded");
        self.home.recent = v
            .get("recent")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).take(home::MAX_RECENT).map(str::to_string).collect())
            .unwrap_or_default();
        if let Some(s) = v.get("shortcut_box") {
            self.sbox.load(s);
        }
        if let Some(s) = v.get("shortcuts") {
            self.keymap.load(s);
        }
        if let Some(t) = v.get("toolbar") {
            self.toolbar_custom.load(t);
        }
        if let Some(p) = v.get("preferences").and_then(|p| serde_json::from_value::<prefs::Prefs>(p.clone()).ok()) {
            self.preferences = p;
        }
        // Saved before the theme was a preference: the Dark/Light flag says which.
        if v.get("preferences").and_then(|p| p.get("theme")).is_none() {
            self.preferences.theme = if self.ui.dark { "dark" } else { "light" }.into();
        }
        if let Some(g) = flag("ground_shadow") {
            self.ui.ground_shadow = g;
        }
        if let Some(a) = v.get("agent_cursor").and_then(|a| serde_json::from_value(a.clone()).ok()) {
            self.ui.agent_cursor = a;
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Run a command programmatically (JSON parameters, no dialogs).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let t0 = now_ms();
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        self.big_operation |= now_ms() - t0 >= recovery_ui::BIG_OPERATION_MS;
        if let Ok(v) = &r {
            // Curves drawn in construction (or centerline) mode take that line type.
            sketch_palette::apply_mode(self, id, v);
        }
        match &r {
            Ok(_) => {
                self.session.echo(format!("{} done", command_label(id)));
                if matches!(id, "file.new" | "doc.open") {
                    self.cam = Camera::default();
                    self.fit_view();
                }
                if matches!(id, "doc.open" | "file.save" | "file.save_as")
                    && let Some(p) = self.session.path.clone()
                {
                    self.home.add_recent(&p);
                }
            }
            Err(e) => {
                let msg = friendly_error(id, e);
                self.session.echo(msg.clone());
                self.set_status(msg, true);
            }
        }
        if self.session.active_sketch.is_some() && self.ui.tab != "SKETCH" {
            self.ui.tab = "SKETCH".into();
        } else if self.session.active_sketch.is_none() && self.ui.tab == "SKETCH" {
            self.ui.tab = "SOLID".into();
            self.tool = None;
        }
        r
    }

    /// Start a command the way a toolbar click does: interactive tools and dialogs for commands
    /// that have them, otherwise run it with defaults.
    pub fn start(&mut self, id: &str) {
        // A component moved without Capture Position: ask first (Capture, Revert or Cancel).
        if browser::needs_capture(self, id) {
            self.tree.capture_prompt = Some(id.to_string());
            return;
        }
        self.tool = None;
        self.home.open = false;
        if matches!(id, "file.insert_step" | "file.insert_mesh") {
            if let Some(p) = self.services.pick_open.as_ref().and_then(|f| f()) {
                self.insert_path(&p);
            }
            return;
        }
        if !sketch_tools::start_hook(self, id) {
            return;
        }
        if id == "parameters.change" {
            params_dialog::open(self);
            return;
        }
        if id == "sketch.finish" {
            self.finish_sketch();
            return;
        }
        // Press Pull: edges get a fillet, faces and profiles an extrude.
        if id == "solid.press_pull" {
            let edges = self.session.selection.iter().any(|s| matches!(s, solvecraft_engine::Sel::Edge { .. }));
            return self.start(if edges { "solid.fillet" } else { "solid.extrude" });
        }
        // Fix/Unfix: the selected sketch points and curves change at once, otherwise pick them.
        if id == "sketch.constraint.fix" {
            let ents: Vec<String> = self
                .session
                .selection
                .iter()
                .filter_map(|s| match s {
                    solvecraft_engine::Sel::SketchCurve { id } | solvecraft_engine::Sel::SketchPoint { id } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            if !ents.is_empty() {
                let _ = self.run(id, json!({ "entities": ents }));
                return;
            }
        }
        // Construction toggle: selected sketch curves switch at once, otherwise pick them.
        // Construction (X): convert the selection, or switch construction mode; no pick tool.
        if id == "sketch.construction" {
            sketch_palette::toggle_construction(self);
            return;
        }
        if let Some(spec) = solvecraft_engine::find_command(id)
            && (tools::Tool::for_command(id).is_some() || dialogs::Dialog::for_command(self, id).is_some())
            && id != "sketch.create"
        {
            self.last_command = Some((spec.id.to_string(), spec.label.to_string()));
        }
        if let Some(t) = tools::Tool::for_command(id) {
            self.tool = Some(t);
            self.set_status(tools::hint(id), false);
            return;
        }
        if let Some(d) = dialogs::Dialog::for_command(self, id) {
            // The pre-selection now belongs to the dialog's inputs.
            if d.wants_picks() && !self.session.selection.is_empty() {
                let _ = self.session.execute("select.clear", &json!({}));
            }
            self.dialog = Some(d);
            return;
        }
        let _ = self.run(id, json!({}));
    }

    /// Edit a feature the way Fusion does: sketches open in sketch mode; other features roll the
    /// timeline back to just before themselves and reopen their dialog, filled in.
    pub fn edit_feature(&mut self, id: u64) {
        use solvecraft_engine::doc::FeatureKind;
        let Some(f) = self.session.doc.feature(id) else { return };
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            self.edit_sketch(id);
            return;
        }
        if self.session.active_sketch.is_some() {
            self.finish_sketch();
        }
        let Some(idx) = self.session.doc.feature_index(id) else { return };
        let marker = self.session.doc.marker;
        self.tool = None;
        if self.run("timeline.roll_to", json!({ "position": idx })).is_err() {
            return;
        }
        match dialogs::for_feature(self, id, marker) {
            Some(d) => self.dialog = Some(d),
            None => {
                let _ = self.run("timeline.roll_to", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
                self.palette.text = format!("timeline.edit {{\"feature\": {id}, \"set\": {{}}}}");
                self.ui.palette_open = true;
            }
        }
    }

    /// Edit a sketch the way Fusion does: roll the timeline back to just after it (so later
    /// features don't hide it), turn the camera to face it, and remember both so Finish Sketch
    /// puts them back. Editing the sketch that is already open does nothing; editing another one
    /// finishes the current sketch first, so edit sessions never nest.
    pub fn edit_sketch(&mut self, id: u64) {
        if self.session.active_sketch == Some(id) {
            return;
        }
        if self.session.active_sketch.is_some() {
            self.finish_sketch();
        }
        self.tool = None;
        self.dialog = None;
        let Some(idx) = self.session.doc.feature_index(id) else { return };
        let marker = self.session.doc.marker;
        let rolled = marker.is_none_or(|m| m > idx + 1) && self.run("timeline.roll_to", json!({ "position": idx + 1 })).is_ok();
        let before = self.cam;
        if self.run("sketch.edit", json!({ "sketch": id })).is_err() {
            if rolled {
                self.restore_marker(marker);
            }
            return;
        }
        self.pre_sketch_marker = rolled.then_some(marker);
        self.pre_sketch_cam = Some(before);
        dialogs::look_at_sketch(self);
    }

    /// Finish the active sketch: stop editing, restore the history marker Edit Sketch moved and
    /// return to the view from before the sketch.
    pub fn finish_sketch(&mut self) {
        self.tool = None;
        let _ = self.run("sketch.finish", json!({}));
        if let Some(marker) = self.pre_sketch_marker.take() {
            self.restore_marker(marker);
        }
        if let Some(c) = self.pre_sketch_cam.take() {
            self.animate_to(c);
        }
    }

    fn restore_marker(&mut self, marker: Option<usize>) {
        let _ = self.run("timeline.roll_to", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
    }

    /// Everything shown as selected: the selection plus the open dialog's inputs.
    pub fn highlighted(&self) -> Vec<solvecraft_engine::Sel> {
        let mut v = self.session.selection.clone();
        if let Some(d) = &self.dialog {
            v.extend(d.items());
        }
        v
    }

    /// Is the origin widget shown (always while picking a sketch plane)?
    /// Origin planes and axes are drawn and pickable. Hidden ones only come back while Create
    /// Sketch has nothing else to offer (no body shown).
    pub fn origin_visible(&self) -> bool {
        self.ui.show_origin
            || (self.dialog.as_ref().is_some_and(|d| matches!(d.kind, dialogs::Kind::Sketch))
                && !self.session.world_state().bodies.iter().any(|b| !self.ui.hidden_bodies.contains(&b.name)))
    }

    pub fn set_status(&mut self, s: impl Into<String>, error: bool) {
        self.status = Some((s.into(), now_ms(), error));
    }

    pub fn open_path(&mut self, path: &str) {
        self.home.open = false;
        documents::prepare_open(self);
        if self.run("doc.open", json!({ "path": path })).is_ok() {
            self.fit_view();
            stand_up::offer(self, path, &[]);
        }
    }

    /// Insert a STEP file's bodies (an Import base feature) or a 3MF/STL file's meshes into the
    /// current design.
    pub fn insert_path(&mut self, path: &str) {
        let cmd = if solvecraft_engine::io::is_mesh_path(path) { "file.insert_mesh" } else { "file.insert_step" };
        let before: Vec<String> = self.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        if let Ok(r) = self.run(cmd, json!({ "path": path })) {
            self.fit_view();
            stand_up::offer(self, path, &before);
            if let Some(w) = r["warnings"].as_array().filter(|w| !w.is_empty()) {
                self.set_status(format!("imported with {} warning(s): {}", w.len(), w.first().and_then(Value::as_str).unwrap_or("")), false);
            }
        }
    }

    /// A dropped file: a STEP, 3MF or STL file joins a design that has features, anything else
    /// opens.
    pub fn drop_path(&mut self, path: &str) {
        let import = solvecraft_engine::io::is_step_path(path) || solvecraft_engine::io::is_mesh_path(path);
        if import && !self.session.doc.features.is_empty() {
            self.insert_path(path);
        } else {
            self.open_path(path);
        }
    }

    /// Frame the model at once (programmatic use; the toolbar animates).
    /// Make `up` the world's up axis: the camera turns to the same standard view in that world
    /// (home) and frames the model.
    pub fn set_up_axis(&mut self, up: solvecraft_engine::render::UpAxis) {
        self.ui.up_axis = up;
        self.cam_anim = None;
        self.cam.up = up;
        self.cam.set_view(StandardView::Iso);
        self.fit_view();
    }

    pub fn fit_view(&mut self) {
        self.cam_anim = None;
        self.cam = self.fitted(self.cam);
    }

    /// `cam` moved to frame the whole model.
    pub fn fitted(&self, mut cam: Camera) -> Camera {
        let b = solvecraft_engine::view::bounds(&self.session);
        if b.is_empty() {
            cam.target = solvecraft_engine::geom::Vec3::ZERO;
            cam.distance = 150.0;
        } else {
            cam.fit(&b);
        }
        cam
    }

    /// Move the camera to `to` over half a second (ease in-out, orientation slerped).
    pub fn animate_to(&mut self, to: Camera) {
        if !to.is_valid() {
            return;
        }
        // Preferences › Navigation: animated (at a chosen speed) or immediate.
        if !self.preferences.look_at_anim {
            self.cam = to;
            self.cam_anim = None;
            return;
        }
        let mut anim = CameraAnim::new(self.cam, to, self.now);
        let speed = f64::from(self.preferences.look_at_speed);
        if speed.is_finite() && speed > 0.0 {
            anim.duration /= speed;
        }
        self.cam_anim = Some(anim);
    }

    /// Animated view changes for the view cube and navigation bar.
    pub fn animate_view(&mut self, v: &str) {
        let mut to = self.cam;
        match v {
            // With something selected, Fit frames the selection.
            "fit" => match viewport::selection_bounds(self) {
                Some(b) => to.fit(&b),
                None => to = self.fitted(to),
            },
            "home" => {
                to.set_view(StandardView::Iso);
                to = self.fitted(to);
            }
            v => match StandardView::parse(v) {
                Some(sv) => to.set_view(sv),
                None => return,
            },
        }
        self.animate_to(to);
    }

    /// Stop any view animation where it is (the user took over the camera).
    pub fn cancel_view_animation(&mut self) {
        self.cam_anim = None;
    }

    fn step_view_animation(&mut self, ctx: &egui::Context) {
        let Some(anim) = self.cam_anim else { return };
        let (cam, done) = anim.sample(self.now);
        self.cam = if cam.is_valid() { cam } else { anim.to };
        if done {
            self.cam_anim = None;
        } else {
            ctx.request_repaint();
        }
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.esc_handled = false;
        prefs::apply(self, ctx);
        if !self.styled || theme::is_dark() != self.ui.dark {
            theme::set_dark(self.ui.dark);
            theme::apply(ctx);
            self.styled = true;
        }
        if !self.fitted {
            self.fit_view();
            self.fitted = true;
        }
        self.now = ctx.input(|i| i.time);
        if self.cam.up != self.ui.up_axis {
            self.set_up_axis(self.ui.up_axis);
        }
        self.step_view_animation(ctx);
        self.drain_control(ctx);
        agent_cursor::step(self, ctx);
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        toolbar::shortcuts(self, ctx);
        recovery_ui::tick(self);
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                self.drop_path(&p);
            }
        }
    }

    /// Inject synthetic events (one pointer event per frame).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        if self.synthetic.is_empty() {
            return;
        }
        let n = match self.synthetic.first() {
            Some(egui::Event::PointerMoved(_) | egui::Event::PointerButton { .. } | egui::Event::MouseWheel { .. }) => 1,
            _ => self.synthetic.iter().position(|e| matches!(e, egui::Event::Key { pressed: false, .. })).map_or(self.synthetic.len(), |i| i + 1),
        };
        if let Some(egui::Event::PointerMoved(p) | egui::Event::PointerButton { pos: p, .. }) = self.synthetic.first() {
            raw.events.push(egui::Event::PointerMoved(*p));
        }
        // egui keeps the modifier keys as input state, changed by their own event.
        match self.synthetic.first() {
            Some(egui::Event::PointerButton { modifiers, pressed, .. }) => {
                // A drag keeps its press's modifiers until the release.
                self.synthetic_mods = if *pressed { *modifiers } else { egui::Modifiers::default() };
                raw.events.push(egui::Event::ModifiersChanged(*modifiers))
            }
            Some(egui::Event::Key { modifiers, .. }) => raw.events.push(egui::Event::ModifiersChanged(*modifiers)),
            Some(egui::Event::PointerMoved(_)) => raw.events.push(egui::Event::ModifiersChanged(self.synthetic_mods)),
            _ => {}
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t0 = now_ms();
        scenario::clear_handles();
        toolbar::app_bar(self, ui);
        documents::prompt(self, ui.ctx());
        if self.home.open {
            home::show(self, ui);
            if self.home.open {
                self.frame_ms = now_ms() - t0;
                return;
            }
        }
        toolbar::toolbar(self, ui);
        if self.ui.show_timeline {
            timeline::timeline(self, ui);
        }
        palette::palette_bar(self, ui);
        if self.ui.show_browser {
            browser::browser(self, ui);
        }
        preview::update(self, ui.ctx());
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            viewport::show(self, ui);
        });
        dialogs::show(self, ui.ctx());
        sketch_palette::show(self, ui.ctx());
        params_dialog::show(self, ui.ctx());
        recovery_ui::show(self, ui.ctx());
        stand_up::show(self, ui.ctx());
        context_menu::show(self, ui.ctx());
        delete::show(self, ui.ctx());
        if self.custom_titlebar {
            titlebar::resize_zones(ui);
        }
        shortcut_box::show(self, ui.ctx());
        prefs::show(self, ui.ctx());
        agent_cursor::paint(self, ui.ctx());
        self.frame_ms = now_ms() - t0;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        // While the agent cursor acts out a request, later ones wait their turn.
        while !self.agent.busy() {
            let Ok(req) = rx.try_recv() else { break };
            let reply = req.reply.clone();
            if agent_cursor::intercept(self, &req.method, &req.params, &reply) {
                ctx.request_repaint();
                continue;
            }
            match control::handle(self, ctx, &req) {
                control::Outcome::Done(v) => {
                    let _ = reply.send(v);
                }
                control::Outcome::Screenshot { path } => {
                    self.shot_token += 1;
                    let token = self.shot_token;
                    self.queued_shots.push((token, now_ms() + 150.0, 0));
                    self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
                }
            }
            ctx.request_repaint();
        }
        self.control_rx = Some(rx);
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        if self.pending_shots.is_empty() {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                let _ = reply.send(control::save_screenshot(&image, path.as_deref()));
            }
        }
        let now = now_ms();
        self.pending_shots.retain(|(_, _, reply, deadline)| {
            if now < *deadline {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (window hidden?); use ui.render"}));
            false
        });
    }
}

/// A command's name as people see it (its label), for the status bar.
pub fn command_label(id: &str) -> &str {
    solvecraft_engine::find_command(id).map_or(id, |c| c.label)
}

/// An engine error with the command's label instead of its id: "Shell: offset must be …".
pub fn friendly_error(id: &str, e: &str) -> String {
    let label = command_label(id);
    let msg = e.replace(&format!("invalid parameters for `{id}`: "), "").replace(&format!("{id}: "), "").replace(&format!("`{id}`"), label);
    if msg.starts_with(label) { msg } else { format!("{label}: {msg}") }
}

pub fn now_ms() -> f64 {
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}
