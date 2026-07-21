"""Tests for deterministic ClawHub skill archives."""

from __future__ import annotations

import importlib.util
from pathlib import Path
from zipfile import ZipFile

from conftest import REPO_ROOT

SCRIPT = REPO_ROOT / "scripts" / "package_openclaw_skill.py"


def load_packager():
    """Load the packaging script without invoking its CLI."""
    spec = importlib.util.spec_from_file_location("package_openclaw_skill_under_test", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_package_excludes_python_cache_and_bytecode(tmp_path: Path) -> None:
    packager = load_packager()
    skill_dir = tmp_path / "skills" / "dcc-mcp"
    script_dir = skill_dir / "scripts"
    agents_dir = skill_dir / "agents"
    cache_dir = script_dir / "__pycache__"
    cache_dir.mkdir(parents=True)
    agents_dir.mkdir()
    (skill_dir / "SKILL.md").write_text("---\nname: dcc-mcp\n---\n", encoding="utf-8")
    (agents_dir / "openai.yaml").write_text('interface:\n  display_name: "DCC-MCP"\n', encoding="utf-8")
    (script_dir / "helper.py").write_text("VALUE = 1\n", encoding="utf-8")
    (cache_dir / "helper.cpython-312.pyc").write_bytes(b"bytecode")
    (script_dir / "orphan.pyc").write_bytes(b"bytecode")

    output_dir = tmp_path / "dist"
    output_dir.mkdir()
    archive_path = packager.package_skill(skill_dir, output_dir, "1.2.3")

    with ZipFile(archive_path) as archive:
        names = set(archive.namelist())
    assert names == {
        "dcc-mcp/SKILL.md",
        "dcc-mcp/agents/openai.yaml",
        "dcc-mcp/scripts/helper.py",
    }
