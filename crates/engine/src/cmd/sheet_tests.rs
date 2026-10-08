use std::f64::consts::{FRAC_PI_2, PI};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "MeasureCommand", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

fn flat(s: &mut Session) -> Value {
    run(s, "FusionSheetMetalFlatPatternCmd", json!({}))
}

/// A w x h base flange on XY.
fn base(s: &mut Session, w: f64, h: f64) {
    run(s, "SketchCreate", json!({"plane": "XY", "name": "Base"}));
    run(s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [w, h]}));
    run(s, "SketchStop", json!({}));
    run(s, "FusionSheetMetalFlangeCommand", json!({"sketch": "Base"}));
}

#[test]
fn base_and_edge_flange_follow_the_k_factor() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    assert!(rel(volume(&mut s), 15000.0) < 1e-9);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    // Oracle 68: flat = 60 − 5 + (π/2)(2.5 + 0.44·2.5) + 20 − 5.
    let want = 60.0 - 5.0 + FRAC_PI_2 * (2.5 + 0.44 * 2.5) + 15.0;
    let f = flat(&mut s);
    assert!((f["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6, "{f}");
    assert_eq!(f["bends"][0]["direction"], "up");
    assert!(rel(volume(&mut s), 18972.622) < 1e-4, "{}", volume(&mut s));
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert_eq!(m["total"]["faces"], 14, "{m}");
    // The angle is a parameter: 45° (oracle 70 uses height 15).
    let names = s.doc.features.last().map(|f| f.param_names.clone()).unwrap_or_default();
    run(&mut s, "ChangeParameterCommand", json!({"name": names[0], "expression": "15"}));
    run(&mut s, "ChangeParameterCommand", json!({"name": names[1], "expression": "45 deg"}));
    assert!(rel(volume(&mut s), 18450.777) < 1e-4, "{}", volume(&mut s));
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - 73.685298).abs() < 1e-5);
    // DXF: outline and an up bend line.
    let d = run(&mut s, "sheet.export_dxf", json!({}));
    let text = d["dxf"].as_str().unwrap_or_default();
    assert!(text.contains("BEND_UP") && text.contains("OUTLINE"), "{text}");
    assert!(s.execute("FusionSheetMetalFlangeCommand", &json!({"edges": [[50, 30, 40]], "height": 20})).is_err(), "not on an edge");
}

#[test]
fn rules_are_parameters() {
    let mut s = Session::default();
    run(&mut s, "ChangeParameterCommand", json!({"name": "Thick", "expression": "1.5 mm"}));
    run(&mut s, "FusionSheetMetalRulesCommand", json!({"name": "Custom", "thickness": "Thick", "k_factor": 0.33, "bend_radius": "3 mm"}));
    base(&mut s, 80.0, 50.0);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[40, 50, 1.5], [40, 0, 1.5]], "height": 20}));
    // Oracle 71: 50 − 9 + 2·(π/2)(3 + 0.495) + 2·15.5 = 82.980.
    let want = 50.0 - 2.0 * 4.5 + 2.0 * FRAC_PI_2 * (3.0 + 0.33 * 1.5) + 2.0 * 15.5;
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6);
    assert!(rel(volume(&mut s), 10053.717) < 1e-4, "{}", volume(&mut s));
    // Thicker: everything follows.
    run(&mut s, "ChangeParameterCommand", json!({"name": "Thick", "expression": "2 mm"}));
    let want = 50.0 - 2.0 * 5.0 + 2.0 * FRAC_PI_2 * (3.0 + 0.33 * 2.0) + 2.0 * 15.0;
    assert!((flat(&mut s)["flat_size_mm"][1].as_f64().unwrap_or(0.0) - want).abs() < 1e-6);
    let l = run(&mut s, "sheet.rules", json!({}));
    assert_eq!(l["active"], "Custom", "{l}");
    assert!(s.execute("FusionSheetMetalRulesCommand", &json!({"name": "Bad", "k_factor": 3})).is_err());
}

