//! Manufacture workspace: CNC setup editing, CAM companion integration, toolpath overlay.
//!
//! Setups live in the browser and in floating dialogs, backed by the `manufacture.settings.*`
//! engine commands, a companion
//! HTTP backend (`CompanionCamBackend`) that is cross-platform (native + wasm via `ehttp`),
//! toolpath line overlays drawn over the shared viewport, and a simulation scrubber.
//!
//! # Transport
//! [`CompanionCamBackend`] uses [`ehttp`] for all HTTP communication.  It never blocks; instead
//! it queues an HTTP action per [`CamBackend::pump`] call (at most twice per second).  The
//! workspace [`tick`] function calls [`CamHost::pump`] on every frame while a job is live.
//!
//! # URL security
//! [`parse_companion_url`] enforces that the endpoint is `http://` (not https), contains no
//! userinfo (`@`), and has a host that is exactly `localhost`, `127.0.0.1`, or `[::1]`.
//! An explicit port is required.  The resulting [`ParsedCompanionUrl`] carries only the
//! normalised origin (`http://HOST:PORT`).
//!
//! # Session state that is NOT persisted
//! - `cam_endpoint` / `cam_token`: cleared on document open; `cam_token` is never written to
//!   storage.  The default endpoint is `http://127.0.0.1:38473`.
//! - The running job handle and its toolpath result.
//!
//! # Workspace switching
//! `active(app)` tests `app.ui.tab == "MANUFACTURE"`.  `enter` / `exit` gate on idle model state.

#![allow(clippy::too_many_lines)] // panel functions are inevitably verbose

use std::sync::{Arc, Mutex};

use egui::{Color32, Stroke};
use serde_json::json;
use solidcraft_manufacture::{
    ValidationIssue,
    job::{
        CamBackend, CamBackendDescriptor, CamErrorCode, CamHandle, CamHost, CamJobError, CamJobId, CamJobInput, CamJobResult, CamPhase,
        CamProvenance, CamResult,
    },
};
use web_time::{Duration, Instant};

use crate::theme::Tokens;
use crate::{SolveApp, viewport};

#[cfg(test)]
#[path = "manufacture_workspace_tests.rs"]
mod tests;

// ── Sub-panel tabs ─────────────────────────────────────────────────────────────

/// Sub-panels of the Manufacture workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MfgTab {
    #[default]
    Setup,
    Tools,
    Operations,
    Simulate,
    Post,
}

/// Manufacture workspace tab (ribbon content switches).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CamMode {
    #[default]
    Milling,
    Turning,
    Additive,
    Inspection,
    Fabrication,
}

// ── Default companion endpoint ─────────────────────────────────────────────────

/// Default companion loopback endpoint.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:38473";

/// Maximum bytes accepted from the companion in a single response body.
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

/// Minimum interval between HTTP poll requests (500 ms → at most 2/sec).
const POLL_MIN_INTERVAL: Duration = Duration::from_millis(500);

// ── State ─────────────────────────────────────────────────────────────────────

/// Per-session manufacture workspace state.  Non-persistent companion / job fields are reset on
/// document open.
pub struct State {
    // Setup editing
    /// Name of the currently active setup (key into `ManufactureSettings::setups`).
    pub active_setup: String,
    /// Which sub-panel is displayed in the right panel.
    pub tab: MfgTab,
    /// Manufacture MILLING / TURNING workspace tab.
    pub cam_mode: CamMode,
    pub(crate) revision: u64,

    // CAM session — UI-only; not persisted
    /// Companion endpoint URL (default: `http://127.0.0.1:38473`).
    pub cam_endpoint: String,
    /// Bearer token sent in `Authorization: Bearer <token>`.  Never stored on disk.
    pub cam_token: String,
    /// ID of the currently running or most recently finished job (engine-serialised form).
    /// Last CAM job id payload (`{"job": …}`) for simulate / post.
    pub cam_job_id: Option<serde_json::Value>,

    // Toolpath result for overlay / simulate / post
    /// Normalised path segments from the last completed job.
    pub toolpath_segments: Vec<solidcraft_manufacture::PathSegment>,
    /// Scrubber position (segments 0..=simulate_step are drawn).
    pub simulate_step: usize,
    /// Post-processed G-code text from the last completed job.
    pub generated_code: Option<String>,

    // Preflight / validation
    /// Latest soft-validation issues for the active setup.
    pub last_issues: Vec<ValidationIssue>,
    /// Last `manufacture.simulate` JSON result (runtime only).
    pub last_sim: Option<serde_json::Value>,

