//! SolveCraft in the browser.
//!
//! Runs the same [`solvecraft_ui_egui::SolveApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?sample` opens the
//! sample part.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod files;

#[cfg(target_arch = "wasm32")]
mod web {
    use std::rc::Rc;

    use solvecraft_engine::Session;
    use solvecraft_engine::io::vfs;
    use solvecraft_ui_egui::{Services, SolveApp};
    use wasm_bindgen::JsCast as _;

    use crate::files::{self, Files, Intent};

    const CANVAS_ID: &str = "solvecraft_canvas";
    const LOADING_ID: &str = "solvecraft_loading";
    const DEPTH_BITS: u8 = 24;

    /// Seconds between autosaves of unsaved work.
    const AUTOSAVE_S: f64 = 30.0;

    struct Shell {
        app: SolveApp,
        files: Rc<Files>,
        /// When the last autosave was (seconds), and the revision it holds (none stored: `None`).
        autosaved_at: f64,
        autosaved: Option<u64>,
        /// A stored design being renamed: (its name, the new name).
        renaming: Option<(String, String)>,
        /// The rename field takes the keyboard when it appears.
        rename_focus: bool,
        /// The start page's recent designs were filled from the stored ones.
        recents_synced: bool,
    }

    impl Shell {
        /// Files from the computer, writes to store, messages and the autosave.
        fn files_tick(&mut self, ctx: &egui::Context) {
            if self.files.ctx.borrow().is_none() {
                *self.files.ctx.borrow_mut() = Some(ctx.clone());
            }
            files::flush(&self.files);
            for (text, error) in self.files.messages.borrow_mut().drain(..) {
                self.app.set_status(text, error);
            }
            let incoming: Vec<_> = self.files.incoming.borrow_mut().drain(..).collect();
            for (name, bytes, intent) in incoming {
                let design = name.ends_with(&format!(".{}", solvecraft_engine::io::DESIGN_EXT));
                let path = if design && intent == Intent::Open {
                    // A design from the computer is kept in the browser (a copy), so Save works.
                    let path = files::free_name(&name);
                    if let Err(e) = vfs::write(&path, &bytes) {
                        self.app.set_status(e.to_string(), true);
                    }
                    files::flush(&self.files);
                    path
                } else {
                    let path = format!("{}{name}", files::UPLOAD);
                    vfs::insert(&path, bytes);
                    path
                };
                match intent {
                    Intent::Open => self.app.open_path(&path),
                    Intent::Insert => self.app.drop_path(&path),
                }
                self.files.panel.set(false);
            }
            if self.files.ready.get() && !self.recents_synced {
                self.recents_synced = true;
                self.sync_recents();
            }
            let now = ctx.input(|i| i.time);
            // Unsaved work is kept (at most every AUTOSAVE_S); saved work needs no autosave.
            if self.files.ready.get() && self.files.recovery.borrow().is_none() {
                let s = &self.app.session;
                let changed = s.is_dirty() && self.autosaved != Some(s.revision);
                let wait = AUTOSAVE_S - (now - self.autosaved_at);
                if changed && wait <= 0.0 {
                    files::autosave(&self.files, Some((&s.doc, s.path.as_deref())));
                    self.autosaved = Some(s.revision);
                    self.autosaved_at = now;
                } else if changed {
                    // Come back for it even when nothing else happens.
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
                } else if !s.is_dirty() && self.autosaved.is_some() {
                    files::autosave(&self.files, None);
                    self.autosaved = None;
                }
            }
            if !self.files.ready.get() || !self.files.incoming.borrow().is_empty() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }

        /// The start page's recent designs are the stored ones: the remembered order first (those
        /// still stored), then the others, newest first.
        fn sync_recents(&mut self) {
            let mut entries = self.files.entries.borrow().clone();
            entries.sort_by(|a, b| b.modified.total_cmp(&a.modified));
            let recent = &mut self.app.home.recent;
            recent.retain(|p| vfs::exists(p));
            for e in entries.iter().filter(|e| e.name.ends_with(&format!(".{}", solvecraft_engine::io::DESIGN_EXT))) {
                let p = format!("{}{}", files::OPFS, e.name);
                if !recent.contains(&p) {
                    recent.push(p);
                }
            }
            recent.truncate(solvecraft_ui_egui::home::MAX_RECENT);
        }

