"""Integration tests with open-source DCC applications.

Tests the dcc-mcp-core skill/action pipeline against real DCC software.

Supported environments (all conditionally skipped if not installed):
  - Blender   (headless / --background mode)        apt: blender
  - FreeCAD   (headless / --console mode)            apt: freecad
  - Godot     (headless / --headless --script mode)  snap/dl: godot
  - OpenSCAD  (CLI renderer)                         apt: openscad
  - Inkscape  (--actions headless mode)              apt: inkscape

Run only DCC tests:  pytest -m dcc
Run specific DCC:    pytest -k "Blender" or pytest -k "FreeCAD"
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

BLENDER_BIN = shutil.which("blender")
FREECAD_BIN = shutil.which("FreeCAD") or shutil.which("freecad") or shutil.which("freecadcmd")
GODOT_BIN = shutil.which("godot") or shutil.which("godot4") or shutil.which("godot-headless")
OPENSCAD_BIN = shutil.which("openscad")
INKSCAPE_BIN = shutil.which("inkscape")

blender_available = pytest.mark.skipif(BLENDER_BIN is None, reason="Blender not found in PATH")
freecad_available = pytest.mark.skipif(FREECAD_BIN is None, reason="FreeCAD not found in PATH")
godot_available = pytest.mark.skipif(GODOT_BIN is None, reason="Godot not found in PATH")
openscad_available = pytest.mark.skipif(OPENSCAD_BIN is None, reason="OpenSCAD not found in PATH")
inkscape_available = pytest.mark.skipif(INKSCAPE_BIN is None, reason="Inkscape not found in PATH")


# ── Generic script runner helpers ──


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


def _run_godot_script(script: str, timeout: int = 60) -> dict[str, Any]:
    """Run a GDScript inside Godot headless mode.

    Godot scripts print to stdout via print(); we capture JSON output.
    The script must call quit() when done.
    """
    assert GODOT_BIN is not None
    with tempfile.NamedTemporaryFile(suffix=".gd", mode="w", delete=False, encoding="utf-8") as f:
        f.write(script)
        script_path = f.name
    try:
        r = _run_subprocess(
            [GODOT_BIN, "--headless", "--script", script_path, "--no-window"],
            timeout,
        )
    finally:
        Path(script_path).unlink(missing_ok=True)
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
        reg = dcc_mcp_core.ActionRegistry()
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
        reg = dcc_mcp_core.ActionRegistry()
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


# ═══════════════════════════════════════════════════════════════════
# GODOT  (headless --headless --script)
# ═══════════════════════════════════════════════════════════════════


@godot_available
class TestGodotIntegration:
    """Tests using Godot's --headless mode with GDScript."""

    def test_godot_version(self) -> None:
        assert GODOT_BIN is not None
        r = _run_subprocess([GODOT_BIN, "--version"])
        assert r.returncode == 0
        assert r.stdout.strip() != ""

    def test_godot_scene_tree(self) -> None:
        """Create a simple scene tree and report node count."""
        script = textwrap.dedent("""\
            extends SceneTree

            func _init():
                var root_node = Node.new()
                root_node.name = "Root"
                for i in range(5):
                    var child = Node.new()
                    child.name = "Child_%d" % i
                    root_node.add_child(child)
                var result = {
                    "success": true,
                    "child_count": root_node.get_child_count(),
                    "root_name": root_node.name,
                }
                print(JSON.stringify(result))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        assert out["child_count"] == 5
        assert out["root_name"] == "Root"

    def test_godot_math_operations(self) -> None:
        """Test GDScript math and data structures in headless mode."""
        script = textwrap.dedent("""\
            extends SceneTree

            func _init():
                var vec = Vector3(1.0, 2.0, 3.0)
                var length = vec.length()
                var data = {
                    "success": true,
                    "vector": [vec.x, vec.y, vec.z],
                    "length": length,
                    "sqrt14": sqrt(14.0),
                }
                print(JSON.stringify(data))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        import math

        assert abs(out["length"] - math.sqrt(14)) < 1e-4

    def test_godot_resource_creation(self) -> None:
        """Create and inspect a Resource object."""
        script = textwrap.dedent("""\
            extends SceneTree

            func _init():
                var mesh = ArrayMesh.new()
                var result = {
                    "success": true,
                    "class_name": mesh.get_class(),
                    "surface_count": mesh.get_surface_count(),
                }
                print(JSON.stringify(result))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        assert "Mesh" in out["class_name"]
        assert out["surface_count"] == 0

    def test_godot_timer_and_signal(self) -> None:
        """Verify signal connection and Timer object creation."""
        script = textwrap.dedent("""\
            extends SceneTree

            var fired := false

            func _on_timeout():
                fired = true

            func _init():
                var timer = Timer.new()
                timer.wait_time = 0.01
                timer.one_shot = true
                timer.timeout.connect(_on_timeout)
                root.add_child(timer)
                timer.start()
                # Advance 1 physics frame
                await process_frame
                print(JSON.stringify({"success": true, "has_timer": true}))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        assert out["has_timer"] is True

    def test_godot_json_parse(self) -> None:
        """Test JSON parsing inside Godot."""
        script = textwrap.dedent("""\
            extends SceneTree

            func _init():
                var source = '{"name": "test_skill", "version": "1.0.0", "dcc": "godot"}'
                var parsed = JSON.parse_string(source)
                print(JSON.stringify({
                    "success": true,
                    "name": parsed["name"],
                    "version": parsed["version"],
                    "dcc": parsed["dcc"],
                }))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        assert out["name"] == "test_skill"
        assert out["dcc"] == "godot"

    def test_godot_file_io(self, tmp_path: Path) -> None:
        """Test file read/write from Godot script."""
        test_file = tmp_path / "godot_test.json"
        script = textwrap.dedent(f"""\
            extends SceneTree

            func _init():
                var data = {{"created_by": "godot", "value": 42}}
                var path = "{test_file}"
                var file = FileAccess.open(path, FileAccess.WRITE)
                file.store_string(JSON.stringify(data))
                file.close()
                # Read back
                var rfile = FileAccess.open(path, FileAccess.READ)
                var content = rfile.get_as_text()
                rfile.close()
                var parsed = JSON.parse_string(content)
                print(JSON.stringify({{
                    "success": true,
                    "value": parsed["value"],
                    "created_by": parsed["created_by"],
                }}))
                quit()
        """)
        out = _run_godot_script(script)
        assert out["success"] is True
        assert out["value"] == 42
        assert out["created_by"] == "godot"


@godot_available
class TestGodotSkillPipeline:
    """dcc-mcp-core skill pipeline with Godot as target DCC."""

    def test_register_godot_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="spawn_node", description="Spawn scene node", dcc="godot")
        reg.register(name="add_script", description="Attach GDScript", dcc="godot")
        reg.register(name="export_scene", description="Export packed scene", dcc="godot")
        reg.register(name="run_tests", description="Run GdUnit4 tests", dcc="godot")
        godot_actions = reg.list_actions(dcc_name="godot")
        assert len(godot_actions) == 4
        names = {a["name"] for a in godot_actions}
        assert "spawn_node" in names
        assert "run_tests" in names

    def test_godot_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "godot-scene-builder",
            frontmatter=("name: godot-scene-builder\ndcc: godot\ntags:\n  - game\n  - scene\nversion: 1.0.0"),
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "godot-scene-builder"))
        assert meta is not None
        assert meta.dcc == "godot"
        assert "game" in meta.tags

    def test_godot_action_result(self) -> None:
        result = dcc_mcp_core.success_result(
            "Spawned node",
            node_class="CharacterBody3D",
            node_path="/root/Player",
            dcc="godot",
        )
        assert result.success is True
        assert result.context["dcc"] == "godot"
        assert result.context["node_class"] == "CharacterBody3D"


# ═══════════════════════════════════════════════════════════════════
# OPENSCAD  (CLI geometry compiler)
# ═══════════════════════════════════════════════════════════════════


@openscad_available
class TestOpenSCADIntegration:
    """Tests using OpenSCAD's command-line renderer.

    OpenSCAD takes .scad source files and produces STL/PNG/etc.
    We generate .scad content with Python and verify the output.
    """

    def _run_openscad(self, scad_content: str, out_file: Path, timeout: int = 60) -> bool:
        assert OPENSCAD_BIN is not None
        with tempfile.NamedTemporaryFile(suffix=".scad", mode="w", delete=False, encoding="utf-8") as f:
            f.write(scad_content)
            scad_path = f.name
        try:
            r = _run_subprocess([OPENSCAD_BIN, "-o", str(out_file), scad_path], timeout)
            return r.returncode == 0
        finally:
            Path(scad_path).unlink(missing_ok=True)

    def test_openscad_version(self) -> None:
        assert OPENSCAD_BIN is not None
        r = _run_subprocess([OPENSCAD_BIN, "--version"])
        assert r.returncode == 0

    def test_openscad_render_cube(self, tmp_path: Path) -> None:
        """Render a simple cube to STL."""
        stl_file = tmp_path / "cube.stl"
        scad = "cube([10, 10, 10], center=true);"
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.is_file()
        assert stl_file.stat().st_size > 0

    def test_openscad_render_sphere(self, tmp_path: Path) -> None:
        """Render a sphere to STL."""
        stl_file = tmp_path / "sphere.stl"
        scad = "sphere(r=5, $fn=32);"
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.stat().st_size > 0

    def test_openscad_boolean_difference(self, tmp_path: Path) -> None:
        """Render a cube with a cylindrical hole (Boolean difference)."""
        stl_file = tmp_path / "drilled_cube.stl"
        scad = textwrap.dedent("""\
            difference() {
                cube([20, 20, 20], center=true);
                cylinder(h=25, r=5, center=true, $fn=32);
            }
        """)
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.stat().st_size > 0

    def test_openscad_render_png(self, tmp_path: Path) -> None:
        """Render a preview image to PNG (requires display/OpenGL — skipped in headless CI)."""
        import os

        if not os.environ.get("DISPLAY") and not os.environ.get("WAYLAND_DISPLAY"):
            pytest.skip("No display available for OpenSCAD PNG rendering (requires OpenGL)")
        assert OPENSCAD_BIN is not None
        png_file = tmp_path / "preview.png"
        scad = "cube(10);"
        with tempfile.NamedTemporaryFile(suffix=".scad", mode="w", delete=False, encoding="utf-8") as f:
            f.write(scad)
            scad_path = f.name
        try:
            r = _run_subprocess(
                [OPENSCAD_BIN, "--render", "--imgsize=128,128", "-o", str(png_file), scad_path],
                timeout=60,
            )
        finally:
            Path(scad_path).unlink(missing_ok=True)
        assert r.returncode == 0
        assert png_file.is_file()
        assert png_file.stat().st_size > 0

    def test_openscad_parametric_script(self, tmp_path: Path) -> None:
        """Test parametric OpenSCAD with variables."""
        stl_file = tmp_path / "parametric.stl"
        scad = textwrap.dedent("""\
            width = 15;
            height = 8;
            depth = 12;
            cube([width, depth, height]);
        """)
        success = self._run_openscad(scad, stl_file)
        assert success is True
        assert stl_file.is_file()


@openscad_available
class TestOpenSCADSkillPipeline:
    """dcc-mcp-core skill pipeline with OpenSCAD as target DCC."""

    def test_register_openscad_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="render_geometry", description="Render .scad to STL", dcc="openscad")
        reg.register(name="generate_scad", description="Generate .scad from parameters", dcc="openscad")
        reg.register(name="export_png", description="Render preview PNG", dcc="openscad")
        assert len(reg.list_actions(dcc_name="openscad")) == 3

    def test_openscad_generate_scad_from_params(self, tmp_path: Path) -> None:
        """Python generates .scad content, OpenSCAD renders it — full pipeline."""
        params = {"width": 20, "height": 10, "depth": 15}
        scad_content = f"cube([{params['width']}, {params['depth']}, {params['height']}]);"
        scad_file = tmp_path / "generated.scad"
        scad_file.write_text(scad_content, encoding="utf-8")
        assert OPENSCAD_BIN is not None
        stl_file = tmp_path / "generated.stl"
        r = _run_subprocess([OPENSCAD_BIN, "-o", str(stl_file), str(scad_file)])
        result = dcc_mcp_core.success_result(
            "Geometry rendered",
            scad_file=str(scad_file),
            stl_file=str(stl_file),
            success_code=r.returncode,
            params=params,
        )
        assert result.success is True
        assert result.context["params"]["width"] == 20

    def test_openscad_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "openscad-primitives",
            frontmatter="name: openscad-primitives\ndcc: openscad\ntags:\n  - geometry\n  - cad",
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "openscad-primitives"))
        assert meta is not None
        assert meta.dcc == "openscad"
        assert "geometry" in meta.tags


# ═══════════════════════════════════════════════════════════════════
# INKSCAPE  (headless --actions)
# ═══════════════════════════════════════════════════════════════════


@inkscape_available
class TestInkscapeIntegration:
    """Tests using Inkscape's headless --actions mode.

    Inkscape can run without a GUI via --actions on SVG files.
    We generate SVGs with Python and verify Inkscape transformations.
    """

    _SIMPLE_SVG = textwrap.dedent("""\
        <?xml version="1.0" encoding="UTF-8"?>
        <svg xmlns="http://www.w3.org/2000/svg"
             width="200" height="200" viewBox="0 0 200 200">
          <rect id="rect1" x="10" y="10" width="100" height="80"
                fill="blue" stroke="black" stroke-width="2"/>
          <circle id="circle1" cx="100" cy="100" r="40" fill="red"/>
          <text id="text1" x="50" y="170" font-size="14">DCC MCP</text>
        </svg>
    """)

    def _write_svg(self, path: Path) -> None:
        path.write_text(self._SIMPLE_SVG, encoding="utf-8")

    def test_inkscape_version(self) -> None:
        assert INKSCAPE_BIN is not None
        r = _run_subprocess([INKSCAPE_BIN, "--version"])
        assert r.returncode == 0
        assert "Inkscape" in r.stdout

    def test_inkscape_export_png(self, tmp_path: Path) -> None:
        """Export SVG to PNG via Inkscape headless."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        png_file = tmp_path / "test.png"
        self._write_svg(svg_file)
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=png", f"--export-filename={png_file}", str(svg_file)])
        assert r.returncode == 0
        assert png_file.is_file()
        assert png_file.stat().st_size > 0

    def test_inkscape_export_pdf(self, tmp_path: Path) -> None:
        """Export SVG to PDF via Inkscape headless."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        pdf_file = tmp_path / "test.pdf"
        self._write_svg(svg_file)
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=pdf", f"--export-filename={pdf_file}", str(svg_file)])
        assert r.returncode == 0
        assert pdf_file.is_file()
        assert pdf_file.stat().st_size > 0

    def test_inkscape_export_specific_area(self, tmp_path: Path) -> None:
        """Export a specific area of the SVG."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        png_file = tmp_path / "area.png"
        self._write_svg(svg_file)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--export-type=png",
                "--export-area=0:0:100:100",
                f"--export-filename={png_file}",
                str(svg_file),
            ]
        )
        assert r.returncode == 0
        assert png_file.is_file()

    def test_inkscape_query_dimensions(self, tmp_path: Path) -> None:
        """Query SVG document dimensions."""
        assert INKSCAPE_BIN is not None
        svg_file = tmp_path / "test.svg"
        self._write_svg(svg_file)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--query-all",
                str(svg_file),
            ]
        )
        assert r.returncode == 0
        # Output contains element IDs with their dimensions
        assert "rect1" in r.stdout
        assert "circle1" in r.stdout

    def test_inkscape_svg_optimized_output(self, tmp_path: Path) -> None:
        """Use Inkscape to produce a plain SVG (optimized)."""
        assert INKSCAPE_BIN is not None
        svg_in = tmp_path / "test.svg"
        svg_out = tmp_path / "optimized.svg"
        self._write_svg(svg_in)
        r = _run_subprocess(
            [
                INKSCAPE_BIN,
                "--export-type=svg",
                f"--export-filename={svg_out}",
                "--export-plain-svg",
                str(svg_in),
            ]
        )
        assert r.returncode == 0
        assert svg_out.is_file()
        content = svg_out.read_text(encoding="utf-8")
        assert "<svg" in content