    /// Active machine plugin id for the UI picker (`none` / `fake` / `mtconnect`).
    pub machine_plugin: String,
    /// MTConnect base URL when using the live/fixture plugin (loopback).
    pub machine_endpoint: String,
    /// Last `manufacture.machine.status` payload (session only).
    pub last_machine_status: Option<serde_json::Value>,
    /// Last machine plugin error string for the panel.
    pub last_machine_error: Option<String>,
    /// Selected tool index in the Tools tab (into the active setup's tool library).
    pub selected_tool: Option<usize>,
    /// Selected operation index in the Operations browser.
    pub selected_op: Option<usize>,
    /// Open drill inspector (session only).
    pub drill_dialog: Option<crate::manufacture_drill::DrillDialogState>,
    /// Open 2D Contour inspector (session only).
    pub contour_dialog: Option<crate::manufacture_contour::ContourDialogState>,
    /// Open New Setup / Edit Setup dialog.
    pub setup_dialog: Option<crate::manufacture_setup::SetupDialogState>,
    /// Open secondary SETUP menu dialog (NC Program / Folder / Pattern / …).
    pub menu_dialog: Option<crate::manufacture_setup::MenuDialogState>,
    /// Open 2D op dialog (Face / Adaptive / Pocket / Slot / …).
    pub twod_dialog: Option<crate::manufacture_2d::TwodDialogState>,
    /// Open 3D milling dialog (Adaptive / Scallop / Parallel / …).
    pub threed_dialog: Option<crate::manufacture_3d::ThreedDialogState>,
    /// Open Hole Recognition dialog (Hole Groups / Libraries / Options).
    pub hole_rec_dialog: Option<crate::manufacture_hole_recognition::HoleRecDialogState>,
    /// Open Multi-Axis dialog (Swarf / Rotary / Finishing / …).
    pub multiaxis_dialog: Option<crate::manufacture_multiaxis::MultiAxisDialogState>,
    /// Open MODIFY dialog (Trim / Delete Passes / …).
    pub modify_dialog: Option<crate::manufacture_modify::ModifyDialogState>,
    /// NC Program / Post Process dialog.
    pub post_dialog: Option<crate::manufacture_post::PostDialogState>,
    /// Machining Time estimate dialog.
    pub machining_time_dialog: Option<crate::manufacture_post::MachiningTimeState>,
    /// Setup Sheet preview dialog.
    pub setup_sheet_dialog: Option<crate::manufacture_post::SetupSheetState>,
    /// Tool Library browser dialog.
    pub tool_library_dialog: Option<crate::manufacture_tools::ToolLibraryState>,
    /// Edit Tool dialog (General / Cutter / Shaft / Holder / Cutting / Post).
    pub edit_tool_dialog: Option<crate::manufacture_tools::EditToolState>,
    /// MANAGE ribbon placeholder dialogs (Form Mill, libraries, defaults).
    pub manage_stub_dialog: Option<crate::manufacture_tools::ManageStubState>,
    /// Inspect → Sustainability Insights dialog.
    pub sustainability_dialog: Option<crate::manufacture_inspect::SustainabilityState>,
    /// Turning operation dialog (Tool / Geometry / Radii / Passes / Linking).
    pub turning_dialog: Option<crate::manufacture_turning::TurningDialogState>,
    /// Part Handling dialog (Subspindle Grab / Bar Pull / Return / Tool Call).
    pub part_handling_dialog: Option<crate::manufacture_part_handling::PartHandlingDialogState>,
    /// Additive toolpath dialog.
    pub additive_dialog: Option<crate::manufacture_additive::AdditiveDialogState>,
    /// New Setup from G-Code dialog.
    pub from_gcode_dialog: Option<crate::manufacture_additive::FromGcodeState>,
    /// Inspection → Probing dialog.
    pub inspection_dialog: Option<crate::manufacture_inspection::InspectionDialogState>,
    /// Inspection → Manual dialog.
    pub manual_inspection_dialog: Option<crate::manufacture_inspection::ManualInspectionDialogState>,
    /// Fabrication → 2D Profile dialog.
    pub fabrication_dialog: Option<crate::manufacture_fabrication::FabricationDialogState>,
    /// Fabrication → Nest dialog.
    pub nest_dialog: Option<crate::manufacture_fabrication::NestDialogState>,
    /// Machine / Post / Template Library browser (with linked remotes).
    pub library_dialog: Option<crate::manufacture_libraries::LibraryDialogState>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active_setup: String::new(),
            tab: MfgTab::default(),
            cam_mode: CamMode::default(),
            revision: u64::MAX,
            cam_endpoint: DEFAULT_ENDPOINT.into(),
            cam_token: String::new(),
            cam_job_id: None,
            toolpath_segments: Vec::new(),
            simulate_step: 0,
            generated_code: None,
            last_issues: Vec::new(),
            last_sim: None,
            machine_plugin: "none".into(),
            machine_endpoint: "http://127.0.0.1:8082/VF-2".into(),
            last_machine_status: None,
            last_machine_error: None,
            selected_tool: None,
            selected_op: None,
            drill_dialog: None,
            contour_dialog: None,
            setup_dialog: None,
            menu_dialog: None,
            twod_dialog: None,
            threed_dialog: None,
            hole_rec_dialog: None,
            multiaxis_dialog: None,
            modify_dialog: None,
            post_dialog: None,
            machining_time_dialog: None,
            setup_sheet_dialog: None,
            tool_library_dialog: None,
            edit_tool_dialog: None,
            manage_stub_dialog: None,
            sustainability_dialog: None,
            turning_dialog: None,
            part_handling_dialog: None,
            additive_dialog: None,
            from_gcode_dialog: None,
            inspection_dialog: None,
            manual_inspection_dialog: None,
            fabrication_dialog: None,
            nest_dialog: None,
            library_dialog: None,
        }
    }
}

// ── Lifecycle ─────────────────────────────────────────────────────────────────

/// Whether the app is currently in the Manufacture workspace.
pub fn active(app: &SolveApp) -> bool {
    app.ui.tab == "MANUFACTURE"
}

/// Cache key for the setup isolate (which bodies the viewport keeps).
pub fn setup_body_key(app: &SolveApp) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    active(app).hash(&mut h);
    app.manufacture_workspace.active_setup.hash(&mut h);
    if let Some(d) = &app.manufacture_workspace.setup_dialog {
        d.picking.hash(&mut h);
        d.draft.model_bodies.hash(&mut h);
    } else if let Some(s) = app.session.doc.manufacture.setups.get(&app.manufacture_workspace.active_setup) {
        s.model_bodies.hash(&mut h);
    }
    h.finish()
}

/// Bodies outside the selected setup stay hidden. While the setup dialog is picking, every body stays visible so more can be added.
pub fn setup_shows_body(app: &SolveApp, name: &str) -> bool {
    if !active(app) {
        return true;
    }
    if app.manufacture_workspace.setup_dialog.as_ref().is_some_and(|d| d.picking) {
        return true;
    }
    let bodies = if let Some(d) = &app.manufacture_workspace.setup_dialog {
        &d.draft.model_bodies
    } else {
        match app.session.doc.manufacture.setups.get(&app.manufacture_workspace.active_setup) {
            Some(s) => &s.model_bodies,
            None => return true,
        }
    };
    bodies.is_empty() || bodies.iter().any(|n| n == name)
}

/// Switch to the Manufacture workspace; refuses while modelling is active.
pub fn enter(app: &mut SolveApp) {
    if app.session.active_sketch.is_some() || app.tool.is_some() || app.dialog.is_some() || app.file_load.busy() {
        app.set_status(app.tr("manufacture.enter_blocked").to_string(), true);
        return;
    }
    app.ui.tab = "MANUFACTURE".into();
    ensure_setup(app);
    apply_companion_backend(app);
}

/// Close floating Manufacture dialogs so a later command (or the layout width tests) is not left
/// with an oversized Tool Library / Machine Library window still on screen.
pub fn close_floating_dialogs(app: &mut SolveApp) {
    let s = &mut app.manufacture_workspace;
    s.drill_dialog = None;
    s.contour_dialog = None;
    s.setup_dialog = None;
    s.menu_dialog = None;
    s.twod_dialog = None;
    s.threed_dialog = None;
    s.hole_rec_dialog = None;
    s.multiaxis_dialog = None;
    s.modify_dialog = None;
    s.post_dialog = None;
    s.machining_time_dialog = None;
    s.setup_sheet_dialog = None;
    s.tool_library_dialog = None;
    s.edit_tool_dialog = None;
    s.manage_stub_dialog = None;
    s.sustainability_dialog = None;
    s.turning_dialog = None;
    s.part_handling_dialog = None;
    s.additive_dialog = None;
    s.from_gcode_dialog = None;
    s.inspection_dialog = None;
    s.manual_inspection_dialog = None;
    s.fabrication_dialog = None;
    s.nest_dialog = None;
    s.library_dialog = None;
}

