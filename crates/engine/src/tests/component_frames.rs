//! Components as frames (regression cases for user bug #4): picks, sketches and bodies on
//! moved components and pasted instances.

use std::f64::consts::PI;

use serde_json::json;

use super::{rel, run};
use crate::*;

/// Components are frames: world picks on a moved component (whichever is active) land on its
/// geometry, bodies moved into it keep their place, and sketches on its faces follow it.
#[test]
fn moved_component_picks_and_frames() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    let c = run(&mut s, "component.create", json!({"name": "Arm"}));
    let (cid, occ) = (c["component"].as_u64().unwrap(), c["occurrence"].as_u64().unwrap());
    run(&mut s, "solid.box", json!({"corner": [20, 0, 0], "length": 10, "width": 4, "height": 4, "body_name": "ArmBody"}));
    run(&mut s, "component.activate", json!({"component": 0}));
    run(&mut s, "occurrence.move", json!({"occurrence": occ, "translate": [0, 50, 0]}));
    let wbox = |s: &Session, b: &str| s.world_state().body(b).unwrap().mesh().bounds();
    let vol = |s: &mut Session, b: &str| run(s, "inspect.measure", json!({"bodies": [b]}))["total"]["volume_mm3"].as_f64().unwrap();
    // Root active: a fillet picked at the moved edge in the world.
    run(&mut s, "solid.fillet", json!({"edges": [[25, 54, 4]], "radius": 1}));
    assert!(rel(vol(&mut s, "ArmBody"), 160.0 - (1.0 - PI / 4.0) * 10.0) < 1e-4, "{}", vol(&mut s, "ArmBody"));
    // A sketch on the moved body's top face: its points sit on that face in the world.
    let sk = run(&mut s, "sketch.create", json!({"plane": {"face": [25, 52, 4]}}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 1}));
    run(&mut s, "sketch.finish", json!({}));
    let si = run(&mut s, "sketch.inspect", json!({"sketch": sk["sketch"]}));
    let z: Vec<f64> = si["points"].as_array().unwrap().iter().map(|p| p["world"][2].as_f64().unwrap()).collect();
    assert!(z.iter().all(|z| (z - 4.0).abs() < 1e-6), "{si}");
    // Base moved into the moved component stays where it was.
    let before = wbox(&s, "Base");
    run(&mut s, "component.move_bodies", json!({"bodies": ["Base"], "component": "Arm"}));
    let after = wbox(&s, "Base");
    assert!(before.min.dist(after.min) < 1e-9 && before.max.dist(after.max) < 1e-9, "{before:?} {after:?}");
    // A pasted instance: a pick on it lands on the shared definition.
    run(&mut s, "occurrence.copy", json!({"component": cid, "translate": [0, 20, 0]}));
    let v0 = vol(&mut s, "ArmBody");
    let names: Vec<String> = s.world_state().bodies.iter().map(|b| b.name.clone()).collect();
    let inst = names.iter().find(|n| n.starts_with("ArmBody (")).cloned().unwrap();
    let ib = wbox(&s, &inst);
    // Its bottom edge along x at the far side (y max, z 0).
    run(&mut s, "solid.fillet", json!({"edges": [[25, ib.max.y, 0]], "radius": 1}));
    assert!(vol(&mut s, "ArmBody") < v0 - 1.0, "the instance's pick reached the definition");
}