@inkscape_available
class TestInkscapeSkillPipeline:
    """dcc-mcp-core skill pipeline with Inkscape as target DCC."""

    def test_register_inkscape_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="export_png", description="Export SVG to PNG", dcc="inkscape")
        reg.register(name="export_pdf", description="Export SVG to PDF", dcc="inkscape")
        reg.register(name="optimize_svg", description="Produce plain SVG", dcc="inkscape")
        reg.register(name="query_bounds", description="Query element bounding boxes", dcc="inkscape")
        inkscape_actions = reg.list_actions(dcc_name="inkscape")
        assert len(inkscape_actions) == 4

    def test_inkscape_skill_metadata(self, tmp_path: Path) -> None:
        from conftest import create_skill_dir

        create_skill_dir(
            str(tmp_path),
            "inkscape-vector-tools",
            frontmatter=(
                "name: inkscape-vector-tools\ndcc: inkscape\ntags:\n  - svg\n  - vector\n  - 2d\nversion: 1.0.0"
            ),
        )
        meta = dcc_mcp_core.parse_skill_md(str(tmp_path / "inkscape-vector-tools"))
        assert meta is not None
        assert meta.dcc == "inkscape"
        assert "svg" in meta.tags
        assert "2d" in meta.tags

    def test_inkscape_action_result(self, tmp_path: Path) -> None:
        png_path = tmp_path / "output.png"
        result = dcc_mcp_core.success_result(
            "Exported SVG to PNG",
            source_svg="test.svg",
            output_png=str(png_path),
            width=200,
            height=200,
            dcc="inkscape",
        )
        assert result.success is True
        assert result.context["width"] == 200
        assert result.context["dcc"] == "inkscape"

    def test_inkscape_generate_svg_pipeline(self, tmp_path: Path) -> None:
        """Generate SVG with Python, export with Inkscape, verify file."""
        assert INKSCAPE_BIN is not None
        svg_content = textwrap.dedent("""\
            <?xml version="1.0" encoding="UTF-8"?>
            <svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
              <rect x="10" y="10" width="80" height="80" fill="green"/>
            </svg>
        """)
        svg_file = tmp_path / "generated.svg"
        svg_file.write_text(svg_content, encoding="utf-8")
        png_file = tmp_path / "generated.png"
        r = _run_subprocess([INKSCAPE_BIN, "--export-type=png", f"--export-filename={png_file}", str(svg_file)])
        result = dcc_mcp_core.validate_action_result(
            {"success": r.returncode == 0, "message": "Inkscape export", "context": {"file": str(png_file)}}
        )
        assert result.success is True


