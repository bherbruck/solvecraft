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

mod component_frames;

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
        "text",
        "lines",
        "driven",
        "from",
        "datum",
        "tolerance",
        "face",
        "new_name",
        "favorite",
        "favorites",
        "filter",
        "kind",
        "prefix",
    ];
    let mut internal = Vec::new();
    for spec in command_specs() {
        if matches!(
            spec.id,
            "doc.open" | "SaveDocumentCommand" | "SaveDocumentAsCommand" | "ExportCommand" | "FusionSaveAsSTLCommand" | "sketch.export_dxf"
        ) {
            continue; // file system side effects are covered by their own tests
        }
        for v in hostile_values() {
            for k in keys {
                if k == "path" && spec.id.starts_with("parameters.") {
                    continue; // parameter files: covered by their own tests (no stray files)
                }
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
    // Taller box: the fillet's edge point is now 5 mm below the edge; the edge is found by its
    // name (no warning) and the hole moves up with the top face.
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "25"}}));
    assert!(rel(volume(&mut s), expect(40.0, 25.0)) < 1e-3, "{}", volume(&mut s));
    let round = s.doc.find_feature("Round").map(|f| f.id).unwrap();
    assert_eq!(s.model.result(round).and_then(|r| r.warning.clone()), None);
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

/// Measuring picked geometry: distance between faces, angle, edge length.
#[test]
fn measure_items() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    let st = s.model.state();
    let m = st.body("Body1").unwrap().mesh();
    // Find the faces by their centroid: top (z = 20), bottom (z = 0), front (y = 0).
    let face_at = |z: Option<f64>, y: Option<f64>| -> usize {
        (0..6)
            .find(|f| {
                let tris: Vec<_> = m.triangles.iter().zip(&m.tri_face).filter(|(_, tf)| **tf as usize == *f).filter_map(|(t, _)| m.tri(t)).collect();
                tris.iter().all(|t| t.iter().all(|p| z.is_none_or(|z| (p.z - z).abs() < 1e-9) && y.is_none_or(|y| (p.y - y).abs() < 1e-9)))
            })
            .unwrap()
    };
    let (ft, fb, ff) = (face_at(Some(20.0), None), face_at(Some(0.0), None), face_at(None, Some(0.0)));
    let face = |i: usize, p: [f64; 3]| json!({"type": "face", "body": "Body1", "index": i, "point": p});
    let r = run(&mut s, "MeasureCommand", json!({"items": [face(ft, [20.0, 15.0, 20.0]), face(fb, [20.0, 15.0, 0.0])]}));
    assert!((r["distance_mm"].as_f64().unwrap() - 20.0).abs() < 1e-9, "{r}");
    assert!((r["angle_deg"].as_f64().unwrap() - 180.0).abs() < 1e-6, "{r}");
    let r = run(&mut s, "MeasureCommand", json!({"items": [face(ft, [20.0, 15.0, 20.0]), face(ff, [20.0, 0.0, 10.0])]}));
    assert!(r["distance_mm"].as_f64().unwrap() < 1e-9);
    assert!((r["angle_deg"].as_f64().unwrap() - 90.0).abs() < 1e-6);
    let r = run(&mut s, "MeasureCommand", json!({"items": [face(ft, [20.0, 15.0, 20.0])]}));
    assert!((r["items"][0]["area_mm2"].as_f64().unwrap() - 1200.0).abs() < 1e-6, "{r}");
    let v = json!({"type": "vertex", "body": "Body1", "point": [0, 0, 0]});
    let w = json!({"type": "vertex", "body": "Body1", "point": [40, 30, 20]});
    let r = run(&mut s, "MeasureCommand", json!({"items": [v, w]}));
    assert!((r["distance_mm"].as_f64().unwrap() - (1600.0f64 + 900.0 + 400.0).sqrt()).abs() < 1e-9);
    assert!(s.execute("MeasureCommand", &json!({"items": []})).is_err());
    assert!(s.execute("MeasureCommand", &json!({"items": [{"type": "nonsense"}]})).is_err());
}

/// Section Analysis sets and clears the view cut without touching the design or history.
#[test]
fn section_analysis() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    let (doc, undo) = (s.doc.clone(), s.undo.len());
    run(&mut s, "FusionHalfSectionViewCommand", json!({"plane": "XZ", "offset": 15}));
    let (o, n) = s.section.unwrap();
    assert!((o - n * 15.0).len() < 1e-12 && (n.y.abs() - 1.0).abs() < 1e-12);
    run(&mut s, "FusionHalfSectionViewCommand", json!({"plane": {"origin": [0, 0, 5], "normal": [0, 0, 2]}, "flip": true}));
    let (o, n) = s.section.unwrap();
    assert!((o.z - 5.0).abs() < 1e-12 && (n.z + 1.0).abs() < 1e-12);
    assert!(s.execute("FusionHalfSectionViewCommand", &json!({"plane": {"normal": [0, 0, 0]}})).is_err());
    run(&mut s, "FusionHalfSectionViewCommand", json!({"clear": true}));
    assert!(s.section.is_none());
    assert!(Arc::ptr_eq(&doc, &s.doc));
    assert_eq!(s.undo.len(), undo);
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

