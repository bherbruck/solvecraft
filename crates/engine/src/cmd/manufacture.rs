//! CNC manufacture commands.
//!
//! # Settings commands (document mutation, one undo step per change)
//! - `manufacture.settings.get` — read the current setups, machines and posts (or one named setup).
//! - `manufacture.settings.set` — atomically patch one named setup; validates strictly; identical
//!   edits are no-ops (no undo step added).
//!
//! # Runtime commands (never touch the document or undo stack)
//! - `manufacture.backends`   — list available CAM backends (metadata; no computation).
//! - `manufacture.preflight`  — validate that the model and setup are ready; extract an immutable
//!   STEP snapshot and report its size and hash without queuing work.
//! - `manufacture.start`      — capture the snapshot and queue a bounded CAM job.
//! - `manufacture.status`     — non-blocking progress and phase (no segment bytes).
//! - `manufacture.cancel`     — cooperative cancellation; completed jobs are untouched.
//! - `manufacture.result`     — retrieve normalised [`PathSegment`]s and post output.
//! - `manufacture.export`     — write G-code (or segment JSON) atomically at a path-scoped
//!   destination; `overwrite` defaults to `false`.
//! - `manufacture.simulate`   — run a deterministic 3-axis height-field simulation on a completed
//!   job's toolpath segments, returning a bounded summary, issue list, and optional scrubber
//!   frames.  Read-only; never touches the document or undo stack.

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_manufacture::job::CamJobId;
use solidcraft_manufacture::library::{self, CamTemplate, CatalogEntry, LibraryKind, LibraryLocation, LibraryRemote, sample_templates};
use solidcraft_manufacture::simulate::{Fixture, SimOptions, simulate_toolpaths};
use solidcraft_manufacture::{
    AdaptiveOperation, AdditiveKind, AdditiveOperation, BoreOperation, ChamferOperation, CircularOperation, DrillOperation, EngraveOperation,
    FabricationProfileOperation, FaceOperation, HoleRecognitionOperation, InspectionKind, InspectionOperation, ManualInspectionKind,
    ManualInspectionOperation, ManualNc, ManufactureSettings, ManufacturingModel, MfgSetup, MultiAxisOperation, MultiAxisStrategy, NcProgram,
    NestSettings, OpFolder, OpPattern, Operation, PartHandlingKind, PartHandlingOperation, PocketOperation, ProfileOperation, SetupOperationType,
    SlotOperation, ThreadOperation, ThreedOperation, ThreedStrategy, Tool, ToolKind, ToolpathModification, TraceOperation, TurningOperation,
    TurningStrategy,
};

use super::CommandSpec;
use crate::manufacture_jobs::job_error;
use crate::params::bad;
use crate::{Result, Session};

