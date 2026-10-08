//! Pre-selection fills every input of a command, each item going to the input that takes its
//! type, whether the command starts from a shortcut, the toolbar or a menu.

use serde_json::json;
use solvecraft_engine::geom::Vec3;
use solvecraft_engine::{Sel, Session};

use crate::dialogs::{Dialog, Kind, apply_commands};
use crate::{Services, SolveApp};

fn app_with(script: serde_json::Value) -> SolveApp {
    let mut s = Session::default();
    s.run_script(&script).unwrap();
    SolveApp::new(s, Services::default())
}

fn boxed() -> SolveApp {
    app_with(json!([{"command": "PrimitiveBox", "params": {"length": 40, "width": 30, "height": 20}}]))
}

/// The face of Body1 whose centre is at `c`.
fn face(app: &SolveApp, c: Vec3) -> Sel {
    let st = app.session.model.state();
    let b = st.body("Body1").unwrap();
    let f = b.body.faces(0.01).unwrap().into_iter().find(|f| f.centroid.dist(c) < 1e-6).unwrap();
    Sel::Face { body: "Body1".into(), index: f.index, point: c }
}

fn edge(app: &SolveApp, p: Vec3) -> Sel {
    let st = app.session.model.state();
    let m = st.body("Body1").unwrap().mesh();
    let i = m.edges.iter().position(|e| e.windows(2).any(|w| p.dist_to_segment(w[0], w[1]) < 1e-9)).unwrap();
    Sel::Edge { body: "Body1".into(), index: i, point: p }
}

fn start(app: &mut SolveApp, sels: Vec<Sel>, id: &str) -> Dialog {
    app.session.selection = sels;
    app.start(id);
    app.dialog.clone().unwrap()
}

#[test]
fn profiles_then_extrude_takes_them_all() {
    let mut app = app_with(json!([
        {"command": "SketchCreate", "params": {"plane": "XY"}},
        {"command": "CircleCenterRadius", "params": {"center": [0, 0], "radius": 5}},
        {"command": "CircleCenterRadius", "params": {"center": [20, 0], "radius": 5}},
        {"command": "SketchStop", "params": {}},
    ]));
    let sk = app.session.doc.features.first().unwrap().id;
    let d = start(&mut app, vec![Sel::Profile { sketch: sk, index: 0 }, Sel::Profile { sketch: sk, index: 1 }], "Extrude");
    assert_eq!(d.inputs[0].items.len(), 2);
    assert!(d.focus, "the value box takes the keyboard");
    assert!(apply_commands(&app, &d).is_ok());
}

#[test]
fn edges_then_fillet_takes_them_with_chains() {
    let mut app = boxed();
    let (a, b) = (edge(&app, Vec3::new(0.0, 0.0, 10.0)), edge(&app, Vec3::new(20.0, 0.0, 20.0)));
    let d = start(&mut app, vec![a, b], "FusionFilletEdgesCommand");
    assert_eq!(d.inputs[0].items.len(), 2, "box edges are not tangent to others");
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["edges"].as_array().unwrap().len(), 2);
}

#[test]
fn face_then_extrude_press_pulls_it() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let d = start(&mut app, vec![top], "Extrude");
    let cmds = apply_commands(&app, &d).unwrap();
    assert!(cmds[0].1.get("face").is_some());
}

#[test]
fn faces_then_shell_and_face_then_hole() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let front = face(&app, Vec3::new(20.0, 0.0, 10.0));
    let d = start(&mut app, vec![top.clone(), front], "FusionShellBodyCommand");
    assert_eq!(d.inputs[0].items.len(), 2);
    let d = start(&mut app, vec![top], "FusionHoleCommand");
    assert_eq!(d.inputs[0].items.len(), 1);
    let cmds = apply_commands(&app, &d).unwrap();
    assert!(cmds[0].1.get("position").is_some());
}

#[test]
fn line_and_profile_then_revolve_routes_each() {
    let mut app = app_with(json!([
        {"command": "SketchCreate", "params": {"plane": "XZ"}},
        {"command": "ShapeRectangleTwoPoint", "params": {"p0": [10, 0], "p1": [20, 10]}},
        {"command": "DrawPolyline", "params": {"points": [[0, 0], [0, 30]], "ids": ["axis"]}},
    ]));
    let sk = app.session.active_sketch.unwrap();
    let d = start(&mut app, vec![Sel::SketchCurve { id: "axis".into() }, Sel::Profile { sketch: sk, index: 0 }], "Revolve");
    assert_eq!(d.inputs[0].items, vec![Sel::Profile { sketch: sk, index: 0 }]);
    assert_eq!(d.inputs[1].items, vec![Sel::SketchCurve { id: "axis".into() }]);
}

#[test]
fn face_then_move_takes_the_body_and_odd_items_are_ignored() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let d = start(&mut app, vec![top, Sel::Axis { name: "X".into() }], "FusionMoveCommand");
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Body1".into() }]);
    assert!(matches!(d.kind, Kind::Move { .. }));
}

