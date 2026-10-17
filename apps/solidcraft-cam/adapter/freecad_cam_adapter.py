#!/usr/bin/env python3
"""
FreeCAD / ocp-freecad-cam 2.x toolpath adapter for solidcraft-cam.

Protocol
--------
Reads one JSON object from stdin, writes one JSON object to stdout.
No other output is written to stdout; diagnostics go to stderr.

Input schema:
  {
    "step_path":   "<absolute path to STEP file>",
    "manufacture": { <ManufactureSettings JSON from crates/manufacture> },
    "probe":       true | false   // optional; when true only check imports
  }

Output — success:
  { "status": "ok", "segments": [...], "post_output": "<gcode>"|null }

Output — backend not installed:
  { "status": "unavailable", "message": "<reason>" }

Output — operation cannot be executed from current domain data (bad request):
  { "status": "preflight_error", "message": "<reason with extension hint>" }

Output — runtime error:
  { "status": "error", "message": "<reason>" }

Pinned versions (kept in sync with config.rs constants):
  FreeCAD:          1.1.1   (FREECAD_VERSION_PIN)
  ocp-freecad-cam:  2.0.2   (OCP_FREECAD_CAM_VERSION_PIN)
  cadquery:         2.4+    (required by ocp-freecad-cam 2.x)

ocp-freecad-cam 2.0.2 public API used here
-------------------------------------------
  from ocp_freecad_cam import Job, Endmill, Drill, Ballnose, Bullnose

  # Job is immutable / fluent: each operation returns a *new* Job
  job = Job(top_face_cq, model_cq, post_processor="linuxcnc"|"grbl"|...)
  job = job.face(face_cq, tool, *, step_down=...)
  job = job.profile(shapes, tool, *, side="out"|"in"|"mid", step_down=...)
  job = job.pocket(shapes, tool, *, step_over=<percent>, step_down=...)
  job = job.drill(shapes, tool, *, peck_depth=None, chip_break_enabled=False,
                  dwell_time=None, ...)
  job = job.adaptive(shapes, tool, *, step_over=<percent>,
                     stock_to_leave=..., step_down=...)
  gcode: str = job.to_gcode()  # triggers build; no separate .run()

  Tool classes (all use keyword-only kwargs):
    Endmill(diameter, length, cutting_edge_height, h_feed, v_feed, speed)
    Ballnose(...)   # same params as Endmill
    Bullnose(...)   # same params as Endmill
    Drill(diameter, length, h_feed, v_feed, speed)

  Path commands live at job.job_impl.fc_job.Path.Commands after to_gcode().
  Each command: .Name (str), .Parameters (dict of float, keys X/Y/Z/F/I/J/K)

Geometric selection
-------------------
  ManufactureSettings operations may carry a "targets" block:
    { "face_sample_points": [[x,y,z], ...], "drill_points": [[x,y,z], ...] }

  resolve_faces()       — selects nearest face center for each sample point
  resolve_drill_targets() — selects nearest cylindrical face for each drill point

  Both raise PreflightError when targets are absent, out-of-tolerance, or ambiguous.
  Silent skips are never permitted.

Licence: MIT OR Apache-2.0 (same as SolidCraft).
"""

from __future__ import annotations

import json
import math
import os
import sys
import traceback
from typing import Any

FREECAD_PIN = "1.1.1"
OCP_FREECAD_CAM_PIN = "2.0.2"

# G-code dialect → ocp-freecad-cam 2.x post-processor name
DIALECT_TO_POST: dict[str, str] = {
    "generic":   "grbl",
    "fanuc":     "fanuc",
    "haas":      "haas",
    "grbl":      "grbl",
    "linux_cnc": "linuxcnc",
}
DEFAULT_POST = "linuxcnc"

# Face selection tolerances for resolve_faces() / resolve_drill_targets()
FACE_SELECT_TOLERANCE_MM: float = 5.0
FACE_SELECT_TIE_TOLERANCE_MM: float = 0.5


# ── Custom exceptions ─────────────────────────────────────────────────────────

class PreflightError(Exception):
    """
    Raised when an operation cannot be configured from ManufactureSettings alone.

    The message names the operation index, the missing field, and hints at the
    domain extension required.
    """


# ── Output helpers ────────────────────────────────────────────────────────────

def _write(obj: dict[str, Any]) -> None:
    print(json.dumps(obj), flush=True)


def _unavailable(msg: str) -> None:
    _write({"status": "unavailable", "message": msg})


