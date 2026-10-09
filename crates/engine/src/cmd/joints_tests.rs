use std::f64::consts::FRAC_PI_2;

use serde_json::{Value, json};
use solvecraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn occ(s: &Session, component: &str) -> u64 {
    let c = s.doc.find_component(component).unwrap_or(0);
    s.doc.occurrence_of(c).map(|o| o.id).unwrap_or(0)
}

fn place(s: &Session, component: &str, p: Vec3) -> Vec3 {
    solvecraft_doc::apply_point(&solvecraft_doc::joints::occurrence_world(&s.doc, occ(s, component)), p)
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

/// A hinge leaf (oracle 63's knuckle geometry): a 40 x 30 x 3 plate and an r4 knuckle along X
/// at y = 33, z = 4.
fn leaf(s: &mut Session, name: &str) {
    run(s, "component.activate", json!({"component": "root"}));
    run(s, "component.create", json!({"name": name}));
    run(s, "solid.box", json!({"length": 40, "width": 30, "height": 3, "corner": [0, 0, 0]}));
    run(s, "solid.cylinder", json!({"base": [0, 33, 4], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
}

#[test]
fn revolute_hinge_turns_about_the_knuckle() {
    let mut s = Session::default();
    leaf(&mut s, "LeafA");
    leaf(&mut s, "LeafB");
    let o = occ(&s, "LeafA");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    let r = run(
        &mut s,
        "joint.create",
        json!({
        "type": "revolute", "a": {"occurrence": "LeafA", "circle": [1, 37, 4]}, "b": {"occurrence": "LeafB", "circle": [1, 37, 4]}, "flip": true, "name": "Hinge"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // Same frames: B stays put at angle 0.
    assert!(near(place(&s, "LeafB", Vec3::new(20.0, 0.0, 0.0)), Vec3::new(20.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    let dof = l["dof"].as_array().unwrap().iter().find(|d| d["occurrence"] == occ(&s, "LeafB")).unwrap()["dof"].clone();
    assert_eq!(dof, 1, "{l}");
    // Drive 90°: B turns about the knuckle axis (through (0, 33, 4), along the frame's z = −X).
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": 90}));
    let want = solvecraft_doc::rigid(Vec3::ZERO, Vec3::new(0.0, 33.0, 4.0), Vec3::new(-1.0, 0.0, 0.0), FRAC_PI_2);
    let p = Vec3::new(20.0, 0.0, 0.0);
    assert!(near(place(&s, "LeafB", p), solvecraft_doc::apply_point(&want, p)), "{:?}", place(&s, "LeafB", p));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
    // LeafA did not move; the knuckle axis is fixed.
    assert!(near(place(&s, "LeafA", p), p));
    assert!(near(place(&s, "LeafB", Vec3::new(10.0, 33.0, 4.0)), Vec3::new(10.0, 33.0, 4.0)));
    // Expressions drive too, and undo puts it back.
    run(&mut s, "joint.drive", json!({"joint": "Hinge", "value": "pi rad"}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 66.0, 8.0)));
    run(&mut s, "edit.undo", json!({}));
    assert!(near(place(&s, "LeafB", p), Vec3::new(20.0, 29.0, 37.0)));
}

/// Two 10 mm cubes in components A (grounded) and B.
fn two_cubes() -> Session {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 20)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [x, 0, 0]}));
    }
    let o = occ(&s, "A");
    run(&mut s, "occurrence.ground", json!({"occurrence": o, "grounded": true}));
    s
}

#[test]
fn slider_limits_clamp() {
    let mut s = two_cubes();
    // B's bottom face onto A's top face (faces meet), sliding along their common normal.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "limits": [[0, 10]], "name": "Rail"}),
    );
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    // B now sits on A.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
    let r = run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 25}));
    assert_eq!(r["values"], json!([10.0]), "{r}");
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 20.0)));
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": -5}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    // Changing the limits clamps the current value.
    run(&mut s, "joint.drive", json!({"joint": "Rail", "value": 8}));
    run(&mut s, "joint.limits", json!({"joint": "Rail", "min": 0, "max": 4}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 14.0)));
    // Joints follow edits: A gets taller, B rides on its new top.
    let h = s
        .doc
        .features
        .iter()
        .find(|f| matches!(f.kind, solvecraft_doc::FeatureKind::Box { .. }))
        .and_then(|f| f.param_names.get(2).cloned())
        .unwrap_or_default();
    run(&mut s, "parameters.change", json!({"name": h, "expression": "15"}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 19.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 0.0)));
}

