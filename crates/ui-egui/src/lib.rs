//! The SolveCraft egui front end.
//!
//! A thin shell over [`solvecraft_engine::Session`]: panels read engine state and act through
//! commands ([`SolveApp::run`]). Nothing here owns design data. The layout follows the familiar
//! parametric-CAD desktop: application bar, a toolbar of workspace tabs and panels, the browser
//! on the left, the 3D viewport with a view cube and navigation bar, the command dialog on the
//! right of the viewport, the command palette and the timeline at the bottom.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod browser;
pub mod control;
pub mod dialogs;
pub mod gpu;
pub mod icons;
pub mod palette;
pub mod selection;
pub mod theme;
pub mod timeline;
pub mod toolbar;
pub mod tools;
pub mod viewport;

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
        }
    }
}

/// Host services (file pickers) injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    pub pick_open: Option<Box<dyn Fn() -> Option<String>>>,
    pub pick_save: Option<Box<dyn Fn(&str, &[&str]) -> Option<String>>>,
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
    pub services: Services,
    pub viewport: viewport::ViewportState,
    pub tool: Option<tools::Tool>,
    pub dialog: Option<dialogs::Dialog>,
    pub palette: palette::Palette,
    pub status: Option<(String, f64, bool)>,
    pub quit_requested: bool,
    pub frame_ms: f64,
    pub synthetic: Vec<egui::Event>,
    control_rx: Option<Receiver<ControlRequest>>,
    pending_shots: Vec<(u64, Option<String>, std::sync::mpsc::Sender<Value>, f64)>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    styled: bool,
    fitted: bool,
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
            services,
            viewport: viewport::ViewportState::default(),
            tool: None,
            dialog: None,
            palette: palette::Palette::default(),
            status: None,
            quit_requested: false,
            frame_ms: 0.0,
            synthetic: Vec::new(),
            control_rx: None,
            pending_shots: Vec::new(),
            queued_shots: Vec::new(),
            shot_token: 0,
            styled: false,
            fitted: false,
        }
    }

    /// Draw the viewport on the GPU (eframe's wgpu render state, with the app's depth buffer
    /// bits and MSAA sample count). Without it the viewport renders on the CPU.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState, depth_bits: u8, samples: u32) {
        self.viewport.gpu = Some(gpu::install(rs, depth_bits, samples));
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Run a command programmatically (JSON parameters, no dialogs).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        let r = self.session.execute(id, &params).map_err(|e| e.to_string());
        match &r {
            Ok(_) => {
                self.session.echo(format!("{id}: done"));
                if matches!(id, "NewDocumentCommand" | "doc.open") {
                    self.cam = Camera::default();
                    self.fit_view();
                }
            }
            Err(e) => {
                self.session.echo(format!("{id}: {e}"));
                self.set_status(e.clone(), true);
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
        self.tool = None;
        if id == "SketchStop" {
            let r = self.run(id, json!({}));
            if r.is_ok()
                && let Some(c) = self.pre_sketch_cam.take()
            {
                self.animate_to(c);
            }
            return;
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
            let _ = self.run("SketchActivate", json!({ "sketch": id }));
            return;
        }
        let Some(idx) = self.session.doc.feature_index(id) else { return };
        let marker = self.session.doc.marker;
        self.tool = None;
        if self.run("timeline.rollTo", json!({ "position": idx })).is_err() {
            return;
        }
        match dialogs::for_feature(self, id, marker) {
            Some(d) => self.dialog = Some(d),
            None => {
                let _ = self.run("timeline.rollTo", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
                self.palette.text = format!("timeline.edit {{\"feature\": {id}, \"set\": {{}}}}");
                self.ui.palette_open = true;
            }
        }
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
    pub fn origin_visible(&self) -> bool {
        self.ui.show_origin || self.dialog.as_ref().is_some_and(|d| matches!(d.kind, dialogs::Kind::Sketch))
    }

    pub fn set_status(&mut self, s: impl Into<String>, error: bool) {
        self.status = Some((s.into(), now_ms(), error));
    }

    pub fn open_path(&mut self, path: &str) {
        if self.run("doc.open", json!({ "path": path })).is_ok() {
            self.fit_view();
        }
    }

    /// Frame the model at once (programmatic use; the toolbar animates).
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
        self.cam_anim = Some(CameraAnim::new(self.cam, to, self.now));
    }

    /// Animated view changes for the view cube and navigation bar.
    pub fn animate_view(&mut self, v: &str) {
        let mut to = self.cam;
        match v {
            "fit" => to = self.fitted(to),
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
        if !self.styled {
            theme::apply(ctx);
            self.styled = true;
        }
        if !self.fitted {
            self.fit_view();
            self.fitted = true;
        }
        self.now = ctx.input(|i| i.time);
        self.step_view_animation(ctx);
        self.drain_control(ctx);
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        toolbar::shortcuts(self, ctx);
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let p = f.path().to_string_lossy().to_string();
            if !p.is_empty() {
                self.open_path(&p);
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
            Some(egui::Event::PointerButton { modifiers, .. } | egui::Event::Key { modifiers, .. }) => {
                raw.events.push(egui::Event::ModifiersChanged(*modifiers))
            }
            Some(egui::Event::PointerMoved(_)) => raw.events.push(egui::Event::ModifiersChanged(egui::Modifiers::default())),
            _ => {}
        }
        let n = n.min(self.synthetic.len());
        raw.events.extend(self.synthetic.drain(..n));
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let t0 = now_ms();
        toolbar::app_bar(self, ui);
        toolbar::toolbar(self, ui);
        if self.ui.show_timeline {
            timeline::timeline(self, ui);
        }
        palette::palette_bar(self, ui);
        if self.ui.show_browser {
            browser::browser(self, ui);
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            viewport::show(self, ui);
        });
        dialogs::show(self, ui.ctx());
        palette::popup(self, ui.ctx());
        self.frame_ms = now_ms() - t0;
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.control_rx.take() else { return };
        while let Ok(req) = rx.try_recv() {
            let reply = req.reply.clone();
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

pub fn now_ms() -> f64 {
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}