def _preflight_error(msg: str) -> None:
    _write({"status": "preflight_error", "message": msg})


def _error(msg: str) -> None:
    _write({"status": "error", "message": msg})


def _ok(segments: list[dict[str, Any]], post_output: str | None = None) -> None:
    result: dict[str, Any] = {"status": "ok", "segments": segments}
    if post_output is not None:
        result["post_output"] = post_output
    _write(result)


# ── Entry point ───────────────────────────────────────────────────────────────

def main() -> None:
    try:
        payload = json.load(sys.stdin)
    except Exception as exc:
        _error(f"failed to parse stdin as JSON: {exc}")
        return

    probe = bool(payload.get("probe", False))

    # ── Import checks (in dependency order) ──────────────────────────────────
    try:
        import FreeCAD as _freecad  # noqa: F401
    except ImportError as exc:
        _unavailable(
            f"FreeCAD not importable (run via FreeCAD's Python interpreter; "
            f"expected FreeCAD {FREECAD_PIN}): {exc}"
        )
        return

    try:
        import cadquery as cq
    except ImportError as exc:
        _unavailable(
            f"cadquery not importable (required by ocp-freecad-cam 2.x; "
            f"install cadquery in the FreeCAD Python environment): {exc}"
        )
        return

    try:
        from ocp_freecad_cam import Job
    except ImportError as exc:
        _unavailable(
            f"ocp_freecad_cam.Job not importable "
            f"(install: pip install ocp-freecad-cam=={OCP_FREECAD_CAM_PIN}): {exc}"
        )
        return

    # Tool classes — try flat namespace (2.0.x) then sub-module (future)
    tool_classes: dict[str, Any] = {}
    try:
        from ocp_freecad_cam import Endmill, Drill, Ballnose, Bullnose  # type: ignore[import]
        tool_classes = {
            "Endmill": Endmill, "Drill": Drill,
            "Ballnose": Ballnose, "Bullnose": Bullnose,
        }
    except ImportError:
        try:
            from ocp_freecad_cam.tools import Endmill, Drill, Ballnose, Bullnose  # type: ignore[import]
            tool_classes = {
                "Endmill": Endmill, "Drill": Drill,
                "Ballnose": Ballnose, "Bullnose": Bullnose,
            }
        except ImportError as exc:
            _unavailable(
                f"ocp-freecad-cam 2.x tool classes (Endmill, Drill, Ballnose, Bullnose) "
                f"not found — is version {OCP_FREECAD_CAM_PIN} installed? {exc}"
            )
            return

    if probe:
        _ok([], None)
        return

    # ── Validate inputs ───────────────────────────────────────────────────────
    step_path: str = payload.get("step_path", "")
    if not step_path or not os.path.isabs(step_path) or not os.path.isfile(step_path):
        _error(f"step_path is invalid or not accessible: {step_path!r}")
        return

    manufacture: dict[str, Any] = payload.get("manufacture", {})
    setups: dict[str, Any] = manufacture.get("setups", {})
    if not setups:
        _ok([], None)
        return

    try:
        segments, post_output = process(step_path, manufacture, cq, Job, tool_classes)
        _ok(segments, post_output)
    except PreflightError as exc:
        _preflight_error(str(exc))
    except Exception as exc:  # noqa: BLE001
        _error(f"toolpath computation failed: {exc}\n{traceback.format_exc()}")


# ── Core computation ──────────────────────────────────────────────────────────