mod jobs;

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("manufacture.settings.get", "Manufacture Settings", get_settings)
        .noundo()
        .doc_only()
        .params("name?: setup name. Without name returns {setups, machines, posts, defaults}; with name returns {name, setup}. Does not evaluate the model."),
    CommandSpec::new("manufacture.settings.set", "Save Manufacture Setup", set_settings)
        .doc_only()
        .params("name: setup name, set: object of fields to replace (stock, wcs, tools, operations, machine, post). Creates from defaults when absent, validates atomically, one undo step; identical edits are no-ops."),
    CommandSpec::new("manufacture.backends", "Manufacture Backends", backends)
        .noundo()
        .doc_only()
        .params("No parameters. Returns {schema_version: 1, backends: [{id, label, availability}], post_presets: […], machine_presets: […]}. Metadata only; does not start computation."),
    CommandSpec::new("manufacture.posts.ensure_presets", "Ensure Post Presets", ensure_post_presets)
        .doc_only()
        .params("No parameters. Inserts built-in GRBL/Mach3/LinuxCNC/Fanuc/Haas post profiles when missing; one undo step; identical state is a no-op."),
    CommandSpec::new("manufacture.machines.ensure_presets", "Ensure Machine Presets", ensure_machine_presets)
        .doc_only()
        .params("No parameters. Inserts easy-OEM machine profiles (Haas/Okuma/Fanuc/Mazak) with ATC + integration metadata when missing; one undo step; identical state is a no-op."),
    CommandSpec::new("manufacture.library.list", "List CAM Libraries", library_list)
        .noundo()
        .doc_only()
        .params(
            "kind?: post|machine|template (default all). Returns {remotes, entries} from Document + Local + synced Linked remotes.",
        ),
    CommandSpec::new("manufacture.library.add_remote", "Add Library Remote", library_add_remote)
        .doc_only()
        .params(
            "url: git or file path; name?: display name; id?: cache id; branch?: default main; \
             kinds?: [post|machine|template]. Links a central/vendor CAM library repo.",
        ),
    CommandSpec::new("manufacture.library.remove_remote", "Remove Library Remote", library_remove_remote)
        .doc_only()
        .params("id: remote id. Removes a linked library remote from the document."),
    CommandSpec::new("manufacture.library.sync", "Sync Library Remotes", library_sync)
        .doc_only()
        .params(
            "id?: remote id (default: all enabled). git clone/fetch into cache and refresh catalog index.",
        ),
    CommandSpec::new("manufacture.library.install", "Install Library Entry", library_install)
        .doc_only()
        .params(
            "entry_id: catalog entry id (document/…|local/…|linked/…). Copies into document posts/machines/templates.",
        ),
    CommandSpec::new("manufacture.library.ensure_default_remote", "Ensure Default Library Remote", library_ensure_default_remote)
        .doc_only()
        .params("No parameters. Ensures the SolidCraft central CAM libraries remote is linked."),
    CommandSpec::new("manufacture.probe.snippet", "Probe Cycle Snippet", probe_snippet)
        .noundo()
        .doc_only()
        .params(
            "cycle: protected_move|single_surface|web_pocket|bore_boss|internal_corner|external_corner; \
             style?: haas_inspection_plus|fanuc_inspection_plus (default haas_inspection_plus); \
             tool_number?: probe ATC pocket (default 25); \
             x?, y?, z?: nominal feature mm (default 0); \
             clearance_z?: mm (default 25); protected_feed?: mm/min (default 150); \
             work_offset_s?: 0–6 (1=G54…; default 1); overtravel_q?: mm; size_d?: mm (web/bore). \
             Returns review G-code for Renishaw Inspection Plus macros — not machine-verified.",
        ),
    CommandSpec::new("manufacture.machine.plugins", "Machine Plugins", machine_plugins)
        .noundo()
        .doc_only()
        .params("No parameters. Lists installable machine plugins (fake, mtconnect) and the active session plugin."),
    CommandSpec::new("manufacture.machine.use", "Use Machine Plugin", machine_use)
        .noundo()
        .doc_only()
        .params(
            "plugin: fake|mtconnect|none; \
             endpoint?: MTConnect base URL (http://127.0.0.1:8082/VF-2) required for mtconnect; \
             fixture?: when true with mtconnect, installs sample Haas XML (no network — for tests). \
             Session-only; not saved with the document.",
        ),
    CommandSpec::new("manufacture.machine.status", "Machine Status", machine_status)
        .noundo()
        .doc_only()
        .params("No parameters. Polls the active machine plugin for live status (execution, tool, spindle, feed)."),
    // ── Ribbon: SETUP (Setup menu order) ───────────────────────────────
    CommandSpec::new("manufacture.setup.new", "New Setup", setup_new)
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("new")
        .params("name?: setup name (default SetupN). Creates an empty setup; UI opens the Setup dialog."),
    CommandSpec::new("manufacture.nc_program.new", "Create NC Program", nc_program_new_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("export")
        .params("UI opens Create NC Program (Settings / post / setups)."),
    CommandSpec::new("manufacture.folder.new", "New Folder", folder_new_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("box")
        .params("UI opens New Folder for grouping operations in the browser."),
    CommandSpec::new("manufacture.pattern.new", "New Pattern", pattern_new_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("combine")
        .params("UI opens New Pattern (linear / circular / mirror / duplication)."),
    CommandSpec::new("manufacture.manual_nc.new", "Manual NC", manual_nc_new_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("offset")
        .params("UI opens Manual NC insert (comment / stop / custom)."),
    CommandSpec::new("manufacture.nest", "Nest", nest_open_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "NEST")
        .icon("combine")
        .params("UI opens Nest (sheet size / spacing for fabrication layouts)."),
    CommandSpec::new("manufacture.op.fab_2d_profile", "2D Profile", op_fab_2d_profile)
        .doc_only()
        .at("MANUFACTURE", "CUTTING")
        .icon("offset")
        .params("setup?: setup name. Appends a Fabrication 2D Profile (waterjet / laser / plasma)."),
    CommandSpec::new("manufacture.setup.from_gcode", "New Setup from G-Code", setup_from_gcode)
        .doc_only()
        .at("MANUFACTURE", "ADDITIVE")
        .icon("export")
        .params(
            "name?: setup name; gcode: print-file text; source_path?: display path. \
             Creates an Additive setup with an Imported Additive Toolpath operation.",
        ),
    CommandSpec::new("manufacture.additive.simulate", "Simulate Additive Toolpath", additive_simulate_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ADDITIVE")
        .icon("revolve")
        .params("setup?: setup name. UI loads layer/G-code segments into Simulate."),
    CommandSpec::new("manufacture.op.probe_wcs", "Probe WCS", op_probe_wcs)
        .doc_only()
        .at("MANUFACTURE", "PROBING")
        .icon("measure")
        .params("setup?: setup name. Appends a Probe WCS inspection operation."),
    CommandSpec::new("manufacture.op.probe_geometry", "Probe Geometry", op_probe_geometry)
        .doc_only()
        .at("MANUFACTURE", "PROBING")
        .icon("hole")
        .params("setup?: setup name. Appends a Probe Geometry inspection operation."),
    CommandSpec::new("manufacture.op.inspect_surface", "Inspect Surface", op_inspect_surface)
        .doc_only()
        .at("MANUFACTURE", "PROBING")
        .icon("shell")
        .params("setup?: setup name. Appends an Inspect Surface operation."),
    CommandSpec::new("manufacture.op.part_alignment", "Part Alignment", op_part_alignment)
        .doc_only()
        .at("MANUFACTURE", "PROBING")
        .icon("move")
        .params("setup?: setup name. Appends a Part Alignment operation."),
    CommandSpec::new("manufacture.op.manual_inspection", "Create Manual Inspection", op_manual_inspection_create)
        .doc_only()
        .at("MANUFACTURE", "MANUAL")
        .icon("measure")
        .params("setup?: setup name. Appends a Create Manual Inspection plan."),
    CommandSpec::new("manufacture.op.manual_inspection_record", "Record Manual Inspection", op_manual_inspection_record)
        .doc_only()
        .at("MANUFACTURE", "MANUAL")
        .icon("export")
        .params("setup?: setup name. Appends a Record Manual Inspection step."),
    CommandSpec::new("manufacture.insert_data", "Insert Manufacturing Data", insert_data_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("shell")
        .params("UI opens Insert Manufacturing Data."),
    CommandSpec::new("manufacture.mfg_model.new", "Create Manufacturing Model", mfg_model_new_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "SETUP")
        .icon("extrude")
        .params("UI opens Create Manufacturing Model."),
    CommandSpec::new("manufacture.setup.focus", "Edit Setup", setup_focus_cmd)
        .noundo()
        .doc_only()
        .params("No parameters. UI opens the Setup dialog for the active setup."),
    CommandSpec::new("manufacture.containers.add", "Add Manufacture Container", containers_add)
        .doc_only()
        .params(
            "Exactly one of: nc_program | folder | pattern | manual_nc | manufacturing_model | \
             toolpath_modification (object). Appends to the document manufacture containers; one undo step.",
        ),
    // ── Ribbon: MODIFY (toolpath edit menu) ────────────────────────────
    CommandSpec::new("manufacture.modify.trim", "Trim", modify_trim_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MODIFY")
        .icon("trim")
        .params("UI opens Trim toolpath modification dialog."),
    CommandSpec::new("manufacture.modify.delete_passes", "Delete Passes", modify_delete_passes_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MODIFY")
        .icon("break")
        .params("UI opens Delete Passes modification dialog."),
    CommandSpec::new("manufacture.modify.leads_links", "Leads and Links", modify_leads_links_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MODIFY")
        .icon("offset")
        .params("UI opens Leads and Links modification dialog."),
    CommandSpec::new("manufacture.modify.replace_tool", "Replace Tool", modify_replace_tool_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MODIFY")
        .icon("measure")
        .params("UI opens Replace Tool modification dialog."),
    CommandSpec::new("manufacture.modify.move_entry", "Move Entry Positions", modify_move_entry_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MODIFY")
        .icon("move")
        .params("UI opens Move Entry Positions modification dialog."),
    // ── Ribbon: 2D (2D menu order) ─────────────────────────────────────
    CommandSpec::new("manufacture.op.adaptive", "2D Adaptive Roughing", op_adaptive)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("extrude")
        .params("setup?: setup name. Appends a 2D Adaptive clearing operation."),
    CommandSpec::new("manufacture.op.pocket", "2D Offset Roughing (Pocket)", op_pocket)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("shell")
        .params("setup?: setup name. Appends a 2D Pocket / offset roughing operation."),
    CommandSpec::new("manufacture.op.face", "Face", op_face)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("box")
        .params("setup?: setup name (default: first / active). Appends a Face operation with defaults."),
    CommandSpec::new("manufacture.op.profile", "2D Contour", op_profile)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("offset")
        .params("setup?: setup name. Appends a Profile (contour) operation."),
    CommandSpec::new("manufacture.op.slot", "Slot", op_slot)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("hole")
        .params("setup?: setup name. Appends a Slot operation."),
    CommandSpec::new("manufacture.op.trace", "Trace", op_trace)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("offset")
        .params("setup?: setup name. Appends a Trace operation."),
    CommandSpec::new("manufacture.op.thread", "Thread", op_thread)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("revolve")
        .params("setup?: setup name. Appends a Thread milling operation."),
    CommandSpec::new("manufacture.op.bore", "Bore", op_bore)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("hole")
        .params("setup?: setup name. Appends a Bore (helical) operation."),
    CommandSpec::new("manufacture.op.circular", "Circular", op_circular)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("cylinder")
        .params("setup?: setup name. Appends a Circular milling operation."),
    CommandSpec::new("manufacture.op.engrave", "Engrave", op_engrave)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("measure")
        .params("setup?: setup name. Appends an Engrave operation."),
    CommandSpec::new("manufacture.op.chamfer", "2D Chamfer", op_chamfer)
        .doc_only()
        .at("MANUFACTURE", "2D")
        .icon("combine")
        .params("setup?: setup name. Appends a 2D Chamfer operation."),
    // ── Ribbon: 3D (3D menu order) ──────────────────────────────────────
    CommandSpec::new("manufacture.op.3d_adaptive", "3D Adaptive Roughing", op_3d_adaptive)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("extrude")
        .params("setup?: setup name. Appends a 3D Adaptive Roughing operation."),
    CommandSpec::new("manufacture.op.3d_pocket", "3D Offset Roughing (Pocket)", op_3d_pocket)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("shell")
        .params("setup?: setup name. Appends a 3D Offset Roughing (Pocket Clearing) operation."),
    CommandSpec::new("manufacture.op.3d_3plus2", "3+2 Roughing", op_3d_3plus2)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("revolve")
        .params("setup?: setup name. Appends a 3+2 Roughing operation."),
    CommandSpec::new("manufacture.op.3d_steep_shallow", "Steep and Shallow", op_3d_steep_shallow)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("combine")
        .params("setup?: setup name. Appends a Steep and Shallow finishing operation."),
    CommandSpec::new("manufacture.op.3d_flat", "Flat", op_3d_flat)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("box")
        .params("setup?: setup name. Appends a Flat area finishing operation."),
    CommandSpec::new("manufacture.op.3d_parallel", "Parallel", op_3d_parallel)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("offset")
        .params("setup?: setup name. Appends a Parallel finishing operation."),
    CommandSpec::new("manufacture.op.3d_scallop", "Scallop", op_3d_scallop)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("offset")
        .params("setup?: setup name. Appends a Scallop finishing operation."),
    CommandSpec::new("manufacture.op.3d_contour", "3D Contour", op_3d_contour)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("offset")
        .params("setup?: setup name. Appends a 3D Contour finishing operation."),
    CommandSpec::new("manufacture.op.3d_ramp", "Ramp", op_3d_ramp)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("extrude")
        .params("setup?: setup name. Appends a Ramp finishing operation."),
    CommandSpec::new("manufacture.op.3d_pencil", "Pencil", op_3d_pencil)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("measure")
        .params("setup?: setup name. Appends a Pencil finishing operation."),
    CommandSpec::new("manufacture.op.3d_horizontal", "Horizontal", op_3d_horizontal)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("box")
        .params("setup?: setup name. Appends a Horizontal finishing operation."),
    CommandSpec::new("manufacture.op.3d_spiral", "Spiral", op_3d_spiral)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("revolve")
        .params("setup?: setup name. Appends a Spiral finishing operation."),
    CommandSpec::new("manufacture.op.3d_radial", "Radial", op_3d_radial)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("cylinder")
        .params("setup?: setup name. Appends a Radial finishing operation."),
    CommandSpec::new("manufacture.op.3d_morphed_spiral", "Morphed Spiral", op_3d_morphed_spiral)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("revolve")
        .params("setup?: setup name. Appends a Morphed Spiral finishing operation."),
    CommandSpec::new("manufacture.op.3d_project", "Project", op_3d_project)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("export")
        .params("setup?: setup name. Appends a Project finishing operation."),
    CommandSpec::new("manufacture.op.3d_blend", "Blend", op_3d_blend)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("combine")
        .params("setup?: setup name. Appends a Blend finishing operation."),
    CommandSpec::new("manufacture.op.3d_morph", "Morph", op_3d_morph)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("combine")
        .params("setup?: setup name. Appends a Morph finishing operation."),
    CommandSpec::new("manufacture.op.3d_corner", "Corner", op_3d_corner)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("fillet")
        .params("setup?: setup name. Appends a Corner clearing operation."),
    CommandSpec::new("manufacture.op.3d_flow", "Flow", op_3d_flow)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("offset")
        .params("setup?: setup name. Appends a Flow finishing operation."),
    CommandSpec::new("manufacture.op.3d_deburr", "Deburr", op_3d_deburr)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("fillet")
        .params("setup?: setup name. Appends a Deburr operation."),
    CommandSpec::new("manufacture.op.3d_geodesic", "Geodesic", op_3d_geodesic)
        .doc_only()
        .at("MANUFACTURE", "3D")
        .icon("sphere")
        .params("setup?: setup name. Appends a Geodesic finishing operation."),
    // ── Ribbon: DRILLING (Drill + Hole Recognition) ───────────────────
    CommandSpec::new("manufacture.op.drill", "Drill", op_drill)
        .doc_only()
        .at("MANUFACTURE", "DRILLING")
        .icon("hole")
        .params("setup?: setup name. Appends a Drill operation."),
    CommandSpec::new("manufacture.op.hole_recognition", "Hole Recognition", op_hole_recognition)
        .doc_only()
        .at("MANUFACTURE", "DRILLING")
        .icon("cylinder")
        .params("setup?: setup name. Appends a Hole Recognition operation (groups + options)."),
    // ── Ribbon: MULTI-AXIS (Multi-Axis menu order) ─────────────────────
    CommandSpec::new("manufacture.op.ma_rotary_pocket", "Rotary Offset Roughing (Pocket)", op_ma_rotary_pocket)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("shell")
        .params("setup?: setup name. Appends Rotary Offset Roughing (Pocket)."),
    CommandSpec::new("manufacture.op.ma_roughing", "Multi-Axis Roughing", op_ma_roughing)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("extrude")
        .params("setup?: setup name. Appends Multi-Axis Roughing."),
    CommandSpec::new("manufacture.op.ma_swarf", "Swarf", op_ma_swarf)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("offset")
        .params("setup?: setup name. Appends a Swarf side-milling operation."),
    CommandSpec::new("manufacture.op.ma_adv_swarf", "Advanced Swarf", op_ma_adv_swarf)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("combine")
        .params("setup?: setup name. Appends Advanced Swarf."),
    CommandSpec::new("manufacture.op.ma_contour", "Multi-Axis Contour", op_ma_contour)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("offset")
        .params("setup?: setup name. Appends Multi-Axis Contour."),
    CommandSpec::new("manufacture.op.ma_rotary_parallel", "Rotary Parallel", op_ma_rotary_parallel)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("revolve")
        .params("setup?: setup name. Appends Rotary Parallel finishing."),
    CommandSpec::new("manufacture.op.ma_rotary_contour", "Rotary Contour", op_ma_rotary_contour)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("cylinder")
        .params("setup?: setup name. Appends Rotary Contour finishing."),
    CommandSpec::new("manufacture.op.ma_finishing", "Multi-Axis Finishing", op_ma_finishing)
        .doc_only()
        .at("MANUFACTURE", "MULTI-AXIS")
        .icon("sphere")
        .params("setup?: setup name. Appends Multi-Axis Finishing."),
    // ── Ribbon: TURNING (Turning tab menu order) ───────────────────────
    CommandSpec::new("manufacture.op.turn_face", "Turning Face", op_turn_face)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("box")
        .params("setup?: setup name. Appends a Turning Face operation."),
    CommandSpec::new("manufacture.op.turn_profile_rough", "Turning Profile Roughing", op_turn_profile_rough)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("extrude")
        .params("setup?: setup name. Appends a Turning Profile Roughing operation."),
    CommandSpec::new("manufacture.op.turn_profile_finish", "Turning Profile Finishing", op_turn_profile_finish)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("offset")
        .params("setup?: setup name. Appends a Turning Profile Finishing operation."),
    CommandSpec::new("manufacture.op.turn_groove_rough", "Turning Groove Roughing", op_turn_groove_rough)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("shell")
        .params("setup?: setup name. Appends a Turning Groove Roughing operation."),
    CommandSpec::new("manufacture.op.turn_groove_finish", "Turning Groove Finishing", op_turn_groove_finish)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("shell")
        .params("setup?: setup name. Appends a Turning Groove Finishing operation."),
    CommandSpec::new("manufacture.op.turn_adaptive", "Turning Adaptive Roughing", op_turn_adaptive)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("extrude")
        .params("setup?: setup name. Appends a Turning Adaptive Roughing operation."),
    CommandSpec::new("manufacture.op.turn_groove", "Turning Groove", op_turn_groove)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("hole")
        .params("setup?: setup name. Appends a Turning Groove operation."),
    CommandSpec::new("manufacture.op.turn_single_groove", "Turning Single Groove", op_turn_single_groove)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("hole")
        .params("setup?: setup name. Appends a Turning Single Groove operation."),
    CommandSpec::new("manufacture.op.turn_thread", "Turning Thread", op_turn_thread)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("revolve")
        .params("setup?: setup name. Appends a Turning Thread operation."),
    CommandSpec::new("manufacture.op.turn_chamfer", "Turning Chamfer", op_turn_chamfer)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("combine")
        .params("setup?: setup name. Appends a Turning Chamfer operation."),
    CommandSpec::new("manufacture.op.turn_part", "Turning Part", op_turn_part)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("break")
        .params("setup?: setup name. Appends a Turning Part (cutoff) operation."),
    CommandSpec::new("manufacture.op.turn_trace", "Turning Trace", op_turn_trace)
        .doc_only()
        .at("MANUFACTURE", "TURNING")
        .icon("offset")
        .params("setup?: setup name. Appends a Turning Trace operation."),
    CommandSpec::new("manufacture.part_handling.subspindle_grab", "Subspindle Grab", op_part_subspindle_grab)
        .doc_only()
        .at("MANUFACTURE", "PART HANDLING")
        .icon("combine")
        .params("setup?: setup name. Appends a Subspindle Grab part-handling step."),
    CommandSpec::new("manufacture.part_handling.bar_pull", "Bar Pull", op_part_bar_pull)
        .doc_only()
        .at("MANUFACTURE", "PART HANDLING")
        .icon("extrude")
        .params("setup?: setup name. Appends a Bar Pull part-handling step."),
    CommandSpec::new("manufacture.part_handling.subspindle_return", "Subspindle Return", op_part_subspindle_return)
        .doc_only()
        .at("MANUFACTURE", "PART HANDLING")
        .icon("move")
        .params("setup?: setup name. Appends a Subspindle Return part-handling step."),
    CommandSpec::new("manufacture.part_handling.tool_call", "Tool Call", op_part_tool_call)
        .doc_only()
        .at("MANUFACTURE", "PART HANDLING")
        .icon("revolve")
        .params("setup?: setup name. Appends a Tool Call part-handling step."),
    // ── Ribbon: ACTIONS (Generate / Simulate / Post / Setup Sheet / Time)
    CommandSpec::new("manufacture.generate", "Generate", generate_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("combine")
        .params("UI starts CAM generate for the active setup (engine marker)."),
    CommandSpec::new("manufacture.simulate.ui", "Simulate", simulate_ui_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("revolve")
        .params("UI opens Simulate and runs stock simulation when a toolpath exists."),
    CommandSpec::new("manufacture.simulate.machine", "Simulate with Machine", simulate_machine_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("cylinder")
        .params("UI runs stock simulation with machine travel envelope checks."),
    CommandSpec::new("manufacture.post.ui", "Post Process", post_ui_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("export")
        .params("UI opens the NC Program / Post Process dialog (Settings + Operations)."),
    CommandSpec::new("manufacture.setup_sheet", "Setup Sheet", setup_sheet_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("export")
        .params("UI opens a shop-floor setup sheet (HTML) for the active NC selection."),
    CommandSpec::new("manufacture.machining_time", "Machining Time", machining_time_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "ACTIONS")
        .icon("measure")
        .params("UI estimates cycle time from toolpath segments and machine rates."),
    // ── Ribbon: MANAGE (Form Mill … Reset Defaults) ───────────────────
    CommandSpec::new("manufacture.manage.form_mill", "Form Mill", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("revolve")
        .params("UI: Form Mill stub (custom cutter profile)."),
    CommandSpec::new("manufacture.manage.solid_holder", "Solid Holder", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("cylinder")
        .params("UI: Solid Holder stub (holder from solid body)."),
    CommandSpec::new("manufacture.tools", "Tool Library", tools_open_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("measure")
        .params("UI opens the Tool Library and Edit Tool dialogs."),
    CommandSpec::new("manufacture.manage.machine_library", "Machine Library", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("cylinder")
        .params("UI: Machine Library stub."),
    CommandSpec::new("manufacture.machines", "Machine", machines_open_cmd)
        .noundo()
        .doc_only()
        .icon("cylinder")
        .params("UI opens Setup focused on machine / ATC / plugin (legacy alias)."),
    CommandSpec::new("manufacture.manage.post_library", "Post Library", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("export")
        .params("UI: Post Library stub."),
    CommandSpec::new("manufacture.manage.template_library", "Template Library", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("box")
        .params("UI: Template Library stub."),
    CommandSpec::new("manufacture.manage.task_manager", "Task Manager", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("combine")
        .params("UI: Task Manager stub."),
    CommandSpec::new("manufacture.manage.export_defaults", "Export Defaults", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("export")
        .params("UI: Export Defaults stub."),
    CommandSpec::new("manufacture.manage.import_defaults", "Import Defaults", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("import")
        .params("UI: Import Defaults stub."),
    CommandSpec::new("manufacture.manage.reset_defaults", "Reset Defaults", manage_stub_cmd)
        .noundo()
        .doc_only()
        .at("MANUFACTURE", "MANAGE")
        .icon("break")
        .params("UI: Reset Defaults stub."),
    CommandSpec::new("manufacture.preflight", "Manufacture Preflight", jobs::preflight)
        .noundo()
        .doc_only()
        .params("setup: saved setup name. Validates the installed CAM host and extracts an immutable STEP snapshot from the current model without queuing work. Rejects stale or empty geometry."),
    CommandSpec::new("manufacture.start", "Run Manufacture", jobs::start)
        .noundo()
        .doc_only()
        .params("setup: saved setup name; replace?: false. Captures an immutable STEP snapshot and queues a bounded CAM job. Returns its session-scoped job identity, scene_generation, and initial progress. Does not change the design."),
    CommandSpec::new("manufacture.status", "Manufacture Progress", jobs::status)
        .noundo()
        .doc_only()
        .params("job: {session, serial, generation}. Non-blocking phase, progress, and error; no segment bytes."),
    CommandSpec::new("manufacture.cancel", "Cancel Manufacture", jobs::cancel)
        .noundo()
        .doc_only()
        .params("job: {session, serial, generation}. Requests cooperative cancellation; completed jobs remain completed."),
    CommandSpec::new("manufacture.result", "Manufacture Result", jobs::result)
        .noundo()
        .doc_only()
        .params("job: {session, serial, generation}. Returns normalised PathSegments, post_output (G-code text or null), and provenance for a completed job."),
    CommandSpec::new("manufacture.gcode", "Manufacture G-code", jobs::gcode)
        .noundo()
        .doc_only()
        .params("job: {session, serial, generation}. Returns review G-code from companion post output or the setup's local post profile."),
    CommandSpec::new("manufacture.export", "Export Manufacture", jobs::export)
        .noundo()
        .doc_only()
        .params("job: {session, serial, generation}, path: destination, overwrite?: false. Exports G-code (or segment JSON) atomically; an existing destination is preserved on failure."),
    CommandSpec::new("manufacture.simulate", "Simulate Toolpaths", simulate)
        .noundo()
        .doc_only()
        .params(
            "job: {session, serial, generation} (completed job); \
             setup: saved setup name (for stock and tool geometry); \
             tool_number?: select tool (default: first tool in setup); \
             grid_cols?: 1–512 (default 256); grid_rows?: 1–512 (default 256); \
             clearance_z?: safe-height in mm (default 5.0); \
             max_frames?: scrubber frame count 0–64 (default 0); \
             interp_step_mm?: interpolation step override; \
             machine_xy_travel?: [x_max, y_max] envelope (mm); \
             machine_z_min?: lower Z limit (mm, ≤ 0); \
             fixtures?: [{label, min:[x,y,z], max:[x,y,z]}]. \
             Returns {removed_volume_mm3, min_remaining_height_mm, max_remaining_height_mm, \
             has_errors, issues, final_heights, grid_cols, grid_rows, frames}. \
             Read-only; does not modify the document or undo stack.",
        ),
];

// ── Settings get ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSettings {
    name: Option<String>,
}

fn get_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.settings.get";
    let req: GetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    match req.name {
        None => Ok(json!({
            "setups": s.doc.manufacture.setups,
            "machines": s.doc.manufacture.machines,
            "posts": s.doc.manufacture.posts,
            "defaults": MfgSetup::default(),
        })),
        Some(name) => {
            ManufactureSettings::validate_name(&name).map_err(|e| bad(cmd, e.to_string()))?;
            let setup = s.doc.manufacture.setups.get(&name).ok_or_else(|| bad(cmd, format!("no manufacture setup named `{name}`")))?;
            Ok(json!({"name": name, "setup": setup}))
        }
    }
}

// ── Settings set ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSettings {
    name: String,
    set: Value,
}

fn set_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.settings.set";
    let req: SetSettings = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    ManufactureSettings::validate_name(&req.name).map_err(|e| bad(cmd, e.to_string()))?;

    let patch = req.set.as_object().filter(|p| p.len() <= 16).ok_or_else(|| bad(cmd, "`set` must be an object with at most 16 fields"))?;

    // Merge patch fields onto the existing (or default) setup.
    let current = s.doc.manufacture.setups.get(&req.name).cloned().unwrap_or_default();
    let mut value = serde_json::to_value(&current).map_err(|e| bad(cmd, e.to_string()))?;
    let obj = value.as_object_mut().ok_or_else(|| bad(cmd, "internal: setup is not a JSON object"))?;
    for (key, val) in patch {
        obj.insert(key.clone(), val.clone());
    }
    let updated: MfgSetup = serde_json::from_value(value).map_err(|e| bad(cmd, format!("invalid field: {e}")))?;

    let reply = json!({"name": req.name, "setup": updated});

    // No-op: return early without touching the document (no undo step).
    if s.doc.manufacture.setups.get(&req.name) == Some(&updated) {
        return Ok(reply);
    }

    // Atomic: validate the entire settings block with the new setup before committing.
    let mut new_manufacture = s.doc.manufacture.clone();
    new_manufacture.setups.insert(req.name.clone(), updated);
    new_manufacture.validate().map_err(|e| bad(cmd, e.to_string()))?;

    // doc_mut() / Arc::make_mut creates a new Arc → execute() detects the change and pushes
    // exactly one undo step (CommandSpec::undoable defaults to true).
    s.doc_mut().manufacture = new_manufacture;

    Ok(reply)
}

// ── Backends ───────────────────────────────────────────────────────────────────

fn backends(s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.backends", "expected an empty parameter object"));
    }
    let backend = s.manufacture_jobs.descriptor();
    let backends: Vec<_> = backend.into_iter().collect();
    let posts: Vec<_> = solidcraft_manufacture::post::preset_posts()
        .into_iter()
        .map(|(id, profile)| json!({"id": id, "name": profile.name, "dialect": profile.dialect}))
        .collect();
    let machines: Vec<_> = solidcraft_manufacture::machines::preset_machines()
        .into_iter()
        .map(|(id, profile)| {
            json!({
                "id": id,
                "name": profile.name,
                "oem": profile.oem,
                "axes": profile.axes,
                "integration": profile.integration.kind,
                "atc_pockets": profile.tool_changer.pockets,
                "tool_changer": profile.tool_changer.kind,
                "probe": profile.probe.present,
                "probe_style": profile.probe.style,
            })
        })
        .collect();
    let probe_cycles: Vec<_> = solidcraft_manufacture::probe::ProbeCycle::all().iter().map(|c| json!({"id": c.as_str()})).collect();
    Ok(json!({
        "schema_version": 1,
        "backends": backends,
        "post_presets": posts,
        "machine_presets": machines,
        "probe_cycles": probe_cycles,
    }))
}

fn ensure_post_presets(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.posts.ensure_presets";
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    let mut mfg = s.doc.manufacture.clone();
    let mut added = Vec::new();
    for (id, profile) in solidcraft_manufacture::post::preset_posts() {
        if let std::collections::btree_map::Entry::Vacant(slot) = mfg.posts.entry(id.into()) {
            slot.insert(profile);
            added.push(id);
        }
    }
    if added.is_empty() {
        return Ok(json!({"added": [], "posts": s.doc.manufacture.posts.keys().cloned().collect::<Vec<_>>()}));
    }
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"added": added, "posts": s.doc.manufacture.posts.keys().cloned().collect::<Vec<_>>()}))
}

fn ensure_machine_presets(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.machines.ensure_presets";
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    let mut mfg = s.doc.manufacture.clone();
    let mut added = Vec::new();
    for (id, profile) in solidcraft_manufacture::machines::preset_machines() {
        if let std::collections::btree_map::Entry::Vacant(slot) = mfg.machines.entry(id.into()) {
            slot.insert(profile);
            added.push(id);
        }
    }
    if added.is_empty() {
        return Ok(json!({"added": [], "machines": s.doc.manufacture.machines.keys().cloned().collect::<Vec<_>>()}));
    }
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"added": added, "machines": s.doc.manufacture.machines.keys().cloned().collect::<Vec<_>>()}))
}

fn parse_library_kind(raw: &str) -> Option<LibraryKind> {
    match raw {
        "post" | "posts" => Some(LibraryKind::Post),
        "machine" | "machines" => Some(LibraryKind::Machine),
        "template" | "templates" => Some(LibraryKind::Template),
        _ => None,
    }
}

fn collect_catalog(s: &Session, filter: Option<LibraryKind>) -> Vec<CatalogEntry> {
    let mut entries = library::document_catalog(&s.doc.manufacture.posts, &s.doc.manufacture.machines, &s.doc.manufacture.templates);
    entries.extend(library::builtin_catalog());
    for remote in &s.doc.manufacture.library_remotes {
        if !remote.enabled {
            continue;
        }
        let checkout = library::remote_checkout_dir(remote);
        if let Ok(index) = library::read_index(&checkout) {
            entries.extend(library::catalog_from_index(remote, &index));
        }
    }
    if let Some(kind) = filter {
        entries.retain(|e| e.kind == kind);
    }
    entries
}

fn library_list(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.list";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        kind: Option<String>,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let filter = match p.kind.as_deref() {
        None => None,
        Some(k) => Some(parse_library_kind(k).ok_or_else(|| bad(cmd, format!("unknown kind `{k}`")))?),
    };
    let entries = collect_catalog(s, filter);
    Ok(json!({
        "default_remote": library::DEFAULT_CAM_LIBRARY_REMOTE,
        "remotes": s.doc.manufacture.library_remotes,
        "entries": entries,
        "cache_root": library::cache_root().to_string_lossy(),
    }))
}

fn library_add_remote(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.add_remote";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        url: String,
        name: Option<String>,
        id: Option<String>,
        branch: Option<String>,
        kinds: Option<Vec<String>>,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let id = p.id.unwrap_or_else(|| {
        let stem = p
            .url
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("remote")
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>();
        if stem.is_empty() { "remote".into() } else { stem }
    });
    let kinds = match p.kinds {
        None => Vec::new(),
        Some(ks) => {
            let mut out = Vec::new();
            for k in ks {
                out.push(parse_library_kind(&k).ok_or_else(|| bad(cmd, format!("unknown kind `{k}`")))?);
            }
            out
        }
    };
    let remote = LibraryRemote {
        id: id.clone(),
        name: p.name.unwrap_or_else(|| id.clone()),
        url: p.url,
        branch: p.branch.unwrap_or_else(|| "main".into()),
        kinds,
        enabled: true,
        last_synced_unix: None,
        last_error: String::new(),
    };
    remote.validate("remote").map_err(|e| bad(cmd, e.to_string()))?;
    let mut mfg = s.doc.manufacture.clone();
    if mfg.library_remotes.iter().any(|r| r.id == remote.id) {
        return Err(bad(cmd, format!("remote id `{}` already linked", remote.id)));
    }
    mfg.library_remotes.push(remote.clone());
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"remote": remote}))
}

fn library_remove_remote(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.remove_remote";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        id: String,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let mut mfg = s.doc.manufacture.clone();
    let before = mfg.library_remotes.len();
    mfg.library_remotes.retain(|r| r.id != p.id);
    if mfg.library_remotes.len() == before {
        return Err(bad(cmd, format!("no remote `{0}`", p.id)));
    }
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"removed": p.id}))
}

fn library_sync(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.sync";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        id: Option<String>,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let mut mfg = s.doc.manufacture.clone();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut results = Vec::new();
    for remote in &mut mfg.library_remotes {
        if let Some(id) = &p.id
            && remote.id != *id
        {
            continue;
        }
        if !remote.enabled {
            continue;
        }
        match library::sync_remote(remote) {
            Ok(path) => {
                remote.last_synced_unix = Some(now);
                remote.last_error.clear();
                let count = library::read_index(&path).map(|i| i.entries.len()).unwrap_or(0);
                results.push(json!({"id": remote.id, "ok": true, "entries": count, "path": path}));
            }
            Err(e) => {
                remote.last_error = e.clone();
                results.push(json!({"id": remote.id, "ok": false, "error": e}));
            }
        }
    }
    if results.is_empty() {
        return Err(bad(cmd, "no matching enabled remotes to sync"));
    }
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"results": results}))
}

fn library_ensure_default_remote(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.ensure_default_remote";
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    let mut mfg = s.doc.manufacture.clone();
    if mfg.library_remotes.iter().any(|r| r.url == library::DEFAULT_CAM_LIBRARY_REMOTE || r.id == "solidcraft-cam") {
        return Ok(json!({"added": false, "remotes": mfg.library_remotes}));
    }
    mfg.library_remotes.push(LibraryRemote::default());
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"added": true, "remotes": s.doc.manufacture.library_remotes}))
}

