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
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
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
    let h = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let v = ids(&run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 5]]}))["curves"])[0].clone();
    // Trim the right half of the horizontal line.
    run(&mut s, "sketch.trim", json!({"curve": h, "at": [15, 0]}));
    let sk = sketch(&s);
    let sh = sk.shape(sk.curve_index(&h).unwrap()).unwrap();
    assert!(
        matches!(sh, solvecraft_sketch::Shape::Line { a, b } if a.dist(Vec2::new(0.0, 0.0)) < 1e-9 && b.dist(Vec2::new(10.0, 0.0)) < 1e-9),
        "{sh:?}"
    );
    // Trim the lower half of the vertical line: the original id survives on the upper piece.
    run(&mut s, "sketch.trim", json!({"curve": v, "at": [10, -3]}));
    let sk = sketch(&s);
    assert_eq!(sk.curves.len(), 2);
    // Extend: a short line reaching toward the vertical one.
    let e = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 3], [4, 3]]}))["curves"])[0].clone();
    run(&mut s, "sketch.extend", json!({"curve": e, "at": [4, 3]}));
    let sk = sketch(&s);
    let end = sk.resolve_point(&format!("{e}.end")).and_then(|i| sk.point(i)).unwrap();
    assert!(end.dist(Vec2::new(10.0, 3.0)) < 1e-7, "{end:?}");
    // Break the horizontal line where... nothing crosses it inside: error.
    assert!(s.execute("sketch.break", &json!({"curve": h, "at": [5, 0]})).is_err());
    // Break the vertical at the extended line: two pieces.
    let r = run(&mut s, "sketch.break", json!({"curve": v, "at": [10, 1]}));
    assert_eq!(ids(&r["curves"]).len(), 2, "{r}");
    // A lone curve trims away entirely.
    let lone = ids(&run(&mut s, "sketch.line", json!({"points": [[50, 50], [60, 50]]}))["curves"])[0].clone();
    run(&mut s, "sketch.trim", json!({"curve": lone, "at": [55, 50]}));
    assert!(sketch(&s).curve_index(&lone).is_none());
}

#[test]
fn trim_circle_makes_an_arc_and_a_profile() {
    let mut s = new_sketch();
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [[-20, 0], [20, 0]]}));
    run(&mut s, "sketch.trim", json!({"curve": c, "at": [0, -10]}));
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
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Pin the rectangle so a dimension change only moves the fillet.
    run(&mut s, "sketch.constraint.fix", json!({"entity": l[2]}));
    run(&mut s, "sketch.constraint.fix", json!({"entity": l[3]}));
    let f = run(&mut s, "sketch.fillet", json!({"a": l[0], "b": l[1], "radius": 5}));
    assert!(f["param"].is_string());
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (25.0 - std::f64::consts::PI * 25.0 / 4.0)), "{a:?}");
    // The fillet radius is a dimension: change it.
    let pname = f["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": pname, "expression": "8 mm"}));
    let a = profiles(&s);
    assert!(close(a[0], 1200.0 - (64.0 - std::f64::consts::PI * 64.0 / 4.0)), "{a:?}");
    run(&mut s, "sketch.chamfer.equal_distance", json!({"a": l[2], "b": l[3], "distance": 4}));
    let a2 = profiles(&s)[0];
    assert!(close(a[0] - a2, 8.0), "{a2}");
    run(&mut s, "sketch.chamfer.two_distance", json!({"a": l[1], "b": l[2], "distance": 2, "distance2": 6}));
    let a3 = profiles(&s)[0];
    assert!(close(a2 - a3, 6.0), "{a3}");
    // Distance-angle at the last corner (origin, 90 degrees): 45 deg gives an isosceles cut.
    run(&mut s, "sketch.chamfer.distance_angle", json!({"a": l[3], "b": l[0], "distance": 3, "angle": 45}));
    let a4 = profiles(&s)[0];
    assert!(close(a3 - a4, 4.5), "{a4}");
    assert!(s.execute("sketch.fillet", &json!({"a": l[0], "b": l[2], "radius": 1})).is_err(), "no shared corner");
}

