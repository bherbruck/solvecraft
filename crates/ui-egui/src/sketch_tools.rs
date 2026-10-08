//! More interactive sketch tools: projection and other model-pick tools, curve tools (trim,
//! extend, break, offset, fillet, chamfer), multi-pick tools (mirror, patterns, polygon), point
//! tools for free-form curves and slots, text placement, and constraint glyphs. Like `tools`,
//! these are UI state only: every action runs a command.

use std::cell::{Cell, RefCell};

use egui::{Pos2, Stroke};
use serde_json::{Value, json};
use solvecraft_engine::geom::{Vec2, Vec3};

use crate::SolveApp;
use crate::tools::{Kind, Tool};
use crate::viewport::{Hit, Proj, pick, sketch_point_at};

thread_local! {
    /// A placed text waiting for its string (position, text so far).
    static TEXT: RefCell<Option<(Vec2, String)>> = const { RefCell::new(None) };
    /// Shift held this frame.
    static SHIFT: Cell<bool> = const { Cell::new(false) };
    /// Glyphs drawn this frame (constraint id, screen centre) and the selected one.
    static GLYPHS: RefCell<Vec<(String, Pos2)>> = const { RefCell::new(Vec::new()) };
    static PICKED: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The inference the cursor snapped to last (label, sketch point), for the hint.
    static SNAP: RefCell<Option<(&'static str, Vec2)>> = const { RefCell::new(None) };
    static PREV: Cell<Option<Vec2>> = const { Cell::new(None) };
    /// Curvature comb teeth (foot, tip) shown while the comb tool is active.
    static COMBS: RefCell<Vec<Vec<[Vec3; 2]>>> = const { RefCell::new(Vec::new()) };
    /// The image file a Canvas or Decal tool places.
    static IMAGE: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Analysis markers shown while an analysis tool is active: (where, label).
    static MARKS: RefCell<Vec<(Vec3, String)>> = const { RefCell::new(Vec::new()) };
    /// Center of Mass has just started: measure everything on the next frame.
    static DEFERRED: Cell<bool> = const { Cell::new(false) };
    /// The canvas whose panel is open.
    static CANVAS_PANEL: Cell<Option<u64>> = const { Cell::new(None) };
    /// The opacity slider while it is dragged: (canvas, value).
    static OPACITY: Cell<Option<(u64, f64)>> = const { Cell::new(None) };
}

/// Run an analysis and keep its result as a marker.
fn analysis(app: &mut SolveApp, cmd: &str, p: Value) {
    let Ok(v) = app.run(cmd, p) else { return };
    let mark = match cmd {
        "FusionCenterOfMassCommand" => v3(&v["center"]).map(|c| (c, "Center of mass".to_string())),
        _ => v3(&v["at"]).zip(v["min_radius"].as_f64()).map(|(c, r)| (c, format!("R min {r:.3} mm"))),
    };
    if let Some(m) = mark {
        MARKS.with(|k| k.borrow_mut().push(m));
    } else if cmd == "FusionMinimumRadiusAnalysisCommand" {
        app.set_status("Minimum radius: it is straight (no curvature)", false);
    }
}

/// Analysis markers: a target with its label.
fn marks(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if !app.tool.as_ref().is_some_and(|t| matches!(t.cmd, "FusionMinimumRadiusAnalysisCommand" | "FusionCenterOfMassCommand")) {
        MARKS.with(|m| m.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    MARKS.with(|m| {
        for (at, label) in m.borrow().iter() {
            let Some(q) = proj.to_screen(*at) else { continue };
            painter.circle(q, 6.0, tk.panel.gamma_multiply(0.8), Stroke::new(1.6, tk.accent));
            painter.line_segment([q - egui::vec2(10.0, 0.0), q + egui::vec2(10.0, 0.0)], Stroke::new(1.2, tk.accent));
            painter.line_segment([q - egui::vec2(0.0, 10.0), q + egui::vec2(0.0, 10.0)], Stroke::new(1.2, tk.accent));
            let r = painter.text(q + egui::vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(12.0), tk.text);
            painter.rect_filled(r.expand(3.0), 3.0, tk.panel.gamma_multiply(0.85));
            painter.text(q + egui::vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, label, egui::FontId::proportional(12.0), tk.text);
        }
    });
}

/// Commands that start with a step of their own: Canvas and Decal ask for the image first,
/// Edit Canvas opens the canvas panel. False when the command should not go on.
pub fn start_hook(app: &SolveApp, id: &str) -> bool {
    if id == "FusionCenterOfMassCommand" {
        // The whole model at once; the tool stays for picking single bodies.
        MARKS.with(|m| m.borrow_mut().clear());
        DEFERRED.with(|d| d.set(true));
        return true;
    }
    if id == "canvas.edit" {
        let first = app.session.doc.canvases.first().map(|c| c.id);
        CANVAS_PANEL.with(|c| c.set(first));
        return false;
    }
    if !matches!(id, "FusionAddCanvasCommand" | "FusionAddEditDecalCommand") {
        return true;
    }
    let path = app.services.pick_open.as_ref().and_then(|f| f());
    let ok = path.is_some();
    IMAGE.with(|i| *i.borrow_mut() = path);
    ok
}

/// The canvas panel: opacity, size, angle, flip, visibility of one canvas.
fn canvas_panel(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(id) = CANVAS_PANEL.with(|c| c.get()) else { return };
    let Some(c) = app.session.doc.canvases.iter().find(|c| c.id == id).cloned() else {
        CANVAS_PANEL.with(|c| c.set(None));
        return;
    };
    let ids: Vec<(u64, String)> = app.session.doc.canvases.iter().map(|c| (c.id, c.name.clone())).collect();
    let mut open = true;
    let mut edit: Option<Value> = None;
    let mut delete = false;
    egui::Window::new("Canvas").open(&mut open).collapsible(false).resizable(false).default_pos(egui::pos2(320.0, 160.0)).show(ctx, |ui| {
        egui::ComboBox::from_id_salt("sc_canvas_pick").selected_text(&c.name).show_ui(ui, |ui| {
            for (cid, name) in &ids {
                if ui.selectable_label(*cid == id, name).clicked() {
                    CANVAS_PANEL.with(|p| p.set(Some(*cid)));
                }
            }
        });
        egui::Grid::new("sc_canvas_grid").num_columns(2).show(ui, |ui| {
            ui.label("Opacity");
            let mut o = OPACITY.with(|v| v.get()).filter(|v| v.0 == id).map(|v| v.1).unwrap_or(c.opacity);
            let r = ui.add(egui::Slider::new(&mut o, 0.0..=1.0).fixed_decimals(2));
            OPACITY.with(|v| v.set(Some((id, o))));
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                edit = Some(json!({"canvas": id, "opacity": o}));
            }
            ui.end_row();
            ui.label("Width");
            let mut w = c.width;
            if ui.add(egui::DragValue::new(&mut w).range(0.001..=1e6).suffix(" mm").speed(c.width * 0.005)).changed() {
                edit = Some(json!({"canvas": id, "width": w}));
            }
            ui.end_row();
            ui.label("Angle");
            let mut a = c.angle.to_degrees();
            if ui.add(egui::DragValue::new(&mut a).suffix("°").speed(0.5)).changed() {
                edit = Some(json!({"canvas": id, "angle": a}));
            }
            ui.end_row();
            let mut flip = c.flip;
            if ui.checkbox(&mut flip, "Flip").changed() {
                edit = Some(json!({"canvas": id, "flip": flip}));
            }
            let mut vis = c.visible;
            if ui.checkbox(&mut vis, "Visible").changed() {
                edit = Some(json!({"canvas": id, "visible": vis}));
            }
            ui.end_row();
        });
        if ui.button("Delete canvas").clicked() {
            delete = true;
        }
    });
    if let Some(p) = edit {
        let _ = app.run("canvas.edit", p);
        OPACITY.with(|v| v.set(None));
    }
    if delete {
        let _ = app.run("canvas.delete", json!({"canvas": id}));
        open = false;
    }
    if !open {
        CANVAS_PANEL.with(|c| c.set(None));
    }
}

fn v3(v: &Value) -> Option<Vec3> {
    Some(Vec3::new(v.get(0)?.as_f64()?, v.get(1)?.as_f64()?, v.get(2)?.as_f64()?))
}

/// Curvature combs: teeth and the envelope through their tips.
fn combs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.as_ref().map(|t| t.cmd) != Some("FusionCurvatureCombAnalysisCommand") {
        COMBS.with(|c| c.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    COMBS.with(|c| {
        for comb in c.borrow().iter() {
            let mut tips: Vec<Pos2> = Vec::new();
            for [a, b] in comb {
                let (Some(p), Some(q)) = (proj.to_screen(*a), proj.to_screen(*b)) else { continue };
                painter.line_segment([p, q], Stroke::new(1.0, tk.warning.gamma_multiply(0.8)));
                tips.push(q);
            }
            painter.add(egui::Shape::line(tips, Stroke::new(1.4, tk.warning)));
        }
    });
}

/// Snap a cursor point on the sketch plane to what Fusion infers while drawing: a line's
/// midpoint, or alignment (horizontal/vertical) with the previous point of the tool. None:
/// nothing near, use the grid.
pub fn refine_snap(
    app: &SolveApp,
    proj: &Proj,
    sk: &solvecraft_engine::sketch::Sketch,
    plane: &solvecraft_engine::geom::Plane,
    lp: Vec2,
) -> Option<(Vec2, Option<String>)> {
    crate::inference::clear();
    // Millimetres per pixel here.
    let (a, b) = (proj.to_screen(plane.to_world(lp))?, proj.to_screen(plane.to_world(lp + Vec2::X))?);
    let px = 1.0 / (a.distance(b) as f64).max(1e-9);
    let tol = 7.0 * px;
    // A model vertex under the cursor: drawing on it projects it into the sketch.
    for h in pick(app, proj, a) {
        if let Hit::Vertex { point, .. } = h {
            SNAP.with(|s| *s.borrow_mut() = Some(("Pt", plane.to_local(point))));
            return Some((plane.to_local(point), Some(format!("vertex:{},{},{}", point.x, point.y, point.z))));
        }
    }
    let mut best: Option<(f64, &'static str, Vec2)> = None;
    let mut mid_of: Option<String> = None;
    for (i, c) in sk.curves.iter().enumerate() {
        if let solvecraft_engine::sketch::CurveKind::Line { .. } = c.kind
            && let Some(solvecraft_engine::sketch::Shape::Line { a, b }) = sk.shape(i)
        {
            let m = (a + b) * 0.5;
            let d = m.dist(lp);
            if d < tol && best.is_none_or(|x| d < x.0) {
                best = Some((d, "Mid", m));
                mid_of = Some(c.id.clone());
            }
        }
    }
    // The tool's previous point (a click takes the tool out of the app while it runs, so the
    // last hover's is kept).
    let prev = match app.tool.as_ref() {
        Some(t) => {
            let p = t.pts.last().map(|p| p.0);
            PREV.with(|c| c.set(p));
            p
        }
        None => PREV.with(Cell::get),
    };
    // Line tool: the tangent point of a circle or arc (before plain "on the curve").
    let line_tool = app.tool.as_ref().is_none_or(|t| t.cmd == "DrawPolyline");
    if best.is_none()
        && line_tool
        && let Some(prev) = prev
        && let Some(i) = crate::inference::tangent(sk, prev, lp, tol)
    {
        best = Some((i.dist, i.label(), i.at));
    }
    // On a curve (coincident with it).
    if best.is_none() {
        for (i, c) in sk.curves.iter().enumerate() {
            let Some(sh) = sk.shape(i) else { continue };
            let d = sh.dist(lp);
            if d < tol * 0.8 && best.is_none_or(|x| d < x.0) {
                let q = sh.project(lp);
                best = Some((d, "On", q));
                mid_of = Some(format!("{}:{},{}", c.id, q.x, q.y));
            }
        }
    }
    if best.is_none()
        && let Some(prev) = prev
    {
        let (dx, dy) = ((lp.x - prev.x).abs(), (lp.y - prev.y).abs());
        // The free coordinate still follows the grid.
        let step = solvecraft_engine::view::grid_step(app.cam.half_height()).0 / 10.0;
        let g = |v: f64| (v / step).round() * step;
        if dy < tol && dx > tol {
            best = Some((dy, "H", Vec2::new(g(lp.x), prev.y)));
        } else if dx < tol && dy > tol {
            best = Some((dx, "V", Vec2::new(prev.x, g(lp.y))));
        }
    }
    // Line tool: parallel or perpendicular to a line, or on a line's extension.
    if best.is_none()
        && line_tool
        && let Some(prev) = prev
        && let Some(i) = crate::inference::lines(sk, prev, lp, tol)
    {
        best = Some((i.dist, i.label(), i.at));
    }
    SNAP.with(|s| *s.borrow_mut() = best.map(|(_, l, p)| (l, p)));
    // A midpoint snap is a point argument the commands understand ("mid:<line>").
    best.map(|(_, l, p)| {
        let arg = match l {
            "Mid" => mid_of.map(|m| format!("mid:{m}")),
            "On" => mid_of.map(|m| format!("on:{m}")),
            _ => None,
        };
        (p, arg)
    })
}

/// The constraint glyph under a screen position (its constraint id).
pub fn glyph_at(pos: Pos2) -> Option<String> {
    GLYPHS.with(|g| g.borrow().iter().find(|(_, c)| c.distance(pos) <= 7.0).map(|(id, _)| id.clone()))
}

/// Constraint glyphs drawn last frame: (constraint id, screen centre).
pub fn glyph_positions() -> Vec<(String, Pos2)> {
    GLYPHS.with(|g| g.borrow().clone())
}

/// A click on a constraint glyph selects it (true: the click was used).
pub fn click_glyph(app: &mut SolveApp, _proj: &Proj, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() {
        return false;
    }
    let hit = GLYPHS.with(|g| g.borrow().iter().find(|(_, c)| c.distance(pos) <= 7.0).map(|(id, _)| id.clone()));
    let used = hit.is_some();
    PICKED.with(|p| *p.borrow_mut() = hit.clone());
    if let Some(id) = hit {
        app.set_status(format!("Constraint {id} selected: Delete removes it"), false);
    }
    used
}

/// Delete the selected constraint glyph (true when one was selected).
pub fn delete_glyph(app: &mut SolveApp) -> bool {
    let Some(id) = PICKED.with(|p| p.borrow_mut().take()) else { return false };
    let _ = app.run("sketch.delete", json!({"entities": [id]}));
    true
}

/// What the extra tools do with clicks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Each click on model geometry runs the command with that reference.
    ModelRef,
    /// Click a curve of the active sketch where it should change.
    CurveAt,
    /// Click points; `n` complete the command (0: until right-click).
    Points(usize),
    /// Pick sketch entities; `n` run the command (0: until right-click).
    Entities(usize),
    /// Pick entities until right-click, then one more pick or point (mirror line, centre).
    EntitiesThen,
    /// Model faces / sketch curves in steps (see the command).
    Steps,
}

fn mode(id: &str) -> Option<Mode> {
    Some(match id {
        "ProjectNewCmd" | "IntersectCmd" | "Include3DGeometry" | "FitCurvesToSectionCommand" | "SketchIsoparametricCurve" => Mode::ModelRef,
        "FusionCurvatureCombAnalysisCommand" | "FusionAddCanvasCommand" | "FusionAddEditDecalCommand" => Mode::ModelRef,
        "FusionMinimumRadiusAnalysisCommand" | "FusionCenterOfMassCommand" => Mode::ModelRef,
        "TrimSketchCmd" | "ExtendSketchCmd" | "BreakSketchCmd" => Mode::CurveAt,
        "SketchMidpointLine" => Mode::Points(2),
        "ArcTangent" => Mode::Points(2),
        "CircleElipse" | "ConicCurveCmd" | "ShapeSlotCenterPoint" => Mode::Points(3),
        "ShapeArcSlotThreePoint" | "ShapeArcSlotCenterTwoPoint" => Mode::Points(4),
        "DrawSpline" | "DrawCVMSpline3D" | "DrawCVMSpline5D" => Mode::Points(0),
        "MTextCmd" => Mode::Points(1),
        "BlendG1CurveSketchCmd" => Mode::Points(2),
        "Offset" => Mode::Entities(1),
        "FilletSketchCmd" | "ChamferSketchEqualDistance" | "ChamferSketchDistanceAngle" | "ChamferSketchDistanceDistance" => Mode::Entities(1),
        "ConstraintSmooth" | "CircleTanTanRadius" => Mode::Entities(2),
        "CircleThreeTangent" => Mode::Entities(3),
        "SketchConstrainer" | "SketchPolygonConstraintCmd" => Mode::Entities(0),
        "sketch.centerline" => Mode::Entities(1),
        "MirrorSketchCommand" | "CircularSketchPatternCommand" | "RectangularSketchPatternCommand" => Mode::EntitiesThen,
        "ProjectToSurface" | "IntersectionCurve" | "SpunProfileCmd" => Mode::Steps,
        _ => return None,
    })
}

/// A tool for one of the extra sketch commands.
pub fn tool_for(id: &str) -> Option<Tool> {
    mode(id)?;
    let spec = solvecraft_engine::find_command(id)?;
    Some(Tool {
        cmd: spec.id,
        kind: Kind::Ext,
        pts: Vec::new(),
        hover: None,
        picks: Vec::new(),
        dims: Vec::new(),
        dims_stage: usize::MAX,
        dims_focus: false,
    })
}

pub fn hint(id: &str) -> Option<String> {
    Some(
        match id {
            "ProjectNewCmd" => "Project: click edges, faces, vertices, other sketches' curves, axes or planes",
            "IntersectCmd" => "Intersect: click faces or edges to cut with the sketch plane",
            "Include3DGeometry" => "Include 3D Geometry: click edges or vertices",
            "FitCurvesToSectionCommand" => "Fit Curves to Mesh Section: click a body",
            "SketchIsoparametricCurve" => "Isoparametric Curve: click a point on a face (Shift: along)",
            "FusionCurvatureCombAnalysisCommand" => "Curvature comb: click sketch curves or model edges",
            "FusionMinimumRadiusAnalysisCommand" => "Minimum radius: click sketch curves, edges or faces",
            "FusionCenterOfMassCommand" => "Center of mass of all bodies; click a body for its own",
            "FusionAddCanvasCommand" => "Canvas: click a plane or a planar face where the image's centre goes",
            "FusionAddEditDecalCommand" => "Decal: click a planar face where the image's centre goes",
            "TrimSketchCmd" => "Trim: click the piece of a curve to remove",
            "ExtendSketchCmd" => "Extend: click a curve near the end to extend",
            "BreakSketchCmd" => "Break: click a curve where it should split",
            "SketchMidpointLine" => "Midpoint line: click the middle, then an end",
            "ArcTangent" => "Tangent arc: click the end of a line or arc, then the arc's end",
            "CircleElipse" => "Ellipse: click the centre, the end of the major axis, then a point on it",
            "ConicCurveCmd" => "Conic: click the start, the end, then the apex",
            "ShapeSlotCenterPoint" => "Slot: click the centre, an arc centre, then the width",
            "ShapeArcSlotThreePoint" => "Arc slot: click start, a point on the arc, the end, then the width",
            "ShapeArcSlotCenterTwoPoint" => "Arc slot: click the centre, the start, the end, then the width",
            "DrawSpline" => "Spline: click fit points; right-click to finish",
            "DrawCVMSpline3D" | "DrawCVMSpline5D" => "Control point spline: click control points; right-click to finish",
            "MTextCmd" => "Text: click where the text starts",
            "BlendG1CurveSketchCmd" => "Blend curve: click the ends of two curves",
            "Offset" => "Offset: click a curve (its chain is offset), then the side and distance",
            "FilletSketchCmd" => "Fillet: click the corner of two lines",
            "ChamferSketchEqualDistance" | "ChamferSketchDistanceAngle" | "ChamferSketchDistanceDistance" => "Chamfer: click the corner of two lines",
            "ConstraintSmooth" => "Curvature: click two curves that share an end",
            "CircleTanTanRadius" => "2-tangent circle: click two curves (the circle goes near the second click)",
            "CircleThreeTangent" => "3-tangent circle: click three curves",
            "SketchConstrainer" => "Constrain: click one or two entities; right-click to apply",
            "SketchPolygonConstraintCmd" => "Polygon: click the lines of a closed chain; right-click to apply",
            "sketch.centerline" => "Centerline: click lines",
            "MirrorSketchCommand" => "Mirror: click entities, right-click, then click the mirror line",
            "CircularSketchPatternCommand" => "Circular pattern: click entities, right-click, then click the centre",
            "RectangularSketchPatternCommand" => "Rectangular pattern: click entities, right-click, then click two points (direction and spacing)",
            "ProjectToSurface" => "Project to surface: click curves of other sketches, right-click, then click the face",
            "IntersectionCurve" => "Intersection curve: click two faces",
            "SpunProfileCmd" => "Spun profile: click the body, then the axis line",
            _ => return None,
        }
        .to_string(),
    )
}

fn active_sketch(app: &SolveApp) -> Option<u64> {
    app.session.active_sketch
}

/// A model reference for a hit (for Project and friends).
fn model_ref(app: &SolveApp, h: &Hit) -> Option<Value> {
    let active = active_sketch(app);
    Some(match h {
        Hit::Vertex { body, point } => json!({"vertex": [point.x, point.y, point.z], "body": body}),
        Hit::Edge { body, mid, .. } => json!({"edge": [mid.x, mid.y, mid.z], "body": body}),
        Hit::Face { body, point, .. } => json!({"face": [point.x, point.y, point.z], "body": body}),
        Hit::SketchCurve { sketch, id, .. } if Some(*sketch) != active => json!({"sketch": sketch, "curve": id}),
        Hit::SketchPoint { sketch, id, .. } if Some(*sketch) != active => json!({"sketch": sketch, "point": id}),
        Hit::Axis { name } => json!({"axis": name}),
        Hit::Plane { name, .. } => json!({"plane": name}),
        _ => return None,
    })
}

/// The active sketch's curve or point under the cursor.
fn sketch_entity(app: &SolveApp, hits: &[Hit]) -> Option<(String, bool)> {
    let active = active_sketch(app)?;
    hits.iter().find_map(|h| match h {
        Hit::SketchPoint { sketch, id, .. } if *sketch == active => Some((id.clone(), true)),
        Hit::SketchCurve { sketch, id, .. } if *sketch == active => Some((id.clone(), false)),
        _ => None,
    })
}

fn xy(p: Vec2) -> Value {
    json!([p.x, p.y])
}

fn arg(p: &(Vec2, Option<String>)) -> Value {
    match &p.1 {
        Some(id) => json!(id),
        None => xy(p.0),
    }
}

/// Length of a curve (for default sizes).
fn curve_len(app: &SolveApp, id: &str) -> f64 {
    let st = app.session.model.state();
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return 10.0 };
    ss.sketch.curve_index(id).map(|c| ss.sketch.polyline(c).windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).unwrap_or(10.0)
}

/// Lines meeting at a sketch point.
fn corner_len(app: &SolveApp, point: &str) -> f64 {
    let st = app.session.model.state();
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return 10.0 };
    let sk = &ss.sketch;
    let Some(q) = sk.resolve_point(point) else { return 10.0 };
    sk.curves
        .iter()
        .enumerate()
        .filter(|(_, c)| c.kind.uses(q) && matches!(c.kind, solvecraft_engine::sketch::CurveKind::Line { .. }))
        .map(|(i, _)| sk.polyline(i).windows(2).map(|w| w[0].dist(w[1])).sum::<f64>())
        .fold(f64::INFINITY, f64::min)
        .min(1e6)
}

fn round_nice(x: f64) -> f64 {
    if !(x.is_finite() && x > 0.0) {
        return 1.0;
    }
    let p = 10f64.powf(x.log10().floor());
    let m = x / p;
    let n = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    n * p
}

/// A click with an extra tool.
pub fn on_click(app: &mut SolveApp, tool: &mut Tool, proj: &Proj, pos: Pos2) {
    let Some(m) = mode(tool.cmd) else { return };
    let hits = pick(app, proj, pos);
    let at = sketch_point_at(app, proj, pos);
    let cmd = tool.cmd;
    match m {
        Mode::ModelRef if cmd == "FusionCurvatureCombAnalysisCommand" => {
            let params = hits.iter().find_map(|h| match h {
                Hit::SketchCurve { sketch, id, .. } => Some(json!({"sketch": sketch, "curves": [id]})),
                Hit::Edge { mid, .. } => Some(json!({"edges": [[mid.x, mid.y, mid.z]]})),
                _ => None,
            });
            if let Some(p) = params
                && let Ok(v) = app.run(cmd, p)
            {
                for comb in v["combs"].as_array().into_iter().flatten() {
                    let teeth: Vec<[Vec3; 2]> =
                        comb["teeth"].as_array().into_iter().flatten().filter_map(|t| Some([v3(t.get(0)?)?, v3(t.get(1)?)?])).collect();
                    COMBS.with(|c| c.borrow_mut().push(teeth));
                }
            }
        }
        Mode::ModelRef if matches!(cmd, "FusionMinimumRadiusAnalysisCommand" | "FusionCenterOfMassCommand") => {
            let params = hits.iter().find_map(|h| match (cmd, h) {
                ("FusionMinimumRadiusAnalysisCommand", Hit::SketchCurve { sketch, id, .. }) => Some(json!({"sketch": sketch, "curves": [id]})),
                ("FusionMinimumRadiusAnalysisCommand", Hit::Edge { mid, .. }) => Some(json!({"edges": [[mid.x, mid.y, mid.z]]})),
                ("FusionMinimumRadiusAnalysisCommand", Hit::Face { point, .. }) => Some(json!({"faces": [[point.x, point.y, point.z]]})),
                ("FusionCenterOfMassCommand", Hit::Face { body, .. }) => Some(json!({"bodies": [body]})),
                _ => None,
            });
            if let Some(p) = params {
                analysis(app, cmd, p);
            }
        }
        Mode::ModelRef if matches!(cmd, "FusionAddCanvasCommand" | "FusionAddEditDecalCommand") => {
            let Some(path) = IMAGE.with(|i| i.borrow().clone()) else { return };
            let decal = cmd == "FusionAddEditDecalCommand";
            let target = hits.iter().find_map(|h| match h {
                Hit::Face { point, .. } => Some((json!({"face": [point.x, point.y, point.z]}), *point)),
                Hit::Plane { name, point } if !decal => Some((json!(name), *point)),
                _ => None,
            });
            // Empty space while sketching: the sketch plane.
            let target = target.or_else(|| {
                let st = app.session.model.state();
                let ss = st.sketch(app.session.active_sketch?)?;
                let (o, d) = proj.ray(pos);
                let w = ss.plane.intersect_ray(o, d)?;
                let pl = &ss.plane;
                Some((
                    json!({"origin": [pl.origin.x, pl.origin.y, pl.origin.z], "x_dir": [pl.x.x, pl.x.y, pl.x.z], "y_dir": [pl.y.x, pl.y.y, pl.y.z]}),
                    w,
                ))
            });
            let Some((plane, at)) = target else { return };
            let p = if decal {
                json!({"path": path, "face": plane.get("face").cloned().unwrap_or(Value::Null)})
            } else {
                json!({"path": path, "plane": plane, "at": [at.x, at.y, at.z]})
            };
            if let Ok(v) = app.run(cmd, p) {
                IMAGE.with(|i| *i.borrow_mut() = None);
                app.tool = None;
                CANVAS_PANEL.with(|c| c.set(v["canvas"].as_u64()));
            }
        }
        Mode::ModelRef => {
            let r = hits.iter().find_map(|h| model_ref(app, h));
            let Some(r) = r else { return };
            match cmd {
                "FitCurvesToSectionCommand" => {
                    if let Some(b) = r.get("body").and_then(Value::as_str) {
                        let _ = app.run(cmd, json!({"body": b}));
                    }
                }
                "SketchIsoparametricCurve" => {
                    if r.get("face").is_some() {
                        let along = SHIFT.with(Cell::get);
                        let _ = app.run(cmd, json!({"face": r["face"], "body": r["body"], "direction": if along { "v" } else { "u" }}));
                    }
                }
                _ => {
                    let _ = app.run(cmd, json!({"refs": [r]}));
                }
            }
        }
        Mode::CurveAt => {
            if let (Some((id, false)), Some((p, _))) = (sketch_entity(app, &hits), at) {
                let _ = app.run(cmd, json!({"curve": id, "at": xy(p)}));
            }
        }
        Mode::Points(n) => {
            let Some(p) = at else { return };
            tool.pts.push(p);
            if n > 0 && tool.pts.len() >= n {
                run_points(app, tool);
                tool.pts.clear();
            }
        }
        Mode::Entities(n) => {
            if cmd == "Offset" && !tool.picks.is_empty() {
                if let Some((p, _)) = at {
                    offset_side(app, tool, p);
                }
                return;
            }
            let Some((id, is_point)) = sketch_entity(app, &hits) else { return };
            if cmd == "Offset" {
                // First the curve, then the side point.
                if tool.picks.is_empty() && !is_point {
                    tool.picks.push(id);
                } else if let Some((p, _)) = at {
                    offset_side(app, tool, p);
                }
                return;
            }
            if cmd.starts_with("Fillet") || cmd.starts_with("Chamfer") {
                if is_point {
                    run_corner(app, cmd, &id);
                }
                return;
            }
            if cmd == "sketch.centerline" {
                let _ = app.run(cmd, json!({"curves": [id]}));
                return;
            }
            if !tool.picks.contains(&id) {
                tool.picks.push(id);
                if let Some((p, _)) = at {
                    tool.pts.push((p, None));
                }
            }
            if n > 0 && tool.picks.len() >= n {
                run_entities(app, tool);
                tool.picks.clear();
                tool.pts.clear();
            }
        }
        Mode::EntitiesThen => {
            // Stage 2 (after a right-click) is marked by a "|" pick.
            if tool.picks.last().is_some_and(|x| x == "|") {
                let ents: Vec<String> = tool.picks.iter().filter(|x| *x != "|").cloned().collect();
                match cmd {
                    "MirrorSketchCommand" => {
                        if let Some((line, false)) = sketch_entity(app, &hits) {
                            let _ = app.run(cmd, json!({"entities": ents, "line": line}));
                            tool.picks.clear();
                        }
                    }
                    "CircularSketchPatternCommand" => {
                        if let Some(p) = at {
                            let _ = app.run(cmd, json!({"entities": ents, "center": arg(&p), "count": 6}));
                            tool.picks.clear();
                        }
                    }
                    _ => {
                        if let Some(p) = at {
                            tool.pts.push(p);
                            if tool.pts.len() >= 2 {
                                let d = tool.pts[1].0 - tool.pts[0].0;
                                if d.len() > 1e-9 {
                                    let _ = app.run(cmd, json!({"entities": ents, "dir": xy(d), "count": 3, "spacing": d.len()}));
                                }
                                tool.picks.clear();
                                tool.pts.clear();
                            }
                        }
                    }
                }
                return;
            }
            if let Some((id, _)) = sketch_entity(app, &hits)
                && !tool.picks.contains(&id)
            {
                tool.picks.push(id);
            }
        }
        Mode::Steps => match cmd {
            "IntersectionCurve" => {
                if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("face").is_some()) {
                    tool.picks.push(r.to_string());
                    if tool.picks.len() >= 2 {
                        let a: Value = serde_json::from_str(&tool.picks[0]).unwrap_or(Value::Null);
                        let b: Value = serde_json::from_str(&tool.picks[1]).unwrap_or(Value::Null);
                        let _ = app.run(cmd, json!({"a": a, "b": b}));
                        tool.picks.clear();
                    }
                }
            }
            "SpunProfileCmd" => {
                if tool.picks.is_empty() {
                    if let Some(b) =
                        hits.iter().find_map(|h| model_ref(app, h)).and_then(|r| r.get("body").and_then(Value::as_str).map(str::to_string))
                    {
                        tool.picks.push(b);
                    }
                } else if let Some((line, false)) = sketch_entity(app, &hits) {
                    let _ = app.run(cmd, json!({"body": tool.picks[0], "axis": line}));
                    tool.picks.clear();
                }
            }
            _ => {
                // ProjectToSurface: curves of other sketches, then (after a right-click) a face.
                let staged = tool.picks.last().is_some_and(|x| x == "|");
                if staged {
                    if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("face").is_some()) {
                        let curves: Vec<Value> = tool.picks.iter().filter(|x| *x != "|").filter_map(|x| serde_json::from_str(x).ok()).collect();
                        let _ = app.run(cmd, json!({"curves": curves, "face": r["face"], "body": r["body"]}));
                        tool.picks.clear();
                    }
                } else if let Some(r) = hits.iter().find_map(|h| model_ref(app, h)).filter(|r| r.get("curve").is_some()) {
                    tool.picks.push(r.to_string());
                }
            }
        },
    }
}