def process(
    step_path: str,
    manufacture: dict[str, Any],
    cq: Any,
    Job: Any,
    tool_classes: dict[str, Any],
) -> tuple[list[dict[str, Any]], str | None]:
    """
    Load the STEP file with CadQuery and compute toolpath segments.

    All FreeCAD/CQ/ocp-freecad-cam dependencies are passed as arguments so that
    contract tests can inject mocks without installing those packages.

    Returns (segments, post_output_or_None).
    Raises PreflightError / ValueError for bad operations (never silently skips).
    """
    model = cq.importers.importStep(step_path)

    # Top face selected once; used as the Job reference plane and for face ops.
    top_face = model.faces(">Z")

    setups: dict[str, Any] = manufacture.get("setups", {})
    posts: dict[str, Any] = manufacture.get("posts", {})

    all_segments: list[dict[str, Any]] = []
    all_gcode_lines: list[str] = []

    for setup_name, setup in setups.items():
        post_dialect = _resolve_post_dialect(setup, posts)
        post_processor = DIALECT_TO_POST.get(post_dialect, DEFAULT_POST)

        # ocp-freecad-cam 2.x: Job is immutable/fluent; each op returns a new Job.
        job = Job(top_face, model, post_processor=post_processor)

        tools: dict[int, dict[str, Any]] = {
            t["number"]: t for t in setup.get("tools", [])
        }
        operations: list[dict[str, Any]] = setup.get("operations", [])

        for i, op in enumerate(operations):
            tool_number: int = op.get("tool_number", 1)
            tool_info: dict[str, Any] = tools.get(tool_number, {})
            feeds: dict[str, Any] = op.get("feeds", {})
            tool_obj = make_tool(tool_info, tool_classes, feeds)
            # Returns modified job (fluent API); raises PreflightError/ValueError on bad op
            job = add_operation(job, model, top_face, op, tool_obj, setup_name, i)

        # to_gcode() triggers the FreeCAD rebuild; no separate job.run() call exists.
        gcode: str | None = None
        try:
            result = job.to_gcode()
            if isinstance(result, str):
                gcode = result
        except AttributeError:
            pass  # post_output is optional per the adapter protocol

        segs = extract_segments(job)
        all_segments.extend(segs)
        if gcode:
            all_gcode_lines.append(f"; --- setup: {setup_name} ---")
            all_gcode_lines.append(gcode)

    post_output = "\n".join(all_gcode_lines) or None
    return all_segments, post_output


def _resolve_post_dialect(setup: dict[str, Any], posts: dict[str, Any]) -> str:
    """Return the GcodeDialect string for this setup, defaulting to 'generic'."""
    post_name = setup.get("post")
    if post_name and post_name in posts:
        return posts[post_name].get("dialect", "generic")
    return "generic"


# ── Tool factory ──────────────────────────────────────────────────────────────

def make_tool(
    tool_info: dict[str, Any],
    tool_classes: dict[str, Any],
    feeds: dict[str, Any],
) -> Any:
    """
    Create an ocp-freecad-cam 2.0.2 tool object from ManufactureSettings data.

    ocp-freecad-cam 2.0.2 tool constructors (all kwargs, kw_only=True dataclasses):
      Endmill(diameter, length, cutting_edge_height, h_feed, v_feed, speed)
      Ballnose(...)      — same params as Endmill
      Bullnose(...)      — same params as Endmill
      Drill(diameter, length, h_feed, v_feed, speed)

    Feed rates travel with the tool, not with the operation call.
    """
    kind: str = tool_info.get("kind", "flat_endmill")
    diameter: float = float(tool_info.get("diameter", 6.0))
    length: float = float(tool_info.get("length", 50.0))
    # "flute_length" in the domain maps to "cutting_edge_height" in the API
    cutting_edge_height: float = float(tool_info.get("flute_length", 16.0))
    h_feed: float = float(feeds.get("feedrate", 1000.0))
    v_feed: float = float(feeds.get("plunge", 300.0))
    speed: float = float(feeds.get("spindle_rpm", 10_000))

    if kind == "drill":
        return tool_classes["Drill"](
            diameter=diameter,
            length=length,
            h_feed=h_feed,
            v_feed=v_feed,
            speed=speed,
        )
    elif kind == "ball_endmill":
        return tool_classes["Ballnose"](
            diameter=diameter,
            length=length,
            cutting_edge_height=cutting_edge_height,
            h_feed=h_feed,
            v_feed=v_feed,
            speed=speed,
        )
    elif kind == "bullnose_endmill":
        # Use Bullnose if available, otherwise fall back to Endmill
        cls = tool_classes.get("Bullnose") or tool_classes["Endmill"]
        return cls(
            diameter=diameter,
            length=length,
            cutting_edge_height=cutting_edge_height,
            h_feed=h_feed,
            v_feed=v_feed,
            speed=speed,
        )
    else:
        # flat_endmill, chamfer, facemill, tap, unknown → Endmill
        return tool_classes["Endmill"](
            diameter=diameter,
            length=length,
            cutting_edge_height=cutting_edge_height,
            h_feed=h_feed,
            v_feed=v_feed,
            speed=speed,
        )


# ── Shape resolution ──────────────────────────────────────────────────────────

