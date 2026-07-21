"""Regression tests for the bundled marketplace extension scaffold."""

from __future__ import annotations

import importlib.util
from pathlib import Path

from conftest import REPO_ROOT
import dcc_mcp_core


def _load_extension_scaffold():
    script = REPO_ROOT / "skills" / "marketplace-create-extension" / "scripts" / "create_extension.py"
    spec = importlib.util.spec_from_file_location("marketplace_create_extension", script)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_extension_scaffold_includes_codex_interface_metadata(tmp_path: Path) -> None:
    module = _load_extension_scaffold()

    skill_dir = Path(
        module.create_extension(
            "maya-pipeline-tools",
            str(tmp_path),
            description="Publish Maya pipeline tools.",
            dcc_targets=["maya"],
        )
    )
    openai_yaml = skill_dir / "agents" / "openai.yaml"
    interface = dcc_mcp_core.yaml_loads(openai_yaml.read_text(encoding="utf-8"))["interface"]

    assert interface == {
        "display_name": "Maya Pipeline Tools",
        "short_description": "Run a DCC-MCP marketplace extension",
        "default_prompt": "Use $maya-pipeline-tools to complete this DCC-MCP marketplace workflow.",
    }
