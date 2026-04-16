"""FreeCAD integration tests for dcc-mcp-core.

Tests the dcc-mcp-core skill/action pipeline against FreeCAD (headless --console mode).
All tests are conditionally skipped if FreeCAD is not installed.

Run:  pytest -m dcc -k "FreeCAD"
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import textwrap
from typing import Any

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── Binary detection ──

FREECAD_BIN = shutil.which("FreeCAD") or shutil.which("freecad") or shutil.which("freecadcmd")

freecad_available = pytest.mark.skipif(FREECAD_BIN is None, reason="FreeCAD not found in PATH")


# ── Script runner helpers ──


def _parse_json_from_output(stdout: str, stderr: str = "") -> dict[str, Any]:
    """Extract the last JSON object from multiline stdout."""
    for line in reversed(stdout.splitlines()):
        line = line.strip()
        if line.startswith("{"):
            return json.loads(line)
    raise AssertionError(f"No JSON in output.\nstdout:\n{stdout}\nstderr:\n{stderr}")


def _run_subprocess(cmd: list[str], timeout: int = 60) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, timeout=timeout, encoding="utf-8")


def _run_freecad_script(script: str, timeout: int = 60) -> dict[str, Any]:
    """Run Python inside FreeCAD --console mode."""
    assert FREECAD_BIN is not None
    with tempfile.NamedTemporaryFile(suffix=".py", mode="w", delete=False, encoding="utf-8") as f:
        f.write(script)
        script_path = f.name
    try:
        r = _run_subprocess([FREECAD_BIN, "--console", script_path], timeout)
    finally:
        Path(script_path).unlink(missing_ok=True)
    return _parse_json_from_output(r.stdout, r.stderr)


# ═══════════════════════════════════════════════════════════════════
# FREECAD  (headless --console)
# ═══════════════════════════════════════════════════════════════════


@freecad_available
class TestFreeCADIntegration:
    """Tests using FreeCAD's --console (headless) Python scripting."""

    def test_freecad_version(self) -> None:
        assert FREECAD_BIN is not None
        r = _run_subprocess([FREECAD_BIN, "--version"])
        assert r.returncode == 0
        assert "FreeCAD" in (r.stdout + r.stderr)

    def test_freecad_create_document(self) -> None:
        script = textwrap.dedent("""\
            import FreeCAD, json
            doc = FreeCAD.newDocument("TestDoc")
            print(json.dumps({"success": True, "doc_name": doc.Name}))
            FreeCAD.closeDocument(doc.Name)
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert out["doc_name"] == "TestDoc"

    def test_freecad_create_box(self) -> None:
        script = textwrap.dedent("""\
            import FreeCAD, Part, json
            doc = FreeCAD.newDocument("BoxTest")
            box = doc.addObject("Part::Box", "MyBox")
            box.Length = 10.0
            box.Width = 5.0
            box.Height = 3.0
            doc.recompute()
            print(json.dumps({
                "success": True,
                "volume": box.Shape.Volume,
                "length": float(box.Length),
                "width": float(box.Width),
                "height": float(box.Height),
            }))
            FreeCAD.closeDocument(doc.Name)
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert abs(out["volume"] - 150.0) < 1e-6  # 10*5*3
        assert out["length"] == 10.0

    def test_freecad_create_cylinder(self) -> None:
        script = textwrap.dedent("""\
            import FreeCAD, Part, json, math
            doc = FreeCAD.newDocument("CylTest")
            cyl = doc.addObject("Part::Cylinder", "MyCyl")
            cyl.Radius = 5.0
            cyl.Height = 10.0
            doc.recompute()
            expected_vol = math.pi * 5.0**2 * 10.0
            print(json.dumps({
                "success": True,
                "volume": cyl.Shape.Volume,
                "expected": expected_vol,
                "close_enough": abs(cyl.Shape.Volume - expected_vol) < 0.01,
            }))
            FreeCAD.closeDocument(doc.Name)
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert out["close_enough"] is True

    def test_freecad_boolean_fuse(self) -> None:
        script = textwrap.dedent("""\
            import FreeCAD, Part, json
            doc = FreeCAD.newDocument("FuseTest")
            box = doc.addObject("Part::Box", "Box")
            box.Length = box.Width = box.Height = 10.0
            cyl = doc.addObject("Part::Cylinder", "Cyl")
            cyl.Radius = 3.0
            cyl.Height = 15.0
            cyl.Placement.Base = FreeCAD.Vector(5, 5, -2)
            fuse = doc.addObject("Part::MultiFuse", "Fuse")
            fuse.Shapes = [box, cyl]
            doc.recompute()
            print(json.dumps({
                "success": True,
                "fused_volume": fuse.Shape.Volume,
                "face_count": len(fuse.Shape.Faces),
            }))
            FreeCAD.closeDocument(doc.Name)
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert out["fused_volume"] > 0
        assert out["face_count"] > 0

    def test_freecad_export_step(self, tmp_path: Path) -> None:
        step_file = tmp_path / "box.step"
        script = textwrap.dedent(f"""\
            import FreeCAD, Part, json
            doc = FreeCAD.newDocument("ExportTest")
            box = doc.addObject("Part::Box", "Box")
            box.Length = box.Width = box.Height = 10.0
            doc.recompute()
            Part.export([box], r"{step_file}")
            import os
            exists = os.path.isfile(r"{step_file}")
            size = os.path.getsize(r"{step_file}") if exists else 0
            print(json.dumps({{"success": exists, "file_size": size}}))
            FreeCAD.closeDocument(doc.Name)
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert out["file_size"] > 0

    def test_freecad_measure_distance(self) -> None:
        script = textwrap.dedent("""\
            import FreeCAD, json
            p1 = FreeCAD.Vector(0, 0, 0)
            p2 = FreeCAD.Vector(3, 4, 0)
            dist = p1.distanceToPoint(p2)
            print(json.dumps({"success": True, "distance": dist}))
        """)
        out = _run_freecad_script(script)
        assert out["success"] is True
        assert abs(out["distance"] - 5.0) < 1e-6  # 3-4-5 right triangle


@freecad_available
class TestFreeCADSkillPipeline:
    """dcc-mcp-core skill pipeline with FreeCAD as target DCC."""

    def test_register_freecad_actions(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register(name="create_box", description="Create parametric box", dcc="freecad")
        reg.register(name="create_cylinder", description="Create parametric cylinder", dcc="freecad")
        reg.register(name="boolean_fuse", description="Boolean union of two shapes", dcc="freecad")
        reg.register(name="export_step", description="Export shape to STEP format", dcc="freecad")
        freecad_actions = reg.list_actions(dcc_name="freecad")
        assert len(freecad_actions) == 4
        names = {a["name"] for a in freecad_actions}
        assert "create_box" in names
        assert "export_step" in names

    def test_freecad_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "freecad-parametric",
            frontmatter="name: freecad-parametric\ndcc: freecad\ntags:\n  - cad\n  - parametric\nversion: 1.0.0",
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "freecad-parametric"))
        assert meta is not None
        assert meta.dcc == "freecad"
        assert "parametric" in meta.tags

    def test_freecad_action_result(self) -> None:
        result = dcc_mcp_core.success_result(
            "Created Box",
            object_name="MyBox",
            volume=150.0,
            dimensions={"length": 10, "width": 5, "height": 3},
            dcc="freecad",
        )
        assert result.success is True
        assert result.context["volume"] == 150.0
        assert result.context["dimensions"]["length"] == 10

    def test_freecad_event_pipeline(self) -> None:
        bus = dcc_mcp_core.EventBus()
        log: list[dict] = []
        bus.subscribe("freecad.shape.created", lambda **kw: log.append(kw))
        bus.subscribe("freecad.document.saved", lambda **kw: log.append(kw))
        bus.subscribe("freecad.export.completed", lambda **kw: log.append(kw))
        bus.publish("freecad.shape.created", shape="Box", volume=150.0)
        bus.publish("freecad.document.saved", path="/tmp/test.FCStd")
        bus.publish("freecad.export.completed", format="STEP", path="/tmp/box.step")
        assert len(log) == 3
        assert log[0]["shape"] == "Box"
        assert log[2]["format"] == "STEP"