#[test]
fn offset_chain_mirror_patterns_move_scale() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    // Outward offset by 2 (toward a point outside).
    let o = run(&mut s, "sketch.offset", json!({"curves": [l[0]], "distance": 2, "side": [20, -10]}));
    assert_eq!(ids(&o["curves"]).len(), 4, "{o}");
    let a = profiles(&s);
    assert!(close(a[1], 44.0 * 34.0 - 1200.0) || close(a.iter().sum::<f64>(), 44.0 * 34.0), "{a:?}");
    // Circle offset inward.
    let c = ids(&run(&mut s, "sketch.circle.center", json!({"center": [100, 0], "radius": 10}))["curves"])[0].clone();
    run(&mut s, "sketch.offset", json!({"curves": [c], "distance": 3, "side": [100, 0]}));
    // Mirror a circle about a vertical line.
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[120, -50], [120, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.mirror", json!({"entities": [c], "line": m}));
    let mc = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    assert!(sk.center(sk.curve_index(&mc).unwrap()).unwrap().dist(Vec2::new(140.0, 0.0)) < 1e-7);
    // Circular pattern of the mirrored circle: 4 around (120, 0).
    let r = run(&mut s, "sketch.pattern.circular", json!({"entities": [mc], "center": [120, 0], "count": 4}));
    assert_eq!(ids(&r["curves"]).len(), 3);
    let sk = sketch(&s);
    let c2 = sk.center(sk.curve_index(&ids(&r["curves"])[0]).unwrap()).unwrap();
    assert!(c2.dist(Vec2::new(120.0, 20.0)) < 1e-7, "{c2:?}");
    // Rectangular pattern 3 x 2.
    let small = ids(&run(&mut s, "sketch.circle.center", json!({"center": [0, 100], "radius": 1}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.pattern.rectangular", json!({"entities": [small], "count": 3, "spacing": 5, "count2": 2, "spacing2": 4}));
    assert_eq!(ids(&r["curves"]).len(), 5);
    // Move and scale.
    run(&mut s, "sketch.move", json!({"entities": [small], "translate": [0, 10]}));
    run(&mut s, "sketch.scale", json!({"entities": [small], "base": [0, 110], "factor": 3}));
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
    let r = run(&mut s, "sketch.line.midpoint", json!({"mid": [0, 0], "end": [10, 5]}));
    let l = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let start = sk.resolve_point(&format!("{l}.start")).and_then(|i| sk.point(i)).unwrap();
    assert!(start.dist(Vec2::new(-10.0, -5.0)) < 1e-9);
    // Tangent arc off the end of a horizontal line.
    let h = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 20], [10, 20]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.arc.tangent", json!({"start": format!("{h}.end"), "end": [20, 30]}));
    let a = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&a).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(10.0, 30.0)) < 1e-7, "{:?}", sk.center(ai));
    assert!(close(sk.radius(ai).unwrap(), 10.0));
    // 2-tangent circle in the corner of two lines.
    let x = ids(&run(&mut s, "sketch.line", json!({"points": [[100, 0], [150, 0]]}))["curves"])[0].clone();
    let y = ids(&run(&mut s, "sketch.line", json!({"points": [[100, 0], [100, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.circle.two_tangent", json!({"curves": [x, y], "radius": 5, "near": [106, 104 - 100]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(sk.center(ci).unwrap().dist(Vec2::new(105.0, 5.0)) < 1e-6, "{:?}", sk.center(ci));
    // 3-tangent circle inside a triangle: the incircle of the 3-4-5 triangle (r = 1).
    let t = run(&mut s, "sketch.line", json!({"points": [[200, 0], [203, 0], [200, 4]], "closed": true}));
    let tl = ids(&t["curves"]);
    let r = run(&mut s, "sketch.circle.three_tangent", json!({"curves": tl, "near": [201, 1.2]}));
    let c = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let ci = sk.curve_index(&c).unwrap();
    assert!(close(sk.radius(ci).unwrap(), 1.0), "{:?}", sk.radius(ci));
    assert!(sk.center(ci).unwrap().dist(Vec2::new(201.0, 1.0)) < 1e-6);
    // Slots.
    let before = profiles(&s).len();
    run(&mut s, "sketch.slot.center_point", json!({"center": [0, -100], "end": [10, -100], "width": 4}));
    run(&mut s, "sketch.slot.arc_three_point", json!({"start": [100, -100], "through": [110, -90], "end": [120, -100], "width": 2}));
    run(&mut s, "sketch.slot.arc_center", json!({"center": [200, -100], "start": [210, -100], "end": [200, -90], "width": 2}));
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
    let e = run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor": [0, 4]}));
    let eid = ids(&e["curves"])[0].clone();
    let a = profiles(&s);
    assert!((a[0] - pi * 40.0).abs() / (pi * 40.0) < 2e-3, "{a:?}");
    // A point constrained onto the ellipse lands on it.
    run(&mut s, "sketch.point", json!({"point": [3, 5], "id": "q"}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": "q", "b": eid}));
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
    let sp = run(&mut s, "sketch.spline.fit_point", json!({"points": [[100, 0], [110, 8], [120, 3], [130, 0]]}));
    let sid = ids(&sp["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sid}.end"), format!("{sid}.start")]}));
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
    run(&mut s, "sketch.spline.control_point", json!({"points": [[0, 50], [0, 60], [10, 60], [10, 50]]}));
    run(&mut s, "sketch.spline.control_point_5", json!({"points": [[20, 50], [20, 60], [25, 65], [30, 60], [35, 62], [40, 50]]}));
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let k = run(&mut s, "sketch.conic", json!({"start": [210, 0], "end": [200, 10], "apex": [210, 10], "rho": w / (1.0 + w)}));
    let kid = ids(&k["curves"])[0].clone();
    // A line tangent to the conic at its end: the tangent arc tool continues it.
    let r = run(&mut s, "sketch.arc.tangent", json!({"start": format!("{kid}.end"), "end": [190, 0]}));
    let aid = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    // The conic is a quarter circle about (200, 0) leaving (200, 10) heading -x: the tangent
    // arc from there to (190, 0) continues the same circle.
    let ai = sk.curve_index(&aid).unwrap();
    assert!(sk.center(ai).unwrap().dist(Vec2::new(200.0, 0.0)) < 1e-6, "{:?}", sk.center(ai));
    assert!(s.execute("sketch.spline.fit_point", &json!({"points": [[0, 0]]})).is_err());
    assert!(s.execute("sketch.conic", &json!({"start": [0, 0], "end": [1, 0], "apex": [0, 1], "rho": 1.5})).is_err());
    // Mirror and pattern copy free-form curves too.
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[-50, -50], [-50, 50]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.mirror", json!({"entities": [eid, sid], "line": m}));
    assert_eq!(ids(&r["curves"]).len(), 2);
}

#[test]
fn text_makes_profiles_that_extrude_and_edit() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.text", json!({"text": "SO", "at": [0, 0], "height": 10}));
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
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2}));
    let m = run(&mut s, "inspect.measure", json!({}));
    let v = m["total"]["volume_mm3"].as_f64().unwrap();
    // An "I" 20 mm tall is a bar a couple of mm wide.
    assert!(v > 40.0 && v < 200.0, "{v}");
    assert!(s.execute("sketch.text", &json!({"text": "", "at": [0, 0]})).is_err());
}

