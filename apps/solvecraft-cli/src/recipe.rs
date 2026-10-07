//! Oracle recipes (`plan/fusion/oracle/*/recipe.json`) → SolveCraft command scripts.
//!
//! A recipe lists features with sketch entities in sketch-local coordinates; point references
//! (`l1.end`) share points. Each feature becomes one or more commands.

use serde_json::{Value, json};

fn plane_param(pl: &Value) -> Value {
    let r = pl.get("ref").and_then(Value::as_str).unwrap_or("");
    let origin_zero = pl.get("origin").and_then(Value::as_array).is_none_or(|o| o.iter().all(|x| x.as_f64().unwrap_or(0.0).abs() < 1e-12));
    let std = match r.to_ascii_uppercase().as_str() {
        "XY" => Some(([1.0, 0.0, 0.0], [0.0, 1.0, 0.0])),
        "XZ" => Some(([1.0, 0.0, 0.0], [0.0, 0.0, 1.0])),
        "YZ" => Some(([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])),
        _ => None,
    };
    let axes_match = |k: &str, want: [f64; 3]| {
        pl.get(k).and_then(Value::as_array).is_some_and(|a| a.iter().zip(want).all(|(x, w)| (x.as_f64().unwrap_or(f64::NAN) - w).abs() < 1e-9))
    };
    match std {
        Some((x, y)) if origin_zero && axes_match("x_dir", x) && axes_match("y_dir", y) => json!(r.to_ascii_uppercase()),
        _ => json!({"origin": pl.get("origin").cloned().unwrap_or(json!([0, 0, 0])), "x_dir": pl.get("x_dir"), "y_dir": pl.get("y_dir")}),
    }
}

/// A point argument: the shared reference if any, else the coordinates.
fn parg(p: Option<&Value>) -> Value {
    match p {
        Some(v) => match (v.get("ref"), v.get("at")) {
            (Some(Value::String(r)), _) => json!(r),
            (_, Some(at)) => at.clone(),
            _ => v.clone(),
        },
        None => Value::Null,
    }
}

fn op(f: &Value) -> Value {
    f.get("operation").cloned().unwrap_or(json!("new"))
}

fn sketch_cmds(f: &Value, out: &mut Vec<Value>) -> Result<(), String> {
    let mut p = json!({"plane": plane_param(f.get("plane").unwrap_or(&Value::Null))});
    if let Some(n) = f.get("name") {
        p["name"] = n.clone();
    }
    out.push(json!({"command": "SketchCreate", "params": p}));
    for e in f.get("entities").and_then(Value::as_array).into_iter().flatten() {
        let id = e.get("id").cloned().unwrap_or(Value::Null);
        let construction = e.get("construction").cloned().unwrap_or(json!(false));
        match e.get("type").and_then(Value::as_str) {
            Some("line") => out.push(json!({"command": "DrawPolyline", "params": {"points": [parg(e.get("start")), parg(e.get("end"))], "ids": [id], "construction": construction}})),
            Some("circle") => out.push(json!({"command": "CircleCenterRadius", "params": {"center": parg(e.get("center")), "radius": e.get("radius"), "id": id, "construction": construction}})),
            Some("arc_three_point") => out.push(json!({"command": "ArcThreePoint", "params": {"start": parg(e.get("start")), "end": parg(e.get("end")), "through": e.get("mid").or(e.get("through")), "id": id}})),
            Some("arc_center_start_sweep") => out.push(json!({"command": "ArcCenterTwoPoint", "params": {"center": parg(e.get("center")), "start": parg(e.get("start")), "sweep": e.get("sweep_deg").or(e.get("sweep")), "id": id}})),
            Some("arc") => {
                if e.get("center").is_some() {
                    out.push(json!({"command": "ArcCenterTwoPoint", "params": {"center": parg(e.get("center")), "start": parg(e.get("start")), "end": parg(e.get("end")), "id": id}}))
                } else {
                    out.push(json!({"command": "ArcThreePoint", "params": {"start": parg(e.get("start")), "end": parg(e.get("end")), "through": parg(e.get("through").or(e.get("mid"))), "id": id}}))
                }
            }
            Some("point") => out.push(json!({"command": "DrawPoint", "params": {"point": parg(e.get("at").map(|a| json!({"at": a})).as_ref().or(Some(e))), "id": id}})),
            Some(other) => return Err(format!("recipe: sketch entity type `{other}` is not supported yet")),
            None => return Err("recipe: sketch entity without type".into()),
        }
    }
    for c in f.get("constraints").and_then(Value::as_array).into_iter().flatten() {
        out.push(constraint_cmd(c)?);
    }
    for d in f.get("dimensions").and_then(Value::as_array).into_iter().flatten() {
        let mut p = json!({"entities": d.get("refs").or(d.get("entities")), "type": d.get("type"), "value": d.get("value")});
        for k in ["orientation", "text_at"] {
            if let Some(v) = d.get(k) {
                p[k] = v.clone();
            }
        }
        out.push(json!({"command": "SketchDimension", "params": p}));
    }
    out.push(json!({"command": "SketchStop", "params": {}}));
    Ok(())
}