fn library_install(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.library.install";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        entry_id: String,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let entries = collect_catalog(s, None);
    let entry = entries.iter().find(|e| e.id == p.entry_id).ok_or_else(|| bad(cmd, format!("unknown entry `{}`", p.entry_id)))?.clone();
    let mut mfg = s.doc.manufacture.clone();
    match entry.location {
        LibraryLocation::Document => {
            return Ok(json!({"installed": false, "reason": "already in document", "entry": entry}));
        }
        LibraryLocation::Local => match entry.kind {
            LibraryKind::Post => {
                let profile = solidcraft_manufacture::post::preset_posts()
                    .into_iter()
                    .find(|(id, _)| *id == entry.path)
                    .map(|(_, p)| p)
                    .ok_or_else(|| bad(cmd, "local post not found"))?;
                mfg.posts.insert(entry.path.clone(), profile);
            }
            LibraryKind::Machine => {
                let profile = solidcraft_manufacture::machines::preset_machines()
                    .into_iter()
                    .find(|(id, _)| *id == entry.path)
                    .map(|(_, m)| m)
                    .ok_or_else(|| bad(cmd, "local machine not found"))?;
                mfg.machines.insert(entry.path.clone(), profile);
            }
            LibraryKind::Template => {
                let tmpl = sample_templates()
                    .into_iter()
                    .find(|t| library_slug(&t.name) == entry.path)
                    .ok_or_else(|| bad(cmd, "local template not found"))?;
                mfg.templates.insert(entry.path.clone(), tmpl);
            }
        },
        LibraryLocation::Linked => {
            let remote = mfg
                .library_remotes
                .iter()
                .find(|r| r.id == entry.source_id)
                .cloned()
                .ok_or_else(|| bad(cmd, "linked remote missing — sync first"))?;
            let repo = library::remote_checkout_dir(&remote);
            match entry.kind {
                LibraryKind::Post => {
                    let profile = library::load_post_file(&repo, &entry.path).map_err(|e| bad(cmd, e.to_string()))?;
                    mfg.posts.insert(entry.name.clone(), profile);
                }
                LibraryKind::Machine => {
                    let profile = library::load_machine_file(&repo, &entry.path).map_err(|e| bad(cmd, e.to_string()))?;
                    mfg.machines.insert(entry.name.clone(), profile);
                }
                LibraryKind::Template => {
                    let tmpl: CamTemplate = library::load_template_file(&repo, &entry.path).map_err(|e| bad(cmd, e.to_string()))?;
                    mfg.templates.insert(tmpl.name.clone(), tmpl);
                }
            }
        }
    }
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"installed": true, "entry": entry}))
}

