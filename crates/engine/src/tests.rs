use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::*;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    let m = run(s, "MeasureCommand", json!({}));
    m["total"]["volume_mm3"].as_f64().unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// Sketch a 40 x 30 rectangle with dimensions, extrude 20, fillet a vertical edge, cut a hole.
#[test]
fn box_fillet_cut_part() {
    let mut s = Session::default();
    run(&mut s, "ChangeParameterCommand", json!({"name": "width", "expression": "40 mm"}));
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    let r = run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [38, 29]}));
    let l: Vec<String> = r["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    let corner = s.doc.sketch(s.active_sketch.unwrap()).unwrap().curves[0].clone();
    let start = match corner.kind {
        solvecraft_sketch::CurveKind::Line { a, .. } => s.doc.sketch(s.active_sketch.unwrap()).unwrap().points[a].id.clone(),
        _ => panic!(),
    };
    run(&mut s, "ConstraintCoincident", json!({"a": start, "b": "origin"}));
    run(&mut s, "SketchDimension", json!({"entities": [l[0]], "value": "width"}));
    run(&mut s, "SketchDimension", json!({"entities": [l[1]], "value": 30}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0, "{si}");
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1);
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 20}));
    assert!(rel(volume(&mut s), 24000.0) < 1e-9);
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[0, 0, 10]], "radius": 3}));
    let filleted = 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0;
    assert!(rel(volume(&mut s), filleted) < 2e-4);
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "CircleCenterRadius", json!({"center": [20, 15], "radius": 5}));
    run(&mut s, "Extrude", json!({"distance": 20, "operation": "cut"}));
    let expect = filleted - PI * 25.0 * 20.0;
    let v = volume(&mut s);
    assert!(rel(v, expect) < 5e-4, "{v} vs {expect}");
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["bodies"][0]["faces"], 8, "{m}");

    // Parameter edit re-evaluates the timeline from the sketch.
    let r = run(&mut s, "ChangeParameterCommand", json!({"name": "width", "expression": "50"}));
    assert_eq!(r["errors"].as_array().unwrap().len(), 0, "{r}");
    let expect2 = 30000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0 - PI * 25.0 * 20.0;
    assert!(rel(volume(&mut s), expect2) < 5e-4);

    // Undo the parameter change, redo it.
    run(&mut s, "UndoCommand", json!({}));
    assert!(rel(volume(&mut s), expect) < 5e-4);
    run(&mut s, "RedoCommand", json!({}));
    assert!(rel(volume(&mut s), expect2) < 5e-4);

    // Export STL and STEP.
    let dir = std::env::temp_dir().join(format!("solvecraft-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("part.stl");
    run(&mut s, "ExportCommand", json!({"path": stl.to_string_lossy()}));
    assert!(std::fs::metadata(&stl).unwrap().len() > 1000);
    let step = dir.join("part.step");
    run(&mut s, "ExportCommand", json!({"path": step.to_string_lossy()}));
    assert!(std::fs::read_to_string(&step).unwrap().contains("ISO-10303-21"));
    let design = dir.join("part.solvecraft");
    run(&mut s, "SaveDocumentAsCommand", json!({"path": design.to_string_lossy()}));
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": design.to_string_lossy()}));
    assert!(rel(volume(&mut s2), expect2) < 5e-4);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn revolve_and_primitives() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XZ"}));
    run(&mut s, "DrawPolyline", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true}));
    run(&mut s, "Revolve", json!({"axis": "y"}));
    assert!(rel(volume(&mut s), PI * 125.0 * 5.0) < 5e-4);
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0]}));
    run(&mut s, "PrimitiveCylinder", json!({"diameter": 4, "height": 20, "base": [35, 5, -5]}));
    run(&mut s, "FusionCombineCommand", json!({"target": "Body2", "tools": ["Body3"], "operation": "cut"}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["body_count"], 2, "{m}");
    assert!(rel(m["bodies"][1]["volume_mm3"].as_f64().unwrap(), 1000.0 - PI * 4.0 * 10.0) < 5e-4);
    run(&mut s, "FusionChamferCommand", json!({"edges": [[30, 5, 10]], "distance": 1}));
    run(&mut s, "timeline.rollback", json!({"position": 3}));
    assert_eq!(run(&mut s, "MeasureCommand", json!({}))["body_count"], 2);
    run(&mut s, "timeline.rollback", json!({}));
    run(&mut s, "timeline.edit", json!({"feature": "Revolve1", "set": {"angle": "180 deg"}}));
    let m = run(&mut s, "MeasureCommand", json!({"bodies": ["Body1"]}));
    assert!(rel(m["total"]["volume_mm3"].as_f64().unwrap(), PI * 125.0 * 5.0 / 2.0) < 5e-4);
}

