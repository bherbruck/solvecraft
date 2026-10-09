use serde_json::{Value, json};

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

/// A 40 x 30 x 20 box from a sketch whose width is the user parameter `width`.
fn box_part(s: &mut Session) {
    run(s, "parameters.change", json!({"name": "width", "expression": "40 mm"}));
    run(s, "sketch.create", json!({"plane": "XY"}));
    let r = run(s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    let start = match sk.curves[0].kind {
        solvecraft_sketch::CurveKind::Line { a, .. } => sk.points[a].id.clone(),
        _ => panic!(),
    };
    run(s, "sketch.constraint.coincident", json!({"a": start, "b": "origin"}));
    run(s, "sketch.dimension", json!({"entities": [l[0]], "value": "width"}));
    run(s, "sketch.dimension", json!({"entities": [l[1]], "value": 30}));
    run(s, "sketch.finish", json!({}));
    run(s, "solid.extrude", json!({"distance": 20}));
}

fn inspect(s: &mut Session) -> Value {
    run(s, "sketch.inspect", json!({}))
}

fn linked_curves(si: &Value) -> Vec<Value> {
    si["curves"].as_array().unwrap().iter().filter(|c| c.get("link").is_some()).cloned().collect()
}

#[test]
fn sketch_on_face_projects_its_edges_and_follows_edits() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_string(), "{r}");
    let si = inspect(&mut s);
    let lc = linked_curves(&si);
    assert_eq!(lc.len(), 4, "{si}");
    assert!(lc.iter().all(|c| c["type"] == "line"));
    // The face loop is one closed profile of projected lines.
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1, "{si}");
    assert!((si["profiles"][0]["area_mm2"].as_f64().unwrap() - 1200.0).abs() < 1e-6);
    // Projected geometry is fixed: the sketch has no degrees of freedom.
    assert_eq!(si["dof"], 0, "{si}");
    // A circle on the face, dimensioned to a projected edge.
    let c = run(&mut s, "sketch.circle.center", json!({"center": [10, 10], "radius": 3}));
    let cid = ids(&c["curves"])[0].clone();
    let e = lc.iter().find(|c| c["start_at"][1].as_f64().unwrap().abs() < 1e-9 && c["end_at"][1].as_f64().unwrap().abs() < 1e-9).unwrap();
    run(&mut s, "sketch.dimension", json!({"entities": [format!("{cid}.center"), e["id"]], "value": 8}));
    let si = inspect(&mut s);
    let center = si["points"].as_array().unwrap().iter().find(|p| p["id"] == format!("{cid}.center")).unwrap();
    assert!((center["at"][1].as_f64().unwrap() - 8.0).abs() < 1e-7, "{center}");
    run(&mut s, "sketch.finish", json!({}));
    // Widen the box: the projected loop follows.
    run(&mut s, "parameters.change", json!({"name": "width", "expression": "50 mm"}));
    let st = s.model.state();
    let ss = st.sketches.last().unwrap();
    assert_eq!(ss.profiles.len(), 2);
    let outer = ss.profiles.iter().map(|p| p.area).fold(0.0, f64::max);
    assert!((outer - (1500.0 - std::f64::consts::PI * 9.0)).abs() < 1e-6, "{outer}");
    let w = s.model.results.last().unwrap().warning.clone();
    assert!(w.is_none(), "{w:?}");
}

#[test]
fn auto_project_can_be_turned_off() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}, "project_edges": false}));
    assert!(r["projected"].is_null());
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "sketch.auto_project", json!({"value": false}));
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_null());
}