#[test]
fn holes_at_sketch_points_threads_and_components() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 60, "width": 40, "height": 10, "name": "Plate"}));
    run(&mut s, "SketchCreate", json!({"plane": {"face": [30, 20, 10]}, "name": "HolePoints"}));
    let mut ids = Vec::new();
    for (k, (x, y)) in [(10.0, 10.0), (50.0, 10.0), (50.0, 30.0), (10.0, 30.0)].into_iter().enumerate() {
        let id = format!("h{k}");
        run(&mut s, "DrawPoint", json!({ "point": [x, y], "id": id }));
        ids.push(id);
    }
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "FusionHoleCommand", json!({"sketch": "HolePoints", "points": ids, "diameter": 5, "thread": "M6", "name": "Holes"}));
    let v = volume(&mut s);
    assert!(rel(v, 60.0 * 40.0 * 10.0 - 4.0 * PI * 6.25 * 10.0) < 1e-3, "{v}");
    let t = run(&mut s, "model.threads", json!({}));
    let threads = t["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 4, "{t}");
    assert_eq!(threads[0]["designation"], "M6");
    assert_eq!(threads[0]["internal"], true);
    // A cosmetic thread on a shaft; designation from its size.
    run(&mut s, "PrimitiveCylinder", json!({"base": [100, 0, 0], "radius": 4, "height": 20, "name": "Shaft", "body_name": "Shaft"}));
    let th = run(&mut s, "FusionThreadCommand", json!({"face": [104, 0, 10], "length": 12}));
    assert_eq!(th["thread"]["designation"], "M8", "{th}");
    assert!((th["thread"]["end"].as_f64().unwrap() - th["thread"]["start"].as_f64().unwrap() - 12.0).abs() < 1e-6, "{th}");
    assert!(s.execute("FusionThreadCommand", &json!({"face": [104, 0, 10], "designation": "M20"})).is_err());
    // Components: features made while one is active belong to it.
    let c = run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Bracket"}));
    let cid = c["component"].as_u64().unwrap();
    run(&mut s, "PrimitiveBox", json!({"corner": [0, 60, 0], "length": 10, "width": 10, "height": 10, "name": "Block"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let l = run(&mut s, "component.list", json!({}));
    let comps = l["components"].as_array().unwrap();
    let bracket = comps.iter().find(|x| x["id"] == cid).unwrap();
    assert_eq!(bracket["features"], json!(["Block"]), "{l}");
    assert_eq!(bracket["bodies"].as_array().map(|a| a.len()), Some(1), "{l}");
    let r = run(&mut s, "FusionCreateComponentsFromBodiesCommand", json!({"bodies": ["Shaft"]}));
    assert_eq!(r["components"].as_array().map(|a| a.len()), Some(1));
    // Saved and loaded with the components.
    let text = s.doc.to_json();
    let back = solvecraft_doc::Document::from_json(&text).unwrap();
    assert_eq!(back.components.len(), 2);
    assert_eq!(back.body_components.len(), 1);
}

#[test]
fn pipe_scale_offset_bounding_and_materials() {
    let mut s = Session::default();
    // Pipe along an L path (line, arc, line) on XZ.
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [30, 0]], "ids": ["l1"]}));
    run(&mut s, "ArcCenterTwoPoint", json!({"center": [30, 10], "start": "l1.end", "end": [40, 10], "id": "a1"}));
    run(&mut s, "DrawPolyline", json!({"points": ["a1.end", [40, 40]], "ids": ["l2"]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "PrimitivePipe", json!({"path_sketch": "Path", "path": ["l1", "a1", "l2"], "diameter": 4, "wall": 1, "body_name": "Tube"}));
    let len = 30.0 + PI * 10.0 / 2.0 + 30.0;
    let v = volume(&mut s);
    assert!(rel(v, PI * (4.0 - 1.0) * len) < 2e-3, "{v} vs {}", PI * 3.0 * len);
    // Bounding solid around it; then scale a box and push one of its faces.
    run(&mut s, "StockModelCommand", json!({"bodies": ["Tube"], "margin": 1, "body_name": "Stock"}));
    let m = run(&mut s, "MeasureCommand", json!({"bodies": ["Stock"]}));
    let bb = &m["bodies"][0]["bbox"];
    assert!((bb["min"][1].as_f64().unwrap() + 3.0).abs() < 1e-6 && (bb["max"][1].as_f64().unwrap() - 41.0).abs() < 1e-6, "{bb}");
    run(&mut s, "PrimitiveBox", json!({"corner": [100, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut s, "ModifyScale", json!({"bodies": ["Cube"], "factor": 2, "origin": [100, 0, 0]}));
    let vc = run(&mut s, "MeasureCommand", json!({"bodies": ["Cube"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(vc, 8000.0) < 1e-9, "{vc}");
    run(&mut s, "FusionOffsetFacesCommand", json!({"faces": [[110, 10, 20]], "distance": 5}));
    let vc = run(&mut s, "MeasureCommand", json!({"bodies": ["Cube"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(vc, 20.0 * 20.0 * 25.0) < 1e-9, "{vc}");
    // Materials and mass.
    run(&mut s, "PhysicalMaterialCommand", json!({"bodies": ["Cube"], "material": "aluminum"}));
    let m = run(&mut s, "MeasureCommand", json!({"bodies": ["Cube"]}));
    assert!((m["bodies"][0]["mass_g"].as_f64().unwrap() - 10_000.0 * 2.7 / 1000.0).abs() < 1e-6, "{m}");
    assert_eq!(m["bodies"][0]["material"], "Aluminum");
    assert!(s.execute("PhysicalMaterialCommand", &json!({"bodies": ["Cube"], "material": "unobtainium"})).is_err());
}

#[test]
fn components_occurrences_and_world_placement() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    // A component, active: its box is authored in its frame.
    let c = run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Arm"}));
    let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "PrimitiveBox", json!({"corner": [20, 0, 0], "length": 10, "width": 4, "height": 4, "body_name": "ArmBody"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    let bbox = |s: &mut Session, b: &str| run(s, "MeasureCommand", json!({ "bodies": [b] }))["bodies"][0]["bbox"].clone();
    assert_eq!(bbox(&mut s, "ArmBody")["min"][0], 20.0);
    // Move the occurrence: the world placement follows, the authoring model doesn't change.
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 50, 0]}));
    assert!((bbox(&mut s, "ArmBody")["min"][1].as_f64().unwrap() - 50.0).abs() < 1e-9);
    assert_eq!(s.model.state().body("ArmBody").unwrap().mesh().bounds().min.y, 0.0);
    // A fillet picked in the world, while the component is active, lands on its edge.
    run(&mut s, "component.activate", json!({"component": "Arm"}));
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[25, 54, 4]], "radius": 1}));
    let v = run(&mut s, "MeasureCommand", json!({"bodies": ["ArmBody"]}))["total"]["volume_mm3"].as_f64().unwrap();
    assert!(rel(v, 160.0 - (1.0 - PI / 4.0) * 10.0) < 1e-4, "{v}");
    run(&mut s, "component.activate", json!({"component": 0}));
    // Instances share the contents; a copy is independent.
    run(&mut s, "occurrence.copy", json!({"component": cid, "translate": [0, 20, 0]}));
    let w = s.world_state();
    assert_eq!(w.bodies.len(), 3, "{:?}", w.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let pn = run(&mut s, "component.paste_new", json!({"component": cid, "translate": [0, -30, 0]}));
    assert_eq!(pn["features"].as_array().map(|a| a.len()), Some(2));
    assert_eq!(s.world_state().bodies.len(), 4);
    // Ground stops moves; pending moves show until captured or reverted.
    run(&mut s, "occurrence.ground", json!({"occurrence": occ}));
    assert!(s.execute("occurrence.move", &json!({"occurrence": occ, "translate": [1, 0, 0]})).is_err());
    run(&mut s, "occurrence.ground", json!({"occurrence": occ, "grounded": false}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 0, 5], "capture": false}));
    assert!((bbox(&mut s, "ArmBody")["min"][2].as_f64().unwrap() - 5.0).abs() < 1e-9);
    run(&mut s, "AsBuiltPositionsCmd", json!({}));
    assert!(bbox(&mut s, "ArmBody")["min"][2].as_f64().unwrap().abs() < 1e-9);
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 0, 5], "capture": false}));
    run(&mut s, "SnapshotCmd", json!({}));
    assert!(s.pending_moves.is_empty());
    assert!((bbox(&mut s, "ArmBody")["min"][2].as_f64().unwrap() - 5.0).abs() < 1e-9);
    // Bodies and sketches move between components.
    run(&mut s, "component.move_bodies", json!({"bodies": ["Base"], "component": "Arm"}));
    let l = run(&mut s, "component.list", json!({}));
    let arm = l["components"].as_array().unwrap().iter().find(|x| x["id"] == cid).unwrap().clone();
    assert!(arm["bodies"].as_array().unwrap().iter().any(|b| b == "Base"), "{arm}");
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "S1"}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "component.move_sketches", json!({"sketches": ["S1"], "component": cid}));
    assert_eq!(s.doc.find_feature("S1").unwrap().component, cid);
    // Undo/redo and the timeline work with features in components.
    run(&mut s, "UndoCommand", json!({}));
    assert_eq!(s.doc.find_feature("S1").unwrap().component, 0);
    run(&mut s, "RedoCommand", json!({}));
    run(&mut s, "timeline.rollTo", json!({"position": 1}));
    // Only Base is left; it now lives in Arm, which is placed twice (original and instance).
    assert_eq!(s.world_state().bodies.len(), 2);
    run(&mut s, "timeline.rollTo", json!({}));
    // Saved and loaded; older designs get occurrences.
    let back = solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.occurrences.len(), s.doc.occurrences.len());
    let mut old: serde_json::Value = serde_json::from_str(&s.doc.to_json()).unwrap();
    old.as_object_mut().unwrap().remove("occurrences");
    let migrated = solvecraft_doc::Document::from_json(&old.to_string()).unwrap();
    assert_eq!(migrated.occurrences.len(), migrated.components.len());
}