fn run_points(app: &mut SolveApp, t: &Tool) {
    let p = &t.pts;
    let width = |a: Vec2, b: Vec2, c: Vec2| -> f64 { 2.0 * (b - a).normalized().map(|d| d.cross(c - a).abs()).unwrap_or(1.0) };
    let params = match (t.cmd, p.as_slice()) {
        ("SketchMidpointLine", [m, e]) => json!({"mid": arg(m), "end": xy(e.0)}),
        ("ArcTangent", [s, e]) => json!({"start": arg(s), "end": xy(e.0)}),
        ("CircleElipse", [c, m, q]) => json!({"center": xy(c.0), "major": xy(m.0), "minor": xy(q.0)}),
        ("ConicCurveCmd", [a, b, x]) => json!({"start": arg(a), "end": arg(b), "apex": arg(x)}),
        ("ShapeSlotCenterPoint", [c, e, w]) => json!({"center": xy(c.0), "end": xy(e.0), "width": width(c.0, e.0, w.0).max(1e-3)}),
        ("ShapeArcSlotThreePoint", [a, th, b, w]) => {
            json!({"start": xy(a.0), "through": xy(th.0), "end": xy(b.0), "width": (2.0 * w.0.dist(b.0)).max(1e-3)})
        }
        ("ShapeArcSlotCenterTwoPoint", [c, a, b, w]) => {
            let r = c.0.dist(a.0);
            json!({"center": xy(c.0), "start": xy(a.0), "end": xy(b.0), "width": (2.0 * (w.0.dist(c.0) - r).abs()).max(1e-3)})
        }
        ("DrawSpline" | "DrawCVMSpline3D" | "DrawCVMSpline5D", pts) if pts.len() >= 2 => {
            json!({"points": pts.iter().map(arg).collect::<Vec<_>>()})
        }
        ("BlendG1CurveSketchCmd", [a, b]) => match (&a.1, &b.1) {
            (Some(x), Some(y)) => json!({"a": x, "b": y}),
            _ => return,
        },
        ("MTextCmd", [at]) => {
            let at = at.0;
            TEXT.with(|t| *t.borrow_mut() = Some((at, String::new())));
            return;
        }
        _ => return,
    };
    let _ = app.run(t.cmd, params);
}

