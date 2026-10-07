//! SolveCraft in the browser.
//!
//! Runs the same [`solvecraft_ui_egui::SolveApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from this
//! directory (output in `dist/web`). URL flags: `?webgl` forces WebGL2, `?sample` opens the
//! sample part.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod web {
    use solvecraft_engine::Session;
    use solvecraft_ui_egui::{Services, SolveApp};
    use wasm_bindgen::JsCast as _;

    const CANVAS_ID: &str = "solvecraft_canvas";
    const LOADING_ID: &str = "solvecraft_loading";
    const DEPTH_BITS: u8 = 24;

    struct Shell(SolveApp);

    impl eframe::App for Shell {
        fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.0.logic(ctx);
        }
        fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
            self.0.raw_input_hook(raw);
        }
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            self.0.ui(ui);
        }
    }

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
                        let mut app = SolveApp::new(Session::default(), Services::default());
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
                        Ok(Box::new(Shell(app)))
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