#[test]
fn sketch_tools_and_constraints() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "YZ"}));
    run(&mut s, "ShapeRectangleCenter", json!({"center": [0, 0], "corner": [10, 5]}));
    run(&mut s, "ShapeRectangleThreePoint", json!({"p0": [20, 0], "p1": [30, 5], "p2": [28, 10]}));
    run(&mut s, "CircleTwoPoint", json!({"p0": [40, 0], "p1": [44, 0]}));
    run(&mut s, "CircleThreePoint", json!({"p0": [50, 0], "p1": [52, 2], "p2": [54, 0]}));
    run(&mut s, "ArcThreePoint", json!({"start": [60, 0], "end": [64, 0], "through": [62, 2]}));
    run(&mut s, "ArcCenterTwoPoint", json!({"center": [70, 0], "start": [72, 0], "end": [70, 2]}));
    run(&mut s, "ShapePolygonInscribed", json!({"center": [80, 0], "radius": 3, "sides": 6}));
    run(&mut s, "ShapePolygonCircumscribed", json!({"center": [90, 0], "radius": 3, "sides": 5}));
    run(&mut s, "ShapePolygonEdge", json!({"p0": [100, 0], "p1": [103, 0], "sides": 4}));
    run(&mut s, "ShapeSlotCenterToCenter", json!({"p0": [110, 0], "p1": [120, 0], "width": 4}));
    run(&mut s, "ShapeSlotOverall", json!({"p0": [130, 0], "p1": [140, 0], "width": 4}));
    run(&mut s, "DrawPoint", json!({"point": [150, 0]}));
    let r = run(&mut s, "DrawPolyline", json!({"points": [[0, 20], [10, 21], [20, 30]], "ids": ["m1", "m2"]}));
    assert_eq!(r["curves"], json!(["m1", "m2"]));
    run(&mut s, "ConstraintHorizontalVertical", json!({"line": "m1"}));
    run(&mut s, "ConstraintPerpendicular", json!({"a": "m1", "b": "m2"}));
    let _ = s.execute("SketchDimension", &json!({"entities": ["m1", "m2"], "type": "angle", "value": "90 deg"}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["status"], "solved", "{}", si["failing"]);
    // Over-constraining is rejected and leaves the sketch unchanged.
    let before = s.doc.clone();
    assert!(s.execute("ConstraintParallel", &json!({"a": "m1", "b": "m2"})).is_err());
    assert_eq!(*s.doc, *before);
    // Profiles: rectangles, circles, closed arcs/polygons/slots.
    let n = si["profiles"].as_array().unwrap().len();
    assert!(n >= 8, "{n} profiles");
    run(&mut s, "sketch.construction", json!({"curves": ["m1"]}));
    run(&mut s, "sketch.delete", json!({"entities": ["m2"]}));
    run(&mut s, "ConstraintFix", json!({"entity": "m1"}));
    run(&mut s, "sketch.move_point", json!({"point": "m1.end", "to": [15, 20]}));
    run(&mut s, "SketchStop", json!({}));
    assert!(s.execute("DrawPolyline", &json!({"points": [[0, 0], [1, 1]]})).is_err(), "needs an active sketch");
}

/// The recipe format of the Fusion oracle (01-box) drives the same commands.
#[test]
fn script_runs_commands() {
    let mut s = Session::default();
    let script = json!({"commands": [
        {"command": "SketchCreate", "params": {"plane": "XY"}},
        {"command": "DrawPolyline", "params": {"points": [[0, 0], [40, 0], [40, 30], [0, 30]], "closed": true, "ids": ["l1", "l2", "l3", "l4"]}},
        {"command": "Extrude", "params": {"distance": 20, "profiles": [{"loops": [{"outer": true, "curves": ["l1", "l2", "l3", "l4"]}]}], "body_name": "Box"}},
    ]});
    let out = s.run_script(&script).unwrap();
    assert_eq!(out.len(), 3);
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["bodies"][0]["name"], "Box");
    assert_eq!((m["total"]["faces"].as_u64(), m["total"]["edges"].as_u64(), m["total"]["vertices"].as_u64()), (Some(6), Some(12), Some(8)));
    assert!(s.run_script(&json!([{"command": "Nope"}])).is_err());
}