/// Ribbon / palette hook for Manufacture commands. Returns `false` when the click was handled
/// here (do not fall through to a bare engine `run` with empty params).
pub fn start_hook(app: &mut SolveApp, id: &str) -> bool {
    if id == "inspect.sustainability" {
        crate::manufacture_inspect::open_sustainability(app);
        return false;
    }
    if !id.starts_with("manufacture.") {
        return true;
    }
    if !active(app) {
        enter(app);
        if !active(app) {
            return false;
        }
    }
    ensure_setup(app);

    match id {
        "manufacture.setup.new" => {
            crate::manufacture_setup::open_new(app);
            app.set_status(app.tr("manufacture.ribbon.setup_dialog").to_string(), false);
            false
        }
        "manufacture.setup.focus" | "manufacture.machines" => {
            let name = app.manufacture_workspace.active_setup.clone();
            if !name.is_empty() && id == "manufacture.setup.focus" {
                crate::manufacture_setup::open_edit(app, &name);
            } else {
                app.manufacture_workspace.tab = MfgTab::Setup;
            }
            app.set_status(app.tr("manufacture.ribbon.setup").to_string(), false);
            false
        }
        "manufacture.nc_program.new" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::NcProgram);
            false
        }
        "manufacture.folder.new" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::Folder);
            false
        }
        "manufacture.pattern.new" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::Pattern);
            false
        }
        "manufacture.manual_nc.new" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::ManualNc);
            false
        }
        "manufacture.nest" => {
            app.manufacture_workspace.cam_mode = CamMode::Fabrication;
            crate::manufacture_fabrication::open_nest(app);
            false
        }
        "manufacture.setup.from_gcode" => {
            app.manufacture_workspace.cam_mode = CamMode::Additive;
            crate::manufacture_additive::open_from_gcode(app);
            false
        }
        "manufacture.additive.simulate" => {
            app.manufacture_workspace.cam_mode = CamMode::Additive;
            let setup = app.manufacture_workspace.active_setup.clone();
            crate::manufacture_additive::run_simulate(app, &setup);
            false
        }
        "manufacture.insert_data" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::InsertData);
            false
        }
        "manufacture.mfg_model.new" => {
            crate::manufacture_setup::open_menu(app, crate::manufacture_setup::SetupMenuDialog::MfgModel);
            false
        }
        "manufacture.modify.trim" => {
            crate::manufacture_modify::open(app, crate::manufacture_modify::ModifyDialogKind::Trim);
            false
        }
        "manufacture.modify.delete_passes" => {
            crate::manufacture_modify::open(app, crate::manufacture_modify::ModifyDialogKind::DeletePasses);
            false
        }
        "manufacture.modify.leads_links" => {
            crate::manufacture_modify::open(app, crate::manufacture_modify::ModifyDialogKind::LeadsAndLinks);
            false
        }
        "manufacture.modify.replace_tool" => {
            crate::manufacture_modify::open(app, crate::manufacture_modify::ModifyDialogKind::ReplaceTool);
            false
        }
        "manufacture.modify.move_entry" => {
            crate::manufacture_modify::open(app, crate::manufacture_modify::ModifyDialogKind::MoveEntryPositions);
            false
        }
        "manufacture.tools" => {
            crate::manufacture_tools::open_tool_library(app);
            false
        }
        "manufacture.manage.form_mill" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::FormMill);
            false
        }
        "manufacture.manage.solid_holder" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::SolidHolder);
            false
        }
        "manufacture.manage.machine_library" => {
            crate::manufacture_libraries::open(app, crate::manufacture_libraries::LibraryDialogKind::Machine);
            false
        }
        "manufacture.manage.post_library" => {
            crate::manufacture_libraries::open(app, crate::manufacture_libraries::LibraryDialogKind::Post);
            false
        }
        "manufacture.manage.template_library" => {
            crate::manufacture_libraries::open(app, crate::manufacture_libraries::LibraryDialogKind::Template);
            false
        }
        "manufacture.manage.task_manager" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::TaskManager);
            false
        }
        "manufacture.manage.export_defaults" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::ExportDefaults);
            false
        }
        "manufacture.manage.import_defaults" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::ImportDefaults);
            false
        }
        "manufacture.manage.reset_defaults" => {
            crate::manufacture_tools::open_manage_stub(app, crate::manufacture_tools::ManageStubKind::ResetDefaults);
            false
        }
        "manufacture.generate" => {
            app.manufacture_workspace.tab = MfgTab::Operations;
            start_cam_job(app);
            false
        }
        "manufacture.simulate.ui" => {
            app.manufacture_workspace.tab = MfgTab::Simulate;
            if !app.manufacture_workspace.toolpath_segments.is_empty() {
                run_simulation(app);
            } else {
                app.set_status(app.tr("manufacture.sim.no_toolpath").to_string(), true);
            }
            false
        }
        "manufacture.simulate.machine" => {
            app.manufacture_workspace.tab = MfgTab::Simulate;
            if !app.manufacture_workspace.toolpath_segments.is_empty() {
                run_simulation_with_machine(app);
            } else {
                app.set_status(app.tr("manufacture.sim.no_toolpath").to_string(), true);
            }
            false
        }
        "manufacture.post.ui" => {
            crate::manufacture_post::open_post_process(app);
            false
        }
        "manufacture.setup_sheet" => {
            crate::manufacture_post::open_setup_sheet(app);
            false
        }
        "manufacture.machining_time" => {
            crate::manufacture_post::open_machining_time(app);
            false
        }
        "manufacture.op.face"
        | "manufacture.op.adaptive"
        | "manufacture.op.pocket"
        | "manufacture.op.profile"
        | "manufacture.op.slot"
        | "manufacture.op.trace"
        | "manufacture.op.thread"
        | "manufacture.op.bore"
        | "manufacture.op.circular"
        | "manufacture.op.engrave"
        | "manufacture.op.chamfer"
        | "manufacture.op.drill"
        | "manufacture.op.3d_adaptive"
        | "manufacture.op.3d_pocket"
        | "manufacture.op.3d_3plus2"
        | "manufacture.op.3d_steep_shallow"
        | "manufacture.op.3d_flat"
        | "manufacture.op.3d_parallel"
        | "manufacture.op.3d_scallop"
        | "manufacture.op.3d_contour"
        | "manufacture.op.3d_ramp"
        | "manufacture.op.3d_pencil"
        | "manufacture.op.3d_horizontal"
        | "manufacture.op.3d_spiral"
        | "manufacture.op.3d_radial"
        | "manufacture.op.3d_morphed_spiral"
        | "manufacture.op.3d_project"
        | "manufacture.op.3d_blend"
        | "manufacture.op.3d_morph"
        | "manufacture.op.3d_corner"
        | "manufacture.op.3d_flow"
        | "manufacture.op.3d_deburr"
        | "manufacture.op.3d_geodesic"
        | "manufacture.op.hole_recognition"
        | "manufacture.op.ma_rotary_pocket"
        | "manufacture.op.ma_roughing"
        | "manufacture.op.ma_swarf"
        | "manufacture.op.ma_adv_swarf"
        | "manufacture.op.ma_contour"
        | "manufacture.op.ma_rotary_parallel"
        | "manufacture.op.ma_rotary_contour"
        | "manufacture.op.ma_finishing"
        | "manufacture.op.turn_face"
        | "manufacture.op.turn_profile_rough"
        | "manufacture.op.turn_profile_finish"
        | "manufacture.op.turn_groove_rough"
        | "manufacture.op.turn_groove_finish"
        | "manufacture.op.turn_adaptive"
        | "manufacture.op.turn_groove"
        | "manufacture.op.turn_single_groove"
        | "manufacture.op.turn_thread"
        | "manufacture.op.turn_chamfer"
        | "manufacture.op.turn_part"
        | "manufacture.op.turn_trace"
        | "manufacture.part_handling.subspindle_grab"
        | "manufacture.part_handling.bar_pull"
        | "manufacture.part_handling.subspindle_return"
        | "manufacture.part_handling.tool_call"
        | "manufacture.op.probe_wcs"
        | "manufacture.op.probe_geometry"
        | "manufacture.op.inspect_surface"
        | "manufacture.op.part_alignment"
        | "manufacture.op.manual_inspection"
        | "manufacture.op.manual_inspection_record"
        | "manufacture.op.fab_2d_profile" => {
            if id.starts_with("manufacture.op.turn_") || id.starts_with("manufacture.part_handling.") {
                app.manufacture_workspace.cam_mode = CamMode::Turning;
            }
            if id.contains("probe") || id.contains("inspect_surface") || id.contains("part_alignment") || id.contains("manual_inspection") {
                app.manufacture_workspace.cam_mode = CamMode::Inspection;
            }
            if id.contains("fab_2d_profile") {
                app.manufacture_workspace.cam_mode = CamMode::Fabrication;
            }
            let setup = app.manufacture_workspace.active_setup.clone();
            let params = if setup.is_empty() { json!({}) } else { json!({"setup": setup}) };
            match app.run(id, params) {
                Ok(_) => {
                    app.manufacture_workspace.tab = MfgTab::Operations;
                    app.manufacture_workspace.revision = u64::MAX;
                    app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
                    if let Some(su) = app.session.doc.manufacture.setups.get(&setup) {
                        let idx = su.operations.len().saturating_sub(1);
                        app.manufacture_workspace.selected_op = Some(idx);
                        open_op_dialog(app, &setup, idx);
                    }
                    app.set_status(app.tr("manufacture.ribbon.op_added").to_string(), false);
                }
                Err(e) => app.set_status(e, true),
            }
            false
        }
        _ => true,
    }
}

