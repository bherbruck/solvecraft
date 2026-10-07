//! More sketch constraints (curvature, polygon), constraint glyph anchors and snapping
//! inference for interactive tools.

use serde_json::{Value, json};
use solvecraft_geom::Vec2;
use solvecraft_sketch::{ConstraintKind, CurveKind, Sketch};

use super::sketch::{add_c, dof_now, edit, reject_redundant};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, num, req_vec2, string_list, vec2};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("ConstraintSmooth", "Curvature", c_smooth)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_smooth")
        .enabled(in_sketch)
        .params("a, b: two curves sharing an end point (curvature continuous, G2)"),
    CommandSpec::new("SketchPolygonConstraintCmd", "Polygon", c_polygon)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_polygon")
        .enabled(in_sketch)
        .params("lines: [line ids] of a closed chain: equal sides with every corner on one circle (a regular polygon)"),
    CommandSpec::new("sketch.glyphs", "Constraint Glyphs", glyphs)
        .enabled(in_sketch)
        .noundo()
        .params("sketch?: id|name: every constraint with where to draw its glyph (sketch coordinates)"),
    CommandSpec::new("sketch.snap", "Snap Inference", snap)
        .enabled(in_sketch)
        .noundo()
        .params("at: [x,y], from?: [x,y] (previous click), radius?: mm (snap distance, default 1): what a click there would snap to and infer"),
];

fn c_smooth(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "ConstraintSmooth";
    let mut before = None;
    let (id, info) = edit(s, p, cmd, true, |sk, _| {
        let a = curve(sk, p.get("a"), cmd)?;
        let b = curve(sk, p.get("b"), cmd)?;
        before = Some(dof_now(sk));
        add_c(sk, ConstraintKind::Smooth { a, b })
    })?;
    reject_redundant(before, &info, cmd)?;
    Ok(json!({"constraint": id, "sketch": info}))
}

fn curve(sk: &Sketch, v: Option<&Value>, cmd: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, "expected a curve id"))?;
    sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))
}

fn c_polygon(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "SketchPolygonConstraintCmd";
    let ids = string_list(p, "lines");
    if ids.len() < 3 || ids.len() > 200 {
        return Err(bad(cmd, "`lines` must list 3 to 200 lines of a closed chain"));
    }
    let (out, info) = edit(s, p, cmd, true, |sk, _| {
        let ls: Vec<usize> = ids.iter().map(|i| sk.curve_index(i).ok_or_else(|| bad(cmd, format!("unknown curve `{i}`")))).collect::<Result<_>>()?;
        let mut corners: Vec<usize> = Vec::new();
        for l in &ls {
            match sk.curves.get(*l).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => {
                    for q in [*a, *b] {
                        if !corners.contains(&q) {
                            corners.push(q);
                        }
                    }
                }
                _ => return Err(bad(cmd, "the polygon must be made of lines")),
            }
        }
        if corners.len() != ls.len() {
            return Err(bad(cmd, "the lines must form one closed chain"));
        }
        let pts: Vec<Vec2> = corners.iter().filter_map(|q| sk.point(*q)).collect();
        let c = pts.iter().fold(Vec2::ZERO, |a, q| a + *q) / pts.len() as f64;
        let r = pts.iter().map(|q| q.dist(c)).sum::<f64>() / pts.len() as f64;
        let circ = sk.add_circle(c, r.max(1e-6), None, None)?;
        if let Some(cu) = sk.curves.get_mut(circ) {
            cu.construction = true;
        }
        let mut cons = Vec::new();
        for w in ls.windows(2) {
            cons.push(add_c(sk, ConstraintKind::Equal { a: w[0], b: w[1] })?);
        }
        for q in corners {
            cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: q, c: circ })?);
        }
        Ok(cons)
    })?;
    Ok(json!({"constraints": out, "sketch": info}))
}

