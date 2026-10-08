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

/// A sketch line across a 100 x 60 plate's top face at x = 40.
fn fold_line(s: &mut Session) {
    run(s, "SketchCreate", json!({"plane": "XY", "offset": 2.5, "name": "FoldLine"}));
    run(s, "DrawPolyline", json!({"points": [[40, 0], [40, 60]], "ids": ["fl"]}));
    run(s, "SketchStop", json!({}));
}

fn bbox(s: &mut Session) -> Value {
    run(s, "MeasureCommand", json!({}))["bodies"][0]["bbox"].clone()
}

#[test]
fn fold_takes_the_bend_out_of_the_flat_sheet() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    fold_line(&mut s);
    run(&mut s, "SheetMetalFoldCmd", json!({"sketch": "FoldLine", "curve": "fl"}));
    // The flat pattern keeps its size; the bend's centre line is the sketch line.
    let f = flat(&mut s);
    assert!((f["flat_size_mm"][0].as_f64().unwrap_or(0.0) - 100.0).abs() < 1e-6, "{f}");
    assert!((f["flat_size_mm"][1].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-6, "{f}");
    assert!((f["bends"][0]["allowance"].as_f64().unwrap_or(0.0) - ba).abs() < 1e-9, "{f}");
    // Folded: the zone's flat volume BA·T·w becomes the bend's θ·T·(R + T/2)·w.
    let want = 15000.0 - ba * t * 60.0 + FRAC_PI_2 * t * (r + t / 2.0) * 60.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    // The smaller side (x < 40) stands up: outer face at the zone start less R + T, leg 40 − BA/2
    // tall above the bend.
    let b = bbox(&mut s);
    assert!((b["min"][0].as_f64().unwrap_or(0.0) - (40.0 + ba / 2.0 - r - t)).abs() < 1e-6, "{b}");
    assert!((b["max"][2].as_f64().unwrap_or(0.0) - (t + r + 40.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    assert!((b["max"][0].as_f64().unwrap_or(0.0) - 100.0).abs() < 1e-6, "{b}");
}

#[test]
fn fold_positions_fixed_side_and_flip() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let case = |p: Value| {
        let mut s = Session::default();
        base(&mut s, 100.0, 60.0);
        fold_line(&mut s);
        let mut q = json!({"sketch": "FoldLine", "curve": "fl"});
        for (k, v) in p.as_object().into_iter().flatten() {
            q[k] = v.clone();
        }
        run(&mut s, "SheetMetalFoldCmd", q);
        bbox(&mut s)
    };
    let x0 = |b: &Value| b["min"][0].as_f64().unwrap_or(f64::NAN);
    // Start: the bend starts at the line (on the fixed side), so the folded outer face is R + T
    // short of it. End: the bend ends there. Mould: the outer face lands on the line.
    assert!((x0(&case(json!({"position": "start"}))) - (40.0 - r - t)).abs() < 1e-6);
    assert!((x0(&case(json!({"position": "end"}))) - (40.0 + ba - r - t)).abs() < 1e-6);
    assert!((x0(&case(json!({"position": "mould"}))) - 40.0).abs() < 1e-6);
    // Keep the small side: the large one (x > 40) stands up instead.
    let b = case(json!({"fixed": [10, 30, 2.5]}));
    assert!((b["max"][0].as_f64().unwrap_or(0.0) - (40.0 - ba / 2.0 + r + t)).abs() < 1e-6, "{b}");
    assert!((b["max"][2].as_f64().unwrap_or(0.0) - (t + r + 60.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    // Flipped: folds down, below the sheet.
    let b = case(json!({"flip": true}));
    assert!((b["min"][2].as_f64().unwrap_or(0.0) + (r + 40.0 - ba / 2.0)).abs() < 1e-6, "{b}");
    // A 45° fold: the leg leans.
    let b = case(json!({"angle": "45 deg"}));
    assert!(b["max"][2].as_f64().unwrap_or(0.0) < t + r + 40.0 - ba / 2.0);
}

#[test]
fn fold_carries_flanges_on_the_moving_side() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    // A flange on the left edge, then a fold between it and the rest.
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[0, 30, 2.5]], "height": 20}));
    let v0 = volume(&mut s);
    fold_line(&mut s);
    run(&mut s, "SheetMetalFoldCmd", json!({"sketch": "FoldLine", "curve": "fl", "fixed": [90, 30, 2.5]}));
    // The flange rode along: it is above the plate now, nothing left at z < 0.
    let b = bbox(&mut s);
    assert!(b["min"][2].as_f64().unwrap_or(-1.0) > -1e-6, "{b}");
    let ba = FRAC_PI_2 * (2.5 + 0.44 * 2.5);
    let want = v0 - ba * 2.5 * 60.0 + FRAC_PI_2 * 2.5 * (2.5 + 1.25) * 60.0;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    assert!(flat(&mut s)["bends"].as_array().map(Vec::len) == Some(2));
    // Errors: off the sheet, through the flange's bend.
    let mut e = Session::default();
    base(&mut e, 100.0, 60.0);
    assert!(e.execute("SheetMetalFoldCmd", &json!({"points": [[40, 0, 30], [40, 60, 30]]})).is_err());
    assert!(e.execute("SheetMetalFoldCmd", &json!({"points": [[40, 0, 2.5], [40, 60, 2.5]], "position": "sideways"})).is_err());
    assert!(e.execute("SheetMetalFoldCmd", &json!({"points": [[0.5, 0, 2.5], [0.5, 60, 2.5]]})).is_err(), "no material past the bend");
}

