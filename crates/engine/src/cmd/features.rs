//! Solid feature commands: extrude, revolve, fillet, chamfer, primitives, combine, move.

use serde_json::{Value, json};
use solvecraft_doc::{AxisRef, Direction, Extent, FeatureKind, Operation, ProfileSel, expr::Kind};
use solvecraft_geom::Vec3;

use super::CommandSpec;
use crate::params::{bad, bool_, expr, req_expr, str_, string_list, vec2, vec3};
use crate::{EngineError, Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("Extrude", "Extrude", extrude)
        .at("SOLID", "CREATE")
        .icon("extrude")
        .key("E")
        .params("distance: expr (or through_all: true); taper?: angle expr; sketch?: id|name (default: active or last sketch); profiles?: all | [index] | [[curve ids]] | [{point:[x,y]}]; direction?: positive|negative|symmetric; distance2?; start_offset?; operation?: new|join|cut|intersect; targets?: [body]; name?; body_name?"),
    CommandSpec::new("Revolve", "Revolve", revolve)
        .at("SOLID", "CREATE")
        .icon("revolve")
        .params("axis: sketch line id | x | y (sketch axes) | X|Y|Z (world); angle?: expr (default 360 deg); sketch?, profiles?, operation?, targets?, name?, body_name?"),
    CommandSpec::new("PrimitiveBox", "Box", prim_box).at("SOLID", "CREATE").icon("box").params("length, width, height: expr; corner?: [x,y,z] | center?: [x,y,z]; operation?"),
    CommandSpec::new("PrimitiveCylinder", "Cylinder", prim_cylinder).at("SOLID", "CREATE").icon("cylinder").params("radius | diameter, height: expr; base?: [x,y,z]; axis?: [x,y,z]; operation?"),
    CommandSpec::new("PrimitiveSphere", "Sphere", prim_sphere).at("SOLID", "CREATE").icon("sphere").params("radius | diameter: expr; center?: [x,y,z]; operation?"),
    CommandSpec::new("PrimitiveTorus", "Torus", prim_torus).at("SOLID", "CREATE").icon("torus").params("major, minor: expr (radii); center?; operation?"),
    CommandSpec::new("FusionFilletEdgesCommand", "Fillet", fillet)
        .at("SOLID", "MODIFY")
        .icon("fillet")
        .key("F")
        .params("edges: [[x,y,z] point on edge | {body, index}]; radius: expr; body?"),
    CommandSpec::new("FusionChamferCommand", "Chamfer", chamfer).at("SOLID", "MODIFY").icon("chamfer").params("edges: [[x,y,z] | {body, index}]; distance: expr; body?"),
    CommandSpec::new("FusionCombineCommand", "Combine", combine).at("SOLID", "MODIFY").icon("combine").params("target: body; tools: [body]; operation?: join|cut|intersect; keep_tools?: bool"),
    CommandSpec::new("FusionMoveCommand", "Move/Copy", move_bodies).at("SOLID", "MODIFY").icon("move").key("M").params("bodies: [names]; translate?: [x,y,z] (exprs or numbers); axis?: [x,y,z]; angle?: expr"),
];

fn operation(p: &Value, cmd: &str) -> Result<Operation> {
    match str_(p, "operation") {
        None => Ok(Operation::NewBody),
        Some(o) => Operation::parse(o).ok_or_else(|| bad(cmd, format!("unknown operation `{o}` (new, join, cut, intersect)"))),
    }
}

