//! Cases found by the robustness runs (`solvecraft-cli fuzz`, docs/robustness.md), cut down to
//! the steps that matter, and a fixed set of seeds that must keep running clean.

use std::f64::consts::PI;

use serde_json::{Value, json};

use super::{run, volume};
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

/// Panics inside the kernel are caught natively, but abort the web build (wasm can't unwind).
/// Building the samples must not panic at all (the Plastic Enclosure's bosses used to, 8 times).
#[test]
fn samples_build_without_panics() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let me = std::thread::current().id();
    let count = Arc::new(AtomicUsize::new(0));
    let c2 = count.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == me {
            c2.fetch_add(1, Ordering::SeqCst);
        }
        let _ = info;
    }));
    let mut failed = Vec::new();
    for sample in crate::sample::samples() {
        let mut s = Session::default();
        for c in (sample.script)()["commands"].as_array().into_iter().flatten() {
            let _ = s.execute(c["command"].as_str().unwrap_or(""), &c["params"]);
        }
        let n = count.swap(0, Ordering::SeqCst);
        if n > 0 {
            failed.push(format!("{}: {n} panic(s)", sample.name));
        }
    }
    std::panic::set_hook(prev);
    assert!(failed.is_empty(), "{failed:?}");
}

fn total_volume(s: &Session) -> f64 {
    s.model.state().bodies.iter().map(|b| solvecraft_kernel::measure(&b.body).map(|m| m.volume).unwrap_or(0.0)).sum()
}

/// Volume of a box L×W×H with its vertical edges rounded (R) and its bottom loop filleted
/// (ρ ≤ R): the straight runs lose (1 − π/4)ρ² each, the corners a quarter turn of that
/// (Pappus, about the corner's axis).
fn rounded_box(l: f64, w: f64, h: f64, r: f64, rho: f64) -> f64 {
    use std::f64::consts::PI;
    let a = (1.0 - PI / 4.0) * rho * rho;
    let xbar = rho * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    l * w * h - (4.0 - PI) * r * r * h - a * (2.0 * (l - 2.0 * r) + 2.0 * (w - 2.0 * r)) - 2.0 * PI * (r - xbar) * a
}

/// Box, fillets, shell: a 60×40×30 box with its vertical edges rounded r5 and its bottom loop
/// r3, shelled 2 mm with the top open. The bottom round runs round the corners as spindle tori
/// (the tube wider than its circle), which the offset now takes.
#[test]
fn shell_of_a_filleted_box() {
    let s = script(json!([
        {"command": "solid.box", "params": {"length": 60, "width": 40, "height": 30}},
        {"command": "solid.fillet", "params": {"edges": [[0, 0, 15], [60, 0, 15], [60, 40, 15], [0, 40, 15]], "radius": 5}},
        {"command": "solid.fillet", "params": {"edges": [[30, 0, 0], [60, 20, 0], [30, 40, 0], [0, 20, 0]], "radius": 3}},
        {"command": "solid.shell", "params": {"faces": [[30, 20, 30]], "thickness": 2}}
    ]));
    let want = rounded_box(60.0, 40.0, 30.0, 5.0, 3.0) - rounded_box(56.0, 36.0, 28.0, 3.0, 1.0);
    let v = total_volume(&s);
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
}

/// The first tutorial part: a 60×40×15 plate with its top front edge and front right edge
/// rounded r2, shelled with the top open. At 2 mm the rounds' inner sides collapse to sharp
/// edges (as in Fusion); at 1.5 mm the top's round, tangent to the open top, keeps its
/// tangency (every face moves in, the opening is swept out through the top).
#[test]
fn shell_of_the_tutorial_plate() {
    let base = json!([
        {"command": "solid.box", "params": {"length": 60, "width": 40, "height": 15}},
        {"command": "solid.fillet", "params": {"edges": [[30, 0, 15], [60, 0, 7]], "radius": 2}}
    ]);
    let s0 = script(base.clone());
    let body = total_volume(&s0);
    // 2 mm: the cavity is a sharp 56 × 36 box from z = 2 up through the top.
    let mut steps = base.as_array().cloned().unwrap_or_default();
    steps.push(json!({"command": "solid.shell", "params": {"faces": [[30, 20, 15]], "thickness": 2}}));
    let v = total_volume(&script(Value::Array(steps)));
    let want = body - 56.0 * 36.0 * 13.0;
    assert!((v - want).abs() < 1e-4 * want, "{v} vs {want}");
    // 1.5 mm: a 57 × 37 × 12 cavity with r0.5 rounds, opened by its top face (57 × 36.5)
    // swept up through the top (estimated: the rounds' mitre is left out, hence 1e-3).
    let mut steps = base.as_array().cloned().unwrap_or_default();
    steps.push(json!({"command": "solid.shell", "params": {"faces": [[30, 20, 15]], "thickness": 1.5}}));
    let v = total_volume(&script(Value::Array(steps)));
    let a = (1.0 - std::f64::consts::PI / 4.0) * 0.25;
    let want = body - (57.0 * 37.0 * 12.0 - a * (57.0 + 12.0)) - 57.0 * 36.5 * 1.5;
    assert!((v - want).abs() < 1e-3 * want, "{v} vs {want}");
}

