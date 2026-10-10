//! A pattern of a suppressed feature has nothing to copy (#42): its copies go too, and the
//! pattern says so.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0)
}

#[test]
fn suppressing_a_patterned_feature_removes_its_copies() {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(include_str!("../../../examples/acceptance/sensor_enclosure.json")).unwrap();
    s.run_script(&script).unwrap();
    let full = volume(&s, "Case");
    run(&mut s, "timeline.suppress", json!({"feature": "Standoff", "suppressed": true}));
    let without = volume(&s, "Case");
    // All four standoffs go (each 116.8 mm³), not just the seed.
    assert!((full - without - 4.0 * 116.8).abs() < 2.0, "{full} → {without}");
    let pattern = s.doc.find_feature("RectangularPattern1").unwrap().id;
    let r = s.model.result(pattern).unwrap();
    assert!(r.error.is_none() && r.warning.as_deref().is_some_and(|w| w.contains("Standoff")), "{r:?}");
    // Back on: the four standoffs return.
    run(&mut s, "timeline.suppress", json!({"feature": "Standoff", "suppressed": false}));
    assert!((volume(&s, "Case") - full).abs() < 1e-6 * full);
}
