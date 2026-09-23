"""P2 coverage for the marketplace-create-extension scaffold helpers.

Pins the name validators, the generated directory layout, the template contents
(``SKILL.md`` / ``tools.yaml`` / action script), and the ``_cli_main`` JSON
contract. The Codex ``agents/openai.yaml`` interface metadata is covered by
``test_marketplace_create_extension_scaffold.py``.
"""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import sys

import pytest

from conftest import REPO_ROOT
import dcc_mcp_core

_SCRIPT = REPO_ROOT / "skills" / "marketplace-create-extension" / "scripts" / "create_extension.py"
_SPEC = importlib.util.spec_from_file_location("marketplace_create_extension_units", _SCRIPT)
assert _SPEC is not None and _SPEC.loader is not None
_CREATE = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(_CREATE)


# ── _validate_skill_name ──────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "name",
    [
        pytest.param("maya-tools", id="kebab-case"),
        pytest.param("tools", id="single-word"),
        pytest.param("maya-tools-2", id="trailing-digit"),
        pytest.param("3d-tools", id="leading-digit"),
        pytest.param("a-b-c-d", id="many-segments"),
    ],
)
def test_validate_skill_name_accepts_kebab_case(name: str) -> None:
    _CREATE._validate_skill_name(name)


@pytest.mark.parametrize(
    "name",
    [
        pytest.param("", id="empty"),
        pytest.param("Maya-Tools", id="uppercase"),
        pytest.param("maya.tools", id="dotted"),
        pytest.param("-maya-tools", id="leading-hyphen"),
        pytest.param("maya-tools-", id="trailing-hyphen"),
        pytest.param("maya--tools", id="double-hyphen"),
        pytest.param("maya_tools", id="underscore"),
        pytest.param("maya tools", id="space"),
        pytest.param("maya/tools", id="slash"),
    ],
)
def test_validate_skill_name_rejects_everything_else(name: str) -> None:
    with pytest.raises(ValueError, match="kebab-case"):
        _CREATE._validate_skill_name(name)


# ── _validate_tool_name ───────────────────────────────────────────────────────


@pytest.mark.parametrize(
    "tool_name",
    [
        pytest.param("run", id="single-word"),
        pytest.param("run_export", id="snake-case"),
        pytest.param("run2_export", id="digits"),
        pytest.param("a_b_c", id="many-segments"),
    ],
)
def test_validate_tool_name_accepts_snake_case(tool_name: str) -> None:
    _CREATE._validate_tool_name(tool_name)


@pytest.mark.parametrize(
    "tool_name",
    [
        pytest.param("", id="empty"),
        pytest.param("run.export", id="dotted"),
        pytest.param("RunExport", id="uppercase"),
        pytest.param("run-export", id="hyphen"),
        pytest.param("_run", id="leading-underscore"),
        pytest.param("run__export", id="double-underscore"),
        pytest.param("run export", id="space"),
    ],
)
def test_validate_tool_name_rejects_client_unsafe_names(tool_name: str) -> None:
    with pytest.raises(ValueError, match="snake_case"):
        _CREATE._validate_tool_name(tool_name)


# ── create_extension ──────────────────────────────────────────────────────────


def test_create_extension_builds_the_expected_layout(tmp_path: Path) -> None:
    skill_dir = Path(
        _CREATE.create_extension(
            "maya-pipeline-tools",
            str(tmp_path),
            description="Publish Maya pipeline tools.",
            dcc_targets=["maya"],
            author="loonghao",
            action_name="run_export",
        )
    )

    assert skill_dir == (tmp_path / "maya-pipeline-tools").resolve()
    assert (skill_dir / "SKILL.md").is_file()
    assert (skill_dir / "tools.yaml").is_file()
    assert (skill_dir / "agents" / "openai.yaml").is_file()
    assert (skill_dir / "scripts" / "run_export.py").is_file()


def test_create_extension_defaults_to_the_run_action(tmp_path: Path) -> None:
    skill_dir = Path(_CREATE.create_extension("generic-tools", str(tmp_path)))

    assert (skill_dir / "scripts" / "run.py").is_file()


def test_create_extension_rejects_an_invalid_name(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="kebab-case"):
        _CREATE.create_extension("Maya.Tools", str(tmp_path))


def test_create_extension_rejects_an_invalid_action_name(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="snake_case"):
        _CREATE.create_extension("maya-tools", str(tmp_path), action_name="run.export")


def test_create_extension_rejects_an_unknown_install_type(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="install_type must be one of"):
        _CREATE.create_extension("maya-tools", str(tmp_path), install_type="nuget")


def test_create_extension_refuses_to_overwrite_an_existing_directory(tmp_path: Path) -> None:
    _CREATE.create_extension("maya-tools", str(tmp_path))

    with pytest.raises(FileExistsError, match="already exists"):
        _CREATE.create_extension("maya-tools", str(tmp_path))


# ── _make_skill_md ────────────────────────────────────────────────────────────