#[test]
fn blend_curve_joins_tangentially() {
    let mut s = new_sketch();
    let a = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]]}))["curves"])[0].clone();
    let b = ids(&run(&mut s, "sketch.line", json!({"points": [[20, 10], [20, 20]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.blend_curve", json!({"a": format!("{a}.end"), "b": format!("{b}.start")}));
    assert_eq!(r["sketch"]["solved"], true, "{r}");
    let sp = ids(&r["curves"])[0].clone();
    let sk = sketch(&s);
    let pts: Vec<Vec2> = sk.curves[sk.curve_index(&sp).unwrap()].kind.point_ids().iter().map(|i| sk.point(*i).unwrap()).collect();
    // Leaves along +x, arrives going +y (control polygon end legs).
    let d0 = (pts[1] - pts[0]).normalized().unwrap();
    let d1 = (pts[3] - pts[2]).normalized().unwrap();
    assert!(d0.y.abs() < 1e-6 && d0.x > 0.0, "{d0:?}");
    assert!(d1.x.abs() < 1e-6 && d1.y > 0.0, "{d1:?}");
}

#[test]
fn centerline_is_left_out_of_profiles() {
    let mut s = new_sketch();
    let r = run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    let l = ids(&r["curves"]);
    let m = ids(&run(&mut s, "sketch.line", json!({"points": [[5, -5], [5, 15]]}))["curves"])[0].clone();
    assert_eq!(profiles(&s).len(), 2);
    run(&mut s, "sketch.centerline", json!({"curves": [m]}));
    assert_eq!(profiles(&s).len(), 1);
    assert!(s.execute("sketch.centerline", &json!({"curves": ["nope"]})).is_err());
    let _ = l;
}

#[test]
fn free_form_profiles_extrude_exactly() {
    let pi = std::f64::consts::PI;
    let vol = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    // Ellipse a = 10, b = 4: π a b h.
    let mut s = new_sketch();
    run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}));
    let a = profiles(&s)[0];
    assert!((a - pi * 40.0).abs() < 1e-9, "{a}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 5}));
    let v = vol(&mut s);
    assert!((v - pi * 40.0 * 5.0).abs() / (pi * 200.0) < 2e-4, "{v}");
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["bodies"][0]["faces"], 3, "an exact ellipse: top, bottom and one side ({m})");
    // A region closed by a fit spline and a line.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 8], [20, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    let a = profiles(&s)[0];
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2}));
    let v = vol(&mut s);
    assert!((v - a * 2.0).abs() / (a * 2.0) < 2e-4, "{v} vs {}", a * 2.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!(m["bodies"][0]["faces"].as_u64().unwrap() <= 5, "{m}");
}