fn library_slug(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect::<String>().trim_matches('-').to_string()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProbeSnippetParams {
    cycle: String,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    tool_number: Option<u32>,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    z: Option<f64>,
    #[serde(default)]
    clearance_z: Option<f64>,
    #[serde(default)]
    protected_feed: Option<f64>,
    #[serde(default)]
    work_offset_s: Option<u8>,
    #[serde(default)]
    overtravel_q: Option<f64>,
    #[serde(default)]
    size_d: Option<f64>,
}

fn probe_snippet(_s: &mut Session, params: &Value) -> Result<Value> {
    use solidcraft_manufacture::probe::{ProbeCycle, ProbeSnippetRequest, emit_snippet};
    use solidcraft_manufacture::{ProbeMacroStyle, Tool, ToolKind};

    let cmd = "manufacture.probe.snippet";
    let p: ProbeSnippetParams = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let cycle = ProbeCycle::parse(&p.cycle).ok_or_else(|| bad(cmd, format!("unknown probe cycle `{}`", p.cycle)))?;
    let style = match p.style.as_deref().unwrap_or("haas_inspection_plus") {
        "haas_inspection_plus" | "haas" => ProbeMacroStyle::HaasInspectionPlus,
        "fanuc_inspection_plus" | "fanuc" => ProbeMacroStyle::FanucInspectionPlus,
        other => return Err(bad(cmd, format!("unknown probe style `{other}`"))),
    };
    let tool_number = p.tool_number.unwrap_or(25);
    let req = ProbeSnippetRequest {
        cycle,
        style,
        probe_tool: Tool {
            number: tool_number,
            kind: ToolKind::Probe,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 100.0,
            flute_length: 20.0,
            flutes: 1,
            description: "spindle probe".into(),
            ..Default::default()
        },
        x: p.x.unwrap_or(0.0),
        y: p.y.unwrap_or(0.0),
        z: p.z.unwrap_or(0.0),
        clearance_z: p.clearance_z.unwrap_or(25.0),
        protected_feed: p.protected_feed.unwrap_or(150.0),
        work_offset_s: p.work_offset_s.unwrap_or(1),
        overtravel_q: p.overtravel_q.unwrap_or(0.0),
        size_d: p.size_d.unwrap_or(0.0),
    };
    let gcode = emit_snippet(&req).map_err(|e| bad(cmd, e))?;
    Ok(json!({
        "cycle": cycle.as_str(),
        "style": style,
        "tool_number": tool_number,
        "gcode": gcode,
        "review_only": true,
    }))
}

fn machine_plugins(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.machine.plugins";
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    let active = s.machine_host().map(|h| h.descriptor());
    Ok(json!({
        "schema_version": 1,
        "plugins": [
            {
                "id": "fake",
                "label": "Fake machine (tests)",
                "protocol": "fake",
                "note": "Deterministic READY status — no network",
            },
            {
                "id": "mtconnect",
                "label": "MTConnect agent",
                "protocol": "mtconnect",
                "note": "Haas NGC / Mazak Smooth / Fanuc FASMTC. Loopback-only StdHttpGet; use fixture:true for offline tests.",
            },
            {
                "id": "focas",
                "label": "Fanuc FOCAS (planned)",
                "protocol": "focas",
                "note": "Native Windows companion — not installed in this build",
                "availability": "unavailable",
            },
            {
                "id": "thinc",
                "label": "Okuma THINC (planned)",
                "protocol": "thinc",
                "note": "Windows-on-control / bridge companion — not installed in this build",
                "availability": "unavailable",
            },
        ],
        "active": active,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MachineUseParams {
    plugin: String,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    fixture: bool,
}

fn machine_use(s: &mut Session, params: &Value) -> Result<Value> {
    use solidcraft_manufacture::machine_host::MachineHost;
    use solidcraft_manufacture::mtconnect::MtConnectBackend;

    let cmd = "manufacture.machine.use";
    let p: MachineUseParams = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let key = p.plugin.trim().to_ascii_lowercase();
    match key.as_str() {
        "none" | "clear" | "off" => {
            s.set_machine_host(None);
            Ok(json!({"plugin": Value::Null, "active": Value::Null}))
        }
        "fake" => {
            let host = MachineHost::fake();
            let desc = host.descriptor();
            s.set_machine_host(Some(host));
            Ok(json!({"plugin": "fake", "active": desc}))
        }
        "mtconnect" => {
            let host = if p.fixture {
                let endpoint = p.endpoint.unwrap_or_else(|| "http://127.0.0.1:8082/VF-2".into());
                MachineHost::new(MtConnectBackend::with_fixed_xml(endpoint, solidcraft_manufacture::mtconnect::sample_haas_current_xml()))
            } else {
                let endpoint = p
                    .endpoint
                    .filter(|e| !e.trim().is_empty())
                    .ok_or_else(|| bad(cmd, "mtconnect requires endpoint (e.g. http://127.0.0.1:8082/VF-2)"))?;
                MachineHost::new(MtConnectBackend::with_std(endpoint))
            };
            let desc = host.descriptor();
            s.set_machine_host(Some(host));
            Ok(json!({"plugin": "mtconnect", "active": desc}))
        }
        "focas" | "thinc" => Err(bad(cmd, format!("plugin `{key}` is planned (native companion) — use fake or mtconnect for now"))),
        other => Err(bad(cmd, format!("unknown machine plugin `{other}`"))),
    }
}

fn machine_status(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.machine.status";
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    let host = s.machine_host().ok_or_else(|| bad(cmd, "no machine plugin installed; run manufacture.machine.use first"))?;
    host.pump();
    let status = host.status().map_err(|e| bad(cmd, e.to_string()))?;
    Ok(json!({
        "descriptor": host.descriptor(),
        "status": status,
    }))
}

// ── Ribbon helpers ─────────────────────────────────────────────────────────────

fn resolve_setup_name(s: &Session, params: &Value, cmd: &str) -> Result<String> {
    if let Some(name) = params.get("setup").and_then(|v| v.as_str()).map(str::trim).filter(|n| !n.is_empty()) {
        ManufactureSettings::validate_name(name).map_err(|e| bad(cmd, e.to_string()))?;
        return Ok(name.to_string());
    }
    s.doc.manufacture.setups.keys().next().cloned().ok_or_else(|| bad(cmd, "no manufacture setup; create one with manufacture.setup.new"))
}

fn ensure_default_tool(setup: &mut MfgSetup) {
    if setup.tools.is_empty() {
        setup.tools.push(Tool {
            number: 1,
            kind: ToolKind::FlatEndmill,
            diameter: 6.0,
            corner_radius: 0.0,
            length: 50.0,
            flute_length: 16.0,
            flutes: 4,
            description: "Default 6mm flat".into(),
            ..Default::default()
        });
    }
}

fn append_operation(s: &mut Session, params: &Value, cmd: &str, op: Operation) -> Result<Value> {
    let name = resolve_setup_name(s, params, cmd)?;
    let mut mfg = s.doc.manufacture.clone();
    let mut setup = mfg.setups.get(&name).cloned().unwrap_or_default();
    ensure_default_tool(&mut setup);
    let tool_number = setup.tools.first().map(|t| t.number).unwrap_or(1);
    let mut op = op;
    match &mut op {
        Operation::Face(o) => o.tool_number = tool_number,
        Operation::Profile(o) => o.tool_number = tool_number,
        Operation::Pocket(o) => o.tool_number = tool_number,
        Operation::Drill(o) => o.tool_number = tool_number,
        Operation::Adaptive(o) => o.tool_number = tool_number,
        Operation::Slot(o) => o.tool_number = tool_number,
        Operation::Trace(o) => o.tool_number = tool_number,
        Operation::Thread(o) => o.tool_number = tool_number,
        Operation::Bore(o) => o.tool_number = tool_number,
        Operation::Circular(o) => o.tool_number = tool_number,
        Operation::Engrave(o) => o.tool_number = tool_number,
        Operation::Chamfer(o) => o.tool_number = tool_number,
        Operation::Threed(o) => o.tool_number = tool_number,
        Operation::HoleRecognition(_) => {}
        Operation::MultiAxis(o) => o.tool_number = tool_number,
        Operation::Turning(o) => o.tool_number = tool_number,
        Operation::PartHandling(o) => {
            if o.kind == PartHandlingKind::ToolCall {
                o.tool_number = tool_number;
            }
        }
        Operation::Additive(_) | Operation::ManualInspection(_) => {}
        Operation::Inspection(o) => {
            // Prefer a probe tool when present; else keep the cycle default (often T25).
            if let Some(probe) = setup.tools.iter().find(|t| t.kind == ToolKind::Probe) {
                o.tool_number = probe.number;
            }
        }
        Operation::FabricationProfile(o) => {
            if let Some(cut) = setup.tools.iter().find(|t| t.kind.is_cutting_nozzle()) {
                o.tool_number = cut.number;
            } else {
                o.tool_number = tool_number;
            }
        }
    }
    setup.operations.push(op);
    mfg.setups.insert(name.clone(), setup);
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    let count = s.doc.manufacture.setups.get(&name).map(|su| su.operations.len()).unwrap_or(0);
    Ok(json!({"setup": name, "operations": count}))
}

fn setup_new(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.setup.new";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        name: Option<String>,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    let name = match p.name {
        Some(n) => {
            ManufactureSettings::validate_name(&n).map_err(|e| bad(cmd, e.to_string()))?;
            n
        }
        None => {
            let n = s.doc.manufacture.setups.len() + 1;
            format!("Setup{n}")
        }
    };
    if s.doc.manufacture.setups.contains_key(&name) {
        return Err(bad(cmd, format!("setup `{name}` already exists")));
    }
    let mut mfg = s.doc.manufacture.clone();
    mfg.setups.insert(name.clone(), MfgSetup::default());
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({"name": name, "setup": MfgSetup::default()}))
}

fn setup_focus_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.setup.focus", "expected an empty parameter object"));
    }
    Ok(json!({"panel": "setup", "edit": true}))
}

fn nc_program_new_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.nc_program.new", params, "nc_program")
}
fn folder_new_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.folder.new", params, "folder")
}
fn pattern_new_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.pattern.new", params, "pattern")
}
fn manual_nc_new_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.manual_nc.new", params, "manual_nc")
}
fn insert_data_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.insert_data", params, "insert_data")
}
fn mfg_model_new_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.mfg_model.new", params, "mfg_model")
}

