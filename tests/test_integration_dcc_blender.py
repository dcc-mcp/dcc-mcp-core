"""Blender integration tests for dcc-mcp-core.

Tests the dcc-mcp-core skill/action pipeline against Blender (headless / --background mode).
All tests are conditionally skipped if Blender is not installed.

Run:  pytest -m dcc -k "Blender"
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
from pathlib import Path
import shutil
import subprocess
import textwrap
from typing import Any

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── Binary detection ──

BLENDER_BIN = shutil.which("blender")

blender_available = pytest.mark.skipif(BLENDER_BIN is None, reason="Blender not found in PATH")


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


def _run_blender_script(script: str, timeout: int = 60) -> dict[str, Any]:
    assert BLENDER_BIN is not None
    r = _run_subprocess([BLENDER_BIN, "--background", "--python-expr", script], timeout)
    return _parse_json_from_output(r.stdout, r.stderr)


# ═══════════════════════════════════════════════════════════════════
# BLENDER  (headless --background)
# ═══════════════════════════════════════════════════════════════════


@blender_available
class TestBlenderIntegration:
    """Tests that exercise dcc-mcp-core concepts inside Blender."""

    def test_blender_version(self) -> None:
        assert BLENDER_BIN is not None
        r = _run_subprocess([BLENDER_BIN, "--version"])
        assert r.returncode == 0
        assert "Blender" in r.stdout

    def test_blender_list_scene_objects(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            objects = [obj.name for obj in bpy.data.objects]
            print(json.dumps({"success": True, "objects": objects}))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert "Camera" in out["objects"]
        assert "Cube" in out["objects"]

    def test_blender_create_sphere(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            bpy.ops.mesh.primitive_uv_sphere_add(radius=2.0, location=(0, 0, 0))
            sphere = bpy.context.active_object
            print(json.dumps({
                "success": True,
                "name": sphere.name,
                "type": sphere.type,
                "vertex_count": len(sphere.data.vertices),
            }))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["type"] == "MESH"
        assert out["vertex_count"] > 0

    def test_blender_batch_rename(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            for i in range(3):
                bpy.ops.mesh.primitive_cube_add(location=(i * 3, 0, 0))
                bpy.context.active_object.name = f"cube_{i}"
            renamed = []
            for obj in bpy.data.objects:
                if obj.name.startswith("cube_"):
                    obj.name = "TEST_" + obj.name
                    renamed.append(obj.name)
            print(json.dumps({"success": True, "renamed": sorted(renamed)}))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert len(out["renamed"]) == 3
        assert all(n.startswith("TEST_") for n in out["renamed"])

    def test_blender_scene_stats(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            print(json.dumps({"success": True, "context": {
                "object_count": len(bpy.data.objects),
                "mesh_count": len(bpy.data.meshes),
                "scene_name": bpy.context.scene.name,
            }}))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["context"]["object_count"] >= 3
        assert out["context"]["scene_name"] == "Scene"

    def test_blender_export_obj(self, tmp_path: Path) -> None:
        obj_file = tmp_path / "test_export.obj"
        script = textwrap.dedent(f"""\
            import bpy, os, json
            # Blender 3.0+ uses wm.obj_export; 2.x uses export_scene.obj
            if bpy.app.version >= (3, 0, 0):
                bpy.ops.wm.obj_export(filepath=r"{obj_file}")
            else:
                bpy.ops.export_scene.obj(filepath=r"{obj_file}")
            exists = os.path.isfile(r"{obj_file}")
            size = os.path.getsize(r"{obj_file}") if exists else 0
            print(json.dumps({{"success": exists, "file_size": size}}))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["file_size"] > 0

    def test_blender_material_create(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            mat = bpy.data.materials.new(name="TestMaterial")
            mat.diffuse_color = (1.0, 0.0, 0.0, 1.0)
            cube = bpy.data.objects.get("Cube")
            if cube and cube.data:
                cube.data.materials.append(mat)
            print(json.dumps({
                "success": True,
                "material_name": mat.name,
                "color_r": mat.diffuse_color[0],
            }))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["material_name"] == "TestMaterial"
        assert out["color_r"] == 1.0

    def test_blender_modifier_add(self) -> None:
        script = textwrap.dedent("""\
            import bpy, json
            cube = bpy.data.objects.get("Cube")
            mod = cube.modifiers.new(name="Subsurf", type="SUBSURF")
            mod.levels = 2
            print(json.dumps({
                "success": True,
                "modifier_type": mod.type,
                "levels": mod.levels,
            }))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["modifier_type"] == "SUBSURF"
        assert out["levels"] == 2

    def test_blender_python_version(self) -> None:
        script = textwrap.dedent("""\
            import sys, json
            print(json.dumps({
                "success": True,
                "major": sys.version_info.major,
                "minor": sys.version_info.minor,
            }))
        """)
        out = _run_blender_script(script)
        assert out["success"] is True
        assert out["major"] >= 3


@blender_available
class TestBlenderSkillPipeline:
    """dcc-mcp-core skill pipeline with Blender as target DCC."""

    def test_register_blender_actions(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register(name="create_sphere", description="Create UV Sphere", dcc="blender")
        reg.register(name="batch_rename", description="Rename objects", dcc="blender")
        reg.register(name="export_scene", description="Export scene", dcc="blender")
        reg.register(name="maya_action", description="Maya only", dcc="maya")
        blender_actions = reg.list_actions(dcc_name="blender")
        assert len(blender_actions) == 3
        names = {a["name"] for a in blender_actions}
        assert "maya_action" not in names

    def test_blender_action_result_model(self) -> None:
        result = dcc_mcp_core.success_result(
            "Created UV Sphere",
            object_name="Sphere",
            vertex_count=482,
            location=[0.0, 0.0, 0.0],
            radius=2.0,
        )
        assert result.success is True
        assert result.context["vertex_count"] == 482

    def test_blender_error_result_with_solutions(self) -> None:
        result = dcc_mcp_core.error_result(
            "Failed to export",
            "FileNotFoundError: directory does not exist",
            possible_solutions=["Create the directory", "Check permissions"],
            dcc="blender",
        )
        assert result.success is False
        assert len(result.context["possible_solutions"]) == 2
        assert result.context["dcc"] == "blender"

    def test_blender_event_bus(self) -> None:
        bus = dcc_mcp_core.EventBus()
        log: list[dict] = []
        bus.subscribe("blender.object.created", lambda **kw: log.append({"type": "created", **kw}))
        bus.subscribe("blender.scene.saved", lambda **kw: log.append({"type": "saved", **kw}))
        bus.publish("blender.object.created", name="Sphere", obj_type="MESH")
        bus.publish("blender.scene.saved", filepath="/tmp/scene.blend")
        assert len(log) == 2
        assert log[0]["name"] == "Sphere"
