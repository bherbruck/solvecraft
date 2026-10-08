//! Context menu tests: what the menus offer for each target, and that their items act.

use egui::pos2;
use serde_json::json;
use solvecraft_engine::Session;

use crate::context_menu::{self, Item, Target};
use crate::{Services, SolveApp};

fn sample_app() -> SolveApp {
    let mut s = Session::default();
    s.run_script(&solvecraft_engine::sample::script()).unwrap();
    SolveApp::new(s, Services::default())
}

fn find(items: &[Item], label: &str) -> Item {
    items
        .iter()
        .find(|i| i.label == label)
        .cloned()
        .unwrap_or_else(|| panic!("no `{label}` in {:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>()))
}

/// Run the item labelled `label` of the target's menu.
fn act(app: &mut SolveApp, t: &Target, label: &str) {
    let item = find(&context_menu::items(app, t), label);
    context_menu::run_item(app, &item, pos2(0.0, 0.0));
}

fn body(app: &SolveApp) -> String {
    app.session.model.state().bodies[0].name.clone()
}

#[test]
fn empty_space_offers_view_items_and_greys_out_undo() {
    let mut app = sample_app();
    app.session.undo.clear();
    let items = context_menu::items(&app, &Target::Viewport);
    assert!(!find(&items, "Undo").enabled, "nothing to undo");
    assert!(find(&items, "Fit").enabled);
    assert_eq!(context_menu::radial_items(&app).len(), 8);
    assert!(!context_menu::radial_items(&app)[0].enabled, "no command to repeat yet");
    // Hide Origin toggles.
    context_menu::run_item(&mut app, &find(&items, "Hide Origin"), pos2(0.0, 0.0));
    assert!(!app.ui.show_origin);
}

#[test]
fn face_menu_hides_and_finds_the_body() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("select.set", json!({"items": [{"type": "face", "body": b, "index": 0, "point": [0, 0, 0]}]})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    for l in ["Create Sketch", "Offset Plane", "Press Pull", "Extrude", "Measure", "Find in Browser", "Find in Timeline"] {
        assert!(find(&items, l).enabled, "{l}");
    }
    assert!(!find(&items, "Appearance…").enabled);
    context_menu::run_item(&mut app, &find(&items, "Find in Timeline"), pos2(0.0, 0.0));
    assert!(matches!(app.session.selection.as_slice(), [solvecraft_engine::Sel::Feature { .. }]));
    context_menu::run_item(&mut app, &find(&items, "Hide Body"), pos2(0.0, 0.0));
    assert_eq!(app.ui.hidden_bodies, vec![b]);
}

#[test]
fn body_menu_isolates_renames_locks_and_deletes() {
    let mut app = sample_app();
    app.run("PrimitiveBox", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let b = body(&app);
    let t = Target::Body { name: b.clone() };
    let items = context_menu::items(&app, &t);
    context_menu::run_item(&mut app, &find(&items, "Isolate"), pos2(0.0, 0.0));
    assert_eq!(app.ui.hidden_bodies.len(), 1, "the other body is hidden");
    assert!(!app.ui.hidden_bodies.contains(&b));
    // Rename through the rename box.
    context_menu::run_item(&mut app, &find(&items, "Rename"), pos2(10.0, 10.0));
    assert!(context_menu::finish_rename(&mut app, Some("Base"), true));
    assert!(app.session.model.state().body("Base").is_some());
    // Locked bodies can't be moved or deleted.
    let t = Target::Body { name: "Base".into() };
    act(&mut app, &t, "Lock");
    let items = context_menu::items(&app, &t);
    assert!(!find(&items, "Delete").enabled && !find(&items, "Move/Copy").enabled);
    context_menu::run_item(&mut app, &find(&items, "Unlock"), pos2(0.0, 0.0));
    let n = app.session.model.state().bodies.len();
    act(&mut app, &t, "Delete");
    assert_eq!(app.session.model.state().bodies.len(), n - 1);
}

#[test]
fn properties_measure_the_body() {
    let mut app = sample_app();
    let b = body(&app);
    let items = context_menu::items(&app, &Target::Body { name: b });
    context_menu::run_item(&mut app, &find(&items, "Properties"), pos2(0.0, 0.0));
    let (_, v) = app.menu.props.clone().unwrap();
    assert!(v["total"]["volume_mm3"].as_f64().unwrap() > 1000.0);
    assert!(v["total"]["mass_g"].as_f64().unwrap() > 0.0);
}

#[test]
fn sketch_menu_hides_and_moves_to_another_plane() {
    let mut app = sample_app();
    let id = app.session.doc.features.iter().find(|f| f.name == "Holes").unwrap().id;
    let t = Target::Sketch { id };
    let items = context_menu::items(&app, &t);
    let vis = crate::browser::sketch_visible(&app, id);
    context_menu::run_item(&mut app, &find(&items, if vis { "Hide" } else { "Show" }), pos2(0.0, 0.0));
    assert_eq!(crate::browser::sketch_visible(&app, id), !vis);
    context_menu::run_item(&mut app, &find(&items, "Edit Sketch"), pos2(0.0, 0.0));
    assert_eq!(app.session.active_sketch, Some(id));
    app.finish_sketch();
    context_menu::run_item(&mut app, &find(&items, "Redefine Sketch Plane"), pos2(0.0, 0.0));
    assert_eq!(app.tree.redefine, Some(id));
}

#[test]
fn sketch_entities_offer_the_constraints_that_fit() {
    let mut app = sample_app();
    app.run("SketchCreate", json!({"plane": "XY"})).unwrap();
    app.run("DrawPolyline", json!({"points": [[0, 0], [10, 1]]})).unwrap();
    app.run("DrawPolyline", json!({"points": [[0, 5], [10, 7]]})).unwrap();
    let ids: Vec<String> = {
        let st = app.session.model.state();
        let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
        ss.sketch.curves.iter().map(|c| c.id.clone()).collect()
    };
    app.run("select.set", json!({"items": ids.iter().map(|i| json!({"type": "sketch_curve", "id": i})).collect::<Vec<_>>()})).unwrap();
    let items = context_menu::items(&app, &Target::Viewport);
    let par = find(&items, "Parallel");
    assert!(find(&items, "Finish Sketch").enabled);
    context_menu::run_item(&mut app, &par, pos2(0.0, 0.0));
    let st = app.session.model.state();
    let ss = st.sketch(app.session.active_sketch.unwrap()).unwrap();
    assert!(ss.sketch.constraints.iter().any(|c| format!("{:?}", c.kind).contains("Parallel")), "{:?}", ss.sketch.constraints);
}

#[test]
fn component_menu_activates_grounds_and_pastes() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("FusionCreateComponentsFromBodiesCommand", json!({"bodies": [b]})).unwrap();
    let c = app.session.doc.components[0].id;
    let t = Target::Component { id: c };
    act(&mut app, &t, "Activate Component");
    assert_eq!(app.session.active_component, c);
    act(&mut app, &t, "Ground");
    assert!(app.session.doc.occurrence_of(c).unwrap().grounded);
    assert!(!find(&context_menu::items(&app, &t), "Move/Copy").enabled, "grounded");
    act(&mut app, &t, "Paste New");
    assert_eq!(app.session.doc.components.len(), 2);
}

#[test]
fn pick_runs_an_item_of_the_open_menu() {
    let mut app = sample_app();
    assert!(context_menu::pick(&mut app, "Fit").is_err(), "no menu open");
    context_menu::open_for(&mut app, pos2(500.0, 400.0), Target::Viewport);
    assert!(context_menu::pick(&mut app, "Nope").is_err());
    app.session.undo.clear();
    assert!(context_menu::pick(&mut app, "Undo").is_err(), "disabled");
    assert!(context_menu::pick(&mut app, "ui.origin").is_ok());
    assert!(app.viewport.context_menu.is_none(), "a pick closes the menu");
    assert!(!app.ui.show_origin);
}
