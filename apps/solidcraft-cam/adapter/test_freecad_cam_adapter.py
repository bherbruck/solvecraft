"""
Contract tests for freecad_cam_adapter.py.

Run WITHOUT FreeCAD, cadquery or ocp-freecad-cam installed:
  python -m pytest apps/solidcraft-cam/adapter/test_freecad_cam_adapter.py -v
  # or from the adapter directory:
  python -m pytest test_freecad_cam_adapter.py -v

Verified behaviours
-------------------
 1.  Import-failure paths → correct unavailable responses (via main())
 2.  probe=True → immediate ok (via main())
 3.  step_path validation → error response (via main())
 4.  make_tool() → correct Endmill/Drill/Ballnose/Bullnose dispatch and kwargs
       - uses 'cutting_edge_height', NOT 'flutes'
       - passes h_feed / v_feed / speed from the feeds dict
 5.  resolve_faces() → nearest face, tolerance & tie-tolerance enforcement
 6.  resolve_drill_targets() → drill_points / face_sample_points / empty → preflight
 7.  add_operation() face  → job.face(top_face, tool, step_down=depth) returned
 8.  add_operation() profile / pocket / adaptive → resolve faces, correct API call
 9.  add_operation() drill → resolve drill targets, correct API call
10.  add_operation() missing targets → PreflightError (never silent skip)
11.  add_operation() unknown type → ValueError (never silent skip)
12.  extract_segments() G0/G1/G2/G3 → correct PathSegment dicts
       - arc centre uses CURRENT position + I/J/K (not destination)
       - real API path: job.job_impl.fc_job.Path.Commands
13.  process() wires Job(top_face, model, post_processor=...) construction
       - captures fluent return value from add_operation
       - calls to_gcode(), NOT run()

Licence: MIT OR Apache-2.0 (same as SolidCraft).
"""

from __future__ import annotations

import json
import math
import sys
import unittest
from io import StringIO
from types import ModuleType
from unittest.mock import MagicMock, call, patch


# ── Mock-module scaffolding ───────────────────────────────────────────────────

def _make_freecad_mock() -> ModuleType:
    mod = ModuleType("FreeCAD")
    return mod


def _make_face_mock(cx: float, cy: float, cz: float, geom_type: str = "PLANE") -> MagicMock:
    """Create a mock cq.Face with a fixed Center() and geomType()."""
    face = MagicMock(name=f"face_{cx}_{cy}_{cz}")
    face.Center.return_value = MagicMock(x=cx, y=cy, z=cz)
    face.geomType.return_value = geom_type
    return face


def _make_cyl_face_mock(cx: float, cy: float, cz: float = 0.0) -> MagicMock:
    return _make_face_mock(cx, cy, cz, geom_type="CYLINDER")


def _make_model_mock(faces: list | None = None) -> MagicMock:
    """Return a mock CadQuery model whose .faces().vals() returns *faces*."""
    model = MagicMock(name="cq_model")
    if faces is not None:
        model.faces.return_value.vals.return_value = faces
    return model


def _make_cq_mock(faces: list | None = None) -> MagicMock:
    """Return a mock cadquery module with importers.importStep wired up."""
    cq = MagicMock(name="cadquery")
    model = _make_model_mock(faces)
    top_face = MagicMock(name="cq_top_face")
    model.faces.return_value = top_face
    cq.importers.importStep.return_value = model
    return cq


def _make_ocp_mock() -> MagicMock:
    """Return a mock ocp_freecad_cam module with Job and tool classes."""
    mod = MagicMock(name="ocp_freecad_cam")
    mod.__version__ = "2.0.2"
    # Job is fluent — each operation call on the instance returns a new mock
    job_instance = MagicMock(name="job_instance")
    mod.Job.return_value = job_instance
    mod.Endmill.return_value = MagicMock(name="endmill_obj")
    mod.Drill.return_value = MagicMock(name="drill_obj")
    mod.Ballnose.return_value = MagicMock(name="ballnose_obj")
    mod.Bullnose.return_value = MagicMock(name="bullnose_obj")
    return mod


def _tool_classes_from_mock(ocp_mock: MagicMock) -> dict:
    return {
        "Endmill":  ocp_mock.Endmill,
        "Drill":    ocp_mock.Drill,
        "Ballnose": ocp_mock.Ballnose,
        "Bullnose": ocp_mock.Bullnose,
    }


# ── Import the adapter (no FreeCAD/CQ needed at import time) ─────────────────

import os as _os
_adapter_dir = _os.path.dirname(_os.path.abspath(__file__))
if _adapter_dir not in sys.path:
    sys.path.insert(0, _adapter_dir)

import freecad_cam_adapter as adapter  # noqa: E402


# ── Helpers ───────────────────────────────────────────────────────────────────

