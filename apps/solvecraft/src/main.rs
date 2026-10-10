//! SolveCraft desktop app.
//!
//! Usage: `solvecraft [--control <port>] [--sample] [design.solvecraft | part.step]`
//!
//! A `.step`/`.stp` file opens as a new design holding the file's bodies (an Import feature); a
//! `.3mf`/`.stl` file as a new design holding mesh bodies.
//!
//! `--control <port>` (or `SOLVECRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"engine.execute","params":{"command":"solid.box","params":{…}}}`.
//! See `solvecraft_ui_egui::control` and docs/control-protocol.md.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod control_server;

use solvecraft_engine::Session;
use solvecraft_ui_egui::{Services, SolveApp};

struct App(SolveApp);

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The OS closing the window (Alt+F4, Cmd+Q, the window manager): ask about unsaved
        // designs first; the window closes once they are answered (`quit_requested`).
        if ctx.input(|i| i.viewport().close_requested()) && !self.0.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            solvecraft_ui_egui::documents::request_quit(&mut self.0);
        }
        self.0.logic(ctx);
        if self.0.quit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.0.raw_input_hook(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.0.ui(ui);
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string(PREFS_KEY, self.0.prefs());
    }
    fn on_exit(&mut self) {
        solvecraft_ui_egui::recovery_ui::close(&mut self.0);
    }
}

/// Where the preferences (theme) are kept in eframe's storage.
const PREFS_KEY: &str = "solvecraft.prefs";

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter(
                    "Designs, STEP, IGES and meshes",
                    &["solvecraft", "step", "stp", "STEP", "STP", "igs", "iges", "IGS", "IGES", "3mf", "3MF", "stl", "STL"],
                )
                .add_filter("SolveCraft design", &["solvecraft"])
                .add_filter("STEP", &["step", "stp", "STEP", "STP"])
                .add_filter("IGES", &["igs", "iges", "IGS", "IGES"])
                .add_filter("Mesh (3MF, STL)", &["3mf", "3MF", "stl", "STL"])
                .add_filter("All files", &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str, exts: &[&str]| {
            rfd::FileDialog::new().set_file_name(name).add_filter("File", exts).save_file().map(|p| p.to_string_lossy().to_string())
        })),
        graphics_path: graphics_path().map(|p| p.to_string_lossy().to_string()),
    }
}

/// The anti-aliasing preference, kept next to eframe's storage (it is needed before the
/// window opens).
fn graphics_path() -> Option<std::path::PathBuf> {
    eframe::storage_dir("SolveCraft").map(|d| d.join("graphics.json"))
}

/// Anti-aliasing samples from Preferences (4 when unset).
fn msaa() -> u16 {
    graphics_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("msaa").and_then(serde_json::Value::as_u64))
        .and_then(|m| u16::try_from(m).ok())
        .filter(|m| [1, 2, 4, 8].contains(m))
        .unwrap_or(MSAA)
}

/// Windows and Linux: no OS title bar; the application bar is the title bar
/// (`solvecraft_ui_egui::titlebar`). macOS keeps its traffic lights over the integrated bar.
const CUSTOM_TITLEBAR: bool = !cfg!(target_os = "macos");

const DEPTH_BITS: u8 = 24;
/// The section cap marks where it shows in the stencil.
const STENCIL_BITS: u8 = 8;
const MSAA: u16 = 4;

fn main() -> eframe::Result {
    let samples = msaa();
    let mut control_port: Option<u16> = std::env::var("SOLVECRAFT_CONTROL_PORT").ok().and_then(|p| p.parse().ok());
    let mut files = Vec::new();
    let mut sample = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--control" => control_port = args.next().and_then(|p| p.parse().ok()),
            "--sample" => sample = true,
            "--version" | "-V" => {
                println!("solvecraft {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("usage: solvecraft [--control PORT] [--sample] [design.solvecraft | part.step]");
                return Ok(());
            }
            _ => files.push(a),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SolveCraft")
            .with_inner_size([1600.0, 1000.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true)
            .with_decorations(!CUSTOM_TITLEBAR)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
            .with_app_id("ai.storyteller.solvecraft"),
        depth_buffer: DEPTH_BITS,
        stencil_buffer: STENCIL_BITS,
        multisampling: samples,
        ..Default::default()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut options = options;
    #[cfg(all(unix, not(target_os = "macos")))]
    // winit has no file drag-and-drop on Wayland (only on X11), so dropping files from the file
    // manager showed a "no" cursor. Run through XWayland when it's there; SOLVECRAFT_WAYLAND=1
    // keeps the native Wayland backend.
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("SOLVECRAFT_WAYLAND").is_none() {
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    eframe::run_native(
        "SolveCraft",
        options,
        Box::new(move |cc| {
            let mut app = SolveApp::new(Session::default(), services());
            app.custom_titlebar = CUSTOM_TITLEBAR;
            app.integrated_titlebar = cfg!(target_os = "macos");
            if let Some(p) = cc.storage.and_then(|s| s.get_string(PREFS_KEY)) {
                app.load_prefs(&p);
            }
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs, DEPTH_BITS, STENCIL_BITS, u32::from(samples));
            }
            if let Some(port) = control_port {
                let rx = control_server::start(port, cc.egui_ctx.clone());
                app = app.with_control(rx);
            }
            if sample {
                if let Err(e) = app.session.run_script(&solvecraft_engine::sample::script()) {
                    app.set_status(format!("sample: {e}"), true);
                }
                app.session.doc_mut().name = "Sample Plate".into();
                app.session.mark_saved();
                app.session.undo.clear();
            }
            for f in &files {
                app.open_path(f);
            }
            // No design given: the start page (not when driven over the control channel, whose
            // clients expect the design view; `ui.home` shows it there).
            if files.is_empty() && !sample && control_port.is_none() {
                let rev = app.session.revision;
                app.home.show(rev);
            }
            // Autosave, and offer what crashed sessions left behind.
            if let Some(dir) = solvecraft_engine::recovery::default_dir() {
                let every = if app.autosave_minutes > 0.0 { app.autosave_minutes * 60.0 } else { f64::MAX / 4.0 };
                solvecraft_ui_egui::recovery_ui::start(&mut app, &dir, std::time::Duration::from_secs_f64(every.min(1e9)));
            }
            Ok(Box::new(App(app)))
        }),
    )
}