#[test]
fn feature_inputs_are_parameters() {
    let mut s = Session::default();
    run(&mut s, "ChangeParameterCommand", json!({"name": "w", "expression": "15 mm", "comment": "width"}));
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": "w", "height": "1 in", "name": "B"}));
    let l = run(&mut s, "parameters.list", json!({}));
    let rows = l["parameters"].as_array().unwrap();
    let feat: Vec<&Value> = rows.iter().filter(|r| r["source"] == "feature").collect();
    assert_eq!(feat.len(), 3, "{l}");
    let height = feat.iter().find(|r| r["input"] == "Height").unwrap();
    assert!((height["value"].as_f64().unwrap() - 25.4).abs() < 1e-9);
    let length_name = feat.iter().find(|r| r["input"] == "Length").unwrap()["name"].as_str().unwrap().to_string();
    // A feature parameter changes the feature; other expressions can use it.
    run(&mut s, "ChangeParameterCommand", json!({"name": length_name, "expression": "2 * w"}));
    assert!(rel(volume(&mut s), 30.0 * 15.0 * 25.4) < 1e-9);
    run(&mut s, "ChangeParameterCommand", json!({"name": "t", "expression": format!("{length_name} / 3")}));
    let t = run(&mut s, "parameters.list", json!({}))["parameters"].as_array().unwrap().iter().find(|r| r["name"] == "t").unwrap()["value"]
        .as_f64()
        .unwrap();
    assert!((t - 10.0).abs() < 1e-9);
    // Cycles are refused.
    assert!(s.execute("ChangeParameterCommand", &json!({"name": "w", "expression": "t"})).is_err());
    // Favourites, export and import.
    run(&mut s, "parameters.favorite", json!({"name": "w"}));
    assert_eq!(run(&mut s, "parameters.list", json!({"favorites": true}))["parameters"].as_array().unwrap().len(), 1);
    let csv = run(&mut s, "parameters.export", json!({}))["text"].as_str().unwrap().to_string();
    assert!(csv.contains("w,mm,15 mm,15,width,true"), "{csv}");
    run(&mut s, "parameters.import", json!({"text": "name,unit,expression,comment\nw,mm,20 mm,wider\nnew_p,mm,\"w + 1\","}));
    assert!(rel(volume(&mut s), 40.0 * 20.0 * 25.4) < 1e-9);
    run(&mut s, "parameters.import", json!({"text": "[{\"name\": \"w\", \"expression\": \"12\"}]"}));
    assert!(rel(volume(&mut s), 24.0 * 12.0 * 25.4) < 1e-9);
    assert!(s.execute("parameters.import", &json!({"text": "name,expression\nw,1/"})).is_err());
    // Saved and reloaded, the names stay.
    let back = solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.features[0].param_names, s.doc.features[0].param_names);
}

#[test]
fn rolling_back_and_forward_leaves_no_projection_warnings() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let warnings = |s: &Session| -> Vec<String> {
        s.doc.features.iter().filter_map(|f| s.model.result(f.id).and_then(|r| r.warning.clone()).map(|w| format!("{}: {w}", f.name))).collect()
    };
    assert!(warnings(&s).is_empty(), "{:?}", warnings(&s));
    for pos in 0..s.doc.features.len() {
        run(&mut s, "timeline.rollTo", json!({ "position": pos }));
        run(&mut s, "timeline.rollTo", json!({}));
        assert!(warnings(&s).is_empty(), "after rolling to {pos}: {:?}", warnings(&s));
    }
}

/// Export STEP, validate the file, read it back: no warnings, same volume, area and faces.
fn step_round_trip(s: &mut Session, tag: &str) -> solvecraft_kernel::StepImport {
    let p = std::env::temp_dir().join(format!("solvecraft-steprt-{tag}-{}.step", std::process::id()));
    run(s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    let text = std::fs::read_to_string(&p).unwrap();
    let _ = std::fs::remove_file(&p);
    solvecraft_kernel::step_validate(&text).unwrap();
    assert_eq!(solvecraft_kernel::step_orientation_errors(&text).unwrap(), Vec::<String>::new());
    assert!(text.contains("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF"));
    let imp = solvecraft_kernel::step_import(&text).unwrap();
    assert!(imp.warnings.is_empty(), "{:?}", imp.warnings);
    imp
}

#[test]
fn sample_plate_step_round_trip() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    let imp = step_round_trip(&mut s, "sample");
    let st = s.world_state();
    assert_eq!(imp.bodies.len(), st.bodies.len());
    for (a, b) in st.bodies.iter().zip(&imp.bodies) {
        assert_eq!(a.name, b.name);
        let (ma, mb) = (solvecraft_kernel::measure(&a.body).unwrap(), solvecraft_kernel::measure(&b.body).unwrap());
        assert!(rel(mb.volume, ma.volume) < 1e-4, "{} vs {}", mb.volume, ma.volume);
        assert!(rel(mb.area, ma.area) < 1e-4, "{} vs {}", mb.area, ma.area);
        assert!(b.file_faces <= a.body.face_count() && b.file_faces >= ma.merged.faces.min(a.body.face_count()));
        assert_eq!(mb.merged.faces, ma.merged.faces);
    }
}