/// Shell after a drilled hole (its tip a cone): the cone moves like the other faces.
#[test]
fn shell_after_a_drilled_hole() {
    for open in [[30.0, 0.0, 7.5], [10.0, 10.0, 15.0]] {
        script(json!([
            {"command": "solid.box", "params": {"length": 60, "width": 40, "height": 15}},
            {"command": "solid.hole", "params": {"position": [30, 20, 15], "diameter": 5, "depth": 8, "type": "drilled"}},
            {"command": "solid.shell", "params": {"faces": [open], "thickness": 2}}
        ]));
    }
}

/// Rounding a box's edges one at a time: an edge running into an earlier round (of the same
/// radius or another) takes off about what each round alone would (the corner where they meet
/// is small).
#[test]
fn fillet_into_an_earlier_round() {
    let box_ = json!({"command": "solid.box", "params": {"length": 60, "width": 40, "height": 15}});
    for r2 in [2.0, 3.0] {
        let s = script(json!([
            box_,
            {"command": "solid.fillet", "params": {"edges": [[30, 0, 15]], "radius": 2}},
            {"command": "solid.fillet", "params": {"edges": [[60, 0, 7]], "radius": r2}}
        ]));
        let lost = |r: f64, len: f64| (1.0 - PI / 4.0) * r * r * len;
        let want = 36000.0 - lost(2.0, 60.0) - lost(r2, 15.0);
        let v = total_volume(&s);
        assert!((v - want).abs() < 2.0, "r2 {r2}: {v} vs {want}");
    }
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

/// A drilled hole as deep as the plate: the drill point breaks through the bottom.
#[test]
fn hole_as_deep_as_the_plate() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 25, "height": 10}));
    let v0 = volume(&mut s);
    run(&mut s, "solid.hole", json!({"position": [25, 12.5, 10], "diameter": 6, "depth": 10, "type": "drilled"}));
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
        let r = s.execute("solid.hole", &p);
        assert!(r.is_ok(), "{i} {p}: {r:?}");
        assert!(volume(&mut s) < before - 1.0, "{i} {p}");
    }
}

/// An enclosure built in the usual order: shell, bosses, then the lip on the rim.
#[test]
fn lip_after_boss() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 30}));
    run(&mut s, "solid.shell", json!({"faces": [[40, 25, 30]], "thickness": 2}));
    run(&mut s, "plastic.boss", json!({"position": [10, 10, 2], "diameter": 7, "height": 12, "hole_diameter": 2.5}));
    run(&mut s, "plastic.boss", json!({"position": [70, 40, 2], "diameter": 7, "height": 12, "hole_diameter": 2.5}));
    let v0 = volume(&mut s);
    run(&mut s, "plastic.lip", json!({"face": [1, 25, 30], "width": 1, "height": 2}));
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
            run(&mut s, "solid.box", json!({"length": 80, "width": 50, "height": 30}));
            run(&mut s, "solid.shell", json!({"faces": [[40, 25, 30]], "thickness": 2}));
            let r = s.execute("plastic.boss", b);
            assert!(r.is_ok(), "boss {i}: {r:?}");
            let v0 = volume(&mut s);
            let r = s.execute("plastic.lip", l);
            assert!(r.is_ok(), "boss {i} lip {j}: {r:?}");
            assert!((volume(&mut s) - v0).abs() > 1.0, "boss {i} lip {j}");
        }
    }
}

/// A lip, a groove and an outside lip on the rim of a rounded, shelled enclosure (#29: the
/// lip's walls continue the shell's walls, which a boolean join couldn't do).
#[test]
fn lip_on_a_rounded_shelled_rim() {
    use std::f64::consts::PI;
    let base = json!([
        {"command": "solid.box", "params": {"corner": [-40, -30, 0], "length": 80, "width": 60, "height": 30}},
        {"command": "solid.fillet", "params": {"edges": [[40, 30, 15], [-40, 30, 15], [-40, -30, 15], [40, -30, 15]], "radius": 6}},
        {"command": "solid.shell", "params": {"faces": [[0, 0, 30]], "thickness": 2}}
    ]);
    let v0 = total_volume(&script(base.clone()));
    let ar = |w: f64, h: f64, r: f64| w * h - (4.0 - PI) * r * r;
    let inside = ar(78.0, 58.0, 5.0) - ar(76.0, 56.0, 4.0);
    for (extra, want) in [
        (json!({}), v0 + inside * 1.5),
        (json!({"side": "outside"}), v0 + (ar(80.0, 60.0, 6.0) - ar(78.0, 58.0, 5.0)) * 1.5),
        // (The groove's band comes from the rim's meshed outline: within its sag.)
        (json!({"type": "groove"}), v0 - inside * 1.5),
    ] {
        let mut p = json!({"face": [0, 29, 30], "width": 1, "height": 1.5});
        if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        let mut steps = base.as_array().cloned().unwrap_or_default();
        steps.push(json!({"command": "plastic.lip", "params": p}));
        let v = total_volume(&script(Value::Array(steps)));
        assert!((v - want).abs() < 1e-3 * want, "{extra}: {v} vs {want}");
    }
}