fn run_corner(app: &mut SolveApp, cmd: &str, point: &str) {
    let size = round_nice(corner_len(app, point) * 0.2);
    let params = match cmd {
        "FilletSketchCmd" => json!({"point": point, "radius": size}),
        "ChamferSketchDistanceAngle" => json!({"point": point, "distance": size, "angle": 45}),
        "ChamferSketchDistanceDistance" => json!({"point": point, "distance": size, "distance2": size}),
        _ => json!({"point": point, "distance": size}),
    };
    if let Ok(v) = app.run(cmd, params)
        && let Some(p) = v["param"].as_str()
    {
        app.dialog = Some(crate::dialogs::Dialog::edit_param(&app.session, p));
    }
}

fn run_entities(app: &mut SolveApp, t: &Tool) {
    let k = &t.picks;
    let params = match t.cmd {
        "ConstraintSmooth" => json!({"a": k[0], "b": k[1]}),
        "CircleTanTanRadius" => {
            let r = round_nice(curve_len(app, &k[0]).min(curve_len(app, &k[1])) * 0.1);
            let near = t.pts.last().map(|p| xy(p.0)).unwrap_or(json!([0, 0]));
            json!({"curves": k, "radius": r, "near": near})
        }
        "CircleThreeTangent" => {
            // Start inside the three picks.
            let c = t.pts.iter().fold(Vec2::ZERO, |a, p| a + p.0) / t.pts.len().max(1) as f64;
            json!({"curves": k, "near": xy(c)})
        }
        "SketchConstrainer" => json!({"entities": k}),
        "SketchPolygonConstraintCmd" => json!({"lines": k}),
        _ => return,
    };
    let _ = app.run(t.cmd, params);
}