/// The sketch a feature uses: `sketch` param, the active sketch, or the last sketch.
fn feature_sketch(s: &Session, p: &Value, cmd: &str) -> Result<u64> {
    if let Some(v) = p.get("sketch") {
        let key = match v {
            Value::Number(n) => n.to_string(),
            Value::String(x) => x.clone(),
            _ => return Err(bad(cmd, "`sketch` must be an id or name")),
        };
        let f = s.doc.find_feature(&key).ok_or_else(|| bad(cmd, format!("no sketch `{key}`")))?;
        return match f.kind {
            FeatureKind::Sketch { .. } => Ok(f.id),
            _ => Err(bad(cmd, format!("`{key}` is not a sketch"))),
        };
    }
    if let Some(a) = s.active_sketch {
        return Ok(a);
    }
    let limit = s.doc.marker.unwrap_or(usize::MAX);
    s.doc
        .features
        .iter()
        .take(limit)
        .rev()
        .find(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
        .map(|f| f.id)
        .ok_or_else(|| bad(cmd, "there is no sketch to use (create one first)"))
}

fn profiles(p: &Value, cmd: &str) -> Result<ProfileSel> {
    let Some(v) = p.get("profiles") else { return Ok(ProfileSel::All) };
    match v {
        Value::String(s) if s.eq_ignore_ascii_case("all") => Ok(ProfileSel::All),
        Value::Number(n) => Ok(ProfileSel::Indices { indices: vec![n.as_u64().ok_or_else(|| bad(cmd, "profile index"))? as usize] }),
        Value::Array(a) if a.is_empty() => Err(bad(cmd, "`profiles` is empty")),
        Value::Array(a) if a.len() > 10_000 => Err(bad(cmd, "too many profiles")),
        Value::Array(a) => {
            if a.iter().all(Value::is_u64) {
                return Ok(ProfileSel::Indices { indices: a.iter().filter_map(Value::as_u64).map(|x| x as usize).collect() });
            }
            if a.iter().all(|x| x.get("point").is_some()) {
                let pts =
                    a.iter().map(|x| x.get("point").and_then(vec2).ok_or_else(|| bad(cmd, "profile point must be [x, y]"))).collect::<Result<_>>()?;
                return Ok(ProfileSel::Points { points: pts });
            }
            // [[ids]], [{curves: [...]}] or recipe-style [{loops: [{outer: true, curves: [...]}]}].
            let mut loops = Vec::new();
            for x in a {
                let ids: Vec<String> = match x {
                    Value::Array(ids) => ids.iter().filter_map(|i| i.as_str().map(str::to_string)).collect(),
                    Value::Object(o) => {
                        if let Some(c) = o.get("curves") {
                            string_list(&json!({"c": c}), "c")
                        } else if let Some(ls) = o.get("loops").and_then(Value::as_array) {
                            let outer = ls.iter().find(|l| l.get("outer").and_then(Value::as_bool).unwrap_or(true)).or(ls.first());
                            outer.map(|l| string_list(l, "curves")).unwrap_or_default()
                        } else {
                            Vec::new()
                        }
                    }
                    _ => Vec::new(),
                };
                if ids.is_empty() {
                    return Err(bad(cmd, "each profile must be an index, a list of curve ids, {curves}, {loops} or {point}"));
                }
                loops.push(ids);
            }
            Ok(ProfileSel::Curves { loops })
        }
        _ => Err(bad(cmd, "`profiles` must be \"all\" or a list")),
    }
}

fn check_expr(s: &Session, e: &str, kind: Kind, cmd: &str, what: &str) -> Result<()> {
    s.doc.eval(e, kind).map(|_| ()).map_err(|err| bad(cmd, format!("{what}: {err}")))
}

/// Add a feature, evaluate, and report its result (an evaluation error fails the command).
fn add_feature(s: &mut Session, p: &Value, kind: FeatureKind) -> Result<Value> {
    let name = str_(p, "name");
    let id = s.doc_mut().add_feature(kind, name)?;
    let mut names = string_list(p, "body_names");
    if let Some(b) = str_(p, "body_name") {
        names.insert(0, b.to_string());
    }
    if !names.is_empty()
        && let Some(f) = s.doc_mut().feature_mut(id)
    {
        f.body_names = names;
    }
    s.active_sketch = None;
    s.refresh();
    let r = s.model.result(id).cloned();
    if let Some(e) = r.as_ref().and_then(|r| r.error.clone()) {
        return Err(EngineError::Other(e));
    }
    let st = s.model.state();
    let bodies: Vec<&str> = st.bodies.iter().map(|b| b.name.as_str()).collect();
    let fname = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    Ok(json!({"feature": id, "name": fname, "bodies": bodies, "ms": r.map(|r| r.ms)}))
}

fn extrude(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "Extrude";
    let sketch = feature_sketch(s, p, cmd)?;
    let through_all = bool_(p, "through_all").unwrap_or(false) || str_(p, "extent").is_some_and(|e| e.eq_ignore_ascii_case("through_all"));
    let distance = if through_all { expr(p, "distance").unwrap_or_else(|| "0".into()) } else { req_expr(cmd, p, "distance")? };
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    let taper = expr(p, "taper");
    if let Some(t) = &taper {
        check_expr(s, t, Kind::Angle, cmd, "taper")?;
    }
    let direction = match str_(p, "direction").map(str::to_ascii_lowercase).as_deref() {
        None | Some("positive") | Some("one_side") => Direction::Positive,
        Some("negative") | Some("reverse") | Some("flip") => Direction::Negative,
        Some("symmetric") => Direction::Symmetric,
        Some(o) => return Err(bad(cmd, format!("unknown direction `{o}`"))),
    };
    let distance2 = expr(p, "distance2");
    if let Some(d) = &distance2 {
        check_expr(s, d, Kind::Length, cmd, "distance2")?;
    }
    let start_offset = expr(p, "start_offset");
    if let Some(d) = &start_offset {
        check_expr(s, d, Kind::Length, cmd, "start_offset")?;
    }
    let kind = FeatureKind::Extrude {
        sketch,
        profiles: profiles(p, cmd)?,
        extent: Extent { distance, direction, distance2, start_offset, through_all, taper },
        operation: operation(p, cmd)?,
        targets: string_list(p, "targets"),
    };
    add_feature(s, p, kind)
}

fn revolve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "Revolve";
    let sketch = feature_sketch(s, p, cmd)?;
    let axis = if let Some(o) = p.get("axis").filter(|v| v.is_object()) {
        let origin = o.get("origin").and_then(vec3).unwrap_or(Vec3::ZERO);
        let dir = o.get("dir").and_then(vec3).ok_or_else(|| bad(cmd, "axis needs `dir`"))?;
        AxisRef::Line { origin, dir }
    } else {
        match str_(p, "axis") {
            Some(a @ ("X" | "Y" | "Z")) => AxisRef::World { axis: a.into() },
            Some(a @ ("x" | "y")) => AxisRef::SketchAxis { axis: a.into() },
            Some(id) => AxisRef::SketchLine { curve: id.into() },
            None => return Err(bad(cmd, "`axis` must be a sketch line id, x, y (sketch axes), X, Y, Z or {origin, dir}")),
        }
    };
    let angle = expr(p, "angle").unwrap_or_else(|| "360 deg".into());
    check_expr(s, &angle, Kind::Angle, cmd, "angle")?;
    let kind =
        FeatureKind::Revolve { sketch, profiles: profiles(p, cmd)?, axis, angle, operation: operation(p, cmd)?, targets: string_list(p, "targets") };
    add_feature(s, p, kind)
}