#[test]
fn project_edges_bodies_sketches_and_origin() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "solid.cylinder", json!({"base": [80, 0, 5], "radius": 6, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    // An edge on the top face projects to a line on XY.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [20, 30, 20]}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "line");
    let ys: Vec<f64> = [c["start_at"][1].as_f64().unwrap(), c["end_at"][1].as_f64().unwrap()].to_vec();
    assert!(ys.iter().all(|y| (y - 30.0).abs() < 1e-9), "{c}");
    // The cylinder's silhouette seen from above is its circle, exactly.
    let name = s.model.state().bodies[1].name.clone();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": name}]}));
    let cid = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == cid).unwrap().clone();
    assert_eq!(c["type"], "circle", "{c}");
    assert!((c["radius"].as_f64().unwrap() - 6.0).abs() < 1e-6, "{c}");
    assert!((c["center_at"][0].as_f64().unwrap() - 80.0).abs() < 1e-6, "{c}");
    // Origin, the Y axis and the YZ plane.
    run(&mut s, "sketch.project", json!({"refs": ["origin", {"axis": "Y"}, {"plane": "YZ"}]}));
    // The box silhouette from above is its 40 x 30 outline.
    let b0 = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.project", json!({"refs": [{"body": b0}]}));
    assert_eq!(r["links"][0]["curves"].as_array().unwrap().len(), 4, "{r}");
    run(&mut s, "sketch.finish", json!({}));
    let first = s.active_sketch;
    assert!(first.is_none());
    let sk1 = s.doc.features.last().unwrap().id;
    // Another sketch projects a curve of the first one.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": sk1, "curve": cid}]}));
    let c2 = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == c2).unwrap().clone();
    assert_eq!(c["type"], "circle");
    // Bad references fail cleanly.
    assert!(s.execute("sketch.project", &json!({"refs": [{"edge": [1000, 1000, 1000]}]})).is_err());
    assert!(s.execute("sketch.project", &json!({"refs": [{"plane": "XY"}]})).is_err(), "parallel plane projects to nothing");
}

#[test]
fn intersect_cuts_a_body_with_the_sketch_plane() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 12}));
    let name = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.intersect", json!({"refs": [{"body": name}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "circle", "{c}");
    assert!((c["radius"].as_f64().unwrap() - 7.0).abs() < 1e-6, "{c}");
    assert_eq!(si["profiles"].as_array().unwrap().len(), 1);
}

#[test]
fn break_link_makes_normal_geometry() {
    let mut s = Session::default();
    box_part(&mut s);
    let r = run(&mut s, "sketch.create", json!({"plane": {"face": [20, 15, 20]}}));
    let link = r["projected"].as_str().unwrap().to_string();
    run(&mut s, "sketch.break_link", json!({"link": link}));
    let si = inspect(&mut s);
    assert!(linked_curves(&si).is_empty());
    assert!(si["links"].as_array().unwrap().is_empty());
    // Now free: 4 lines with 4 shared corners, 8 degrees of freedom.
    assert_eq!(si["dof"], 8, "{si}");
    assert!(s.execute("sketch.break_link", &json!({"link": "j99"})).is_err());
}

#[test]
fn lost_reference_keeps_geometry_with_a_warning() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    let boxf = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.project", json!({"refs": [{"edge": [5, 0, 10]}]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "timeline.suppress", json!({"feature": boxf, "suppressed": true}));
    let st = s.model.state();
    let ss = st.sketches.last().unwrap();
    assert_eq!(ss.sketch.curves.len(), 1, "geometry kept");
    assert!(ss.sketch.links[0].lost);
    let w = s.model.results.last().unwrap().warning.clone().unwrap_or_default();
    assert!(w.contains("lost"), "{w}");
}

#[test]
fn drawing_on_a_model_vertex_projects_it() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.line", json!({"points": ["vertex:40,30,20", [60, 45]]}));
    let l = ids(&r["curves"])[0].clone();
    let si = inspect(&mut s);
    let start = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == l).unwrap()["start"].clone();
    let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == start).unwrap().clone();
    assert!(p["link"].is_string(), "{p}");
    assert_eq!(p["fixed"], true);
    assert!((p["at"][0].as_f64().unwrap() - 40.0).abs() < 1e-9);
    // Snapping onto an edge projects it and keeps the point on it.
    let r = run(&mut s, "sketch.line", json!({"points": [{"on_edge": [20, 0, 20]}, [20, -15]]}));
    assert_eq!(ids(&r["curves"]).len(), 1);
    let si = inspect(&mut s);
    assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["type"] == "point_on_curve"), "{si}");
}

#[test]
fn fit_curves_to_a_mesh_section() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10}));
    let name = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "sketch.fit_curves_to_section", json!({"body": name}));
    assert_eq!(r["curves"].as_array().unwrap().len(), 1, "{r}");
    let si = inspect(&mut s);
    assert!(si["links"].as_array().unwrap().is_empty());
    let c = &si["curves"][0];
    assert_eq!(c["type"], "circle");
    assert!(c.get("link").is_none());
    assert!(s.execute("sketch.fit_curves_to_section", &json!({"body": "nope"})).is_err());
}

