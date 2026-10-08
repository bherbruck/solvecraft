//! User scenarios run headless: the real app (`SolveApp`) drawn frame by frame into an egui
//! context without a window, driven by the same requests as the control channel (clicks, keys,
//! drags, toolbar starts) the way a person would use it, with checks on the result.
//!
//! A scenario is a JSON list of steps:
//! - `{"start": "Extrude"}`: a toolbar click; `{"key": "E", "shift"?, "cmd"?}`; `{"text": "20"}`
//! - `{"click": AT, "double"?, "shift"?, "ctrl"?, "button"?}`, `{"move": AT}`,
//!   `{"drag": [AT, AT], "shift"?, "steps"?}` where AT is `[x, y]` (screen), `{"world": [x,y,z]}`
//!   or `{"sketch": [x,y]}` (the sketch being edited)
//! - `{"call": "ui.view", "params": {…}}`: any other control request (`fail: true` expects an
//!   error)
//! - `{"shot": "name"}`: a screenshot when run in a window (ignored headless); `{"debug": 1}`
//!   prints the UI state
//! - AT may also be `{"plane": "XY"}`: the middle of an origin plane's square
//! - `{"expect": {…}}`: checks, see [`check`]
//!
//! The same files drive the live app over the control channel for screenshots.

use serde_json::{Value, json};
use solvecraft_engine::Session;

use crate::control::{ControlRequest, Outcome, handle};
use crate::{Services, SolveApp};

/// The app in a headless egui context.
pub struct Harness {
    pub app: SolveApp,
    pub ctx: egui::Context,
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
            time: 0.0,
            size: egui::vec2(1600.0, 1000.0),
        };
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
        } else if s.get("shot").is_some() {
            return Ok(());
        } else if s.get("debug").is_some() {
            let ui = self.call("ui.inspect", json!({}));
            let sel = self.call("ui.selection", json!({}));
            eprintln!(
                "debug: dialog {} tool {} hover {}\nselection {}\nstatus {:?}",
                ui["result"]["dialog"], ui["result"]["tool"], ui["result"]["hover"], sel["result"]["selection"], self.app.status
            );
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
/// `selection` (count), `param` ({name: value}), `sketch_status` (solved|…), `dof`.
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
            other => return Err(format!("unknown check `{other}`")),
        }
    }
    Ok(())
}
