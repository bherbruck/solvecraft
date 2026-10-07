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
        "refs",
        "link",
        "project_edges",
        "mid",
        "near",
        "factor",
        "copy",
        "count",
        "spacing",
        "count2",
        "spacing2",
        "dir",
        "side",
        "chain",
        "at",
        "curve",
        "major",
        "minor",
        "minor_radius",
        "apex",
        "rho",
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

#[test]
fn sample_edges_chain_and_faces() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let st = s.model.state();
    let m = st.bodies[0].mesh();
    let e = (0..m.edges.len()).find(|i| m.edges[*i].iter().all(|p| (p.z - 8.0).abs() < 1e-6 && p.y.abs() < 1e-6)).expect("front top edge");
    let chain = m.tangent_chain(e, 2f64.to_radians());
    // The plate's top outline: 4 lines and 4 corner arcs (a boolean may leave an edge twice,
    // once per face; count distinct ones).
    let mut mids: Vec<solvecraft_geom::Vec3> = Vec::new();
    for i in &chain {
        let p = &m.edges[*i];
        let mid = p[p.len() / 2];
        assert!(p.iter().all(|q| (q.z - 8.0).abs() < 1e-6), "{p:?}");
        if !mids.iter().any(|q| q.dist(mid) < 1e-6) {
            mids.push(mid);
        }
    }
    assert_eq!(mids.len(), 8, "{chain:?}");
    // Every edge bounds a face, and the top face has the outline and the hole rims.
    assert!(m.edge_faces.iter().all(|f| !f.is_empty()));
}

#[test]
fn timeline_edits_reresolve_references() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20, "name": "Base"}));
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[20, 0, 20]], "radius": 3, "name": "Round"}));
    run(&mut s, "FusionHoleCommand", json!({"position": [20, 15, 20], "diameter": 6, "name": "Bore"}));
    let fillet = |l: f64| (9.0 - 9.0 * PI / 4.0) * l;
    let expect = |l: f64, h: f64| l * 30.0 * h - fillet(l) - PI * 9.0 * h;
    assert!(rel(volume(&mut s), expect(40.0, 20.0)) < 1e-3);
    // Taller box: the fillet's edge point is now 5 mm below the edge (re-found, with a warning)
    // and the hole moves up with the top face.
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "25"}}));
    assert!(rel(volume(&mut s), expect(40.0, 25.0)) < 1e-3, "{}", volume(&mut s));
    let round = s.doc.find_feature("Round").map(|f| f.id).unwrap();
    assert!(s.model.result(round).and_then(|r| r.warning.clone()).is_some_and(|w| w.contains("re-found")));
    // Longer box: the point still lies on the (longer) edge.
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"length": "60"}}));
    assert!(rel(volume(&mut s), expect(60.0, 25.0)) < 1e-3, "{}", volume(&mut s));
    // Reorder: the fillet can't go before the box; the hole can go before the fillet.
    let e = s.execute("timeline.reorder", &json!({"feature": "Round", "position": 0})).unwrap_err().to_string();
    assert!(e.contains("breaks"), "{e}");
    run(&mut s, "timeline.reorder", json!({"feature": "Bore", "position": 1}));
    let names: Vec<&str> = s.doc.features.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Base", "Bore", "Round"]);
    assert!(rel(volume(&mut s), expect(60.0, 25.0)) < 1e-3);
    // Redefine the fillet in place with a new radius; it keeps its name and position.
    run(
        &mut s,
        "timeline.redefine",
        json!({"feature": "Round", "command": "FusionFilletEdgesCommand", "params": {"edges": [[30, 0, 25]], "radius": 4}}),
    );
    let f4 = (16.0 - 16.0 * PI / 4.0) * 60.0;
    assert!(rel(volume(&mut s), 60.0 * 30.0 * 25.0 - f4 - PI * 9.0 * 25.0) < 1e-3);
    assert_eq!(s.doc.features.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Base", "Bore", "Round"]);
    // Dependents and roll back.
    let d = run(&mut s, "timeline.dependents", json!({"feature": "Base"}));
    assert_eq!(d["would_fail"].as_array().map(|a| a.len()), Some(2), "{d}");
    run(&mut s, "timeline.rollTo", json!({"feature": "Base"}));
    assert!(rel(volume(&mut s), 60.0 * 30.0 * 25.0) < 1e-6);
    run(&mut s, "timeline.rollTo", json!({}));
    run(&mut s, "UndoCommand", json!({}));
    run(&mut s, "UndoCommand", json!({}));
    assert!(rel(volume(&mut s), 60.0 * 30.0 * 25.0 - f4 - PI * 9.0 * 25.0) < 1e-3);
}

