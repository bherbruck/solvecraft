//! User scenarios run headless: the real app (`SolveApp`) drawn frame by frame into an egui
//! context without a window, driven by the same requests as the control channel (clicks, keys,
//! drags, toolbar starts) the way a person would use it, with checks on the result.
//!
//! A scenario is a JSON list of steps:
//! - `{"start": "Extrude"}`: a toolbar click; `{"key": "E", "shift"?, "cmd"?}`; `{"text": "20"}`
//! - `{"widget": "Revolute"}`: click a dialog field, choice or button by its text
//! - `{"click": AT, "double"?, "shift"?, "ctrl"?, "button"?}`, `{"move": AT}`,
//!   `{"drag": [AT, AT], "shift"?, "steps"?}` where AT is `[x, y]` (screen), `{"world": [x,y,z]}`
//!   or `{"sketch": [x,y]}` (the sketch being edited)
//! - `{"call": "ui.view", "params": {…}}`: any other control request (`fail: true` expects an
//!   error)
//! - `{"choose_file": "part.step"}`: the next Open picker returns that file of the scenario's
//!   folder (Save pickers save there)
//! - `{"shot": "name"}`: a screenshot when run in a window; headless, a render of the model
//!   into `$SOLVECRAFT_SCENARIO_SHOTS` when that is set; `{"debug": 1}`
//!   prints the UI state
//! - AT may also be `{"plane": "XY"}`: the middle of an origin plane's square
//! - `{"expect": {…}}`: checks, see [`check`]
//!
//! The same files drive the live app over the control channel for screenshots.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use serde_json::{Value, json};
use solvecraft_engine::Session;

use crate::control::{ControlRequest, Outcome, handle};
use crate::{Services, SolveApp};

/// A fresh folder for one scenario's files.
fn scratch_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("solvecraft-scenario-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::create_dir_all(&d);
    d
}

/// The app in a headless egui context.
pub struct Harness {
    pub app: SolveApp,
    pub ctx: egui::Context,
    /// The last frame's platform output (with its accessibility tree: dialog widgets are found
    /// by their text there).
    pub output: egui::PlatformOutput,
    /// Files the scenario saves and opens live here; file pickers answer from it.
    pub dir: PathBuf,
    /// The file the next Open picker returns (a `choose_file` step).
    next_file: Rc<RefCell<Option<String>>>,
    time: f64,
    size: egui::Vec2,
}

impl Default for Harness {
    fn default() -> Self {
        Harness::new()
    }
}

impl Harness {
    pub fn new() -> Harness {
        let mut h = Harness {
            app: SolveApp::new(Session::default(), Services::default()),
            ctx: egui::Context::default(),
            output: Default::default(),
            dir: scratch_dir(),
            next_file: Rc::new(RefCell::new(None)),
            time: 0.0,
            size: egui::vec2(1600.0, 1000.0),
        };
        // File pickers answer like a person would: save into the scenario's folder, open the
        // file the scenario chose.
        let dir = h.dir.clone();
        h.app.services.pick_save = Some(Box::new(move |name, _| Some(dir.join(name).to_string_lossy().into_owned())));
        let next = h.next_file.clone();
        h.app.services.pick_open = Some(Box::new(move || next.borrow_mut().take()));
        h.ctx.enable_accesskit();
        h.frames(3);
        h
    }

    /// Lay out and draw one frame (feeding queued pointer and key events).
    pub fn frame(&mut self) {
        self.time += 1.0 / 60.0;
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            focused: true,
            ..Default::default()
        };
        self.app.raw_input_hook(&mut raw);
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        // Nothing is painted: drop the frame's texture changes.
        out.textures_delta.clear();
        self.output = out.platform_output;
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Frames until the queued input is used up (and the view has settled).
    pub fn settle(&mut self) {
        for _ in 0..2000 {
            // Queued input used up and the view no longer moving.
            if self.app.synthetic.is_empty() && self.app.cam_anim.is_none() {
                break;
            }
            self.frame();
        }
        self.frames(4);
    }