fn open_op_dialog(app: &mut SolveApp, setup_name: &str, op_index: usize) {
    let Some(setup) = app.session.doc.manufacture.setups.get(setup_name) else {
        return;
    };
    let Some(op) = setup.operations.get(op_index) else {
        return;
    };
    match op {
        solidcraft_manufacture::Operation::Drill(_) => crate::manufacture_drill::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::HoleRecognition(_) => crate::manufacture_hole_recognition::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Profile(_) => crate::manufacture_contour::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Threed(_) => crate::manufacture_3d::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::MultiAxis(_) => crate::manufacture_multiaxis::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Turning(_) => crate::manufacture_turning::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::PartHandling(_) => crate::manufacture_part_handling::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Additive(_) => crate::manufacture_additive::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Inspection(_) => crate::manufacture_inspection::open_inspection(app, setup_name, op_index),
        solidcraft_manufacture::Operation::ManualInspection(_) => crate::manufacture_inspection::open_manual(app, setup_name, op_index),
        solidcraft_manufacture::Operation::FabricationProfile(_) => crate::manufacture_fabrication::open(app, setup_name, op_index),
        solidcraft_manufacture::Operation::Face(_)
        | solidcraft_manufacture::Operation::Adaptive(_)
        | solidcraft_manufacture::Operation::Pocket(_)
        | solidcraft_manufacture::Operation::Slot(_)
        | solidcraft_manufacture::Operation::Trace(_)
        | solidcraft_manufacture::Operation::Thread(_)
        | solidcraft_manufacture::Operation::Bore(_)
        | solidcraft_manufacture::Operation::Circular(_)
        | solidcraft_manufacture::Operation::Engrave(_)
        | solidcraft_manufacture::Operation::Chamfer(_) => {
            crate::manufacture_2d::open(app, setup_name, op_index);
        }
    }
}

/// Return to the SOLID workspace.
pub fn exit(app: &mut SolveApp) {
    app.ui.tab = "SOLID".into();
}

/// Clear transient state on document open/new.
pub fn reset_document(app: &mut SolveApp) {
    // Keep endpoint; clear token and job state.
    let endpoint = std::mem::take(&mut app.manufacture_workspace.cam_endpoint);
    app.manufacture_workspace = State { cam_endpoint: endpoint, ..State::default() };
    if active(app) {
        app.ui.tab = "SOLID".into();
    }
}

