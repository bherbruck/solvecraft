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
    s12_pattern_bodies,
    s13_plastic_enclosure,
    s14_appearance_export,
    s15_start_page_sample_save_reopen,
    s16_tabs_separate_undo,
    s17_toolbox_and_visibility,
    s18_3d_sketch_pipe,
    s19_canvas_calibrate,
    s20_section_with_joint_drive,
    s21_named_views,
    s26_drive_joint_typed,
    s22_sheet_metal_fold,
    s23_configurations,
    s24_motion_study,
    s25_autosave_recovery
);

/// Scenarios written ahead of their fixes or features (the user's open bugs): run with
/// `--ignored`; each moves to the list above once its fix lands.
macro_rules! pending {
    ($($name:ident: $why:literal),* $(,)?) => {
        $(#[test]
        #[ignore = $why]
        fn $name() {
            run(stringify!($name));
        })*
    };
}

pending!(
    s31_sketch_on_face_projects: "user bug: a sketch on a face projects its loops as reference (solvecraft-sketch)",
    s32_cut_in_active_component: "user bug: a cut in the active component leaves others alone (solvecraft-params)",
    s33_move_x_arrow: "user bug: Move opens at 0 and its X arrow moves along X only (solvecraft-ui)",
    s34_component_move_carries_sketches: "user bug: moving a component carries its sketches (solvecraft-repo)",
    s30_pattern_a_hole: "a hole patterned as a feature gives one hole, not three (solvecraft-ui)",
);