/// Bodies export in the colour they are shown in: a material's look, an appearance over it,
/// none for the default; and colours read from a STEP file survive the next export.
#[test]
fn material_and_appearance_colours_export_to_step() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "body_name": "BrassBox"}));
    run(&mut s, "PrimitiveBox", json!({"corner": [20, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "RedAbs"}));
    run(&mut s, "PrimitiveBox", json!({"corner": [40, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Plain"}));
    run(&mut s, "PhysicalMaterialCommand", json!({"bodies": ["BrassBox"], "material": "Brass"}));
    run(&mut s, "PhysicalMaterialCommand", json!({"bodies": ["RedAbs"], "material": "ABS Plastic"}));
    run(&mut s, "AppearanceCommand", json!({"bodies": ["RedAbs"], "color": "#d01c1c"}));
    assert!(s.execute("AppearanceCommand", &json!({"bodies": ["RedAbs"], "color": "#12"})).is_err());
    let rgb = |c: [u8; 3]| c.map(|x| x as f32 / 255.0);
    let want =
        [("BrassBox", Some(rgb(solvecraft_doc::material_color("Brass").unwrap()))), ("RedAbs", Some(rgb([0xd0, 0x1c, 0x1c]))), ("Plain", None)];
    let same = |a: Option<[f32; 3]>, b: Option<[f32; 3]>| match (a, b) {
        (Some(a), Some(b)) => a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3),
        (None, None) => true,
        _ => false,
    };
    let check = |imp: &solvecraft_kernel::StepImport| {
        for (name, c) in want {
            let b = imp.bodies.iter().find(|b| b.name == name).unwrap_or_else(|| panic!("no body {name}"));
            assert!(same(b.body.color(), c), "{name}: {:?} vs {c:?}", b.body.color());
        }
    };
    check(&step_round_trip(&mut s, "colours"));
    // Open the export and export again: the file's colours are kept.
    let p = std::env::temp_dir().join(format!("solvecraft-colours-{}.step", std::process::id()));
    run(&mut s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
    let _ = std::fs::remove_file(&p);
    check(&step_round_trip(&mut s2, "colours2"));
}

/// Face appearances and see-through bodies export: STEP styles on the faces with a
/// transparency, read back as face colours.
#[test]
fn face_colours_and_opacity_export() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut s, "AppearanceCommand", json!({"bodies": ["Cube"], "color": "#2040c0", "opacity": 0.5}));
    run(&mut s, "AppearanceCommand", json!({"faces": [[5, 5, 10]], "color": "#ff0000"}));
    let imp = step_round_trip(&mut s, "facecol");
    let b = &imp.bodies[0];
    let paint = b.body.paint().expect("paint read back");
    assert!((paint.opacity - 0.5).abs() < 1e-3, "{paint:?}");
    let faces = b.body.faces(0.01).unwrap();
    let top = faces.iter().position(|f| (f.centroid.z - 10.0).abs() < 1e-6).unwrap();
    assert_eq!(paint.faces.len(), 1, "{paint:?}");
    assert_eq!(paint.faces[0].face, top);
    assert!((paint.faces[0].color[0] - 1.0).abs() < 1e-3 && paint.faces[0].color[1].abs() < 1e-3, "{paint:?}");
    // Opened again, the face keeps its colour (shown and exported).
    let p = std::env::temp_dir().join(format!("solvecraft-facecol-{}.step", std::process::id()));
    run(&mut s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
    let _ = std::fs::remove_file(&p);
    let st = s2.world_state();
    let looks = s2.doc.face_colors(&st.bodies[0]);
    assert_eq!(looks.len(), 1);
    assert_eq!(looks[0].1.color, [255, 0, 0]);
}

/// IGES: a design exports as IGES and the file opens again as an Import feature with the same
/// bodies, names and colours.
#[test]
fn iges_export_and_open() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 5, "body_name": "Plate"}));
    run(&mut s, "PrimitiveCylinder", json!({"radius": 4, "height": 20, "base": [10, 10, 5], "body_name": "Pin"}));
    run(&mut s, "AppearanceCommand", json!({"bodies": ["Pin"], "color": "#ff0000"}));
    let want: Vec<f64> = s.world_state().bodies.iter().map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).collect();
    let p = std::env::temp_dir().join(format!("solvecraft-iges-{}.igs", std::process::id()));
    run(&mut s, "ExportCommand", json!({"path": p.to_string_lossy()}));
    let mut s2 = Session::default();
    run(&mut s2, "doc.open", json!({"path": p.to_string_lossy()}));
    let _ = std::fs::remove_file(&p);
    let st = s2.world_state();
    let names: Vec<&str> = st.bodies.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["Plate", "Pin"]);
    for (b, v) in st.bodies.iter().zip(&want) {
        assert!(rel(solvecraft_kernel::measure(&b.body).unwrap().volume, *v) < 1e-4);
    }
    assert_eq!(st.bodies[1].body.color(), Some([1.0, 0.0, 0.0]));
}

/// Split Body by a face of another body (its surface extended): a box cut by a cylinder's
/// side into the core and the rest; volumes add up exactly.
#[test]
fn split_body_by_a_face() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 20, "height": 30, "body_name": "Block"}));
    run(&mut s, "PrimitiveCylinder", json!({"radius": 4, "height": 40, "base": [5, 10, -5], "body_name": "Tool"}));
    run(&mut s, "FusionSplitBodyCommand", json!({"body": "Block", "tool": {"body": "Tool", "point": [9, 10, 10]}}));
    let st = s.world_state();
    let mut v: Vec<f64> =
        st.bodies.iter().filter(|b| b.name.starts_with("Block")).map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).collect();
    v.sort_by(f64::total_cmp);
    let core = PI * 16.0 * 30.0;
    assert!(v.len() == 2 && rel(v[0], core) < 1e-3 && rel(v[1], 6000.0 - core) < 1e-3, "{v:?}");
    assert!(s.execute("FusionSplitBodyCommand", &json!({"body": "Block", "tool": {"body": "Tool", "point": [100, 0, 0]}})).is_err());
}

/// Components export as a STEP assembly: products, occurrences, placements.
#[test]
fn components_export_as_step_assembly() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 5, "name": "Plate"}));
    run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Pin"}));
    run(&mut s, "PrimitiveCylinder", json!({"radius": 2, "height": 10, "base": [5, 5, 5]}));
    run(&mut s, "occurrence.copy", json!({"component": "Pin", "translate": [20, 0, 0]}));
    let want: f64 = s.world_state().bodies.iter().map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).sum();
    let imp = step_round_trip(&mut s, "asm");
    assert_eq!(imp.bodies.len(), 3, "{:?}", imp.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let got: f64 = imp.bodies.iter().map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).sum();
    assert!(rel(got, want) < 1e-4, "{got} vs {want}");
    let root = &imp.tree[0];
    assert_eq!(root.children.len(), 2);
    assert!(root.children.iter().all(|c| c.name == "Pin"));
    // The second pin sits 20 mm along X from the first.
    let xs: Vec<f64> = imp
        .bodies
        .iter()
        .filter(|b| b.path.last().is_some_and(|p| p == "Pin"))
        .map(|b| solvecraft_kernel::measure(&b.body).unwrap().centroid.x)
        .collect();
    assert_eq!(xs.len(), 2);
    assert!(((xs[0] - xs[1]).abs() - 20.0).abs() < 1e-6, "{xs:?}");
}

/// A face extrude follows its face when earlier features change.
#[test]
fn face_extrude_follows_edits() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20, "name": "Base"}));
    run(&mut s, "Extrude", json!({"face": [20, 15, 20], "distance": 10, "operation": "join", "name": "Pad"}));
    assert!(rel(volume(&mut s), 40.0 * 30.0 * 30.0) < 1e-6);
    run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "25", "length": "50"}}));
    assert!(rel(volume(&mut s), 50.0 * 30.0 * 35.0) < 1e-6, "{}", volume(&mut s));
    // A face revolved about a line: Pappus (area × centroid path).
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    let before = volume(&mut s);
    run(&mut s, "Revolve", json!({"face": [40, 15, 10], "axis": {"origin": [40, 0, 30], "dir": [0, 1, 0]}, "angle": "90 deg", "operation": "new"}));
    let added = volume(&mut s) - before;
    let want = 30.0 * 20.0 * 20.0 * PI / 2.0;
    assert!(rel(added, want) < 1e-4, "{added} vs {want}");
}

