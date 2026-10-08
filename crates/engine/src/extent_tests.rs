//! To Object extents on curved targets, and shells outside/both on curved bodies.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn body(s: &mut Session, b: &str) -> Value {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0].clone()
}

/// A square post extruded up to a horizontal cylinder (axis along x, r 10, centre z 30): its
/// top takes the cylinder's shape, so the post is shortest in the middle.
#[test]
fn extrude_to_a_cylinder_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 30], "axis": [1, 0, 0], "radius": 10, "height": 40, "body_name": "Bar"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-4, -4], "p1": [4, 4]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 20], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    // Highest at the post's sides (y = ±4): z = 30 − √(100 − 16); the volume follows the arc.
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (30.0 - (100.0f64 - 16.0).sqrt())).abs() < 1e-3, "{b}");
    // ∫ over y of (30 − √(100 − y²)) × 8 (x width), y from −4 to 4.
    let n = 2000;
    let want: f64 = (0..n).map(|i| -4.0 + 8.0 * (i as f64 + 0.5) / n as f64).map(|y| (30.0 - (100.0 - y * y).sqrt()) * 8.0 * 8.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// A round post extruded up to a sphere (r 15 at z 40): it stops on the sphere's underside.
#[test]
fn extrude_to_a_sphere_follows_it() {
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 40], "radius": 15, "body_name": "Ball"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "name": "P"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"sketch": "P", "to": [0, 0, 25], "body_name": "Post"}));
    let b = body(&mut s, "Post");
    let top = b["bbox"]["max"][2].as_f64().unwrap_or(0.0);
    assert!((top - (40.0 - (225.0f64 - 25.0).sqrt())).abs() < 1e-2, "{b}");
    // Volume: cylinder to z 25 plus the ring between the sphere cap and the plane z = 25.
    let cap = |r: f64| 40.0 - (225.0 - r * r).sqrt();
    let n = 2000;
    let want: f64 = (0..n).map(|i| 5.0 * (i as f64 + 0.5) / n as f64).map(|r| cap(r) * 2.0 * PI * r * 5.0 / n as f64).sum();
    assert!((b["volume_mm3"].as_f64().unwrap_or(0.0) - want).abs() < want * 2e-3, "{} vs {want}", b["volume_mm3"]);
}

/// Shell Outside and Both on a cylinder (open top): exact offsets of the curved wall.
#[test]
fn shell_outside_and_both_on_a_cylinder() {
    let shell = |dir: &str| {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "C"}));
        run(&mut s, "solid.shell", json!({"faces": [[0, 0, 20]], "thickness": 2, "direction": dir}));
        body(&mut s, "C")["volume_mm3"].as_f64().unwrap_or(0.0)
    };
    let out = PI * (144.0 * 22.0 - 100.0 * 20.0);
    assert!((shell("outside") - out).abs() < out * 1e-3, "{} vs {out}", shell("outside"));
    let both = PI * (121.0 * 21.0 - 81.0 * 19.0);
    assert!((shell("both") - both).abs() < both * 1e-3, "{} vs {both}", shell("both"));
}

/// A ring section revolved To Object up to a post standing in its path: it ends on the post's
/// round side (touching, not overlapping), between first contact and the post's centre.
#[test]
fn revolve_to_a_cylinder_follows_it() {
    for side in [1.0, -1.0] {
        let mut s = Session::default();
        run(&mut s, "solid.cylinder", json!({"base": [0, 14.0 * side, -5], "radius": 4, "height": 15, "body_name": "Post"}));
        run(&mut s, "sketch.create", json!({"plane": "XZ", "name": "S"}));
        run(&mut s, "sketch.line", json!({"points": [[10, 0], [15, 0], [15, 5], [10, 5]], "closed": true}));
        run(&mut s, "sketch.finish", json!({}));
        let r = s.execute("solid.revolve", &json!({"sketch": "S", "axis": "y", "to": [4, 14.0 * side, 2], "body_name": "Arc"}));
        if r.is_err() {
            continue; // the post on the side the revolve turns away from is past the turn
        }
        let st = s.model.state();
        let (arc, post) = (st.body("Arc").unwrap(), st.body("Post").unwrap());
        let overlap = solvecraft_kernel::boolean(&arc.body, &post.body, solvecraft_kernel::BoolOp::Intersect)
            .ok()
            .flatten()
            .map(|b| solvecraft_kernel::measure(&b).map(|m| m.volume).unwrap_or(0.0))
            .unwrap_or(0.0);
        assert!(overlap < 1e-3, "no overlap: {overlap}");
        let v = solvecraft_kernel::measure(&arc.body).unwrap().volume;
        let full = PI * (225.0 - 100.0) * 5.0;
        // The post is at a quarter turn one way (three quarters the other): the ring stops
        // short of the post's centre line, within its half-width of it.
        let turned = v / full * 360.0;
        let to_centre = if turned < 180.0 { 90.0 } else { 270.0 };
        assert!(turned < to_centre && turned > to_centre - 30.0, "{turned}°");
        return;
    }
    panic!("neither post was reached");
}
