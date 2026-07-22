"""Metadata contract tests for official and bundled skills."""

from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

from conftest import REPO_ROOT
import dcc_mcp_core
from dcc_mcp_core import skill as skill_module
from dcc_mcp_core._server import skill_discovery
from dcc_mcp_core.skill_reference_docs import _handle_list

SKILL_ROOTS = [
    REPO_ROOT / "skills" / "dcc-mcp-skills-creator",
    REPO_ROOT / "skills" / "dcc-mcp-creator",
    REPO_ROOT / "skills" / "dcc-mcp",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "dcc-diagnostics",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "media",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "workflow",
]


def test_removed_app_ui_skill_is_not_bundled() -> None:
    bundled = REPO_ROOT / "python" / "dcc_mcp_core" / "skills"

    assert (bundled / "ui-control").is_dir()
    assert not (bundled / "app-ui").exists()


def test_bundled_api_keeps_root_contract_while_internal_discovery_ignores_upgrade_leftovers(
    tmp_path, monkeypatch
) -> None:
    bundled = tmp_path / "skills"
    for name in ("ui-control", "app-ui"):
        skill_dir = bundled / name
        skill_dir.mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text(f"---\nname: {name}\ndescription: test\n---\n", encoding="utf-8")
    monkeypatch.setattr(skill_module, "_BUNDLED_SKILLS_DIR", bundled)
    monkeypatch.setattr(skill_discovery, "_default_skill_paths_disabled", lambda: True)
    monkeypatch.setattr(skill_discovery, "get_app_skill_paths_from_env", lambda _dcc_name: [])
    monkeypatch.setattr(skill_discovery, "get_skill_paths_from_env", lambda: [])

    public_paths = skill_module.get_bundled_skill_paths()
    owner = SimpleNamespace(_builtin_skills_dir=tmp_path / "missing", _dcc_name="unity")
    discovery_paths = skill_discovery.SkillDiscoveryController(owner).collect_skill_search_paths(
        filter_existing=True,
        include_admin_custom=False,
    )

    assert public_paths == [str(bundled)]
    assert [Path(path).name for path in discovery_paths] == ["ui-control"]


def test_official_and_bundled_skills_validate_clean() -> None:
    for skill_dir in SKILL_ROOTS:
        report = dcc_mcp_core.validate_skill(str(skill_dir))
        assert report.is_clean, (skill_dir, [(issue.severity, issue.message) for issue in report.issues])


def test_bundled_tool_declarations_include_execution_and_affinity() -> None:
    for skill_dir in SKILL_ROOTS:
        meta = dcc_mcp_core.parse_skill_md(str(skill_dir))
        assert meta is not None, skill_dir
        for tool in meta.tools:
            assert tool.execution in ("sync", "async"), (skill_dir, tool.name)
            assert tool.enforce_thread_affinity is True, (skill_dir, tool.name)


def test_dcc_mcp_skills_creator_reference_docs_are_indexed() -> None:
    skill_dir = REPO_ROOT / "skills" / "dcc-mcp-skills-creator"
    meta = dcc_mcp_core.parse_skill_md(str(skill_dir))

    assert meta is not None
    result = _handle_list({meta.name: meta}, {"skill": meta.name})
    paths = {entry["path"] for entry in result["context"]["files"]}

    assert result["success"] is True
    assert {
        "references/AUTHORING_WORKFLOW.md",
        "references/DCC_TOOL_CONTRACTS.md",
    } <= paths


def test_dcc_mcp_skills_creator_exposes_improvement_prompt() -> None:
    prompt_path = REPO_ROOT / "skills" / "dcc-mcp-skills-creator" / "prompts.yaml"
    payload = dcc_mcp_core.yaml_loads(prompt_path.read_text(encoding="utf-8"))
    prompt = payload["prompts"][0]

    assert prompt["name"] == "review_skill_improvement"
    assert {argument["name"] for argument in prompt["arguments"]} == {
        "task_summary",
        "stats_json",
        "validation_summary",
        "existing_skill",
    }