/// Right-click with an extra tool: finish a chain or move to the next stage. Returns false
/// when the tool should be dropped.
pub fn finish(app: &mut SolveApp, tool: &mut Tool) -> bool {
    let Some(m) = mode(tool.cmd) else { return false };
    match m {
        Mode::Points(0) if tool.pts.len() >= 2 => {
            run_points(app, tool);
            tool.pts.clear();
            true
        }
        Mode::Entities(0) if !tool.picks.is_empty() => {
            run_entities(app, tool);
            tool.picks.clear();
            tool.pts.clear();
            true
        }
        Mode::EntitiesThen | Mode::Steps if !tool.picks.is_empty() && tool.picks.last().is_some_and(|x| x != "|") => {
            tool.picks.push("|".into());
            true
        }
        _ if !tool.pts.is_empty() || !tool.picks.is_empty() => {
            tool.pts.clear();
            tool.picks.clear();
            true
        }
        _ => false,
    }
}

/// Offset: the second click sets the side and distance.
fn offset_side(app: &mut SolveApp, tool: &mut Tool, p: Vec2) -> bool {
    if tool.cmd != "Offset" || tool.picks.is_empty() {
        return false;
    }
    let st = app.session.model.state();
    let d = active_sketch(app)
        .and_then(|s| st.sketch(s))
        .and_then(|ss| ss.sketch.curve_index(&tool.picks[0]).and_then(|c| ss.sketch.shape(c)))
        .map(|sh| sh.dist(p))
        .unwrap_or(0.0);
    if d > 1e-6 {
        let _ = app.run("Offset", json!({"curves": [tool.picks[0]], "distance": d, "side": xy(p)}));
    }
    tool.picks.clear();
    true
}

