//! Components as frames: what a component holds moves with its occurrence, and picks in the
//! world land in the component's own frame (user bug #4).

use serde_json::{Value, json};
use solvecraft_geom::Vec3;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn near(a: Vec3, b: Vec3) -> bool {
    a.dist(b) < 1e-6
}

fn world_bbox(s: &Session, body: &str) -> (Vec3, Vec3) {
    let w = s.world_state();
    let b = w.body(body).unwrap_or_else(|| panic!("no body {body}")).mesh().bounds();
    (b.min, b.max)
}

fn sketch_point_world(s: &mut Session, sketch: &str, point: &str) -> Vec3 {
    let si = run(s, "sketch.inspect", json!({ "sketch": sketch }));
    let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == point).unwrap()["world"].clone();
    Vec3::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap(), p[2].as_f64().unwrap())
}

fn volume(s: &mut Session) -> f64 {
    run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap()
}

/// A component "Part" with a 30 x 20 x 10 block (sketch "Base" on XY, "Top" on its top face)
/// and a construction plane, moved 50 mm along X.
fn moved_part() -> Session {
    let mut s = Session::default();
    run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Part"}));
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [30, 20]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 10}));
    run(&mut s, "SketchCreate", json!({"plane": {"face": [15, 10, 10]}, "name": "Top"}));
    run(&mut s, "CircleCenterRadius", json!({"center": [15, 10], "radius": 3}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "ConstructionPlaneOffsetFromPlaneCommand", json!({"base": "XY", "offset": 25, "name": "Shelf"}));
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [50, 0, 0]}));
    s
}

/// (a) Sketches and construction planes move with their component's occurrence.
#[test]
fn sketches_and_planes_move_with_the_component() {
    let mut s = moved_part();
    let (lo, hi) = world_bbox(&s, "Body1");
    assert!(near(lo, Vec3::new(50.0, 0.0, 0.0)) && near(hi, Vec3::new(80.0, 20.0, 10.0)), "{lo:?} {hi:?}");
    assert!(near(sketch_point_world(&mut s, "Base", "p1"), Vec3::new(50.0, 0.0, 0.0)));
    let c = sketch_point_world(&mut s, "Top", "p1");
    assert!((c.z - 10.0).abs() < 1e-6 && c.x >= 50.0 - 1e-6, "{c:?}");
    let shelf = crate::view::construction_planes(&s).into_iter().find(|(_, n, _)| n == "Shelf").unwrap().2;
    assert!(near(shelf.origin, Vec3::new(50.0, 0.0, 25.0)), "{:?}", shelf.origin);
    // A turn about Z carries them too.
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "axis": [0, 0, 1], "angle": "90 deg", "origin": [50, 0, 0]}));
    assert!(near(sketch_point_world(&mut s, "Base", "p3"), Vec3::new(30.0, 30.0, 0.0)), "{:?}", sketch_point_world(&mut s, "Base", "p3"));
}

/// (b) Picks on a moved component, given in the world, land on its geometry: fillet edges, a
/// sketch on a face, a hole, a shell; new geometry of the active component goes where asked.
#[test]
fn world_picks_map_into_the_component() {
    let mut s = moved_part();
    let v0 = volume(&mut s);
    // The edge along X at the top front: world (65, 0, 10).
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[65, 0, 10]], "radius": 2}));
    assert!(volume(&mut s) < v0 - 1.0, "the fillet cut material");
    // A sketch on the moved top face sits at world z = 10 over the moved block.
    run(&mut s, "SketchCreate", json!({"plane": {"face": [70, 10, 10]}, "name": "Pad"}));
    run(&mut s, "CircleCenterRadius", json!({"center": [20, 10], "radius": 2}));
    run(&mut s, "SketchStop", json!({}));
    let pad = sketch_point_world(&mut s, "Pad", "p1");
    assert!((pad.z - 10.0).abs() < 1e-6 && pad.x > 50.0, "{pad:?}");
    let v1 = volume(&mut s);
    let h = run(&mut s, "FusionHoleCommand", json!({"position": [60, 10, 10], "diameter": 4, "depth": 5}));
    assert!(volume(&mut s) < v1 - 1.0, "the hole cut material: {h}");
    // A box made at world (100, 0, 0) in the active (moved) component shows there.
    run(&mut s, "PrimitiveBox", json!({"length": 5, "width": 5, "height": 5, "corner": [100, 0, 0], "operation": "new"}));
    let last = s.model.state().bodies.last().map(|b| b.name.clone()).unwrap();
    let (lo, _) = world_bbox(&s, &last);
    assert!(near(lo, Vec3::new(100.0, 0.0, 0.0)), "{lo:?}");
}

/// (b) A pasted instance's pick fillets the component (every instance).
#[test]
fn picks_on_an_instance_map_through_its_placement() {
    let mut s = Session::default();
    run(&mut s, "FusionCreateNewComponentCommand", json!({"name": "Pin"}));
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "occurrence.copy", json!({"component": "Pin", "translate": [0, 40, 0]}));
    let v0 = volume(&mut s);
    // An edge of the second instance only (world y = 40).
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[5, 40, 10]], "radius": 2}));
    let v1 = volume(&mut s);
    assert!(v1 < v0 - 1.0, "{v0} {v1}");
}

/// (c) Moving a body into a moved component keeps it where it is in the world.
#[test]
fn moving_bodies_between_components_keeps_their_place() {
    let mut s = moved_part();
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "PrimitiveBox", json!({"length": 4, "width": 4, "height": 4, "corner": [0, 40, 0], "body_name": "Loose"}));
    let before = world_bbox(&s, "Loose");
    run(&mut s, "component.move_bodies", json!({"bodies": ["Loose"], "component": "Part"}));
    let after = world_bbox(&s, "Loose");
    assert!(near(before.0, after.0) && near(before.1, after.1), "{before:?} {after:?}");
    // It moves with the component from now on, and a pick on it still lands.
    run(&mut s, "occurrence.move", json!({"occurrence": "Part:1", "translate": [0, 0, 10]}));
    let moved = world_bbox(&s, "Loose");
    assert!(near(moved.0, before.0 + Vec3::new(0.0, 0.0, 10.0)), "{moved:?}");
    let v0 = volume(&mut s);
    run(&mut s, "FusionFilletEdgesCommand", json!({"edges": [[2, 40, 14]], "radius": 1}));
    assert!(volume(&mut s) < v0 - 0.1);
    // Back to the root: still in place.
    run(&mut s, "component.move_bodies", json!({"bodies": ["Loose"], "component": "root"}));
    let back = world_bbox(&s, "Loose");
    assert!(near(back.0, moved.0) && near(back.1, moved.1), "{back:?} {moved:?}");
}

/// (d) A sketch on a body's face follows the body when a Move feature moves it.
#[test]
fn sketch_on_a_face_follows_a_moved_body() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 20, "width": 20, "height": 10}));
    run(&mut s, "SketchCreate", json!({"plane": {"face": [10, 10, 10]}, "name": "OnTop"}));
    run(&mut s, "CircleCenterRadius", json!({"center": [10, 10], "radius": 2}));
    run(&mut s, "SketchStop", json!({}));
    // The move goes before the sketch in the timeline: the face the sketch is on moves up.
    run(&mut s, "timeline.rollTo", json!({"position": 1}));
    run(&mut s, "FusionMoveCommand", json!({"bodies": ["Body1"], "translate": [0, 0, 15]}));
    run(&mut s, "timeline.rollTo", json!({}));
    let c = sketch_point_world(&mut s, "OnTop", "p1");
    assert!((c.z - 25.0).abs() < 1e-6, "{c:?}");
}