/// Per-frame tick: drive the HTTP state machine, check background job completion,
/// and request a repaint while busy.
///
/// Pump is always called first so the HTTP client makes progress even when the
/// job transitions to terminal on the same frame.
pub fn tick(app: &mut SolveApp, ctx: &egui::Context) {
    // Drive the companion HTTP state machine (no-op when no job is live).
    let host = app.session.manufacture_job_host().cloned();
    if let Some(h) = host {
        h.pump();
    }

    // Check job status via the in-memory engine command (never triggers network calls).
    let Some(id) = app.manufacture_workspace.cam_job_id.clone() else { return };

    let phase_str: Option<String> = match app.session.execute("manufacture.status", &id) {
        Ok(status) => status["progress"]["phase"].as_str().map(str::to_string),
        Err(_) => return, // job no longer in registry
    };

    let terminal = phase_str.as_deref().is_none_or(|p| matches!(p, "completed" | "failed" | "cancelled"));
    if !terminal {
        ctx.request_repaint();
        return;
    }

    // Auto-pull results when newly completed and not yet extracted.
    if phase_str.as_deref() == Some("completed")
        && app.manufacture_workspace.toolpath_segments.is_empty()
        && app.manufacture_workspace.generated_code.is_none()
    {
        match app.session.execute("manufacture.result", &id) {
            Ok(v) => {
                if let Some(segs) = v.get("segments").and_then(|s| serde_json::from_value(s.clone()).ok()) {
                    app.manufacture_workspace.toolpath_segments = segs;
                    app.manufacture_workspace.simulate_step = app.manufacture_workspace.toolpath_segments.len();
                }
                if let Some(code) = v.get("post_output").and_then(|c| c.as_str()).map(str::to_string) {
                    app.manufacture_workspace.generated_code = Some(code);
                } else if let Ok(g) = app.session.execute("manufacture.gcode", &id)
                    && let Some(code) = g.get("gcode").and_then(|c| c.as_str()).map(str::to_string)
                {
                    app.manufacture_workspace.generated_code = Some(code);
                }
            }
            Err(e) => app.set_status(e.to_string(), true),
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Make sure `active_setup` is valid; pick the first available setup if empty or stale.
pub(crate) fn ensure_setup(app: &mut SolveApp) {
    let mfg = &app.session.doc.manufacture;
    if !mfg.setups.contains_key(&app.manufacture_workspace.active_setup) {
        app.manufacture_workspace.active_setup = mfg.setups.keys().next().cloned().unwrap_or_default();
    }
    app.manufacture_workspace.last_issues = mfg.issues();
    app.manufacture_workspace.revision = app.session.revision;
}

/// Re-sync setup list and issues when the document revision advances.
fn maybe_sync(app: &mut SolveApp) {
    if app.manufacture_workspace.revision != app.session.revision {
        ensure_setup(app);
    }
}

// ── Floating dialogs (no sticky right aside) ───────────────────────────────────

/// Manufacture uses floating dialogs + Browser setups — not a right dock.
/// Setup / Tool Library / Post / ops open as drag-and-drop windows; dismiss with Cancel/Close/X.
pub fn panel(app: &mut SolveApp, ui: &mut egui::Ui) {
    maybe_sync(app);
    let _ = crate::manufacture_drill::show_dialog(app, ui);
    let _ = crate::manufacture_contour::show_dialog(app, ui);
    crate::manufacture_setup::show_dialogs(app, ui);
    let _ = crate::manufacture_2d::show_dialog(app, ui);
    let _ = crate::manufacture_3d::show_dialog(app, ui);
    let _ = crate::manufacture_hole_recognition::show_dialog(app, ui);
    let _ = crate::manufacture_multiaxis::show_dialog(app, ui);
    let _ = crate::manufacture_modify::show_dialog(app, ui);
    crate::manufacture_post::show_dialogs(app, ui);
    crate::manufacture_tools::show_dialogs(app, ui);
    crate::manufacture_inspect::show_dialogs(app, ui);
    let _ = crate::manufacture_turning::show_dialog(app, ui);
    let _ = crate::manufacture_part_handling::show_dialog(app, ui);
    crate::manufacture_additive::show_dialogs(app, ui);
    crate::manufacture_inspection::show_dialogs(app, ui);
    crate::manufacture_fabrication::show_dialogs(app, ui);
    crate::manufacture_libraries::show_dialogs(app, ui);
}

/// Browser tree: **Setups** → each setup → operations the ribbon added (Face, Drill, 3D Contour, …).
pub fn browser_setup_rows(app: &mut SolveApp, ui: &mut egui::Ui, depth: usize) {
    use crate::browser::{Row, draw_row, is_open, toggle};

    let folder_key = "mfg/setups";
    let folder_open = is_open(app, folder_key, true);
    let folder_row = Row { depth, fold: Some(folder_open), icon: "folder", label: "Setups", ..Default::default() };
    let fr = draw_row(ui, ui.id().with("mfg_setups_folder"), &folder_row);
    if fr.fold {
        toggle(app, folder_key, true);
    }
    if fr.double {
        crate::manufacture_setup::open_new(app);
    }
    if !is_open(app, folder_key, true) {
        return;
    }

    let mut names: Vec<String> = app.session.doc.manufacture.setups.keys().cloned().collect();
    names.sort();
    let active = app.manufacture_workspace.active_setup.clone();

    for name in names {
        let setup_key = format!("mfg/setup/{name}");
        let setup_open = is_open(app, &setup_key, true);
        let selected = active == name;
        let label = name.clone();
        let sr = draw_row(
            ui,
            ui.id().with(("mfg_setup", name.as_str())),
            &Row { depth: depth + 1, fold: Some(setup_open), icon: "box", label: &label, name: true, selected, ..Default::default() },
        );
        if sr.fold {
            toggle(app, &setup_key, true);
        }
        if sr.clicked {
            app.manufacture_workspace.active_setup = name.clone();
            app.manufacture_workspace.selected_op = None;
            app.manufacture_workspace.last_issues = app.session.doc.manufacture.issues();
        }
        if sr.double {
            app.manufacture_workspace.active_setup = name.clone();
            crate::manufacture_setup::open_edit(app, &name);
        }
        if !is_open(app, &setup_key, true) {
            continue;
        }

        let Some(setup) = app.session.doc.manufacture.setups.get(&name).cloned() else {
            continue;
        };
        for (i, op) in setup.operations.iter().enumerate() {
            let tnum = op.tool_number();
            let op_label = format!("[T{tnum}] {}", op.display_name());
            let op_sel = active == name && app.manufacture_workspace.selected_op == Some(i);
            let orow = draw_row(
                ui,
                ui.id().with(("mfg_op", name.as_str(), i)),
                &Row { depth: depth + 2, icon: "extrude", label: &op_label, name: true, selected: op_sel, ..Default::default() },
            );
            if orow.clicked || orow.double {
                app.manufacture_workspace.active_setup = name.clone();
                app.manufacture_workspace.selected_op = Some(i);
                open_op_dialog(app, &name, i);
            }
            // Nested tool line (under the op).
            if let Some(tool) = setup.tools.iter().find(|t| t.number == tnum) {
                let desc = if tool.description.is_empty() { tool_kind_label(tool.kind) } else { tool.description.as_str() };
                let tool_label = format!("#{tnum} - ø{:.2}mm {desc}", tool.diameter);
                let _ = draw_row(
                    ui,
                    ui.id().with(("mfg_op_tool", name.as_str(), i)),
                    &Row { depth: depth + 3, icon: "hole", label: &tool_label, name: true, dim: true, ..Default::default() },
                );
            }
        }
    }

    if app.session.doc.manufacture.setups.is_empty() {
        let hint = draw_row(
            ui,
            ui.id().with("mfg_setups_empty"),
            &Row { depth: depth + 1, icon: "settings", label: "New Setup…", dim: true, ..Default::default() },
        );
        if hint.clicked || hint.double {
            crate::manufacture_setup::open_new(app);
        }
    }
}

fn tool_kind_label(kind: solidcraft_manufacture::ToolKind) -> &'static str {
    match kind {
        solidcraft_manufacture::ToolKind::FlatEndmill => "Flat endmill",
        solidcraft_manufacture::ToolKind::BallEndmill => "Ball endmill",
        solidcraft_manufacture::ToolKind::BullnoseEndmill => "Bullnose",
        solidcraft_manufacture::ToolKind::Drill => "Drill",
        solidcraft_manufacture::ToolKind::Chamfer => "Chamfer",
        solidcraft_manufacture::ToolKind::Facemill => "Face mill",
        solidcraft_manufacture::ToolKind::Tap => "Tap",
        solidcraft_manufacture::ToolKind::Probe => "Probe",
        solidcraft_manufacture::ToolKind::Waterjet => "Waterjet",
        solidcraft_manufacture::ToolKind::Laser => "Laser",
        solidcraft_manufacture::ToolKind::Plasma => "Plasma",
    }
}

fn run_simulation(app: &mut SolveApp) {
    run_simulation_inner(app, false);
}

fn run_simulation_with_machine(app: &mut SolveApp) {
    run_simulation_inner(app, true);
}

fn run_simulation_inner(app: &mut SolveApp, with_machine: bool) {
    let setup_name = app.manufacture_workspace.active_setup.clone();
    let Some(job_params) = app.manufacture_workspace.cam_job_id.clone() else {
        app.set_status(app.tr("manufacture.sim.no_toolpath").to_string(), true);
        return;
    };
    let mut params = job_params;
    if let Some(obj) = params.as_object_mut() {
        obj.insert("setup".into(), serde_json::Value::String(setup_name.clone()));
        if with_machine {
            // Use stock box as a stand-in travel envelope when a machine is linked.
            if let Some(setup) = app.session.doc.manufacture.setups.get(&setup_name) {
                if let solidcraft_manufacture::Stock::Box { width, depth, height } = setup.stock {
                    obj.insert("machine_xy_travel".into(), json!([width, depth]));
                    obj.insert("machine_z_min".into(), json!(-height));
                }
                app.set_status(app.tr("manufacture.sim.with_machine").to_string(), false);
            }
        }
    }
    match app.session.execute("manufacture.simulate", &params) {
        Ok(v) => app.manufacture_workspace.last_sim = Some(v),
        Err(e) => app.set_status(e.to_string(), true),
    }
}

fn start_cam_job(app: &mut SolveApp) {
    let setup = app.manufacture_workspace.active_setup.clone();
    if setup.is_empty() {
        app.set_status(app.tr("manufacture.cam.no_setup").to_string(), true);
        return;
    }
    // Clear previous result.
    app.manufacture_workspace.toolpath_segments = Vec::new();
    app.manufacture_workspace.simulate_step = 0;
    app.manufacture_workspace.generated_code = None;

    match app.session.execute("manufacture.start", &json!({"setup": setup})) {
        Ok(v) => {
            let job_id = v.get("job").cloned().unwrap_or_default();
            app.manufacture_workspace.cam_job_id = Some(json!({"job": job_id}));
        }
        Err(e) => app.set_status(format!("{}: {e}", app.tr("manufacture.cam.start_fail")), true),
    }
}

// ── URL validation ────────────────────────────────────────────────────────────

/// A validated companion endpoint URL.  Contains only the normalised origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedCompanionUrl {
    /// Normalised HTTP origin: `http://HOST:PORT` (no path, query, or fragment).
    pub origin: String,
}

impl ParsedCompanionUrl {
    /// Build a companion API URL by appending `path` to the origin.
    pub fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.origin, path)
    }
}