/// Extent parameters for the Extrude command.
fn extent(f: &Value, p: &mut Value) {
    let e = f.get("extent").cloned().unwrap_or(Value::Null);
    let ty = e.get("type").and_then(Value::as_str).unwrap_or("distance");
    let dist = e.get("distance").cloned().or_else(|| f.get("distance").cloned()).unwrap_or(Value::Null);
    match ty {
        "symmetric" => {
            let full = e.get("is_full_length").and_then(Value::as_bool).unwrap_or(false);
            p["direction"] = json!("symmetric");
            p["distance"] = if full { json!(format!("({}) / 2", expr_text(&dist))) } else { dist };
        }
        "two_sides" => {
            p["distance"] = e.get("side1").cloned().unwrap_or(Value::Null);
            p["distance2"] = e.get("side2_opposite").or(e.get("side2")).cloned().unwrap_or(Value::Null);
        }
        "through_all" => {
            p["through_all"] = json!(true);
            p["direction"] = e.get("direction").cloned().unwrap_or(json!("positive"));
        }
        _ => {
            p["distance"] = dist;
            p["direction"] = e.get("direction").cloned().unwrap_or(json!("positive"));
        }
    }
    if let Some(t) = e.get("taper_deg") {
        p["taper"] = t.clone();
    }
}