fn radius_expr(p: &Value, cmd: &str) -> Result<String> {
    match (expr(p, "radius"), expr(p, "diameter")) {
        (Some(r), _) => Ok(r),
        (None, Some(d)) => Ok(format!("({d}) / 2")),
        _ => Err(bad(cmd, "needs `radius` or `diameter`")),
    }
}

fn prim_box(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "PrimitiveBox";
    let (l, w, h) = (req_expr(cmd, p, "length")?, req_expr(cmd, p, "width")?, req_expr(cmd, p, "height")?);
    for (e, n) in [(&l, "length"), (&w, "width"), (&h, "height")] {
        check_expr(s, e, Kind::Length, cmd, n)?;
    }
    let corner = match (p.get("corner").and_then(vec3), p.get("center").and_then(vec3)) {
        (Some(c), _) => c,
        (None, Some(c)) => {
            let v = |e: &str| s.doc.eval(e, Kind::Length).unwrap_or(0.0);
            c - Vec3::new(v(&l), v(&w), v(&h)) * 0.5
        }
        _ => Vec3::ZERO,
    };
    add_feature(s, p, FeatureKind::Box { corner, length: l, width: w, height: h, operation: operation(p, cmd)? })
}

fn prim_cylinder(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "PrimitiveCylinder";
    let radius = radius_expr(p, cmd)?;
    let height = req_expr(cmd, p, "height")?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    check_expr(s, &height, Kind::Length, cmd, "height")?;
    let base = p.get("base").and_then(vec3).unwrap_or(Vec3::ZERO);
    let axis = p.get("axis").and_then(vec3).unwrap_or(Vec3::Z);
    add_feature(s, p, FeatureKind::Cylinder { base, axis, radius, height, operation: operation(p, cmd)? })
}

fn prim_sphere(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "PrimitiveSphere";
    let radius = radius_expr(p, cmd)?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    let center = p.get("center").and_then(vec3).unwrap_or(Vec3::ZERO);
    add_feature(s, p, FeatureKind::Sphere { center, radius, operation: operation(p, cmd)? })
}