def resolve_faces(
    model: Any,
    sample_points: list[list[float]],
    op_name: str,
) -> list[Any]:
    """
    Resolve face_sample_points to a list of cq.Face objects.

    For each sample point, selects the face whose Center() is nearest.

    Raises PreflightError when:
    - the STEP model has no faces
    - any point's nearest face centre > FACE_SELECT_TOLERANCE_MM away
    - two face centres are within FACE_SELECT_TIE_TOLERANCE_MM of each other
      (ambiguous — caller must adjust the sample point)
    """
    all_faces = model.faces().vals()
    if not all_faces:
        raise PreflightError(f"{op_name}: STEP model has no selectable faces")

    selected: list[Any] = []
    for i, point in enumerate(sample_points):
        px, py, pz = float(point[0]), float(point[1]), float(point[2])

        face_dists = sorted(
            [
                (
                    math.sqrt(
                        (f.Center().x - px) ** 2
                        + (f.Center().y - py) ** 2
                        + (f.Center().z - pz) ** 2
                    ),
                    f,
                )
                for f in all_faces
            ],
            key=lambda x: x[0],
        )

        best_dist, best_face = face_dists[0]

        if best_dist > FACE_SELECT_TOLERANCE_MM:
            raise PreflightError(
                f"{op_name}: face_sample_points[{i}] ({px:.3f},{py:.3f},{pz:.3f}) — "
                f"nearest face centre is {best_dist:.3f} mm away "
                f"(tolerance {FACE_SELECT_TOLERANCE_MM} mm). "
                f"Move the sample point to within {FACE_SELECT_TOLERANCE_MM} mm of the intended face centre."
            )

        if len(face_dists) > 1:
            second_dist = face_dists[1][0]
            gap = second_dist - best_dist
            if gap < FACE_SELECT_TIE_TOLERANCE_MM:
                raise PreflightError(
                    f"{op_name}: face_sample_points[{i}] ({px:.3f},{py:.3f},{pz:.3f}) — "
                    f"ambiguous: nearest face is {best_dist:.3f} mm away, "
                    f"second-nearest is {second_dist:.3f} mm away "
                    f"(gap {gap:.3f} mm < tie tolerance {FACE_SELECT_TIE_TOLERANCE_MM} mm). "
                    f"Adjust the sample point to be unambiguously closer to one face."
                )

        if not any(f is best_face for f in selected):
            selected.append(best_face)

    return selected


def resolve_drill_targets(
    model: Any,
    op_targets: dict[str, Any],
    op_name: str,
) -> list[Any]:
    """
    Resolve drill geometric targets to a list of face shapes.

    Prefers 'drill_points' (XY proximity to cylindrical faces).
    Falls back to 'face_sample_points' (3D proximity to any face).
    Raises PreflightError if both are absent or empty.

    Uses cylindrical faces (geomType() == 'CYLINDER') when available for drill_points;
    falls back to all faces if the model has no cylindrical faces.
    """
    drill_points: list[list[float]] = op_targets.get("drill_points", [])
    face_points: list[list[float]] = op_targets.get("face_sample_points", [])

    if not drill_points and not face_points:
        raise PreflightError(
            f"{op_name}: DrillOperation has no geometric targets. "
            f"Set targets.drill_points (list of [x,y,z] hole-centre XY coordinates in model space) "
            f"or targets.face_sample_points (3D proximity for cylindrical faces). "
            f"Extend crates/manufacture DrillOperation.targets in the document."
        )

    if drill_points:
        all_faces = model.faces().vals()
        cyl_faces = [f for f in all_faces if getattr(f, "geomType", lambda: "")() == "CYLINDER"]
        search_faces = cyl_faces if cyl_faces else all_faces

        if not search_faces:
            raise PreflightError(f"{op_name}: STEP model has no selectable faces for drilling")

        selected: list[Any] = []
        for i, point in enumerate(drill_points):
            px, py = float(point[0]), float(point[1])

            face_dists = sorted(
                [
                    (
                        math.sqrt((f.Center().x - px) ** 2 + (f.Center().y - py) ** 2),
                        f,
                    )
                    for f in search_faces
                ],
                key=lambda x: x[0],
            )

            best_dist, best_face = face_dists[0]
            kind_label = "cylindrical face" if cyl_faces else "face"

            if best_dist > FACE_SELECT_TOLERANCE_MM:
                raise PreflightError(
                    f"{op_name}: drill_points[{i}] ({px:.3f},{py:.3f}) — "
                    f"nearest {kind_label} centre is {best_dist:.3f} mm away (XY, "
                    f"tolerance {FACE_SELECT_TOLERANCE_MM} mm). "
                    f"Adjust the drill point to be within {FACE_SELECT_TOLERANCE_MM} mm of the hole centre."
                )

            if len(face_dists) > 1:
                second_dist = face_dists[1][0]
                gap = second_dist - best_dist
                if gap < FACE_SELECT_TIE_TOLERANCE_MM:
                    raise PreflightError(
                        f"{op_name}: drill_points[{i}] ({px:.3f},{py:.3f}) — "
                        f"ambiguous: two {kind_label} centres are {best_dist:.3f} mm and "
                        f"{second_dist:.3f} mm away (gap {gap:.3f} mm < "
                        f"tie tolerance {FACE_SELECT_TIE_TOLERANCE_MM} mm)."
                    )

            if not any(f is best_face for f in selected):
                selected.append(best_face)

        return selected

    # Fallback: use face_sample_points with 3D proximity
    return resolve_faces(model, face_points, op_name)