    /// One control request, then frames until its input has been handled.
    pub fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, _rx) = ControlRequest::new(method, params);
        let out = match handle(&mut self.app, &self.ctx, &req) {
            Outcome::Done(v) => v,
            Outcome::Screenshot { .. } => json!({"ok": true, "result": null}),
        };
        self.settle();
        out
    }

    /// The centre of the visible widget whose label or value is `text` (the last one drawn wins:
    /// popups are drawn after what opened them).
    pub fn widget(&self, text: &str) -> Option<[f64; 2]> {
        let tree = self.output.accesskit_update.as_ref()?;
        tree.nodes
            .iter()
            .rev()
            // A menu button's label ends with its shortcut ("Save Ctrl+S").
            .filter(|(_, n)| {
                n.label().is_some_and(|l| l == text || l.strip_prefix(text).is_some_and(|r| r.starts_with(' '))) || n.value() == Some(text)
            })
            .filter_map(|(_, n)| n.bounds())
            .find(|b| b.x1 > b.x0 && b.y1 > b.y0)
            .map(|b| [(b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0])
    }

    /// A step's point on screen.
    pub fn at(&mut self, v: &Value) -> Result<[f64; 2], String> {
        if v.is_array() {
            let x = v.get(0).and_then(Value::as_f64).ok_or_else(|| format!("not a point: {v}"))?;
            let y = v.get(1).and_then(Value::as_f64).ok_or_else(|| format!("not a point: {v}"))?;
            return Ok([x, y]);
        }
        let r = self.call("ui.at", v.clone());
        let p = &r["result"];
        match (p.get(0).and_then(Value::as_f64), p.get(1).and_then(Value::as_f64)) {
            (Some(x), Some(y)) => Ok([x, y]),
            _ => Err(format!("{v} is not on screen: {r}")),
        }
    }

    /// Run one step.
    pub fn step(&mut self, s: &Value) -> Result<(), String> {
        let flag = |k: &str| s.get(k).and_then(Value::as_bool).unwrap_or(false);
        let res = if let Some(id) = s.get("start").and_then(Value::as_str) {
            self.call("ui.start", json!({ "command": id }))
        } else if let Some(k) = s.get("key").and_then(Value::as_str) {
            self.call("ui.key", json!({"key": k, "shift": flag("shift"), "cmd": flag("cmd")}))
        } else if let Some(t) = s.get("text").and_then(Value::as_str) {
            self.call("ui.text", json!({ "text": t }))
        } else if let Some(at) = s.get("click") {
            let [x, y] = self.at(at)?;
            // Hover first, as a hand does.
            self.call("ui.move", json!({"x": x, "y": y}));
            let mut p = json!({"x": x, "y": y, "double": flag("double"), "shift": flag("shift"), "ctrl": flag("ctrl")});
            if let Some(b) = s.get("button") {
                p["button"] = b.clone();
            }
            self.call("ui.click", p)
        } else if let Some(text) = s.get("widget").and_then(Value::as_str) {
            // A dialog field, choice or button, found by its text.
            let [x, y] = self.widget(text).ok_or_else(|| format!("no widget `{text}` on screen"))?;
            self.call("ui.move", json!({"x": x, "y": y}));
            self.call("ui.click", json!({"x": x, "y": y}))
        } else if let Some(at) = s.get("move") {
            let [x, y] = self.at(at)?;
            self.call("ui.move", json!({"x": x, "y": y}))
        } else if let Some(d) = s.get("drag") {
            let [x0, y0] = self.at(d.get(0).unwrap_or(&Value::Null))?;
            let [x1, y1] = self.at(d.get(1).unwrap_or(&Value::Null))?;
            let steps = s.get("steps").cloned().unwrap_or(json!(8));
            self.call("ui.drag", json!({"x0": x0, "y0": y0, "x1": x1, "y1": y1, "steps": steps, "shift": flag("shift")}))
        } else if let Some(m) = s.get("call").and_then(Value::as_str) {
            self.call(m, s.get("params").cloned().unwrap_or(json!({})))
        } else if let Some(name) = s.get("shot").and_then(Value::as_str) {
            // With SOLVECRAFT_SCENARIO_SHOTS set: a render of the model as the camera sees it.
            if let Ok(dir) = std::env::var("SOLVECRAFT_SCENARIO_SHOTS") {
                let path = std::path::Path::new(&dir).join(format!("{name}.png"));
                self.call("ui.render", json!({"path": path.to_string_lossy(), "width": 1000, "height": 700}));
            }
            return Ok(());
        } else if let Some(f) = s.get("choose_file").and_then(Value::as_str) {
            *self.next_file.borrow_mut() = Some(self.dir.join(f).to_string_lossy().into_owned());
            return Ok(());
        } else if let Some(m) = s.get("print").and_then(Value::as_str) {
            let r = self.call(m, s.get("params").cloned().unwrap_or(json!({})));
            eprintln!("print {m}: {r}");
            return Ok(());
        } else if s.get("debug").is_some() {
            let ui = self.call("ui.inspect", json!({}));
            let sel = self.call("ui.selection", json!({}));
            eprintln!(
                "debug: dialog {} tool {} hover {}\nselection {}\nstatus {:?}",
                ui["result"]["dialog"], ui["result"]["tool"], ui["result"]["hover"], sel["result"]["selection"], self.app.status
            );
            if s["debug"] == json!("widgets") {
                let texts: Vec<String> = self
                    .output
                    .accesskit_update
                    .iter()
                    .flat_map(|t| t.nodes.iter())
                    .filter_map(|(_, n)| n.label().or(n.value()).map(|l| format!("{:?} {l}", n.role())))
                    .collect();
                eprintln!("widgets: {texts:?}");
            }
            return Ok(());
        } else if let Some(e) = s.get("expect") {
            return check(self, e);
        } else {
            return Err(format!("unknown step {s}"));
        };
        let failed = res["ok"] != json!(true);
        if failed != flag("fail") {
            return Err(format!("{s}: {res}"));
        }
        Ok(())
    }

    /// Run a scenario (its steps); the error names the failing step.
    pub fn run(&mut self, steps: &Value) -> Result<(), String> {
        for (i, s) in steps.as_array().ok_or("a scenario is a list of steps")?.iter().enumerate() {
            self.step(s).map_err(|e| format!("step {i}: {e}"))?;
        }
        Ok(())
    }
}

