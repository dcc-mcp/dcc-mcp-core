"""Godot integration tests for dcc-mcp-core.

Tests the dcc-mcp-core skill/action pipeline against Godot (headless --headless --script mode).
All tests are conditionally skipped if Godot is not installed.

Run:  pytest -m dcc -k "Godot"
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

GODOT_BIN = shutil.which("godot") or shutil.which("godot4") or shutil.which("godot-headless")

godot_available = pytest.mark.skipif(GODOT_BIN is None, reason="Godot not found in PATH")


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
