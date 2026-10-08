//! Split Face: the top of a box divided by a plane, by another body's face and by sketch curves.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// (kernel faces, volume) of a body: the merged count joins pieces of one surface back up.
fn faces_volume(s: &mut Session, b: &str) -> (u64, f64) {
    let m = run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone();
    (m["kernel"]["faces"].as_u64().unwrap_or(0), m["volume_mm3"].as_f64().unwrap_or(0.0))
}

fn boxed() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    s
}

#[test]
fn split_a_face_by_a_plane() {
    let mut s = boxed();
    run(&mut s, "solid.split_face", json!({"faces": [[10, 15, 20]], "plane": {"origin": [20, 0, 0], "normal": [1, 0, 0]}}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 7);
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
    assert_eq!(s.doc.features.last().unwrap().kind.type_name(), "SplitFaceFeature");
}

#[test]
fn split_a_face_by_another_bodys_face() {
    let mut s = boxed();
    run(&mut s, "solid.box", json!({"corner": [-5, 10, 25], "length": 50, "width": 5, "height": 5, "body_name": "Tool"}));
    // The tool's face at y = 10, extended down through the box's top.
    run(&mut s, "solid.split_face", json!({"faces": [[20, 5, 20]], "body": "B", "tool": {"body": "Tool", "point": [20, 10, 27]}}));
    assert_eq!(faces_volume(&mut s, "B").0, 7);
}

#[test]
fn split_a_face_by_sketch_curves() {
    let mut s = boxed();
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "Cuts"}));
    run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 35]]}));
    run(&mut s, "sketch.line", json!({"points": [[30, -5], [30, 35]]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.split_face", json!({"faces": [[20, 15, 20]], "body": "B", "sketch": "Cuts"}));
    let (f, v) = faces_volume(&mut s, "B");
    assert_eq!(f, 8, "two lines make three pieces of the top");
    assert!((v - 24000.0).abs() < 1e-6 * 24000.0, "{v}");
}

#[test]
fn a_tool_that_misses_is_refused() {
    let mut s = boxed();
    let n = s.doc.features.len();
    assert!(s.execute("solid.split_face", &json!({"faces": [[10, 15, 20]], "plane": {"origin": [100, 0, 0], "normal": [1, 0, 0]}})).is_err());
    assert_eq!(s.doc.features.len(), n);
}
