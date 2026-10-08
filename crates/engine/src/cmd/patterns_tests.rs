use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

/// A 100 x 50 x 5 plate with a hole at the start of a straight path (10,10) → (90,10).
fn plate_with_hole() -> Session {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 100, "width": 50, "height": 5, "corner": [0, 0, 0]}));
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Path"}));
    run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [90, 10]], "ids": ["l1"]}));
    run(&mut s, "CircleCenterRadius", json!({"center": [10, 10], "radius": 2, "id": "c1"}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"sketch": "Path", "through_all": true, "operation": "cut", "name": "Hole1"}));
    s
}

const PLATE: f64 = 100.0 * 50.0 * 5.0;
const HOLE: f64 = PI * 4.0 * 5.0;

#[test]
fn pattern_on_path_spacing_extent_and_flip() {
    let mut s = plate_with_hole();
    assert!(rel(volume(&mut s), PLATE - HOLE) < 1e-4);
    run(&mut s, "PatternOnPath", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 5, "spacing": 20}));
    assert!(rel(volume(&mut s), PLATE - 5.0 * HOLE) < 1e-4);
    // The count is a parameter like any other input.
    let n = s.doc.features.last().and_then(|f| f.param_names.first().cloned()).unwrap_or_default();
    run(&mut s, "ChangeParameterCommand", json!({"name": n, "expression": "3"}));
    assert!(rel(volume(&mut s), PLATE - 3.0 * HOLE) < 1e-4);
    // Instances past the end of the path are an error.
    run(&mut s, "ChangeParameterCommand", json!({"name": n, "expression": "6"}));
    let err = s.model.results.last().and_then(|r| r.error.clone()).unwrap_or_default();
    assert!(err.contains("past the end of the path"), "{err}");

    // Distance = first to last instance.
    let mut s = plate_with_hole();
    run(&mut s, "PatternOnPath", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 3, "distance": 80}));
    assert!(rel(volume(&mut s), PLATE - 3.0 * HOLE) < 1e-4);

    // Flipped, the path runs from (90,10) back toward (10,10).
    let mut s = plate_with_hole();
    run(&mut s, "PatternOnPath", json!({"features": ["Hole1"], "path_sketch": "Path", "path": ["l1"], "count": 2, "spacing": 30, "flip": true}));
    // The copy moves by path(30) − path(0) = (60,10) − (90,10) = −30: off the plate side → only 1 hole.
    assert!(rel(volume(&mut s), PLATE - HOLE) < 1e-4);
}

#[test]
fn patterns_of_bodies() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0], "body_name": "Cube"}));
    run(&mut s, "PatternCircular", json!({"bodies": ["Cube"], "axis": "Z", "count": 4}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["body_count"], 4, "{m}");
    assert!(rel(volume(&mut s), 4000.0) < 1e-4);
    run(&mut s, "PatternRectangular", json!({"bodies": ["Cube"], "dir1": [0, 0, 1], "count1": 3, "spacing1": 20}));
    assert!(rel(volume(&mut s), 6000.0) < 1e-4);
    assert!(s.execute("PatternRectangular", &json!({"bodies": ["Nope"], "dir1": [1, 0, 0], "count1": 2, "spacing1": 5})).is_err());
}

#[test]
fn patterns_of_components_place_occurrences() {
    let mut s = Session::default();
    run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Peg"}));
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10, "corner": [20, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    let r = run(&mut s, "PatternRectangular", json!({"components": ["Peg"], "dir1": [0, 0, 1], "count1": 3, "spacing1": 20}));
    assert_eq!(r["occurrences"].as_array().map(Vec::len), Some(2), "{r}");
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["body_count"], 3, "{m}");
    let zs: Vec<f64> = m["bodies"].as_array().unwrap().iter().filter_map(|b| b["bbox"]["min"][2].as_f64()).collect();
    for z in [0.0, 20.0, 40.0] {
        assert!(zs.iter().any(|x| (x - z).abs() < 1e-6), "{zs:?}");
    }
    assert!(s.execute("PatternCircular", &json!({"components": ["Nope"], "axis": "Z", "count": 2})).is_err());
}

#[test]
fn path_pattern_orientation_follows_the_path() {
    let width_of_copy = |orient: &str| {
        let mut s = Session::default();
        run(&mut s, "PrimitiveBox", json!({"length": 2, "width": 4, "height": 2, "corner": [19, -2, 0], "body_name": "Tab"}));
        run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Arc"}));
        run(&mut s, "ArcCenterTwoPoint", json!({"center": [0, 0], "start": [20, 0], "sweep": 90, "id": "a1"}));
        run(&mut s, "SketchStop", json!({}));
        run(
            &mut s,
            "PatternOnPath",
            json!({"bodies": ["Tab"], "path_sketch": "Arc", "path": ["a1"], "count": 2, "distance": "pi * 10", "orientation": orient}),
        );
        let m = run(&mut s, "MeasureCommand", json!({}));
        let b = m["bodies"].as_array().unwrap().iter().find(|b| b["name"] != "Tab").unwrap().clone();
        let (lo, hi) = (b["bbox"]["min"].clone(), b["bbox"]["max"].clone());
        // The copy sits at the arc's end (0, 20).
        assert!(((lo[1].as_f64().unwrap() + hi[1].as_f64().unwrap()) / 2.0 - 20.0).abs() < 1e-6, "{b}");
        hi[0].as_f64().unwrap() - lo[0].as_f64().unwrap()
    };
    assert!((width_of_copy("identical") - 2.0).abs() < 1e-6);
    assert!((width_of_copy("path") - 4.0).abs() < 1e-6);
}