fn prim_torus(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "PrimitiveTorus";
    let (major, minor) = (req_expr(cmd, p, "major")?, req_expr(cmd, p, "minor")?);
    check_expr(s, &major, Kind::Length, cmd, "major")?;
    check_expr(s, &minor, Kind::Length, cmd, "minor")?;
    let center = p.get("center").and_then(vec3).unwrap_or(Vec3::ZERO);
    add_feature(s, p, FeatureKind::Torus { center, major, minor, operation: operation(p, cmd)? })
}

/// Edge references → points on the edges.
fn edge_points(s: &Session, p: &Value, cmd: &str) -> Result<Vec<Vec3>> {
    let a = p.get("edges").and_then(Value::as_array).ok_or_else(|| bad(cmd, "`edges` must be a list"))?;
    if a.is_empty() || a.len() > 1000 {
        return Err(bad(cmd, "select 1…1000 edges"));
    }
    let st = s.model.state();
    let mut out = Vec::new();
    for e in a {
        if let Some(pt) = vec3(e).or_else(|| e.get("point").and_then(vec3)) {
            out.push(pt);
            continue;
        }
        let body = e.get("body").and_then(Value::as_str).ok_or_else(|| bad(cmd, "an edge is a point [x,y,z], {point} or {body, index}"))?;
        let idx = e.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "edge `index` must be a number"))? as usize;
        let b = st.body(body).ok_or_else(|| bad(cmd, format!("no body `{body}`")))?;
        let tol = (b.body.size() * 2e-3).max(1e-3);
        let edges = b.body.edges(tol)?;
        out.push(edges.get(idx).ok_or_else(|| bad(cmd, format!("{body} has no edge {idx}")))?.mid);
    }
    Ok(out)
}

fn fillet(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionFilletEdgesCommand";
    let edges = edge_points(s, p, cmd)?;
    let radius = req_expr(cmd, p, "radius")?;
    check_expr(s, &radius, Kind::Length, cmd, "radius")?;
    add_feature(s, p, FeatureKind::Fillet { edges, radius, body: str_(p, "body").map(str::to_string) })
}

fn chamfer(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionChamferCommand";
    let edges = edge_points(s, p, cmd)?;
    let distance = req_expr(cmd, p, "distance")?;
    check_expr(s, &distance, Kind::Length, cmd, "distance")?;
    add_feature(s, p, FeatureKind::Chamfer { edges, distance, body: str_(p, "body").map(str::to_string) })
}

fn combine(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionCombineCommand";
    let target = str_(p, "target").ok_or_else(|| bad(cmd, "`target` must be a body name"))?.to_string();
    let tools = string_list(p, "tools");
    if tools.is_empty() {
        return Err(bad(cmd, "`tools` must list bodies"));
    }
    let op = match str_(p, "operation") {
        None => Operation::Join,
        Some(o) => Operation::parse(o).filter(|o| *o != Operation::NewBody).ok_or_else(|| bad(cmd, "operation must be join, cut or intersect"))?,
    };
    add_feature(s, p, FeatureKind::Combine { target, tools, operation: op, keep_tools: bool_(p, "keep_tools").unwrap_or(false) })
}

fn move_bodies(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "FusionMoveCommand";
    let bodies = string_list(p, "bodies");
    if bodies.is_empty() {
        return Err(bad(cmd, "`bodies` must list bodies"));
    }
    let t = match p.get("translate") {
        Some(Value::Array(a)) if a.len() == 3 => {
            let mut out: [String; 3] = Default::default();
            for (i, v) in a.iter().enumerate() {
                let e = expr(&json!({"v": v}), "v").ok_or_else(|| bad(cmd, "translate entries must be numbers or expressions"))?;
                check_expr(s, &e, Kind::Length, cmd, "translate")?;
                if let Some(slot) = out.get_mut(i) {
                    *slot = e;
                }
            }
            out
        }
        None => ["0".into(), "0".into(), "0".into()],
        _ => return Err(bad(cmd, "`translate` must be [x, y, z]")),
    };
    let angle = expr(p, "angle");
    if let Some(a) = &angle {
        check_expr(s, a, Kind::Angle, cmd, "angle")?;
    }
    add_feature(s, p, FeatureKind::Move { bodies, translate: t, rotate_axis: p.get("axis").and_then(vec3), angle })
}
