//! Interactive sketch tools: click points on the sketch plane (or pick sketch entities) and the
//! tool runs the matching command. Tools are UI state only; the work is done by commands.

use egui::{Pos2, Shape, Stroke};
use serde_json::{Value, json};
use solvecraft_engine::geom::Vec2;

use crate::SolveApp;
use crate::viewport::{Hit, Proj, pick, sketch_point_at};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Click points; `n` points complete the shape (0 = chain until finished).
    Draw(usize),
    /// Pick sketch entities; `n` picks run the command.
    Pick(usize),
    Dimension,
    /// A tool from `sketch_tools` (it handles its own clicks).
    Ext,
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub cmd: &'static str,
    pub kind: Kind,
    /// Clicked points (sketch coordinates) and the sketch point they snapped to.
    pub pts: Vec<(Vec2, Option<String>)>,
    pub hover: Option<(Vec2, Option<String>)>,
    /// Picked entity ids.
    pub picks: Vec<String>,
    /// Inline dimension boxes for the shape being drawn, the point count they were made for,
    /// and whether the first box should take the keyboard.
    pub dims: Vec<crate::sketch_dims::DimBox>,
    pub dims_stage: usize,
    pub dims_focus: bool,
}

impl Tool {
    pub fn for_command(id: &str) -> Option<Tool> {
        let (cmd, kind): (&'static str, Kind) = match id {
            "DrawPolyline" => ("DrawPolyline", Kind::Draw(0)),
            "ShapeRectangleTwoPoint" => ("ShapeRectangleTwoPoint", Kind::Draw(2)),
            "ShapeRectangleCenter" => ("ShapeRectangleCenter", Kind::Draw(2)),
            "ShapeRectangleThreePoint" => ("ShapeRectangleThreePoint", Kind::Draw(3)),
            "CircleCenterRadius" => ("CircleCenterRadius", Kind::Draw(2)),
            "CircleTwoPoint" => ("CircleTwoPoint", Kind::Draw(2)),
            "CircleThreePoint" => ("CircleThreePoint", Kind::Draw(3)),
            "ArcThreePoint" => ("ArcThreePoint", Kind::Draw(3)),
            "ArcCenterTwoPoint" => ("ArcCenterTwoPoint", Kind::Draw(3)),
            "ShapePolygonInscribed" => ("ShapePolygonInscribed", Kind::Draw(2)),
            "ShapePolygonCircumscribed" => ("ShapePolygonCircumscribed", Kind::Draw(2)),
            "ShapePolygonEdge" => ("ShapePolygonEdge", Kind::Draw(2)),
            "ShapeSlotCenterToCenter" => ("ShapeSlotCenterToCenter", Kind::Draw(3)),
            "ShapeSlotOverall" => ("ShapeSlotOverall", Kind::Draw(3)),
            "DrawPoint" => ("DrawPoint", Kind::Draw(1)),
            "SketchDimension" => ("SketchDimension", Kind::Dimension),
            "ConstraintHorizontalVertical" => ("ConstraintHorizontalVertical", Kind::Pick(1)),
            "ConstraintFix" => ("ConstraintFix", Kind::Pick(1)),
            "ConstraintCoincident" => ("ConstraintCoincident", Kind::Pick(2)),
            "ConstraintTangent" => ("ConstraintTangent", Kind::Pick(2)),
            "ConstraintEqual" => ("ConstraintEqual", Kind::Pick(2)),
            "ConstraintParallel" => ("ConstraintParallel", Kind::Pick(2)),
            "ConstraintPerpendicular" => ("ConstraintPerpendicular", Kind::Pick(2)),
            "ConstraintConcentric" => ("ConstraintConcentric", Kind::Pick(2)),
            "ConstraintCollinear" => ("ConstraintCollinear", Kind::Pick(2)),
            "ConstraintMidPoint" => ("ConstraintMidPoint", Kind::Pick(2)),
            "ConstraintSymmetry" => ("ConstraintSymmetry", Kind::Pick(3)),
            "sketch.construction" => ("sketch.construction", Kind::Pick(1)),
            _ => return crate::sketch_tools::tool_for(id),
        };
        Some(Tool { cmd, kind, pts: Vec::new(), hover: None, picks: Vec::new(), dims: Vec::new(), dims_stage: usize::MAX, dims_focus: false })
    }
}

