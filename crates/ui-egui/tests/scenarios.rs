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