fn approx(got: f64, want: &Value, what: &str) -> Result<(), String> {
    let (w, tol) = match want {
        Value::Array(a) => (a.first().and_then(Value::as_f64).unwrap_or(f64::NAN), a.get(1).and_then(Value::as_f64).unwrap_or(1e-3)),
        v => (v.as_f64().unwrap_or(f64::NAN), 1e-3),
    };
    if (got - w).abs() <= tol * w.abs().max(1.0) { Ok(()) } else { Err(format!("{what}: got {got}, want {w} (±{tol} rel)")) }
}

/// Checks: `bodies` (count), `volume` / `area` (total, number or [value, rel tol]), `faces`,
/// `features` (count), `errors` (timeline features in error, default 0), `dialog` (null: none
/// open, or a word its kind must contain), `tool` (null or command id), `sketching` (bool),
/// `selection` (count), `param` ({name: value}), `sketch_status` (solved|…), `dof`, `query`
/// ({command, params?, path, value}: a command's result).
pub fn check(h: &mut Harness, e: &Value) -> Result<(), String> {
    let doc = h.call("document.inspect", json!({"measure": true}))["result"].clone();
    let ui = h.call("ui.inspect", json!({}))["result"].clone();
    let sum = |k: &str| doc["bodies"].as_array().into_iter().flatten().filter_map(|b| b[k].as_f64()).sum::<f64>();
    let total = json!({"volume_mm3": sum("volume_mm3"), "area_mm2": sum("area_mm2"), "faces": sum("faces") as u64});
    let total = &total;
    let bodies = doc["bodies"].as_array().map_or(0, Vec::len);
    let obj = e.as_object().ok_or("expect takes an object")?;
    for (k, v) in obj {
        match k.as_str() {
            "bodies" => {
                if v.as_u64() != Some(bodies as u64) {
                    return Err(format!("bodies: got {bodies}, want {v}"));
                }
            }
            "volume" => approx(total["volume_mm3"].as_f64().unwrap_or(f64::NAN), v, "volume")?,
            "area" => approx(total["area_mm2"].as_f64().unwrap_or(f64::NAN), v, "area")?,
            "faces" => {
                if total["faces"] != *v {
                    return Err(format!("faces: got {}, want {v}", total["faces"]));
                }
            }
            "features" => {
                let n = doc["timeline"].as_array().map_or(0, Vec::len);
                if v.as_u64() != Some(n as u64) {
                    return Err(format!("features: got {n}, want {v}"));
                }
            }
            "errors" => {
                let bad: Vec<String> = doc["timeline"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|f| !f["error"].is_null())
                    .map(|f| format!("{}: {}", f["name"], f["error"]))
                    .collect();
                if v.as_u64() != Some(bad.len() as u64) {
                    return Err(format!("errors: {bad:?}"));
                }
            }
            "dialog" => {
                let d = &ui["dialog"];
                let ok = match v {
                    Value::Null => d.is_null(),
                    Value::String(w) => d.as_str().is_some_and(|d| d.contains(w.as_str())),
                    _ => false,
                };
                if !ok {
                    return Err(format!("dialog: got {d}, want {v}"));
                }
            }
            "tool" => {
                if ui["tool"] != *v {
                    return Err(format!("tool: got {}, want {v}", ui["tool"]));
                }
            }
            "sketching" => {
                if doc["active_sketch"].is_null() == v.as_bool().unwrap_or(false) {
                    return Err(format!("sketching: want {v}, active sketch {}", doc["active_sketch"]));
                }
            }
            "selection" => {
                let n = doc["selection"].as_array().map_or(0, Vec::len);
                if v.as_u64() != Some(n as u64) {
                    return Err(format!("selection: got {n}, want {v}"));
                }
            }
            "param" => {
                for (name, want) in v.as_object().into_iter().flatten() {
                    let p = doc["params"].as_array().into_iter().flatten().find(|p| p["name"] == json!(name)).cloned().unwrap_or_default();
                    approx(p["value"].as_f64().unwrap_or(f64::NAN), want, &format!("param {name}"))?;
                }
            }
            "sketch_status" | "dof" => {
                let si = h.call("engine.execute", json!({"command": "sketch.inspect", "params": {}}))["result"].clone();
                let key = if k == "dof" { "dof" } else { "status" };
                if si[key] != *v {
                    return Err(format!("{k}: got {}, want {v}", si[key]));
                }
            }
            "file" => {
                // A file the scenario saved: it exists and is not empty.
                let p = h.dir.join(v.as_str().unwrap_or_default());
                if std::fs::metadata(&p).map_or(0, |m| m.len()) == 0 {
                    return Err(format!("no file {}", p.display()));
                }
            }
            "query" => {
                // {command, params?, path: [keys or indices], value}: a command's result at path.
                let cmd = v["command"].as_str().ok_or("query needs `command`")?;
                let r = h.call("engine.execute", json!({"command": cmd, "params": v.get("params").cloned().unwrap_or(json!({}))}));
                let mut at = &r["result"];
                for k in v["path"].as_array().into_iter().flatten() {
                    at = match k {
                        Value::Number(n) => &at[n.as_u64().unwrap_or(0) as usize],
                        Value::String(s) => &at[s.as_str()],
                        _ => &Value::Null,
                    };
                }
                let want = &v["value"];
                let ok = match (at.as_f64(), want.as_f64()) {
                    (Some(a), Some(w)) => (a - w).abs() <= 1e-6 * w.abs().max(1.0),
                    _ => at == want,
                };
                if !ok {
                    return Err(format!("query {cmd} {}: got {at}, want {want}", v["path"]));
                }
            }
            other => return Err(format!("unknown check `{other}`")),
        }
    }
    Ok(())
}