fn marker_empty(cmd: &str, params: &Value, kind: &str) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad(cmd, "expected an empty parameter object"));
    }
    Ok(json!({"dialog": kind}))
}

fn modify_trim_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.modify.trim", params, "trim")
}
fn modify_delete_passes_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.modify.delete_passes", params, "delete_passes")
}
fn modify_leads_links_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.modify.leads_links", params, "leads_links")
}
fn modify_replace_tool_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.modify.replace_tool", params, "replace_tool")
}
fn modify_move_entry_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    marker_empty("manufacture.modify.move_entry", params, "move_entry")
}

fn containers_add(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.containers.add";
    let obj = params.as_object().ok_or_else(|| bad(cmd, "expected an object"))?;
    if obj.len() != 1 {
        return Err(bad(cmd, "provide exactly one container field"));
    }
    let mut mfg = s.doc.manufacture.clone();
    let reply = if let Some(v) = obj.get("nc_program") {
        let item: NcProgram = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.nc_programs.push(item);
        json!({"kind": "nc_program", "count": mfg.nc_programs.len()})
    } else if let Some(v) = obj.get("folder") {
        let item: OpFolder = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.folders.push(item);
        json!({"kind": "folder", "count": mfg.folders.len()})
    } else if let Some(v) = obj.get("pattern") {
        let item: OpPattern = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.patterns.push(item);
        json!({"kind": "pattern", "count": mfg.patterns.len()})
    } else if let Some(v) = obj.get("manual_nc") {
        let item: ManualNc = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.manual_nc.push(item);
        json!({"kind": "manual_nc", "count": mfg.manual_nc.len()})
    } else if let Some(v) = obj.get("manufacturing_model") {
        let item: ManufacturingModel = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.manufacturing_models.push(item);
        json!({"kind": "manufacturing_model", "count": mfg.manufacturing_models.len()})
    } else if let Some(v) = obj.get("toolpath_modification") {
        let item: ToolpathModification = serde_json::from_value(v.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mfg.toolpath_modifications.push(item);
        json!({"kind": "toolpath_modification", "count": mfg.toolpath_modifications.len()})
    } else {
        return Err(bad(cmd, "unknown field; expected nc_program|folder|pattern|manual_nc|manufacturing_model|toolpath_modification"));
    };
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(reply)
}

fn op_face(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.face", Operation::Face(FaceOperation::default()))
}
fn op_adaptive(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.adaptive", Operation::Adaptive(AdaptiveOperation::default()))
}
fn op_pocket(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.pocket", Operation::Pocket(PocketOperation::default()))
}
fn op_profile(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.profile", Operation::Profile(ProfileOperation::default()))
}
fn op_slot(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.slot", Operation::Slot(SlotOperation::default()))
}
fn op_trace(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.trace", Operation::Trace(TraceOperation::default()))
}
fn op_thread(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.thread", Operation::Thread(ThreadOperation::default()))
}
fn op_bore(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.bore", Operation::Bore(BoreOperation::default()))
}
fn op_circular(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.circular", Operation::Circular(CircularOperation::default()))
}
fn op_engrave(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.engrave", Operation::Engrave(EngraveOperation::default()))
}
fn op_chamfer(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.chamfer", Operation::Chamfer(ChamferOperation::default()))
}
fn op_drill(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.drill", Operation::Drill(DrillOperation::default()))
}
fn op_hole_recognition(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.hole_recognition", Operation::HoleRecognition(HoleRecognitionOperation::default()))
}

fn append_multiaxis(s: &mut Session, params: &Value, cmd: &str, strategy: MultiAxisStrategy) -> Result<Value> {
    append_operation(s, params, cmd, Operation::MultiAxis(MultiAxisOperation::with_strategy(strategy)))
}
fn op_ma_rotary_pocket(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_rotary_pocket", MultiAxisStrategy::RotaryOffsetRoughing)
}
fn op_ma_roughing(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_roughing", MultiAxisStrategy::MultiAxisRoughing)
}
fn op_ma_swarf(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_swarf", MultiAxisStrategy::Swarf)
}
fn op_ma_adv_swarf(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_adv_swarf", MultiAxisStrategy::AdvancedSwarf)
}
fn op_ma_contour(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_contour", MultiAxisStrategy::MultiAxisContour)
}
fn op_ma_rotary_parallel(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_rotary_parallel", MultiAxisStrategy::RotaryParallel)
}
fn op_ma_rotary_contour(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_rotary_contour", MultiAxisStrategy::RotaryContour)
}
fn op_ma_finishing(s: &mut Session, params: &Value) -> Result<Value> {
    append_multiaxis(s, params, "manufacture.op.ma_finishing", MultiAxisStrategy::MultiAxisFinishing)
}

fn append_threed(s: &mut Session, params: &Value, cmd: &str, strategy: ThreedStrategy) -> Result<Value> {
    append_operation(s, params, cmd, Operation::Threed(ThreedOperation::with_strategy(strategy)))
}
fn op_3d_adaptive(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_adaptive", ThreedStrategy::AdaptiveRoughing)
}
fn op_3d_pocket(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_pocket", ThreedStrategy::OffsetRoughing)
}
fn op_3d_3plus2(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_3plus2", ThreedStrategy::ThreePlusTwoRoughing)
}
fn op_3d_steep_shallow(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_steep_shallow", ThreedStrategy::SteepAndShallow)
}
fn op_3d_flat(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_flat", ThreedStrategy::Flat)
}
fn op_3d_parallel(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_parallel", ThreedStrategy::Parallel)
}
fn op_3d_scallop(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_scallop", ThreedStrategy::Scallop)
}
fn op_3d_contour(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_contour", ThreedStrategy::Contour)
}
fn op_3d_ramp(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_ramp", ThreedStrategy::Ramp)
}
fn op_3d_pencil(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_pencil", ThreedStrategy::Pencil)
}
fn op_3d_horizontal(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_horizontal", ThreedStrategy::Horizontal)
}
fn op_3d_spiral(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_spiral", ThreedStrategy::Spiral)
}
fn op_3d_radial(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_radial", ThreedStrategy::Radial)
}
fn op_3d_morphed_spiral(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_morphed_spiral", ThreedStrategy::MorphedSpiral)
}
fn op_3d_project(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_project", ThreedStrategy::Project)
}
fn op_3d_blend(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_blend", ThreedStrategy::Blend)
}
fn op_3d_morph(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_morph", ThreedStrategy::Morph)
}
fn op_3d_corner(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_corner", ThreedStrategy::Corner)
}
fn op_3d_flow(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_flow", ThreedStrategy::Flow)
}
fn op_3d_deburr(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_deburr", ThreedStrategy::Deburr)
}
fn op_3d_geodesic(s: &mut Session, params: &Value) -> Result<Value> {
    append_threed(s, params, "manufacture.op.3d_geodesic", ThreedStrategy::Geodesic)
}

fn append_turning(s: &mut Session, params: &Value, cmd: &str, strategy: TurningStrategy) -> Result<Value> {
    append_operation(s, params, cmd, Operation::Turning(TurningOperation::with_strategy(strategy)))
}
fn op_turn_face(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_face", TurningStrategy::Face)
}
fn op_turn_profile_rough(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_profile_rough", TurningStrategy::ProfileRoughing)
}
fn op_turn_profile_finish(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_profile_finish", TurningStrategy::ProfileFinishing)
}
fn op_turn_groove_rough(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_groove_rough", TurningStrategy::GrooveRoughing)
}
fn op_turn_groove_finish(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_groove_finish", TurningStrategy::GrooveFinishing)
}
fn op_turn_adaptive(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_adaptive", TurningStrategy::AdaptiveRoughing)
}
fn op_turn_groove(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_groove", TurningStrategy::Groove)
}
fn op_turn_single_groove(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_single_groove", TurningStrategy::SingleGroove)
}
fn op_turn_thread(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_thread", TurningStrategy::Thread)
}
fn op_turn_chamfer(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_chamfer", TurningStrategy::Chamfer)
}
fn op_turn_part(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_part", TurningStrategy::Part)
}
fn op_turn_trace(s: &mut Session, params: &Value) -> Result<Value> {
    append_turning(s, params, "manufacture.op.turn_trace", TurningStrategy::Trace)
}

fn append_part_handling(s: &mut Session, params: &Value, cmd: &str, kind: PartHandlingKind) -> Result<Value> {
    append_operation(s, params, cmd, Operation::PartHandling(PartHandlingOperation::with_kind(kind)))
}
fn op_part_subspindle_grab(s: &mut Session, params: &Value) -> Result<Value> {
    append_part_handling(s, params, "manufacture.part_handling.subspindle_grab", PartHandlingKind::SubspindleGrab)
}
fn op_part_bar_pull(s: &mut Session, params: &Value) -> Result<Value> {
    append_part_handling(s, params, "manufacture.part_handling.bar_pull", PartHandlingKind::BarPull)
}
fn op_part_subspindle_return(s: &mut Session, params: &Value) -> Result<Value> {
    append_part_handling(s, params, "manufacture.part_handling.subspindle_return", PartHandlingKind::SubspindleReturn)
}
fn op_part_tool_call(s: &mut Session, params: &Value) -> Result<Value> {
    append_part_handling(s, params, "manufacture.part_handling.tool_call", PartHandlingKind::ToolCall)
}

fn append_inspection(s: &mut Session, params: &Value, cmd: &str, kind: InspectionKind) -> Result<Value> {
    append_operation(s, params, cmd, Operation::Inspection(InspectionOperation::with_kind(kind)))
}
fn op_probe_wcs(s: &mut Session, params: &Value) -> Result<Value> {
    append_inspection(s, params, "manufacture.op.probe_wcs", InspectionKind::ProbeWcs)
}
fn op_probe_geometry(s: &mut Session, params: &Value) -> Result<Value> {
    append_inspection(s, params, "manufacture.op.probe_geometry", InspectionKind::ProbeGeometry)
}
fn op_inspect_surface(s: &mut Session, params: &Value) -> Result<Value> {
    append_inspection(s, params, "manufacture.op.inspect_surface", InspectionKind::InspectSurface)
}
fn op_part_alignment(s: &mut Session, params: &Value) -> Result<Value> {
    append_inspection(s, params, "manufacture.op.part_alignment", InspectionKind::PartAlignment)
}
fn op_manual_inspection_create(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(
        s,
        params,
        "manufacture.op.manual_inspection",
        Operation::ManualInspection(ManualInspectionOperation::with_kind(ManualInspectionKind::Create)),
    )
}
fn op_manual_inspection_record(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(
        s,
        params,
        "manufacture.op.manual_inspection_record",
        Operation::ManualInspection(ManualInspectionOperation::with_kind(ManualInspectionKind::Record)),
    )
}

fn setup_from_gcode(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.setup.from_gcode";
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct P {
        name: Option<String>,
        gcode: String,
        source_path: Option<String>,
    }
    let p: P = serde_json::from_value(params.clone()).map_err(|e| bad(cmd, e.to_string()))?;
    if p.gcode.trim().is_empty() {
        return Err(bad(cmd, "gcode must not be empty"));
    }
    let name = match p.name {
        Some(n) => {
            ManufactureSettings::validate_name(&n).map_err(|e| bad(cmd, e.to_string()))?;
            n
        }
        None => {
            let stem = p
                .source_path
                .as_deref()
                .and_then(|path| std::path::Path::new(path).file_stem().and_then(|s| s.to_str()).map(|s| s.to_string()))
                .filter(|s| !s.is_empty());
            stem.unwrap_or_else(|| format!("Additive{}", s.doc.manufacture.setups.len() + 1))
        }
    };
    let mut mfg = s.doc.manufacture.clone();
    if mfg.setups.contains_key(&name) {
        return Err(bad(cmd, format!("setup `{name}` already exists")));
    }
    let mut setup = MfgSetup { operation_type: SetupOperationType::Additive, ..MfgSetup::default() };
    let source = p.source_path.unwrap_or_default();
    setup.operations.push(Operation::Additive(AdditiveOperation::from_gcode(&source, &p.gcode)));
    mfg.setups.insert(name.clone(), setup);
    mfg.validate().map_err(|e| bad(cmd, e.to_string()))?;
    s.doc_mut().manufacture = mfg;
    Ok(json!({
        "setup": name,
        "operation_type": "additive",
        "kind": "imported_gcode",
    }))
}

fn nest_open_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.nest", "expected an empty parameter object"));
    }
    Ok(json!({"dialog": "nest", "defaults": NestSettings::default()}))
}

