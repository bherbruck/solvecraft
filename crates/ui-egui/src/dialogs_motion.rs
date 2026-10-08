//! Assembly motion dialogs: Contact Sets, Motion Study (keyframes on a timeline with playback),
//! Exploded View (a scale slider) and Configurations (a table of rows with Activate). What they
//! show while open (a study's pose, an explosion) is display only: pending occurrence moves,
//! computed on a scratch copy of the design and put back when the dialog closes.

use std::collections::BTreeMap;

use egui::{Color32, Pos2, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};
use solvecraft_engine::Session;
use solvecraft_engine::doc::Mat;
use solvecraft_engine::doc::joints::JointKind;

use crate::SolveApp;
use crate::dialogs::{Dialog, Kind, combo, field, row_label};
use crate::selection::{BODIES, SelInput};
use crate::theme::Tokens;

/// One keyed value of a motion study: a joint's value (index) at steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    /// Index into the design's joints.
    pub joint: usize,
    pub index: usize,
    /// (step, value expression), in step order.
    pub points: Vec<(u32, String)>,
}

#[derive(Clone, Debug)]
pub enum Mo {
    ContactSet {
        name: String,
        /// The last collision check.
        check: Option<Value>,
    },
    MotionStudy {
        name: String,
        /// Editing this study (its name).
        study: Option<String>,
        steps: String,
        tracks: Vec<Track>,
        step: f64,
        playing: bool,
        /// The step the shown pose is for (and its revision).
        shown: Option<(u64, u64)>,
        saved: Option<BTreeMap<u64, Mat>>,
        error: Option<String>,
    },
    Explode {
        name: String,
        scale: f64,
        shown: Option<(u64, u64)>,
        saved: Option<BTreeMap<u64, Mat>>,
        error: Option<String>,
    },
    Configurations {
        new_row: String,
        error: Option<String>,
    },
}

impl Mo {
    pub fn title(&self) -> &'static str {
        match self {
            Mo::ContactSet { .. } => "CONTACT SET",
            Mo::MotionStudy { study: Some(_), .. } => "EDIT MOTION STUDY",
            Mo::MotionStudy { .. } => "MOTION STUDY",
            Mo::Explode { .. } => "EXPLODED VIEW",
            Mo::Configurations { .. } => "CONFIGURATIONS",
        }
    }

    /// A table that needs the wide dialog.
    pub fn wide(&self) -> bool {
        matches!(self, Mo::Configurations { .. } | Mo::MotionStudy { .. })
    }
}

/// The dialog for a motion command.
pub fn start(app: &SolveApp, id: &str) -> Option<(Kind, Vec<SelInput>)> {
    let n = |prefix: &str, list: &str| -> String {
        let taken: Vec<String> = app
            .session
            .scratch()
            .execute(list, &json!({}))
            .ok()
            .and_then(|v| v.as_object().and_then(|o| o.values().find_map(|x| x.as_array().cloned())))
            .unwrap_or_default()
            .iter()
            .filter_map(|x| x.get("name").and_then(Value::as_str).map(str::to_string))
            .collect();
        (1..).map(|k| format!("{prefix}{k}")).find(|c| !taken.contains(c)).unwrap_or_default()
    };
    let (mo, inputs) = match id {
        "ContactSetCmd" => (Mo::ContactSet { name: n("Contact Set", "contact.list"), check: None }, vec![SelInput::new("Components", BODIES, true)]),
        "FusionMotionStudyCommand" => (
            Mo::MotionStudy {
                name: n("Motion Study", "motion.list"),
                study: None,
                steps: "100".into(),
                tracks: Vec::new(),
                step: 0.0,
                playing: false,
                shown: None,
                saved: None,
                error: None,
            },
            vec![],
        ),
        "explode.create" => (Mo::Explode { name: n("Exploded View", "explode.list"), scale: 1.0, shown: None, saved: None, error: None }, vec![]),
        "FusionShowDesignConfigPanelCmd" | "FusionStartDesignConfigModeCmd" => (Mo::Configurations { new_row: String::new(), error: None }, vec![]),
        _ => return None,
    };
    Some((Kind::Motion(mo), inputs))
}

// ---- pure mappings (tested) ----

