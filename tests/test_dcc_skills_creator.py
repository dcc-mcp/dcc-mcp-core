"""Regression tests for the dcc-skills-creator bundled skill."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from types import ModuleType

import pytest

from conftest import REPO_ROOT
import dcc_mcp_core

SKILL_DIR = REPO_ROOT / "skills" / "dcc-skills-creator"
CREATE_SKILL_SCRIPT = SKILL_DIR / "scripts" / "create_skill.py"
SKILL_TEMPLATE_SCRIPT = SKILL_DIR / "scripts" / "skill_template.py"


def _load_module(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_create_skill_generates_valid_current_layout(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill")

    generated = Path(module.create_skill("maya-rigging-tools", str(tmp_path), dcc="maya"))

    assert (generated / "SKILL.md").is_file()
    assert (generated / "tools.yaml").is_file()
    assert (generated / "scripts" / "example_tool.py").is_file()

    report = dcc_mcp_core.validate_skill(str(generated))
    assert report.is_clean, [(issue.severity, issue.message) for issue in report.issues]

    meta = dcc_mcp_core.parse_skill_md(str(generated))
    assert meta is not None
    assert meta.name == "maya-rigging-tools"
    assert meta.dcc == "maya"
    assert meta.layer == "thin-harness"
    assert [tool.name for tool in meta.tools] == ["example_tool"]
    assert meta.tools[0].source_file == "scripts/example_tool.py"


def test_create_skill_example_tool_runs_successfully(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill_run")
    generated = Path(module.create_skill("python-smoke-tool", str(tmp_path)))

    env = dict(os.environ)
    python_path = str(REPO_ROOT / "python")
    env["PYTHONPATH"] = python_path + os.pathsep + env.get("PYTHONPATH", "")

    result = subprocess.run(
        [sys.executable, str(generated / "scripts" / "example_tool.py")],
        capture_output=True,
        text=True,
        timeout=30,
        check=False,
        env=env,
    )

    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    assert payload["success"] is True
    assert payload["message"] == "Hello from example_tool!"


def test_create_skill_rejects_non_kebab_case_name(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill_invalid")

    with pytest.raises(ValueError, match="kebab-case"):
        module.create_skill("Not Valid", str(tmp_path))


def test_skill_template_uses_valid_current_frontmatter(tmp_path: Path) -> None:
    module = _load_module(SKILL_TEMPLATE_SCRIPT, "dcc_skills_creator_skill_template")
    skill_dir = tmp_path / "my-skill"
    scripts_dir = skill_dir / "scripts"
    scripts_dir.mkdir(parents=True)

    (skill_dir / "SKILL.md").write_text(module.skill_template(), encoding="utf-8")
    (skill_dir / "tools.yaml").write_text(
        """tools:
  - name: my_tool
    description: "What this tool does"
    source_file: scripts/my_tool.py
    execution: sync
    affinity: any
""",
        encoding="utf-8",
    )
    (scripts_dir / "my_tool.py").write_text("def my_tool():\n    return {'success': True}\n", encoding="utf-8")

    report = dcc_mcp_core.validate_skill(str(skill_dir))
    assert report.is_clean, [(issue.severity, issue.message) for issue in report.issues]