/// Appearances on faces, bodies and components: face > body > component > material.
#[test]
fn appearances_by_face_body_and_component() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "body_name": "Root"}));
    run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Case"}));
    run(&mut s, "PrimitiveBox", json!({"corner": [20, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Shell"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let lib = run(&mut s, "appearance.library", json!({}));
    assert!(lib["appearances"].as_array().is_some_and(|a| a.len() >= 15 && a.iter().any(|x| x["name"] == "Glass - Clear")));
    let look = |s: &mut Session, b: &str| run(s, "appearance.list", json!({"body": b}))["body"].clone();
    // Material, then component, then body.
    run(&mut s, "PhysicalMaterialCommand", json!({"bodies": ["Shell"], "material": "Brass"}));
    assert_eq!(look(&mut s, "Shell")["appearance"]["name"], "Brass");
    run(&mut s, "AppearanceCommand", json!({"components": ["Case"], "appearance": "ABS - Blue"}));
    assert_eq!(look(&mut s, "Shell")["appearance"]["name"], "ABS - Blue");
    assert!(look(&mut s, "Root")["appearance"].is_null(), "root bodies have no component look");
    run(&mut s, "AppearanceCommand", json!({"bodies": ["Shell"], "color": [255, 0, 0], "opacity": 0.5}));
    let l = look(&mut s, "Shell");
    assert_eq!((l["appearance"]["color"].as_str(), l["appearance"]["opacity"].as_f64()), (Some("#ff0000"), Some(0.5)), "{l}");
    // A face: the top of Shell.
    run(&mut s, "AppearanceCommand", json!({"faces": [[25, 5, 10]], "appearance": "Glass - Clear"}));
    let l = look(&mut s, "Shell");
    let faces = l["faces"].as_array().cloned().unwrap_or_default();
    assert_eq!(faces.len(), 1, "{l}");
    assert_eq!(faces[0]["appearance"]["name"], "Glass - Clear");
    // Clearing goes back down the chain.
    run(&mut s, "AppearanceCommand", json!({"faces": [[26, 6, 10]]}));
    assert_eq!(look(&mut s, "Shell")["faces"].as_array().map(Vec::len), Some(0), "cleared by another point on the same face");
    run(&mut s, "AppearanceCommand", json!({"bodies": ["Shell"], "color": null}));
    assert_eq!(look(&mut s, "Shell")["appearance"]["name"], "ABS - Blue");
    run(&mut s, "AppearanceCommand", json!({"components": ["Case"]}));
    assert_eq!(look(&mut s, "Shell")["appearance"]["name"], "Brass");
    // Saved and opened again.
    run(&mut s, "AppearanceCommand", json!({"faces": [[5, 5, 10]], "color": "#00ff00"}));
    let back = solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.appearances, s.doc.appearances);
    assert!(s.execute("AppearanceCommand", &json!({"bodies": ["Shell"], "appearance": "Unobtainium"})).is_err());
    assert!(s.execute("AppearanceCommand", &json!({"color": "#ffffff"})).is_err(), "no target");
    assert!(s.execute("AppearanceCommand", &json!({"faces": [[500, 500, 500]], "color": "#ffffff"})).is_err(), "no face there");
}

/// The evaluation cache gives exactly what evaluating from scratch gives, through edits,
/// suppression, roll-back, undo and redo.
#[test]
fn cached_evaluation_matches_from_scratch() {
    let mut s = Session::default();
    s.run_script(&crate::sample::script()).unwrap();
    run(
        &mut s,
        "PatternRectangular",
        json!({"bodies": ["Plate"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 100, "dir2": [0, 1, 0], "count2": 2, "spacing2": 70}),
    );
    run(&mut s, "PrimitiveBox", json!({"corner": [-50, 0, 0], "length": 10, "width": 10, "height": 10, "body_name": "Side"}));
    let same = |s: &Session, what: &str| {
        let mut fresh = solvecraft_doc::Model::new();
        fresh.no_cache = true;
        fresh.evaluate(&s.doc);
        let errs = |m: &solvecraft_doc::Model| m.results.iter().map(|r| (r.id, r.error.clone(), r.skipped)).collect::<Vec<_>>();
        assert_eq!(errs(&s.model), errs(&fresh), "{what}: feature results");
        let shape = |st: &solvecraft_doc::ModelState| {
            st.bodies
                .iter()
                .map(|b| {
                    let m = solvecraft_kernel::measure(&b.body).unwrap();
                    // Volume and area to 1e-9: two evaluations from scratch differ in the last
                    // bits too (the kernel's tessellation is not bit-for-bit repeatable).
                    (b.name.clone(), b.feature, (m.volume * 1e6).round(), (m.area * 1e6).round(), m.faces, m.edges, m.vertices, m.merged.faces)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(shape(&s.model.state()), shape(&fresh.state()), "{what}: bodies");
        assert_eq!(s.model.state().sketches.len(), fresh.state().sketches.len(), "{what}: sketches");
    };
    // Two evaluations from scratch agree to the same precision.
    let mut a = solvecraft_doc::Model::new();
    a.no_cache = true;
    a.evaluate(&s.doc);
    let mut b = solvecraft_doc::Model::new();
    b.no_cache = true;
    b.evaluate(&s.doc);
    let vols = |m: &solvecraft_doc::Model| {
        m.state().bodies.iter().map(|x| (solvecraft_kernel::measure(&x.body).unwrap().volume * 1e6).round()).collect::<Vec<_>>()
    };
    assert_eq!(vols(&a), vols(&b));
    same(&s, "built");
    let hits = |s: &Session| s.model.cache.lock().map(|c| c.hits).unwrap_or(0);
    for (step, (id, p)) in [
        ("ChangeParameterCommand", json!({"name": "width", "expression": "90 mm"})),
        ("ChangeParameterCommand", json!({"name": "width", "expression": "80 mm"})),
        ("ChangeParameterCommand", json!({"name": "thickness", "expression": "10 mm"})),
        ("UndoCommand", json!({})),
        ("RedoCommand", json!({})),
        ("timeline.suppress", json!({"feature": "Holes", "suppressed": true})),
        ("timeline.suppress", json!({"feature": "Holes", "suppressed": false})),
        ("timeline.rollTo", json!({"position": 6})),
        ("timeline.rollTo", json!({})),
        ("UndoCommand", json!({})),
        ("UndoCommand", json!({})),
    ]
    .into_iter()
    .enumerate()
    {
        let before = hits(&s);
        run(&mut s, id, p.clone());
        same(&s, &format!("step {step}: {id} {p}"));
        if step == 1 {
            assert!(hits(&s) > before, "setting the width back reuses the earlier results");
        }
    }
}

/// A fillet keeps its edge when an earlier sketch dimension resizes the body (QA s06).
#[test]
fn fillet_follows_its_edge_after_an_upstream_resize() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "S"}));
    let r = run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    let bottom = r["curves"][0].as_str().unwrap_or_default().to_string();
    let d = run(&mut s, "SketchDimension", json!({"entities": [bottom], "value": 40}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 10}));
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[40, 15, 10]], "radius": 2}));
    let v = |s: &mut Session| run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(0.0);
    let strip = (4.0 - std::f64::consts::PI) * 30.0;
    assert!((v(&mut s) - (12000.0 - strip)).abs() < 0.05, "{}", v(&mut s));
    let name = d["param"].as_str().or_else(|| d["name"].as_str()).map(str::to_string).unwrap_or_else(|| "d1".into());
    run(&mut s, "ChangeParameterCommand", json!({"name": name, "expression": "60"}));
    // The right top edge, now at x = 60: 18000 less the same 30 mm strip.
    assert!((v(&mut s) - (18000.0 - strip)).abs() < 0.05, "{}", v(&mut s));
    let fillet = s.doc.features.last().map(|f| f.id).unwrap_or(0);
    assert!(s.model.result(fillet).and_then(|r| r.warning.clone()).is_none(), "followed without a warning");
}