/// Rubber band for point tools and pick markers.
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    let Some(sid) = active_sketch(app) else { return };
    let st = app.session.model.state();
    let Some(ss) = st.sketch(sid) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = crate::theme::Tokens::get();
    let stroke = Stroke::new(1.5, tk.rubber_band);
    if let Some((hp, snap)) = &t.hover
        && let Some(sp) = to(*hp)
    {
        painter.circle_stroke(sp, if snap.is_some() { 6.0 } else { 3.0 }, stroke);
    }
    let pts: Vec<Pos2> = t.pts.iter().filter_map(|p| to(p.0)).collect();
    for w in pts.windows(2) {
        painter.line_segment([w[0], w[1]], Stroke::new(1.0, tk.rubber_band.gamma_multiply(0.6)));
    }
    if let (Some(last), Some((h, _))) = (pts.last(), t.hover.as_ref())
        && let Some(hs) = to(*h)
    {
        painter.line_segment([*last, hs], stroke);
    }
    // Picked curves highlighted.
    for id in t.picks.iter().filter(|x| *x != "|") {
        if let Some(ci) = ss.sketch.curve_index(id) {
            let v: Vec<Pos2> = ss.sketch.polyline(ci).iter().filter_map(|q| to(*q)).collect();
            painter.add(egui::Shape::line(v, Stroke::new(3.0, tk.accent.gamma_multiply(0.7))));
        }
    }
}

