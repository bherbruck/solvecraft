use serde_json::json;
use solvecraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, face_sel, preview_commands};

/// Components A (grounded) and B: a 20 x 20 x 10 box each, B 30 mm along X.
fn two_boxes() -> SolveApp {
    let mut s = Session::default();
    for (name, x) in [("A", 0), ("B", 30)] {
        s.execute("component.activate", &json!({"component": "root"})).unwrap();
        s.execute("FusionCreateNewComponentCommand", &json!({ "name": name })).unwrap();
        s.execute("PrimitiveBox", &json!({"length": 20, "width": 20, "height": 10, "corner": [x, 0, 0]})).unwrap();
    }
    s.execute("component.activate", &json!({"component": "root"})).unwrap();
    s.execute("occurrence.ground", &json!({"occurrence": "A:1", "grounded": true})).unwrap();
    SolveApp::new(s, Services::default())
}

fn occ(app: &SolveApp, name: &str) -> u64 {
    let c = app.session.doc.find_component(name).unwrap();
    app.session.doc.occurrence_of(c).unwrap().id
}

/// Two face picks make a joint between the components that own them; offsets across the face
/// turn component 1's origin into a frame; limits follow the type.
#[test]
fn joint_from_two_snaps() {
    let mut app = two_boxes();
    app.start("JointAssembleCmdNew");
    let mut d = app.dialog.clone().unwrap();
    let (fa, fb) = (face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap(), face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    assert!(apply_commands(&app, &d).is_err(), "no snaps yet");
    d.pick(&app.session, fa);
    assert_eq!(d.active, 1, "component 2 is next");
    d.pick(&app.session, fb);
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.fit_limits();
        f.limits[0] = (true, "-45 deg".into(), "45 deg".into());
    }
    let c = apply_commands(&app, &d).unwrap();
    let (id, p) = &c[0];
    assert_eq!(id, "JointAssembleCmdNew");
    assert_eq!(p["type"], "revolute");
    assert_eq!(p["a"]["occurrence"], occ(&app, "A"));
    assert_eq!(p["b"]["occurrence"], occ(&app, "B"));
    assert!(p["a"]["face"].is_array() && p["b"]["face"].is_array(), "{p}");
    assert_eq!(p["limits"], json!([["-45 deg", "45 deg"]]));
    // The bottom face of B snaps at its centre, z down.
    let sb = snap_of(&app.session, &d.inputs[1].items[0]).unwrap();
    assert!(sb.at.dist(Vec3::new(40.0, 10.0, 0.0)) < 1e-6 && sb.z.dist(-Vec3::Z) < 1e-9, "{sb:?}");
    // Offset X: an explicit frame 5 mm along the face.
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.offset[0] = "5 mm".into();
    }
    let p = apply_commands(&app, &d).unwrap()[0].1.clone();
    let o = &p["a"]["frame"]["origin"];
    let at = Vec3::new(o[0].as_f64().unwrap(), o[1].as_f64().unwrap(), o[2].as_f64().unwrap());
    assert!((at.dist(Vec3::new(10.0, 10.0, 10.0)) - 5.0).abs() < 1e-9, "{p}");
    // OK makes it.
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    assert_eq!(app.session.doc.assembly.joints.len(), 1);
}

