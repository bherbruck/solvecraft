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
        let ty = c.get("type").and_then(Value::as_str).unwrap_or("");
        let cmd = match ty.to_ascii_lowercase().as_str() {
            "horizontal" | "vertical" => {
                let mut p = c.clone();
                p["mode"] = json!(ty.to_ascii_lowercase());
                out.push(json!({"command": "ConstraintHorizontalVertical", "params": p}));
                continue;
            }
            "coincident" => "ConstraintCoincident",
            "tangent" => "ConstraintTangent",
            "equal" => "ConstraintEqual",
            "parallel" => "ConstraintParallel",
            "perpendicular" => "ConstraintPerpendicular",
            "fix" => "ConstraintFix",
            "midpoint" => "ConstraintMidPoint",
            "concentric" => "ConstraintConcentric",
            "collinear" => "ConstraintCollinear",
            "symmetry" | "symmetric" => "ConstraintSymmetry",
            other => return Err(format!("recipe: constraint `{other}` is not supported yet")),
        };
        out.push(json!({"command": cmd, "params": c}));
    }
    for d in f.get("dimensions").and_then(Value::as_array).into_iter().flatten() {
        out.push(json!({"command": "SketchDimension", "params": d}));
    }
    out.push(json!({"command": "SketchStop", "params": {}}));
    Ok(())
}

fn extent(f: &Value) -> (Value, Value, Value) {
    let e = f.get("extent").cloned().unwrap_or(Value::Null);
    let d = e.get("distance").cloned().or_else(|| f.get("distance").cloned()).unwrap_or(Value::Null);
    let dir = e.get("direction").cloned().unwrap_or(json!("positive"));
    (d, dir, e.get("distance2").cloned().unwrap_or(Value::Null))
}

/// The sketch a feature uses (name), from its profiles.
fn profile_sketch(f: &Value) -> Value {
    f.get("profiles")
        .and_then(Value::as_array)
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
                let (d, dir, d2) = extent(f);
                let mut p = json!({"sketch": profile_sketch(f), "profiles": f.get("profiles").cloned().unwrap_or(json!("all")), "distance": d, "direction": dir, "operation": op(f), "name": name, "body_names": bodies});
                if !d2.is_null() {
                    p["distance2"] = d2;
                }
                out.push(json!({"command": "Extrude", "params": p}));
            }
            Some("revolve") => out.push(json!({"command": "Revolve", "params": {
                "sketch": profile_sketch(f), "profiles": f.get("profiles").cloned().unwrap_or(json!("all")),
                "axis": f.get("axis").and_then(|a| a.get("ref").or(a.get("line")).cloned()).or_else(|| f.get("axis").cloned()),
                "angle": f.get("angle").cloned().unwrap_or(json!("360 deg")), "operation": op(f), "name": name, "body_names": bodies}})),
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