/// Constraint glyphs of the active sketch: a small badge per constraint at its anchor
/// (conflicting ones red).
fn glyphs(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    let Some(sid) = active_sketch(app) else { return };
    let st = app.session.model.state();
    let Some(ss) = st.sketch(sid) else { return };
    if ss.sketch.view.hide_constraints {
        GLYPHS.with(|g| g.borrow_mut().clear());
        return;
    }
    let tk = crate::theme::Tokens::get();
    let mut placed: Vec<Pos2> = Vec::new();
    let picked = PICKED.with(|p| p.borrow().clone());
    let mut drawn: Vec<(String, Pos2)> = Vec::new();
    for c in &ss.sketch.constraints {
        if c.kind.is_dimension() {
            continue;
        }
        let Some(at) = glyph_anchor(&ss.sketch, &c.kind) else { continue };
        let Some(mut sp) = proj.to_screen(ss.plane.to_world(at)) else { continue };
        sp += egui::vec2(9.0, -9.0);
        // Keep glyphs at one spot from overlapping.
        while placed.iter().any(|q| q.distance(sp) < 12.0) {
            sp += egui::vec2(13.0, 0.0);
        }
        placed.push(sp);
        let failing = ss.report.failing.contains(&c.id);
        let selected = picked.as_deref() == Some(c.id.as_str())
            || app.session.selection.iter().any(|s| matches!(s, solvecraft_engine::Sel::SketchConstraint { id } if *id == c.id));
        let col = if failing {
            tk.error
        } else if selected {
            tk.accent
        } else {
            tk.text_dim
        };
        drawn.push((c.id.clone(), sp));
        let r = egui::Rect::from_center_size(sp, egui::vec2(12.0, 12.0));
        painter.rect_filled(r, 2.0, tk.panel.gamma_multiply(0.85));
        painter.rect_stroke(r, 2.0, Stroke::new(0.8, col), egui::StrokeKind::Inside);
        painter.text(sp, egui::Align2::CENTER_CENTER, glyph_letter(c.kind.name()), egui::FontId::proportional(9.0), col);
    }
    GLYPHS.with(|g| *g.borrow_mut() = drawn);
}

