//! SolveCraft desktop app.
//!
//! Usage: `solvecraft [--control <port>] [--sample] [design.solvecraft | part.step]`
//!
//! A `.step`/`.stp` file opens as a new design holding the file's bodies (an Import feature).
//!
//! `--control <port>` (or `SOLVECRAFT_CONTROL_PORT`) starts a localhost JSON-lines control
//! server: `{"id":1,"method":"engine.execute","params":{"command":"PrimitiveBox","params":{…}}}`.
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
}

fn services() -> Services {
    Services {
        pick_open: Some(Box::new(|| {
            rfd::FileDialog::new()
                .add_filter("Designs and STEP", &["solvecraft", "step", "stp", "STEP", "STP"])
                .add_filter("SolveCraft design", &["solvecraft"])
                .add_filter("STEP", &["step", "stp", "STEP", "STP"])
                .add_filter("All files", &["*"])
                .pick_file()
                .map(|p| p.to_string_lossy().to_string())
        })),
        pick_save: Some(Box::new(|name: &str, exts: &[&str]| {
            rfd::FileDialog::new().set_file_name(name).add_filter("File", exts).save_file().map(|p| p.to_string_lossy().to_string())
        })),
    }
}

const DEPTH_BITS: u8 = 24;
const MSAA: u16 = 4;

fn main() -> eframe::Result {
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
            .with_app_id("ai.storyteller.solvecraft"),
        depth_buffer: DEPTH_BITS,
        multisampling: MSAA,
        ..Default::default()
    };
    eframe::run_native(
        "SolveCraft",
        options,
        Box::new(move |cc| {
            let mut app = SolveApp::new(Session::default(), services());
            if let Some(rs) = &cc.wgpu_render_state {
                app.set_wgpu(rs, DEPTH_BITS, u32::from(MSAA));
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
            Ok(Box::new(App(app)))
        }),
    )
}