# ── Operation dispatcher ──────────────────────────────────────────────────────

def add_operation(
    job: Any,
    model: Any,
    top_face: Any,
    op: dict[str, Any],
    tool_obj: Any,
    setup_name: str,
    op_index: int,
) -> Any:
    """
    Dispatch one ManufactureSettings operation to the ocp-freecad-cam 2.x job.

    RETURNS the new Job (ocp-freecad-cam uses an immutable/fluent API).
    Raises PreflightError when geometric targets are absent or unresolvable.
    Raises ValueError for unknown operation types — never silently skips.

    Supported operations
    --------------------
    face      — auto-selects the highest-Z face (no targets needed)
    profile   — requires targets.face_sample_points
    pocket    — requires targets.face_sample_points
    drill     — requires targets.drill_points or targets.face_sample_points
    adaptive  — requires targets.face_sample_points

    ocp-freecad-cam 2.0.2 method signatures used
    ----------------------------------------------
    job.face(shapes, tool, *, step_down=...)
    job.profile(shapes, tool, *, side="out"|"in"|"mid", step_down=...)
    job.pocket(shapes, tool, *, step_over=<percent 0-100>, step_down=...)
    job.drill(shapes, tool, *, peck_depth=None, chip_break_enabled=False,
              dwell_time=None, ...)
    job.adaptive(shapes, tool, *, step_over=<percent 0-100>,
                 stock_to_leave=..., step_down=...)

    Feed rates are NOT passed to the operation; they are part of the tool object.
    step_over is in percent (0-100), not fraction (0-1).
    """
    op_type: str = op.get("type", "")
    targets: dict[str, Any] = op.get("targets", {})
    op_name = f"setup '{setup_name}' operations[{op_index}] type={op_type!r}"

    if op_type == "face":
        depth: float = float(op.get("depth", 0.5))
        # Face op auto-selects highest-Z face; no explicit shape selection needed.
        return job.face(top_face, tool_obj, step_down=depth)

    elif op_type == "profile":
        fsp = targets.get("face_sample_points", [])
        if not fsp:
            raise PreflightError(
                f"{op_name}: requires targets.face_sample_points — "
                f"add at least one model-space sample point near the intended profile edge/face. "
                f"Extend crates/manufacture ProfileOperation.targets with face_sample_points: Vec<[f64; 3]>."
            )
        shapes = resolve_faces(model, fsp, op_name)
        depth = float(op.get("depth", 3.0))
        side_map = {"outside": "out", "inside": "in", "on": "mid"}
        side = side_map.get(op.get("side", "outside"), "out")
        return job.profile(shapes, tool_obj, side=side, step_down=depth)

    elif op_type == "pocket":
        fsp = targets.get("face_sample_points", [])
        if not fsp:
            raise PreflightError(
                f"{op_name}: requires targets.face_sample_points — "
                f"add model-space sample points near each intended pocket face. "
                f"Extend crates/manufacture PocketOperation.targets with face_sample_points: Vec<[f64; 3]>."
            )
        shapes = resolve_faces(model, fsp, op_name)
        depth = float(op.get("depth", 3.0))
        stepover_frac: float = float(op.get("stepover", 0.45))
        # ocp-freecad-cam step_over is a percentage (0–100), not a fraction
        return job.pocket(shapes, tool_obj, step_over=stepover_frac * 100.0, step_down=depth)

    elif op_type == "drill":
        shapes = resolve_drill_targets(model, targets, op_name)
        peck_depth: float = float(op.get("peck_depth", 2.0))
        dwell_s: float = float(op.get("dwell_s", 0.0))
        cycle: str = op.get("cycle", "simple")

        peck_arg: float | None = None
        chip_break: bool = False
        dwell_arg: float | None = None

        if cycle == "peck":
            peck_arg = peck_depth
        elif cycle == "chip_break":
            peck_arg = peck_depth
            chip_break = True
        elif cycle == "dwell":
            dwell_arg = dwell_s if dwell_s > 0.0 else None

        return job.drill(
            shapes,
            tool_obj,
            peck_depth=peck_arg,
            chip_break_enabled=chip_break,
            dwell_time=dwell_arg,
        )

    elif op_type == "adaptive":
        fsp = targets.get("face_sample_points", [])
        if not fsp:
            raise PreflightError(
                f"{op_name}: requires targets.face_sample_points — "
                f"add model-space sample points near each intended clearing face. "
                f"Extend crates/manufacture AdaptiveOperation.targets with face_sample_points: Vec<[f64; 3]>."
            )
        shapes = resolve_faces(model, fsp, op_name)
        depth = float(op.get("depth", 4.0))
        optimal_load_frac: float = float(op.get("optimal_load", 0.35))
        finish_allowance: float = float(op.get("finish_allowance", 0.1))
        # step_over is a percentage (0–100)
        return job.adaptive(
            shapes,
            tool_obj,
            step_over=optimal_load_frac * 100.0,
            stock_to_leave=finish_allowance,
            step_down=depth,
        )

    else:
        raise ValueError(
            f"{op_name}: unknown operation type {op_type!r}. "
            f"Known types: face, profile, pocket, drill, adaptive."
        )


