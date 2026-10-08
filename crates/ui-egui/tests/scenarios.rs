//! User scenarios in the real UI, headless (see `solvecraft_ui_egui::scenario`). Each JSON file
//! under `tests/scenarios/` is one workflow done with clicks, keys and drags.

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

macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        $(#[test]
        fn $name() {
            run(stringify!($name));
        })*
    };
}

scenarios!(
    s01_box_fillet_hole_shell,
    s02_revolve_on_xz,
    s03_sketch_on_face_cut,
    s04_mirror_body,
    s05_delete_and_undo,
    s06_edit_early_feature,
    s07_components_joint_drive,
    s08_sheet_metal_flat_dxf,
    s09_step_roundtrip_hole,
    s10_parameters_drive_model,
    s11_sketch_constraints,
    s12_pattern_bodies
);