#[test]
fn fold_on_a_flange_panel() {
    let (t, r, k) = (2.5, 2.5, 0.44);
    let ba = FRAC_PI_2 * (r + k * t);
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[50, 0, 2.5]], "height": 30}));
    let flat0 = flat(&mut s)["flat_size_mm"].clone();
    let v0 = volume(&mut s);
    // The flange stands up at the plate's y = 0 edge: its outer face is the body's min y.
    let y = bbox(&mut s)["min"][1].as_f64().unwrap_or(f64::NAN);
    run(&mut s, "SheetMetalFoldCmd", json!({"points": [[10, y, 18], [90, y, 18]]}));
    let f = flat(&mut s);
    for i in 0..2 {
        assert!(
            (f["flat_size_mm"][i].as_f64().unwrap_or(0.0) - flat0[i].as_f64().unwrap_or(1.0)).abs() < 1e-6,
            "the flat pattern keeps its size: {f}"
        );
    }
    assert_eq!(f["bends"].as_array().map(Vec::len), Some(2));
    let len = f["bends"][1]["length"].as_f64().unwrap_or(0.0);
    assert!(len > 90.0, "{f}");
    let want = v0 - ba * t * len + FRAC_PI_2 * t * (r + t / 2.0) * len;
    assert!(rel(volume(&mut s), want) < 1e-4, "{} {want}", volume(&mut s));
    // The top part now leans over the plate (folded up, toward its top face).
    let b = bbox(&mut s);
    assert!(b["max"][2].as_f64().unwrap_or(99.0) < 25.0, "{b}");
    // Folding across the flange's own bend is refused.
    assert!(s.execute("SheetMetalFoldCmd", &json!({"points": [[50, y, 2], [50, y, 25]]})).is_err());
}

#[test]
fn fold_through_cut_outs() {
    let mut s = Session::default();
    base(&mut s, 100.0, 60.0);
    // A slot across the fold line (edges along and across the bend).
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Slot"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [30, 20], "p1": [50, 40]}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"sketch": "Slot", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    let mut plain = Session::default();
    base(&mut plain, 100.0, 60.0);
    fold_line(&mut plain);
    run(&mut plain, "SheetMetalFoldCmd", json!({"sketch": "FoldLine", "curve": "fl"}));
    fold_line(&mut s);
    run(&mut s, "SheetMetalFoldCmd", json!({"sketch": "FoldLine", "curve": "fl"}));
    let f = flat(&mut s);
    assert_eq!(f["cutouts"].as_array().map(Vec::len), Some(1), "{f}");
    let (v, vp) = (volume(&mut s), volume(&mut plain));
    // The slot takes 20 x 20 of the flat sheet; across the bend it removes the bend's share.
    assert!(v < vp - 900.0 && v > vp - 1100.0, "{v} {vp}");
    run(&mut s, "FusionSheetmetalUnfoldCommand", json!({}));
    assert!((volume(&mut s) - (15000.0 - 1000.0)).abs() < 1e-3, "{}", volume(&mut s));
    // A round hole across the bend is not supported yet, and says so.
    let mut c = Session::default();
    base(&mut c, 100.0, 60.0);
    run(&mut c, "SketchCreate", json!({"plane": "XY", "name": "Hole"}));
    run(&mut c, "CircleCenterRadius", json!({"center": [40, 30], "radius": 5}));
    run(&mut c, "SketchStop", json!({}));
    run(&mut c, "Extrude", json!({"sketch": "Hole", "through_all": true, "operation": "cut", "direction": "symmetric"}));
    fold_line(&mut c);
    let e = c.execute("SheetMetalFoldCmd", &json!({"sketch": "FoldLine", "curve": "fl"})).map(|_| ()).unwrap_err().to_string();
    assert!(e.contains("not supported yet"), "{e}");
}

#[test]
fn flange_edges_are_named_and_follow_the_base_sketch() {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY", "name": "Base"}));
    run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [100, 60]}));
    let d = run(&mut s, "SketchDimension", json!({"entities": ["l1"], "value": 100}));
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"sketch": "Base"}));
    run(&mut s, "FusionSheetMetalFlangeCommand", json!({"edges": [[100, 30, 2.5]], "height": 20, "name": "Side"}));
    let side = s.doc.find_feature("Side").cloned().unwrap();
    assert_eq!(side.edge_names, vec!["base:l2:top".to_string()]);
    let v0 = volume(&mut s);
    // Narrower base: the flange stays on the right edge (now at x = 40).
    let name = d["param"].as_str().or_else(|| d["name"].as_str()).map(str::to_string).unwrap_or_else(|| "d1".into());
    run(&mut s, "ChangeParameterCommand", json!({"name": name, "expression": "40"}));
    let b = run(&mut s, "MeasureCommand", json!({}))["bodies"][0]["bbox"].clone();
    let w = b["max"][0].as_f64().unwrap_or(0.0) - b["min"][0].as_f64().unwrap_or(0.0);
    assert!((w - 40.0).abs() < 1e-3 && b["max"][2].as_f64().unwrap_or(0.0) > 19.0, "{b}");
    assert!(volume(&mut s) < v0);
    let id = side.id;
    assert_eq!(s.model.result(id).and_then(|r| r.warning.clone()), None);
    // A hem on the flange's far edge is named after the flange.
    let top = b["max"][2].as_f64().unwrap_or(0.0);
    let x = b["max"][0].as_f64().unwrap_or(0.0);
    run(&mut s, "FusionSheetMetalHemFlangeCommand", json!({"edges": [[x, 30, top]], "length": 6, "name": "H"}));
    let h = s.doc.find_feature("H").cloned().unwrap();
    assert!(h.edge_names[0].starts_with(&format!("F{id}.0:far")), "{:?}", h.edge_names);
}
