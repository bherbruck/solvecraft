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
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn sketch(s: &Session) -> solvecraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

#[test]
fn redundant_constraints_and_dimensions_are_refused_driven_ones_measure() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "infer": true}))["curves"])[0].clone();
    // Already horizontal (inferred).
    let e = s.execute("sketch.constraint.horizontal_vertical", &json!({"line": l})).unwrap_err().to_string();
    assert!(e.contains("over-constrain"), "{e}");
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{l}.start"), "b": "origin"}));
    run(&mut s, "sketch.dimension", json!({"entities": [l], "value": 10}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0, "{si}");
    // A second length on it over-constrains: refused, unless driven.
    assert!(s.execute("sketch.dimension", &json!({"entities": [format!("{l}.start"), format!("{l}.end")]})).is_err());
    let d = run(&mut s, "sketch.dimension", json!({"entities": [format!("{l}.start"), format!("{l}.end")], "driven": true}));
    assert_eq!(d["driven"], true);
    // The driven dimension follows the driving one.
    let pname = si["constraints"].as_array().unwrap().iter().find_map(|c| c["param"].as_str().map(str::to_string)).unwrap();
    run(&mut s, "parameters.change", json!({"name": pname, "expression": "25 mm"}));
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
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, 0], [0, 0]], "infer": true}))["curves"])[0].clone();
    let sp = ids(&run(&mut s, "sketch.spline.control_point", json!({"points": [format!("{l}.end"), [5, 2], [10, 6], [15, 15]]}))["curves"])[0].clone();
    run(&mut s, "sketch.constraint.curvature", json!({"a": l, "b": sp}));
    let sk = sketch(&s);
    let pts: Vec<Vec2> = sk.curves[sk.curve_index(&sp).unwrap()].kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
    // Tangent: first leg along +x; zero curvature: the second leg too (collinear first three).
    assert!((pts[1] - pts[0]).y.abs() < 1e-5, "{pts:?}");
    assert!((pts[2] - pts[1]).cross(pts[1] - pts[0]).abs() < 1e-3, "{pts:?}");

    // Polygon constraint on a rough quadrilateral: becomes a square-ish regular polygon.
    let q = run(&mut s, "sketch.line", json!({"points": [[100, 0], [111, 1], [110, 9], [99, 10]], "closed": true}));
    let ql = ids(&q["curves"]);
    run(&mut s, "sketch.constraint.polygon", json!({"lines": ql}));
    let sk = sketch(&s);
    let lens: Vec<f64> = ql.iter().map(|i| sk.polyline(sk.curve_index(i).unwrap())).map(|p| p[0].dist(p[p.len() - 1])).collect();
    assert!(lens.iter().all(|x| (x - lens[0]).abs() < 1e-6), "{lens:?}");

    // Arc length.
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [0, 50], "start": [10, 50], "end": [0, 60]}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [a], "type": "arc_length", "value": 20}));
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    let len = match sk.segs(ai)[0] {
        solvecraft_geom::Seg2::Arc { radius, sweep, .. } => radius * sweep,
        _ => 0.0,
    };
    assert!((len - 20.0).abs() < 1e-6, "{len}");

    // Linear diameter about a centerline.
    let cl = ids(&run(&mut s, "sketch.line", json!({"points": [[200, 0], [200, 50]]}))["curves"])[0].clone();
    run(&mut s, "sketch.centerline", json!({"curves": [cl]}));
    run(&mut s, "sketch.point", json!({"point": [207, 10], "id": "rim"}));
    let d = run(&mut s, "sketch.dimension", json!({"entities": ["rim", cl], "value": 30}));
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
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [20, 10]}));
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
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0.004], [40.003, 30], [0, 30.002]], "closed": true}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 15], "radius": 5}));
    let r = run(&mut s, "sketch.auto_constrain", json!({}));
    assert_eq!(r["dof"], 0, "{r}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["fully_constrained"], true, "{si}");
    let names: Vec<String> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_string()).collect();
    assert!(names.iter().filter(|n| *n == "Horizontal").count() >= 2, "{names:?}");
    assert!(names.iter().any(|n| n == "DiameterDimension"), "{names:?}");
    // From a datum, and finishing.
    let mut s = new_sketch();
    run(&mut s, "sketch.point", json!({"point": [5, 5], "id": "d"}));
    run(&mut s, "sketch.line", json!({"points": [[10, 10], [20, 10]]}));
    let r = run(&mut s, "sketch.finish_auto_constrain", json!({"datum": "d"}));
    assert!(s.active_sketch.is_none());
    assert!(r["added"].as_array().unwrap().len() >= 3, "{r}");
}

