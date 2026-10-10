//! User scenarios in the real UI, headless (see `solvecraft_ui_egui::scenario`). Each JSON file
//! under `tests/scenarios/` is one workflow done with clicks, keys and drags, and one test here:
//! `build.rs` makes a test per file, named after it. A scenario waiting for a fix starts with a
//! `{"pending": "why"}` step (its test is ignored with that reason). Adding a scenario is adding
//! its file.

use solvecraft_ui_egui::scenario::Harness;

fn run(name: &str) {
    let path = format!("{}/tests/scenarios/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let steps: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut h = Harness::new();
    if let Err(e) = h.run(&steps) {
        panic!("{name}: {e}");
    }
}

include!(concat!(env!("OUT_DIR"), "/scenario_tests.rs"));

/// A scenario file named by `SOLVECRAFT_SCENARIO_FILE` (for trying one out):
/// `SOLVECRAFT_SCENARIO_FILE=x.json cargo test --test scenarios adhoc -- --ignored --nocapture`.
#[test]
#[ignore]
fn adhoc() {
    let Ok(path) = std::env::var("SOLVECRAFT_SCENARIO_FILE") else { return };
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let steps: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut h = Harness::new();
    if let Err(e) = h.run(&steps) {
        panic!("{path}: {e}");
    }
}

#[test]
fn angle_dimension_both_interactive_workflows() {
    use serde_json::json;
    for preselect in [false, true] {
        let mut h = Harness::new();
        h.run(&json!([
            {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
            {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[40,0]]}}},
            {"call":"engine.execute","params":{"command":"sketch.line","params":{"points":[[0,0],[30,30]]}}},
            {"call":"ui.view","params":{"view":"top"}}
        ]))
        .unwrap();
        let picks = json!([
            {"click":{"sketch":[20,0]}},
            {"click":{"sketch":[15,15]},"shift":true}
        ]);
        if preselect {
            h.run(&picks).unwrap();
            h.run(&json!([{ "key":"D" }])).unwrap();
        } else {
            h.run(&json!([{ "key":"D" }])).unwrap();
            h.run(&picks).unwrap();
        }
        h.run(&json!([
            {"click":{"sketch":[20,8]}},
            {"text":"45"}, {"key":"Enter"},
            {"expect":{"errors":0,"param":{"d1":std::f64::consts::FRAC_PI_4}}}
        ]))
        .unwrap();
        let sk = &h.app.session.model.state().sketches[0].sketch;
        assert!(sk.constraints.iter().any(|c| matches!(c.kind, solvecraft_engine::sketch::ConstraintKind::Angle { .. })));
    }
}

#[test]
fn home_tab_and_file_recent_open_designs() {
    use serde_json::json;
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"solid.box","params":{"length":10,"width":10,"height":10}}},
        {"call":"engine.execute","params":{"command":"file.save_as","params":{"path":"$DIR/recent.solvecraft"}}},
        {"click":{"handle":"tab:home"}},
        {"expect":{"home":true,"documents":1}},
        {"click":{"handle":"tab:design:0"}},
        {"expect":{"home":false,"bodies":1}},
        {"click":{"handle":"tab:home"}},
        {"click":{"handle":"home:recent:0"}},
        {"expect":{"home":false,"bodies":1,"documents":2}},
        {"click":{"handle":"appbar:folder"}},
        {"widget":"Open Recent"}
    ]))
    .unwrap();
    let path = h.dir.join("recent.solvecraft").to_string_lossy().into_owned();
    h.run(&json!([
        {"widget":path},
        {"expect":{"home":false,"bodies":1,"documents":3}}
    ]))
    .unwrap();
}

#[test]
fn sketch_toolbar_imports_use_file_picker() {
    use serde_json::json;
    let mut h = Harness::new();
    let path = h.dir.join("drawing.svg");
    std::fs::write(
        &path,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40mm" height="20mm" viewBox="0 0 40 20"><path d="M 0 0 L 40 0 L 40 20 L 0 20 Z"/></svg>"#,
    )
    .unwrap();
    h.run(&json!([
        {"call":"engine.execute","params":{"command":"sketch.create","params":{"plane":"XY"}}},
        {"choose_file":path},
        {"start":"sketch.insert_svg"},
        {"expect":{"errors":0,"sketch_curves":{"own":4}}}
    ]))
    .unwrap();
}

#[test]
fn narrow_toolbar_reaches_collapsed_commands() {
    use serde_json::json;
    let mut h = Harness::new();
    h.run(&json!([
        {"call":"ui.resize","params":{"width":640,"height":800}},
        {"expect":{"shown_handles":["toolbar:overflow"]}},
        {"click":{"handle":"toolbar:overflow"}},
        {"widget":"CONSTRUCT"},
        {"expect":{"shown_handles":["toolbar:construct.plane.offset"]}},
        {"click":{"handle":"toolbar:construct.plane.offset"}},
        {"expect":{"dialog":"OffsetPlane"}}
    ]))
    .unwrap();
}