/// The study command's parameters.
pub fn study_params(s: &Session, name: &str, study: Option<&str>, steps: &str, tracks: &[Track]) -> Result<Value, String> {
    let steps: u32 = steps.trim().parse().map_err(|_| "steps must be a whole number".to_string())?;
    let keys: Vec<Value> = tracks
        .iter()
        .filter(|t| !t.points.is_empty())
        .map(|t| {
            let j = s.doc.assembly.joints.get(t.joint).ok_or("no such joint")?;
            Ok(json!({"joint": j.id, "index": t.index, "points": t.points.iter().map(|(st, v)| json!([st, v])).collect::<Vec<_>>()}))
        })
        .collect::<Result<_, &str>>()?;
    if keys.is_empty() {
        return Err("add a key for a joint first".into());
    }
    let mut p = json!({"name": name, "steps": steps, "keys": keys});
    if let Some(st) = study {
        p["study"] = json!(st);
    }
    Ok(p)
}

/// The configurations table: columns and rows (name, value per column).
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<(String, Vec<String>)>,
    pub active: Option<String>,
}

pub fn table(v: &Value) -> Table {
    let columns: Vec<String> =
        v.get("columns").and_then(Value::as_array).into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
    let cell = |x: Option<&Value>| match x {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => {
            if *b {
                "suppressed".into()
            } else {
                "on".into()
            }
        }
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    };
    let rows = v
        .get("rows")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let name = r.get("name")?.as_str()?.to_string();
            let vals = r.get("values");
            Some((name, columns.iter().map(|c| cell(vals.and_then(|m| m.get(c)))).collect()))
        })
        .collect();
    Table { columns, rows, active: v.get("active").and_then(Value::as_str).map(str::to_string) }
}

/// The unit a value of this kind is shown in.
fn unit_of(k: solvecraft_engine::doc::expr::Kind) -> Option<&'static str> {
    match k {
        solvecraft_engine::doc::expr::Kind::Length => Some("mm"),
        solvecraft_engine::doc::expr::Kind::Angle => Some("deg"),
        _ => None,
    }
}

/// A column key ("param:width", "suppress:Fillet1") as (kind, name).
pub fn column_parts(c: &str) -> (&str, &str) {
    c.split_once(':').unwrap_or(("param", c))
}

/// The value a table cell edit sends: suppress columns take booleans, parameters expressions.
pub fn cell_value(column: &str, text: &str) -> Value {
    match column_parts(column).0 {
        "suppress" => json!(matches!(text.trim().to_ascii_lowercase().as_str(), "suppressed" | "true" | "yes" | "1" | "off")),
        _ => json!(text),
    }
}

// ---- display ----

/// Show `moves` as pending occurrence moves (display only), keeping what was there before.
fn display(app: &mut SolveApp, saved: &mut Option<BTreeMap<u64, Mat>>, moves: BTreeMap<u64, Mat>) {
    if saved.is_none() {
        *saved = Some(app.session.pending_moves.clone());
    }
    app.session.pending_moves = moves;
    app.session.revision += 1;
}

fn restore(app: &mut SolveApp, saved: &mut Option<BTreeMap<u64, Mat>>) {
    if let Some(m) = saved.take() {
        app.session.pending_moves = m;
        app.session.revision += 1;
    }
}

/// The dialog closed (`applied`: by OK): what it showed goes away, unless OK made it the view.
pub fn closed(app: &mut SolveApp, d: &mut Dialog, applied: bool) {
    let Kind::Motion(k) = &mut d.kind else { return };
    match k {
        Mo::MotionStudy { saved, .. } => restore(app, saved),
        // OK shows the new view itself (explode.show).
        Mo::Explode { saved, .. } if !applied => restore(app, saved),
        _ => {}
    }
}

/// The occurrence placements of `after` that differ from `before` (as pending moves).
fn moved(before: &Session, after: &Session) -> BTreeMap<u64, Mat> {
    let mut out = before.pending_moves.clone();
    for o in &after.doc.occurrences {
        let was = before.doc.occurrences.iter().find(|b| b.id == o.id).map(|b| b.transform);
        if was != Some(o.transform) {
            out.insert(o.id, o.transform);
        }
    }
    out
}