/// The preview moves B where the joint puts it and plays the motion while animating.
#[test]
fn joint_and_drive_preview_on_the_placed_model() {
    let mut app = two_boxes();
    app.start("JointAssembleCmdNew");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    let colors = crate::preview::Colors { body: [1, 1, 1, 255], added: [2, 2, 2, 255], removed: [3, 3, 3, 100], edge: [0, 0, 0, 255] };
    let cmds = preview_commands(&app, &d).unwrap();
    let b = world_preview(&app.session, &cmds, colors).unwrap().unwrap();
    assert_eq!(b.replaced, vec!["Body2".to_string()], "B moves");
    assert!(app.session.doc.assembly.joints.is_empty(), "the preview changes nothing");
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.animate = true;
        f.phase = 0.25;
    }
    let cmds = preview_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].1["values"], json!(["90 deg"]), "a quarter turn into the animation");
    assert!(apply_commands(&app, &d).unwrap()[0].1.get("values").is_none(), "OK makes the joint at rest");
    // Drive: the dialog's value turns the joint, the preview shows it.
    for (id, p) in apply_commands(&app, &d).unwrap() {
        app.run(&id, p).unwrap();
    }
    app.start("FusionMoveJointsCommand");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Assembly(Asm::Drive { values, .. }) = &mut d.kind {
        assert_eq!(values, &vec!["0 deg".to_string()]);
        values[0] = "30 deg".into();
    }
    let cmds = apply_commands(&app, &d).unwrap();
    assert_eq!(cmds[0].0, "FusionMoveJointsCommand");
    let b = world_preview(&app.session, &cmds, colors).unwrap().unwrap();
    assert_eq!(b.replaced, vec!["Body2".to_string()]);
    // Features preview as before while nothing is placed away from its frame, and on the
    // placed model once something is (B moved by its joint).
    assert!(world_preview(&two_boxes().session, &[("Extrude".into(), json!({}))], colors).is_none());
    assert!(world_preview(&app.session, &[("Extrude".into(), json!({}))], colors).is_some());
}

/// Edit Joint changes type, alignment and limits of the joint in place.
#[test]
fn edit_joint_runs_edit_and_limits() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run(
        "JointAssembleCmdNew",
        json!({"type": "slider", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}),
    )
    .unwrap();
    let id = app.session.doc.assembly.joints[0].id;
    let mut d = edit_joint(&app, id).unwrap();
    assert!(d.inputs.is_empty());
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        assert_eq!(TYPES[f.kind], "slider");
        f.limits[0] = (true, "0 mm".into(), "15 mm".into());
        f.offset[2] = "2 mm".into();
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), ["joint.edit", "joint.limits"]);
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let j = &app.session.doc.assembly.joints[0];
    assert_eq!(j.limits[0], Some((0.0, 15.0)));
    assert!((j.offset - 2.0).abs() < 1e-12);
}

/// As-built joints and rigid groups take components from body picks; New Component makes one
/// from bodies when bodies are picked.
#[test]
fn component_picks_become_occurrences() {
    let mut app = two_boxes();
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("JointAsBuiltCmd");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["a"], occ(&app, "A"));
    assert_eq!(c[0].1["b"], occ(&app, "B"));
    app.session.selection = vec![Sel::Body { name: "Body1".into() }, Sel::Body { name: "Body2".into() }];
    app.start("RigidGroupCmd");
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].1["occurrences"], json!([occ(&app, "A"), occ(&app, "B")]));
    app.dialog = None;
    app.start("FusionCreateNewComponentCommand");
    let c = apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap();
    assert_eq!(c[0].0, "FusionCreateNewComponentCommand");
}

/// Round edges snap to their centre; straight ones are not circles.
#[test]
fn circles_from_edge_polylines() {
    let pts: Vec<Vec3> =
        (0..=24).map(|i| f64::from(i) / 24.0 * std::f64::consts::TAU).map(|t| Vec3::new(5.0 + 3.0 * t.cos(), 2.0 + 3.0 * t.sin(), 7.0)).collect();
    let (c, n) = circle_of(&pts).unwrap();
    assert!(c.dist(Vec3::new(5.0, 2.0, 7.0)) < 1e-9 && n.cross(Vec3::Z).len() < 1e-9);
    let line: Vec<Vec3> = (0..10).map(|i| Vec3::new(f64::from(i), 0.0, 0.0)).collect();
    assert!(circle_of(&line).is_none());
}

/// The centre of the first text shape reading `text` in a frame's output.
fn text_at(out: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    out.shapes.iter().find_map(|c| match &c.shape {
        egui::Shape::Text(t) if t.galley.text() == text => Some(t.pos + t.galley.rect.center().to_vec2()),
        _ => None,
    })
}

