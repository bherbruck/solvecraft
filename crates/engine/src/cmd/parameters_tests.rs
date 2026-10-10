use serde_json::{Value, json};

use crate::{EngineError, Session};

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should fail, gave {v}"),
        Err(e) => e.to_string(),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn row<'a>(list: &'a Value, name: &str) -> &'a Value {
    list["parameters"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name)).unwrap_or(&Value::Null)
}

fn expr_of(s: &mut Session, name: &str) -> String {
    let l = run(s, "parameters.list", json!({}));
    row(&l, name)["expression"].as_str().unwrap_or_default().to_string()
}

/// A sketched block 40 x (len / 2) x 10 whose width is an expression of a user parameter.
fn block(s: &mut Session) {
    run(s, "parameters.add", json!({"name": "len", "expression": "60 mm", "comment": "overall length", "favorite": true}));
    run(s, "sketch.create", json!({"plane": "XY", "name": "Base"}));
    run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(s, "sketch.constraint.coincident", json!({"a": "p1", "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": ["l1"], "value": 40}));
    run(s, "sketch.dimension", json!({"entities": ["l2"], "value": "len/2"}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 10, "body_name": "Block"}));
}

#[test]
fn width_follows_an_expression() {
    let mut s = Session::default();
    block(&mut s);
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    assert_eq!(r["errors"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 50.0 * 10.0) < 1e-9);
    // Units convert: 4 in = 101.6 mm.
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "4 in"}));
    assert!(rel(volume(&mut s), 40.0 * 50.8 * 10.0) < 1e-9);
    // A parameter in inches reads bare numbers as inches.
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "5", "unit": "in"}));
    assert!(rel(volume(&mut s), 40.0 * 63.5 * 10.0) < 1e-9);
    let l = run(&mut s, "parameters.list", json!({}));
    assert_eq!(row(&l, "len")["value"], 5.0, "{l}");
    assert_eq!(row(&l, "len")["unit"], "in");
    // A failing parameter fails the features using it, with its error.
    let e = err(&mut s, "parameters.change", json!({"name": "len", "expression": "5 deg"}));
    assert!(e.contains("expected a length, got an angle"), "{e}");
}

/// Setting a parameter back rebuilds nothing (the earlier results are reused) and says so: the
/// features it restored are reported next to `recomputed`, so `recomputed: 0` is not mistaken
/// for "nothing changed". timeline.edit reports the same.
#[test]
fn setting_a_parameter_back_reports_restored_features() {
    let mut s = Session::default();
    block(&mut s);
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "100 mm"}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(2), Some(0)), "{r}");
    let r = run(&mut s, "parameters.change", json!({"name": "len", "expression": "60 mm"}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(0), Some(2)), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
    let r = run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "20"}}}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(1), Some(0)), "{r}");
    let r = run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "10"}}}));
    assert_eq!((r["recomputed"].as_u64(), r["restored"].as_u64()), (Some(0), Some(1)), "{r}");
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 10.0) < 1e-9);
}

