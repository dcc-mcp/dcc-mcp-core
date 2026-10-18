"""Public first-party adapter catalog boundary tests."""

from __future__ import annotations

import json

import pytest

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
        "dcc-mcp-tiled": "0.5.1",
        "dcc-mcp-material-maker": "0.3.1",
        "dcc-mcp-wwise": "0.1.6",
    }

    assert {name: entries[name]["version"] for name in expected_versions} == expected_versions
    assert entries["dcc-mcp-comfyui"]["min_core_version"] == "0.19.91"
    assert "21 typed" in entries["dcc-mcp-comfyui"]["description"]
    assert "19 typed" in entries["dcc-mcp-touchdesigner"]["description"]
    shogun = entries["dcc-mcp-shogun"]
    assert "67 typed" in shogun["description"]
    assert shogun["install"]["sha256"] == ("c6c223d86d25902a32fd4cf057feb78de7670c6bacc5c848aeb7a8e82e14392b")
    assert shogun["install"]["instructions_url"].endswith("/main/install.md")


def test_obs_release_is_available_to_the_install_planner() -> None:
    entries = {entry["name"]: entry for entry in _entries()}
    obs = entries["dcc-mcp-obs"]

    assert obs["dcc"] == ["obs"]
    assert obs["version"] == "1.5.1"
    assert obs["min_core_version"] == "0.20.14"
    assert obs["install"] == {
        "type": "pip",
        "pip_package": "dcc-mcp-obs",
        "url": (
            "https://files.pythonhosted.org/packages/50/37/"
            "65b42856500d50548d5673a399ccfa1a85a61a7458858bb52881422ed6c2/"
            "dcc_mcp_obs-1.5.1-py3-none-any.whl"
        ),
        "sha256": "c47e2127e2331e5b8650a98ff777e97f76cedfccaf0fe2a5187981dfbffe57af",
        "entry_point": "dcc_mcp_obs:ObsMcpServer",
        "instructions_url": (
            "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-obs/475046ba823a199195f32ee729983770c2da6898/install.md"
        ),
    }


def test_godot_release_replaces_the_resolved_timeout_hold() -> None:
    entries = {entry["name"]: entry for entry in _entries()}
    godot = entries["dcc-mcp-godot"]

    assert godot["dcc"] == ["godot"]
    assert godot["version"] == "0.9.6"
    assert godot["min_core_version"] == "0.20.41"
    assert "policy" not in godot
    assert godot["install"] == {
        "type": "pip",
        "pip_package": "dcc-mcp-godot",
        "url": (
            "https://files.pythonhosted.org/packages/c3/f3/"
            "4a104c92c6a7b9dd9dc7519d08a8804c50896a1ecc9bbf796a80493f89a6/"
            "dcc_mcp_godot-0.9.6-py3-none-any.whl"
        ),
        "sha256": "1aa17201803e800638033e2581002e5d01d00c6ac772d6b54688bf2a1cc384dc",
        "instructions_url": (
            "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-godot/141a2a2253d72035ef099dab8e81e0f98aa4db80/install.md"
        ),
    }


def test_skill_only_packages_are_not_pip_adapters() -> None:
    entry_names = {entry["name"] for entry in _entries()}

    # Cache Inspector is distributed through marketplace.json as a Skill pack.
    # It intentionally has no PyPI project or first-party adapter catalog entry.
    assert "dcc-mcp-cache-inspector" not in entry_names


def test_liquigen_is_installable_from_pinned_pypi_wheel() -> None:
    entries = {entry["name"]: entry for entry in _entries()}
    liquigen = entries["dcc-mcp-liquigen"]

    assert liquigen["dcc"] == ["liquigen"]
    assert liquigen["version"] == "0.1.0"
    assert liquigen["min_core_version"] == "0.20.22"
    assert "typed node-graph" in liquigen["description"]
    assert liquigen["install"] == {
        "type": "pip",
        "pip_package": "dcc-mcp-liquigen",
        "url": (
            "https://files.pythonhosted.org/packages/57/04/09ef0e03c75d0aa2694338a4bb5d3e19d1b5c3d996dac3e635dab2ad450c/"
            "dcc_mcp_liquigen-0.1.0-py3-none-any.whl"
        ),
        "sha256": "130caacfb16b99b9365db071f145a7613f20b45d9b11a7397934c918b7691d0b",
        "entry_point": "dcc_mcp_liquigen.server:main",
        "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-liquigen/main/README.md",
    }


