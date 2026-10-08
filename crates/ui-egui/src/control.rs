//! Programmatic control of the running app (agents, tests).
//!
//! Methods (JSON lines over the host's transport, `{"id", "method", "params"}`):
//! - `engine.execute {command, params}`: run any command with JSON parameters (no dialogs)
//! - `engine.script {commands: [...]}`: run a command script
//! - `engine.commands`: every command with tab, panel, params and enablement
//! - `document.inspect {measure?}`: the design (parameters, timeline, bodies, sketches)
//! - `ui.inspect`: UI state, viewport rect, camera, tool and dialog
//! - `ui.drag {x0, y0, x1, y1, button?, shift?, ctrl?, steps?, hold?}` (box selection, navigation;
//!   `hold` keeps the button down that many frames before moving);
//!   `ui.selection` (selection, dialog inputs, hover, drawn sketch dimensions);
//!   `ui.sketchToScreen {points: [[x,y]…]}` (screen points of active-sketch coordinates); `ui.editFeature {feature}` (edit dialog)
//! - `ui.set {...UiState fields}`; `ui.view {view: front|back|top|bottom|left|right|iso|home|fit, animate?: bool}` (snaps unless animate)
//! - `ui.start {command}`: like clicking the toolbar button (starts tools/dialogs)
//! - `ui.click {x, y, button?, shift?}`, `ui.move {x, y}`, `ui.scroll {x, y, delta}`: real
//!   pointer input in screen points; `ui.key {key, cmd?, shift?}`, `ui.text {text}`
//! - `ui.screenshot {path?}`: PNG of the window; `ui.render {path, width?, height?}`: headless
//!   CPU render of the model with the current camera (no window needed)
//! - `ui.menu {x?, y?, target?, close?}`: open a context menu at a point (the viewport's marking
//!   menu, or a browser menu with `target: {type: body|sketch|component, …}`); without a point,
//!   the open menu and its items. `ui.menuPick {item}` runs an item by id or label;
//!   `ui.rename {text?, commit?}` finishes the rename box
//! - `ui.window {action: minimize|maximize|restore|toggle|close}`: what the title bar's buttons do
//! - `ui.confirm {accept?}`: the Delete confirmation (without `accept`: what it lists)
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
            "preview": {"active": app.preview.active, "busy": app.preview.busy, "error": app.preview.error, "ms": app.preview.ms, "replaced": app.preview.replaced},
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
                let modifiers = egui::Modifiers { shift: b("shift"), ctrl: b("ctrl"), command: b("ctrl"), ..Default::default() };
                // `double: true` sends two clicks in one frame, which egui reads as a double-click.
                for _ in 0..if b("double") { 2 } else { 1 } {
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers });
                    app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers });
                }
            }
            ok(Value::Null)
        }
        "ui.drag" => {
            let (Some(x0), Some(y0), Some(x1), Some(y1)) = (f("x0"), f("y0"), f("x1"), f("y1")) else { return err("missing x0/y0/x1/y1") };
            let button = match s("button") {
                Some("right") => egui::PointerButton::Secondary,
                Some("middle") => egui::PointerButton::Middle,
                _ => egui::PointerButton::Primary,
            };
            let modifiers = egui::Modifiers { shift: b("shift"), ctrl: b("ctrl"), command: b("ctrl"), ..Default::default() };
            let (a, z) = (egui::pos2(x0 as f32, y0 as f32), egui::pos2(x1 as f32, y1 as f32));
            app.synthetic.push(egui::Event::PointerMoved(a));
            app.synthetic.push(egui::Event::PointerButton { pos: a, button, pressed: true, modifiers });
            // `hold`: frames to keep the button down before moving (press-and-hold).
            for _ in 0..f("hold").unwrap_or(0.0).clamp(0.0, 600.0) as usize {
                app.synthetic.push(egui::Event::PointerMoved(a));
            }
            let steps = f("steps").unwrap_or(10.0).clamp(1.0, 200.0) as usize;
            for k in 1..=steps {
                app.synthetic.push(egui::Event::PointerMoved(a + (z - a) * (k as f32 / steps as f32)));
            }
            app.synthetic.push(egui::Event::PointerButton { pos: z, button, pressed: false, modifiers });
            ok(Value::Null)
        }
        "ui.editFeature" => {
            let key = match p.get("feature") {
                Some(Value::Number(n)) => n.to_string(),
                Some(Value::String(x)) => x.clone(),
                _ => return err("`feature` must be an id or name"),
            };
            let Some(id) = app.session.doc.find_feature(&key).map(|f| f.id) else { return err(format!("no feature `{key}`")) };
            app.edit_feature(id);
            ok(json!({"dialog": app.dialog.as_ref().map(|d| format!("{d:?}")), "marker": app.session.doc.marker}))
        }
        "ui.selection" => ok(json!({
            "selection": app.session.selection,
            "dialog": app.dialog.as_ref().map(|d| d.inputs.iter().map(|i| json!({"label": i.label, "items": i.items})).collect::<Vec<_>>()),
            "hover": app.viewport.hover.as_ref().map(|h| format!("{h:?}")),
            "dimensions": crate::dim_view::drawn().into_iter().map(|(id, p)| json!({"id": id, "x": p.x, "y": p.y})).collect::<Vec<_>>(),
            "dimension_selected": crate::dim_view::selected(app),
            "dimension_editing": crate::dim_view::editing(),
        })),
        "ui.sketchToScreen" => {
            // Screen points of active-sketch coordinates (for driving sketch interaction).
            let st = app.session.model.state();
            let (Some(rect), Some(ss)) = (app.viewport.rect, app.session.active_sketch.and_then(|s| st.sketch(s))) else {
                return err("no active sketch in a drawn viewport");
            };
            let proj = crate::viewport::projection(app, rect);
            let pts: Vec<Value> = p
                .get("points")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(1000)
                .map(|v| {
                    let c = |i: usize| v.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                    let q = solvecraft_engine::geom::Vec2::new(c(0), c(1));
                    proj.to_screen(ss.plane.to_world(q)).map_or(Value::Null, |p| json!([p.x, p.y]))
                })
                .collect();
            ok(json!(pts))
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
        "ui.menu" => {
            if b("close") {
                crate::context_menu::close(app);
                return ok(json!({"open": false}));
            }
            if let (Some(x), Some(y)) = (f("x"), f("y")) {
                let target = match p.get("target") {
                    Some(v) => match serde_json::from_value::<crate::context_menu::Target>(v.clone()) {
                        Ok(t) => t,
                        Err(e) => return err(e),
                    },
                    None => crate::context_menu::Target::Viewport,
                };
                crate::context_menu::request_open(app, egui::pos2(x as f32, y as f32), target);
                return ok(json!({"pending": true}));
            }
            ok(crate::context_menu::describe(app))
        }
        "ui.confirm" => match p.get("accept").and_then(Value::as_bool) {
            Some(a) => {
                if crate::delete::confirm(app, a) {
                    ok(json!({"accepted": a}))
                } else {
                    err("nothing is waiting for a confirmation")
                }
            }
            None => ok(json!({"pending": app.menu.confirm.as_ref().map(|c| c.report.clone())})),
        },
        "ui.menuPick" => {
            let Some(key) = s("item") else { return err("missing `item` (id or label)") };
            wrap(crate::context_menu::pick(app, key))
        }
        "ui.rename" => {
            if crate::context_menu::finish_rename(app, s("text"), p.get("commit").and_then(Value::as_bool).unwrap_or(true)) {
                ok(json!({"done": true}))
            } else {
                err("no rename in progress")
            }
        }
        "ui.window" => match crate::titlebar::window_action(app, ctx, s("action").unwrap_or("")) {
            Ok(()) => ok(json!({"custom_titlebar": app.custom_titlebar, "maximized": ctx.input(|i| i.viewport().maximized)})),
            Err(e) => err(e),
        },
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