#[test]
fn feature_inputs_are_named_and_editable() {
    let mut s = Session::default();
    block(&mut s);
    let l = run(&mut s, "parameters.list", json!({"filter": "feature"}));
    let dist = l["parameters"].as_array().unwrap().iter().find(|r| r["input"] == "Distance").unwrap().clone();
    assert_eq!(dist["feature"], "Extrude1", "{l}");
    let d = dist["name"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": d, "expression": "len / 3"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 20.0) < 1e-9);
    // Editing the feature keeps its parameter name.
    run(&mut s, "timeline.edit", json!({"feature": "Extrude1", "set": {"extent": {"distance": "12 mm"}}}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 12.0) < 1e-9);
    assert_eq!(expr_of(&mut s, &d), "12 mm");
    // Another feature can use it.
    run(&mut s, "solid.box", json!({"length": d, "width": 5, "height": 5, "corner": [100, 0, 0]}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 12.0 + 12.0 * 25.0) < 1e-9);
    let u = run(&mut s, "parameters.users", json!({"name": d}));
    assert_eq!(u["users"].as_array().map(Vec::len), Some(1), "{u}");
    assert_eq!(u["drives"].as_array().map(Vec::len), Some(2), "{u}");
    // Feature inputs can't be deleted on their own.
    assert!(err(&mut s, "parameters.delete", json!({"name": d})).contains("feature's input"));
}

#[test]
fn unit_errors_and_cycles_are_refused() {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
    let e = err(&mut s, "parameters.add", json!({"name": "bad", "expression": "a + 5 deg"}));
    assert!(e.contains("cannot add a length and an angle"), "{e}");
    let e = err(&mut s, "parameters.add", json!({"name": "ang", "expression": "a", "unit": "deg"}));
    assert!(e.contains("expected an angle, got a length"), "{e}");
    assert!(err(&mut s, "parameters.add", json!({"name": "x", "expression": "1", "unit": "furlong"})).contains("unknown unit"));
    run(&mut s, "parameters.add", json!({"name": "b", "expression": "a * 2"}));
    let e = err(&mut s, "parameters.change", json!({"name": "a", "expression": "b + 1"}));
    assert!(e.contains("circular reference: a → b → a"), "{e}");
    assert!(err(&mut s, "parameters.add", json!({"name": "selfref", "expression": "selfref"})).contains("circular"));
    assert!(err(&mut s, "parameters.add", json!({"name": "a", "expression": "1"})).contains("already"));
    assert!(err(&mut s, "parameters.add", json!({"name": "sin", "expression": "1"})).contains("function"));
    assert!(err(&mut s, "parameters.add", json!({"name": "2x", "expression": "1"})).contains("not a valid parameter name"));
    // Units are inferred for new parameters.
    run(&mut s, "parameters.add", json!({"name": "tilt", "expression": "30 deg"}));
    run(&mut s, "parameters.add", json!({"name": "n", "expression": "b / a"}));
    assert_eq!(s.doc.param("tilt").map(|p| p.unit.as_str()), Some("deg"));
    assert_eq!(s.doc.param("n").map(|p| p.unit.as_str()), Some(""));
    let r = run(&mut s, "expr.evaluate", json!({"expression": "atan2(b; a) + tilt", "kind": "angle"}));
    assert!((r["display_value"].as_f64().unwrap_or(0.0) - (2.0f64.atan().to_degrees() + 30.0)).abs() < 1e-9, "{r}");
    let r = run(&mut s, "expr.evaluate", json!({"expression": "a + tilt"}));
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap_or("").contains("cannot add"), "{r}");
    let r = run(&mut s, "expr.evaluate", json!({"expression": "2", "unit": "in"}));
    assert!((r["value"].as_f64().unwrap_or(0.0) - 50.8).abs() < 1e-9, "{r}");
}

#[test]
fn rename_updates_every_reference() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "twice", "expression": "len * 2"}));
    let r = run(&mut s, "parameters.rename", json!({"name": "len", "new_name": "length"}));
    assert_eq!(r["references_updated"], 2, "{r}");
    assert_eq!(expr_of(&mut s, "twice"), "length * 2");
    let l = run(&mut s, "parameters.list", json!({"favorites": true}));
    assert_eq!(l["count"], 1, "the favourite follows the rename");
    assert_eq!(row(&l, "length")["comment"], "overall length");
    run(&mut s, "parameters.change", json!({"name": "length", "expression": "80"}));
    assert!(rel(volume(&mut s), 40.0 * 40.0 * 10.0) < 1e-9);
    // Model and feature parameters can be renamed too: dimensions and features follow.
    let dim = s.doc.features.first().and_then(|f| match &f.kind {
        solvecraft_doc::FeatureKind::Sketch { sketch, .. } => sketch.constraints.iter().filter_map(|c| c.param.clone()).next(),
        _ => None,
    });
    let dim = dim.unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": dim, "new_name": "base_w", "expression": "50"}));
    assert!(rel(volume(&mut s), 50.0 * 40.0 * 10.0) < 1e-9);
    let ext = s.doc.features.get(1).and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.rename", json!({"name": ext, "new_name": "thick"}));
    run(&mut s, "parameters.change", json!({"name": "thick", "expression": "base_w / 10"}));
    assert!(rel(volume(&mut s), 50.0 * 40.0 * 5.0) < 1e-9);
    assert!(err(&mut s, "parameters.rename", json!({"name": "twice", "new_name": "length"})).contains("already"));
    assert!(err(&mut s, "parameters.rename", json!({"name": "nope", "new_name": "x"})).contains("unknown"));
    // Undo puts the old name back.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert!(s.doc.features.get(1).is_some_and(|f| f.param_names.first() == Some(&ext)));
}