pub fn hint(id: &str) -> String {
    match id {
        "DrawPolyline" => "Line: click points; right-click or Esc to finish".into(),
        "ShapeRectangleTwoPoint" => "Rectangle: click two opposite corners".into(),
        "ShapeRectangleCenter" => "Center rectangle: click the centre, then a corner".into(),
        "ShapeRectangleThreePoint" => "3-point rectangle: click two points of one edge, then the width".into(),
        "CircleCenterRadius" => "Circle: click the centre, then a point on the circle".into(),
        "ArcThreePoint" => "Arc: click start, end, then a point on the arc".into(),
        "ArcCenterTwoPoint" => "Arc: click centre, start, then end (counter-clockwise)".into(),
        "SketchDimension" => "Dimension: pick a line, circle or arc (or two points/lines)".into(),
        "ShapeSlotCenterToCenter" | "ShapeSlotOverall" => "Slot: click both ends, then the width".into(),
        _ if id.starts_with("Constraint") => "Constraint: pick the sketch entities".into(),
        _ => crate::sketch_tools::hint(id).unwrap_or_else(|| "Click in the sketch".into()),
    }
}

fn arg(p: &(Vec2, Option<String>)) -> Value {
    match &p.1 {
        Some(id) => json!(id),
        None => json!([p.0.x, p.0.y]),
    }
}

fn xy(p: &(Vec2, Option<String>)) -> Value {
    json!([p.0.x, p.0.y])
}

pub fn on_hover(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    crate::sketch3d::on_hover(app, proj, pos);
    let h = sketch_point_at(app, proj, pos);
    if let Some(t) = app.tool.as_mut() {
        t.hover = h;
    }
}

pub fn on_click(app: &mut SolveApp, proj: &Proj, pos: Pos2) {
    let Some(mut tool) = app.tool.take() else { return };
    // In a 3D sketch the drawing tools take points off the plane.
    if crate::sketch3d::on_click(app, &mut tool, proj, pos) {
        app.tool = Some(tool);
        return;
    }
    match tool.kind {
        Kind::Draw(_) => {
            if let Some(mut p) = sketch_point_at(app, proj, pos) {
                // Typed values win over the cursor; lines snap to horizontal/vertical.
                if tool.dims.iter().any(|b| b.locked) || (tool.cmd == "DrawPolyline" && p.1.is_none()) {
                    p = (crate::sketch_dims::effective(app, &tool, p.0), None);
                }
                app.tool = Some(tool);
                place(app, p);
                return;
            }
        }
        Kind::Pick(n) => {
            let hits = pick(app, proj, pos);
            if let Some(id) = hits.iter().find_map(|h| match h {
                Hit::SketchPoint { id, .. } | Hit::SketchCurve { id, .. } => Some(id.clone()),
                _ => None,
            }) {
                tool.picks.push(id);
            }
            if tool.picks.len() >= n {
                run_pick(app, &tool);
                tool.picks.clear();
            }
        }
        Kind::Ext => crate::sketch_tools::on_click(app, &mut tool, proj, pos),
        Kind::Dimension => {
            let hits = pick(app, proj, pos);
            let hit = hits.iter().find_map(|h| match h {
                Hit::SketchPoint { id, .. } => Some((id.clone(), true)),
                Hit::SketchCurve { id, .. } => Some((id.clone(), false)),
                _ => None,
            });
            if let Some((id, is_point)) = hit {
                tool.picks.push(id.clone());
                let st = app.session.model.state();
                let is_line = app
                    .session
                    .active_sketch
                    .and_then(|s| st.sketch(s))
                    .and_then(|ss| ss.sketch.curve_index(&id).and_then(|c| ss.sketch.curves.get(c).cloned()))
                    .map(|c| matches!(c.kind, solvecraft_engine::sketch::CurveKind::Line { .. }));
                let ready = (tool.picks.len() == 1 && !is_point && is_line != Some(true)) || tool.picks.len() >= 2;
                let single_line = tool.picks.len() == 1 && is_line == Some(true);
                if ready || single_line {
                    // A single line gets its length right away; a second pick (line or point) replaces it.
                    if let Ok(v) = app.run("SketchDimension", json!({"entities": tool.picks}))
                        && let Some(p) = v["param"].as_str()
                    {
                        app.dialog = Some(crate::dialogs::Dialog::edit_param(&app.session, p));
                    }
                    tool.picks.clear();
                }
            }
        }
    }
    app.tool = Some(tool);
}