#[test]
fn drawing_infers_tangent_and_perpendicular() {
    let mut s = new_sketch();
    let l1 = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 10]], "infer": true}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("{l1}.end"), [20, 0]], "infer": true}));
    assert_eq!(r["constraints"].as_array().unwrap().len(), 1, "{r}");
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [50, 0], "start": [60, 0], "end": [50, 10]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("{a}.end"), [30, 10]], "infer": true}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    let names: Vec<&str> = si["constraints"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Perpendicular") && names.contains(&"Tangent"), "{names:?} {r}");
}

#[test]
fn smart_constrain_and_driven_toggle() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0.5]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 5], [10, 6]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a]}));
    assert_eq!(r["type"], "Horizontal");
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [a, b]}));
    assert_eq!(r["type"], "Parallel");
    let c1 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50, 0], "radius": 5}))["curves"])[0].clone();
    let c2 = ids(&run(&mut s, "sketch.circle.center", json!({"center": [50.3, 0.2], "radius": 3}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.constrainer", json!({"entities": [c1, c2]}));
    assert_eq!(r["type"], "Concentric");
    // Driven toggle.
    let d = run(&mut s, "sketch.dimension", json!({"entities": [c1], "value": 10}));
    let g = run(&mut s, "sketch.glyphs", json!({}));
    let did = g["glyphs"].as_array().unwrap().iter().find(|x| x["param"] == d["param"]).unwrap()["id"].as_str().unwrap().to_string();
    let r = run(&mut s, "sketch.toggle_driven", json!({"constraint": did}));
    assert_eq!(r["driven"], true);
    assert!(s.doc.param(d["param"].as_str().unwrap()).is_none(), "the parameter goes");
    let r = run(&mut s, "sketch.toggle_driven", json!({"constraint": did}));
    assert_eq!(r["driven"], false);
}

#[test]
fn drawing_from_a_line_midpoint_keeps_it_there() {
    let mut s = new_sketch();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("mid:{l}"), [10, 15]]}));
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["name"] == "MidPoint"), "{si}");
    // Lengthen the first line: the second one's start follows the middle.
    run(&mut s, "sketch.move_point", json!({"point": format!("{l}.end"), "to": [40, 0]}));
    let sk = sketch(&s);
    let n = ids(&r["curves"])[0].clone();
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    let a = sk.resolve_point(&format!("{l}.start")).and_then(|i| sk.point(i)).unwrap();
    let b = sk.resolve_point(&format!("{l}.end")).and_then(|i| sk.point(i)).unwrap();
    assert!(st.dist((a + b) * 0.5) < 1e-7, "{st:?}");
}

#[test]
fn drawing_onto_a_curve_keeps_the_point_on_it() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.line", json!({"points": [format!("on:{c}:7.2,7.1"), [30, 30]]}));
    let n = ids(&r["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [c], "value": 16}));
    let sk = sketch(&s);
    let st = sk.resolve_point(&format!("{n}.start")).and_then(|i| sk.point(i)).unwrap();
    let cc = sk.center(sk.curve_index(&c).unwrap()).unwrap();
    assert!((st.dist(cc) - 8.0).abs() < 1e-7, "{st:?} {cc:?}");
}
