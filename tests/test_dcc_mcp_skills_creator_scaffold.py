"""Regression tests for the bundled DCC-MCP skill scaffold."""

from __future__ import annotations

import importlib.util
from pathlib import Path

from conftest import REPO_ROOT
import dcc_mcp_core


def _load_creator_scaffold():
    script = REPO_ROOT / "skills" / "dcc-mcp-skills-creator" / "scripts" / "create_skill.py"
    spec = importlib.util.spec_from_file_location("creator_create_skill", script)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_creator_scaffold_includes_codex_interface_metadata(tmp_path: Path) -> None:
    module = _load_creator_scaffold()

    skill_dir = Path(module.create_skill("maya-rigging-tools", str(tmp_path), dcc="maya"))
    openai_yaml = skill_dir / "agents" / "openai.yaml"
    interface = dcc_mcp_core.yaml_loads(openai_yaml.read_text(encoding="utf-8"))["interface"]

    assert interface == {
        "display_name": "Maya Rigging Tools",
        "short_description": "Run a structured DCC-MCP workflow",
        "default_prompt": "Use $maya-rigging-tools to complete this DCC-MCP workflow.",
    }