fn op_fab_2d_profile(s: &mut Session, params: &Value) -> Result<Value> {
    append_operation(s, params, "manufacture.op.fab_2d_profile", Operation::FabricationProfile(FabricationProfileOperation::default()))
}

fn additive_simulate_cmd(s: &mut Session, params: &Value) -> Result<Value> {
    let cmd = "manufacture.additive.simulate";
    let name = resolve_setup_name(s, params, cmd)?;
    let setup = s.doc.manufacture.setups.get(&name).ok_or_else(|| bad(cmd, format!("setup `{name}` not found")))?;
    let additive = setup.operations.iter().find_map(|op| match op {
        Operation::Additive(o) => Some(o),
        _ => None,
    });
    let Some(op) = additive else {
        return Err(bad(cmd, "setup has no additive toolpath operation"));
    };
    let segments = op.simulation_segments();
    if segments.is_empty() {
        return Err(bad(cmd, "additive simulation produced no segments"));
    }
    Ok(json!({
        "action": "additive_simulate",
        "setup": name,
        "segments": segments,
        "layer_count": op.layer_count,
        "kind": match op.kind {
            AdditiveKind::GeneratedToolpath => "generated",
            AdditiveKind::ImportedGcode => "imported_gcode",
        },
    }))
}

fn generate_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.generate", "expected an empty parameter object"));
    }
    Ok(json!({"action": "generate"}))
}
fn simulate_ui_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.simulate.ui", "expected an empty parameter object"));
    }
    Ok(json!({"action": "simulate"}))
}
fn simulate_machine_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.simulate.machine", "expected an empty parameter object"));
    }
    Ok(json!({"action": "simulate_machine"}))
}
fn post_ui_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.post.ui", "expected an empty parameter object"));
    }
    Ok(json!({"action": "post", "dialog": "nc_program"}))
}
fn setup_sheet_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.setup_sheet", "expected an empty parameter object"));
    }
    Ok(json!({"action": "setup_sheet"}))
}
fn machining_time_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.machining_time", "expected an empty parameter object"));
    }
    Ok(json!({"action": "machining_time"}))
}
fn tools_open_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.tools", "expected an empty parameter object"));
    }
    Ok(json!({"panel": "tool_library"}))
}
fn machines_open_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.machines", "expected an empty parameter object"));
    }
    Ok(json!({"panel": "setup"}))
}
fn manage_stub_cmd(_s: &mut Session, params: &Value) -> Result<Value> {
    if !params.as_object().is_some_and(|p| p.is_empty()) {
        return Err(bad("manufacture.manage", "expected an empty parameter object"));
    }
    Ok(json!({"action": "manage_stub"}))
}