mod naming {
    use serde_json::{Value, json};

    use super::run;
    use crate::Session;

    fn volume(s: &mut Session) -> f64 {
        run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(0.0)
    }

    fn warning(s: &Session, name: &str) -> Option<String> {
        let id = s.doc.find_feature(name).map(|f| f.id)?;
        s.model.result(id).and_then(|r| r.warning.clone())
    }

    /// A 40 x 30 x 10 block (sketch S: l1…l4 counter-clockwise from the origin), a hole H, a
    /// fillet on the right top edge.
    fn block() -> Session {
        let mut s = Session::default();
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "SketchDimension", json!({"entities": ["l1"], "value": 40}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"distance": 10, "name": "Block"}));
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "H"}));
        run(&mut s, "CircleCenterRadius", json!({"center": [12, 15], "radius": 4}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"sketch": "H", "distance": 10, "operation": "cut", "name": "Hole"}));
        run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[40, 15, 10]], "radius": 2, "name": "Round"}));
        s
    }

    const STRIP: f64 = (4.0 - std::f64::consts::PI) * 30.0;

    #[test]
    fn faces_and_edges_are_named_from_their_history() {
        let mut s = block();
        let f = run(&mut s, "model.faces", json!({"body": "Body1"}));
        let names: Vec<&str> = f["faces"].as_array().into_iter().flatten().filter_map(|x| x["name"].as_str()).collect();
        for want in ["F2:start", "F2:top", "F2:side:l1", "F2:side:l2", "F2:side:l3", "F2:side:l4"] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
        assert!(names.iter().any(|n| n.starts_with("F4:side:c1")), "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("F5:blend:")), "{names:?}");
        let round = s.doc.find_feature("Round").cloned().unwrap();
        assert_eq!(round.edge_names, vec!["F2:side:l2|F2:top".to_string()]);
    }

    #[test]
    fn a_resize_keeps_the_edge() {
        let mut s = block();
        let d = s.doc.find_feature("S").and_then(|f| f.param_names.first().cloned()).unwrap_or_else(|| "d1".into());
        run(&mut s, "ChangeParameterCommand", json!({"name": d, "expression": "60"}));
        assert!((volume(&mut s) - (18000.0 - std::f64::consts::PI * 16.0 * 10.0 - STRIP)).abs() < 0.1, "{}", volume(&mut s));
        assert_eq!(warning(&s, "Round"), None);
    }

    #[test]
    fn suppress_and_reorder_upstream_keep_the_edge() {
        let mut s = block();
        let v = volume(&mut s);
        run(&mut s, "timeline.suppress", json!({"feature": "Hole", "suppressed": true}));
        assert!((volume(&mut s) - (12000.0 - STRIP)).abs() < 0.1);
        assert_eq!(warning(&s, "Round"), None);
        run(&mut s, "timeline.suppress", json!({"feature": "Hole", "suppressed": false}));
        assert!((volume(&mut s) - v).abs() < 1e-6);
        // The hole after the fillet: same part, same edge.
        run(&mut s, "timeline.reorder", json!({"feature": "Hole", "position": 4}));
        assert!((volume(&mut s) - v).abs() < 0.1, "{} {v}", volume(&mut s));
        assert_eq!(warning(&s, "Round"), None);
    }

    #[test]
    fn a_split_edge_warns_and_takes_the_nearest_piece() {
        let mut s = block();
        // Upstream of the fillet, a notch through the middle of the filleted edge.
        run(&mut s, "timeline.rollTo", json!({"feature": "Hole"}));
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "N"}));
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [36, 12], "p1": [44, 18]}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"sketch": "N", "distance": 10, "operation": "cut", "name": "Notch"}));
        run(&mut s, "timeline.rollTo", json!({}));
        let w = warning(&s, "Round").unwrap_or_default();
        assert!(w.contains("split"), "{w}");
    }

    #[test]
    fn a_removed_segment_warns() {
        let mut s = block();
        // Replace the right side (l2) of the rectangle with a new line: the face it made is gone.
        let sk = s.doc.find_feature("S").map(|f| f.id).unwrap();
        run(&mut s, "SketchActivate", json!({"sketch": sk}));
        run(&mut s, "sketch.delete", json!({"entities": ["l2"]}));
        run(&mut s, "DrawPolyline", json!({"points": [[40, 0], [40, 30]], "ids": ["right"]}));
        let r: Result<Value, _> = s.execute("SketchStop", &json!({}));
        assert!(r.is_ok() || r.is_err());
        let w = warning(&s, "Round").unwrap_or_default();
        assert!(w.contains("no longer exists"), "{w}");
    }

    fn face_names(s: &mut Session, body: &str) -> Vec<String> {
        let f = run(s, "model.faces", json!({ "body": body }));
        f["faces"].as_array().into_iter().flatten().filter_map(|x| x["name"].as_str().map(str::to_string)).collect()
    }

    #[test]
    fn primitives_revolves_and_pattern_copies_are_named() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "body_name": "B"}));
        let n = face_names(&mut s, "B");
        for want in ["F1:+x", "F1:-x", "F1:+y", "F1:-y", "F1:+z", "F1:-z"] {
            assert!(n.contains(&want.to_string()), "{want} in {n:?}");
        }
        run(&mut s, "PatternRectangular", json!({"bodies": ["B"], "dir1": [1, 0, 0], "count1": 3, "spacing1": 20}));
        let bodies: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
        let copy = face_names(&mut s, &bodies[2]);
        assert!(copy.iter().all(|x| x.starts_with("F1:") && x.contains("@F2.")), "{copy:?}");
        // A revolved ring: the section's four lines name the four faces.
        let mut r = Session::default();
        run(&mut r, "SketchCreate", json!({"plane": "XZ"}));
        run(&mut r, "DrawPolyline", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true, "ids": ["a", "b", "c", "d"]}));
        run(&mut r, "SketchStop", json!({}));
        run(&mut r, "Revolve", json!({"axis": "y", "angle": "270 deg"}));
        let body = r.model.state().bodies[0].name.clone();
        let n = face_names(&mut r, &body);
        for want in ["F2:side:a", "F2:side:b", "F2:side:c", "F2:side:d", "F2:start", "F2:end"] {
            assert!(n.iter().any(|x| x.starts_with(want)), "{want} in {n:?}");
        }
    }

    #[test]
    fn a_sketch_on_a_face_stays_on_that_face() {
        let mut s = Session::default();
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "S"}));
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"distance": 10, "name": "Block"}));
        run(&mut s, "SketchCreate", json!({"plane": {"face": [30, 15, 10]}, "name": "Top"}));
        run(&mut s, "CircleCenterRadius", json!({"center": [30, 15], "radius": 3}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"sketch": "Top", "distance": 5, "operation": "join", "name": "Peg"}));
        let top = |s: &mut Session| run(s, "MeasureCommand", json!({}))["bodies"][0]["bbox"]["max"][2].as_f64().unwrap_or(0.0);
        assert!((top(&mut s) - 15.0).abs() < 1e-6);
        // Upstream, a tall boss over the picked point: the nearest upward face is now its top,
        // but the sketch keeps to the block's top face by name.
        run(&mut s, "timeline.rollTo", json!({"feature": "Block"}));
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "B"}));
        run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [8, -5], "p1": [45, 35]}));
        run(&mut s, "SketchStop", json!({}));
        run(&mut s, "Extrude", json!({"sketch": "B", "distance": 20, "operation": "join", "name": "Boss"}));
        run(&mut s, "timeline.rollTo", json!({}));
        // (Its projected face outline changed, which it says; its plane did not.)
        assert!(!warning(&s, "Top").unwrap_or_default().contains("no longer exists"));
        // The peg starts on the block's top (z = 10), inside the boss: the part stays 20 tall.
        assert!((top(&mut s) - 20.0).abs() < 1e-6, "{}", top(&mut s));
        let sk = s.model.state().sketch(s.doc.find_feature("Top").map(|f| f.id).unwrap_or(0)).map(|x| x.plane.origin.z);
        assert_eq!(sk.map(|z| (z * 1e6).round() / 1e6), Some(10.0));
    }

    #[test]
    fn shell_inner_faces_are_named_after_the_walls() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
        run(&mut s, "FusionShellBodyCommand", json!({"faces": [[20, 15, 20]], "thickness": 2}));
        let n = face_names(&mut s, "B");
        for want in ["F2:inner:F1:-z", "F2:inner:F1:+x", "F2:inner:F1:-x", "F2:inner:F1:+y", "F2:inner:F1:-y"] {
            assert!(n.contains(&want.to_string()), "{want} in {n:?}");
        }
    }

    #[test]
    fn shell_and_hole_faces_follow_by_name() {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20, "name": "Base"}));
        run(&mut s, "FusionShellBodyCommand", json!({"faces": [[20, 15, 20]], "thickness": 2, "name": "Hollow"}));
        let shell = |h: f64| 40.0 * 30.0 * h - 36.0 * 26.0 * (h - 2.0);
        assert!((volume(&mut s) - shell(20.0)).abs() < 0.5, "{}", volume(&mut s));
        // Taller: the picked point is 5 mm under the open face now; its name still finds it.
        run(&mut s, "timeline.edit", json!({"feature": "Base", "set": {"height": "25"}}));
        assert!((volume(&mut s) - shell(25.0)).abs() < 0.5, "{}", volume(&mut s));
        assert_eq!(warning(&s, "Hollow"), None);
        let h = s.doc.find_feature("Hollow").map(|f| f.face_names.clone()).unwrap_or_default();
        assert_eq!(h, vec!["F1:+z".to_string()]);
    }
}