fn glyph_letter(name: &str) -> &'static str {
    match name {
        "Coincident" => "\u{25cf}",
        "Horizontal" => "H",
        "Vertical" => "V",
        "Parallel" => "//",
        "Perpendicular" => "\u{22a5}",
        "Collinear" => "\u{2261}",
        "Tangent" => "T",
        "Smooth" => "G2",
        "Equal" => "=",
        "Concentric" => "\u{25ce}",
        "MidPoint" => "M",
        "Symmetry" => "[]",
        "Fix" => "\u{1f512}",
        _ => "?",
    }
}

fn glyph_anchor(sk: &solvecraft_engine::sketch::Sketch, k: &solvecraft_engine::sketch::ConstraintKind) -> Option<Vec2> {
    use solvecraft_engine::sketch::ConstraintKind::*;
    // Half way along the curve.
    let mid = |c: usize| -> Option<Vec2> {
        let poly = sk.polyline(c);
        let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
        let mut acc = 0.0;
        for w in poly.windows(2) {
            let l = w[0].dist(w[1]);
            if acc + l >= total * 0.5 && l > 0.0 {
                return Some(w[0].lerp(w[1], (total * 0.5 - acc) / l));
            }
            acc += l;
        }
        poly.first().copied()
    };
    match *k {
        Coincident { p, .. } | Fix { p } | Midpoint { p, .. } | PointOnCurve { p, .. } => sk.point(p),
        HorizontalPoints { p, q } | VerticalPoints { p, q } | Symmetric { p, q, .. } => Some((sk.point(p)? + sk.point(q)?) * 0.5),
        Horizontal { l } | Vertical { l } => mid(l),
        Parallel { a, .. }
        | Perpendicular { a, .. }
        | Collinear { a, .. }
        | Equal { a, .. }
        | Concentric { a, .. }
        | Tangent { a, .. }
        | Smooth { a, .. } => mid(a),
        _ => None,
    }
}