// ── Simulate ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimulateParams {
    job: CamJobId,
    setup: String,
    /// Select tool by number (default: first tool in the setup's library).
    #[serde(default)]
    tool_number: Option<u32>,
    // SimOptions overrides (all optional)
    #[serde(default)]
    grid_cols: Option<u32>,
    #[serde(default)]
    grid_rows: Option<u32>,
    #[serde(default)]
    clearance_z: Option<f64>,
    #[serde(default)]
    max_frames: Option<u32>,
    #[serde(default)]
    interp_step_mm: Option<f64>,
    #[serde(default)]
    machine_xy_travel: Option<[f64; 2]>,
    #[serde(default)]
    machine_z_min: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fixtures: Vec<Fixture>,
}

fn simulate(s: &mut Session, params: &Value) -> Result<Value> {
    const CMD: &str = "manufacture.simulate";

    let p: SimulateParams = serde_json::from_value(params.clone()).map_err(|e| bad(CMD, e.to_string()))?;

    ManufactureSettings::validate_name(&p.setup).map_err(|e| bad(CMD, e.to_string()))?;

    let setup = s.doc.manufacture.setups.get(&p.setup).ok_or_else(|| bad(CMD, format!("no manufacture setup named `{}`", p.setup)))?.clone();

    if setup.tools.is_empty() {
        return Err(bad(CMD, "setup has no tools; cannot simulate toolpaths"));
    }

    // Resolve tool: by number if specified, otherwise the first tool.
    let tool = match p.tool_number {
        Some(n) => {
            setup.tools.iter().find(|t| t.number == n).ok_or_else(|| bad(CMD, format!("tool number {n} not found in setup `{}`", p.setup)))?.clone()
        }
        None => setup.tools.first().ok_or_else(|| bad(CMD, "setup has no tools"))?.clone(),
    };

    // Retrieve completed job result (segments).
    let cam_result = s.manufacture_jobs.result(p.job).map_err(job_error)?;

    // Build SimOptions from overrides (fall back to defaults).
    let defaults = SimOptions::default();
    let opts = SimOptions {
        grid_cols: p.grid_cols.unwrap_or(defaults.grid_cols),
        grid_rows: p.grid_rows.unwrap_or(defaults.grid_rows),
        clearance_z: p.clearance_z.unwrap_or(defaults.clearance_z),
        max_frames: p.max_frames.unwrap_or(defaults.max_frames),
        interp_step_mm: p.interp_step_mm.or(defaults.interp_step_mm),
        machine_xy_travel: p.machine_xy_travel.or(defaults.machine_xy_travel),
        machine_z_min: p.machine_z_min.or(defaults.machine_z_min),
        fixtures: p.fixtures,
    };

    // Run the simulation (pure; no mutation of session state).
    let result = simulate_toolpaths(&setup.stock, &tool, &cam_result.segments, &opts);

    serde_json::to_value(&result).map_err(|e| bad(CMD, format!("could not serialise result: {e}")))
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_manufacture::job::CamHost;
    use solidcraft_manufacture::{FaceOperation, Feeds, MfgSetup, Operation, Stock, Tool, ToolKind, WorkCoordinateSystem};

    fn mfg_session() -> Session {
        let mut s = Session::default();
        s.set_manufacture_job_host(Some(CamHost::fake()));
        s
    }

    fn make_setup() -> MfgSetup {
        MfgSetup {
            stock: Stock::Box { width: 100.0, depth: 80.0, height: 20.0 },
            wcs: WorkCoordinateSystem::default(),
            tools: vec![Tool {
                number: 1,
                kind: ToolKind::FlatEndmill,
                diameter: 6.0,
                corner_radius: 0.0,
                length: 50.0,
                flute_length: 16.0,
                flutes: 4,
                description: String::new(),
                ..Default::default()
            }],
            operations: vec![Operation::Face(FaceOperation {
                tool_number: 1,
                feeds: Feeds::default(),
                depth: 0.5,
                stepover: 0.75,
                ..Default::default()
            })],
            machine: None,
            post: None,
            ..Default::default()
        }
    }

    fn add_setup(s: &mut Session) {
        let mut mfg = s.doc.manufacture.clone();
        mfg.setups.insert("Setup1".into(), make_setup());
        s.doc_mut().manufacture = mfg;
    }

    #[test]
    fn backends_returns_descriptor() {
        let mut s = mfg_session();
        let r = s.execute("manufacture.backends", &json!({})).unwrap();
        assert_eq!(r["schema_version"], 1);
        assert!(r["backends"].as_array().is_some_and(|b| !b.is_empty()));
        let presets = r["post_presets"].as_array().expect("post_presets");
        assert!(presets.iter().any(|p| p["id"] == "GRBL"));
        assert!(presets.iter().any(|p| p["id"] == "Mach3"));
    }

    #[test]
    fn ensure_post_presets_is_idempotent() {
        let mut s = Session::default();
        let r = s.execute("manufacture.posts.ensure_presets", &json!({})).unwrap();
        assert_eq!(r["added"].as_array().unwrap().len(), solidcraft_manufacture::post::preset_posts().len());
        assert!(s.doc.manufacture.posts.contains_key("GRBL"));
        assert!(s.doc.manufacture.posts.contains_key("Haas"));
        let undo = s.undo.len();
        let r2 = s.execute("manufacture.posts.ensure_presets", &json!({})).unwrap();
        assert!(r2["added"].as_array().unwrap().is_empty());
        assert_eq!(s.undo.len(), undo, "second ensure must not push undo");
    }

    #[test]
    fn ensure_machine_presets_is_idempotent_and_lists_atc() {
        let mut s = Session::default();
        let r = s.execute("manufacture.machines.ensure_presets", &json!({})).unwrap();
        assert_eq!(r["added"].as_array().unwrap().len(), solidcraft_manufacture::machines::preset_machines().len());
        assert!(s.doc.manufacture.machines.contains_key("Haas VF-2"));
        assert_eq!(s.doc.manufacture.machines["Haas VF-2"].tool_changer.pockets, 20);
        let undo = s.undo.len();
        let r2 = s.execute("manufacture.machines.ensure_presets", &json!({})).unwrap();
        assert!(r2["added"].as_array().unwrap().is_empty());
        assert_eq!(s.undo.len(), undo, "second ensure must not push undo");
        let b = s.execute("manufacture.backends", &json!({})).unwrap();
        let machines = b["machine_presets"].as_array().expect("machine_presets");
        assert!(machines.iter().any(|m| m["id"] == "Okuma GENOS M560" && m["integration"] == "thinc"));
        assert!(machines.iter().any(|m| m["id"] == "Fanuc 0i-MF" && m["integration"] == "focas"));
        assert!(machines.iter().any(|m| m["id"] == "Haas VF-2" && m["probe"] == true));
        assert!(b["probe_cycles"].as_array().is_some_and(|c| c.len() >= 6));
    }

    #[test]
    fn probe_snippet_emits_inspection_plus() {
        let mut s = Session::default();
        let r = s
            .execute(
                "manufacture.probe.snippet",
                &json!({
                    "cycle": "single_surface",
                    "style": "haas",
                    "tool_number": 25,
                    "z": 0.0,
                    "work_offset_s": 1,
                    "overtravel_q": 0.25
                }),
            )
            .unwrap();
        assert_eq!(r["review_only"], true);
        let g = r["gcode"].as_str().unwrap();
        assert!(g.contains("G65 P9811"), "{g}");
        assert!(g.contains("G65 P9832"), "{g}");
        assert!(g.contains("T25 M6"), "{g}");
    }

    #[test]
    fn machine_plugin_fake_status_roundtrip() {
        let mut s = Session::default();
        let use_r = s.execute("manufacture.machine.use", &json!({"plugin": "fake"})).unwrap();
        assert_eq!(use_r["plugin"], "fake");
        let st = s.execute("manufacture.machine.status", &json!({})).unwrap();
        assert_eq!(st["status"]["source"], "fake");
        assert_eq!(st["status"]["execution"], "READY");
        assert_eq!(st["status"]["tool_number"], 1);
        let plugins = s.execute("manufacture.machine.plugins", &json!({})).unwrap();
        assert_eq!(plugins["active"]["id"], "fake-machine");
        s.execute("manufacture.machine.use", &json!({"plugin": "none"})).unwrap();
        let err = s.execute("manufacture.machine.status", &json!({})).unwrap_err().to_string();
        assert!(err.contains("no machine plugin"), "{err}");
    }

    #[test]
    fn ribbon_op_face_appends_default_tool_and_op() {
        let mut s = Session::default();
        s.execute("manufacture.setup.new", &json!({"name": "Mill1"})).unwrap();
        let r = s.execute("manufacture.op.face", &json!({"setup": "Mill1"})).unwrap();
        assert_eq!(r["operations"], 1);
        let setup = &s.doc.manufacture.setups["Mill1"];
        assert_eq!(setup.tools.len(), 1);
        assert_eq!(setup.operations.len(), 1);
        assert!(matches!(setup.operations[0], solidcraft_manufacture::Operation::Face(_)));
    }

    #[test]
    fn part_handling_ribbon_ops_append() {
        let mut s = Session::default();
        s.execute("manufacture.setup.new", &json!({"name": "Turn1"})).unwrap();
        for (cmd, kind) in [
            ("manufacture.part_handling.subspindle_grab", PartHandlingKind::SubspindleGrab),
            ("manufacture.part_handling.bar_pull", PartHandlingKind::BarPull),
            ("manufacture.part_handling.subspindle_return", PartHandlingKind::SubspindleReturn),
            ("manufacture.part_handling.tool_call", PartHandlingKind::ToolCall),
        ] {
            s.execute(cmd, &json!({"setup": "Turn1"})).unwrap();
            let setup = &s.doc.manufacture.setups["Turn1"];
            let op = setup.operations.last().unwrap();
            match op {
                solidcraft_manufacture::Operation::PartHandling(o) => assert_eq!(o.kind, kind),
                other => panic!("expected PartHandling, got {other:?}"),
            }
        }
        assert_eq!(s.doc.manufacture.setups["Turn1"].operations.len(), 4);
    }

    #[test]
    fn library_remote_local_sync_and_install() {
        let tmp = std::env::temp_dir().join(format!("sc-lib-eng-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("posts")).unwrap();
        let idx = solidcraft_manufacture::library::example_index();
        // Ship a real post file for the example entry so install works.
        let post = solidcraft_manufacture::post::preset_posts()[0].1.clone();
        std::fs::create_dir_all(tmp.join("posts/Generic")).unwrap();
        std::fs::write(tmp.join("posts/Generic/grbl-laser.post.json"), serde_json::to_vec_pretty(&post).unwrap()).unwrap();
        std::fs::write(tmp.join("index.json"), serde_json::to_vec_pretty(&idx).unwrap()).unwrap();
        let mut s = Session::default();
        s.execute("manufacture.library.add_remote", &json!({"url": tmp.to_string_lossy(), "id": "test-cam", "name": "Test CAM"})).unwrap();
        let sync = s.execute("manufacture.library.sync", &json!({"id": "test-cam"})).unwrap();
        assert_eq!(sync["results"][0]["ok"], true);
        let list = s.execute("manufacture.library.list", &json!({"kind": "post"})).unwrap();
        let entries = list["entries"].as_array().unwrap();
        assert!(entries.iter().any(|e| e["location"] == "linked"));
        let linked_id = entries.iter().find(|e| e["location"] == "linked" && e["kind"] == "post").unwrap()["id"].as_str().unwrap().to_string();
        s.execute("manufacture.library.install", &json!({"entry_id": linked_id})).unwrap();
        assert!(!s.doc.manufacture.posts.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn fabrication_2d_profile_appends() {
        let mut s = Session::default();
        s.execute("manufacture.setup.new", &json!({"name": "Cut1"})).unwrap();
        let r = s.execute("manufacture.op.fab_2d_profile", &json!({"setup": "Cut1"})).unwrap();
        assert_eq!(r["operations"], 1);
        assert!(matches!(&s.doc.manufacture.setups["Cut1"].operations[0], solidcraft_manufacture::Operation::FabricationProfile(_)));
        let nest = s.execute("manufacture.nest", &json!({})).unwrap();
        assert_eq!(nest["dialog"], "nest");
    }

    #[test]
    fn additive_from_gcode_and_inspection_ops() {
        let mut s = Session::default();
        let r = s
            .execute(
                "manufacture.setup.from_gcode",
                &json!({
                    "name": "Print1",
                    "source_path": "part.gcode",
                    "gcode": "G0 X0 Y0 Z0.2\nG1 X10 Y0 F1200\nG1 X10 Y10\n"
                }),
            )
            .unwrap();
        assert_eq!(r["setup"], "Print1");
        assert!(matches!(&s.doc.manufacture.setups["Print1"].operations[0], solidcraft_manufacture::Operation::Additive(_)));
        let sim = s.execute("manufacture.additive.simulate", &json!({"setup": "Print1"})).unwrap();
        assert!(sim["segments"].as_array().is_some_and(|a| !a.is_empty()));

        s.execute("manufacture.setup.new", &json!({"name": "Insp1"})).unwrap();
        s.execute("manufacture.op.probe_wcs", &json!({"setup": "Insp1"})).unwrap();
        s.execute("manufacture.op.probe_geometry", &json!({"setup": "Insp1"})).unwrap();
        s.execute("manufacture.op.inspect_surface", &json!({"setup": "Insp1"})).unwrap();
        s.execute("manufacture.op.part_alignment", &json!({"setup": "Insp1"})).unwrap();
        s.execute("manufacture.op.manual_inspection", &json!({"setup": "Insp1"})).unwrap();
        s.execute("manufacture.op.manual_inspection_record", &json!({"setup": "Insp1"})).unwrap();
        assert_eq!(s.doc.manufacture.setups["Insp1"].operations.len(), 6);
    }

    #[test]
    fn machine_plugin_mtconnect_fixture_no_network() {
        let mut s = Session::default();
        let r =
            s.execute("manufacture.machine.use", &json!({"plugin": "mtconnect", "fixture": true, "endpoint": "http://127.0.0.1:8082/VF-2"})).unwrap();
        assert_eq!(r["plugin"], "mtconnect");
        let st = s.execute("manufacture.machine.status", &json!({})).unwrap();
        assert_eq!(st["status"]["source"], "mtconnect");
        assert_eq!(st["status"]["execution"], "ACTIVE");
        assert_eq!(st["status"]["tool_number"], 7);
        assert_eq!(st["status"]["program"], "O2001");
    }

    #[test]
    fn local_gcode_posts_completed_job() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        let mut mfg = s.doc.manufacture.clone();
        let mut setup = make_setup();
        setup.post = Some("GRBL".into());
        mfg.setups.insert("Setup1".into(), setup);
        for (id, profile) in solidcraft_manufacture::post::preset_posts() {
            mfg.posts.insert(id.into(), profile);
        }
        s.doc_mut().manufacture = mfg;
        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();
        // Fake host returns empty segments → local post refuses empty toolpath.
        let err = s.execute("manufacture.gcode", &json!({"job": job})).unwrap_err().to_string();
        assert!(err.contains("no segments") || err.contains("toolpath"), "{err}");
    }

    #[test]
    fn settings_get_returns_empty_by_default() {
        let mut s = Session::default();
        let r = s.execute("manufacture.settings.get", &json!({})).unwrap();
        assert!(r["setups"].as_object().is_some_and(|m| m.is_empty()));
        assert!(r["machines"].as_object().is_some_and(|m| m.is_empty()));
    }

    #[test]
    fn settings_set_creates_and_returns_setup() {
        let mut s = Session::default();
        let patch = json!({
            "stock": {"kind": "box", "width": 100.0, "depth": 80.0, "height": 20.0},
            "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0, "corner_radius": 0.0,
                       "length": 50.0, "flute_length": 16.0, "flutes": 4}],
            "operations": [{"type": "face", "tool_number": 1, "depth": 0.5, "stepover": 0.75}]
        });
        let r = s.execute("manufacture.settings.set", &json!({"name": "Setup1", "set": patch})).unwrap();
        assert_eq!(r["name"], "Setup1");
        assert!(s.doc.manufacture.setups.contains_key("Setup1"));
    }

    #[test]
    fn settings_set_noop_if_unchanged() {
        let mut s = Session::default();
        let patch = json!({
            "stock": {"kind": "box", "width": 100.0, "depth": 80.0, "height": 20.0}
        });
        s.execute("manufacture.settings.set", &json!({"name": "S", "set": patch.clone()})).unwrap();
        let undo_before = s.undo.len();
        // Identical patch → no undo step.
        s.execute("manufacture.settings.set", &json!({"name": "S", "set": patch})).unwrap();
        assert_eq!(s.undo.len(), undo_before, "no-op must not create an undo step");
    }

    #[test]
    fn settings_set_rejects_invalid_name() {
        let mut s = Session::default();
        let r = s.execute("manufacture.settings.set", &json!({"name": " bad", "set": {}}));
        assert!(r.is_err());
    }

    #[test]
    fn runtime_commands_do_not_touch_document_or_undo() {
        let mut s = mfg_session();
        // Add a box body so STEP export succeeds, then add a setup.
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let doc = s.doc.clone();
        let rev = s.revision;
        let undo_len = s.undo.len();

        s.execute("manufacture.preflight", &json!({"setup": "Setup1"})).unwrap();
        let status = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = status["job"].clone();
        s.execute("manufacture.status", &json!({"job": job})).unwrap();
        s.execute("manufacture.result", &json!({"job": job})).unwrap();
        s.execute("manufacture.cancel", &json!({"job": job})).unwrap();

        // Nothing changed in the document.
        assert!(std::sync::Arc::ptr_eq(&doc, &s.doc));
        assert_eq!(rev, s.revision);
        assert_eq!(undo_len, s.undo.len());
    }

    #[test]
    fn preflight_rejects_missing_setup() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        assert!(s.execute("manufacture.preflight", &json!({"setup": "NoSuch"})).is_err());
    }

    #[test]
    fn preflight_rejects_empty_setup_operations() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        // Insert a setup with no operations.
        let mut mfg = s.doc.manufacture.clone();
        mfg.setups.insert("Empty".into(), MfgSetup::default());
        s.doc_mut().manufacture = mfg;
        assert!(s.execute("manufacture.preflight", &json!({"setup": "Empty"})).is_err());
    }

    #[test]
    fn preflight_rejects_empty_geometry() {
        let mut s = mfg_session();
        // No bodies in the model.
        add_setup(&mut s);
        assert!(s.execute("manufacture.preflight", &json!({"setup": "Setup1"})).is_err());
    }

    #[test]
    fn start_requires_installed_host() {
        let mut s = Session::default(); // no host
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        assert!(s.execute("manufacture.start", &json!({"setup": "Setup1"})).is_err());
    }

    #[test]
    fn result_returns_segments_and_provenance() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();
        let r = s.execute("manufacture.result", &json!({"job": job})).unwrap();
        assert_eq!(r["provenance"]["setup"], "Setup1");
        assert!(r["segments"].as_array().is_some());
    }

    #[test]
    fn status_isolation_between_sessions() {
        let mut a = mfg_session();
        let mut b = mfg_session();
        for s in [&mut a, &mut b] {
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
            add_setup(s);
        }
        let sta = a.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job_a = sta["job"].clone();
        // Session b cannot access session a's job.
        assert!(b.execute("manufacture.status", &json!({"job": job_a})).is_err());
    }

    // ── manufacture.simulate ─────────────────────────────────────────────────

    /// Simulate with a completed FakeCamHost job (empty segments) → nothing removed, no errors.
    #[test]
    fn simulate_empty_segments_no_errors() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();

        let r = s.execute("manufacture.simulate", &json!({"job": job, "setup": "Setup1"})).unwrap();
        assert_eq!(r["removed_volume_mm3"], 0.0);
        assert!(!r["has_errors"].as_bool().unwrap_or(true));
        assert!(r["final_heights"].as_array().is_some());
        assert_eq!(r["grid_cols"], 256);
        assert_eq!(r["grid_rows"], 256);
    }

    /// Simulate does not touch the document or undo stack.
    #[test]
    fn simulate_is_non_mutating() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let doc_before = s.doc.clone();
        let rev_before = s.revision;
        let undo_before = s.undo.len();

        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();
        s.execute("manufacture.simulate", &json!({"job": job, "setup": "Setup1"})).unwrap();

        assert!(std::sync::Arc::ptr_eq(&doc_before, &s.doc), "doc changed during simulate");
        assert_eq!(rev_before, s.revision);
        assert_eq!(undo_before, s.undo.len());
    }

    /// simulate rejects a missing setup.
    #[test]
    fn simulate_rejects_missing_setup() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();
        assert!(s.execute("manufacture.simulate", &json!({"job": job, "setup": "NoSuch"})).is_err());
    }

    /// simulate accepts grid_cols / grid_rows overrides.
    #[test]
    fn simulate_accepts_grid_overrides() {
        let mut s = mfg_session();
        s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
        add_setup(&mut s);
        let st = s.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job = st["job"].clone();
        let r = s.execute("manufacture.simulate", &json!({"job": job, "setup": "Setup1", "grid_cols": 32, "grid_rows": 32})).unwrap();
        assert_eq!(r["grid_cols"], 32);
        assert_eq!(r["grid_rows"], 32);
        let heights = r["final_heights"].as_array().unwrap();
        assert_eq!(heights.len(), 32 * 32);
    }

    /// simulate rejects a job that belongs to another session.
    #[test]
    fn simulate_rejects_wrong_session_job() {
        let mut a = mfg_session();
        let mut b = mfg_session();
        for s in [&mut a, &mut b] {
            s.execute("solid.box", &json!({"length": 10, "width": 10, "height": 10})).unwrap();
            add_setup(s);
        }
        let sta = a.execute("manufacture.start", &json!({"setup": "Setup1"})).unwrap();
        let job_a = sta["job"].clone();
        assert!(b.execute("manufacture.simulate", &json!({"job": job_a, "setup": "Setup1"})).is_err());
    }
}