/// The study's pose at `step`, computed on a scratch copy.
fn study_pose(app: &SolveApp, params: &Value, step: u32) -> Result<BTreeMap<u64, Mat>, String> {
    let mut sc = app.session.scratch();
    let name = params["study"].as_str().or(params["name"].as_str()).unwrap_or_default().to_string();
    sc.execute("FusionMotionStudyCommand", params).map_err(|e| e.to_string())?;
    sc.execute("motion.play", &json!({"study": name, "step": step})).map_err(|e| e.to_string())?;
    Ok(moved(&app.session, &sc))
}

/// The explosion at `scale`, computed on a scratch copy.
fn explode_pose(app: &SolveApp, scale: f64) -> Result<BTreeMap<u64, Mat>, String> {
    let mut sc = app.session.scratch();
    sc.execute("explode.create", &json!({"name": "__preview", "scale": scale})).map_err(|e| e.to_string())?;
    sc.execute("explode.show", &json!({ "view": "__preview" })).map_err(|e| e.to_string())?;
    Ok(sc.pending_moves.clone())
}

// ---- rows ----

fn joints_moving(app: &SolveApp) -> Vec<(usize, String, JointKind)> {
    app.session.doc.assembly.joints.iter().enumerate().filter(|(_, j)| j.kind != JointKind::Rigid).map(|(i, j)| (i, j.name.clone(), j.kind)).collect()
}