        /// The stored designs: open, insert, rename, delete, download; open from the computer.
        fn file_panel(&mut self, ctx: &egui::Context) {
            if !self.files.panel.get() {
                return;
            }
            let mut open = true;
            let mut act: Option<(&'static str, String)> = None;
            egui::Window::new("Files in this browser")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center())
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Open from computer…").clicked() {
                            files::pick_from_disk(&self.files, Intent::Open);
                        }
                        if ui.button("Insert from computer…").on_hover_text("STEP, 3MF or STL into the open design").clicked() {
                            files::pick_from_disk(&self.files, Intent::Insert);
                        }
                        if ui.button("Download open design").clicked() {
                            act = Some(("download_open", String::new()));
                        }
                    });
                    ui.separator();
                    if !self.files.ready.get() {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Reading the browser's storage…");
                        });
                        return;
                    }
                    let entries = self.files.entries.borrow().clone();
                    if entries.is_empty() {
                        ui.label("Designs you save appear here. They stay in this browser.");
                    }
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        egui::Grid::new("opfs_files").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                            for e in &entries {
                                match &mut self.renaming {
                                    Some((from, to)) if *from == e.name => {
                                        let r = ui.add(egui::TextEdit::singleline(to).desired_width(200.0));
                                        if std::mem::take(&mut self.rename_focus) {
                                            r.request_focus();
                                        }
                                        if r.lost_focus() {
                                            act = Some(if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                                ("rename", to.clone())
                                            } else {
                                                ("rename_cancel", String::new())
                                            });
                                        }
                                    }
                                    _ => {
                                        ui.label(&e.name);
                                    }
                                }
                                ui.label(format!("{:.1} KB", e.size as f64 / 1024.0));
                                ui.horizontal(|ui| {
                                    if ui.small_button("Open").clicked() {
                                        act = Some(("open", e.name.clone()));
                                    }
                                    let design = e.name.ends_with(&format!(".{}", solvecraft_engine::io::DESIGN_EXT));
                                    if !design && ui.small_button("Insert").on_hover_text("Into the open design (STEP, 3MF, STL)").clicked() {
                                        act = Some(("insert", e.name.clone()));
                                    }
                                    if ui.small_button("Rename").clicked() {
                                        self.renaming = Some((e.name.clone(), e.name.clone()));
                                        self.rename_focus = true;
                                    }
                                    if ui.small_button("Download").clicked() {
                                        act = Some(("download", e.name.clone()));
                                    }
                                    if ui.small_button("Delete").clicked() {
                                        act = Some(("delete", e.name.clone()));
                                    }
                                });
                                ui.end_row();
                            }
                        });
                    });
                });
            if !open {
                self.files.panel.set(false);
                self.renaming = None;
            }
            let Some((what, name)) = act else { return };
            let path = format!("{}{name}", files::OPFS);
            match what {
                "open" => {
                    self.app.open_path(&path);
                    self.files.panel.set(false);
                }
                "insert" => {
                    self.app.insert_path(&path);
                    self.files.panel.set(false);
                }
                "download" => match vfs::read(&path) {
                    Ok(b) => {
                        if let Err(e) = files::download(&name, &b) {
                            self.app.set_status(e, true);
                        }
                    }
                    Err(e) => self.app.set_status(e.to_string(), true),
                },
                "download_open" => {
                    let d = &self.app.session.doc;
                    let bytes = solvecraft_engine::io::write_design(d);
                    if let Err(e) = files::download(&format!("{}.{}", d.name, solvecraft_engine::io::DESIGN_EXT), &bytes) {
                        self.app.set_status(e, true);
                    }
                }
                "delete" => {
                    files::delete(&self.files, &name);
                    self.app.home.recent.retain(|r| *r != path);
                }
                "rename_cancel" => self.renaming = None,
                "rename" => {
                    if let Some((from, _)) = self.renaming.take() {
                        match files::rename(&self.files, &from, &name) {
                            Ok(()) => {
                                // Recent designs follow the new name.
                                let (old, new) = (format!("{}{from}", files::OPFS), format!("{}{name}", files::OPFS));
                                for r in &mut self.app.home.recent {
                                    if *r == old {
                                        *r = new.clone();
                                    }
                                }
                            }
                            Err(e) => self.app.set_status(e, true),
                        }
                    }
                }
                _ => {}
            }
        }

        /// Offer the autosave an earlier session left.
        fn recovery_prompt(&mut self, ctx: &egui::Context) {
            let Some((name, path)) = self.files.recovery.borrow().clone() else { return };
            let mut answer = None;
            egui::Window::new("Recover unsaved work?").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(
                ctx,
                |ui| {
                    ui.label(format!("“{name}” had unsaved changes when this page last closed."));
                    ui.horizontal(|ui| {
                        if ui.button("Recover").clicked() {
                            answer = Some(true);
                        }
                        if ui.button("Discard").clicked() {
                            answer = Some(false);
                        }
                    });
                },
            );
            let Some(recover) = answer else { return };
            *self.files.recovery.borrow_mut() = None;
            if recover {
                self.app.open_path(&files::recovery_path());
                // It belongs to its own file again (saving goes there); it is unsaved.
                self.app.session.path = path;
                self.app.session.doc_mut().name = name;
                self.app.session.mark_unsaved();
                self.app.home.open = false;
            }
            files::autosave(&self.files, None);
        }
    }

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.app.logic(ctx);
            self.files_tick(ctx);
        }
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
            files::take_drops(&self.files, raw);
            self.app.raw_input_hook(raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.app.ui(ui);
            let ctx = ui.ctx().clone();
            self.file_panel(&ctx);
            self.recovery_prompt(&ctx);
        }
        fn save(&mut self, storage: &mut dyn eframe::Storage) {
            storage.set_string(PREFS_KEY, self.app.prefs());
        }
    }

    /// Where the preferences (theme) are kept (the browser's local storage).
    const PREFS_KEY: &str = "solvecraft.prefs";

    fn query() -> String {
        web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
    }

    pub fn start() {
        eframe::WebLogger::init(log::LevelFilter::Info).ok();
        wasm_bindgen_futures::spawn_local(async {
            let Some(document) = web_sys::window().and_then(|w| w.document()) else { return };
            let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
                log::error!("missing <canvas id=\"{CANVAS_ID}\">");
                return;
            };
            let mut options = eframe::WebOptions { depth_buffer: DEPTH_BITS, ..Default::default() };
            if query().contains("webgl")
                && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
            {
                create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
            }
            let result = eframe::WebRunner::new()
                .start(
                    canvas,
                    options,
                    Box::new(move |cc| {
                        let files = Rc::new(Files::default());
                        files::start(&files);
                        let panel = files.clone();
                        let services = Services {
                            // Open shows the browser's files (and the computer's, from there).
                            pick_open: Some(Box::new(move || {
                                panel.panel.set(true);
                                None
                            })),
                            // Designs are kept in the browser; everything else is downloaded.
                            pick_save: Some(Box::new(|name: &str, exts: &[&str]| {
                                let design = exts.contains(&solvecraft_engine::io::DESIGN_EXT);
                                Some(format!("{}{name}", if design { files::OPFS } else { files::DOWNLOADS }))
                            })),
                            ..Default::default()
                        };
                        let mut app = SolveApp::new(Session::default(), services);
                        if let Some(p) = cc.storage.and_then(|s| s.get_string(PREFS_KEY)) {
                            app.load_prefs(&p);
                        }
                        if let Some(rs) = &cc.wgpu_render_state {
                            app.set_wgpu(rs, DEPTH_BITS, 1);
                        }
                        if query().contains("sample") {
                            if let Err(e) = app.session.run_script(&solvecraft_engine::sample::script()) {
                                app.set_status(format!("sample: {e}"), true);
                            }
                            app.session.doc_mut().name = "Sample Plate".into();
                            app.session.mark_saved();
                        }
                        if !query().contains("sample") {
                            let rev = app.session.revision;
                            app.home.show(rev);
                        }
                        Ok(Box::new(Shell {
                            app,
                            files,
                            autosaved_at: f64::NEG_INFINITY,
                            autosaved: None,
                            renaming: None,
                            rename_focus: false,
                            recents_synced: false,
                        }))
                    }),
                )
                .await;
            if let Some(el) = document.get_element_by_id(LOADING_ID) {
                match result {
                    Ok(()) => el.remove(),
                    Err(e) => {
                        el.set_inner_html(&format!("<p>SolveCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>"))
                    }
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("solvecraft-web only runs in the browser: build it with `trunk build --release` in apps/solvecraft-web");
}