def _run_main(stdin_json: dict, sys_modules_overrides: dict) -> dict:
    """Run adapter.main() with mocked sys.stdin and sys.modules."""
    stdin_text = json.dumps(stdin_json)
    captured = StringIO()
    with (
        patch.object(sys, "stdin", StringIO(stdin_text)),
        patch.object(sys, "stdout", captured),
        patch.dict(sys.modules, sys_modules_overrides, clear=False),
    ):
        adapter.main()
    output = captured.getvalue().strip()
    assert output, "adapter.main() wrote nothing to stdout"
    return json.loads(output)


def _full_sys_modules(
    freecad: object = None,
    cq: object = None,
    ocp: object = None,
) -> dict:
    return {
        "FreeCAD":        freecad if freecad is not None else _make_freecad_mock(),
        "cadquery":       cq      if cq      is not None else _make_cq_mock(),
        "ocp_freecad_cam": ocp    if ocp     is not None else _make_ocp_mock(),
    }


# ─────────────────────────────────────────────────────────────────────────────
# 1. Import failure paths
# ─────────────────────────────────────────────────────────────────────────────

class TestImportFailures(unittest.TestCase):

    def test_freecad_missing_returns_unavailable(self):
        result = _run_main(
            {"probe": True},
            {"FreeCAD": None, "cadquery": _make_cq_mock(), "ocp_freecad_cam": _make_ocp_mock()},
        )
        self.assertEqual(result["status"], "unavailable")
        self.assertIn("FreeCAD", result["message"])

    def test_cadquery_missing_returns_unavailable(self):
        result = _run_main(
            {"probe": True},
            {"FreeCAD": _make_freecad_mock(), "cadquery": None, "ocp_freecad_cam": _make_ocp_mock()},
        )
        self.assertEqual(result["status"], "unavailable")
        self.assertIn("cadquery", result["message"])

    def test_ocp_job_missing_returns_unavailable(self):
        bad_ocp = ModuleType("ocp_freecad_cam")  # no Job attribute
        result = _run_main(
            {"probe": True},
            {"FreeCAD": _make_freecad_mock(), "cadquery": _make_cq_mock(), "ocp_freecad_cam": bad_ocp},
        )
        self.assertEqual(result["status"], "unavailable")

    def test_ocp_tool_classes_missing_returns_unavailable(self):
        partial_ocp = ModuleType("ocp_freecad_cam")
        partial_ocp.Job = MagicMock()  # type: ignore[attr-defined]
        # no Endmill / Drill / Ballnose / Bullnose
        result = _run_main(
            {"probe": True},
            {"FreeCAD": _make_freecad_mock(), "cadquery": _make_cq_mock(), "ocp_freecad_cam": partial_ocp},
        )
        self.assertEqual(result["status"], "unavailable")
        self.assertIn("Endmill", result["message"])


# ─────────────────────────────────────────────────────────────────────────────
# 2. Probe response
# ─────────────────────────────────────────────────────────────────────────────

class TestProbe(unittest.TestCase):

    def test_probe_returns_ok_with_empty_segments(self):
        result = _run_main({"probe": True}, _full_sys_modules())
        self.assertEqual(result["status"], "ok")
        self.assertEqual(result["segments"], [])

    def test_probe_does_not_touch_step_path(self):
        result = _run_main(
            {"probe": True, "step_path": "/nonexistent/file.step"},
            _full_sys_modules(),
        )
        self.assertEqual(result["status"], "ok")


# ─────────────────────────────────────────────────────────────────────────────
# 3. step_path validation
# ─────────────────────────────────────────────────────────────────────────────

class TestStepPathValidation(unittest.TestCase):

    def test_empty_step_path_returns_error(self):
        result = _run_main({"step_path": "", "manufacture": {}}, _full_sys_modules())
        self.assertEqual(result["status"], "error")
        self.assertIn("step_path", result["message"])

    def test_relative_step_path_returns_error(self):
        result = _run_main(
            {"step_path": "relative/path.step", "manufacture": {}}, _full_sys_modules()
        )
        self.assertEqual(result["status"], "error")

    def test_nonexistent_step_path_returns_error(self):
        result = _run_main(
            {"step_path": "/absolutely/nonexistent/model.step", "manufacture": {}},
            _full_sys_modules(),
        )
        self.assertEqual(result["status"], "error")


# ─────────────────────────────────────────────────────────────────────────────
# 4. make_tool() — correct kwargs for ocp-freecad-cam 2.0.2
# ─────────────────────────────────────────────────────────────────────────────