/// The dialog's rows; true when Enter was pressed in a value.
pub fn rows(app: &mut SolveApp, ui: &mut egui::Ui, k: &mut Mo, inputs: &mut [SelInput]) -> bool {
    let t = Tokens::get();
    let mut enter = false;
    match k {
        Mo::ContactSet { name, check } => {
            row_label(ui, "Name");
            ui.text_edit_singleline(name);
            ui.end_row();
            let list = app.session.execute("contact.list", &json!({})).unwrap_or(Value::Null);
            row_label(ui, "Contact");
            let mut on = list.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut on, "enabled").changed() {
                let _ = app.run("EnableContactSetsCmd", json!({ "enabled": on }));
            }
            ui.end_row();
            for s in list.get("sets").and_then(Value::as_array).cloned().unwrap_or_default() {
                let n = s.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let off = s.get("suppressed").and_then(Value::as_bool).unwrap_or(false);
                row_label(ui, &n);
                ui.horizontal(|ui| {
                    let count = s.get("occurrences").and_then(Value::as_array).map_or(0, Vec::len);
                    ui.label(RichText::new(format!("{count} components")).color(t.text_dim));
                    if ui.small_button(if off { "Unsuppress" } else { "Suppress" }).clicked() {
                        let _ = app.run("contact.edit", json!({"set": n, "suppressed": !off}));
                    }
                    if ui.small_button("×").on_hover_text("Delete the set").clicked() {
                        let _ = app.run("contact.edit", json!({"set": n, "delete": true}));
                    }
                });
                ui.end_row();
            }
            row_label(ui, "");
            if ui.button("Check collisions").clicked() {
                *check = Some(app.session.execute("contact.check", &json!({})).unwrap_or_else(|e| json!({"error": e.to_string()})));
            }
            ui.end_row();
            if let Some(c) = check {
                let hits = c.get("collisions").and_then(Value::as_array).cloned().unwrap_or_default();
                row_label(ui, "Collisions");
                if let Some(e) = c.get("error").and_then(Value::as_str) {
                    ui.label(RichText::new(e).color(t.error));
                } else if hits.is_empty() {
                    ui.label(RichText::new("none").color(t.text_dim));
                } else {
                    ui.vertical(|ui| {
                        for h in hits {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}  {:.3} mm",
                                    name_of(&h["a"]),
                                    name_of(&h["b"]),
                                    h["depth_mm"].as_f64().unwrap_or(0.0)
                                ))
                                .color(t.error),
                            );
                        }
                    });
                }
                ui.end_row();
            }
        }
        Mo::MotionStudy { name, study, steps, tracks, step, playing, shown, saved, error } => {
            row_label(ui, "Name");
            ui.text_edit_singleline(name);
            ui.end_row();
            row_label(ui, "Steps");
            enter |= field(ui, steps);
            ui.end_row();
            let total = steps.trim().parse::<u32>().unwrap_or(100).clamp(1, 100_000);
            let moving = joints_moving(app);
            if moving.is_empty() {
                row_label(ui, "");
                ui.label(RichText::new("no joint that moves yet").color(t.text_dim));
                ui.end_row();
                return enter;
            }
            // Playback.
            row_label(ui, "Step");
            ui.horizontal(|ui| {
                if ui.button(if *playing { "■" } else { "▶" }).on_hover_text(if *playing { "Stop" } else { "Play" }).clicked() {
                    *playing = !*playing;
                }
                ui.spacing_mut().slider_width = 220.0;
                ui.add(egui::Slider::new(step, 0.0..=f64::from(total)).integer());
            });
            ui.end_row();
            if *playing {
                let dt = ui.input(|i| i.stable_dt).min(0.1);
                *step = (*step + f64::from(dt) * 24.0) % (f64::from(total) + 1.0);
                ui.ctx().request_repaint();
            }
            // Tracks: a row per keyed joint value, its keys on a strip under the steps.
            let now = step.round() as u32;
            let mut remove: Option<usize> = None;
            for (ti, tr) in tracks.iter_mut().enumerate() {
                let labels: Vec<String> = moving.iter().map(|(_, n, _)| n.clone()).collect();
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let mut pos = moving.iter().position(|(i, _, _)| *i == tr.joint).unwrap_or(0);
                row_label(ui, &format!("Track {}", ti + 1));
                ui.horizontal(|ui| {
                    combo(ui, &format!("ms_joint{ti}"), &refs, &mut pos);
                    if ui.small_button("×").on_hover_text("Remove the track").clicked() {
                        remove = Some(ti);
                    }
                });
                ui.end_row();
                if let Some((ji, _, kind)) = moving.get(pos) {
                    tr.joint = *ji;
                    let dofs = kind.dofs();
                    if dofs.len() > 1 {
                        row_label(ui, "  Of");
                        let caps: Vec<&str> = dofs.to_vec();
                        combo(ui, &format!("ms_idx{ti}"), &caps, &mut tr.index);
                        ui.end_row();
                    } else {
                        tr.index = 0;
                    }
                }
                row_label(ui, "  Keys");
                key_strip(ui, tr, total, now, &t);
                ui.end_row();
                // The key at this step, if any: its value.
                if let Some(p) = tr.points.iter_mut().find(|(s, _)| *s == now) {
                    // The key's value (Enter commits it; OK makes the study).
                    row_label(ui, "Value");
                    let _ = field(ui, &mut p.1);
                    ui.end_row();
                }
                row_label(ui, "");
                ui.horizontal(|ui| {
                    if ui.small_button("Key here").on_hover_text("Add a key at this step with the joint's value now").clicked()
                        && !tr.points.iter().any(|(s, _)| *s == now)
                    {
                        let unit = app
                            .session
                            .doc
                            .assembly
                            .joints
                            .get(tr.joint)
                            .map(|j| if j.kind.is_angle(tr.index) { "deg" } else { "mm" })
                            .unwrap_or("mm");
                        tr.points.push((now, format!("0 {unit}")));
                        tr.points.sort_by_key(|p| p.0);
                    }
                    if ui.small_button("Delete key").clicked() {
                        tr.points.retain(|(s, _)| *s != now);
                    }
                });
                ui.end_row();
            }
            if let Some(i) = remove {
                tracks.remove(i);
            }
            row_label(ui, "");
            if ui.button("Add Key").on_hover_text("Key a joint's value at this step").clicked()
                && let Some((ji, _, kind)) = moving.first()
            {
                let unit = if kind.is_angle(0) { "deg" } else { "mm" };
                tracks.push(Track { joint: *ji, index: 0, points: vec![(now, format!("0 {unit}"))] });
            }
            ui.end_row();
            // Show the pose at this step (again when the step or the keys change).
            let params = study_params(&app.session, name, study.as_deref(), steps, tracks);
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                format!("{params:?}").hash(&mut h);
                h.finish()
            };
            if *shown != Some((u64::from(now), key)) {
                match params.and_then(|p| study_pose(app, &p, now)) {
                    Ok(m) => {
                        display(app, saved, m);
                        *error = None;
                    }
                    Err(e) => *error = Some(e),
                }
                *shown = Some((u64::from(now), key));
            }
            if let Some(e) = error {
                row_label(ui, "");
                ui.label(RichText::new(e.as_str()).color(t.text_dim));
                ui.end_row();
            }
        }
        Mo::Explode { name, scale, shown, saved, error } => {
            row_label(ui, "Name");
            ui.text_edit_singleline(name);
            ui.end_row();
            row_label(ui, "Explode");
            ui.spacing_mut().slider_width = 160.0;
            ui.add(egui::Slider::new(scale, 0.0..=2.0).fixed_decimals(2));
            ui.end_row();
            let key = ((*scale * 1000.0).round() as u64, 0);
            if *shown != Some(key) {
                match explode_pose(app, *scale) {
                    Ok(m) => {
                        display(app, saved, m);
                        *error = None;
                    }
                    Err(e) => *error = Some(e),
                }
                *shown = Some(key);
            }
            if let Some(e) = error {
                row_label(ui, "");
                ui.label(RichText::new(e.as_str()).color(t.error));
                ui.end_row();
            }
        }
        Mo::Configurations { new_row, error } => {
            let v = app.session.execute("FusionShowDesignConfigPanelCmd", &json!({})).unwrap_or(Value::Null);
            let mut tb = table(&v);
            // Bare numbers show with their parameter's unit ("40" is 40 mm), as Fusion shows them.
            let kinds = app.session.doc.all_param_exprs();
            for (_, vals) in &mut tb.rows {
                for (c, val) in tb.columns.iter().zip(vals.iter_mut()) {
                    let (kind, n) = column_parts(c);
                    if kind == "param"
                        && val.trim().parse::<f64>().is_ok()
                        && let Some(unit) = kinds.iter().find(|x| x.0 == n).and_then(|x| unit_of(x.2))
                    {
                        *val = format!("{} {unit}", val.trim());
                    }
                }
            }
            let run = |app: &mut SolveApp, error: &mut Option<String>, id: &str, p: Value| {
                *error = app.run(id, p).err();
            };
            // Header.
            ui.label(RichText::new("Configuration").strong());
            ui.horizontal(|ui| {
                for c in &tb.columns {
                    let (kind, n) = column_parts(c);
                    ui.add_sized(vec2(96.0, 18.0), egui::Label::new(RichText::new(n).strong()).truncate())
                        .on_hover_text(format!("{} {n}", if kind == "suppress" { "Suppress" } else { "Parameter" }));
                    if ui.small_button("×").on_hover_text("Remove the column").clicked() {
                        let key = if kind == "suppress" { "feature" } else { "param" };
                        run(app, error, "config.column", json!({ key: n, "delete": true }));
                    }
                }
            });
            ui.end_row();
            for (name, vals) in &tb.rows {
                let active = tb.active.as_deref() == Some(name.as_str());
                ui.horizontal(|ui| {
                    ui.label(if active { RichText::new(name).strong() } else { RichText::new(name) });
                    if active {
                        ui.label(RichText::new("Active").color(t.accent));
                    } else if ui.button("Activate").clicked() {
                        run(app, error, "config.activate", json!({ "row": name }));
                    }
                });
                ui.horizontal(|ui| {
                    for (c, val) in tb.columns.iter().zip(vals) {
                        let id = egui::Id::new(("cfg", name, c));
                        let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| val.clone());
                        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(96.0));
                        if r.changed() {
                            ui.data_mut(|d| d.insert_temp(id, text.clone()));
                        }
                        if r.lost_focus() {
                            ui.data_mut(|d| d.remove::<String>(id));
                            if text != *val {
                                let (_, n) = column_parts(c);
                                run(app, error, "config.row", json!({"name": name, "values": { n: cell_value(c, &text) }}));
                            }
                        }
                        ui.add_space(18.0);
                    }
                    if ui.small_button("×").on_hover_text("Delete the row").clicked() {
                        run(app, error, "config.row", json!({"name": name, "delete": true}));
                    }
                });
                ui.end_row();
            }
            // New row (a copy of the active one) and new column.
            ui.add(egui::TextEdit::singleline(new_row).hint_text("new configuration").desired_width(130.0));
            ui.horizontal(|ui| {
                if ui.button("Add Row").on_hover_text("A new configuration, a copy of the active one").clicked() {
                    let name = if new_row.trim().is_empty() {
                        (tb.rows.len() + 1..).map(|k| format!("Configuration{k}")).find(|n| !tb.rows.iter().any(|r| &r.0 == n)).unwrap_or_default()
                    } else {
                        new_row.trim().to_string()
                    };
                    let mut p = json!({ "name": name });
                    if let Some(a) = &tb.active {
                        p["from"] = json!(a);
                    }
                    run(app, error, "config.row", p);
                    new_row.clear();
                }
                // A column: a parameter's value or a feature's suppression.
                let mut options: Vec<(String, Value)> = Vec::new();
                for p in app.session.doc.all_param_exprs() {
                    if !tb.columns.contains(&format!("param:{}", p.0)) {
                        options.push((format!("{} (parameter)", p.0), json!({ "param": p.0 })));
                    }
                }
                for f in &app.session.doc.features {
                    if !tb.columns.contains(&format!("suppress:{}", f.name)) {
                        options.push((format!("Suppress {}", f.name), json!({ "feature": f.name })));
                    }
                }
                let mut pick: Option<Value> = None;
                ui.menu_button("Add Column", |ui| {
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        for (label, p) in &options {
                            if ui.button(label.as_str()).clicked() {
                                pick = Some(p.clone());
                                ui.close();
                            }
                        }
                    });
                });
                if let Some(p) = pick {
                    // The first column starts the table (with a Default row of the current values).
                    let (id, p) = if tb.columns.is_empty() {
                        (
                            "FusionStartDesignConfigModeCmd",
                            match p.get("param") {
                                Some(n) => json!({ "params": [n] }),
                                None => json!({ "features": [p.get("feature")] }),
                            },
                        )
                    } else {
                        ("config.column", p)
                    };
                    run(app, error, id, p);
                }
            });
            ui.end_row();
            if let Some(e) = error {
                ui.label("");
                ui.label(RichText::new(e.as_str()).color(t.error));
                ui.end_row();
            }
        }
    }
    let _ = inputs;
    enter
}

