//! Programmatic control of the running app (agents, tests).
//!
//! Methods (JSON lines over the host's transport, `{"id", "method", "params"}`):
//! - `engine.execute {command, params}`: run any command with JSON parameters (no dialogs)
//! - `engine.script {commands: [...]}`: run a command script
//! - `engine.commands`: every command with tab, panel, params and enablement
//! - `document.inspect {measure?}`: the design (parameters, timeline, bodies, sketches)
//! - `ui.inspect`: UI state, viewport rect, camera, tool and dialog
//! - `ui.set {...UiState fields}`; `ui.view {view: front|back|top|bottom|left|right|iso|home|fit, animate?: bool}` (snaps unless animate)
//! - `ui.start {command}`: like clicking the toolbar button (starts tools/dialogs)
//! - `ui.click {x, y, button?, shift?}`, `ui.move {x, y}`, `ui.scroll {x, y, delta}`: real
//!   pointer input in screen points; `ui.key {key, cmd?, shift?}`, `ui.text {text}`
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path, width?, height?}`: headless
//!   CPU render of the model with the current camera (no window needed)
//! - `ui.resize {width, height}`, `app.quit`

use std::sync::mpsc::Sender;

use serde_json::{Value, json};
use solvecraft_engine::render::{StandardView, render_png};

use crate::SolveApp;

pub type ControlResponse = Value;

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<ControlResponse>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, std::sync::mpsc::Receiver<ControlResponse>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self { method: method.into(), params, reply: tx }, rx)
    }
}

pub enum Outcome {
    Done(Value),
    Screenshot { path: Option<String> },
}

fn ok(v: Value) -> Outcome {
    Outcome::Done(json!({"ok": true, "result": v}))
}
fn err(e: impl std::fmt::Display) -> Outcome {
    Outcome::Done(json!({"ok": false, "error": e.to_string()}))
}
fn wrap(r: Result<Value, String>) -> Outcome {
    match r {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "enter" | "return" => Some(egui::Key::Enter),
        "esc" | "escape" => Some(egui::Key::Escape),
        "delete" => Some(egui::Key::Delete),
        "space" => Some(egui::Key::Space),
        "tab" => Some(egui::Key::Tab),
        _ => None,
    })
}

pub fn handle(app: &mut SolveApp, ctx: &egui::Context, req: &ControlRequest) -> Outcome {
    let p = &req.params;
    let s = |k: &str| p.get(k).and_then(Value::as_str);
    let f = |k: &str| p.get(k).and_then(Value::as_f64).filter(|x| x.is_finite());
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    match req.method.as_str() {
        "engine.execute" | "command" => {
            let Some(id) = s("command").or(s("id")) else { return err("missing `command`") };
            let params = p.get("params").cloned().filter(|v| !v.is_null()).unwrap_or(json!({}));
            let r = app.run(id, params);
            if r.is_ok() && app.session.active_sketch.is_none() && app.session.model.state().bodies.len() == 1 && app.session.undo.len() <= 2 {
                app.fit_view();
            }
            wrap(r)
        }
        "engine.script" => wrap(app.session.run_script(p).map(|v| json!(v)).map_err(|e| e.to_string())),
        "engine.commands" => wrap(app.session.execute("engine.commands", &json!({})).map_err(|e| e.to_string())),
        "document.inspect" => wrap(app.session.execute("document.inspect", p).map_err(|e| e.to_string())),
        "ui.inspect" => ok(json!({
            "ui": serde_json::to_value(&app.ui).unwrap_or_default(),
            "viewport": app.viewport.rect.map(|r| json!([r.left(), r.top(), r.width(), r.height()])),
            "camera": app.cam,
            "tool": app.tool.as_ref().map(|t| t.cmd),
            "dialog": app.dialog.as_ref().map(|d| format!("{d:?}")),
            "hover": app.viewport.hover.as_ref().map(|h| format!("{h:?}")),
            "renderer": if app.viewport.gpu.is_some() { "gpu" } else { "cpu" },
            "frame_ms": app.frame_ms,
            "build_ms": app.viewport.build_ms,
        })),
        "ui.set" => {
            let mut cur = serde_json::to_value(&app.ui).unwrap_or(json!({}));
            if let (Some(o), Some(src)) = (cur.as_object_mut(), p.as_object()) {
                for (k, v) in src {
                    o.insert(k.clone(), v.clone());
                }
            }
            match serde_json::from_value::<crate::UiState>(cur) {
                Ok(u) => {
                    app.ui = u;
                    ok(serde_json::to_value(&app.ui).unwrap_or_default())
                }
                Err(e) => err(e),
            }
        }
        "ui.view" => {
            let v = s("view").unwrap_or("home");
            if p.get("animate").and_then(Value::as_bool).unwrap_or(false) {
                if v != "fit" && v != "home" && StandardView::parse(v).is_none() {
                    return err(format!("unknown view `{v}`"));
                }
                app.animate_view(v);
                return ok(json!({ "animating": true, "to": app.cam_anim.map(|a| a.to) }));
            }
            app.cam_anim = None;
            match v {
                "fit" => app.fit_view(),
                "home" => {
                    app.cam.set_view(StandardView::Iso);
                    app.fit_view();
                }
                v => match StandardView::parse(v) {
                    Some(sv) => app.cam.set_view(sv),
                    None => return err(format!("unknown view `{v}`")),
                },
            }
            ok(json!(app.cam))
        }
        "ui.start" => {
            let Some(id) = s("command") else { return err("missing `command`") };
            app.start(id);
            ok(json!({"tool": app.tool.as_ref().map(|t| t.cmd), "dialog": app.dialog.as_ref().map(|d| format!("{d:?}"))}))
        }
        "ui.move" | "ui.click" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            let pos = egui::pos2(x as f32, y as f32);
            app.synthetic.push(egui::Event::PointerMoved(pos));
            if req.method == "ui.click" {
                let button = match s("button") {
                    Some("right") => egui::PointerButton::Secondary,
                    Some("middle") => egui::PointerButton::Middle,
                    _ => egui::PointerButton::Primary,
                };
                let modifiers = egui::Modifiers { shift: b("shift"), ..Default::default() };
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
            }
            ok(Value::Null)
        }
        "ui.scroll" => {
            let (Some(x), Some(y)) = (f("x"), f("y")) else { return err("missing x/y") };
            app.synthetic.push(egui::Event::PointerMoved(egui::pos2(x as f32, y as f32)));
            app.synthetic.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, f("delta").unwrap_or(-100.0) as f32),
                modifiers: Default::default(),
                phase: egui::TouchPhase::Move,
            });
            ok(Value::Null)
        }
        "ui.key" => {
            let Some(key) = s("key").and_then(key_from) else { return err("unknown key") };
            let modifiers = egui::Modifiers { shift: b("shift"), command: b("cmd"), ctrl: b("cmd") || b("ctrl"), ..Default::default() };
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
            ok(Value::Null)
        }
        "ui.text" => {
            app.synthetic.push(egui::Event::Text(s("text").unwrap_or("").to_string()));
            ok(Value::Null)
        }
        "ui.resize" => {
            let (Some(w), Some(h)) = (f("width"), f("height")) else { return err("missing width/height") };
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w.clamp(320.0, 8192.0) as f32, h.clamp(240.0, 8192.0) as f32)));
            ok(Value::Null)
        }
        "ui.screenshot" => Outcome::Screenshot { path: s("path").map(str::to_string) },
        "ui.render" => {
            let w = f("width").unwrap_or(1280.0).clamp(16.0, 8192.0) as usize;
            let h = f("height").unwrap_or(800.0).clamp(16.0, 8192.0) as usize;
            let scene = solvecraft_engine::view::scene(&app.session, &app.cam);
            let Some(png) = render_png(&scene, &app.cam, w, h) else { return err("render failed") };
            match s("path") {
                Some(path) => match std::fs::write(path, &png) {
                    Ok(()) => ok(json!({"path": path, "bytes": png.len()})),
                    Err(e) => err(e),
                },
                None => ok(json!({"bytes": png.len()})),
            }
        }
        "app.quit" => {
            app.quit_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ok(Value::Null)
        }
        other => err(format!("unknown method `{other}`")),
    }
}

/// Save a screenshot PNG.
pub fn save_screenshot(image: &egui::ColorImage, path: Option<&str>) -> Value {
    let [w, h] = image.size;
    let mut buf = Vec::with_capacity(w * h * 4);
    for c in &image.pixels {
        buf.extend_from_slice(&c.to_array());
    }
    let Some(img) = image::RgbaImage::from_raw(w as u32, h as u32, buf) else { return json!({"ok": false, "error": "bad image"}) };
    let path = path.map(str::to_string).unwrap_or_else(|| std::env::temp_dir().join("solvecraft-shot.png").to_string_lossy().to_string());
    match img.save(&path) {
        Ok(()) => json!({"ok": true, "result": {"path": path, "width": w, "height": h}}),
        Err(e) => json!({"ok": false, "error": e.to_string()}),
    }
}