#[test]
fn degree_five_and_cut_free_form_profiles_extrude_exactly() {
    let vol = |s: &mut Session| run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap();
    // A degree-5 control spline closed by a line: one B-spline side face.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.control_point_5", json!({"points": [[0, 0], [4, 10], [10, 12], [16, 12], [22, 10], [26, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    let a = profiles(&s)[0];
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 3}));
    let v = vol(&mut s);
    assert!((v - a * 3.0).abs() / (a * 3.0) < 2e-4, "{v} vs {}", a * 3.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert_eq!(m["bodies"][0]["faces"], 4, "{m}");
    // A fit spline arch cut by a line: both sides extrude with exact curved faces.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [6, 9], [14, 9], [20, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.line", json!({"points": [format!("{sp}.end"), format!("{sp}.start")]}));
    run(&mut s, "sketch.line", json!({"points": [[10, -5], [10, 20]]}));
    let areas = profiles(&s);
    assert_eq!(areas.len(), 2, "{areas:?}");
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.extrude", json!({"distance": 2, "profiles": [0]}));
    let v = vol(&mut s);
    assert!((v - areas[0] * 2.0).abs() / (areas[0] * 2.0) < 2e-4, "{v} vs {}", areas[0] * 2.0);
    let m = run(&mut s, "inspect.measure", json!({}));
    assert!(m["bodies"][0]["faces"].as_u64().unwrap() <= 7, "no polyline facets: {m}");
}