/// Where a constraint's glyph goes (sketch coordinates): beside the middle of its first curve,
/// or at its point.
fn anchor(sk: &Sketch, k: &ConstraintKind) -> Option<Vec2> {
    use ConstraintKind::*;
    let mid = |c: usize| -> Option<Vec2> {
        let poly = sk.polyline(c);
        poly.get(poly.len() / 2).copied()
    };
    let pt = |p: usize| sk.point(p);
    match *k {
        Coincident { p, .. } | Fix { p } | Midpoint { p, .. } => pt(p),
        PointOnCurve { p, .. } => pt(p),
        HorizontalPoints { p, q } | VerticalPoints { p, q } | Symmetric { p, q, .. } => Some((pt(p)? + pt(q)?) * 0.5),
        Horizontal { l } | Vertical { l } => mid(l),
        Parallel { a, .. } | Perpendicular { a, .. } | Collinear { a, .. } | Equal { a, .. } | Concentric { a, .. } | Angle { a, .. } => mid(a),
        Tangent { a, b } | Smooth { a, b } => {
            // At the shared end when there is one.
            let ends = |c: usize| sk.curves.get(c).and_then(|c| c.kind.ends());
            match (ends(a), ends(b)) {
                (Some((a0, a1)), Some((b0, b1))) => [a0, a1].into_iter().find(|x| *x == b0 || *x == b1).and_then(pt).or_else(|| mid(a)),
                _ => mid(a),
            }
        }
        Distance { p, q, .. } | DistanceX { p, q, .. } | DistanceY { p, q, .. } => Some((pt(p)? + pt(q)?) * 0.5),
        PointLineDistance { p, .. } | LinearDiameter { p, .. } => pt(p),
        Length { l, .. } => mid(l),
        Radius { c, .. } | Diameter { c, .. } | ArcLength { c, .. } => mid(c),
    }
}

fn glyphs(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match p.get("sketch") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
        _ => s.active_sketch,
    }
    .ok_or_else(|| bad("sketch.glyphs", "no such sketch"))?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("that sketch is not evaluated".into()))?;
    let failing = &ss.report.failing;
    let list: Vec<Value> = ss
        .sketch
        .constraints
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "name": c.kind.name(),
                "dimension": c.kind.is_dimension(),
                "driven": c.driven,
                "value": c.kind.value(),
                "param": c.param,
                "at": anchor(&ss.sketch, &c.kind),
                "failing": failing.contains(&c.id),
            })
        })
        .collect();
    Ok(json!({"sketch": id, "glyphs": list, "dof": ss.report.dof, "fully_constrained": ss.report.fully_constrained(), "conflicts": failing}))
}

/// Snap inference for a click at `at`: the nearest sketch point (coincident), the midpoint of a
/// line, a point on a curve, and horizontal/vertical alignment with `from` or with other
/// points. Results are ordered by priority; the first one is what a click would use.
fn snap(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.snap";
    let at = req_vec2(cmd, p, "at")?;
    let from = p.get("from").and_then(vec2);
    let r = num(p, "radius").filter(|r| *r > 0.0).unwrap_or(1.0);
    let id = s.active_sketch.ok_or_else(|| bad(cmd, "no sketch is being edited"))?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("the sketch is not evaluated".into()))?;
    let sk = &ss.sketch;
    let mut out: Vec<(f64, Value)> = Vec::new();
    // Points (end points, centres, the origin).
    for q in &sk.points {
        let d = q.pos.dist(at);
        if d <= r {
            out.push((d * 0.5, json!({"type": "coincident", "point": q.id, "at": q.pos})));
        }
    }
    for (i, c) in sk.curves.iter().enumerate() {
        if let CurveKind::Line { a, b } = c.kind
            && let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b))
        {
            let m = (pa + pb) * 0.5;
            let d = m.dist(at);
            if d <= r {
                out.push((d * 0.6, json!({"type": "midpoint", "curve": c.id, "at": m})));
            }
        }
        if let Some(sh) = sk.shape(i) {
            let q = sh.project(at);
            if sh.dist(at) <= r && q.dist(at) <= r {
                out.push((sh.dist(at) + r * 0.5, json!({"type": "on_curve", "curve": c.id, "at": q})));
            }
        }
    }
    // Alignment with the previous click.
    if let Some(f) = from {
        let d = at - f;
        if d.y.abs() <= r && d.x.abs() > r {
            out.push((d.y.abs() + r, json!({"type": "horizontal", "at": [at.x, f.y]})));
        }
        if d.x.abs() <= r && d.y.abs() > r {
            out.push((d.x.abs() + r, json!({"type": "vertical", "at": [f.x, at.y]})));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    let list: Vec<Value> = out.into_iter().take(8).map(|x| x.1).collect();
    Ok(json!({"snaps": list}))
}

#[cfg(test)]
#[path = "sketch_constraints_tests.rs"]
mod tests;