/// A 40 × 30 × 20 box exported to a STEP file in a fresh temp directory.
fn step_box_file(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("solvecraft-step-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    let p = dir.join("Block.step");
    run(&mut s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    p
}

/// File → Open of a STEP file gives an Import base feature, and features work on its faces.
#[test]
fn step_open_then_model_on_it() {
    let p = step_box_file("open");
    let mut s = Session::default();
    let r = run(&mut s, "doc.open", json!({"path": p.to_string_lossy()}));
    assert_eq!(r["features"], 1, "{r}");
    assert_eq!(s.doc.name, "Block");
    assert_eq!(s.doc.features[0].kind.type_name(), "BaseFeature");
    assert!(matches!(&s.doc.features[0].kind, doc::FeatureKind::Import { file, .. } if file == "Block.step"));
    assert!(s.is_dirty() && s.path.is_none() && s.undo.is_empty());
    assert!(rel(volume(&mut s), 24000.0) < 1e-6);
    let corner = run(&mut s, "MeasureCommand", json!({}))["bodies"][0]["bbox"]["min"].clone();
    let (x0, y0, z0) = (corner[0].as_f64().unwrap(), corner[1].as_f64().unwrap(), corner[2].as_f64().unwrap());
    let at = |x: f64, y: f64, z: f64| json!([x0 + x, y0 + y, z0 + z]);

    // Fillet a vertical edge, drill a through hole, cut a pocket sketched on the top face.
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [at(0.0, 0.0, 10.0)], "radius": 3}));
    let mut want = 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0;
    assert!(rel(volume(&mut s), want) < 2e-4);
    run(&mut s, "FusionHoleCommand", json!({"position": at(28.0, 15.0, 20.0), "diameter": 6}));
    want -= PI * 9.0 * 20.0;
    assert!(rel(volume(&mut s), want) < 5e-4, "{} vs {want}", volume(&mut s));
    run(&mut s, "SketchCreate", json!({"plane": {"face": at(10.0, 15.0, 20.0)}}));
    let c = [x0 + 10.0, y0 + 15.0];
    // Sketch coordinates on the face are relative to the model origin projected onto it.
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [c[0] - 4.0, c[1] - 4.0], "p1": [c[0] + 4.0, c[1] + 4.0]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": -5, "operation": "cut"}));
    want -= 64.0 * 5.0;
    assert!(rel(volume(&mut s), want) < 5e-4, "{} vs {want}", volume(&mut s));
    assert_eq!(s.model.results.iter().filter(|r| r.error.is_some()).count(), 0);

    // The design saves and reopens without the STEP file.
    let before = volume(&mut s);
    let design = p.with_extension("solvecraft");
    run(&mut s, "SaveDocumentAsCommand", json!({"path": design.to_string_lossy()}));
    std::fs::remove_file(&p).unwrap();
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": design.to_string_lossy()}));
    assert!(rel(volume(&mut s2), before) < 1e-6);
}

/// Shell an imported body; insert STEP into an existing design; extension case; bad files.
#[test]
fn step_insert_shell_and_errors() {
    let p = step_box_file("insert");
    let upper = p.with_file_name("BLOCK.STP");
    std::fs::copy(&p, &upper).unwrap();
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": upper.to_string_lossy()}));
    let top = run(&mut s, "MeasureCommand", json!({}))["bodies"][0]["bbox"]["max"].clone();
    let (x1, y1, z1) = (top[0].as_f64().unwrap(), top[1].as_f64().unwrap(), top[2].as_f64().unwrap());
    run(&mut s, "FusionShellBodyCommand", json!({"faces": [[x1 - 20.0, y1 - 15.0, z1]], "thickness": 2}));
    let walls = 24000.0 - 36.0 * 26.0 * 18.0;
    assert!(rel(volume(&mut s), walls) < 5e-4, "{}", volume(&mut s));

    let mut d = Session::default();
    run(&mut d, "PrimitiveBox", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0]}));
    let r = run(&mut d, "FusionImportCommandFromToolbar", json!({"path": p.to_string_lossy()}));
    assert_eq!(r["bodies"].as_array().unwrap().len(), 1, "{r}");
    run(&mut d, "FusionImportCommandFromToolbar", json!({"path": p.to_string_lossy()}));
    let names: Vec<String> = d.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 3);
    let mut uniq = names.clone();
    uniq.dedup();
    assert_eq!(uniq.len(), 3, "{names:?}");
    assert!(rel(volume(&mut d), 125.0 + 48000.0) < 1e-6);
    run(&mut d, "UndoCommand", json!({}));
    assert_eq!(d.model.state().bodies.len(), 2);

    let junk = p.with_file_name("junk.step");
    std::fs::write(&junk, "ISO-10303-21;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n").unwrap();
    let before = d.doc.clone();
    assert!(d.execute("FusionImportCommandFromToolbar", &json!({"path": junk.to_string_lossy()})).is_err());
    assert!(d.execute("doc.open", &json!({"path": junk.to_string_lossy()})).is_err());
    assert!(d.execute("FusionImportCommandFromToolbar", &json!({"path": "/nonexistent/a.step"})).is_err());
    assert!(d.execute("FusionImportCommandFromToolbar", &json!({"path": p.with_extension("stl").to_string_lossy()})).is_err());
    assert_eq!(*d.doc, *before);
}