/// Parse and validate an HTTP companion endpoint URL.
///
/// Enforces:
/// - Scheme is exactly `http` (not https, ftp, etc.)
/// - No userinfo (`@` character) — rejects `http://user:pass@host` and
///   credential-injection tricks like `http://127.0.0.1:PORT@evil.com`
/// - Host is exactly `localhost`, `127.0.0.1`, or `[::1]`
/// - An explicit port number is present
///
/// Returns a [`ParsedCompanionUrl`] whose `origin` is the normalised
/// `http://HOST:PORT` form, stripping any path, query, or fragment.
pub fn parse_companion_url(endpoint: &str) -> Result<ParsedCompanionUrl, String> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    use url::Url;

    let url = Url::parse(endpoint).map_err(|e| format!("invalid companion endpoint URL: {e}"))?;

    // Only http scheme.
    if url.scheme() != "http" {
        return Err(format!("companion endpoint must use http scheme, got: {:?}", url.scheme()));
    }

    // Reject any userinfo component — catches credential-injection URLs like
    // `http://127.0.0.1:PORT@evil.com` where the real host would be `evil.com`.
    if !url.username().is_empty() || url.password().is_some() {
        return Err("companion endpoint must not contain userinfo (user:pass@ is forbidden)".into());
    }

    // Host must be exactly one of the three allowed loopback forms.
    let host = url.host().ok_or_else(|| "companion endpoint has no host".to_string())?;
    let normalized_host: &str = match host {
        url::Host::Domain("localhost") => "localhost",
        url::Host::Ipv4(addr) if addr == Ipv4Addr::LOCALHOST => "127.0.0.1",
        url::Host::Ipv6(addr) if addr == Ipv6Addr::LOCALHOST => "[::1]",
        _ => {
            return Err(format!("companion endpoint host must be localhost, 127.0.0.1, or [::1], got: {}", url.host_str().unwrap_or("?")));
        }
    };

    // Explicit port required — default port 80 is not accepted.
    let port = url.port().ok_or_else(|| format!("companion endpoint requires an explicit port (e.g. http://127.0.0.1:38473), got: {endpoint:?}"))?;

    Ok(ParsedCompanionUrl { origin: format!("http://{}:{}", normalized_host, port) })
}

// ── CompanionCamBackend ────────────────────────────────────────────────────────

// ── Companion API response types ──────────────────────────────────────────────

/// POST /cam/v1/jobs → 202 Accepted
#[derive(serde::Deserialize)]
struct SubmitResp {
    job_id: String,
    #[allow(dead_code)]
    status: String,
}

/// GET /cam/v1/jobs/:id/status → 200 OK
#[derive(serde::Deserialize)]
struct StatusResp {
    #[allow(dead_code)]
    job_id: String,
    /// `"pending"` | `"running"` | `"done"` | `"failed"` | `"cancelled"`
    status: String,
    error: Option<String>,
}

/// GET /cam/v1/jobs/:id/result → 200 OK
#[derive(serde::Deserialize)]
struct ResultResp {
    #[allow(dead_code)]
    job_id: String,
    #[allow(dead_code)]
    status: String,
    result: Option<ResultBody>,
}

#[derive(serde::Deserialize)]
struct ResultBody {
    segments: Vec<solidcraft_manufacture::PathSegment>,
    post_output: Option<String>,
}

// ── Companion HTTP state machine ──────────────────────────────────────────────

/// Tag for what a pending ehttp response should be interpreted as.
/// All captured values are `Send + 'static`.
enum ResponseKind {
    Post { handle: CamHandle, seq: u64 },
    Poll { job_id: String, handle: CamHandle, seq: u64 },
    FetchResult { job_id: String, handle: CamHandle, seq: u64 },
    Cancel { handle: CamHandle, seq: u64 },
}

/// Current phase of the companion HTTP state machine.
enum CompanionPhase {
    /// No active job.
    Idle,
    /// Job queued locally; next pump will POST it.
    Queued { handle: CamHandle, input: Box<CamJobInput> },
    /// POST in flight; waiting for callback.
    PostInflight,
    /// POST succeeded; we have a companion job_id and poll when the interval elapses.
    Polling { job_id: String, handle: CamHandle },
    /// Status poll in flight.
    PollInflight,
    /// Status said done; next pump should GET the result (not yet in flight).
    WantResult { job_id: String, handle: CamHandle },
    /// Result fetch in flight.
    ResultInflight,
    /// DELETE (cancel) in flight.
    CancelInflight,
}

/// Shared mutable state owned by [`CompanionCamBackend`].
struct CompanionInner {
    origin: String,
    token: String,
    phase: CompanionPhase,
    last_action: Instant,
    /// Incremented on each new submit; stale callbacks detect the mismatch and no-op.
    generation: u64,
    // Provenance captured from CamJobInput at submit time.
    job_setup: String,
    job_scene_gen: u64,
    job_step_sha: String,
}

/// Cross-platform HTTP companion backend using [`ehttp`].
///
/// Works on native (ehttp uses a background thread) and WASM (ehttp uses the browser
/// Fetch API).  No `std::thread` assumptions anywhere in this type.
///
/// The endpoint must be an HTTP loopback URL validated by [`parse_companion_url`].
pub struct CompanionCamBackend {
    inner: Arc<Mutex<CompanionInner>>,
}

impl CompanionCamBackend {
    /// Create a new backend from a validated URL and optional bearer token.
    pub fn new(url: ParsedCompanionUrl, token: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CompanionInner {
                origin: url.origin,
                token: token.to_string(),
                phase: CompanionPhase::Idle,
                last_action: Instant::now(),
                generation: 0,
                job_setup: String::new(),
                job_scene_gen: 0,
                job_step_sha: String::new(),
            })),
        }
    }
}