fn expr_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn refs(c: &Value) -> Vec<Value> {
    c.get("refs").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// A recipe constraint `{type, refs}` → a constraint command.
fn constraint_cmd(c: &Value) -> Result<Value, String> {
    let ty = c.get("type").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let r = refs(c);
    let at = |i: usize| r.get(i).cloned().unwrap_or(Value::Null);
    let is_point = |v: &Value| v.as_str().is_some_and(|s| s.contains('.') || s == "origin" || s.starts_with('p'));
    let (cmd, p) = match ty.as_str() {
        "horizontal" | "vertical" if r.len() >= 2 => ("ConstraintHorizontalVertical", json!({"points": [at(0), at(1)], "mode": ty})),
        "horizontal" | "vertical" => ("ConstraintHorizontalVertical", json!({"line": at(0), "mode": ty})),
        "horizontal_points" => ("ConstraintHorizontalVertical", json!({"points": [at(0), at(1)], "mode": "horizontal"})),
        "vertical_points" => ("ConstraintHorizontalVertical", json!({"points": [at(0), at(1)], "mode": "vertical"})),
        "coincident" | "point_on_curve" => {
            // The point goes first.
            if is_point(&at(0)) {
                ("ConstraintCoincident", json!({"a": at(0), "b": at(1)}))
            } else {
                ("ConstraintCoincident", json!({"a": at(1), "b": at(0)}))
            }
        }
        "midpoint" => {
            if is_point(&at(0)) {
                ("ConstraintMidPoint", json!({"point": at(0), "line": at(1)}))
            } else {
                ("ConstraintMidPoint", json!({"point": at(1), "line": at(0)}))
            }
        }
        "symmetric" | "symmetry" => ("ConstraintSymmetry", json!({"a": at(0), "b": at(1), "line": at(2)})),
        "fix" => ("ConstraintFix", json!({"entity": at(0), "fixed": true})),
        "tangent" => ("ConstraintTangent", json!({"a": at(0), "b": at(1)})),
        "equal" => ("ConstraintEqual", json!({"a": at(0), "b": at(1)})),
        "parallel" => ("ConstraintParallel", json!({"a": at(0), "b": at(1)})),
        "perpendicular" => ("ConstraintPerpendicular", json!({"a": at(0), "b": at(1)})),
        "concentric" => ("ConstraintConcentric", json!({"a": at(0), "b": at(1)})),
        "collinear" => ("ConstraintCollinear", json!({"a": at(0), "b": at(1)})),
        other => return Err(format!("recipe: constraint `{other}` is not supported yet")),
    };
    Ok(json!({"command": cmd, "params": p}))
}

/// The profiles a feature uses (`profiles` list or a single `profile`).
fn profiles_of(f: &Value) -> Value {
    match (f.get("profiles"), f.get("profile")) {
        (Some(p), _) => p.clone(),
        (None, Some(p)) => json!([p]),
        _ => json!("all"),
    }
}

/// The sketch a feature uses (name), from its profiles.
fn profile_sketch(f: &Value) -> Value {
    profiles_of(f)
        .as_array()
        .and_then(|a| a.first())
        .and_then(|p| p.get("sketch"))
        .cloned()
        .or_else(|| f.get("sketch").cloned())
        .unwrap_or(Value::Null)
}

pub fn to_script(recipe: &Value) -> Result<Value, String> {
    let mut out: Vec<Value> = Vec::new();
    for p in recipe.get("parameters").and_then(Value::as_array).into_iter().flatten() {
        out.push(json!({"command": "ChangeParameterCommand", "params": {"name": p.get("name"), "expression": p.get("expression").or(p.get("value")), "unit": p.get("unit").cloned().unwrap_or(json!("mm"))}}));
    }
    let features = recipe.get("features").and_then(Value::as_array).ok_or("recipe: missing `features`")?;
    for f in features {
        let name = f.get("name").cloned().unwrap_or(Value::Null);
        let bodies = f.get("result_bodies").cloned().unwrap_or(json!([]));
        match f.get("op").and_then(Value::as_str) {
            Some("sketch") => sketch_cmds(f, &mut out)?,
            Some("extrude") => {
                let mut p = json!({"sketch": profile_sketch(f), "profiles": profiles_of(f), "operation": op(f), "name": name, "body_names": bodies});
                extent(f, &mut p);
                out.push(json!({"command": "Extrude", "params": p}));
            }
            Some("combine") => out.push(json!({"command": "FusionCombineCommand", "params": {
                "target": f.get("target_body").or(f.get("target")), "tools": f.get("tool_bodies").or(f.get("tools")),
                "operation": op(f), "keep_tools": f.get("keep_tools").cloned().unwrap_or(json!(false)), "name": name}})),
            Some("revolve") => {
                let axis = match f.get("axis") {
                    Some(a) if a.get("dir").is_some() => json!({"origin": a.get("origin").cloned().unwrap_or(json!([0, 0, 0])), "dir": a.get("dir")}),
                    Some(a) => a.get("line").or(a.get("ref")).cloned().unwrap_or_else(|| a.clone()),
                    None => Value::Null,
                };
                out.push(json!({"command": "Revolve", "params": {
                    "sketch": profile_sketch(f), "profiles": profiles_of(f), "axis": axis,
                    "angle": f.get("angle").cloned().unwrap_or(json!("360 deg")), "operation": op(f), "name": name, "body_names": bodies}}));
            }
            Some("rectangular_pattern") => {
                let d1 = f.get("direction1").cloned().unwrap_or(Value::Null);
                let d2 = f.get("direction2").cloned().unwrap_or(Value::Null);
                let mut p = json!({"features": f.get("features"), "dir1": d1.get("axis"), "count1": d1.get("count"), "spacing1": d1.get("spacing"), "name": name});
                if !d2.is_null() {
                    p["dir2"] = d2.get("axis").cloned().unwrap_or(Value::Null);
                    p["count2"] = d2.get("count").cloned().unwrap_or(Value::Null);
                    p["spacing2"] = d2.get("spacing").cloned().unwrap_or(Value::Null);
                }
                out.push(json!({"command": "PatternRectangular", "params": p}));
            }
            Some("circular_pattern") => {
                let axis = f.get("axis").map(|a| json!({"origin": a.get("origin").cloned().unwrap_or(json!([0, 0, 0])), "dir": a.get("dir")})).unwrap_or(Value::Null);
                out.push(json!({"command": "PatternCircular", "params": {"features": f.get("features"), "axis": axis, "count": f.get("count"), "angle": f.get("total_angle").cloned().unwrap_or(json!(360)), "name": name}}));
            }
            Some("shell") => {
                let faces: Vec<Value> = f.get("faces_removed").and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.get("point").cloned()).collect();
                out.push(json!({"command": "FusionShellBodyCommand", "params": {"faces": faces, "thickness": f.get("inside_thickness"), "name": name}}));
            }
            Some("draft") => {
                let faces: Vec<Value> = f.get("faces").and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.get("point").cloned()).collect();
                let np = f.get("neutral_plane").cloned().unwrap_or(Value::Null);
                let n = np.get("plane_normal_outward").or(np.get("normal_at_point_on_face")).cloned().unwrap_or(json!([0, 0, 1]));
                let neutral = json!({"origin": np.get("point").or(np.get("centroid")).cloned().unwrap_or(json!([0, 0, 0])), "normal": n.clone()});
                let mut angle = f.get("angle").and_then(Value::as_f64).unwrap_or(0.0);
                if f.get("flipped").and_then(Value::as_bool).unwrap_or(false) {
                    angle = -angle;
                }
                out.push(json!({"command": "FusionDraftCommand", "params": {"faces": faces, "angle": angle, "neutral": neutral, "pull": n, "name": name}}));
            }
            Some("hole") => {
                let n = f.get("face").and_then(|fc| fc.get("plane_normal_outward").or(fc.get("normal_at_point_on_face"))).cloned();
                let dir = n.and_then(|n| n.as_array().map(|a| a.iter().map(|x| -x.as_f64().unwrap_or(0.0)).collect::<Vec<f64>>()));
                let ty = f.get("hole_type").and_then(Value::as_str).unwrap_or("simple");
                let mut p = json!({"position": f.get("position"), "diameter": f.get("diameter"), "type": ty, "name": name});
                if let Some(d) = dir {
                    p["direction"] = json!(d);
                }
                if f.get("extent").and_then(Value::as_str) != Some("through_all") {
                    p["depth"] = f.get("depth").cloned().unwrap_or(Value::Null);
                    if let Some(t) = f.get("tip_angle") {
                        p["tip_angle"] = t.clone();
                    }
                }
                for (k, src) in [("cb_diameter", "counterbore_diameter"), ("cb_depth", "counterbore_depth"), ("cs_diameter", "countersink_diameter"), ("cs_angle", "countersink_angle")] {
                    if let Some(v) = f.get(src) {
                        p[k] = v.clone();
                    }
                }
                out.push(json!({"command": "FusionHoleCommand", "params": p}));
            }
            Some("construction_plane") => match f.get("method").and_then(Value::as_str) {
                Some("offset") => out.push(json!({"command": "ConstructionPlaneOffsetFromPlaneCommand", "params": {"base": f.get("base"), "offset": f.get("offset"), "name": name}})),
                _ => {
                    // Use the resulting plane as recorded.
                    let (o, n) = (f.get("result_origin").cloned().unwrap_or(json!([0, 0, 0])), f.get("result_normal").cloned().unwrap_or(Value::Null));
                    if n.is_null() {
                        return Err(format!("recipe: construction plane method {:?} without result_normal", f.get("method")));
                    }
                    out.push(json!({"command": "ConstructionPlaneOffsetFromPlaneCommand", "params": {"base": {"origin": o, "normal": n}, "offset": 0, "name": name}}));
                }
            },
            Some("split_body") => out.push(json!({"command": "FusionSplitBodyCommand", "params": {"body": f.get("body"), "plane": f.get("tool"), "name": name}})),
            Some("mirror") => {
                let pl = f.get("plane").cloned().unwrap_or(Value::Null);
                let plane = match pl.get("ref").and_then(Value::as_str) {
                    Some(r @ ("XY" | "XZ" | "YZ")) => json!(r),
                    _ => json!({"origin": pl.get("origin").cloned().unwrap_or(json!([0, 0, 0])), "normal": pl.get("normal")}),
                };
                out.push(json!({"command": "MirrorCommand", "params": {"features": f.get("features"), "plane": plane, "name": name}}));
            }
            Some("fillet") => {
                out.push(json!({"command": "FusionFilletEdgesCommand", "params": {"edges": edge_points(f), "radius": f.get("radius"), "name": name}}))
            }
            Some("chamfer") => {
                out.push(json!({"command": "FusionChamferCommand", "params": {"edges": edge_points(f), "distance": f.get("distance"), "name": name}}))
            }
            Some(other) => return Err(format!("recipe: feature op `{other}` is not supported yet")),
            None => return Err("recipe: feature without `op`".into()),
        }
    }
    Ok(json!({"commands": out}))
}

fn edge_points(f: &Value) -> Value {
    Value::Array(
        f.get("edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|e| e.get("point").or(e.get("mid")).cloned().unwrap_or_else(|| e.clone()))
            .collect(),
    )
}
