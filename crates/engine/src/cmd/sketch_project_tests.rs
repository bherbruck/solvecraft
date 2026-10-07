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
    run(s, "ChangeParameterCommand", json!({"name": "width", "expression": "40 mm"}));
    run(s, "SketchCreate", json!({"plane": "XY"}));
    let r = run(s, "ShapeRectangleTwoPoint", json!({"p0": [0, 0], "p1": [40, 30]}));
    let l = ids(&r["curves"]);
    let sk = s.doc.sketch(s.active_sketch.unwrap()).unwrap();
    let start = match sk.curves[0].kind {
        solvecraft_sketch::CurveKind::Line { a, .. } => sk.points[a].id.clone(),
        _ => panic!(),
    };
    run(s, "ConstraintCoincident", json!({"a": start, "b": "origin"}));
    run(s, "SketchDimension", json!({"entities": [l[0]], "value": "width"}));
    run(s, "SketchDimension", json!({"entities": [l[1]], "value": 30}));
    run(s, "SketchStop", json!({}));
    run(s, "Extrude", json!({"distance": 20}));
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
    let r = run(&mut s, "SketchCreate", json!({"plane": {"face": [20, 15, 20]}}));
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
    let c = run(&mut s, "CircleCenterRadius", json!({"center": [10, 10], "radius": 3}));
    let cid = ids(&c["curves"])[0].clone();
    let e = lc.iter().find(|c| c["start_at"][1].as_f64().unwrap().abs() < 1e-9 && c["end_at"][1].as_f64().unwrap().abs() < 1e-9).unwrap();
    run(&mut s, "SketchDimension", json!({"entities": [format!("{cid}.center"), e["id"]], "value": 8}));
    let si = inspect(&mut s);
    let center = si["points"].as_array().unwrap().iter().find(|p| p["id"] == format!("{cid}.center")).unwrap();
    assert!((center["at"][1].as_f64().unwrap() - 8.0).abs() < 1e-7, "{center}");
    run(&mut s, "SketchStop", json!({}));
    // Widen the box: the projected loop follows.
    run(&mut s, "ChangeParameterCommand", json!({"name": "width", "expression": "50 mm"}));
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
    let r = run(&mut s, "SketchCreate", json!({"plane": {"face": [20, 15, 20]}, "project_edges": false}));
    assert!(r["projected"].is_null());
    run(&mut s, "SketchStop", json!({}));
    run(&mut s, "sketch.auto_project", json!({"value": false}));
    let r = run(&mut s, "SketchCreate", json!({"plane": {"face": [20, 15, 20]}}));
    assert!(r["projected"].is_null());
}

#[test]
fn project_edges_bodies_sketches_and_origin() {
    let mut s = Session::default();
    box_part(&mut s);
    run(&mut s, "PrimitiveCylinder", json!({"base": [80, 0, 5], "radius": 6, "height": 10}));
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    // An edge on the top face projects to a line on XY.
    let r = run(&mut s, "ProjectNewCmd", json!({"refs": [{"edge": [20, 30, 20]}]}));
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == r["links"][0]["curves"][0]).unwrap().clone();
    assert_eq!(c["type"], "line");
    let ys: Vec<f64> = [c["start_at"][1].as_f64().unwrap(), c["end_at"][1].as_f64().unwrap()].to_vec();
    assert!(ys.iter().all(|y| (y - 30.0).abs() < 1e-9), "{c}");
    // The cylinder's silhouette seen from above is its circle, exactly.
    let name = s.model.state().bodies[1].name.clone();
    let r = run(&mut s, "ProjectNewCmd", json!({"refs": [{"body": name}]}));
    let cid = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == cid).unwrap().clone();
    assert_eq!(c["type"], "circle", "{c}");
    assert!((c["radius"].as_f64().unwrap() - 6.0).abs() < 1e-6, "{c}");
    assert!((c["center_at"][0].as_f64().unwrap() - 80.0).abs() < 1e-6, "{c}");
    // Origin, the Y axis and the YZ plane.
    run(&mut s, "ProjectNewCmd", json!({"refs": ["origin", {"axis": "Y"}, {"plane": "YZ"}]}));
    // The box silhouette from above is its 40 x 30 outline.
    let b0 = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "ProjectNewCmd", json!({"refs": [{"body": b0}]}));
    assert_eq!(r["links"][0]["curves"].as_array().unwrap().len(), 4, "{r}");
    run(&mut s, "SketchStop", json!({}));
    let first = s.active_sketch;
    assert!(first.is_none());
    let sk1 = s.doc.features.last().unwrap().id;
    // Another sketch projects a curve of the first one.
    run(&mut s, "SketchCreate", json!({"plane": "XY", "offset": 50}));
    let r = run(&mut s, "ProjectNewCmd", json!({"refs": [{"sketch": sk1, "curve": cid}]}));
    let c2 = r["links"][0]["curves"][0].clone();
    let si = inspect(&mut s);
    let c = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == c2).unwrap().clone();
    assert_eq!(c["type"], "circle");
    // Bad references fail cleanly.
    assert!(s.execute("ProjectNewCmd", &json!({"refs": [{"edge": [1000, 1000, 1000]}]})).is_err());
    assert!(s.execute("ProjectNewCmd", &json!({"refs": [{"plane": "XY"}]})).is_err(), "parallel plane projects to nothing");
}

#[test]
fn intersect_cuts_a_body_with_the_sketch_plane() {
    let mut s = Session::default();
    run(&mut s, "PrimitiveCylinder", json!({"base": [0, 0, 0], "radius": 7, "height": 30}));
    run(&mut s, "SketchCreate", json!({"plane": "XY", "offset": 12}));
    let name = s.model.state().bodies[0].name.clone();
    let r = run(&mut s, "IntersectCmd", json!({"refs": [{"body": name}]}));
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
    let r = run(&mut s, "SketchCreate", json!({"plane": {"face": [20, 15, 20]}}));
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
    run(&mut s, "PrimitiveBox", json!({"length": 10, "width": 10, "height": 10}));
    let boxf = s.doc.features.last().unwrap().id;
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    run(&mut s, "ProjectNewCmd", json!({"refs": [{"edge": [5, 0, 10]}]}));
    run(&mut s, "SketchStop", json!({}));
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
    run(&mut s, "SketchCreate", json!({"plane": "XY"}));
    let r = run(&mut s, "DrawPolyline", json!({"points": [{"vertex": [40, 30, 20]}, [60, 45]]}));
    let l = ids(&r["curves"])[0].clone();
    let si = inspect(&mut s);
    let start = si["curves"].as_array().unwrap().iter().find(|c| c["id"] == l).unwrap()["start"].clone();
    let p = si["points"].as_array().unwrap().iter().find(|p| p["id"] == start).unwrap().clone();
    assert!(p["link"].is_string(), "{p}");
    assert_eq!(p["fixed"], true);
    assert!((p["at"][0].as_f64().unwrap() - 40.0).abs() < 1e-9);
    // Snapping onto an edge projects it and keeps the point on it.
    let r = run(&mut s, "DrawPolyline", json!({"points": [{"on_edge": [20, 0, 20]}, [20, -15]]}));
    assert_eq!(ids(&r["curves"]).len(), 1);
    let si = inspect(&mut s);
    assert!(si["constraints"].as_array().unwrap().iter().any(|c| c["type"] == "point_on_curve"), "{si}");
}
