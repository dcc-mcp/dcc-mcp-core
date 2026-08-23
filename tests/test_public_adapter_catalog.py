"""Public first-party adapter catalog boundary tests."""

from __future__ import annotations

from conftest import REPO_ROOT
from dcc_mcp_core import yaml_loads

CATALOG = REPO_ROOT / "dcc-mcp-catalog.yml"


def _entries() -> list:
    document = yaml_loads(CATALOG.read_text(encoding="utf-8"))
    return document["entries"]


def test_recent_adapter_releases_are_current() -> None:
    entries = {entry["name"]: entry for entry in _entries()}
    expected_versions = {
        "dcc-mcp-comfyui": "0.1.1",
        "dcc-mcp-touchdesigner": "0.1.1",
        "dcc-mcp-shogun": "0.10.0",
        "dcc-mcp-tiled": "0.3.0",
        "dcc-mcp-material-maker": "0.3.1",
        "dcc-mcp-wwise": "0.1.2",
    }

    assert {name: entries[name]["version"] for name in expected_versions} == expected_versions
    assert entries["dcc-mcp-comfyui"]["min_core_version"] == "0.19.91"
    assert "17 typed" in entries["dcc-mcp-comfyui"]["description"]
    assert "19 typed" in entries["dcc-mcp-touchdesigner"]["description"]
    shogun = entries["dcc-mcp-shogun"]
    assert "67 typed" in shogun["description"]
    assert shogun["install"]["sha256"] == ("c6c223d86d25902a32fd4cf057feb78de7670c6bacc5c848aeb7a8e82e14392b")
    assert shogun["install"]["instructions_url"].endswith("/main/install.md")


def test_skill_only_packages_are_not_pip_adapters() -> None:
    entry_names = {entry["name"] for entry in _entries()}

    # Cache Inspector is distributed through marketplace.json as a Skill pack.
    # It intentionally has no PyPI project or first-party adapter catalog entry.
    assert "dcc-mcp-cache-inspector" not in entry_names