/// OK pressed while the animated preview is computing and released once it is not (the busy
/// spinner coming and going between the two frames) still applies the joint.
#[test]
fn ok_click_survives_a_busy_preview() {
    let mut app = two_boxes();
    app.start("JointAssembleCmdNew");
    let mut d = app.dialog.take().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 0.0)).unwrap());
    if let Kind::Assembly(Asm::Joint(f)) = &mut d.kind {
        f.kind = 1;
        f.animate = true;
    }
    app.dialog = Some(d);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, busy: bool, events: Vec<egui::Event>| {
        app.preview.busy = busy;
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()));
        // Nothing is painted: drop the texture changes (an unapplied delta panics on drop).
        out.textures_delta.clear();
        out
    };
    frame(&mut app, false, vec![]);
    let out = frame(&mut app, false, vec![]);
    let ok = text_at(&out, "OK").expect("an OK button");
    let button = |pressed| egui::Event::PointerButton { pos: ok, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
    frame(&mut app, true, vec![egui::Event::PointerMoved(ok)]);
    frame(&mut app, true, vec![button(true)]);
    frame(&mut app, false, vec![button(false)]);
    assert_eq!(app.session.doc.assembly.joints.len(), 1, "the click applied the joint");
    assert!(app.dialog.is_none());
}

/// Two top faces mate face to face as in Fusion: B ends up upside down on A, not inside it;
/// Flip turns it back over (into A).
#[test]
fn face_snaps_mate_face_to_face() {
    let mut app = two_boxes();
    app.start("JointAssembleCmdNew");
    let mut d = app.dialog.clone().unwrap();
    d.pick(&app.session, face_sel(&app.session, Vec3::new(10.0, 10.0, 10.0)).unwrap());
    d.pick(&app.session, face_sel(&app.session, Vec3::new(40.0, 10.0, 10.0)).unwrap());
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["flip"], false, "mating is the engine's default; no flip");
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
    let st = app.session.world_state();
    let b = st.body("Body2").unwrap().mesh().bounds();
    assert!((b.min.z - 10.0).abs() < 1e-6 && (b.max.z - 20.0).abs() < 1e-6, "{b:?}");
    // Edit Joint shows the mate unflipped; Flip puts B back into A's volume.
    let id = app.session.doc.assembly.joints[0].id;
    let mut e = edit_joint(&app, id).unwrap();
    if let Kind::Assembly(Asm::Joint(f)) = &mut e.kind {
        assert!(!f.flip);
        f.flip = true;
    }
    for (id, p) in apply_commands(&app, &e).unwrap() {
        app.run(&id, p).unwrap();
    }
    let b = app.session.world_state().body("Body2").unwrap().mesh().bounds();
    assert!((b.max.z - 10.0).abs() < 1e-6, "{b:?}");
}

/// Drive Joints takes the keyboard: typing a value and Enter drives the joint.
#[test]
fn drive_value_takes_the_keyboard() {
    let mut app = two_boxes();
    let (a, b) = (occ(&app, "A"), occ(&app, "B"));
    app.run(
        "JointAssembleCmdNew",
        json!({"type": "revolute", "a": {"occurrence": a, "face": [10, 10, 10]}, "b": {"occurrence": b, "face": [40, 10, 0]}}),
    )
    .unwrap();
    app.start("FusionMoveJointsCommand");
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
    let frame = |app: &mut SolveApp, events: Vec<egui::Event>| {
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        ctx.run_ui(input, |ui| crate::dialogs::show(app, ui.ctx()))
    };
    frame(&mut app, vec![]);
    frame(&mut app, vec![]);
    frame(&mut app, vec![egui::Event::Text("90".into())]);
    let key = |pressed| egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
    frame(&mut app, vec![key(true), key(false)]);
    frame(&mut app, vec![]);
    let j = &app.session.doc.assembly.joints[0];
    assert!((j.values[0] - 90f64.to_radians()).abs() < 1e-9, "{:?}", j.values);
    assert!(app.dialog.is_none());
}