fn lock_inner(inner: &Mutex<CompanionInner>) -> std::sync::MutexGuard<'_, CompanionInner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl CamBackend for CompanionCamBackend {
    fn descriptor(&self) -> CamBackendDescriptor {
        CamBackendDescriptor {
            id: "companion".into(),
            label: "Companion CAM".into(),
            availability: solidcraft_manufacture::job::Availability::Available,
            note: None,
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    /// Queue a new job. The actual HTTP POST is sent by the next [`pump`] call.
    fn submit(&self, id: CamJobId, input: CamJobInput) -> CamResult<CamHandle> {
        let total_ops = input.settings.setups.get(&input.setup).map_or(0, |s| s.operations.len() as u32);
        let handle = CamHandle::new(id, input.setup.clone(), input.scene_generation, total_ops);

        let mut g = lock_inner(&self.inner);
        // Bump generation to invalidate any in-flight callbacks for the previous job.
        g.generation = g.generation.wrapping_add(1);
        // Save provenance data (needed by the result callback).
        g.job_setup = input.setup.clone();
        g.job_scene_gen = input.scene_generation;
        g.job_step_sha = input.step_sha256.clone();
        g.phase = CompanionPhase::Queued { handle: handle.clone(), input: Box::new(input) };

        Ok(handle)
    }

    /// Drive the HTTP state machine.  Called from the workspace tick; at most twice per second.
    ///
    /// Decides what (if any) HTTP request to send, transitions the phase atomically while
    /// holding the lock, then releases the lock before invoking ehttp::fetch so the callback
    /// (which re-acquires the lock) never deadlocks.
    fn pump(&self) {
        self.pump_with_result();
    }
}

/// Decide the next HTTP action and perform the phase transition atomically.
/// Returns `None` when no request should be sent right now.
fn next_action(g: &mut CompanionInner) -> Option<(ehttp::Request, ResponseKind)> {
    match g.phase {
        // ── Queued: POST the job ────────────────────────────────────────────
        CompanionPhase::Queued { .. } => {
            // Move handle + input out of the Queued phase using a sentinel swap.
            let CompanionPhase::Queued { handle, input } = std::mem::replace(&mut g.phase, CompanionPhase::Idle) else {
                return None;
            };

            let body_bytes = match build_post_body(&input) {
                Ok(b) => b,
                Err(e) => {
                    handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, e));
                    return None;
                }
            };

            let url = format!("{}/cam/v1/jobs", g.origin);
            let request = make_request(ehttp::Method::POST, &url, &g.token, body_bytes, Some("application/json"));

            let seq = g.generation;
            g.phase = CompanionPhase::PostInflight;
            g.last_action = Instant::now();

            Some((request, ResponseKind::Post { handle, seq }))
        }

        // ── Polling: decide whether to poll or cancel ───────────────────────
        CompanionPhase::Polling { ref job_id, ref handle } => {
            // Check if the UI has requested cancellation.
            let cancelling = handle.status().is_ok_and(|s| s.progress.phase == CamPhase::Cancelling);

            if cancelling {
                let job_id = job_id.clone();
                let handle = handle.clone();
                let seq = g.generation;
                let url = format!("{}/cam/v1/jobs/{}", g.origin, job_id);
                let request = make_request(ehttp::Method::DELETE, &url, &g.token, vec![], None);
                g.phase = CompanionPhase::CancelInflight;
                g.last_action = Instant::now();
                return Some((request, ResponseKind::Cancel { handle, seq }));
            }

            // Rate-limit polling to at most twice per second.
            if g.last_action.elapsed() < POLL_MIN_INTERVAL {
                return None;
            }

            let job_id = job_id.clone();
            let handle = handle.clone();
            let seq = g.generation;
            let url = format!("{}/cam/v1/jobs/{}/status", g.origin, job_id);
            let request = make_request(ehttp::Method::GET, &url, &g.token, vec![], None);
            g.phase = CompanionPhase::PollInflight;
            g.last_action = Instant::now();

            Some((request, ResponseKind::Poll { job_id, handle, seq }))
        }

        // All inflight phases: wait for the callback.
        _ => None,
    }
}

/// Build an ehttp request for the companion API.
fn make_request(method: ehttp::Method, url: &str, token: &str, body: Vec<u8>, content_type: Option<&str>) -> ehttp::Request {
    let mut req = ehttp::Request::new(method, url, ehttp::Headers::default()).with_body(body);
    if let Some(ct) = content_type {
        req.headers.insert("Content-Type", ct);
    }
    if !token.is_empty() {
        req.headers.insert("Authorization", format!("Bearer {token}"));
    }
    req
}

/// Build the JSON POST body: `{ "step_b64": "…", "manufacture": { … } }`.
fn build_post_body(input: &CamJobInput) -> Result<Vec<u8>, String> {
    let step_b64 = base64_encode(&input.step_bytes);
    let body = serde_json::json!({
        "step_b64": step_b64,
        "manufacture": *input.settings,
    });
    serde_json::to_vec(&body).map_err(|e| format!("serialize companion POST body: {e}"))
}

/// Apply an ehttp response to the companion state machine.
fn apply_response(g: &mut CompanionInner, kind: ResponseKind, result: ehttp::Result<ehttp::Response>) {
    match kind {
        ResponseKind::Post { handle, seq } => on_post_response(g, handle, seq, result),
        ResponseKind::Poll { job_id, handle, seq } => on_poll_response(g, handle, job_id, seq, result),
        ResponseKind::FetchResult { job_id, handle, seq } => on_result_response(g, handle, job_id, seq, result),
        ResponseKind::Cancel { handle, seq } => on_cancel_response(g, handle, seq, result),
    }
}

fn on_post_response(g: &mut CompanionInner, handle: CamHandle, seq: u64, result: ehttp::Result<ehttp::Response>) {
    if g.generation != seq {
        return;
    } // stale: a newer job replaced this one
    match result {
        Err(e) => {
            handle.adapter_fail(CamJobError::new(CamErrorCode::HostUnavailable, e));
            g.phase = CompanionPhase::Idle;
        }
        Ok(resp) => {
            if resp.bytes.len() > MAX_RESPONSE_BYTES {
                handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, "companion POST response exceeds size limit"));
                g.phase = CompanionPhase::Idle;
                return;
            }
            if !resp.ok {
                let body = resp.text().unwrap_or_default();
                handle.adapter_fail(CamJobError::new(CamErrorCode::InvalidRequest, format!("companion returned HTTP {}: {body}", resp.status)));
                g.phase = CompanionPhase::Idle;
                return;
            }
            match serde_json::from_slice::<SubmitResp>(&resp.bytes) {
                Ok(r) => {
                    handle.adapter_set_progress(CamPhase::Planning, 0);
                    g.phase = CompanionPhase::Polling { job_id: r.job_id, handle };
                }
                Err(e) => {
                    handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, format!("parse companion submit response: {e}")));
                    g.phase = CompanionPhase::Idle;
                }
            }
        }
    }
}

fn on_poll_response(g: &mut CompanionInner, handle: CamHandle, job_id: String, seq: u64, result: ehttp::Result<ehttp::Response>) {
    if g.generation != seq {
        return;
    }
    match result {
        Err(e) => {
            handle.adapter_fail(CamJobError::new(CamErrorCode::HostUnavailable, format!("poll: {e}")));
            g.phase = CompanionPhase::Idle;
        }
        Ok(resp) => {
            if resp.bytes.len() > MAX_RESPONSE_BYTES {
                handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, "companion poll response exceeds size limit"));
                g.phase = CompanionPhase::Idle;
                return;
            }
            if !resp.ok {
                let body = resp.text().unwrap_or_default();
                handle.adapter_fail(CamJobError::new(CamErrorCode::HostUnavailable, format!("poll returned HTTP {}: {body}", resp.status)));
                g.phase = CompanionPhase::Idle;
                return;
            }
            match serde_json::from_slice::<StatusResp>(&resp.bytes) {
                Ok(r) => match r.status.as_str() {
                    "done" => {
                        // Defer the result GET to the next pump (we hold the lock here).
                        g.phase = CompanionPhase::WantResult { job_id, handle };
                    }
                    "failed" => {
                        let msg = r.error.unwrap_or_else(|| "companion job failed".into());
                        handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, msg));
                        g.phase = CompanionPhase::Idle;
                    }
                    "cancelled" => {
                        handle.adapter_set_progress(CamPhase::Cancelled, 0);
                        g.phase = CompanionPhase::Idle;
                    }
                    // "pending" | "running" | anything else: keep polling.
                    _ => {
                        handle.adapter_set_progress(CamPhase::Planning, 0);
                        g.phase = CompanionPhase::Polling { job_id, handle };
                    }
                },
                Err(e) => {
                    handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, format!("parse poll response: {e}")));
                    g.phase = CompanionPhase::Idle;
                }
            }
        }
    }
}

