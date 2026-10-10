//! Booleans that failed, panicked or hung, replayed from the scripts in
//! `examples/regressions/` (each script's description says what went wrong).

use serde_json::{Value, json};

use crate::Session;

fn replay(script: &str) -> Session {
    let mut s = Session::default();
    let script: Value = serde_json::from_str(script).unwrap();
    s.run_script(&script).unwrap();
    s
}

fn volume(s: &Session, body: &str) -> f64 {
    s.model.state().body(body).map(|b| solvecraft_kernel::measure(&b.body).unwrap().volume).unwrap_or(0.0)
}

/// Slab less a 557.68 × 100 × 120 pocket and the loft below its floor (556.68 × 99 down 4 mm to
/// 80 × 80; prismatoid: h/6 · (A_top + A_bottom + 4 A_mid)).
fn slab_pocket_loft() -> (f64, f64) {
    let slab = 900.0 * 480.0 * 160.0;
    let loft = 4.0 / 6.0 * (556.68 * 99.0 + 80.0 * 80.0 + 4.0 * (556.68 + 80.0) / 2.0 * (99.0 + 80.0) / 2.0);
    (slab - 557.68 * 100.0 * 120.0, loft)
}

#[test]
fn shallow_loft_cut_from_the_top_face() {
    let s = replay(include_str!("../../../examples/regressions/shallow_loft_cut_from_top_face.json"));
    let (_, loft) = slab_pocket_loft();
    let want = 900.0 * 480.0 * 160.0 - loft;
    let got = volume(&s, "Body1");
    assert!((got - want).abs() < 1e-9 * want, "{got} vs {want}");
}

#[test]
fn oblique_cut_through_a_small_arc_matches_a_split() {
    let scripts = [
        include_str!("../../../examples/regressions/oblique_cut_of_cylinder.json"),
        include_str!("../../../examples/regressions/oblique_cut_of_cylinder_hang.json"),
    ];
    // The same bar split by the prism's tilted top: the piece above it is the cut's result.
    let mut split = Session::default();
    let script: Value = serde_json::from_str(scripts[0]).unwrap();
    let steps = script.get("commands").and_then(Value::as_array).unwrap();
    split.run_script(&Value::Array(steps[..5].to_vec())).unwrap();
    let whole = volume(&split, "Tool");
    split.execute("solid.split_body", &json!({"body": "Tool", "plane": {"origin": [0, -472, 853.91], "normal": [0, 15.44, 40]}})).unwrap();
    let above = volume(&split, "Tool");
    assert!(above < whole - 10.0, "{above} of {whole}");
    for script in scripts {
        let s = replay(script);
        let got = volume(&s, "Tool");
        assert!((got - above).abs() < 1e-6 * above, "{got} vs {above}");
    }
}
