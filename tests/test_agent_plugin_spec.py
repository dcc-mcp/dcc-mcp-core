"""Contract tests for the DCCMCP agent plugin manifest."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path

from conftest import REPO_ROOT

MANIFEST = REPO_ROOT / "skills" / "dcc-mcp" / "agent-plugin.json"
VALIDATOR = REPO_ROOT / "scripts" / "validate_agent_plugin.py"


def load_validator():
    spec = importlib.util.spec_from_file_location("validate_agent_plugin", VALIDATOR)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_checked_in_manifest_is_valid() -> None:
    validator = load_validator()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    assert validator.validate_manifest(manifest, MANIFEST.parent) == []


def test_manifest_declares_dccmcp_and_compatibility_command() -> None:
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    command = manifest["commands"][0]
    assert command["name"] == "dccmcp"
    assert "dcc-mcp-cli" in command["compatibility_aliases"]
    assert manifest["runtime"]["kind"] == "dccmcp-cli"
    assert manifest["version"] == "0.21.2"
    assert manifest["runtime"]["minimum_cli"] == "0.21.2"
    assert manifest["permissions"]["process"] == ["dccmcp", "dcc-mcp-cli"]


def test_validator_rejects_unknown_top_level_field() -> None:
    validator = load_validator()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    manifest["unexpected"] = True
    errors = validator.validate_manifest(manifest, MANIFEST.parent)
    assert any("unknown top-level fields" in error for error in errors)


def test_validator_requires_the_primary_command_and_alias() -> None:
    validator = load_validator()
    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    manifest["commands"][0] = {"name": "dcc-mcp-cli", "compatibility_aliases": []}
    errors = validator.validate_manifest(manifest, MANIFEST.parent)
    assert any("dccmcp" in error and "compatibility alias" in error for error in errors)


def test_validator_cli_accepts_checked_in_manifest() -> None:
    import subprocess
    import sys

    result = subprocess.run(
        [sys.executable, str(VALIDATOR), str(MANIFEST)],
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr
