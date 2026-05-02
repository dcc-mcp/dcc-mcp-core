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

    def test_blender_to_blender_fbx_round_trip(self, tmp_path: Path) -> None:
        fbx_file = tmp_path / "sphere.fbx"
        export_script = textwrap.dedent(f"""\
            import bpy, os, json
            bpy.ops.object.select_all(action="SELECT")
            bpy.ops.object.delete()
            bpy.ops.mesh.primitive_uv_sphere_add(segments=32, ring_count=16, radius=2.0, location=(0, 0, 0))
            sphere = bpy.context.active_object
            expected_vertices = len(sphere.data.vertices)
            bpy.ops.export_scene.fbx(filepath=r"{fbx_file}", use_selection=False)
            print(json.dumps({{
                "success": os.path.isfile(r"{fbx_file}"),
                "file_size": os.path.getsize(r"{fbx_file}") if os.path.isfile(r"{fbx_file}") else 0,
                "expected_vertices": expected_vertices,
            }}))
        """)
        exported = _run_blender_script(export_script, timeout=90)
        assert exported["success"] is True
        assert exported["file_size"] > 0

        import_script = textwrap.dedent(f"""\
            import bpy, json
            from mathutils import Vector
            bpy.ops.object.select_all(action="SELECT")
            bpy.ops.object.delete()
            bpy.ops.import_scene.fbx(filepath=r"{fbx_file}")
            meshes = [obj for obj in bpy.context.scene.objects if obj.type == "MESH"]
            vertex_count = sum(len(obj.data.vertices) for obj in meshes)
            bounds = []
            for obj in meshes:
                bounds.extend([obj.matrix_world @ Vector(corner) for corner in obj.bound_box])
            bbox = {{
                "min": [min(v[i] for v in bounds) for i in range(3)] if bounds else [],
                "max": [max(v[i] for v in bounds) for i in range(3)] if bounds else [],
            }}
            print(json.dumps({{
                "success": True,
                "object_count": len(bpy.context.scene.objects),
                "mesh_count": len(meshes),
                "vertex_count": vertex_count,
                "has_mesh": bool(meshes),
                "bounding_box": bbox,
            }}))
        """)
        inspected = _run_blender_script(import_script, timeout=90)
        assert inspected["success"] is True
        assert inspected["has_mesh"] is True
        assert inspected["mesh_count"] >= 1
        assert inspected["vertex_count"] >= exported["expected_vertices"] * 0.95
        assert inspected["bounding_box"]["min"]
        assert inspected["bounding_box"]["max"]

    def test_blender_fbx_import_reports_bogus_path(self, tmp_path: Path) -> None:
        bogus_file = tmp_path / "missing.fbx"
        script = textwrap.dedent(f"""\
            import bpy, json
            try:
                bpy.ops.import_scene.fbx(filepath=r"{bogus_file}")
            except Exception as exc:
                print(json.dumps({{"success": False, "error": type(exc).__name__, "message": str(exc)}}))
            else:
                print(json.dumps({{"success": True}}))
        """)
        out = _run_blender_script(script, timeout=60)
        assert out["success"] is False
        assert out["error"]
        assert str(bogus_file) in out["message"] or "No such file" in out["message"]

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

    def test_blender_py37_can_load_schema_derivation_helper(self) -> None:
        schema_path = Path(dcc_mcp_core.__file__).resolve().parent / "schema.py"
        script = textwrap.dedent(f"""\
            import importlib.util, json
            from dataclasses import dataclass
            from typing import List, Optional, Tuple

            spec = importlib.util.spec_from_file_location("dcc_mcp_schema", r"{schema_path}")
            schema = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(schema)

            @dataclass
            class BlenderExportInput:
                object_names: List[str]
                frame_range: Tuple[int, int]
                collection: Optional[str] = None

            derived = schema.derive_schema(BlenderExportInput)
            props = derived["properties"]
            print(json.dumps({{
                "success": True,
                "title": derived["title"],
                "required": sorted(derived["required"]),
                "object_names_type": props["object_names"]["type"],
                "frame_range_items": len(props["frame_range"]["prefixItems"]),
                "collection_anyof": len(props["collection"]["anyOf"]),
            }}))
        """)
        out = _run_blender_script(script)
        assert out == {
            "success": True,
            "title": "BlenderExportInput",
            "required": ["frame_range", "object_names"],
            "object_names_type": "array",
            "frame_range_items": 2,
            "collection_anyof": 2,
        }


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