#[test]
fn rigid_cycles_solve_and_conflicts_report() {
    let mut s = two_cubes();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "C"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [40, 0, 0]}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    // A rigid group and a closing as-built joint: a consistent loop.
    run(&mut s, "joint.rigid_group", json!({"occurrences": ["A", "B", "C"]}));
    let r = run(&mut s, "joint.as_built", json!({"type": "rigid", "a": "B", "b": "C"}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0), "{r}");
    assert!(near(place(&s, "C", Vec3::new(40.0, 0.0, 0.0)), Vec3::new(40.0, 0.0, 0.0)));
    let l = run(&mut s, "joint.list", json!({}));
    assert!(l["dof"].as_array().unwrap().iter().all(|d| d["dof"] == 0), "{l}");
    // A joint that wants C elsewhere conflicts and says so.
    let r = run(
        &mut s,
        "joint.create",
        json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "C", "point": [40, 0, 0]}, "name": "Wrong"}),
    );
    let c = r["conflicts"].as_array().cloned().unwrap_or_default();
    assert_eq!(c.len(), 1, "{r}");
    assert!(c[0]["why"].as_str().unwrap_or("").contains("off by"), "{r}");
    // Suppressing it clears the conflict.
    let r = run(&mut s, "joint.edit", json!({"joint": "Wrong", "suppressed": true}));
    assert_eq!(r["conflicts"].as_array().map(Vec::len), Some(0));
    assert!(
        s.execute(
            "joint.create",
            &json!({"type": "rigid", "a": {"occurrence": "A", "point": [0, 0, 0]}, "b": {"occurrence": "A", "point": [1, 0, 0]}})
        )
        .is_err()
    );
    assert!(s.execute("joint.create", &json!({"type": "hinge"})).is_err());
}

#[test]
fn motion_links_and_interference() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "cylindrical", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Screw"}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 0]}));
    // No overlap while B sits on A; then push B 4 mm into A.
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, -4]}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert!((i["interferences"][0]["volume_mm3"].as_f64().unwrap_or(0.0) - 400.0).abs() < 1e-3, "{i}");
    // A motion link: a second joint's value follows.
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "component.create", json!({"name": "D"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [60, 0, 0]}));
    run(
        &mut s,
        "joint.create",
        json!({"type": "slider", "a": {"occurrence": "A", "face": [5, 5, 10]}, "b": {"occurrence": "D", "face": [65, 5, 0]}, "name": "Follower"}),
    );
    run(&mut s, "joint.motion_link", json!({"a": "Screw", "ia": 1, "b": "Follower", "ratio": 2}));
    run(&mut s, "joint.drive", json!({"joint": "Screw", "values": [0, 3]}));
    assert!(near(place(&s, "D", Vec3::new(65.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 16.0)), "{:?}", place(&s, "D", Vec3::new(65.0, 5.0, 0.0)));
    // Saved and reopened, joints come back.
    let back = solvecraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    assert_eq!(back.assembly, s.doc.assembly);
    let mut t = Session::new(back);
    let l = run(&mut t, "joint.list", json!({}));
    assert_eq!(l["joints"].as_array().map(Vec::len), Some(2));
}

/// Two faces joined mate: B's bottom face lands on A's top face, B on top of A (not inside
/// it); Flip turns B over instead.
#[test]
fn faces_mate_by_default() {
    let mut s = two_cubes();
    run(&mut s, "joint.create", json!({"type": "rigid", "a": {"face": [5, 5, 10]}, "b": {"face": [25, 5, 0]}, "name": "Stack"}));
    // B's bottom centre (25, 5, 0) at A's top centre, B above it.
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 0.0)), Vec3::new(5.0, 5.0, 10.0)));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 20.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
    run(&mut s, "joint.edit", json!({"joint": "Stack", "flip": true}));
    assert!(near(place(&s, "B", Vec3::new(25.0, 5.0, 10.0)), Vec3::new(5.0, 5.0, 0.0)), "{:?}", place(&s, "B", Vec3::new(25.0, 5.0, 10.0)));
}