/// Tapped holes take the tap drill (major − pitch); clearance holes the ISO 273 diameter.
#[test]
fn tapped_and_clearance_holes() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 10}));
    let v0 = volume(&mut s);
    run(&mut s, "FusionHoleCommand", json!({"position": [10, 15, 10], "thread": "M6"}));
    let v1 = volume(&mut s);
    assert!(rel(v0 - v1, PI * 2.5 * 2.5 * 10.0) < 1e-3, "{}", v0 - v1);
    run(&mut s, "FusionHoleCommand", json!({"position": [30, 15, 10], "clearance": "M6"}));
    let v2 = volume(&mut s);
    assert!(rel(v1 - v2, PI * 3.3 * 3.3 * 10.0) < 1e-3, "{}", v1 - v2);
    run(&mut s, "FusionHoleCommand", json!({"position": [20, 5, 10], "clearance": "M4", "fit": "close"}));
    assert!(rel(v2 - volume(&mut s), PI * 2.15 * 2.15 * 10.0) < 1e-3);
    assert!(s.execute("FusionHoleCommand", &json!({"position": [20, 25, 10], "clearance": "M7"})).is_err());
    assert!(s.execute("FusionHoleCommand", &json!({"position": [20, 25, 10]})).is_err());
}

/// In an inch design, a bare length typed into a feature means inches (stored with the unit);
/// lengths with units and angles are left alone.
#[test]
fn bare_lengths_take_the_design_units() {
    let mut s = Session::default();
    s.doc_mut().units = "in".into();
    run(&mut s, "PrimitiveBox", json!({"length": 2, "width": "10 mm", "height": 1}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    let max = |i: usize| m["bodies"][0]["bbox"]["max"][i].as_f64().unwrap_or(0.0);
    assert!((max(0) - 50.8).abs() < 1e-9 && (max(1) - 10.0).abs() < 1e-9 && (max(2) - 25.4).abs() < 1e-9, "{m}");
    let d = s.doc.features[0].param_names[2].clone();
    run(&mut s, "ChangeParameterCommand", json!({"name": d, "expression": "2"}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert!((m["bodies"][0]["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 50.8).abs() < 1e-9);
    assert_eq!(s.doc.features[0].kind.inputs()[2].1, "2 in");
}

/// Press Pull on a fillet's face changes its radius; on another curved face it offsets it.
#[test]
fn press_pull_on_curved_faces() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[40, 0, 10]], "radius": 3, "name": "Round"}));
    let corner = |r: f64| (4.0 - PI) * r * r / 4.0 * 20.0;
    assert!(rel(volume(&mut s), 24000.0 - corner(3.0)) < 1e-4);
    // A point on the fillet: the arc's middle.
    let c = Vec3::new(37.0, 3.0, 10.0);
    let on = c + Vec3::new(1.0, -1.0, 0.0).normalized().unwrap() * 3.0;
    let out = run(&mut s, "FusionPressPullCommand", json!({"face": [on.x, on.y, on.z], "distance": 2}));
    assert_eq!(out["edited"], "Round", "{out}");
    assert!(rel(volume(&mut s), 24000.0 - corner(5.0)) < 1e-4, "{}", volume(&mut s));
    // A hole's wall: pulled along its normal (into the hole), the hole gets smaller.
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    run(&mut s, "FusionHoleCommand", json!({"position": [20, 15, 20], "diameter": 10}));
    let v0 = volume(&mut s);
    run(&mut s, "FusionPressPullCommand", json!({"face": [25, 15, 10], "distance": 1}));
    let v1 = volume(&mut s);
    assert!(rel(v1 - v0, PI * (25.0 - 16.0) * 20.0) < 1e-3, "{v0} {v1}");
}

/// A cut in one component leaves other components' bodies alone unless they are named as
/// participants; auto join/cut looks at the active component only.
#[test]
fn booleans_stay_in_their_component() {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "FusionCreateNewComponentCommand", json!({"name": name}));
        run(&mut s, "PrimitiveBox", json!({"length": 20, "width": 10, "height": 10, "corner": [x, 0, 0], "body_name": format!("{name}Box")}));
    }
    // A's box spans x 0…20, B's x 20…40 (in their own frames, both occurrences in place).
    run(&mut s, "component.activate", json!({"component": "A"}));
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [8, 2], "p1": [28, 8]}));
    run(&mut s, "SketchStop", json!({}));
    let vol = |s: &mut Session, b: &str| run(s, "MeasureCommand", json!({"bodies": [b]}))["total"]["volume_mm3"].as_f64().unwrap_or(0.0);
    assert_eq!(auto_operation(&s, &json!({"sketch": "Slot", "distance": 10})), Some("cut"));
    run(&mut s, "Extrude", json!({"sketch": "Slot", "distance": 10, "operation": "cut", "name": "Cut"}));
    assert!((vol(&mut s, "ABox") - (2000.0 - 12.0 * 6.0 * 10.0)).abs() < 1e-6, "{}", vol(&mut s, "ABox"));
    assert!((vol(&mut s, "BBox") - 2000.0).abs() < 1e-6, "B untouched: {}", vol(&mut s, "BBox"));
    // Named as a participant, B is cut too.
    run(&mut s, "UndoCommand", json!({}));
    run(&mut s, "Extrude", json!({"sketch": "Slot", "distance": 10, "operation": "cut", "participants": ["ABox", "BBox"]}));
    assert!((vol(&mut s, "BBox") - (2000.0 - 8.0 * 6.0 * 10.0)).abs() < 1e-6, "{}", vol(&mut s, "BBox"));
    assert!(s.execute("Extrude", &json!({"sketch": "Slot", "distance": 1, "operation": "cut", "participants": ["Nope"]})).is_err());
}