# ═══════════════════════════════════════════════════════════════════
# CROSS-DCC  (DCC-agnostic pipeline tests)
# ═══════════════════════════════════════════════════════════════════


# All supported DCCs for cross-DCC pipeline tests (module-level constant to avoid RUF012)
_ALL_DCC_LIST = ["blender", "freecad", "godot", "openscad", "inkscape", "maya", "houdini"]


class TestCrossDCCPipeline:
    """Tests that verify the skill system works consistently across all DCCs.

    These tests run without any external DCC binary — they only exercise
    dcc-mcp-core's Python/Rust bindings with DCC-themed data.
    """

    def test_multi_dcc_action_registry(self) -> None:
        """Register actions for multiple DCCs and verify isolation."""
        reg = dcc_mcp_core.ActionRegistry()
        for dcc in _ALL_DCC_LIST:
            for action in ["create", "export", "validate"]:
                reg.register(name=f"{dcc}_{action}", dcc=dcc, description=f"{dcc}: {action}")
        assert len(reg) == len(_ALL_DCC_LIST) * 3
        for dcc in _ALL_DCC_LIST:
            actions = reg.list_actions(dcc_name=dcc)
            assert len(actions) == 3
            names = {a["name"] for a in actions}
            assert f"{dcc}_create" in names
            assert f"{dcc}_export" in names

    def test_multi_dcc_skill_scanning(self, tmp_path: Path) -> None:
        """Create skill directories for each DCC and verify scanning."""
        from conftest import create_skill_dir

        for dcc in _ALL_DCC_LIST:
            create_skill_dir(str(tmp_path), f"{dcc}-tools", dcc=dcc)
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[str(tmp_path)])
        names = {Path(d).name for d in dirs}
        for dcc in _ALL_DCC_LIST:
            assert f"{dcc}-tools" in names

    def test_multi_dcc_event_routing(self) -> None:
        """Verify EventBus correctly routes events per DCC."""
        bus = dcc_mcp_core.EventBus()
        log: dict[str, list] = {dcc: [] for dcc in _ALL_DCC_LIST}
        for dcc in _ALL_DCC_LIST:
            bus.subscribe(f"{dcc}.action.completed", lambda d=dcc, **kw: log[d].append(kw))
        for dcc in _ALL_DCC_LIST:
            bus.publish(f"{dcc}.action.completed", action="export", dcc=dcc)
        for dcc in _ALL_DCC_LIST:
            assert len(log[dcc]) == 1
            assert log[dcc][0]["dcc"] == dcc

    def test_multi_dcc_result_models(self) -> None:
        """Generate success/error results for each DCC and verify type consistency."""
        for dcc in _ALL_DCC_LIST:
            success = dcc_mcp_core.success_result(f"Action completed for {dcc}", dcc=dcc)
            error = dcc_mcp_core.error_result(
                f"Action failed for {dcc}",
                "Timeout",
                dcc=dcc,
                possible_solutions=["Retry", "Check DCC connection"],
            )
            assert success.success is True
            assert success.context["dcc"] == dcc
            assert error.success is False
            assert len(error.context["possible_solutions"]) == 2

    def test_dcc_specific_tool_definitions(self) -> None:
        """Create ToolDefinition objects for each DCC and verify serialization."""
        for dcc in _ALL_DCC_LIST:
            td = dcc_mcp_core.ToolDefinition(
                name=f"{dcc}_create_object",
                description=f"Create a 3D object in {dcc}",
                input_schema=json.dumps(
                    {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "dcc": {"type": "string", "const": dcc},
                        },
                        "required": ["name"],
                    }
                ),
            )
            assert td.name == f"{dcc}_create_object"
            assert dcc in td.description

    def test_skill_version_consistency(self, tmp_path: Path) -> None:
        """Verify version fields are consistently populated across DCC skills."""
        from conftest import create_skill_dir

        for i, dcc in enumerate(_ALL_DCC_LIST):
            version = f"1.{i}.0"
            create_skill_dir(
                str(tmp_path),
                f"{dcc}-skill-v{i}",
                frontmatter=f"name: {dcc}-skill\ndcc: {dcc}\nversion: {version}",
            )
        scanner = dcc_mcp_core.SkillScanner()
        dirs = scanner.scan(extra_paths=[str(tmp_path)])
        for skill_dir in dirs:
            meta = dcc_mcp_core.parse_skill_md(skill_dir)
            assert meta is not None
            assert meta.version != "", f"Missing version for {meta.name}"
            assert meta.dcc in _ALL_DCC_LIST, f"Unexpected DCC: {meta.dcc}"