fn hostile_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(1e308),
        json!(-1e308),
        json!(""),
        json!("nope"),
        json!("1/0"),
        json!("((((((((((1"),
        json!([]),
        json!([1e308, -1e308]),
        json!([0, 0, 0]),
        json!([[0, 0], [0, 0]]),
        json!(["origin", "origin"]),
        json!({}),
        json!({"x": 1}),
        json!([{"point": [0, 0]}]),
        json!([{"body": "Body1", "index": 999999}]),
    ]
}

/// Every command, every parameter name it documents, every hostile value: no panic escapes and
/// no command panics internally.
#[test]
fn hostile_params_never_panic() {
    let keys = [
        "plane",
        "offset",
        "name",
        "sketch",
        "points",
        "closed",
        "construction",
        "ids",
        "infer",
        "p0",
        "p1",
        "p2",
        "center",
        "corner",
        "radius",
        "diameter",
        "start",
        "end",
        "through",
        "sides",
        "angle",
        "width",
        "point",
        "id",
        "entities",
        "type",
        "value",
        "line",
        "mode",
        "a",
        "b",
        "entity",
        "fixed",
        "curves",
        "to",
        "distance",
        "profiles",
        "direction",
        "distance2",
        "start_offset",
        "operation",
        "targets",
        "body_name",
        "axis",
        "length",
        "height",
        "base",
        "major",
        "minor",
        "edges",
        "body",
        "target",
        "tools",
        "keep_tools",
        "bodies",
        "translate",
        "expression",
        "unit",
        "comment",
        "delete",
        "features",
        "feature",
        "position",
        "suppressed",
        "set",
        "items",
        "add",
        "path",
        "format",
        "ascii",
        "measure",
    ];
    let mut internal = Vec::new();
    for spec in command_specs() {
        if matches!(spec.id, "doc.open" | "SaveDocumentCommand" | "SaveDocumentAsCommand" | "ExportCommand" | "FusionSaveAsSTLCommand") {
            continue; // file system side effects are covered by their own tests
        }
        for v in hostile_values() {
            for k in keys {
                let mut s = Session::default();
                // A model with a sketch, a body and an active sketch, so most commands get far.
                let _ = s.execute("PrimitiveBox", &json!({"length": 10, "width": 10, "height": 10}));
                let _ = s.execute("SketchCreate", &json!({"plane": "XY"}));
                let _ = s.execute("ShapeRectangleTwoPoint", &json!({"p0": [0, 0], "p1": [5, 5]}));
                let p = json!({ k: v.clone() });
                if let Err(EngineError::Internal(id, msg)) = s.execute(spec.id, &p) {
                    internal.push(format!("{id} {p}: {msg}"));
                }
                // Whole-value hostility too.
                if let Err(EngineError::Internal(id, msg)) = s.execute(spec.id, &v) {
                    internal.push(format!("{id} {v}: {msg}"));
                }
            }
        }
    }
    assert!(internal.is_empty(), "commands panicked:\n{}", internal.join("\n"));
}

#[test]
fn registry_is_consistent() {
    let specs = command_specs();
    let mut ids: Vec<&str> = specs.iter().map(|c| c.id).collect();
    ids.sort();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "duplicate command ids");
    assert!(find_command("extrude").is_some(), "case-insensitive lookup");
    let s = Session::default();
    assert!(!find_command("DrawPolyline").unwrap().info(&s).enabled);
}

#[test]
fn sample_design_builds() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["body_count"], 1, "{m}");
    let plate = 80.0 * 50.0 * 8.0 - 4.0 * (36.0 - PI * 9.0) * 8.0 - 2.0 * PI * 16.0 * 8.0;
    let boss = PI * 144.0 * 14.0;
    let bore = PI * 36.0 * 22.0;
    let v = m["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(v, plate + boss - bore) < 1e-3, "{v} vs {}", plate + boss - bore);
}