/// A unit-less parameter used as a length means the design's units.
#[test]
fn unitless_parameters_used_as_lengths_take_the_design_units() {
    let mut s = Session::default();
    s.doc_mut().units = "in".into();
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "2", "unit": ""}));
    run(&mut s, "PrimitiveBox", json!({"length": "w", "width": "w * 1", "height": "10 mm"}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    let max = |i: usize| m["bodies"][0]["bbox"]["max"][i].as_f64().unwrap_or(0.0);
    assert!((max(0) - 50.8).abs() < 1e-9 && (max(1) - 50.8).abs() < 1e-9 && (max(2) - 10.0).abs() < 1e-9, "{m}");
}

/// Chamfers with two distances or a distance and an angle.
#[test]
fn unequal_chamfers() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 40, "width": 30, "height": 20}));
    // Top front edge: 2 along the top (the face facing up), 4 down the front.
    run(&mut s, "FusionChamferCommand", json!({"edges": [[20, 0, 20]], "distance": 2, "distance2": 4}));
    assert!(rel(volume(&mut s), 24000.0 - 0.5 * 2.0 * 4.0 * 40.0) < 1e-6, "{}", volume(&mut s));
    // Bottom back edge: 3 along the bottom, at 30° from it (down the back face 3·tan 30°).
    run(&mut s, "FusionChamferCommand", json!({"edges": [[20, 30, 0]], "distance": 3, "angle": "30 deg", "flip": true}));
    let d2 = 3.0 * (30f64).to_radians().tan();
    assert!(rel(volume(&mut s), 24000.0 - 160.0 - 0.5 * 3.0 * d2 * 40.0) < 1e-6, "{}", volume(&mut s));
    assert!(s.execute("FusionChamferCommand", &json!({"edges": [[0, 15, 20]], "distance": 1, "distance2": 2, "angle": "30 deg"})).is_err());
}

/// An enclosure built in the usual order: shell, bosses, then the lip on the rim.
#[test]
fn lip_after_boss() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 80, "width": 50, "height": 30}));
    run(&mut s, "FusionShellBodyCommand", json!({"faces": [[40, 25, 30]], "thickness": 2}));
    run(&mut s, "FusionBossCommand", json!({"position": [10, 10, 2], "diameter": 7, "height": 12, "hole_diameter": 2.5}));
    run(&mut s, "FusionBossCommand", json!({"position": [70, 40, 2], "diameter": 7, "height": 12, "hole_diameter": 2.5}));
    let v0 = volume(&mut s);
    run(&mut s, "FusionLipCommand", json!({"face": [1, 25, 30], "width": 1, "height": 2}));
    let v = volume(&mut s);
    assert!(v > v0 + 1.0, "{v0} {v}");
    // Bosses up to the rim, with ribs, against a wall; lips, grooves and outside rims.
    let bosses = [
        json!({"position": [15, 15, 2], "diameter": 8, "height": 28, "hole_diameter": 3}),
        // Ribs out to the inner walls (15 − 4 − 9 = 2), and short of them.
        json!({"position": [15, 15, 2], "diameter": 8, "height": 12, "ribs": 4, "rib_thickness": 1.5, "rib_length": 9}),
        json!({"position": [15, 15, 2], "diameter": 8, "height": 12, "ribs": 4, "rib_thickness": 1.5, "rib_length": 6}),
        json!({"position": [5.5, 25, 2], "diameter": 7, "height": 12, "hole_diameter": 2.5}),
        json!({"position": [40, 25, 2], "diameter": 8, "height": 20, "draft": "1 deg", "fillet": 1}),
    ];
    let lips = [
        json!({"face": [1, 25, 30], "width": 1, "height": 2}),
        json!({"face": [1, 25, 30], "width": 1, "height": 2, "type": "groove"}),
        json!({"face": [1, 25, 30], "width": 1, "height": 2, "side": "outside"}),
    ];
    for (i, b) in bosses.iter().enumerate() {
        for (j, l) in lips.iter().enumerate() {
            let mut s = Session::default();
            run(&mut s, "PrimitiveBox", json!({"length": 80, "width": 50, "height": 30}));
            run(&mut s, "FusionShellBodyCommand", json!({"faces": [[40, 25, 30]], "thickness": 2}));
            let r = s.execute("FusionBossCommand", b);
            assert!(r.is_ok(), "boss {i}: {r:?}");
            let v0 = volume(&mut s);
            let r = s.execute("FusionLipCommand", l);
            assert!(r.is_ok(), "boss {i} lip {j}: {r:?}");
            assert!((volume(&mut s) - v0).abs() > 1.0, "boss {i} lip {j}");
        }
    }
}

/// A drilled hole as deep as the plate: the drill point breaks through the bottom.
#[test]
fn hole_as_deep_as_the_plate() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 50, "width": 25, "height": 10}));
    let v0 = volume(&mut s);
    run(&mut s, "FusionHoleCommand", json!({"position": [25, 12.5, 10], "diameter": 6, "depth": 10, "type": "drilled"}));
    let v = volume(&mut s);
    assert!(v < v0 - PI * 9.0 * 9.0 && v > v0 - PI * 9.0 * 10.0 - 1.0, "{v0} {v}");
    // Other sizes and places, the defaults, and a tapped hole.
    for (i, p) in [
        json!({"position": [10, 10, 10], "diameter": 5, "depth": 10, "type": "drilled"}),
        json!({"position": [40, 15, 10], "diameter": 5, "depth": 10}),
        json!({"position": [12, 6, 10], "diameter": 8.5, "depth": 10}),
        json!({"position": [30, 7, 10], "thread": "M6", "depth": 10}),
        json!({"position": [20, 18, 10], "diameter": 4, "depth": 10, "tip_angle": 90}),
    ]
    .into_iter()
    .enumerate()
    {
        let before = volume(&mut s);
        let r = s.execute("FusionHoleCommand", &p);
        assert!(r.is_ok(), "{i} {p}: {r:?}");
        assert!(volume(&mut s) < before - 1.0, "{i} {p}");
    }
}