class TestMakeTool(unittest.TestCase):

    def setUp(self):
        self.ocp = _make_ocp_mock()
        self.tc = _tool_classes_from_mock(self.ocp)
        self.feeds = {"feedrate": 1500.0, "plunge": 350.0, "spindle_rpm": 12000}

    def test_flat_endmill_uses_cutting_edge_height_not_flutes(self):
        adapter.make_tool(
            {"kind": "flat_endmill", "diameter": 6.0, "length": 50.0, "flute_length": 16.0},
            self.tc,
            self.feeds,
        )
        self.ocp.Endmill.assert_called_once_with(
            diameter=6.0, length=50.0, cutting_edge_height=16.0,
            h_feed=1500.0, v_feed=350.0, speed=12000.0,
        )

    def test_flat_endmill_passes_feeds(self):
        adapter.make_tool({"kind": "flat_endmill", "diameter": 8.0, "length": 60.0, "flute_length": 20.0},
                          self.tc, {"feedrate": 2000.0, "plunge": 500.0, "spindle_rpm": 8000})
        _, kwargs = self.ocp.Endmill.call_args
        self.assertEqual(kwargs["h_feed"], 2000.0)
        self.assertEqual(kwargs["v_feed"], 500.0)
        self.assertEqual(kwargs["speed"], 8000.0)

    def test_bullnose_endmill_uses_Bullnose_class(self):
        adapter.make_tool(
            {"kind": "bullnose_endmill", "diameter": 10.0, "length": 60.0, "flute_length": 18.0},
            self.tc,
            self.feeds,
        )
        self.ocp.Bullnose.assert_called_once_with(
            diameter=10.0, length=60.0, cutting_edge_height=18.0,
            h_feed=1500.0, v_feed=350.0, speed=12000.0,
        )

    def test_ball_endmill_creates_Ballnose(self):
        adapter.make_tool(
            {"kind": "ball_endmill", "diameter": 8.0, "length": 55.0, "flute_length": 20.0},
            self.tc,
            self.feeds,
        )
        self.ocp.Ballnose.assert_called_once_with(
            diameter=8.0, length=55.0, cutting_edge_height=20.0,
            h_feed=1500.0, v_feed=350.0, speed=12000.0,
        )

    def test_drill_omits_cutting_edge_height(self):
        adapter.make_tool(
            {"kind": "drill", "diameter": 3.0, "length": 40.0},
            self.tc,
            self.feeds,
        )
        self.ocp.Drill.assert_called_once_with(
            diameter=3.0, length=40.0,
            h_feed=1500.0, v_feed=350.0, speed=12000.0,
        )

    def test_unknown_kind_falls_back_to_Endmill(self):
        adapter.make_tool({"kind": "tap", "diameter": 5.0, "length": 30.0}, self.tc, self.feeds)
        self.ocp.Endmill.assert_called_once()

    def test_defaults_when_fields_missing(self):
        adapter.make_tool({}, self.tc, {})
        _, kwargs = self.ocp.Endmill.call_args
        self.assertEqual(kwargs["diameter"], 6.0)
        self.assertEqual(kwargs["length"], 50.0)
        self.assertEqual(kwargs["cutting_edge_height"], 16.0)
        self.assertEqual(kwargs["h_feed"], 1000.0)
        self.assertEqual(kwargs["v_feed"], 300.0)
        self.assertEqual(kwargs["speed"], 10000.0)


# ─────────────────────────────────────────────────────────────────────────────
# 5. resolve_faces()
# ─────────────────────────────────────────────────────────────────────────────

class TestResolveFaces(unittest.TestCase):

    def test_returns_nearest_face(self):
        f1 = _make_face_mock(0.0, 0.0, 10.0)
        f2 = _make_face_mock(50.0, 50.0, 10.0)
        model = _make_model_mock([f1, f2])
        result = adapter.resolve_faces(model, [[1.0, 0.0, 10.0]], "op")
        self.assertIs(result[0], f1)

    def test_multiple_points_return_multiple_faces(self):
        f1 = _make_face_mock(0.0, 0.0, 0.0)
        f2 = _make_face_mock(100.0, 0.0, 0.0)
        model = _make_model_mock([f1, f2])
        result = adapter.resolve_faces(model, [[0.5, 0.0, 0.0], [99.5, 0.0, 0.0]], "op")
        self.assertIs(result[0], f1)
        self.assertIs(result[1], f2)

    def test_duplicate_sample_points_deduplicated(self):
        f1 = _make_face_mock(0.0, 0.0, 0.0)
        model = _make_model_mock([f1])
        result = adapter.resolve_faces(model, [[0.0, 0.0, 0.0], [0.1, 0.0, 0.0]], "op")
        self.assertEqual(len(result), 1)
        self.assertIs(result[0], f1)

    def test_point_beyond_tolerance_raises_preflight(self):
        f1 = _make_face_mock(0.0, 0.0, 0.0)
        model = _make_model_mock([f1])
        far = adapter.FACE_SELECT_TOLERANCE_MM + 1.0
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.resolve_faces(model, [[far, far, far]], "op_x")
        self.assertIn("op_x", str(ctx.exception))
        self.assertIn("mm away", str(ctx.exception))

    def test_ambiguous_tie_raises_preflight(self):
        # Two faces within TIE_TOLERANCE of a sample point
        tie_gap = adapter.FACE_SELECT_TIE_TOLERANCE_MM * 0.1  # well within tie window
        f1 = _make_face_mock(0.0, 0.0, 0.0)
        f2 = _make_face_mock(tie_gap, 0.0, 0.0)
        model = _make_model_mock([f1, f2])
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.resolve_faces(model, [[tie_gap / 2, 0.0, 0.0]], "op_y")
        self.assertIn("ambiguous", str(ctx.exception))

    def test_no_faces_raises_preflight(self):
        model = _make_model_mock([])
        with self.assertRaises(adapter.PreflightError):
            adapter.resolve_faces(model, [[0.0, 0.0, 0.0]], "op")


# ─────────────────────────────────────────────────────────────────────────────
# 6. resolve_drill_targets()
# ─────────────────────────────────────────────────────────────────────────────

class TestResolveDrillTargets(unittest.TestCase):

    def test_empty_targets_raises_preflight(self):
        model = _make_model_mock([])
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.resolve_drill_targets(model, {}, "drill_op")
        self.assertIn("drill_op", str(ctx.exception))
        self.assertIn("drill_points", str(ctx.exception))

    def test_drill_points_selects_cylindrical_face(self):
        plane_face = _make_face_mock(0.0, 0.0, 0.0, "PLANE")
        cyl_face = _make_cyl_face_mock(10.0, 10.0, 0.0)
        model = _make_model_mock([plane_face, cyl_face])
        result = adapter.resolve_drill_targets(
            model, {"drill_points": [[10.0, 10.0, 0.0]]}, "drill_op"
        )
        # cyl_face is nearest cylindrical face
        self.assertIs(result[0], cyl_face)

    def test_drill_points_falls_back_to_any_face_when_no_cylinders(self):
        f1 = _make_face_mock(5.0, 5.0, 0.0, "PLANE")
        f2 = _make_face_mock(50.0, 50.0, 0.0, "PLANE")
        model = _make_model_mock([f1, f2])
        result = adapter.resolve_drill_targets(
            model, {"drill_points": [[5.0, 5.0, 0.0]]}, "drill_op"
        )
        self.assertIs(result[0], f1)

    def test_drill_points_beyond_tolerance_raises_preflight(self):
        cyl_face = _make_cyl_face_mock(0.0, 0.0, 0.0)
        model = _make_model_mock([cyl_face])
        far = adapter.FACE_SELECT_TOLERANCE_MM + 1.0
        with self.assertRaises(adapter.PreflightError):
            adapter.resolve_drill_targets(
                model, {"drill_points": [[far, far, 0.0]]}, "drill_op"
            )

    def test_face_sample_points_used_when_no_drill_points(self):
        f1 = _make_face_mock(0.0, 0.0, 0.0)
        model = _make_model_mock([f1])
        result = adapter.resolve_drill_targets(
            model, {"face_sample_points": [[0.0, 0.0, 0.0]]}, "drill_op"
        )
        self.assertIs(result[0], f1)

    def test_drill_points_take_precedence_over_face_sample_points(self):
        cyl_face = _make_cyl_face_mock(10.0, 10.0)
        plane_face = _make_face_mock(20.0, 20.0, 0.0)
        model = _make_model_mock([cyl_face, plane_face])
        result = adapter.resolve_drill_targets(
            model,
            {"drill_points": [[10.0, 10.0, 0.0]], "face_sample_points": [[20.0, 20.0, 0.0]]},
            "drill_op",
        )
        # drill_points wins → cyl_face selected
        self.assertIs(result[0], cyl_face)


# ─────────────────────────────────────────────────────────────────────────────
# 7. add_operation() — face op (no targets needed)
# ─────────────────────────────────────────────────────────────────────────────

class TestFaceOperation(unittest.TestCase):

    def setUp(self):
        self.job_in = MagicMock(name="job_in")
        self.job_out = MagicMock(name="job_out")
        self.job_in.face.return_value = self.job_out
        self.model = _make_model_mock()
        self.top_face = MagicMock(name="top_face")
        self.tool_obj = MagicMock(name="tool_obj")

    def test_face_returns_fluent_job(self):
        op = {"type": "face", "depth": 0.5, "feeds": {}}
        result = adapter.add_operation(
            self.job_in, self.model, self.top_face, op, self.tool_obj, "S1", 0
        )
        self.assertIs(result, self.job_out)

    def test_face_calls_job_face_with_step_down(self):
        op = {"type": "face", "depth": 0.75, "feeds": {}}
        adapter.add_operation(self.job_in, self.model, self.top_face, op, self.tool_obj, "S1", 0)
        self.job_in.face.assert_called_once_with(self.top_face, self.tool_obj, step_down=0.75)

    def test_face_uses_default_depth(self):
        op = {"type": "face", "feeds": {}}
        adapter.add_operation(self.job_in, self.model, self.top_face, op, self.tool_obj, "S1", 0)
        _, kwargs = self.job_in.face.call_args
        self.assertEqual(kwargs["step_down"], 0.5)

    def test_face_does_not_pass_feedrate_or_stepover(self):
        """Feed rates travel with the tool, not the operation call."""
        op = {"type": "face", "depth": 0.5, "stepover": 0.75,
              "feeds": {"feedrate": 2000.0, "plunge": 400.0, "spindle_rpm": 12000}}
        adapter.add_operation(self.job_in, self.model, self.top_face, op, self.tool_obj, "S1", 0)
        _, kwargs = self.job_in.face.call_args
        self.assertNotIn("feedrate", kwargs)
        self.assertNotIn("stepover", kwargs)
        self.assertNotIn("depth", kwargs)  # old wrong kwarg name
        self.assertNotIn("spindle_rpm", kwargs)


# ─────────────────────────────────────────────────────────────────────────────
# 8. add_operation() — profile
# ─────────────────────────────────────────────────────────────────────────────

class TestProfileOperation(unittest.TestCase):

    def _setup(self, face_center=(10.0, 0.0, 0.0)):
        face = _make_face_mock(*face_center)
        model = _make_model_mock([face])
        job_in = MagicMock(name="job_in")
        job_out = MagicMock(name="job_out")
        job_in.profile.return_value = job_out
        return face, model, job_in, job_out

    def test_profile_calls_job_profile_with_resolved_face(self):
        face, model, job_in, job_out = self._setup((10.0, 0.0, 0.0))
        op = {
            "type": "profile",
            "depth": 3.0,
            "side": "outside",
            "feeds": {},
            "targets": {"face_sample_points": [[10.0, 0.0, 0.0]]},
        }
        result = adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        self.assertIs(result, job_out)
        job_in.profile.assert_called_once_with([face], job_in.profile.call_args[0][1],
                                               side="out", step_down=3.0)

    def test_profile_maps_inside_side(self):
        face, model, job_in, _ = self._setup()
        op = {"type": "profile", "depth": 2.0, "side": "inside",
              "feeds": {}, "targets": {"face_sample_points": [[10.0, 0.0, 0.0]]}}
        adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        _, kwargs = job_in.profile.call_args
        self.assertEqual(kwargs["side"], "in")

    def test_profile_maps_on_side(self):
        face, model, job_in, _ = self._setup()
        op = {"type": "profile", "depth": 2.0, "side": "on",
              "feeds": {}, "targets": {"face_sample_points": [[10.0, 0.0, 0.0]]}}
        adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        _, kwargs = job_in.profile.call_args
        self.assertEqual(kwargs["side"], "mid")

    def test_profile_no_targets_raises_preflight(self):
        model = _make_model_mock([])
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.add_operation(
                MagicMock(), model, MagicMock(),
                {"type": "profile", "feeds": {}}, MagicMock(), "S1", 0
            )
        self.assertIn("face_sample_points", str(ctx.exception))


# ─────────────────────────────────────────────────────────────────────────────
# 9. add_operation() — pocket
# ─────────────────────────────────────────────────────────────────────────────

class TestPocketOperation(unittest.TestCase):

    def test_pocket_calls_job_pocket_with_step_over_percent(self):
        face = _make_face_mock(0.0, 0.0, 0.0)
        model = _make_model_mock([face])
        job_in = MagicMock()
        job_out = MagicMock()
        job_in.pocket.return_value = job_out
        op = {
            "type": "pocket",
            "depth": 4.0,
            "stepover": 0.45,
            "feeds": {},
            "targets": {"face_sample_points": [[0.0, 0.0, 0.0]]},
        }
        result = adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        self.assertIs(result, job_out)
        _, kwargs = job_in.pocket.call_args
        self.assertAlmostEqual(kwargs["step_over"], 45.0)  # fraction → percent
        self.assertEqual(kwargs["step_down"], 4.0)

    def test_pocket_no_targets_raises_preflight(self):
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.add_operation(
                MagicMock(), _make_model_mock([]), MagicMock(),
                {"type": "pocket", "feeds": {}}, MagicMock(), "S1", 0
            )
        self.assertIn("face_sample_points", str(ctx.exception))


# ─────────────────────────────────────────────────────────────────────────────
# 10. add_operation() — drill
# ─────────────────────────────────────────────────────────────────────────────

class TestDrillOperation(unittest.TestCase):

    def _drill_model(self):
        cyl = _make_cyl_face_mock(10.0, 10.0)
        return cyl, _make_model_mock([cyl])

    def test_drill_simple_cycle_no_peck_no_dwell(self):
        cyl, model = self._drill_model()
        job_in = MagicMock()
        job_out = MagicMock()
        job_in.drill.return_value = job_out
        op = {
            "type": "drill", "cycle": "simple", "depth": 10.0, "peck_depth": 2.0, "dwell_s": 0.0,
            "feeds": {}, "targets": {"drill_points": [[10.0, 10.0, 0.0]]},
        }
        result = adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        self.assertIs(result, job_out)
        _, kwargs = job_in.drill.call_args
        self.assertIsNone(kwargs["peck_depth"])
        self.assertFalse(kwargs["chip_break_enabled"])
        self.assertIsNone(kwargs["dwell_time"])

    def test_drill_peck_cycle(self):
        cyl, model = self._drill_model()
        job_in = MagicMock()
        job_in.drill.return_value = MagicMock()
        op = {
            "type": "drill", "cycle": "peck", "depth": 10.0, "peck_depth": 2.5, "dwell_s": 0.0,
            "feeds": {}, "targets": {"drill_points": [[10.0, 10.0, 0.0]]},
        }
        adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        _, kwargs = job_in.drill.call_args
        self.assertAlmostEqual(kwargs["peck_depth"], 2.5)
        self.assertFalse(kwargs["chip_break_enabled"])

    def test_drill_chip_break_cycle(self):
        cyl, model = self._drill_model()
        job_in = MagicMock()
        job_in.drill.return_value = MagicMock()
        op = {
            "type": "drill", "cycle": "chip_break", "depth": 10.0, "peck_depth": 1.5, "dwell_s": 0.0,
            "feeds": {}, "targets": {"drill_points": [[10.0, 10.0, 0.0]]},
        }
        adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        _, kwargs = job_in.drill.call_args
        self.assertAlmostEqual(kwargs["peck_depth"], 1.5)
        self.assertTrue(kwargs["chip_break_enabled"])

    def test_drill_dwell_cycle(self):
        cyl, model = self._drill_model()
        job_in = MagicMock()
        job_in.drill.return_value = MagicMock()
        op = {
            "type": "drill", "cycle": "dwell", "depth": 10.0, "peck_depth": 2.0, "dwell_s": 0.5,
            "feeds": {}, "targets": {"drill_points": [[10.0, 10.0, 0.0]]},
        }
        adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        _, kwargs = job_in.drill.call_args
        self.assertAlmostEqual(kwargs["dwell_time"], 0.5)

    def test_drill_no_targets_raises_preflight(self):
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.add_operation(
                MagicMock(), _make_model_mock([]), MagicMock(),
                {"type": "drill", "feeds": {}}, MagicMock(), "S1", 0
            )
        self.assertIn("drill_points", str(ctx.exception))


# ─────────────────────────────────────────────────────────────────────────────
# 11. add_operation() — adaptive
# ─────────────────────────────────────────────────────────────────────────────

class TestAdaptiveOperation(unittest.TestCase):

    def test_adaptive_calls_job_adaptive_with_step_over_percent(self):
        face = _make_face_mock(5.0, 5.0, 0.0)
        model = _make_model_mock([face])
        job_in = MagicMock()
        job_out = MagicMock()
        job_in.adaptive.return_value = job_out
        op = {
            "type": "adaptive",
            "depth": 4.0,
            "optimal_load": 0.35,
            "finish_allowance": 0.1,
            "feeds": {},
            "targets": {"face_sample_points": [[5.0, 5.0, 0.0]]},
        }
        result = adapter.add_operation(job_in, model, MagicMock(), op, MagicMock(), "S1", 0)
        self.assertIs(result, job_out)
        _, kwargs = job_in.adaptive.call_args
        self.assertAlmostEqual(kwargs["step_over"], 35.0)  # fraction → percent
        self.assertAlmostEqual(kwargs["stock_to_leave"], 0.1)
        self.assertEqual(kwargs["step_down"], 4.0)

    def test_adaptive_no_targets_raises_preflight(self):
        with self.assertRaises(adapter.PreflightError) as ctx:
            adapter.add_operation(
                MagicMock(), _make_model_mock([]), MagicMock(),
                {"type": "adaptive", "feeds": {}}, MagicMock(), "S1", 0
            )
        self.assertIn("face_sample_points", str(ctx.exception))


# ─────────────────────────────────────────────────────────────────────────────
# 12. add_operation() unknown type → ValueError (never silent skip)
# ─────────────────────────────────────────────────────────────────────────────

class TestUnknownOpType(unittest.TestCase):

    def test_unknown_type_raises_value_error(self):
        job = MagicMock()
        with self.assertRaises(ValueError) as ctx:
            adapter.add_operation(
                job, _make_model_mock([]), MagicMock(),
                {"type": "laser_cut", "feeds": {}}, MagicMock(), "S", 3
            )
        self.assertIn("laser_cut", str(ctx.exception))
        # Must NOT have called any operation method on the job
        job.face.assert_not_called()
        job.profile.assert_not_called()

    def test_empty_type_raises_value_error(self):
        with self.assertRaises(ValueError):
            adapter.add_operation(
                MagicMock(), _make_model_mock([]), MagicMock(),
                {"type": "", "feeds": {}}, MagicMock(), "S", 0
            )


# ─────────────────────────────────────────────────────────────────────────────
# 13. extract_segments() — G0/G1/G2/G3 via new API path
# ─────────────────────────────────────────────────────────────────────────────

def _make_cmd(name: str, **params: float) -> MagicMock:
    cmd = MagicMock()
    cmd.Name = name
    cmd.Parameters = params
    return cmd


class TestExtractSegments(unittest.TestCase):

    def _job_with(self, commands: list) -> MagicMock:
        """Create a mock job whose job_impl.fc_job.Path.Commands is *commands*."""
        job = MagicMock()
        job.job_impl.fc_job.Path.Commands = commands
        return job

    def test_g0_produces_rapid(self):
        job = self._job_with([_make_cmd("G0", X=10.0, Y=20.0, Z=5.0)])
        segs = adapter.extract_segments(job)
        self.assertEqual(len(segs), 1)
        self.assertEqual(segs[0], {"kind": "rapid", "to": [10.0, 20.0, 5.0]})

    def test_g00_alias_accepted(self):
        job = self._job_with([_make_cmd("G00", X=1.0, Y=2.0, Z=3.0)])
        segs = adapter.extract_segments(job)
        self.assertEqual(segs[0]["kind"], "rapid")

    def test_g1_produces_linear_with_feed(self):
        job = self._job_with([_make_cmd("G1", X=5.0, Y=5.0, Z=0.0, F=1000.0)])
        segs = adapter.extract_segments(job)
        self.assertEqual(segs[0], {"kind": "linear", "to": [5.0, 5.0, 0.0], "feed": 1000.0})

    def test_g1_with_zero_feed_uses_fallback(self):
        job = self._job_with([_make_cmd("G1", X=5.0, Y=5.0, Z=0.0, F=0.0)])
        segs = adapter.extract_segments(job)
        self.assertGreater(segs[0]["feed"], 0.0)

    def test_g2_produces_clockwise_arc(self):
        job = self._job_with([_make_cmd("G2", X=10.0, Y=0.0, Z=0.0, F=500.0, I=5.0, J=0.0, K=0.0)])
        segs = adapter.extract_segments(job)
        self.assertEqual(segs[0]["kind"], "arc")
        self.assertTrue(segs[0]["clockwise"])

    def test_g3_produces_counter_clockwise_arc(self):
        job = self._job_with([_make_cmd("G3", X=10.0, Y=0.0, Z=0.0, F=500.0, I=5.0, J=0.0, K=0.0)])
        segs = adapter.extract_segments(job)
        self.assertFalse(segs[0]["clockwise"])

    def test_arc_centre_uses_current_position_not_destination(self):
        """
        Arc centre = current_pos + (I, J, K), NOT destination + (I, J, K).

        Sequence: G0 to (10,0,0) → current=(10,0,0)
                  G2 to (10,10,0) with I=0, J=5 → centre = (10+0, 0+5, 0+0) = (10,5,0)
        """
        commands = [
            _make_cmd("G0", X=10.0, Y=0.0, Z=0.0),
            _make_cmd("G2", X=10.0, Y=10.0, Z=0.0, F=500.0, I=0.0, J=5.0, K=0.0),
        ]
        segs = adapter.extract_segments(self._job_with(commands))
        arc = segs[1]
        self.assertEqual(arc["kind"], "arc")
        self.assertAlmostEqual(arc["center"][0], 10.0)   # current.x + I = 10 + 0
        self.assertAlmostEqual(arc["center"][1], 5.0)    # current.y + J = 0 + 5
        self.assertAlmostEqual(arc["center"][2], 0.0)    # current.z + K = 0 + 0
        self.assertNotAlmostEqual(arc["center"][1], 15.0)  # NOT destination + J

    def test_position_tracked_across_moves(self):
        commands = [
            _make_cmd("G0", X=5.0, Y=5.0, Z=1.0),
            _make_cmd("G1", X=10.0, Y=5.0, Z=1.0, F=500.0),
            _make_cmd("G2", X=10.0, Y=10.0, Z=1.0, F=300.0, I=0.0, J=2.5, K=0.0),
        ]
        segs = adapter.extract_segments(self._job_with(commands))
        arc = segs[2]
        # After G1 to (10,5,1), arc centre = (10+0, 5+2.5, 1+0) = (10, 7.5, 1)
        self.assertAlmostEqual(arc["center"][0], 10.0)
        self.assertAlmostEqual(arc["center"][1], 7.5)
        self.assertAlmostEqual(arc["center"][2], 1.0)

    def test_non_motion_codes_ignored(self):
        """M-codes, G4, T-codes etc. do not produce segments."""
        job = self._job_with([
            _make_cmd("M3", S=10000),
            _make_cmd("G4", P=0.5),
            _make_cmd("M5"),
            _make_cmd("G0", X=0.0, Y=0.0, Z=5.0),
        ])
        segs = adapter.extract_segments(job)
        self.assertEqual(len(segs), 1)
        self.assertEqual(segs[0]["kind"], "rapid")

    def test_missing_fc_job_returns_empty(self):
        """When job.job_impl.fc_job is None the result is empty."""
        job = MagicMock()
        job.job_impl.fc_job = None
        segs = adapter.extract_segments(job)
        self.assertEqual(segs, [])

    def test_empty_commands_returns_empty(self):
        self.assertEqual(adapter.extract_segments(self._job_with([])), [])


