//! Browser commands: renaming bodies, moving a sketch to another plane.

use serde_json::{Value, json};

use solvecraft_doc::FeatureKind;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("body.rename", "Rename Body", rename_body).params("body: its name, name: the new name (features that use the body follow)"),
    CommandSpec::new("sketch.redefine", "Redefine Sketch Plane", redefine_sketch)
        .icon("plane")
        .params("sketch: id or name; plane: XY|XZ|YZ | construction plane name | {face: [x,y,z]} | {origin, normal} (the sketch keeps its geometry)"),
];

/// Replace every string equal to `old` with `new`; true when something changed.
fn replace_str(v: &mut Value, old: &str, new: &str, depth: usize) -> bool {
    if depth > 64 {
        return false;
    }
    match v {
        Value::String(s) if s == old => {
            *s = new.to_string();
            true
        }
        Value::Array(a) => a.iter_mut().fold(false, |acc, x| replace_str(x, old, new, depth + 1) | acc),
        Value::Object(o) => o.values_mut().fold(false, |acc, x| replace_str(x, old, new, depth + 1) | acc),
        _ => false,
    }
}

fn rename_body(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "body.rename";
    let old = str_(p, "body").ok_or_else(|| bad(cmd, "`body` must be a body name"))?.to_string();
    let name =
        str_(p, "name").map(str::trim).filter(|n| !n.is_empty() && n.len() <= 128).ok_or_else(|| bad(cmd, "`name` must be 1…128 characters"))?;
    if name == old {
        return Ok(json!({"body": old, "name": name}));
    }
    let st = s.model.state();
    let b = st.body(&old).ok_or_else(|| bad(cmd, format!("no body `{old}`")))?;
    if st.body(name).is_some() {
        return Err(bad(cmd, format!("another body is called `{name}`")));
    }
    // The body is the k-th new body of its feature; the names before it are kept as they are.
    let fid = b.feature;
    let siblings: Vec<String> = st.bodies.iter().filter(|x| x.feature == fid).map(|x| x.name.clone()).collect();
    let k = siblings.iter().position(|n| *n == old).unwrap_or(0);
    let d = s.doc_mut();
    let f = d.feature_mut(fid).ok_or_else(|| bad(cmd, "the body's feature is gone"))?;
    while f.body_names.len() <= k {
        let i = f.body_names.len();
        f.body_names.push(siblings.get(i).cloned().unwrap_or_default());
    }
    if let Some(slot) = f.body_names.get_mut(k) {
        *slot = name.to_string();
    }
    // Later features refer to bodies by name.
    let mut failed = None;
    for f in d.features.iter_mut().filter(|f| f.id != fid) {
        let Ok(mut v) = serde_json::to_value(&f.kind) else { continue };
        if replace_str(&mut v, &old, name, 0) {
            match serde_json::from_value(v) {
                Ok(k) => f.kind = k,
                Err(e) => failed = Some(e.to_string()),
            }
        }
    }
    if let Some(e) = failed {
        return Err(bad(cmd, e));
    }
    if let Some(m) = d.materials.remove(&old) {
        d.materials.insert(name.to_string(), m);
    }
    if let Some(c) = d.body_components.remove(&old) {
        d.body_components.insert(name.to_string(), c);
    }
    Ok(json!({"body": old, "name": name}))
}

fn redefine_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.redefine";
    let key = match p.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad(cmd, "`sketch` must be an id or name")),
    };
    let id = match s.doc.find_feature(&key) {
        Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => f.id,
        _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
    };
    if p.get("plane").is_none() {
        return Err(bad(cmd, "`plane` is required"));
    }
    let plane = super::sketch::plane_ref(s, p, cmd)?;
    let (vals, _) = s.doc.param_values();
    s.doc.resolve_plane(&vals, &plane, 0)?;
    if let Some(FeatureKind::Sketch { plane: slot, .. }) = s.doc_mut().feature_mut(id).map(|f| &mut f.kind) {
        *slot = plane;
    }
    Ok(json!({ "sketch": id }))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::Session;

    fn run(s: &mut Session, id: &str, p: Value) -> Value {
        match s.execute(id, &p) {
            Ok(v) => v,
            Err(e) => panic!("{id} {p}: {e}"),
        }
    }

    fn names(s: &Session) -> Vec<String> {
        s.model.state().bodies.iter().map(|b| b.name.clone()).collect()
    }

    #[test]
    fn renamed_body_keeps_its_users() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10}));
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
        let [a, b] = [names(&s)[0].clone(), names(&s)[1].clone()];
        run(&mut s, "FusionMoveCommand", json!({"bodies": [b.clone()], "translate": [0, 0, 5]}));
        run(&mut s, "body.rename", json!({"body": b, "name": "Lid"}));
        assert_eq!(names(&s), vec![a.clone(), "Lid".to_string()]);
        assert!(s.model.results.iter().all(|r| r.error.is_none()), "the move still finds its body");
        // Duplicate names and unknown bodies are refused; undo restores the old name.
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": a})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Nope", "name": "X"})).is_err());
        assert!(s.execute("body.rename", &json!({"body": "Lid", "name": " "})).is_err());
        run(&mut s, "UndoCommand", json!({}));
        assert_eq!(names(&s)[1], b);
    }

    #[test]
    fn redefined_sketch_moves_its_extrude() {
        let mut s = Session::default();
        let sk = run(&mut s, "SketchCreate", json!({"plane": "XY"}))["sketch"].as_u64().unwrap();
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [10, 20]}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"distance": 5}));
        let zmax = |s: &mut Session| run(s, "MeasureCommand", json!({}))["bodies"][0]["bbox"]["max"][2].as_f64().unwrap();
        assert!((zmax(&mut s) - 5.0).abs() < 1e-6);
        run(&mut s, "sketch.redefine", json!({"sketch": sk, "plane": "XZ"}));
        let z = zmax(&mut s);
        assert!(z > 9.0, "the profile stands up on XZ: {z}");
        assert!(s.execute("sketch.redefine", &json!({"sketch": sk, "plane": "nope"})).is_err());
        assert!(s.execute("sketch.redefine", &json!({"sketch": 999, "plane": "XY"})).is_err());
    }
}