fn on_result_response(g: &mut CompanionInner, handle: CamHandle, _job_id: String, seq: u64, result: ehttp::Result<ehttp::Response>) {
    if g.generation != seq {
        return;
    }
    match result {
        Err(e) => {
            handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, format!("fetch result: {e}")));
            g.phase = CompanionPhase::Idle;
        }
        Ok(resp) => {
            if resp.bytes.len() > MAX_RESPONSE_BYTES {
                handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, "companion result response exceeds size limit"));
                g.phase = CompanionPhase::Idle;
                return;
            }
            if !resp.ok {
                let body = resp.text().unwrap_or_default();
                handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, format!("result returned HTTP {}: {body}", resp.status)));
                g.phase = CompanionPhase::Idle;
                return;
            }
            match serde_json::from_slice::<ResultResp>(&resp.bytes) {
                Ok(r) => {
                    if let Some(body) = r.result {
                        let provenance =
                            CamProvenance { setup: g.job_setup.clone(), scene_generation: g.job_scene_gen, step_sha256: g.job_step_sha.clone() };
                        handle.adapter_complete(CamJobResult {
                            job: handle.id(),
                            provenance,
                            segments: body.segments,
                            post_output: body.post_output,
                        });
                    } else {
                        handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, "result response has no result body"));
                    }
                    g.phase = CompanionPhase::Idle;
                }
                Err(e) => {
                    handle.adapter_fail(CamJobError::new(CamErrorCode::Internal, format!("parse result response: {e}")));
                    g.phase = CompanionPhase::Idle;
                }
            }
        }
    }
}

fn on_cancel_response(g: &mut CompanionInner, handle: CamHandle, seq: u64, _result: ehttp::Result<ehttp::Response>) {
    if g.generation != seq {
        return;
    }
    // Regardless of the HTTP result, mark the job cancelled.
    handle.adapter_set_progress(CamPhase::Cancelled, 0);
    g.phase = CompanionPhase::Idle;
}

/// Fire a deferred result GET when the phase is [`CompanionPhase::WantResult`].
fn next_result_action(g: &mut CompanionInner) -> Option<(ehttp::Request, ResponseKind)> {
    let CompanionPhase::WantResult { ref job_id, ref handle } = g.phase else {
        return None;
    };
    let job_id = job_id.clone();
    let handle = handle.clone();
    let seq = g.generation;
    let url = format!("{}/cam/v1/jobs/{}/result", g.origin, job_id);
    let request = make_request(ehttp::Method::GET, &url, &g.token, vec![], None);
    g.phase = CompanionPhase::ResultInflight;
    g.last_action = Instant::now();
    Some((request, ResponseKind::FetchResult { job_id, handle, seq }))
}

impl CompanionCamBackend {
    fn pump_with_result(&self) {
        let inner = Arc::clone(&self.inner);

        let action: Option<(ehttp::Request, ResponseKind)> = {
            let mut g = lock_inner(&inner);
            next_action(&mut g).or_else(|| next_result_action(&mut g))
        };

        if let Some((request, kind)) = action {
            let inner2 = Arc::clone(&inner);
            ehttp::fetch(request, move |result| {
                let mut g = lock_inner(&inner2);
                apply_response(&mut g, kind, result);
            });
        }
    }
}

// ── base64 encoding ───────────────────────────────────────────────────────────

/// Standard base64 encoding (RFC 4648 alphabet with `+` and `/`, with padding).
/// Used to encode STEP bytes for the companion POST body.
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = *chunk.get(1).unwrap_or(&0) as usize;
        let b2 = *chunk.get(2).unwrap_or(&0) as usize;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[(triple >> 18) & 63] as char);
        out.push(CHARS[(triple >> 12) & 63] as char);
        out.push(if chunk.len() > 1 { CHARS[(triple >> 6) & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { CHARS[triple & 63] as char } else { '=' });
    }
    out
}

// ── Backend lifecycle ─────────────────────────────────────────────────────────

/// Install a companion backend into the session when the endpoint/token are configured.
/// Called whenever the user edits the endpoint or token field.
pub fn apply_companion_backend(app: &mut SolveApp) {
    let endpoint = app.manufacture_workspace.cam_endpoint.clone();
    let token = app.manufacture_workspace.cam_token.clone();
    match parse_companion_url(&endpoint) {
        Ok(url) => {
            let backend = CompanionCamBackend::new(url, &token);
            app.session.set_manufacture_job_host(Some(CamHost::new(backend)));
        }
        Err(e) => {
            app.session.set_manufacture_job_host(None);
            app.set_status(format!("{}: {e}", app.tr("manufacture.backend.invalid_url")), true);
        }
    }
}

// ── Viewport: design + toolpath overlay ──────────────────────────────────────

/// Draw the design viewport and overlay toolpath lines on top of it.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    // Delegate to the normal design viewport (doesn't replace design geometry).
    viewport::show(app, ui);

    // Setup WCS / stock, drill, and contour indicators.
    crate::manufacture_setup::paint_overlays(app, ui);
    crate::manufacture_drill::paint_overlays(app, ui);
    crate::manufacture_contour::paint_overlays(app, ui);
    crate::manufacture_2d::paint_overlays(app, ui);
    crate::manufacture_3d::paint_overlays(app, ui);
    crate::manufacture_hole_recognition::paint_overlays(app, ui);
    crate::manufacture_multiaxis::paint_overlays(app, ui);

    // Toolpath overlay: skip if nothing to draw.
    let shown = app.manufacture_workspace.simulate_step.min(app.manufacture_workspace.toolpath_segments.len());
    if shown == 0 {
        return;
    }

    let rect = match app.viewport.rect {
        Some(r) if r.area() > 0.0 => r,
        _ => return,
    };
    let proj = viewport::projection(app, rect);
    let painter = ui.painter().clone();
    let t = Tokens::get();

    let rapid_color = Color32::from_rgba_premultiplied(160, 160, 160, 180);
    let cut_color = t.accent;
    let arc_color = t.sketch_accent;

    let mut current = [0.0f64; 3];
    for seg in app.manufacture_workspace.toolpath_segments.iter().take(shown) {
        let (to, color): ([f64; 3], Color32) = match seg {
            solidcraft_manufacture::PathSegment::Rapid { to } => (*to, rapid_color),
            solidcraft_manufacture::PathSegment::Linear { to, .. } => (*to, cut_color),
            solidcraft_manufacture::PathSegment::Arc { to, .. } => (*to, arc_color),
        };
        let w_from = solidcraft_engine::geom::Vec3::new(current[0], current[1], current[2]);
        let w_to = solidcraft_engine::geom::Vec3::new(to[0], to[1], to[2]);
        if let (Some(s), Some(e)) = (proj.to_screen(w_from), proj.to_screen(w_to)) {
            painter.line_segment([s, e], Stroke::new(1.5, color));
        }
        current = to;
    }
}
