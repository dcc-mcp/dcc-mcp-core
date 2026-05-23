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
from dcc_mcp_core._server.inprocess_executor import run_skill_script

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

    generated = Path(
        module.create_skill(
            "maya-rigging-tools",
            str(tmp_path),
            dcc="maya",
            tool_name="create_locator",
            affinity="main",
            stage="authoring",
        )
    )

    assert (generated / "SKILL.md").is_file()
    assert (generated / "tools.yaml").is_file()
    assert (generated / "scripts" / "create_locator.py").is_file()

    report = dcc_mcp_core.validate_skill(str(generated))
    assert report.is_clean, [(issue.severity, issue.message) for issue in report.issues]

    meta = dcc_mcp_core.parse_skill_md(str(generated))
    assert meta is not None
    assert meta.name == "maya-rigging-tools"
    assert meta.dcc == "maya"
    assert meta.layer == "thin-harness"
    assert meta.stage == "authoring"
    assert [tool.name for tool in meta.tools] == ["create_locator"]
    assert meta.tools[0].source_file == "scripts/create_locator.py"
    assert meta.tools[0].execution == "sync"
    assert meta.tools[0].enforce_thread_affinity is True
    assert json.loads(meta.tools[0].input_schema)["type"] == "object"
    assert json.loads(meta.tools[0].output_schema)["type"] == "object"
    assert meta.tools[0].annotations["readOnlyHint"] is True
    assert "affinity: main" in (generated / "tools.yaml").read_text(encoding="utf-8")


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
    assert payload["message"] == "Example tool completed"
    assert payload["context"]["label"] == "example"
    assert payload["context"]["dry_run"] is True
    assert payload["context"]["extra_params"] == {}


def test_create_skill_example_tool_runs_with_inprocess_executor(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill_inprocess")
    generated = Path(module.create_skill("python-inprocess-tool", str(tmp_path)))

    payload = run_skill_script(
        str(generated / "scripts" / "example_tool.py"),
        {"unused_param": "accepted"},
    )

    assert payload["success"] is True
    assert payload["message"] == "Example tool completed"
    assert payload["context"]["label"] == "example"
    assert payload["context"]["dry_run"] is True
    assert payload["context"]["extra_params"] == {"unused_param": "accepted"}


def test_create_skill_rejects_non_kebab_case_name(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill_invalid")

    with pytest.raises(ValueError, match="kebab-case"):
        module.create_skill("Not Valid", str(tmp_path))


def test_create_skill_rejects_dotted_tool_name(tmp_path: Path) -> None:
    module = _load_module(CREATE_SKILL_SCRIPT, "dcc_skills_creator_create_skill_bad_tool")

    with pytest.raises(ValueError, match="dotted names are not supported"):
        module.create_skill("bad-tool-skill", str(tmp_path), tool_name="bad.tool")


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
    input_schema:
      type: object
      properties: {}
    output_schema:
      type: object
    execution: sync
    affinity: any
    enforce_thread_affinity: true
    annotations:
      read_only_hint: true
      destructive_hint: false
      idempotent_hint: true
      open_world_hint: false
""",
        encoding="utf-8",
    )
    (scripts_dir / "my_tool.py").write_text("def my_tool():\n    return {'success': True}\n", encoding="utf-8")

    report = dcc_mcp_core.validate_skill(str(skill_dir))
    assert report.is_clean, [(issue.severity, issue.message) for issue in report.issues]
    assert "client-safe" in module.skill_template()
