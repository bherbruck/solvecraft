use serde_json::{Value, json};
use solvecraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    s
}

fn sketch(s: &Session) -> solvecraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

#[test]
fn redundant_constraints_and_dimensions_are_refused_driven_ones_measure() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [10, 0]], "infer": true}))["curves"])[0].clone();
    // Already horizontal (inferred).
    let e = s.execute("ConstraintHorizontalVertical", &json!({"line": l})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    run(&mut s, "ConstraintCoincident", json!({"a": format!("{l}.start"), "b": "origin"}));
    run(&mut s, "SketchDimension", json!({"entities": [l], "value": 10}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0, "{si}");
    // A second length on it over-constrains: refused, unless driven.
    assert!(s.execute("SketchDimension", &json!({"entities": [format!("{l}.start"), format!("{l}.end")]})).is_err());
    let d = run(&mut s, "SketchDimension", json!({"entities": [format!("{l}.start"), format!("{l}.end")], "driven": true}));
    assert_eq!(d["driven"], true);
    // The driven dimension follows the driving one.
    let pname = si["constraints"].as_array().unwrap().iter().find_map(|c| c["param"].as_str().map(str::to_string)).unwrap();
    run(&mut s, "ChangeParameterCommand", json!({"name": pname, "expression": "25 mm"}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let driven = g["glyphs"].as_array().unwrap().iter().find(|x| x["driven"] == true).unwrap().clone();
    assert!((driven["value"].as_f64().unwrap() - 25.0).abs() < 1e-7, "{driven}");
    assert_eq!(g["fully_constrained"], true);
}

#[test]
fn curvature_polygon_arc_length_linear_diameter() {
    let mut s = new_sketch();
    // A horizontal line ending where a control-point spline starts: G2 keeps the spline's
    // start straight (zero curvature) and tangent.
    let l = ids(&run(&mut s, "DrawPolyline", json!({"points": [[-10, 0], [0, 0]], "infer": true}))["curves"])[0].clone();
    let sp = ids(&run(&mut s, "DrawCVMSpline3D", json!({"points": [format!("{l}.end"), [5, 2], [10, 6], [15, 15]]}))["curves"])[0].clone();
    run(&mut s, "ConstraintSmooth", json!({"a": l, "b": sp}));
    let sk = sketch(&s);
    let pts: Vec<Vec2> = sk.curves[sk.curve_index(&sp).unwrap()].kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
    // Tangent: first leg along +x; zero curvature: the second leg too (collinear first three).
    assert!((pts[1] - pts[0]).y.abs() < 1e-5, "{pts:?}");
    assert!((pts[2] - pts[1]).cross(pts[1] - pts[0]).abs() < 1e-3, "{pts:?}");

    // Polygon constraint on a rough quadrilateral: becomes a square-ish regular polygon.
    let q = run(&mut s, "DrawPolyline", json!({"points": [[100, 0], [111, 1], [110, 9], [99, 10]], "closed": true}));
    let ql = ids(&q["curves"]);
    run(&mut s, "SketchPolygonConstraintCmd", json!({"lines": ql}));
    let sk = sketch(&s);
    let lens: Vec<f64> = ql.iter().map(|i| sk.polyline(sk.curve_index(i).unwrap())).map(|p| p[0].dist(p[p.len() - 1])).collect();
    assert!(lens.iter().all(|x| (x - lens[0]).abs() < 1e-6), "{lens:?}");

    // Arc length.
    let a = ids(&run(&mut s, "ArcCenterTwoPoint", json!({"center": [0, 50], "start": [10, 50], "end": [0, 60]}))["curves"])[0].clone();
    run(&mut s, "SketchDimension", json!({"entities": [a], "type": "arc_length", "value": 20}));
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    let len = match sk.segs(ai)[0] {
        solvecraft_geom::Seg2::Arc { radius, sweep, .. } => radius * sweep,
        _ => 0.0,
    };
    assert!((len - 20.0).abs() < 1e-6, "{len}");

    // Linear diameter about a centerline.
    let cl = ids(&run(&mut s, "DrawPolyline", json!({"points": [[200, 0], [200, 50]]}))["curves"])[0].clone();
    run(&mut s, "sketch.centerline", json!({"curves": [cl]}));
    run(&mut s, "DrawPoint", json!({"point": [207, 10], "id": "rim"}));
    let d = run(&mut s, "SketchDimension", json!({"entities": ["rim", cl], "value": 30}));
    assert_eq!(d["type"], "LinearDiameterDimension", "{d}");
    let sk = sketch(&s);
    let rim = sk.point(sk.point_index("rim").unwrap()).unwrap();
    let line = sk.polyline(sk.curve_index(&cl).unwrap());
    let (a, b) = (line[0], line[line.len() - 1]);
    let dist = (b - a).normalized().unwrap().cross(rim - a).abs();
    assert!((dist - 15.0).abs() < 1e-6, "{rim:?} {dist}");
}

#[test]
fn glyphs_and_snaps() {
    let mut s = new_sketch();
    let r = run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [20, 10]}));
    let l = ids(&r["curves"]);
    let g = run(&mut s, "sketch.glyphs", json!({}));
    assert_eq!(g["glyphs"].as_array().unwrap().len(), 4);
    assert!(g["glyphs"][0]["at"].is_array());
    let sn = run(&mut s, "sketch.snap", json!({"at": [20.3, 0.2], "radius": 1}));
    assert_eq!(sn["snaps"][0]["type"], "coincident", "{sn}");
    let sn = run(&mut s, "sketch.snap", json!({"at": [10.2, 0.3], "radius": 1}));
    assert_eq!(sn["snaps"][0]["type"], "midpoint", "{sn}");
    assert_eq!(sn["snaps"][0]["curve"], json!(l[0]));
    let sn = run(&mut s, "sketch.snap", json!({"at": [40, 10.4], "from": [30, 10], "radius": 1}));
    assert!(sn["snaps"].as_array().unwrap().iter().any(|x| x["type"] == "horizontal"), "{sn}");
}

