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
    s26_drive_joint_typed
);

/// Scenarios written ahead of their features (the commands are in solvecraft-params' unpushed
/// work, their dialogs follow): run with `--ignored`, enabled once the features land.
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
    s22_sheet_metal_fold: "waits for SheetMetalFoldCmd and its dialog",
    s23_configurations: "waits for configurations (FusionStartDesignConfigModeCmd, config.activate) and their panel",
    s24_motion_study: "waits for FusionMotionStudyCommand and its dialog",
    s25_autosave_recovery: "waits for autosave and Recover unsaved design",
);
