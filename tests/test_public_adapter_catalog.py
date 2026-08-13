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
        "dcc-mcp-shogun": "0.7.0",
        "dcc-mcp-tiled": "0.3.0",
        "dcc-mcp-material-maker": "0.3.1",
        "dcc-mcp-wwise": "0.1.2",
    }

    assert {name: entries[name]["version"] for name in expected_versions} == expected_versions
    assert "61 typed" in entries["dcc-mcp-shogun"]["description"]


def test_skill_only_packages_are_not_pip_adapters() -> None:
    entry_names = {entry["name"] for entry in _entries()}

    # Cache Inspector is distributed through marketplace.json as a Skill pack.
    # It intentionally has no PyPI project or first-party adapter catalog entry.
    assert "dcc-mcp-cache-inspector" not in entry_names