#[test]
fn auto_constrain_fully_constrains_a_rough_sketch() {
    let mut s = new_sketch();
    // A slightly crooked closed rectangle and a circle inside.
    run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [40, 0.004], [40.003, 30], [0, 30.002]], "closed": true}));
    run(&mut s, "CircleCenterRadius", json!({"center": [20, 15], "radius": 5}));
    let r = run(&mut s, "SketchAutoConstraintAndDimCmd", json!({}));
    assert_eq!(r["dof"], 0, "{r}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["fully_constrained"], true, "{si}");
    let names: Vec<String> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    assert!(names.iter().filter(|n| *n == "Horizontal").count() >= 2, "{names:?}");
    assert!(names.iter().any(|n| n == "DiameterDimension"), "{names:?}");
    // From a datum, and finishing.
    let mut s = new_sketch();
    run(&mut s, "DrawPoint", json!({"point": [5, 5], "id": "d"}));
    run(&mut s, "DrawPolyline", json!({"points": [[10, 10], [20, 10]]}));
    let r = run(&mut s, "SketchAutoConstrainAndFinish", json!({"datum": "d"}));
    assert!(s.active_sketch.is_none());
    assert!(r["added"].as_array().unwrap().len() >= 3, "{r}");
}

#[test]
fn drawing_infers_tangent_and_perpendicular() {
    let mut s = new_sketch();
    let l1 = ids(&run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [10, 10]], "infer": true}))["curves"])[0].clone();
    let r = run(&mut s, "DrawPolyline", json!({"points": [format!("{l1}.end"), [20, 0]], "infer": true}));
    assert_eq!(r["constraints"].as_array().unwrap().len(), 1, "{r}");
    let a = ids(&run(&mut s, "ArcCenterTwoPoint", json!({"center": [50, 0], "start": [60, 0], "end": [50, 10]}))["curves"])[0].clone();
    let r = run(&mut s, "DrawPolyline", json!({"points": [format!("{a}.end"), [30, 10]], "infer": true}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    let names: Vec<&str> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Perpendicular") && names.contains(&"Tangent"), "{names:?} {r}");
}
