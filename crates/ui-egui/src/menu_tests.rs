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
    assert!(!find(&items, "Appearance").enabled);
    context_menu::run_item(&mut app, &find(&items, "Find in Timeline"), pos2(0.0, 0.0));
    assert!(matches!(app.session.selection.as_slice(), [solvecraft_engine::Sel::Feature { .. }]));
    context_menu::run_item(&mut app, &find(&items, "Show/Hide"), pos2(0.0, 0.0));
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
    context_menu::run_item(&mut app, &find(&items, "Show/Hide"), pos2(0.0, 0.0));
    assert_eq!(crate::browser::sketch_visible(&app, id), !vis);
    context_menu::run_item(&mut app, &find(&items, "Show Dimension"), pos2(0.0, 0.0));
    assert_eq!(app.ui.shown_dims, vec![id]);
    assert!(find(&context_menu::items(&app, &t), "Hide Dimension").enabled);
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
    act(&mut app, &t, "Activate");
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

#[test]
fn folder_and_group_menus_make_rename_and_drop_groups() {
    let mut app = sample_app();
    app.run("PrimitiveBox", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let names: Vec<String> = app.session.model.state().bodies.iter().map(|b| b.name.clone()).collect();
    let items: Vec<_> = names.iter().map(|n| json!({"type": "body", "name": n})).collect();
    app.run("select.set", json!({ "items": items })).unwrap();
    let folder = Target::Folder { component: 0, folder: "bodies".into() };
    act(&mut app, &folder, "Group Selected");
    let g = app.session.doc.browser_groups[0].clone();
    assert_eq!(g.items.len(), 2);
    // A body's menu offers grouping too while several are selected.
    assert!(context_menu::items(&app, &Target::Body { name: names[0].clone() }).iter().any(|i| i.label == "Group Selected"));
    let t = Target::Group { id: g.id };
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Parts"), true));
    assert_eq!(app.session.doc.browser_groups[0].name, "Parts");
    act(&mut app, &t, "Show/Hide");
    assert_eq!(app.ui.hidden_bodies.len(), 2);
    act(&mut app, &t, "Ungroup");
    assert!(app.session.doc.browser_groups.is_empty());
    act(&mut app, &folder, "New Group");
    assert_eq!(app.session.doc.browser_groups[0].items.len(), 0);
}

#[test]
fn browser_folds_are_kept_in_the_preferences() {
    let mut app = sample_app();
    app.tree.collapsed.insert("c0/sketches".into());
    app.tree.expanded.insert("origin".into());
    let prefs = app.prefs();
    let mut other = sample_app();
    other.load_prefs(&prefs);
    assert!(other.tree.collapsed.contains("c0/sketches") && other.tree.expanded.contains("origin"));
}

#[test]
fn delete_key_handles_every_kind_and_asks_about_dependents() {
    use solvecraft_engine::Sel;
    let mut app = sample_app();
    app.run("ConstructionPlaneOffsetFromPlaneCommand", json!({"base": "XY", "offset": 10})).unwrap();
    app.run("PrimitiveBox", json!({"length": 5, "width": 5, "height": 5, "corner": [200, 0, 0]})).unwrap();
    let n = app.session.doc.features.len();
    // A body and a construction plane together: one undo step, no question.
    let b = app.session.model.state().bodies.last().unwrap().name.clone();
    app.run("select.set", json!({"items": [{"type": "body", "name": b}, {"type": "plane", "name": "Plane1"}]})).unwrap();
    crate::delete::delete_selection(&mut app);
    assert!(app.menu.confirm.is_none());
    assert!(app.session.model.state().body(&b).is_none());
    assert!(app.session.doc.find_feature("Plane1").is_none());
    app.run("UndoCommand", json!({})).unwrap();
    assert_eq!(app.session.doc.features.len(), n);
    // A sketch with features built on it: the confirmation lists them first.
    let sk = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    app.run("select.set", json!({"items": [Sel::Feature { id: sk }]})).unwrap();
    crate::delete::delete_selection(&mut app);
    let pending = app.menu.confirm.clone().expect("asks first");
    assert!(pending.report["deleted"].as_array().unwrap().len() > 1);
    assert!(crate::delete::confirm(&mut app, false));
    assert_eq!(app.session.doc.features.len(), n, "cancel keeps everything");
    crate::delete::delete_selection(&mut app);
    crate::delete::confirm(&mut app, true);
    assert!(app.session.doc.features.len() < n);
    app.run("UndoCommand", json!({})).unwrap();
    assert_eq!(app.session.doc.features.len(), n);
    // A face alone: not available yet, nothing changes.
    let b0 = app.session.model.state().bodies[0].name.clone();
    app.run("select.set", json!({"items": [{"type": "face", "body": b0, "index": 0, "point": [0, 0, 0]}]})).unwrap();
    crate::delete::delete_selection(&mut app);
    assert!(app.status.as_ref().is_some_and(|s| s.0.contains("not available yet")), "{:?}", app.status);
    assert_eq!(app.session.doc.features.len(), n);
}

#[test]
fn menu_delete_items_use_the_same_path() {
    let mut app = sample_app();
    let b = body(&app);
    app.run("FusionCreateComponentsFromBodiesCommand", json!({"bodies": [b]})).unwrap();
    let c = app.session.doc.components[0].id;
    act(&mut app, &Target::Component { id: c }, "Delete");
    assert!(app.session.doc.components.is_empty());
    // A sketch from the browser: asks, since its features go too.
    let id = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    act(&mut app, &Target::Sketch { id }, "Delete");
    assert!(app.menu.confirm.is_some());
}

#[test]
fn canvas_menu_hides_renames_calibrates_and_deletes() {
    let mut app = sample_app();
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(400u32.to_be_bytes());
    png.extend(200u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let data = solvecraft_engine::doc::canvas::base64_encode(&png);
    let id = app.run("FusionAddCanvasCommand", json!({"data": data, "plane": "XY", "width": 80})).unwrap()["canvas"].as_u64().unwrap();
    let t = Target::Canvas { id };
    act(&mut app, &t, "Show/Hide");
    assert!(!app.session.doc.canvases[0].visible);
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Photo"), true));
    assert_eq!(app.session.doc.canvases[0].name, "Photo");
    act(&mut app, &t, "Calibrate");
    let p = app.tree.canvas_panel.clone().unwrap();
    assert!(p.calibrate && (p.distance - 80.0).abs() < 1e-9, "starts from the image's width");
    act(&mut app, &t, "Delete");
    assert!(app.session.doc.canvases.is_empty());
    app.run("UndoCommand", json!({})).unwrap();
    assert_eq!(app.session.doc.canvases.len(), 1);
    // A canvas picked in the browser goes with the Delete key.
    app.tree.picked_canvases = vec![id];
    crate::delete::delete_selection(&mut app);
    assert!(app.session.doc.canvases.is_empty());
}

/// The canvas and browser menus keep Fusion's items in Fusion's order (the ones we have).
#[test]
fn menus_follow_fusion_order() {
    let mut app = sample_app();
    let b = body(&app);
    let order = |items: &[Item], want: &[&str]| {
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        let pos: Vec<usize> = want.iter().map(|w| labels.iter().position(|l| l == w).unwrap_or_else(|| panic!("no `{w}` in {labels:?}"))).collect();
        assert!(pos.windows(2).all(|p| p[0] < p[1]), "{want:?} out of order in {labels:?}");
    };
    app.run("select.set", json!({"items": [{"type": "face", "body": b, "index": 0, "point": [0, 0, 0]}]})).unwrap();
    let face = context_menu::items(&app, &Target::Viewport);
    order(
        &face,
        &[
            "Create Sketch",
            "Extrude",
            "Offset Plane",
            "Shell",
            "Edit Feature",
            "Edit Profile Sketch",
            "Appearance",
            "Properties",
            "Delete",
            "Show/Hide",
            "Find in Browser",
        ],
    );
    assert_eq!(find(&face, "Extrude").shortcut, "E");
    assert!(find(&face, "Edit Profile Sketch").enabled, "the plate's extrude has a sketch");
    let body_menu = context_menu::items(&app, &Target::Body { name: b.clone() });
    order(
        &body_menu,
        &[
            "Move/Copy",
            "Create Components from Bodies",
            "Physical Material",
            "Appearance",
            "Properties",
            "Save As Mesh",
            "Delete",
            "Remove",
            "Rename",
            "Show/Hide",
            "Isolate",
        ],
    );
    let id = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    let sketch = context_menu::items(&app, &Target::Sketch { id });
    order(
        &sketch,
        &[
            "Extrude",
            "Edit Sketch",
            "Redefine Sketch Plane",
            "Export DXF…",
            "Delete",
            "Rename",
            "Look At",
            "Hide Profile",
            "Show Dimension",
            "Show/Hide",
            "Find in Timeline",
        ],
    );
    let folder = context_menu::items(&app, &Target::Folder { component: 0, folder: "bodies".into() });
    order(&folder, &["Create Components from Bodies", "New Group", "Show/Hide", "Show All"]);
    let origin = context_menu::items(&app, &Target::Origin);
    order(&origin, &["Show/Hide", "Show All", "Hide Planes", "Hide Axes"]);
    context_menu::run_item(&mut app, &find(&origin, "Hide Planes"), pos2(0.0, 0.0));
    assert!(["XY", "XZ", "YZ"].iter().all(|p| app.ui.hidden_origin.iter().any(|h| h == p)));
    app.run("select.clear", json!({})).unwrap();
    let empty = context_menu::items(&app, &Target::Viewport);
    order(&empty, &["Pan", "Zoom", "Orbit", "Show All", "Unisolate", "Extrude", "Fillet"]);
    context_menu::run_item(&mut app, &find(&empty, "Pan"), pos2(0.0, 0.0));
    assert!(matches!(app.viewport.nav, Some(crate::viewport::NavMode::Pan)));
}

#[test]
fn v_toggles_what_is_selected() {
    let mut app = sample_app();
    let b = body(&app);
    let sk = app.session.doc.features.iter().find(|f| f.name == "Base").unwrap().id;
    app.run("select.set", json!({"items": [{"type": "body", "name": b}, {"type": "plane", "name": "XY"}, {"type": "feature", "id": sk}]})).unwrap();
    crate::browser::set_sketch_visible(&mut app, sk, true);
    context_menu::toggle_visibility(&mut app);
    assert!(app.ui.hidden_bodies.contains(&b) && app.ui.hidden_origin.iter().any(|h| h == "XY"));
    assert!(!crate::browser::sketch_visible(&app, sk));
    context_menu::toggle_visibility(&mut app);
    assert!(app.ui.hidden_bodies.is_empty() && app.ui.hidden_origin.is_empty());
    assert!(crate::browser::sketch_visible(&app, sk));
    // Components picked in the browser hide their bodies.
    app.run("FusionCreateComponentsFromBodiesCommand", json!({"bodies": [b.clone()]})).unwrap();
    app.run("select.clear", json!({})).unwrap();
    app.tree.picked_components = vec![app.session.doc.components[0].id];
    context_menu::toggle_visibility(&mut app);
    assert_eq!(app.ui.hidden_bodies, vec![b]);
    assert_eq!(find(&context_menu::items(&app, &Target::Body { name: body(&app) }), "Show/Hide").shortcut, "V");
}

#[test]
fn named_views_save_restore_rename_and_delete() {
    let mut app = sample_app();
    act(&mut app, &Target::NamedViews, "New Named View");
    assert_eq!(app.session.doc.named_views.len(), 1);
    let saved = app.cam;
    app.cam.set_view(solvecraft_engine::render::StandardView::Top);
    let t = Target::NamedView { name: "Named View1".into() };
    act(&mut app, &t, "Restore");
    assert_eq!(app.cam_anim.map(|a| a.to), Some(saved), "animates back to the saved camera");
    act(&mut app, &t, "Rename");
    assert!(context_menu::finish_rename(&mut app, Some("Front detail"), true));
    let t = Target::NamedView { name: "Front detail".into() };
    act(&mut app, &t, "Delete");
    assert!(app.session.doc.named_views.is_empty());
    // Standard views restore but can't be renamed or deleted.
    let top = context_menu::items(&app, &Target::NamedView { name: "Top".into() });
    assert!(!find(&top, "Delete").enabled);
    context_menu::run_item(&mut app, &find(&top, "Restore"), pos2(0.0, 0.0));
    assert!(app.cam_anim.is_some());
}

#[test]
fn units_menu_changes_the_design_units_and_new_designs_take_the_preference() {
    let mut app = sample_app();
    let items = context_menu::items(&app, &Target::Units);
    assert_eq!(items.len(), 5);
    act(&mut app, &Target::Units, "in");
    assert_eq!(app.session.doc.units, "in");
    assert!(crate::prefs::show_mm(&app, 25.4, 1).starts_with("1.00 in"));
    app.preferences.default_units = "cm".into();
    crate::documents::new_design(&mut app);
    assert_eq!(app.session.doc.units, "cm");
    assert!(!app.session.is_dirty(), "a fresh design stays unmodified");
}