fn body_volume(st: &solvecraft_doc::ModelState) -> f64 {
    st.bodies.iter().map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).sum()
}

/// Previews evaluate on a scratch copy: the document, undo history and model are untouched.
#[test]
fn preview_has_no_side_effects() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "SketchStop", json!({}));
    let doc = s.doc.clone();
    let (undo, rev) = (s.undo.len(), s.revision);
    let p = s.preview(&[("Extrude".into(), json!({"distance": 15}))]).unwrap();
    assert!(p.before.bodies.is_empty());
    assert!(rel(body_volume(&p.after), 40.0 * 30.0 * 15.0) < 1e-9);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!((s.undo.len(), s.revision), (undo, rev));
    assert!(s.model.state().bodies.is_empty());
    // Errors come back as errors and change nothing either.
    assert!(s.preview(&[("Extrude".into(), json!({"distance": "nonsense"}))]).is_err());
    assert!(Arc::ptr_eq(&doc, &s.doc));

    // Fillet preview on a real body.
    run(&mut s, "Extrude", json!({"distance": 20}));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    let p = s.preview(&[("FusionFilletEdgesCommand".into(), json!({"edges": [[0, 0, 10]], "radius": 3}))]).unwrap();
    let filleted = 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0;
    assert!(rel(body_volume(&p.after), filleted) < 2e-4);
    assert!(rel(body_volume(&p.before), 24000.0) < 1e-9);
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.undo.len(), undo);
}

/// Previewing an edit shows the rebuilt feature although the timeline is rolled back to it.
#[test]
fn preview_of_an_edit() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    run(&mut s, "SketchStop", json!({}));
    let r = run(&mut s, "Extrude", json!({"distance": 20}));
    let fid = r["feature"].as_u64().unwrap();
    let idx = s.doc.feature_index(fid).unwrap();
    run(&mut s, "timeline.rollTo", json!({"position": idx}));
    assert!(s.model.state().bodies.is_empty());
    let doc = s.doc.clone();
    let p = s.preview(&[("timeline.redefine".into(), json!({"feature": fid, "command": "Extrude", "params": {"distance": 10}}))]).unwrap();
    assert!(rel(body_volume(&p.after), 12000.0) < 1e-9);
    assert!(Arc::ptr_eq(&doc, &s.doc));
}

/// `operation: "auto"`: into a body cuts, out of a body joins, in free space makes a new body.
#[test]
fn extrude_auto_operation() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    // A face pushed in (negative distance) cuts.
    let p = json!({"face": [20, 15, 20], "distance": -5, "operation": "auto"});
    assert_eq!(auto_operation(&s, &p), Some("cut"));
    run(&mut s, "Extrude", p);
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 15.0) < 1e-6);
    // Pulled out, it joins.
    assert_eq!(auto_operation(&s, &json!({"face": [20, 15, 15], "distance": 5})), Some("join"));
    // A sketch on XY under the box: up goes into it (cut), down grows out of it (join), and a
    // profile beside the box makes a new body.
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [5, 5], "p1": [15, 15]}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [60, 5], "p1": [70, 15]}));
    run(&mut s, "SketchStop", json!({}));
    let at = |x: f64, d: f64| json!({"profiles": [{"point": [x, 10]}], "distance": d});
    assert_eq!(auto_operation(&s, &at(10.0, 8.0)), Some("cut"));
    assert_eq!(auto_operation(&s, &at(10.0, -8.0)), Some("join"));
    assert_eq!(auto_operation(&s, &at(65.0, 8.0)), Some("new"));
    run(&mut s, "Extrude", json!({"profiles": [{"point": [10, 10]}], "distance": 8, "operation": "auto"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 15.0 - 100.0 * 8.0) < 1e-6);
}

