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
    app_with(json!([{"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}}]))
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
        {"command": "sketch.create", "params": {"plane": "XY"}},
        {"command": "sketch.circle.center", "params": {"center": [0, 0], "radius": 5}},
        {"command": "sketch.circle.center", "params": {"center": [20, 0], "radius": 5}},
        {"command": "sketch.finish", "params": {}},
    ]));
    let sk = app.session.doc.features.first().unwrap().id;
    let d = start(&mut app, vec![Sel::Profile { sketch: sk, index: 0 }, Sel::Profile { sketch: sk, index: 1 }], "solid.extrude");
    assert_eq!(d.inputs[0].items.len(), 2);
    assert!(d.focus, "the value box takes the keyboard");
    assert!(apply_commands(&app, &d).is_ok());
}

#[test]
fn edges_then_fillet_takes_them_with_chains() {
    let mut app = boxed();
    let (a, b) = (edge(&app, Vec3::new(0.0, 0.0, 10.0)), edge(&app, Vec3::new(20.0, 0.0, 20.0)));
    let d = start(&mut app, vec![a, b], "solid.fillet");
    assert_eq!(d.inputs[0].items.len(), 2, "box edges are not tangent to others");
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["edges"].as_array().unwrap().len(), 2);
}

#[test]
fn face_then_extrude_press_pulls_it() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let d = start(&mut app, vec![top], "solid.extrude");
    let cmds = apply_commands(&app, &d).unwrap();
    assert!(cmds[0].1.get("face").is_some());
}

#[test]
fn faces_then_shell_and_face_then_hole() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let front = face(&app, Vec3::new(20.0, 0.0, 10.0));
    let d = start(&mut app, vec![top.clone(), front], "solid.shell");
    assert_eq!(d.inputs[0].items.len(), 2);
    let d = start(&mut app, vec![top], "solid.hole");
    assert_eq!(d.inputs[0].items.len(), 1);
    let cmds = apply_commands(&app, &d).unwrap();
    assert!(cmds[0].1.get("position").is_some());
}

#[test]
fn line_and_profile_then_revolve_routes_each() {
    let mut app = app_with(json!([
        {"command": "sketch.create", "params": {"plane": "XZ"}},
        {"command": "sketch.rectangle.two_point", "params": {"p0": [10, 0], "p1": [20, 10]}},
        {"command": "sketch.line", "params": {"points": [[0, 0], [0, 30]], "ids": ["axis"]}},
    ]));
    let sk = app.session.active_sketch.unwrap();
    let d = start(&mut app, vec![Sel::SketchCurve { id: "axis".into() }, Sel::Profile { sketch: sk, index: 0 }], "solid.revolve");
    assert_eq!(d.inputs[0].items, vec![Sel::Profile { sketch: sk, index: 0 }]);
    assert_eq!(d.inputs[1].items, vec![Sel::SketchCurve { id: "axis".into() }]);
}

#[test]
fn face_then_move_takes_the_body_and_odd_items_are_ignored() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let d = start(&mut app, vec![top, Sel::Axis { name: "X".into() }], "solid.move");
    assert_eq!(d.inputs[0].items, vec![Sel::Body { name: "Body1".into() }]);
    assert!(matches!(d.kind, Kind::Move { .. }));
}

#[test]
fn hole_at_sketch_points() {
    let mut app = app_with(json!([
        {"command": "solid.box", "params": {"length": 40, "width": 30, "height": 20}},
        {"command": "sketch.create", "params": {"plane": {"face": [20, 15, 20]}}},
        {"command": "sketch.point", "params": {"point": [10, 10], "id": "h1"}},
        {"command": "sketch.point", "params": {"point": [30, 20], "id": "h2"}},
        {"command": "sketch.finish", "params": {}},
    ]));
    let sk = app.session.doc.features.iter().find(|f| matches!(f.kind, solvecraft_engine::doc::FeatureKind::Sketch { .. })).unwrap().id;
    app.start("solid.hole");
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
        {"command": "solid.box", "params": {"length": 60, "width": 40, "height": 10}},
        {"command": "solid.hole", "params": {"position": [10, 10, 10], "diameter": 5}}
    ]));
    let hole = app.session.doc.features.last().unwrap().id;
    // Picked in the timeline before the command: the objects become features.
    let mut d = start(&mut app, vec![Sel::Feature { id: hole }], "solid.pattern.rectangular");
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
    let mut d = start(&mut app, vec![], "solid.pattern.circular");
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
        {"command": "solid.box", "params": {"length": 60, "width": 40, "height": 10}},
        {"command": "solid.hole", "params": {"position": [10, 10, 10], "diameter": 5}}
    ]));
    let hole = app.session.doc.features.last().unwrap().id;
    let mut d = start(&mut app, vec![Sel::Feature { id: hole }], "solid.mirror");
    assert_eq!(d.inputs[0].accept, crate::selection::FEATURES);
    d.inputs[1].items = vec![Sel::Plane { name: "YZ".into() }];
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["features"], json!(["Hole1"]));
}