#[test]
fn trim_and_break_free_form_curves_exactly() {
    let pi = std::f64::consts::PI;
    let mut s = new_sketch();
    // An ellipse cut by a line through its centre: trimming the lower half leaves the upper
    // half, which closes with the line into half the ellipse.
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}))["curves"])[0].clone();
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-15, 0], [15, 0]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.trim", json!({"curve": e, "at": [0, -4]}));
    assert!(!r["result"]["pieces"].as_array().unwrap().is_empty(), "{r}");
    let a = profiles(&s);
    assert_eq!(a.len(), 1, "{a:?}");
    assert!((a[0] - pi * 20.0).abs() < 1e-6, "{a:?}");
    let _ = l;
    // A fit spline broken where a line crosses it: two pieces that still trace the curve.
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 10], [20, 0], [30, 5]]}))["curves"])[0].clone();
    let before = sketch(&s).polyline(sketch(&s).curve_index(&sp).unwrap());
    run(&mut s, "sketch.line", json!({"points": [[15, -10], [15, 20]]}));
    let r = run(&mut s, "sketch.break", json!({"curve": sp, "at": [15, 5]}));
    let pieces = ids(&r["curves"]);
    assert_eq!(pieces.len(), 4, "three Béziers, the middle one split: {r}");
    let sk = sketch(&s);
    // Every original sample lies on one of the pieces.
    let polys: Vec<Vec<Vec2>> = pieces.iter().map(|id| sk.polyline(sk.curve_index(id).unwrap())).collect();
    for q in before.iter().step_by(5) {
        let d = polys
            .iter()
            .flat_map(|p| {
                p.windows(2).map(|w| {
                    let (a, b) = (w[0], w[1]);
                    let t = ((*q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
                    q.dist(a + (b - a) * t)
                })
            })
            .fold(f64::INFINITY, f64::min);
        assert!(d < 0.05, "{q:?} {d}");
    }
}

#[test]
fn ellipse_radii_dimensions() {
    let mut s = new_sketch();
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [0, 0], "major": [10, 0], "minor_radius": 4}))["curves"])[0].clone();
    run(&mut s, "sketch.dimension", json!({"entities": [e], "type": "major", "value": 12}));
    run(&mut s, "sketch.dimension", json!({"entities": [e], "value": 5}));
    let a = profiles(&s)[0];
    assert!((a - std::f64::consts::PI * 60.0).abs() < 1e-6, "{a}");
}

#[test]
fn fillet_a_line_and_an_arc() {
    let mut s = new_sketch();
    // A line into a quarter arc meeting at a sharp corner.
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0]]}))["curves"])[0].clone();
    let a = ids(&run(&mut s, "sketch.arc.center_point", json!({"center": [30, 0], "start": format!("{l}.end"), "end": [30, -10]}))["curves"])[0].clone();
    let _ = a;
    let f = run(&mut s, "sketch.fillet", json!({"point": format!("{l}.end"), "radius": 2}));
    let arc = ids(&f["curves"])[0].clone();
    let sk = sketch(&s);
    let ai = sk.curve_index(&arc).unwrap();
    assert!((sk.radius(ai).unwrap() - 2.0).abs() < 1e-7);
    // The fillet touches the line (its centre 2 below it) and the arc's circle (outside it: 10 + 2).
    let c = sk.center(ai).unwrap();
    assert!((c.y - 2.0).abs() < 1e-6 || (c.y + 2.0).abs() < 1e-6, "{c:?}");
    let d = c.dist(Vec2::new(30.0, 0.0));
    assert!((d - 12.0).abs() < 1e-6, "{d} {c:?}");
}

#[test]
fn offset_free_form_curves() {
    let mut s = new_sketch();
    let sp = ids(&run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 8], [20, 0], [30, 6]]}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.offset", json!({"curves": [sp], "distance": 2, "side": [10, 20]}));
    let made = ids(&r["curves"]);
    assert_eq!(made.len(), 1, "{r}");
    let sk = sketch(&s);
    let orig = sk.polyline(sk.curve_index(&sp).unwrap());
    let off = sk.polyline(sk.curve_index(&made[0]).unwrap());
    let dist = |q: Vec2| {
        orig.windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let t = ((q - a).dot(b - a) / (b - a).len2()).clamp(0.0, 1.0);
                q.dist(a + (b - a) * t)
            })
            .fold(f64::INFINITY, f64::min)
    };
    for q in off.iter().step_by(4) {
        assert!((dist(*q) - 2.0).abs() < 0.08, "{q:?} {}", dist(*q));
    }
    // An ellipse grows by the offset all round.
    let e = ids(&run(&mut s, "sketch.ellipse", json!({"center": [100, 0], "major": [110, 0], "minor_radius": 5}))["curves"])[0].clone();
    let r = run(&mut s, "sketch.offset", json!({"curves": [e], "distance": 1, "side": [100, 20]}));
    assert_eq!(ids(&r["curves"]).len(), 1, "{r}");
}
