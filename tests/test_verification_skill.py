"""Tests for the bundled cross-DCC verification skill (issue #2261)."""

from __future__ import annotations

from contextlib import contextmanager
import importlib.util
from pathlib import Path
import sys

import pytest

_SKILL_DIR = Path(__file__).parent.parent / "python" / "dcc_mcp_core" / "skills" / "verification"
_SCRIPTS = _SKILL_DIR / "scripts"


@contextmanager
def _script_import_context(script_path: Path):
    script_dir = str(script_path.resolve().parent)
    owns_path = script_dir not in sys.path
    if owns_path:
        sys.path.insert(0, script_dir)
    try:
        yield
    finally:
        if owns_path and script_dir in sys.path:
            sys.path.remove(script_dir)


def _load_script(name: str):
    script_path = _SCRIPTS / name
    spec = importlib.util.spec_from_file_location("_verification_{}_under_test".format(name[: -len(".py")]), script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    with _script_import_context(script_path):
        spec.loader.exec_module(module)
    return module


def test_verification_skill_declares_expected_tools():
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    assert meta.name == "verification"
    assert {tool.name for tool in meta.tools} == {
        "capture_review_views",
        "image_stats",
        "validate_scene_vs_spec",
        "make_comparison_sheet",
    }


def test_verification_skill_discoverable_and_loadable():
    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])

    names = [skill.name for skill in catalog.list_skills()]
    assert "verification" in names

    catalog.load_skill("verification")
    action_names = {action["name"] for action in registry.list_actions()}
    assert "verification__image_stats" in action_names
    assert "verification__validate_scene_vs_spec" in action_names
    assert "verification__capture_review_views" in action_names
    assert "verification__make_comparison_sheet" in action_names


def test_read_only_tools_are_limited_to_verification_reads():
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    read_only = {tool.name for tool in meta.tools if tool.read_only}
    assert read_only == {"capture_review_views", "image_stats", "validate_scene_vs_spec"}


def _write_ppm(path: Path, color, size: int = 8) -> None:
    r, g, b = color
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = b"P6\n%d %d\n255\n" % (size, size) + bytes([r, g, b] * (size * size))
    path.write_bytes(payload)


def test_image_stats_script_flags_white_frame(tmp_path):
    module = _load_script("image_stats.py")
    frame = tmp_path / "white.ppm"
    _write_ppm(frame, (255, 255, 255))

    result = module.main(input_path=str(frame))
    assert result["success"] is True
    assert result["context"]["stats"]["mean_luma"] == pytest.approx(1.0, abs=1e-6)
    assert result["context"]["flags"]["is_white"] is True
    assert result["context"]["flags"]["is_blank"] is True


def test_capture_review_views_emits_plan_and_validates(tmp_path):
    module = _load_script("capture_review_views.py")

    plan_result = module.main(resolution=[640, 480])
    assert plan_result["success"] is True
    assert [view["view"] for view in plan_result["context"]["plan"]] == ["front", "side", "top", "three_quarter"]
    assert plan_result["context"]["plan"][0]["resolution"] == [640, 480]

    good = [
        {"view": "front", "width": 640, "height": 480, "target": "active_viewport"},
        {"view": "side", "width": 640, "height": 480, "target": "active_viewport"},
        {"view": "top", "width": 640, "height": 480, "target": "active_viewport"},
        {"view": "three_quarter", "width": 640, "height": 480, "target": "active_viewport"},
    ]
    ok_result = module.main(resolution=[640, 480], captures=good)
    assert ok_result["context"]["passed"] is True
    assert ok_result["context"]["failures"] == []


def test_capture_review_views_fails_on_hidden_and_blank(tmp_path):
    module = _load_script("capture_review_views.py")

    bad = [
        {"view": "front", "width": 640, "height": 480, "target": "active_viewport", "hidden": True},
        {"view": "side", "width": 640, "height": 480, "target": "active_viewport", "is_blank": True},
        {"view": "top", "width": 640, "height": 480, "target": "active_viewport"},
        {"view": "three_quarter", "width": 640, "height": 480, "target": "active_viewport"},
    ]
    result = module.main(resolution=[640, 480], captures=bad)
    assert result["context"]["passed"] is False
    checks = {failure["view"]: failure["check"] for failure in result["context"]["failures"]}
    assert checks["front"] == "hidden"
    assert checks["side"] == "blank"


def test_validate_scene_vs_spec_script_reports_uv_gap(tmp_path):
    module = _load_script("validate_scene_vs_spec.py")

    scene = {
        "parts": ["body"],
        "meshes": [
            {"name": "body", "uv_sets": 0, "material": None, "is_non_manifold": False},
        ],
    }
    spec = {"min_uv_coverage": 1.0, "required_materials": ["mat"]}
    result = module.main(scene=scene, spec=spec)
    assert result["success"] is True
    assert result["context"]["result"]["passed"] is False
    assert {failure["check"] for failure in result["context"]["result"]["failures"]} == {"uv_coverage", "materials"}


def test_validate_scene_vs_spec_script_rejects_missing_scene(tmp_path):
    module = _load_script("validate_scene_vs_spec.py")
    result = module.main(spec={})
    assert result["success"] is False
    assert result["error"] == "invalid_input"


def test_make_comparison_sheet_builds_ffmpeg_command(tmp_path):
    module = _load_script("make_comparison_sheet.py")

    reference = tmp_path / "ref.ppm"
    render = tmp_path / "render.ppm"
    _write_ppm(reference, (255, 0, 0))
    _write_ppm(render, (0, 255, 0))
    output = tmp_path / "sheet.png"

    command = module._build_command([reference, render], output, "horizontal", False)
    assert command[0] == "ffmpeg"
    assert "-filter_complex" in command
    assert "hstack=inputs=2" in command
    assert command[-2:] == ["-n", str(output)]