# ─────────────────────────────────────────────────────────────────────────────
# 14. process() wires Job() and fluent chain correctly
# ─────────────────────────────────────────────────────────────────────────────

class TestProcess(unittest.TestCase):

    def _make_step_file(self) -> str:
        import tempfile
        f = tempfile.NamedTemporaryFile(suffix=".step", delete=False, mode="w")
        f.write("ISO-10303-21;\n")
        f.close()
        return f.name

    def setUp(self):
        self.step = self._make_step_file()
        self.ocp = _make_ocp_mock()
        self.tc = _tool_classes_from_mock(self.ocp)
        self.cq = _make_cq_mock()
        self.Job = self.ocp.Job

    def tearDown(self):
        import os
        os.unlink(self.step)

    def test_job_constructor_receives_top_face_model_and_post(self):
        manufacture = {
            "setups": {
                "S1": {
                    "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0,
                               "length": 50.0, "flute_length": 16.0}],
                    "operations": [{"type": "face", "depth": 0.5, "tool_number": 1, "feeds": {}}],
                }
            }
        }
        adapter.process(self.step, manufacture, self.cq, self.Job, self.tc)

        model = self.cq.importers.importStep.return_value
        top_face = model.faces.return_value

        self.Job.assert_called_once()
        args, kwargs = self.Job.call_args
        self.assertIs(args[0], top_face)
        self.assertIs(args[1], model)
        self.assertIn("post_processor", kwargs)

    def test_fluent_job_returned_from_add_operation_is_used(self):
        """process() must capture the return value of each add_operation call."""
        manufacture = {
            "setups": {
                "S1": {
                    "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0,
                               "length": 50.0, "flute_length": 16.0}],
                    "operations": [{"type": "face", "tool_number": 1, "feeds": {}}],
                }
            }
        }
        adapter.process(self.step, manufacture, self.cq, self.Job, self.tc)
        job_instance = self.Job.return_value
        # job.face() must have been called on job_instance (not on a stale reference)
        job_instance.face.assert_called_once()

    def test_to_gcode_is_called_not_run(self):
        """to_gcode() triggers the build; there is no .run() method in the real API."""
        manufacture = {
            "setups": {
                "S1": {
                    "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0,
                               "length": 50.0, "flute_length": 16.0}],
                    "operations": [{"type": "face", "tool_number": 1, "feeds": {}}],
                }
            }
        }
        adapter.process(self.step, manufacture, self.cq, self.Job, self.tc)
        job_instance = self.Job.return_value
        # to_gcode must be called on the result of job.face()
        job_after_face = job_instance.face.return_value
        job_after_face.to_gcode.assert_called_once()
        # run() must NOT have been called
        job_instance.run.assert_not_called()

    def test_empty_setups_returns_empty_segments(self):
        segments, post = adapter.process(self.step, {"setups": {}}, self.cq, self.Job, self.tc)
        self.assertEqual(segments, [])
        self.assertIsNone(post)

    def test_preflight_error_propagates_from_process(self):
        manufacture = {
            "setups": {
                "S1": {
                    "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0,
                               "length": 50.0, "flute_length": 16.0}],
                    "operations": [{"type": "pocket", "tool_number": 1, "feeds": {}}],
                }
            }
        }
        with self.assertRaises(adapter.PreflightError):
            adapter.process(self.step, manufacture, self.cq, self.Job, self.tc)

    def test_value_error_for_unknown_op_propagates(self):
        manufacture = {
            "setups": {
                "S1": {
                    "tools": [{"number": 1, "kind": "flat_endmill", "diameter": 6.0,
                               "length": 50.0, "flute_length": 16.0}],
                    "operations": [{"type": "engrave", "tool_number": 1, "feeds": {}}],
                }
            }
        }
        with self.assertRaises(ValueError):
            adapter.process(self.step, manufacture, self.cq, self.Job, self.tc)


if __name__ == "__main__":
    unittest.main(verbosity=2)
