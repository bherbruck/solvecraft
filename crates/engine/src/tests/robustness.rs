//! Cases found by the robustness runs (`solvecraft-cli fuzz`, docs/robustness.md), cut down to
//! the steps that matter, and a fixed set of seeds that must keep running clean.

use serde_json::{Value, json};

use crate::Session;
use crate::fuzz;

/// Run a script; every command must succeed and every body be a sound solid.
fn script(steps: Value) -> Session {
    let mut s = Session::default();
    for st in steps.as_array().into_iter().flatten() {
        let (c, p) = (st["command"].as_str().unwrap_or(""), &st["params"]);
        if let Err(e) = s.execute(c, p) {
            panic!("{c} {p}: {e}");
        }
    }
    for b in &s.model.state().bodies {
        let bad = b.body.validity();
        assert!(bad.is_empty(), "{}: {bad:?}", b.name);
    }
    s
}

/// Shell of a notched plate closed by a shallow arc, opening a notch wall: the arc's cylinder
/// meets the plates at an angle, so its corners need more than one linear step to move.
#[test]
fn shell_with_a_shallow_arc_side() {
    script(json!([
        {"command": "sketch.create", "params": {"plane": "XY"}},
        {"command": "sketch.line", "params": {"points": [[0, 0], [37.5, 0], [37.5, 37.5], [22.5, 37.5], [22.5, 20.0], [12.5, 20.0], [12.5, 37.5], [0, 37.5]]}},
        {"command": "sketch.arc.three_point", "params": {"end": [0, 0], "start": [0, 37.5], "through": [-5.0, 18.75]}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.extrude", "params": {"distance": 7.5}},
        {"command": "solid.shell", "params": {"faces": [[12.5, 28.75, 3.75]], "thickness": 2.0}}
    ]));
}

/// Shell of a plate with a hole whose wall meets the plate's arc side: the edge where the two
/// cylinders meet is neither a line nor an arc, and is rebuilt through its moved points.
#[test]
fn shell_with_a_hole_through_a_curved_side() {
    script(json!([
        {"command": "sketch.create", "params": {"plane": "XY"}},
        {"command": "sketch.line", "params": {"points": [[0, 0], [42.5, 0], [42.5, 20.0], [0, 20.0]]}},
        {"command": "sketch.arc.three_point", "params": {"end": [0, 0], "start": [0, 20.0], "through": [-10.0, 10.0]}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.extrude", "params": {"distance": 27.5}},
        {"command": "solid.cylinder", "params": {"axis": [1.0, 0.0, 0.0], "base": [-10.000000000000002, 12.5, 12.5], "height": 17.5, "operation": "cut", "radius": 5.0}},
        {"command": "solid.shell", "params": {"faces": [[42.5, 10.0, 13.75]], "thickness": 2.0}}
    ]));
}

/// Shell of a body in two pieces (a cylinder joined clear of a ring): each piece is shelled.
#[test]
fn shell_of_a_body_in_two_pieces() {
    script(json!([
        {"command": "sketch.create", "params": {"plane": "XZ"}},
        {"command": "sketch.rectangle.two_point", "params": {"p0": [2.5, 0], "p1": [22.5, 17.5]}},
        {"command": "sketch.finish", "params": {}},
        {"command": "solid.revolve", "params": {"angle": "360 deg", "axis": "Z"}},
        {"command": "solid.cylinder", "params": {"axis": [0.0, 1.0, 0.0], "base": [-22.5, 22.5, 0.0], "height": 30.0, "operation": "join", "radius": 10.0}},
        {"command": "solid.shell", "params": {"faces": [[-22.5, 22.5, 0.0]], "thickness": 2.0}}
    ]));
}

/// A shell thicker than a step under a notch would turn the cavity inside out: a clear "not
/// supported yet" error instead of a failed boolean.
#[test]
fn shell_thicker_than_a_step_says_so() {
    let mut s = Session::default();
    for (c, p) in [
        ("solid.box", json!({"height": 7.5, "length": 30.0, "width": 35.0})),
        ("solid.box", json!({"corner": [0.0, 30.0, 2.5], "height": 22.5, "length": 22.5, "operation": "cut", "width": 20.0})),
    ] {
        s.execute(c, &p).unwrap();
    }
    let e = s.execute("solid.shell", &json!({"faces": [[18.75, 35.0, 2.5]], "thickness": 1.5})).unwrap_err();
    assert!(e.to_string().contains("not supported yet"), "{e}");
}

/// Seeds that run clean: no failure, no invalid body, no volume going the wrong way.
#[test]
fn fixed_seeds_run_clean() {
    for seed in FIXED_SEEDS {
        let r = fuzz::run(*seed, 8);
        assert!(!r.outcome.is_bug(), "seed {seed}: {:?}\n{}", r.outcome, r.script());
    }
}

const FIXED_SEEDS: &[u64] = &[3, 5, 11, 16, 21, 29, 33, 35];