# ── Segment extraction ────────────────────────────────────────────────────────

def extract_segments(job: Any) -> list[dict[str, Any]]:
    """
    Walk the FreeCAD Path commands and convert to PathSegment dicts.

    After job.to_gcode() the job is built.  Real API path:
      job.job_impl.fc_job.Path.Commands

    Falls back to job.path.Commands for legacy mock compatibility.

    FreeCAD Path.Commands[i]:
      .Name        — "G0"/"G00" (rapid), "G1"/"G01" (linear),
                     "G2"/"G02" (CW arc), "G3"/"G03" (CCW arc)
      .Parameters  — dict with X, Y, Z, F, I, J, K (float, mm)

    I/J/K are offsets from the CURRENT position to the arc centre.
    """
    # Real ocp-freecad-cam 2.x path: job.job_impl.fc_job.Path.Commands
    job_impl = getattr(job, "job_impl", None)
    if job_impl is not None:
        fc_job = getattr(job_impl, "fc_job", None)
        if fc_job is not None:
            path_obj = getattr(fc_job, "Path", None)
            if path_obj is not None:
                commands = getattr(path_obj, "Commands", [])
                return _commands_to_segments(commands)

    # Legacy fallback (for mocks that set job.path.Commands)
    path_obj = getattr(job, "path", None) or getattr(job, "Path", None)
    if path_obj is None:
        return []
    commands = getattr(path_obj, "Commands", None) or path_obj
    if not commands:
        return []
    return _commands_to_segments(commands)


def _commands_to_segments(commands: Any) -> list[dict[str, Any]]:
    """Convert an iterable of FreeCAD Path commands to PathSegment dicts."""
    segments: list[dict[str, Any]] = []
    current: list[float] = [0.0, 0.0, 0.0]

    for cmd in commands:
        name: str = getattr(cmd, "Name", "")
        params: dict[str, float] = getattr(cmd, "Parameters", {})

        to = [
            float(params.get("X", current[0])),
            float(params.get("Y", current[1])),
            float(params.get("Z", current[2])),
        ]
        feed = float(params.get("F", 0.0))

        if name in ("G0", "G00"):
            segments.append({"kind": "rapid", "to": to})

        elif name in ("G1", "G01"):
            if feed <= 0.0:
                feed = 1000.0  # safe fallback; real toolpaths always have F
            segments.append({"kind": "linear", "to": to, "feed": feed})

        elif name in ("G2", "G02", "G3", "G03"):
            # I/J/K are relative to the current position (before the move)
            center = [
                current[0] + float(params.get("I", 0.0)),
                current[1] + float(params.get("J", 0.0)),
                current[2] + float(params.get("K", 0.0)),
            ]
            if feed <= 0.0:
                feed = 1000.0
            segments.append({
                "kind": "arc",
                "center": center,
                "to": to,
                "feed": feed,
                "clockwise": name in ("G2", "G02"),
            })

        # Other codes (G4 dwell, M-codes, T-codes, etc.) are not PathSegments.

        if name in ("G0", "G00", "G1", "G01", "G2", "G02", "G3", "G03"):
            current = to

    return segments


if __name__ == "__main__":
    main()