#[test]
fn contour_flange_hem_and_tray() {
    // Oracle 72.
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": {"origin": [0, 0, 0], "x_dir": [1, 0, 0], "y_dir": [0, 0, -1]}, "name": "Contour"}));
    run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [40, 0], [40, -30], [70, -30]], "ids": ["l1", "l2", "l3"]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"sketch": "Contour", "curves": ["l1", "l2", "l3"], "distance": 50}));
    assert!(rel(volume(&mut s), 12097.622) < 1e-4, "{}", volume(&mut s));
    assert!((flat(&mut s)["flat_size_mm"][0].as_f64().unwrap_or(0.0) - 96.309734).abs() < 1e-5);
    // Oracle 73: flange, then a flat hem of 8.
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    run(&mut s, "FusionSheetMetalHemFlangeCommand", json!({"edges": [[50, 0, 20]], "length": 8}));
    assert!(rel(volume(&mut s), 21334.723) < 1e-4, "{}", volume(&mut s));
    let b = flat(&mut s)["bends"].clone();
    assert!(b.as_array().is_some_and(|a| a.iter().any(|x| (x["angle_deg"].as_f64().unwrap_or(0.0) - 180.0).abs() < 1e-9)), "{b}");
    // A tray: four flanges at once stop at each other's bends.
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[50, 60, 2.5], [100, 30, 2.5], [50, 0, 2.5], [0, 30, 2.5]], "height": 25}));
    let f = flat(&mut s);
    // Oracle 69: 100 − 10 + 2·5.655 + 2·20 = 141.310 across.
    assert!((f["flat_size_mm"][0].as_f64().unwrap_or(0.0) - (90.0 + 2.0 * FRAC_PI_2 * 3.6 + 40.0)).abs() < 1e-6, "{f}");
    let v = volume(&mut s);
    // Each flange runs on past its bend toward the corner until the inner corners are the
    // rule's gap apart (diagonally): 5 − (2.5 + 2.5/√2) = 0.732 at each of 8 ends.
    let section = PI / 4.0 * (25.0 - 6.25) + 2.5 * 20.0;
    let ext = 5.0 - (2.5 + 2.5 / std::f64::consts::SQRT_2);
    let want = 90.0 * 50.0 * 2.5 + section * (2.0 * 90.0 + 2.0 * 50.0) + 8.0 * ext * 2.5 * 20.0;
    assert!(rel(v, want) < 1e-4 && rel(v, 29666.234) < 1e-4, "{v} {want}");
}

#[test]
fn unfold_cut_refold_and_convert() {
    // Oracle 74: two flanges, unfold, a slot across the bend, refold.
    let mut s = Session::default();
    base(&mut s, 80.0, 50.0);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[40, 50, 2.5], [40, 0, 2.5]], "height": 20}));
    let folded = volume(&mut s);
    run(&mut s, "FusionSheetmetalUnfoldCommand", json!({}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    assert!((m["bodies"][0]["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 2.5).abs() < 1e-6, "{m}");
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [35, -12], "p1": [45, 10]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"sketch": "Slot", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    run(&mut s, "sheet.refold", json!({}));
    let v = volume(&mut s);
    assert!(v < folded - 300.0, "{v} {folded}");
    assert!(rel(v, 15800.304) < 1e-4, "{v}");
    assert_eq!(flat(&mut s)["cutouts"].as_array().map(Vec::len), Some(1));
    // Convert a plate.
    let mut s = Session::default();
    run(&mut s, "PrimitiveBox", json!({"length": 100, "width": 60, "height": 2.5, "body_name": "Plate"}));
    run(&mut s, "ConvertToSheetMetalCmd", json!({"body": "Plate", "face": [50, 30, 2.5]}));
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[50, 0, 2.5]], "height": 20}));
    assert!(rel(volume(&mut s), 18972.622) < 1e-4, "{}", volume(&mut s));
    let mut t = Session::default();
    run(&mut t, "PrimitiveCylinder", json!({"radius": 10, "height": 30, "body_name": "Rod"}));
    assert!(t.execute("ConvertToSheetMetalCmd", &json!({"body": "Rod", "face": [0, 0, 30]})).is_err());
}