#[test]
fn chamfer_types_send_their_values() {
    let mut app = boxed();
    let e = edge(&app, Vec3::new(20.0, 0.0, 20.0));
    let mut d = start(&mut app, vec![e], "solid.chamfer");
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

/// Apply a dialog's commands to the session; the new feature's id.
fn apply(app: &mut SolveApp, d: &Dialog) -> u64 {
    for (id, p) in apply_commands(app, d).unwrap() {
        app.session.execute(&id, &p).unwrap();
    }
    app.session.doc.features.last().unwrap().id
}

#[test]
fn shell_direction_and_tangent_chain_round_trip() {
    let mut app = boxed();
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let mut d = start(&mut app, vec![top], "solid.shell");
    // Tangent Chain is on, as in Fusion; Direction Outside.
    assert_eq!(d.extra.get("tangent_chain"), Some(&json!(true)));
    d.extra.insert("direction".into(), json!("outside"));
    let p = apply_commands(&app, &d).unwrap().remove(0).1;
    assert_eq!((p["direction"].clone(), p["tangent_chain"].clone()), (json!("outside"), json!(true)));
    let id = apply(&mut app, &d);
    let e = crate::dialogs::for_feature(&app, id, None).unwrap();
    assert_eq!(e.extra.get("direction"), Some(&json!("outside")));
    assert_eq!(e.extra.get("tangent_chain"), Some(&json!(true)));
}

#[test]
fn variable_fillet_round_trip() {
    let mut app = boxed();
    let e0 = edge(&app, Vec3::new(40.0, 15.0, 20.0));
    let mut d = start(&mut app, vec![e0], "solid.fillet");
    d.inputs[0].items.truncate(1);
    d.extra.insert("type".into(), json!("variable"));
    d.extra.insert("radius2".into(), json!("4 mm"));
    let p = apply_commands(&app, &d).unwrap().remove(0).1;
    assert_eq!((p["type"].clone(), p["radius2"].clone()), (json!("variable"), json!("4 mm")));
    assert!(p["start"].is_array(), "{p}");
    let id = apply(&mut app, &d);
    let e = crate::dialogs::for_feature(&app, id, None).unwrap();
    assert_eq!(e.extra.get("type"), Some(&json!("variable")));
    assert_eq!(e.extra.get("radius2"), Some(&json!("4 mm")));
}

#[test]
fn newer_options_reach_their_commands() {
    let mut app = boxed();
    // Thread: Modeled.
    let side = face(&app, Vec3::new(20.0, 0.0, 10.0));
    let mut d = start(&mut app, vec![side.clone()], "solid.thread");
    d.extra.insert("modeled".into(), json!(true));
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["modeled"], json!(true));
    // Combine into a new component.
    app.dialog = None;
    let mut d = start(&mut app, vec![], "solid.combine");
    d.extra.insert("new_component".into(), json!(true));
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.inputs[1].items = vec![Sel::Body { name: "Body2".into() }];
    assert_eq!(apply_commands(&app, &d).unwrap()[0].1["new_component"], json!(true));
    // Hole: To Object sends the place to drill to instead of a depth.
    app.dialog = None;
    let top = face(&app, Vec3::new(20.0, 15.0, 20.0));
    let mut d = start(&mut app, vec![top], "solid.hole");
    d.inputs.push(crate::selection::SelInput::new("To Object", crate::selection::FACES | crate::selection::VERTICES, false));
    d.inputs[1].items = vec![face(&app, Vec3::new(20.0, 15.0, 0.0))];
    let p = apply_commands(&app, &d).unwrap().remove(0).1;
    assert!(p["to"].is_array() && p.get("depth").is_none(), "{p}");
}

#[test]
fn boundary_fill_finds_the_cells_and_fills_the_ticked_ones() {
    let mut app = app_with(json!([
        {"command": "solid.box", "params": {"length": 20, "width": 20, "height": 20}},
        {"command": "solid.box", "params": {"length": 20, "width": 20, "height": 20, "corner": [10, 10, 0], "operation": "new"}}
    ]));
    let mut d = start(&mut app, vec![], "SurfaceSculpt");
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.dialog = Some(d);
    // The dialog finds the cells on its next frame.
    let ctx = egui::Context::default();
    let _ = ctx.run_ui(Default::default(), |ui| crate::dialogs::show(&mut app, ui.ctx()));
    let mut d = app.dialog.take().unwrap();
    let Kind::BoundaryFill { cells, .. } = &mut d.kind else { panic!() };
    // Only A, only B, and both.
    assert_eq!(cells.len(), 3, "{cells:?}");
    assert!(cells.iter().any(|c| c.1 == "Inside Body1, Body2"));
    // Fill just the overlap.
    for c in cells.iter_mut() {
        c.2 = c.1 == "Inside Body1, Body2";
    }
    let id = apply(&mut app, &d);
    assert!(app.session.model.result(id).is_some_and(|r| r.error.is_none()));
    // A new body filling the 10 x 10 x 20 overlap.
    let vols: Vec<f64> = app.session.model.state().bodies.iter().map(|b| b.mesh().measure().volume).collect();
    assert!(vols.len() == 3 && vols.iter().any(|v| (v - 2000.0).abs() < 1.0), "{vols:?}");
}

#[test]
fn align_snaps_to_centres_and_edge_middles() {
    let mut app = boxed();
    let mut d = start(&mut app, vec![], "solid.align");
    d.inputs[0].items = vec![Sel::Body { name: "Body1".into() }];
    d.inputs[1].items = vec![face(&app, Vec3::new(20.0, 15.0, 0.0))];
    d.inputs[2].items = vec![edge(&app, Vec3::new(20.0, 0.0, 20.0))];
    d.extra.insert("from_snap".into(), json!("center"));
    let p = apply_commands(&app, &d).unwrap().remove(0).1;
    assert_eq!(p["from"]["snap"], json!("face_center"));
    assert_eq!(p["to"]["snap"], json!("edge_mid"));
}
