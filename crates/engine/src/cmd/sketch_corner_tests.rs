//! Sketch Fillet and Chamfer at corners (#46): line–line, line–arc and arc–arc corners, the
//! corners of a constrained rectangle, several corners at once, and radii too large for the
//! corner (a clear error that leaves the sketch as it was).

use serde_json::{Value, json};

use crate::Session;

fn new_sketch() -> Session {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY"})).unwrap();
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn curves(r: &Value) -> Vec<String> {
    r["curves"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sk(s: &Session) -> solvecraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn solved(s: &Session) -> bool {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().report.ok()
}

fn area(s: &Session) -> f64 {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.iter().map(|p| p.area).sum()
}

#[test]
fn fillet_and_chamfer_corners() {
    use std::f64::consts::PI;
    // A constrained 40 × 20 rectangle: a fillet r5 on one corner keeps it solved and closed,
    // with the area of the corner taken off.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.fillet", json!({"a": c[0], "b": c[1], "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Several corners at once: the rest of the rectangle's corners.
    let pts: Vec<String> = {
        let k = sk(&s);
        let li = |id: &str| k.curve_index(id).unwrap();
        [(1, 2), (2, 3), (3, 0)]
            .iter()
            .map(|(x, y)| {
                let (cx, cy) = (&k.curves[li(&c[*x])].kind, &k.curves[li(&c[*y])].kind);
                let shared = cx.point_ids().into_iter().find(|p| cy.point_ids().contains(p)).unwrap();
                k.points[shared].id.clone()
            })
            .collect()
    };
    run(&mut s, "sketch.fillet", json!({"points": pts, "radius": 5}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 4.0 * (25.0 - PI * 25.0 / 4.0))).abs() < 1e-6, "{}", area(&s));
    // Too large for a corner: a clear error, nothing changed.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let n = sk(&s).curves.len();
    let e = s.execute("sketch.fillet", &json!({"a": c[0], "b": c[1], "radius": 30})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    let e = s.execute("sketch.chamfer.equal_distance", &json!({"a": c[0], "b": c[1], "distance": 25})).unwrap_err().to_string();
    assert!(e.contains("too large") || e.contains("does not fit") || e.contains("longer"), "{e}");
    assert_eq!(sk(&s).curves.len(), n);
    // A line meeting an arc, and two arcs (a slot's end rounded into a line).
    let mut s = new_sketch();
    let l = curves(&run(&mut s, "sketch.line", json!({"points": [[-20, 10], [0, 10]]})))[0].clone();
    let a = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{l}.end"), "end": [10, 0], "through": [7.0710678, 7.0710678]})))
        [0]
    .clone();
    run(&mut s, "sketch.fillet", json!({"a": l, "b": a, "radius": 2}));
    assert!(solved(&s));
    let mut s = new_sketch();
    let a1 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": [-10, 0], "end": [0, 0], "through": [-5, -5]})))[0].clone();
    let a2 = curves(&run(&mut s, "sketch.arc.three_point", json!({"start": format!("{a1}.end"), "end": [10, 0], "through": [5, 2]})))[0].clone();
    run(&mut s, "sketch.fillet", json!({"a": a1, "b": a2, "radius": 1}));
    assert!(solved(&s));
    // Chamfers keep a closed profile.
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    let c = curves(&r);
    let a0 = area(&s);
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": c[0], "b": c[1], "distance": 4}));
    assert!(solved(&s));
    assert!((area(&s) - (a0 - 8.0)).abs() < 1e-6, "{}", area(&s));
}
