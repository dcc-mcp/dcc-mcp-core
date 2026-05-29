"""Metadata contract tests for official and bundled skills."""

from __future__ import annotations

from conftest import REPO_ROOT
import dcc_mcp_core
from dcc_mcp_core.skill_reference_docs import _handle_list

SKILL_ROOTS = [
    REPO_ROOT / "skills" / "dcc-mcp-skills-creator",
    REPO_ROOT / "skills" / "dcc-mcp-creator",
    REPO_ROOT / "skills" / "dcc-cli-gateway",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "app-ui",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "dcc-diagnostics",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "media",
    REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "workflow",
]


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
