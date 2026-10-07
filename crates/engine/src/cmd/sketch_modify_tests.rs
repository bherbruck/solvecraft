use serde_json::{Value, json};
use solvecraft_geom::Vec2;
use solvecraft_sketch::CurveKind;

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

fn sketch(s: &Session) -> solvecraft_sketch::Sketch {
    let id = s.active_sketch.unwrap();
    s.model.state().sketch(id).unwrap().sketch.clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    s
}

fn profiles(s: &Session) -> Vec<f64> {
    let id = s.active_sketch.unwrap();
    let mut a: Vec<f64> = s.model.state().sketch(id).unwrap().profiles.iter().map(|p| p.area).collect();
    a.sort_by(|x, y| x.total_cmp(y));
    a
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn trim_extend_break() {
    let mut s = new_sketch();
    // A cross: horizontal line crossed by a vertical one.
    let h = ids(&run(&mut s, "DrawPolyline", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let v = ids(&run(&mut s, "DrawPolyline", json!({"points": [[10, -5], [10, 5]]}))["curves"])[0].clone();
    // Trim the right half of the horizontal line.
    run(&mut s, "TrimSketchCmd", json!({"curve": h, "at": [15, 0]}));
    let sk = sketch(&s);
    let sh = sk.shape(sk.curve_index(&h).unwrap()).unwrap();
    assert!(
        matches!(sh, solvecraft_sketch::Shape::Line { a, b } if a.dist(Vec2::new(0.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, 0.0)) < 1e-9),
        "{sh:?}"
    );
    // Trim the lower half of the vertical line: the original id survives on the upper piece.
    run(&mut s, "TrimSketchCmd", json!({"curve": v, "at": [10, -3]}));
    let sk = sketch(&s);
    assert_eq!(sk.curves.len(), 2);
    // Extend: a short line reaching toward the vertical one.
    let e = ids(&run(&mut s, "DrawPolyline", json!({"points": [[0, 3], [4, 3]]}))["curves"])[0].clone();
    run(&mut s, "ExtendSketchCmd", json!({"curve": e, "at": [4, 3]}));
    let sk = sketch(&s);
    let end = sk.resolve_point(&format!("{e}.end")).and_then(|i| sk.point(i)).unwrap();
    assert!(end.dist(Vec2::new(10.0, 3.0)) < 1e-7, "{end:?}");
    // Break the horizontal line where... nothing crosses it inside: error.
    assert!(s.execute("BreakSketchCmd", &json!({"curve": h, "at": [5, 0]})).is_err());
    // Break the vertical at the extended line: two pieces.
    let r = run(&mut s, "BreakSketchCmd", json!({"curve": v, "at": [10, 1]}));
    assert_eq!(ids(&r["curves"]).len(), 2, "{r}");
    // A lone curve trims away entirely.
    let lone = ids(&run(&mut s, "DrawPolyline", json!({"points": [[50, 50], [60, 50]]}))["curves"])[0].clone();
    run(&mut s, "TrimSketchCmd", json!({"curve": lone, "at": [55, 50]}));
    assert!(sketch(&s).curve_index(&lone).is_none());
}

#[test]
fn trim_circle_makes_an_arc_and_a_profile() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "CircleCenterRadius", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "DrawPolyline", json!({"points": [[-20, 0], [20, 0]]}));
    run(&mut s, "TrimSketchCmd", json!({"curve": c, "at": [0, -10]}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(matches!(sk.curves[ci].kind, CurveKind::Arc { .. }));
    // Upper half disc closes against the line.
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    assert!(close(a[0], std::f64::consts::PI * 50.0), "{a:?}");
}

#[test]
fn fillet_and_chamfers() {
    let mut s = new_sketch();
    let r = run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Pin the rectangle so a dimension change only moves the fillet.
    run(&mut s, "ConstraintFix", json!({"entity": l[2]}));
    run(&mut s, "ConstraintFix", json!({"entity": l[3]}));
    let f = run(&mut s, "FilletSketchCmd", json!({"a": l[0], "b": l[1], "radius": 5}));
    assert!(f["param"].is_string());
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (25.0 - std::f64::consts::PI * 25.0 / 4.0)), "{a:?}");
    // The fillet radius is a dimension: change it.
    let pname = f["param"].as_str().unwrap().to_string();
    run(&mut s, "ChangeParameterCommand", json!({"name": pname, "expression": "8 mm"}));
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (64.0 - std::f64::consts::PI * 64.0 / 4.0)), "{a:?}");
    run(&mut s, "ChamferSketchEqualDistance", json!({"a": l[2], "b": l[3], "distance": 4}));
    let a2 = profiles(&s)[0];
    assert!(close(a[0] - a2, 8.0), "{a2}");
    run(&mut s, "ChamferSketchDistanceDistance", json!({"a": l[1], "b": l[2], "distance": 2, "distance2": 6}));
    let a3 = profiles(&s)[0];
    assert!(close(a2 - a3, 6.0), "{a3}");
    // Distance-angle at the last corner (origin, 90 degrees): 45 deg gives an isosceles cut.
    run(&mut s, "ChamferSketchDistanceAngle", json!({"a": l[3], "b": l[0], "distance": 3, "angle": 45}));
    let a4 = profiles(&s)[0];
    assert!(close(a3 - a4, 4.5), "{a4}");
    assert!(s.execute("FilletSketchCmd", &json!({"a": l[0], "b": l[2], "radius": 1})).is_err(), "no shared corner");
}