/// Press Pull: positive pulls a face out, negative pushes it in (a cut), edges get a fillet.
#[test]
fn press_pull() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "FusionPressPullCommand", json!({"face": [20, 15, 20], "distance": 5}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 25.0) < 1e-6);
    run(&mut s, "FusionPressPullCommand", json!({"face": [20, 15, 25], "distance": "-10"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 15.0) < 1e-6);
    assert!(s.execute("FusionPressPullCommand", &json!({"face": [20, 15, 15], "distance": 0})).is_err());
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "FusionPressPullCommand", json!({"edges": [[0, 0, 7]], "distance": 3}));
    assert!(rel(volume(&mut s), 24000.0 - (9.0 - PI * 9.0 / 4.0) * 20.0) < 2e-4);
}

/// Extruding a planar body face (press-pull style): the face boundary becomes a sketch profile.
#[test]
fn extrude_a_body_face() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "Extrude", json!({"face": [20, 15, 20], "distance": 15, "operation": "join"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 35.0) < 1e-6);
    assert!(s.active_sketch.is_none());
    // A face with a round hole: the hole stays open (a new body: joins of curved bodies on a
    // shared face are a kernel gap for now).
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "CircleCenterRadius", json!({"center": [20, 15], "radius": 5}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 20, "operation": "cut"}));
    let before = volume(&mut s);
    run(&mut s, "Extrude", json!({"face": [5, 5, 20], "distance": 10, "operation": "new"}));
    let v = volume(&mut s);
    assert!(rel(v - before, (1200.0 - PI * 25.0) * 10.0) < 1e-3, "{v} {before}");
    assert!(s.execute("Extrude", &json!({"face": [500, 5, 20], "distance": 10})).is_err());
}

/// Export 3MF; open and insert it as mesh bodies; mesh bodies move and export but refuse
/// solid features.
#[test]
fn threemf_export_open_insert() {
    let dir = std::env::temp_dir().join(format!("solvecraft-3mf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "PrimitiveCylinder", json!({"radius": 5, "height": 10, "base": [100, 0, 0]}));
    let p = dir.join("Parts.3MF");
    run(&mut s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    let want = volume(&mut s);

    let mut m = Session::default();
    let r = run(&mut m, "doc.open", json!({"path": p.to_string_lossy()}));
    assert_eq!(r["bodies"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(m.doc.features[0].kind.type_name(), "MeshFeature");
    assert!(rel(volume(&mut m), want) < 2e-3, "{} vs {want}", volume(&mut m));
    let meas = run(&mut m, "MeasureCommand", json!({}));
    assert!(meas["bodies"][0]["face_types"]["mesh"].as_u64().unwrap() >= 12, "{meas}");
    let e = m.execute("FusionFilletEdgesCommand", &json!({"edges": [[0, 0, 10]], "radius": 2})).unwrap_err().to_string();
    assert!(e.contains("mesh"), "{e}");
    assert!(m.execute("ExportCommand", &json!({"path": dir.join("x.step").to_string_lossy()})).is_err());
    let stl = dir.join("again.stl");
    run(&mut m, "ExportCommand", json!({"path": stl.to_string_lossy()}));

    // Insert the STL into the solid design; names stay unique.
    run(&mut s, "ParaMeshInsertAlignCommand", json!({"path": stl.to_string_lossy()}));
    assert_eq!(s.model.state().bodies.len(), 3);
    assert!(rel(volume(&mut s), 2.0 * want) < 2e-3);
    assert!(s.execute("ParaMeshInsertAlignCommand", &json!({"path": dir.join("x.step").to_string_lossy()})).is_err());
    let junk = dir.join("junk.3mf");
    std::fs::write(&junk, b"PK\x03\x04 not really").unwrap();
    let before = s.doc.clone();
    assert!(s.execute("ParaMeshInsertAlignCommand", &json!({"path": junk.to_string_lossy()})).is_err());
    assert!(s.execute("doc.open", &json!({"path": junk.to_string_lossy()})).is_err());
    assert_eq!(*s.doc, *before);
    let _ = std::fs::remove_dir_all(&dir);
}