#[test]
fn three_d_sketch_curves() {
    let mut s = Session::default();
    // Include keeps an out-of-plane edge 3D.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.include_3d", json!({"refs": [{"edge": [10, 10, 5]}]}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    assert_eq!(sk.wires.len(), 1, "{r}");
    assert!((sk.wires[0].pts.iter().map(|p| p.z).fold(0.0, f64::max) - 10.0).abs() < 1e-9);
    run(&mut s, "sketch.finish", json!({}));

    // Project a line onto a sphere along the sketch normal.
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-10, 0], [10, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let src = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project_to_surface", json!({"curves": [{"sketch": src, "curve": l}], "face": [0, 0, 20]}));
    let wid = r["links"][0]["wires"][0].as_str().unwrap().to_string();
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(&wid).unwrap()];
    assert!(w.pts.len() > 20);
    for p in &w.pts {
        assert!((p.len() - 20.0).abs() < 0.1, "{p:?}");
        assert!(p.z > 0.0);
    }
    // Delete the wire.
    run(&mut s, "sketch.delete", json!({"entities": [wid]}));
    assert!(s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.wires.is_empty());
    run(&mut s, "sketch.finish", json!({}));

    // Two crossing cylinders meet in 3D curves.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, -20], "radius": 5, "height": 40}));
    run(&mut s, "solid.cylinder", json!({"base": [-20, 0, 0], "axis": [1, 0, 0], "radius": 4, "height": 40, "operation": "new"}));
    let names: Vec<String> = s.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.intersection_curve", json!({"a": {"body": names[0]}, "b": {"body": names[1]}}));
    assert!(!r["wires"].as_array().unwrap().is_empty(), "{r}");
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    for w in &sk.wires {
        for p in &w.pts {
            assert!(((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs() < 0.1, "{p:?}");
            assert!(((p.y * p.y + p.z * p.z).sqrt() - 4.0).abs() < 0.1, "{p:?}");
        }
    }
    run(&mut s, "sketch.finish", json!({}));

    // Spun profile of a cylinder about Z, on the XZ plane: a straight outline 6 from the axis.
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    let name = s.model.state().bodies[0].name.clone();
    run(&mut s, "sketch.create", json!({"plane": "XZ"}));
    let r = run(&mut s, "sketch.spun_profile", json!({"body": name, "axis": "Z"}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "line", "{c}");
    assert!((c["start_at"][0].as_f64().unwrap().abs() - 6.0).abs() < 1e-3, "{c}");
}

#[test]
fn isoparametric_curves_of_a_cylinder() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "radius": 6, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    // u: around (a circle at z = 10); v: along (a line from z 0 to 20).
    let r = run(&mut s, "sketch.isoparametric_curve", json!({"face": [6, 0, 10]}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(r["wires"][0].as_str().unwrap()).unwrap()];
    assert!(w.pts.iter().all(|p| (p.z - 10.0).abs() < 1e-6 && ((p.x * p.x + p.y * p.y).sqrt() - 6.0).abs() < 1e-3), "{:?}", &w.pts[..3]);
    let r = run(&mut s, "sketch.isoparametric_curve", json!({"face": [6, 0, 10], "direction": "v"}));
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    let w = &sk.wires[sk.wire_index(r["wires"][0].as_str().unwrap()).unwrap()];
    let zs: Vec<f64> = w.pts.iter().map(|p| p.z).collect();
    assert!(zs.iter().cloned().fold(f64::MIN, f64::max) > 19.9 && zs.iter().cloned().fold(f64::MAX, f64::min) < 0.1, "{zs:?}");
    assert!(w.pts.iter().all(|p| (p.x - 6.0).abs() < 1e-3 && p.y.abs() < 1e-3));
}

#[test]
fn tilted_circles_project_to_exact_ellipses_and_splines_stay_splines() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"base": [0, 0, 0], "axis": [1, 0, 1], "radius": 5, "height": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": -10}));
    // The base circle (centred at the origin) seen from above.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, 5, 0]}]}));
    // One seam half: exact conics (rational quadratic arcs).
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "conic", "{c}");
    let st = s.model.state();
    let sk = &st.sketch(s.active_sketch.unwrap()).unwrap().sketch;
    for ci in sk.link_curves(r["links"][0]["link"].as_str().unwrap()) {
        for q in sk.polyline(ci) {
            // On the ellipse x²/(5/√2)² + y²/5² = 1.
            let e = (q.x / (5.0 * std::f64::consts::FRAC_1_SQRT_2)).powi(2) + (q.y / 5.0).powi(2);
            assert!((e - 1.0).abs() < 1e-9, "{q:?}");
        }
    }
    // The whole face (both halves of its rim, plus its seam) gives the full ellipse.
    let r = run(&mut s, "sketch.project", json!({"refs": [{"face": [0, 0, 0]}]}));
    let si = inspect(&mut s);
    let types: Vec<String> = r["links"][0]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| si["curves"].as_array().unwrap().iter().find(|c| c["id"] == *id).unwrap()["type"].as_str().unwrap().to_string())
        .collect();
    assert!(types.iter().all(|t| t == "conic" || t == "ellipse"), "{types:?}");
    let r = run(&mut s, "sketch.project", json!({"refs": [{"edge": [0, -5, 0]}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    if c["type"] == "ellipse" {
        assert!((c["minor_radius"].as_f64().unwrap() - 5.0 * std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-6, "{c}");
        let major = &c["major_at"];
        assert!(((major[0].as_f64().unwrap().powi(2) + major[1].as_f64().unwrap().powi(2)).sqrt() - 5.0).abs() < 1e-6, "{c}");
    }
    run(&mut s, "sketch.finish", json!({}));
    // A spline of a parallel sketch projects as the same spline.
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 30}));
    let sp = run(&mut s, "sketch.spline.fit_point", json!({"points": [[0, 0], [10, 5], [20, 0]]}))["curves"][0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let sk1 = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project", json!({"refs": [{"sketch": sk1, "curve": sp}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "spline", "{c}");
    assert_eq!(c["points"].as_array().unwrap().len(), 3);
}

#[test]
fn sweeps_and_pipes_follow_3d_sketch_curves() {
    // A line projected onto a sphere: a 3D arc on its top.
    let mut s = Session::default();
    run(&mut s, "solid.sphere", json!({"center": [0, 0, 0], "radius": 20}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50}));
    let l = ids(&run(&mut s, "sketch.line", json!({"points": [[-12, 0], [12, 0]]}))["curves"])[0].clone();
    run(&mut s, "sketch.finish", json!({}));
    let src = s.doc.features.last().unwrap().id;
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    let r = run(&mut s, "sketch.project_to_surface", json!({"curves": [{"sketch": src, "curve": l}], "face": [0, 0, 20]}));
    let wid = r["links"][0]["wires"][0].as_str().unwrap().to_string();
    let path_sketch = s.active_sketch.unwrap();
    let pts = {
        let st = s.model.state();
        let sk = &st.sketch(path_sketch).unwrap().sketch;
        sk.wires[sk.wire_index(&wid).unwrap()].pts.clone()
    };
    run(&mut s, "sketch.finish", json!({}));
    let len: f64 = pts.windows(2).map(|w| w[0].dist(w[1])).sum();
    // A pipe along it: volume ≈ π r² × length.
    let before = s.model.state().bodies.len();
    run(&mut s, "solid.pipe", json!({"path_sketch": path_sketch, "path": [wid.clone()], "diameter": "2 mm", "operation": "new"}));
    let st = s.model.state();
    assert_eq!(st.bodies.len(), before + 1);
    let v = st.bodies.last().unwrap().mesh().measure().volume;
    let want = std::f64::consts::PI * len;
    assert!((v - want).abs() < 0.05 * want, "{v} vs {want}");
    drop(st);
    // A sweep of a square profile drawn square to the curve at its start.
    let (a, b) = (pts[0], pts[1]);
    let t = (b - a).normalized().unwrap();
    let x = t.cross(solvecraft_geom::Vec3::Z).normalized().unwrap();
    let y = t.cross(x);
    run(&mut s, "sketch.create", json!({"plane": {"origin": [a.x, a.y, a.z], "x_dir": [x.x, x.y, x.z], "y_dir": [y.x, y.y, y.z]}}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [-1, -1], "p1": [1, 1]}));
    let prof = s.active_sketch.unwrap();
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "solid.sweep", json!({"sketch": prof, "path_sketch": path_sketch, "path": [wid], "operation": "new"}));
    let st = s.model.state();
    assert_eq!(st.bodies.len(), before + 2);
    let v = st.bodies.last().unwrap().mesh().measure().volume;
    assert!((v - 4.0 * len).abs() < 0.05 * 4.0 * len, "{v} vs {}", 4.0 * len);
}