/// A JSON name or id as text.
fn name_of(v: &Value) -> String {
    v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())
}

/// A track's keys as diamonds over the steps, with the playhead.
fn key_strip(ui: &mut egui::Ui, tr: &Track, total: u32, now: u32, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(240.0, 16.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 3.0, t.field);
    let x = |s: u32| r.left() + 6.0 + (r.width() - 12.0) * s as f32 / total.max(1) as f32;
    // Keys joined by a line (the motion between them).
    let pts: Vec<Pos2> = tr.points.iter().map(|(s, _)| pos2(x(*s), r.center().y)).collect();
    if pts.len() > 1 {
        p.add(egui::Shape::line(pts.clone(), Stroke::new(1.5, t.accent.gamma_multiply(0.6))));
    }
    for q in pts {
        p.add(egui::Shape::convex_polygon(
            vec![q + vec2(0.0, -5.0), q + vec2(5.0, 0.0), q + vec2(0.0, 5.0), q + vec2(-5.0, 0.0)],
            t.accent,
            Stroke::new(1.0, Color32::WHITE),
        ));
    }
    let px = x(now);
    p.line_segment([pos2(px, r.top()), pos2(px, r.bottom())], Stroke::new(1.5, t.warning));
}

/// The commands OK runs.
pub fn commands(app: &SolveApp, k: &Mo, inputs: &[SelInput]) -> Result<Vec<(String, Value)>, String> {
    Ok(match k {
        Mo::ContactSet { name, .. } => {
            let mut occs: Vec<u64> = Vec::new();
            for x in inputs.first().map(|i| i.items.as_slice()).unwrap_or(&[]) {
                if let solvecraft_engine::Sel::Body { name } = x
                    && let Some(b) = app.session.world_state().body(name)
                {
                    let comp = app.session.doc.body_component(&b.name, b.feature);
                    if let Some(o) = app.session.doc.occurrence_of(comp).map(|o| o.id)
                        && !occs.contains(&o)
                    {
                        occs.push(o);
                    }
                }
            }
            if occs.is_empty() {
                // Only managing the sets: nothing to add.
                return Ok(Vec::new());
            }
            vec![("ContactSetCmd".into(), json!({"name": name, "occurrences": occs}))]
        }
        Mo::MotionStudy { name, study, steps, tracks, .. } => {
            vec![("FusionMotionStudyCommand".into(), study_params(&app.session, name, study.as_deref(), steps, tracks)?)]
        }
        Mo::Explode { name, scale, .. } => {
            vec![("explode.create".into(), json!({"name": name, "scale": scale})), ("explode.show".into(), json!({ "view": name }))]
        }
        Mo::Configurations { .. } => Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a dialog shows goes away when it closes; an exploded view applied with OK stays.
    #[test]
    fn shown_poses_are_put_back() {
        let mut app = SolveApp::new(Session::default(), crate::Services::default());
        app.session.pending_moves.insert(9, solvecraft_engine::doc::IDENTITY);
        let before = app.session.pending_moves.clone();
        let mut m = solvecraft_engine::doc::IDENTITY;
        m[3] = [5.0, 0.0, 0.0, 1.0];
        let shown: BTreeMap<u64, Mat> = [(3, m)].into_iter().collect();
        for (kind, applied, kept) in [("study", true, false), ("explode", false, false), ("explode", true, true)] {
            let mut saved = None;
            display(&mut app, &mut saved, shown.clone());
            assert_eq!(app.session.pending_moves, shown);
            let mo = if kind == "study" {
                Mo::MotionStudy {
                    name: "S".into(),
                    study: None,
                    steps: "10".into(),
                    tracks: vec![],
                    step: 0.0,
                    playing: false,
                    shown: None,
                    saved,
                    error: None,
                }
            } else {
                Mo::Explode { name: "E".into(), scale: 1.0, shown: None, saved, error: None }
            };
            let mut d = Dialog::new(Kind::Motion(mo), vec![]);
            closed(&mut app, &mut d, applied);
            assert_eq!(app.session.pending_moves == shown, kept, "{kind} {applied}");
            app.session.pending_moves = before.clone();
        }
    }

    #[test]
    fn configuration_table_and_cells() {
        let v = json!({"columns": ["param:width", "suppress:Fillet1"], "rows": [
            {"name": "Default", "values": {"param:width": "80 mm", "suppress:Fillet1": false}},
            {"name": "Long", "values": {"param:width": "120 mm", "suppress:Fillet1": true}},
        ], "active": "Long"});
        let t = table(&v);
        assert_eq!(t.columns.len(), 2);
        assert_eq!(t.rows[1], ("Long".to_string(), vec!["120 mm".to_string(), "suppressed".to_string()]));
        assert_eq!(t.active.as_deref(), Some("Long"));
        assert_eq!(column_parts("suppress:Fillet1"), ("suppress", "Fillet1"));
        assert_eq!(cell_value("suppress:Fillet1", "Suppressed"), json!(true));
        assert_eq!(cell_value("suppress:Fillet1", "on"), json!(false));
        assert_eq!(cell_value("param:width", "90 mm"), json!("90 mm"));
    }

    #[test]
    fn study_keys_by_joint_id() {
        let mut s = Session::default();
        for (name, x) in [("A", 0), ("B", 30)] {
            s.execute("component.activate", &json!({"component": "root"})).unwrap();
            s.execute("FusionCreateNewComponentCommand", &json!({ "name": name })).unwrap();
            s.execute("PrimitiveBox", &json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
        }
        s.execute(
            "JointAssembleCmdNew",
            &json!({"type": "revolute", "a": {"occurrence": "A:1", "face": [10, 10, 10]}, "b": {"occurrence": "B:1", "face": [40, 10, 0]}}),
        )
        .unwrap();
        let tracks = vec![Track { joint: 0, index: 0, points: vec![(0, "0 deg".into()), (30, "90 deg".into())] }];
        let p = study_params(&s, "Study1", None, "60", &tracks).unwrap();
        assert_eq!(p["steps"], 60);
        assert_eq!(p["keys"][0]["joint"], s.doc.assembly.joints[0].id);
        assert_eq!(p["keys"][0]["points"], json!([[0, "0 deg"], [30, "90 deg"]]));
        assert!(study_params(&s, "S", None, "x", &tracks).is_err());
        assert!(study_params(&s, "S", None, "10", &[]).is_err());
    }
}
