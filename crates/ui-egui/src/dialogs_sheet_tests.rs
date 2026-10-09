use serde_json::json;
use solvecraft_engine::Session;

use super::*;
use crate::Services;
use crate::dialogs::{apply_commands, for_feature as edit_dialog};

/// A 100 x 60 base flange on XY (2.5 mm steel).
fn plate() -> SolveApp {
    let mut s = Session::default();
    s.execute("sketch.create", &json!({"plane": "XY", "name": "Base"})).unwrap();
    s.execute("sketch.rectangle.two_point", &json!({"p0": [0, 0], "p1": [100, 60]})).unwrap();
    s.execute("sketch.finish", &json!({})).unwrap();
    SolveApp::new(s, Services::default())
}

fn run_all(app: &mut SolveApp, c: Vec<(String, Value)>) {
    for (id, p) in c {
        app.run(&id, p).unwrap();
    }
}

/// The picks decide the flange: a profile makes the base, edges then edge flanges with the
/// dialog's height, angle and bend position; Edit Feature brings the edges back.
#[test]
fn flange_follows_the_picks() {
    let mut app = plate();
    let sk = app.session.doc.features[0].id;
    app.session.selection = vec![Sel::Profile { sketch: sk, index: 0 }];
    app.start("sheet.flange");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["type"], "base");
    assert_eq!(c[0].1["profiles"], json!([0]));
    run_all(&mut app, c);
    assert_eq!(app.session.model.state().sheets.len(), 1);
    // Four edge flanges at once, 20 high, outside.
    let edges: Vec<Sel> = [[50.0, 0.0, 2.5], [50.0, 60.0, 2.5], [0.0, 30.0, 2.5], [100.0, 30.0, 2.5]]
        .iter()
        .filter_map(|p| edge_sel(&app.session, Vec3::new(p[0], p[1], p[2])))
        .collect();
    assert_eq!(edges.len(), 4);
    app.session.selection = edges;
    app.start("sheet.flange");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Sheet(Sm::Flange { height, position, .. }) = &mut d.kind {
        *height = "20 mm".into();
        *position = 1;
    }
    assert_eq!(d.primary().map(|p| p.0), Some("Height"), "the arrow drives the height");
    assert!(arrow(&app, &d).is_some_and(|n| n.dist(Vec3::Z) < 1e-9), "edge flanges rise out of the sheet");
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["type"], "edge");
    assert_eq!(c[0].1["edges"].as_array().map(Vec::len), Some(4));
    assert_eq!(c[0].1["position"], "outside");
    run_all(&mut app, c);
    let f = app.session.doc.features.last().unwrap().clone();
    let e = edit_dialog(&app, f.id, None).unwrap();
    assert_eq!(e.inputs[0].items.len(), 4);
    assert!(matches!(&e.kind, Kind::Sheet(Sm::Flange { ty: 1, position: 1, .. })));
    // Rebuilt in place.
    let c = apply_commands(&app, &e).unwrap();
    assert_eq!(c[0].0, "timeline.redefine");
    assert_eq!(c[0].1["params"]["height"], "20 mm");
    // Flat pattern: four bends; Unfold previews.
    let flat = app.session.execute("sheet.flat_pattern", &json!({})).unwrap();
    assert_eq!(flat["bends"].as_array().map(Vec::len), Some(4), "{flat}");
    app.dialog = None;
    app.start("sheet.unfold");
    assert_eq!(apply_commands(&app, app.dialog.as_ref().unwrap()).unwrap()[0].0, "sheet.unfold");
}

/// Hem takes edges and an optional gap; rules make and pick a rule with its expressions.
#[test]
fn hem_and_rules() {
    let mut app = plate();
    app.run("sheet.flange", json!({"sketch": "Base"})).unwrap();
    app.session.selection = edge_sel(&app.session, Vec3::new(50.0, 0.0, 2.5)).into_iter().collect();
    app.start("sheet.hem");
    let d = app.dialog.clone().unwrap();
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["length"], "5 mm");
    assert!(c[0].1.get("gap").is_none(), "the rule's gap");
    run_all(&mut app, c);
    app.dialog = None;
    app.start("sheet.manage_rules");
    let mut d = app.dialog.clone().unwrap();
    if let Kind::Sheet(Sm::Rules { pick, name, values, loaded, .. }) = &mut d.kind {
        *pick = 1;
        *loaded = Some(1);
        *name = "Alu".into();
        *values = vec![
            "1.5 mm".into(),
            "0.4".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
            "Thickness".into(),
        ];
    }
    let c = apply_commands(&app, &d).unwrap();
    assert_eq!(c[0].1["name"], "Alu");
    assert_eq!(c[0].1["thickness"], "1.5 mm");
    run_all(&mut app, c);
    assert!(app.session.doc.sheet.rules.iter().any(|r| r.name == "Alu"));
}