def test_make_skill_md_renders_frontmatter_and_license() -> None:
    text = _CREATE._make_skill_md("maya-tools", "Export Maya rigs.", "maya", "1.2.3", "run_export", "loonghao")

    assert text.startswith("---\n")
    assert "name: maya-tools\n" in text
    assert "description: >-\n  Export Maya rigs.\n" in text
    assert "license: MIT-0\n" in text
    assert "MIT-0 — see <https://github.com/aws/mit-0>" in text
    assert "    dcc: maya\n" in text
    assert '    version: "1.2.3"\n' in text
    assert "    layer: domain\n" in text
    assert "    tools: tools.yaml\n" in text
    assert "    maintainer: loonghao\n" in text


def test_make_skill_md_omits_the_maintainer_block_without_an_author() -> None:
    text = _CREATE._make_skill_md("maya-tools", "", "python", "0.1.0", "run", "")

    assert "maintainer" not in text
    assert "    layer: infrastructure\n" in text
    assert "    dcc: python\n" in text
    # A blank description falls back to the scaffold placeholder.
    assert "TODO: describe the user intent this extension serves." in text


def test_make_skill_md_closes_the_frontmatter_with_a_delimiter() -> None:
    text = _CREATE._make_skill_md("maya-tools", "Do a thing.", "maya", "0.1.0", "run", "")

    head, _, body = text.partition("\n---\n")
    assert head.startswith("---\n")
    assert body.startswith("\n# Maya Tools\n")


# ── _make_tools_yaml ──────────────────────────────────────────────────────────


def test_make_tools_yaml_parses_into_a_valid_tool_declaration() -> None:
    parsed = dcc_mcp_core.yaml_loads(_CREATE._make_tools_yaml("run_export", "Export Maya rigs."))

    tool = parsed["tools"][0]
    assert tool["name"] == "run_export"
    assert tool["description"] == "Export Maya rigs."
    assert tool["source_file"] == "scripts/run_export.py"
    assert tool["execution"] == "sync"
    assert tool["input_schema"]["type"] == "object"
    assert tool["input_schema"]["additionalProperties"] is False
    assert tool["input_schema"]["properties"]["dry_run"]["default"] is True
    assert tool["output_schema"]["properties"]["success"]["type"] == "boolean"
    assert tool["annotations"] == {
        "read_only_hint": True,
        "destructive_hint": False,
        "idempotent_hint": True,
        "open_world_hint": False,
        "deferred_hint": False,
    }
    assert tool["next-tools"] == {"on-success": [], "on-failure": []}


def test_make_tools_yaml_falls_back_to_a_placeholder_description() -> None:
    parsed = dcc_mcp_core.yaml_loads(_CREATE._make_tools_yaml("run", ""))

    assert "Replace with a concrete extension intent." in parsed["tools"][0]["description"]


# ── _make_action_script ───────────────────────────────────────────────────────


def test_make_action_script_is_importable_python_with_the_expected_signature() -> None:
    source = _CREATE._make_action_script("maya-tools", "run_export")

    compile(source, "run_export.py", "exec")
    assert "from dcc_mcp_core.skills_helper import run_main, skill_entry, skill_success" in source
    assert '@skill_entry\ndef main(label: str = "example", dry_run: bool = True, **params):' in source
    assert 'if __name__ == "__main__":\n    run_main(main)' in source


# ── _cli_main ─────────────────────────────────────────────────────────────────


def test_cli_main_prints_a_success_payload(tmp_path: Path, monkeypatch, capsys) -> None:
    monkeypatch.setattr(
        sys,
        "argv",
        [
            "create_extension.py",
            "--name",
            "maya-tools",
            "--description",
            "Export Maya rigs.",
            "--dcc_targets",
            "maya",
            "blender",
            "--author",
            "loonghao",
            "--version",
            "1.2.3",
            "--action_name",
            "run_export",
            "--output_dir",
            str(tmp_path),
        ],
    )

    _CREATE._cli_main()
    payload = json.loads(capsys.readouterr().out)

    assert payload["success"] is True
    assert payload["message"] == "Created marketplace extension package: maya-tools"
    assert payload["context"] == {
        "package_name": "maya-tools",
        "output_path": str((tmp_path / "maya-tools").resolve()),
        "install_type": "git",
        "dcc_targets": ["maya", "blender"],
        "version": "1.2.3",
        "author": "loonghao",
        "license": "MIT-0",
    }
    assert (tmp_path / "maya-tools" / "scripts" / "run_export.py").is_file()


def test_cli_main_reports_failures_and_exits_non_zero(tmp_path: Path, monkeypatch, capsys) -> None:
    monkeypatch.setattr(
        sys,
        "argv",
        ["create_extension.py", "--name", "Maya.Tools", "--output_dir", str(tmp_path)],
    )

    with pytest.raises(SystemExit) as excinfo:
        _CREATE._cli_main()
    payload = json.loads(capsys.readouterr().out)

    assert excinfo.value.code == 1
    assert payload["success"] is False
    assert "kebab-case" in payload["message"]
    assert not (tmp_path / "Maya.Tools").exists()