#[test]
fn deleting_a_used_parameter_lists_its_users() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "spare", "expression": "len + 1"}));
    let e = err(&mut s, "parameters.delete", json!({"name": "len"}));
    assert!(e.contains("parameter spare") && e.contains("parameter d"), "{e}");
    let u = run(&mut s, "parameters.users", json!({"name": "len"}));
    assert_eq!(u["users"].as_array().map(Vec::len), Some(2), "{u}");
    assert_eq!(u["drives"][0]["name"], "Base", "{u}");
    let dim = u["users"].as_array().unwrap().iter().find(|x| x["name"] != "spare").unwrap()["name"].as_str().unwrap().to_string();
    let e = err(&mut s, "parameters.change", json!({"delete": dim}));
    assert!(e.contains("Base dimension"), "{e}");
    run(&mut s, "parameters.delete", json!({"name": "spare"}));
    assert!(s.doc.param("spare").is_none());
    let g = run(&mut s, "parameters.graph", json!({}));
    assert!(g["edges"].as_array().unwrap().iter().any(|e| e[0] == dim.as_str() && e[1] == "len"), "{g}");
    assert!(g["edges"].as_array().unwrap().iter().any(|e| e[0] == "Feature:Base" && e[1] == dim.as_str()), "{g}");
}

#[test]
fn favorites_comments_and_completion() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.favorite", json!({"name": "len", "favorite": false}));
    assert_eq!(run(&mut s, "parameters.list", json!({"favorites": true}))["count"], 0);
    let ext = s.doc.features.get(1).and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "parameters.comment", json!({"name": ext, "comment": "plate thickness"}));
    let l = run(&mut s, "parameters.list", json!({}));
    assert_eq!(row(&l, &ext)["comment"], "plate thickness");
    let c = run(&mut s, "parameters.complete", json!({"prefix": "2 * le"}));
    assert_eq!(c["word"], "le");
    assert_eq!(c["suggestions"][0]["text"], "len", "{c}");
    let c = run(&mut s, "parameters.complete", json!({"prefix": "at"}));
    assert!(c["suggestions"].as_array().unwrap().iter().any(|x| x["text"] == "atan2("), "{c}");
}

#[test]
fn csv_and_json_round_trip() {
    let mut s = Session::default();
    block(&mut s);
    run(&mut s, "parameters.add", json!({"name": "note", "expression": "len / 4", "comment": "has, a \"comma\""}));
    let csv = run(&mut s, "parameters.export", json!({"format": "csv"}))["text"].as_str().unwrap_or_default().to_string();
    assert!(csv.contains("note,mm,len / 4,15,\"has, a \"\"comma\"\"\",false"), "{csv}");
    let json_text = run(&mut s, "parameters.export", json!({"format": "json"}))["text"].as_str().unwrap_or_default().to_string();

    let mut t = Session::default();
    run(&mut t, "parameters.import", json!({"text": csv}));
    assert_eq!(t.doc.param("note").map(|p| p.comment.as_str()), Some("has, a \"comma\""));
    let mut u = Session::default();
    run(&mut u, "parameters.import", json!({"text": json_text}));
    assert_eq!(u.doc.param("note").map(|p| p.expr.as_str()), Some("len / 4"));

    // Importing into the design updates values and rebuilds; all or nothing.
    run(&mut s, "parameters.import", json!({"text": "name,expression\nlen,80 mm\nextra,3\n"}));
    assert!(rel(volume(&mut s), 40.0 * 40.0 * 10.0) < 1e-9);
    assert!(s.execute("parameters.import", &json!({"text": "name,expression\nlen,90 mm\nbroken,len + 1 deg\n"})).is_err());
    assert_eq!(s.doc.param("len").map(|p| p.expr.as_str()), Some("80 mm"));
    assert!(matches!(s.execute("parameters.import", &json!({})), Err(EngineError::BadParams { .. })));

    let dir = std::env::temp_dir().join(format!("solvecraft-params-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("p.json");
    let r = run(&mut s, "parameters.export", json!({"path": path.to_string_lossy()}));
    assert_eq!(r["format"], "json");
    let mut v = Session::default();
    run(&mut v, "parameters.import", json!({"path": path.to_string_lossy()}));
    assert_eq!(v.doc.param("len").map(|p| p.expr.as_str()), Some("80 mm"));
    let _ = std::fs::remove_dir_all(&dir);
}