#[test]
fn offset_chain_mirror_patterns_move_scale() {
    let mut s = new_sketch();
    let r = run(&mut s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Outward offset by 2 (toward a point outside).
    let o = run(&mut s, "Offset", json!({"curves": [l[0]], "distance": 2, "side": [20, -10]}));
    assert_eq!(ids(&o["curves"]).len(), 4, "{o}");
    let a = profiles(&s);
    assert!(close(a[1], 44.0 * 34.0 - 1200.0) || close(a.iter().sum::<f64>(), 44.0 * 34.0), "{a:?}");
    // Circle offset inward.
    let c = ids(&run(&mut s, "CircleCenterRadius", json!({"center": [100, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "Offset", json!({"curves": [c], "distance": 3, "side": [100, 0]}));
    // Mirror a circle about a vertical line.
    let m = ids(&run(&mut s, "DrawPolyline", json!({"points": [[120, -50], [120, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "MirrorSketchCommand", json!({"entities": [c], "line": m}));
    let mc = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    assert!(sk.center(sk.curve_index(&mc).unwrap()).unwrap().dist(Vec2::new(140.0, 0.0)) < 1e-7);
    // Circular pattern of the mirrored circle: 4 around (120, 0).
    let r = run(&mut s, "CircularSketchPatternCommand", json!({"entities": [mc], "center": [120, 0], "count": 4}));
    assert_eq!(ids(&r["curves"]).len(), 3);
    let sk = sketch(&s);
    let c2 = sk.center(sk.curve_index(&ids(&r["curves"])[0]).unwrap()).unwrap();
    assert!(c2.dist(Vec2::new(120.0, 20.0)) < 1e-7, "{c2:?}");
    // Rectangular pattern 3 x 2.
    let small = ids(&run(&mut s, "CircleCenterRadius", json!({"center": [0, 100], "radius": 1}))["curves"])[0].clone();
    let r = run(&mut s, "RectangularSketchPatternCommand", json!({"entities": [small], "count": 3, "spacing": 5, "count2": 2, "spacing2": 4}));
    assert_eq!(ids(&r["curves"]).len(), 5);
    // Move and scale.
    run(&mut s, "sketch.move", json!({"entities": [small], "translate": [0, 10]}));
    run(&mut s, "SketchScaleCmd", json!({"entities": [small], "base": [0, 110], "factor": 3}));
    let sk = sketch(&s);
    let ci = sk.curve_index(&small).unwrap();
    assert!(sk.center(ci).unwrap().dist(Vec2::new(0.0, 110.0)) < 1e-7);
    assert!(close(sk.radius(ci).unwrap(), 3.0));
    let r = run(&mut s, "sketch.move", json!({"entities": [small], "angle": 90, "center": [0, 0], "copy": true}));
    let cp = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    assert!(sk.center(sk.curve_index(&cp).unwrap()).unwrap().dist(Vec2::new(-110.0, 0.0)) < 1e-7);
}

#[test]
fn new_creation_tools() {
    let mut s = new_sketch();
    let r = run(&mut s, "SketchMidpointLine", json!({"mid": [0, 0], "end": [10, 5]}));
    let l = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let start = sk.resolve_point(&format!("{l}.start")).and_then(|i| sk.point(i)).unwrap();
    assert!(start.dist(Vec2::new(-10.0, -5.0)) < 1e-9);
    // Tangent arc off the end of a horizontal line.
    let h = ids(&run(&mut s, "DrawPolyline", json!({"points": [[0, 20], [10, 20]]}))["curves"])[0].clone();
    let r = run(&mut s, "ArcTangent", json!({"start": format!("{h}.end"), "end": [20, 30]}));
    let a = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(10.0, 30.0)) < 1e-7, "{:?}", sk.center(ai));
    assert!(close(sk.radius(ai).unwrap(), 10.0));
    // 2-tangent circle in the corner of two lines.
    let x = ids(&run(&mut s, "DrawPolyline", json!({"points": [[100, 0], [150, 0]]}))["curves"])[0].clone();
    let y = ids(&run(&mut s, "DrawPolyline", json!({"points": [[100, 0], [100, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "CircleTanTanRadius", json!({"curves": [x, y], "radius": 5, "near": [106, 104 - 100]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(sk.center(ci).unwrap().dist(Vec2::new(105.0, 5.0)) < 1e-6, "{:?}", sk.center(ci));
    // 3-tangent circle inside a triangle: the incircle of the 3-4-5 triangle (r = 1).
    let t = run(&mut s, "DrawPolyline", json!({"points": [[200, 0], [203, 0], [200, 4]], "closed": true}));
    let tl = ids(&t["curves"]);
    let r = run(&mut s, "CircleThreeTangent", json!({"curves": tl, "near": [201, 1.2]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(close(sk.radius(ci).unwrap(), 1.0), "{:?}", sk.radius(ci));
    assert!(sk.center(ci).unwrap().dist(Vec2::new(201.0, 1.0)) < 1e-6);
    // Slots.
    let before = profiles(&s).len();
    run(&mut s, "ShapeSlotCenterPoint", json!({"center": [0, -100], "end": [10, -100], "width": 4}));
    run(&mut s, "ShapeArcSlotThreePoint", json!({"start": [100, -100], "through": [110, -90], "end": [120, -100], "width": 2}));
    run(&mut s, "ShapeArcSlotCenterTwoPoint", json!({"center": [200, -100], "start": [210, -100], "end": [200, -90], "width": 2}));
    let a = profiles(&s);
    assert_eq!(a.len(), before + 3, "{a:?}");
    let pi = std::f64::consts::PI;
    assert!(a.iter().any(|x| close(*x, 20.0 * 4.0 + pi * 4.0)), "{a:?}");
    // Half-circle arc slot: centre arc length pi*10, width 2, plus a full cap circle.
    assert!(a.iter().any(|x| close(*x, pi * 10.0 * 2.0 + pi)), "{a:?}");
    assert!(a.iter().any(|x| close(*x, pi * 5.0 * 2.0 + pi)), "{a:?}");
}

#[test]
fn ellipse_splines_conic() {
    let pi = std::f64::consts::PI;
    let mut s = new_sketch();
    let e = run(&mut s, "CircleElipse", json!({"center": [0, 0], "major": [10, 0], "minor": [0, 4]}));
    let eid = ids(&e["curves"])[0].clone();
    let a = profiles(&s);
    assert!((a[0] - pi * 40.0).abs() / (pi * 40.0) < 2e-3, "{a:?}");
    // A point constrained onto the ellipse lands on it.
    run(&mut s, "DrawPoint", json!({"point": [3, 5], "id": "q"}));
    run(&mut s, "ConstraintCoincident", json!({"a": "q", "b": eid}));
    let sk = sketch(&s);
    let q = sk.point(sk.point_index("q").unwrap()).unwrap();
    let poly = sk.polyline(sk.curve_index(&eid).unwrap());
    let d = poly.windows(2).map(|w| {
        let (a, b) = (w[0], w[1]);
        let t = ((q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
        q.dist(a + (b - a) * t)
    });
    assert!(d.fold(f64::INFINITY, f64::min) < 1e-6, "{q:?}");
    // Fit spline closed by a line: one profile.
    let sp = run(&mut s, "DrawSpline", json!({"points": [[100, 0], [110, 8], [120, 3], [130, 0]]}));
    let sid = ids(&sp["curves"])[0].clone();
    run(&mut s, "DrawPolyline", json!({"points": [format!("{sid}.end"), format!("{sid}.start")]}));
    assert_eq!(profiles(&s).len(), 2);
    // Moving a fit point reshapes the spline (it is a sketch point).
    let sk = sketch(&s);
    let si = sk.curve_index(&sid).unwrap();
    let pts = sk.curves[si].kind.point_ids();
    let pid = sk.points[pts[1]].id.clone();
    run(&mut s, "sketch.move_point", json!({"point": pid, "to": [110, 12]}));
    let sk = sketch(&s);
    assert!(sk.polyline(si).iter().any(|q| q.dist(Vec2::new(110.0, 12.0)) < 1e-9));
    // Control point splines (degree 3 and 5) and a conic.
    run(&mut s, "DrawCVMSpline3D", json!({"points": [[0, 50], [0, 60], [10, 60], [10, 50]]}));
    run(&mut s, "DrawCVMSpline5D", json!({"points": [[20, 50], [20, 60], [25, 65], [30, 60], [35, 62], [40, 50]]}));
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let k = run(&mut s, "ConicCurveCmd", json!({"start": [210, 0], "end": [200, 10], "apex": [210, 10], "rho": w / (1.0 + w)}));
    let kid = ids(&k["curves"])[0].clone();
    // A line tangent to the conic at its end: the tangent arc tool continues it.
    let r = run(&mut s, "ArcTangent", json!({"start": format!("{kid}.end"), "end": [190, 0]}));
    let aid = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    // The conic is a quarter circle about (200, 0) leaving (200, 10) heading -x: the tangent
    // arc from there to (190, 0) continues the same circle.
    let ai = sk.curve_index(&aid).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(200.0, 0.0)) < 1e-6, "{:?}", sk.center(ai));
    assert!(s.execute("DrawSpline", &json!({"points": [[0, 0]]})).is_err());
    assert!(s.execute("ConicCurveCmd", &json!({"start": [0, 0], "end": [1, 0], "apex": [0, 1], "rho": 1.5})).is_err());
    // Mirror and pattern copy free-form curves too.
    let m = ids(&run(&mut s, "DrawPolyline", json!({"points": [[-50, -50], [-50, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "MirrorSketchCommand", json!({"entities": [eid, sid], "line": m}));
    assert_eq!(ids(&r["curves"]).len(), 2);
}

#[test]
fn text_makes_profiles_that_extrude_and_edit() {
    let mut s = new_sketch();
    let r = run(&mut s, "MTextCmd", json!({"text": "SO", "at": [0, 0], "height": 10}));
    let link = r["link"].as_str().unwrap().to_string();
    let a = profiles(&s);
    // S (one region) + O (ring = outer with a hole, plus the counter as its own region).
    assert!(a.len() >= 2, "{a:?}");
    let si = run(&mut s, "sketch.inspect", json!({}));
    assert_eq!(si["dof"], 0);
    // Edit the text: the outlines change, the link stays.
    let r2 = run(&mut s, "sketch.edit_text", json!({"link": link, "text": "I", "height": 20}));
    assert_eq!(r2["link"], json!(link));
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "Extrude", json!({"distance": 2}));
    let m = run(&mut s, "MeasureCommand", json!({}));
    let v = m["total"]["volume_mm3"].as_f64().unwrap();
    // An "I" 20 mm tall is a bar a couple of mm wide.
    assert!(v > 40.0 && v < 200.0, "{v}");
    assert!(s.execute("MTextCmd", &json!({"text": "", "at": [0, 0]})).is_err());
}
