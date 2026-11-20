"""Regression tests for the iteration-playbook documentation review (#2896).

These tests check the documented contract and its propagation across agent
entry points; they do not require a live DCC host.
"""

from __future__ import annotations

import ast

from conftest import REPO_ROOT
from dcc_mcp_core import derive_script_parameters_schema
from dcc_mcp_core import yaml_loads


PLAYBOOK_FILES = (
    "AGENTS.md",
    "docs/guide/agents-reference.md",
    "crates/dcc-mcp-gateway/src/gateway/native_resources/agent_workflows.md",
    ".agents/skills/dcc-mcp-core/SKILL.md",
    "llms-full.txt",
)


def _thin_harness_example() -> str:
    text = (REPO_ROOT / "docs" / "guide" / "thin-harness.md").read_text(encoding="utf-8")
    blocks = [
        block.split("\n", 1)[1]
        for block in text.split("```python\n")[1:]
        if "def execute_python(" in block
    ]
    assert len(blocks) == 1
    return blocks[0].split("\n```", 1)[0]


def test_thin_harness_example_declares_the_file_path_and_script_path_alias() -> None:
    tree = ast.parse(_thin_harness_example())
    entrypoints = [
        node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "execute_python"
    ]
    assert len(entrypoints) == 1
    arguments = {argument.arg for argument in entrypoints[0].args.args}
    assert {"file_path", "script_path", "params", "sha256", "timeout_secs"} <= arguments


def test_thin_harness_example_uses_one_namespace_and_helper_boundary() -> None:
    source = _thin_harness_example()
    assert "normalize_file_backed_script_execution_params(" in source
    assert "read_text" not in source
    assert "ast.parse" not in source
    assert "script_validation_failed" in source
    assert 'exec(compile(script.code, script.file_path, "exec"), namespace)' in source
    assert source.count("exec(compile") == 1


def test_documented_recipe_schema_rejects_unknown_arguments() -> None:
    source = (
        "def main(name: str, width: float, height: float, depth: float) -> dict:\n"
        "    return {}\n"
    )
    schema = derive_script_parameters_schema(source)
    assert schema is not None
    assert schema["additionalProperties"] is False
    assert set(schema["required"]) == {"name", "width", "height", "depth"}


def test_playbook_entry_points_teach_all_four_moves() -> None:
    for relative in PLAYBOOK_FILES:
        text = (REPO_ROOT / relative).read_text(encoding="utf-8")
        for tool in ("workflows_run", "workflows_get_status", "workflows_resume"):
            assert f"`{tool}`" in text, (relative, tool)
        assert "--json-file" in text, relative


def test_resume_guidance_never_falls_back_to_a_new_run() -> None:
    text = (
        REPO_ROOT
        / "crates/dcc-mcp-gateway/src/gateway/native_resources/agent_workflows.md"
    ).read_text(encoding="utf-8")
    section = text.split("## Iteration Playbook", 1)[1].split("---", 1)[0]
    assert "— fall back to `workflows_run`" not in section
    assert "Do not fall back to `workflows_run`" in section


def test_thin_harness_workflow_keys_use_cannot_annotations() -> None:
    text = (REPO_ROOT / "docs" / "guide" / "thin-harness.md").read_text(encoding="utf-8")
    block = text.split("### tools.yaml\n", 1)[1].split("```yaml\n", 1)[1].split("```", 1)[0]
    declaration = yaml_loads(block)["tools"][0]
    assert declaration["annotations"]["read_only_hint"] is False
    assert "read_only" not in declaration