/// The inference label next to the snapped cursor (while a tool is active).
fn snap_hint(app: &SolveApp, painter: &egui::Painter, proj: &Proj) {
    if app.tool.is_none() {
        return;
    }
    let Some((label, p)) = SNAP.with(|s| *s.borrow()) else { return };
    let st = app.session.model.state();
    let Some(ss) = active_sketch(app).and_then(|s| st.sketch(s)) else { return };
    let Some(sp) = proj.to_screen(ss.plane.to_world(p)) else { return };
    let tk = crate::theme::Tokens::get();
    let r = egui::Rect::from_min_size(sp + egui::vec2(12.0, -24.0), egui::vec2(24.0, 14.0));
    painter.rect_filled(r, 3.0, tk.accent_soft);
    painter.text(r.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(10.0), tk.text);
}

/// Per frame, after the viewport is drawn: constraint glyphs and the text-entry box.
pub fn show(app: &mut SolveApp, ui: &egui::Ui, painter: &egui::Painter, proj: &Proj) {
    SHIFT.with(|c| c.set(ui.input(|i| i.modifiers.shift)));
    glyphs(app, painter, proj);
    snap_hint(app, painter, proj);
    combs(app, painter, proj);
    if DEFERRED.with(|d| d.replace(false)) {
        analysis(app, "FusionCenterOfMassCommand", json!({}));
    }
    marks(app, painter, proj);
    text_entry(app, ui.ctx());
    canvas_panel(app, ui.ctx());
}

/// The text-entry box for a placed text.
fn text_entry(app: &mut SolveApp, ctx: &egui::Context) {
    let Some((at, mut text)) = TEXT.with(|t| t.borrow_mut().take()) else { return };
    let mut done = None;
    egui::Window::new("Text").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).show(ctx, |ui| {
        let r = ui.text_edit_singleline(&mut text);
        r.request_focus();
        ui.horizontal(|ui| {
            if ui.button("OK").clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        });
    });
    match done {
        Some(true) => {
            if !text.trim().is_empty() {
                let h = round_nice(app.cam.half_height() * 0.1);
                let _ = app.run("MTextCmd", json!({"text": text, "at": xy(at), "height": h}));
            }
        }
        Some(false) => {}
        None => TEXT.with(|t| *t.borrow_mut() = Some((at, text))),
    }
}
