"""Regression tests for the bundled dcc-mcp-skills-creator validation script."""

from __future__ import annotations

import importlib.util
from pathlib import Path

from conftest import REPO_ROOT
import dcc_mcp_core


def _load_creator_validator():
    script = REPO_ROOT / "skills" / "dcc-mcp-skills-creator" / "scripts" / "validate_skill_dir.py"
    spec = importlib.util.spec_from_file_location("creator_validate_skill_dir", script)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _write_skill(skill_dir: Path, frontmatter: str) -> None:
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(f"---\n{frontmatter}\n---\n# Skill\n", encoding="utf-8")


def test_creator_validation_rejects_mgear_like_top_level_version(tmp_path: Path) -> None:
    module = _load_creator_validator()
    skill_dir = tmp_path / "maya-mgear"
    _write_skill(
        skill_dir,
        'name: maya-mgear\ndescription: mGear integration\nversion: "1.0.0"',
    )

    result = module.validate_skill_dir(str(skill_dir))

    assert result["has_errors"] is True
    messages = [issue["message"] for issue in result["issues"]]
    assert any("Non-spec top-level" in message for message in messages)
    assert any("metadata.dcc-mcp.version" in message for message in messages)


def test_creator_validation_accepts_nested_version_metadata(tmp_path: Path) -> None:
    module = _load_creator_validator()
    skill_dir = tmp_path / "maya-mgear"
    _write_skill(
        skill_dir,
        "\n".join(
            [
                "name: maya-mgear",
                "description: mGear integration",
                "metadata:",
                "  dcc-mcp:",
                "    dcc: maya",
                '    version: "1.0.0"',
            ]
        ),
    )

    result = module.validate_skill_dir(str(skill_dir))
    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))

    assert result["has_errors"] is False
    assert meta is not None
    assert meta.version == "1.0.0"