def test_discovery_decision_fixtures_match_curated_instructions() -> None:
    entries = {entry["name"]: entry for entry in _entries()}
    fixture_path = REPO_ROOT / "tests" / "fixtures" / "discovery-decision" / "game-engines.json"
    fixtures = json.loads(fixture_path.read_text(encoding="utf-8"))

    for case in fixtures["cases"]:
        entry = entries[f"dcc-mcp-{case['dcc_type']}"]
        assert case["expected_instructions_url"] == entry["install"]["instructions_url"], case["id"]


@pytest.mark.parametrize(
    "app,version,min_core_version,install",
    [
        (
            "photoshop",
            "0.1.44",
            "0.20.14",
            {
                "type": "pip",
                "pip_package": "dcc-mcp-photoshop",
                "url": "https://files.pythonhosted.org/packages/fc/b2/db13d1dbb4d39559e83b77be87bc7151fd6c6840b7c6edd7e359c313f3ee/dcc_mcp_photoshop-0.1.44-py3-none-any.whl",
                "sha256": "302dd849e52c69587e17822be4343f9136db9e25220c12a7ea073301fbbf13a7",
                "entry_point": "dcc_mcp_photoshop.cli:main",
                "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-photoshop/57f7ad4c60ab2597fbdd6eeb8d3e1ef1eeb50ff3/install.md",
            },
        ),
        (
            "premiere",
            "0.6.3",
            "0.20.34",
            {
                "type": "pip",
                "pip_package": "dcc-mcp-premiere",
                "url": "https://files.pythonhosted.org/packages/45/e4/21e305116bee10ce6153665528e4fc65fd21bf2e4039a4fa60c31c0e7ae4/dcc_mcp_premiere-0.6.3-py3-none-any.whl",
                "sha256": "d872850c2c693c5aa423bffca262eaf8d1c2a81b150a3f7532909c91ca54d999",
                "entry_point": "dcc_mcp_premiere.cli:main",
                "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-premiere/a4a541492e7f8ab3e3192aafcb44f66b13388f98/install.md",
            },
        ),
        (
            "aftereffects",
            "0.7.3",
            "0.20.14",
            {
                "type": "pip",
                "pip_package": "dcc-mcp-aftereffects",
                "url": "https://files.pythonhosted.org/packages/f0/eb/39410aedfd363aa8f9877d35da52c472e29b4f2b4a4b5df98c77b4bdaa58/dcc_mcp_aftereffects-0.7.3-py3-none-any.whl",
                "sha256": "e2ca0e74ad03c6314a2788793cac38669a67fdd8457ec3b5e2a02e4f4dd41417",
                "entry_point": "dcc_mcp_aftereffects.cli:main",
                "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-aftereffects/3907a3da3e318efd78a43833b9e518cd4c8a0bbc/install.md",
            },
        ),
        (
            "illustrator",
            "0.3.3",
            "0.20.14",
            {
                "type": "pip",
                "pip_package": "dcc-mcp-illustrator",
                "url": "https://files.pythonhosted.org/packages/6e/9a/c335d49bb8768e4eb32407459a922e387f4f57eac76511392fe257ad4ebe/dcc_mcp_illustrator-0.3.3-py3-none-any.whl",
                "sha256": "36f1261b36f9126972e15b2d7b7d6bf7ca6a58e6e0f2980f359b8bc4eff3e85e",
                "entry_point": "dcc_mcp_illustrator.cli:main",
                "instructions_url": "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-illustrator/6a6b6fdf607e362089c5355663c85f126701953e/install.md",
            },
        ),
    ],
)
def test_adobepy_012_adapter_releases_are_available_to_the_install_planner(
    app, version, min_core_version, install
) -> None:
    """Keep published wheel pins, CLI entry points, and release runbooks together."""
    entries = {entry["name"]: entry for entry in _entries()}
    entry = entries["dcc-mcp-" + app]

    assert entry["version"] == version
    assert entry["min_core_version"] == min_core_version
    assert "policy" not in entry
    assert entry["install"] == install

    matrix = (REPO_ROOT / "docs/guide/adapter-compatibility-matrix.md").read_text(encoding="utf-8")
    row = next(line for line in matrix.splitlines() if "[dcc-mcp-" + app + "](" in line)
    columns = [column.strip() for column in row.split("|")]
    upper_core = "0.21.0" if app == "photoshop" else "1.0.0"
    assert columns[3] == version
    assert columns[4] == ">=" + min_core_version + ",<" + upper_core
