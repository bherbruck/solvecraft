//! Inspection: measure bodies, describe the document, sketches and model topology.

use serde_json::{Value, json};
use solvecraft_sketch::CurveKind;

use super::CommandSpec;
use crate::params::{bad, bool_, str_, string_list};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("MeasureCommand", "Measure", measure)
        .at("SOLID", "INSPECT")
        .icon("measure")
        .key("I")
        .noundo()
        .params("bodies?: [names] (default all)"),
    CommandSpec::new("document.inspect", "Inspect Design", inspect).noundo().params("measure?: bool (include body measurements)"),
    CommandSpec::new("sketch.inspect", "Inspect Sketch", sketch_inspect).noundo().params("sketch?: id|name (default active)"),
    CommandSpec::new("model.edges", "List Edges", model_edges).noundo().params("body: name"),
    CommandSpec::new("model.faces", "List Faces", model_faces).noundo().params("body: name"),
    CommandSpec::new("engine.commands", "List Commands", commands).noundo(),
];

pub fn measure_json(b: &solvecraft_doc::ModelBody) -> Value {
    match solvecraft_kernel::measure(&b.body) {
        Ok(m) => json!({
            "name": b.name,
            "volume_mm3": m.volume,
            "area_mm2": m.area,
            "center_of_mass": m.centroid,
            "bbox": {"min": m.bbox.min, "max": m.bbox.max},
            "faces": m.merged.faces,
            "edges": m.merged.edges,
            "vertices": m.merged.vertices,
            "face_types": m.merged.face_types,
            "kernel": {"faces": m.faces, "edges": m.edges, "vertices": m.vertices, "shells": m.shells},
        }),
        Err(e) => json!({"name": b.name, "error": e.to_string()}),
    }
}

fn measure(s: &mut Session, p: &Value) -> Result<Value> {
    let want = string_list(p, "bodies");
    let st = s.model.state();
    let bodies: Vec<Value> = st.bodies.iter().filter(|b| want.is_empty() || want.contains(&b.name)).map(measure_json).collect();
    if !want.is_empty() && bodies.len() != want.len() {
        return Err(bad("MeasureCommand", "unknown body name"));
    }
    let tv: f64 = bodies.iter().filter_map(|b| b["volume_mm3"].as_f64()).sum();
    let ta: f64 = bodies.iter().filter_map(|b| b["area_mm2"].as_f64()).sum();
    let tf: u64 = bodies.iter().filter_map(|b| b["faces"].as_u64()).sum();
    let te: u64 = bodies.iter().filter_map(|b| b["edges"].as_u64()).sum();
    let tvx: u64 = bodies.iter().filter_map(|b| b["vertices"].as_u64()).sum();
    Ok(json!({"body_count": bodies.len(), "bodies": bodies, "total": {"volume_mm3": tv, "area_mm2": ta, "faces": tf, "edges": te, "vertices": tvx}}))
}

fn sketch_json(s: &Session, id: u64) -> Option<Value> {
    let state = s.model.state();
    let ss = state.sketch(id)?;
    let sk = &ss.sketch;
    let curves: Vec<Value> = sk
        .curves
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let det = ss.report.curve_determined.get(i).copied().unwrap_or(false);
            let pid = |k: usize| sk.points.get(k).map(|p| p.id.clone()).unwrap_or_default();
            let mut v = match c.kind {
                CurveKind::Line { a, b } => json!({"id": c.id, "type": "line", "start": pid(a), "end": pid(b), "start_at": sk.point(a), "end_at": sk.point(b), "construction": c.construction, "fully_constrained": det}),
                CurveKind::Circle { c: cc, r } => json!({"id": c.id, "type": "circle", "center": pid(cc), "center_at": sk.point(cc), "radius": r, "construction": c.construction, "fully_constrained": det}),
                CurveKind::Arc { c: cc, a, b } => json!({"id": c.id, "type": "arc", "center": pid(cc), "start": pid(a), "end": pid(b), "center_at": sk.point(cc), "start_at": sk.point(a), "end_at": sk.point(b), "radius": sk.radius(i), "construction": c.construction, "fully_constrained": det}),
                CurveKind::Ellipse { c: cc, m, r } => json!({"id": c.id, "type": "ellipse", "center": pid(cc), "major": pid(m), "center_at": sk.point(cc), "major_at": sk.point(m), "minor_radius": r, "construction": c.construction, "fully_constrained": det}),
                CurveKind::Spline { ref pts, control, degree } => json!({"id": c.id, "type": "spline", "points": pts.iter().map(|q| pid(*q)).collect::<Vec<_>>(), "points_at": pts.iter().map(|q| sk.point(*q)).collect::<Vec<_>>(), "control": control, "degree": degree, "construction": c.construction, "fully_constrained": det}),
                CurveKind::Conic { a, b, apex, rho } => json!({"id": c.id, "type": "conic", "start": pid(a), "end": pid(b), "apex": pid(apex), "start_at": sk.point(a), "end_at": sk.point(b), "apex_at": sk.point(apex), "rho": rho, "construction": c.construction, "fully_constrained": det}),
            };
            if let (Some(l), Some(o)) = (&c.link, v.as_object_mut()) {
                o.insert("link".into(), json!(l));
            }
            v
        })
        .collect();
    let points: Vec<Value> = sk
        .points
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"id": p.id, "at": p.pos, "world": ss.plane.to_world(p.pos), "fixed": p.fixed, "link": p.link, "fully_constrained": ss.report.point_determined.get(i).copied().unwrap_or(false)}))
        .collect();
    let constraints: Vec<Value> = sk
        .constraints
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c.kind).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.insert("id".into(), json!(c.id));
                o.insert("name".into(), json!(c.kind.name()));
                if let Some(p) = &c.param {
                    o.insert("param".into(), json!(p));
                    o.insert("expression".into(), json!(s.doc.param(p).map(|x| x.expr.clone())));
                }
            }
            v
        })
        .collect();
    let profiles: Vec<Value> = ss
        .profiles
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"index": i, "area_mm2": p.area, "centroid_sketch": p.centroid, "curves": p.outer_curves, "holes": p.hole_curves}))
        .collect();
    Some(json!({
        "id": id,
        "name": ss.name,
        "plane": ss.plane,
        "status": ss.report.status,
        "dof": ss.report.dof,
        "fully_constrained": ss.report.fully_constrained(),
        "failing": ss.report.failing,
        "curves": curves,
        "points": points,
        "constraints": constraints,
        "profiles": profiles,
        "links": sk.links,
    }))
}

fn sketch_inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match p.get("sketch") {
        Some(Value::Number(n)) => s.doc.find_feature(&n.to_string()).map(|f| f.id),
        Some(Value::String(x)) => s.doc.find_feature(x).map(|f| f.id),
        _ => s.active_sketch,
    }
    .ok_or_else(|| bad("sketch.inspect", "no such sketch (and none is active)"))?;
    sketch_json(s, id).ok_or_else(|| EngineError::Other("that sketch is not evaluated (rolled back or not a sketch)".into()))
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let (vals, perr) = s.doc.param_values();
    let params: Vec<Value> = s
        .doc
        .params
        .iter()
        .map(|p| json!({"name": p.name, "expression": p.expr, "unit": p.unit, "value": vals.get(&p.name).map(|v| v.v), "model": p.model, "comment": p.comment, "error": perr.get(&p.name)}))
        .collect();
    let marker = s.doc.marker.unwrap_or(s.doc.features.len());
    let timeline: Vec<Value> = s
        .doc
        .features
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let r = s.model.result(f.id);
            json!({
                "id": f.id, "name": f.name, "type": f.kind.type_name(), "suppressed": f.suppressed, "rolled_back": i >= marker,
                "error": r.and_then(|r| r.error.clone()), "warning": r.and_then(|r| r.warning.clone()), "ms": r.map(|r| r.ms),
            })
        })
        .collect();
    let with_measure = bool_(p, "measure").unwrap_or(false);
    let bodies: Vec<Value> =
        st.bodies.iter().map(|b| if with_measure { measure_json(b) } else { json!({"name": b.name, "feature": b.feature}) }).collect();
    let sketches: Vec<Value> = st
        .sketches
        .iter()
        .map(|ss| json!({"id": ss.feature, "name": ss.name, "dof": ss.report.dof, "status": ss.report.status, "curves": ss.sketch.curves.len(), "profiles": ss.profiles.len()}))
        .collect();
    Ok(json!({
        "name": s.doc.name, "units": s.doc.units, "path": s.path, "dirty": s.is_dirty(), "revision": s.revision,
        "active_sketch": s.active_sketch, "marker": marker,
        "params": params, "timeline": timeline, "bodies": bodies, "sketches": sketches,
        "selection": s.selection, "undo": s.undo.len(), "redo": s.redo.len(),
    }))
}

fn body_of<'a>(st: &'a solvecraft_doc::ModelState, p: &Value, cmd: &str) -> Result<&'a solvecraft_doc::ModelBody> {
    let n = str_(p, "body").ok_or_else(|| bad(cmd, "`body` is required"))?;
    st.body(n).ok_or_else(|| bad(cmd, format!("no body `{n}`")))
}

fn model_edges(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let b = body_of(&st, p, "model.edges")?;
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let edges: Vec<Value> = b
        .body
        .edges(tol)?
        .iter()
        .map(|e| json!({"index": e.index, "mid": e.mid, "length": e.length, "start": e.points.first(), "end": e.points.last()}))
        .collect();
    Ok(json!({"body": b.name, "edges": edges}))
}

fn model_faces(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.model.state();
    let b = body_of(&st, p, "model.faces")?;
    let tol = (b.body.size() * 2e-3).max(1e-3);
    let faces: Vec<Value> = b
        .body
        .faces(tol)?
        .iter()
        .map(|f| json!({"index": f.index, "area": f.area, "centroid": f.centroid, "plane_normal": f.plane_normal}))
        .collect();
    Ok(json!({"body": b.name, "faces": faces}))
}

fn commands(s: &mut Session, _p: &Value) -> Result<Value> {
    Ok(Value::Array(crate::command_specs().iter().map(|c| serde_json::to_value(c.info(s)).unwrap_or_default()).collect()))
}