/// Put down the next point of the shape being drawn; the last point runs the command (with
/// the typed values as dimension constraints).
pub fn place(app: &mut SolveApp, p: (Vec2, Option<String>)) {
    let Some(mut tool) = app.tool.take() else { return };
    let Kind::Draw(n) = tool.kind else {
        app.tool = Some(tool);
        return;
    };
    tool.pts.push(p);
    if tool.cmd == "DrawPolyline" {
        if tool.pts.len() >= 2 {
            let (a, b) = (tool.pts[tool.pts.len() - 2].clone(), tool.pts[tool.pts.len() - 1].clone());
            if let Ok(v) = app.run("DrawPolyline", json!({"points": [arg(&a), arg(&b)], "infer": true})) {
                let curves: Vec<String> = v["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
                dimension(app, &tool, &curves);
                // Chain: the next segment starts at this one's end point.
                if let Some(id) = curves.first() {
                    let last = tool.pts.len() - 1;
                    tool.pts[last].1 = Some(format!("{id}.end"));
                }
            }
        }
    } else if n > 0 && tool.pts.len() >= n {
        let curves = run_shape(app, &tool);
        dimension(app, &tool, &curves);
        tool.pts.clear();
    }
    app.tool = Some(tool);
}

/// Typed values become dimension constraints on the new curves.
fn dimension(app: &mut SolveApp, t: &Tool, curves: &[String]) {
    for (ents, ty, value) in crate::sketch_dims::constraints(t, curves) {
        let _ = app.run("SketchDimension", crate::sketch_dims::dimension_params(&ents, ty, &value));
    }
}

fn run_shape(app: &mut SolveApp, t: &Tool) -> Vec<String> {
    let p = &t.pts;
    let d = |a: usize, b: usize| p[a].0.dist(p[b].0);
    let sides = crate::sketch_dims::locked_value(app, t, "Sides").map_or(6, |v| v.round().clamp(3.0, 200.0) as usize);
    let params = match t.cmd {
        "ShapeRectangleTwoPoint" => json!({"p0": xy(&p[0]), "p1": xy(&p[1])}),
        "ShapeRectangleCenter" => json!({"center": xy(&p[0]), "corner": xy(&p[1])}),
        "ShapeRectangleThreePoint" => json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "p2": xy(&p[2])}),
        "CircleCenterRadius" => json!({"center": arg(&p[0]), "radius": d(0, 1)}),
        "CircleTwoPoint" => json!({"p0": xy(&p[0]), "p1": xy(&p[1])}),
        "CircleThreePoint" => json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "p2": xy(&p[2])}),
        "ArcThreePoint" => json!({"start": arg(&p[0]), "end": arg(&p[1]), "through": xy(&p[2])}),
        "ArcCenterTwoPoint" => json!({"center": arg(&p[0]), "start": arg(&p[1]), "end": xy(&p[2])}),
        "ShapePolygonInscribed" | "ShapePolygonCircumscribed" => {
            let a = (p[1].0 - p[0].0).angle().to_degrees();
            json!({"center": xy(&p[0]), "radius": d(0, 1), "sides": sides, "angle": a})
        }
        "ShapePolygonEdge" => json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "sides": sides}),
        "ShapeSlotCenterToCenter" | "ShapeSlotOverall" => {
            let dir = (p[1].0 - p[0].0).normalized().unwrap_or(Vec2::X);
            let w = 2.0 * dir.cross(p[2].0 - p[0].0).abs();
            json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "width": w.max(1e-3)})
        }
        "DrawPoint" => json!({"point": xy(&p[0])}),
        _ => return Vec::new(),
    };
    app.run(t.cmd, params)
        .ok()
        .map(|v| v["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

fn run_pick(app: &mut SolveApp, t: &Tool) {
    let k = &t.picks;
    let params = match t.cmd {
        "ConstraintHorizontalVertical" => json!({"line": k[0]}),
        "ConstraintFix" => json!({"entity": k[0]}),
        "sketch.construction" => json!({"curves": [k[0]]}),
        "ConstraintCoincident" => json!({"a": k[0], "b": k[1]}),
        "ConstraintMidPoint" => json!({"point": k[0], "line": k[1]}),
        "ConstraintSymmetry" => json!({"a": k[0], "b": k[1], "line": k[2]}),
        _ => json!({"a": k[0], "b": k[1]}),
    };
    let _ = app.run(t.cmd, params);
}

/// Right click / Esc: end the current chain (line) or cancel the tool.
pub fn finish(app: &mut SolveApp) {
    if crate::sketch3d::finish(app) {
        return;
    }
    if let Some(mut t) = app.tool.take_if(|t| t.kind == Kind::Ext) {
        if crate::sketch_tools::finish(app, &mut t) {
            app.tool = Some(t);
        }
        return;
    }
    if let Some(t) = app.tool.as_mut()
        && !t.pts.is_empty()
    {
        t.pts.clear();
        t.picks.clear();
        return;
    }
    app.tool = None;
}

/// Rubber-band preview of the shape being drawn.
pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    if crate::sketch3d::preview(app, t, painter, proj) {
        return;
    }
    if t.kind == Kind::Ext {
        return crate::sketch_tools::preview(app, t, painter, proj);
    }
    let Some(sid) = app.session.active_sketch else { return };
    let st = app.session.model.state();
    let Some(ss) = st.sketch(sid) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    let tk = crate::theme::Tokens::get();
    let stroke = Stroke::new(1.5, tk.rubber_band);
    if let Some((hp, snap)) = &t.hover
        && let Some(sp) = to(*hp)
    {
        painter.circle_stroke(sp, if snap.is_some() { 6.0 } else { 3.0 }, stroke);
        painter.text(
            sp + egui::vec2(10.0, 10.0),
            egui::Align2::LEFT_TOP,
            format!("{:.2}, {:.2}", hp.x, hp.y),
            egui::FontId::proportional(11.0),
            tk.text_dim,
        );
    }
    let Some((h, _)) = t.hover.clone() else { return };
    let h = crate::sketch_dims::effective(app, t, h);
    let pts: Vec<Vec2> = t.pts.iter().map(|p| p.0).collect();
    let line = |a: Vec2, b: Vec2| {
        if let (Some(x), Some(y)) = (to(a), to(b)) {
            painter.line_segment([x, y], stroke);
        }
    };
    let circle = |c: Vec2, r: f64| {
        let v: Vec<Pos2> = (0..=64).filter_map(|i| to(c + Vec2::from_angle(i as f64 / 64.0 * std::f64::consts::TAU) * r)).collect();
        painter.add(Shape::line(v, stroke));
    };
    match (t.cmd, pts.as_slice()) {
        ("DrawPolyline", [.., last]) => {
            line(*last, h);
            // Horizontal/vertical inference marker.
            if let Some(horizontal) = crate::sketch_dims::axis_aligned(*last, h)
                && let Some(m) = to((*last + h) * 0.5)
            {
                let g = if horizontal { egui::vec2(0.0, -12.0) } else { egui::vec2(12.0, 0.0) };
                painter.text(
                    m + g,
                    egui::Align2::CENTER_CENTER,
                    if horizontal { "H" } else { "V" },
                    egui::FontId::proportional(10.0),
                    tk.sketch_accent,
                );
            }
        }
        ("ShapeRectangleTwoPoint", [a]) => {
            let (b, c) = (Vec2::new(h.x, a.y), Vec2::new(a.x, h.y));
            line(*a, b);
            line(b, h);
            line(h, c);
            line(c, *a);
        }
        ("ShapeRectangleCenter", [c]) => {
            let d = h - *c;
            let k =
                [Vec2::new(c.x - d.x, c.y - d.y), Vec2::new(c.x + d.x, c.y - d.y), Vec2::new(c.x + d.x, c.y + d.y), Vec2::new(c.x - d.x, c.y + d.y)];
            for i in 0..4 {
                line(k[i], k[(i + 1) % 4]);
            }
        }
        ("CircleCenterRadius" | "ShapePolygonInscribed" | "ShapePolygonCircumscribed", [c]) => circle(*c, c.dist(h)),
        ("CircleTwoPoint", [a]) => circle((*a + h) * 0.5, a.dist(h) * 0.5),
        (_, [a]) => line(*a, h),
        (_, [a, b]) => {
            line(*a, *b);
            line(*b, h);
        }
        _ => {}
    }
}