#[test]
fn hole_at_sketch_points() {
    let mut app = app_with(json!([
        {"command": "PrimitiveBox", "params": {"length": 40, "width": 30, "height": 20}},
        {"command": "SketchCreate", "params": {"plane": {"face": [20, 15, 20]}}},
        {"command": "DrawPoint", "params": {"point": [10, 10], "id": "h1"}},
        {"command": "DrawPoint", "params": {"point": [30, 20], "id": "h2"}},
        {"command": "SketchStop", "params": {}},
    ]));
    let sk = app.session.doc.features.iter().find(|f| matches!(f.kind, solvecraft_engine::doc::FeatureKind::Sketch { .. })).unwrap().id;
    app.start("FusionHoleCommand");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Hole { opts, .. } = &mut d.kind {
        opts.multiple = true;
    }
    d.inputs = vec![crate::selection::SelInput::new("Points", crate::selection::POINTS, true)];
    d.inputs[0].items = vec![Sel::SketchPoint { id: format!("{sk}:h1") }, Sel::SketchPoint { id: format!("{sk}:h2") }];
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].1["points"], json!(["h1", "h2"]));
    let r = app.session.execute(&cmds[0].0, &cmds[0].1);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn a_hole_is_patterned_as_a_feature() {
    let mut app = app_with(json!([
        {"command": "PrimitiveBox", "params": {"length": 60, "width": 40, "height": 10}},
        {"command": "FusionHoleCommand", "params": {"position": [10, 10, 10], "diameter": 5}}
    ]));
    let hole = app.session.doc.features.last().unwrap().id;
    // Picked in the timeline before the command: the objects become features.
    let mut d = start(&mut app, vec![Sel::Feature { id: hole }], "PatternRectangular");
    assert_eq!(d.inputs[0].accept, crate::selection::FEATURES);
    assert_eq!(d.inputs[0].items, vec![Sel::Feature { id: hole }]);
    d.inputs[1].items = vec![Sel::Axis { name: "X".into() }];
    if let Kind::PatternRect { count, spacing, .. } = &mut d.kind {
        *count = "4".into();
        *spacing = "12 mm".into();
    }
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["features"], json!(["Hole1"]));
    let before = app.session.model.state().bodies[0].mesh().measure().volume;
    for (id, p) in cmds {
        app.session.execute(&id, &p).unwrap();
    }
    let after = app.session.model.state().bodies[0].mesh().measure().volume;
    assert!(after < before - 500.0, "three more holes: {before} -> {after}");
    // On the canvas, a hole's wall stands for the hole; the top face for the box.
    let st = app.session.model.state();
    let m = st.bodies[0].mesh();
    let face_at = |p: Vec3| {
        let t = m.triangles.iter().position(|t| super::dialogs::point_tri_dist(p, m.tri(t).unwrap()) < 0.05).unwrap();
        let [a, b, c] = m.tri(&m.triangles[t]).unwrap();
        crate::viewport::Hit::Face { body: "Body1".into(), index: m.tri_face[t] as usize, point: (a + b + c) * (1.0 / 3.0) }
    };
    let mut d = start(&mut app, vec![], "PatternCircular");
    d.inputs[0].accept = crate::selection::FEATURES;
    let box_id = app.session.doc.features[0].id;
    assert_eq!(d.candidate(&app.session, &face_at(Vec3::new(12.5, 10.0, 5.0))), Some(Sel::Feature { id: hole }));
    assert_eq!(d.candidate(&app.session, &face_at(Vec3::new(30.0, 30.0, 10.0))), Some(Sel::Feature { id: box_id }));
    app.dialog = None;
    // Edit Feature shows the hole as a feature again.
    let pat = app.session.doc.features.last().unwrap().id;
    let e = crate::dialogs::for_feature(&app, pat, None).unwrap();
    assert_eq!(e.inputs[0].accept, crate::selection::FEATURES);
    assert_eq!(e.inputs[0].items, vec![Sel::Feature { id: hole }]);
}

#[test]
fn a_hole_is_mirrored_as_a_feature() {
    let mut app = app_with(json!([
        {"command": "PrimitiveBox", "params": {"length": 60, "width": 40, "height": 10}},
        {"command": "FusionHoleCommand", "params": {"position": [10, 10, 10], "diameter": 5}}
    ]));
    let hole = app.session.doc.features.last().unwrap().id;
    let mut d = start(&mut app, vec![Sel::Feature { id: hole }], "MirrorCommand");
    assert_eq!(d.inputs[0].accept, crate::selection::FEATURES);
    d.inputs[1].items = vec![Sel::Plane { name: "YZ".into() }];
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["features"], json!(["Hole1"]));
}

#[test]
fn chamfer_types_send_their_values() {
    let mut app = boxed();
    let e = edge(&app, Vec3::new(20.0, 0.0, 20.0));
    let mut d = start(&mut app, vec![e], "FusionChamferCommand");
    let mut with = |t: usize| {
        if let Kind::Fillet { ctype, distance2, angle, flip, .. } = &mut d.kind {
            *ctype = t;
            *distance2 = "3 mm".into();
            *angle = "30 deg".into();
            *flip = true;
        }
        apply_commands(&app, &d).unwrap().remove(0).1
    };
    let p = with(0);
    assert!(p.get("distance2").is_none() && p.get("angle").is_none() && p.get("flip").is_none());
    let p = with(1);
    assert_eq!((p["distance2"].clone(), p["flip"].clone()), (json!("3 mm"), json!(true)));
    let p = with(2);
    assert_eq!(p["angle"], json!("30 deg"));
    assert!(p.get("distance2").is_none());
    // Applied and edited again, the chamfer keeps its type, angle and flip.
    let (id, p) = apply_commands(&app, &d).unwrap().remove(0);
    app.session.execute(&id, &p).unwrap();
    let ch = app.session.doc.features.last().unwrap().id;
    let e = crate::dialogs::for_feature(&app, ch, None).unwrap();
    assert!(matches!(&e.kind, Kind::Fillet { chamfer: true, ctype: 2, angle, flip: true, .. } if angle == "30 deg"), "{:?}", e.kind);
}
