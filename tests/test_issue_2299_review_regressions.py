"""Regression tests for the iteration-playbook documentation review (#2896).

The documented recipe is compiled with a stubbed decorator and run against a
real script executor. A DCC host or the native extension is not required.
"""

from __future__ import annotations

import ast
import hashlib
import itertools
from pathlib import Path
import textwrap

import pytest
import yaml

from conftest import REPO_ROOT
from dcc_mcp_core import derive_script_parameters_schema
from dcc_mcp_core import normalize_file_backed_script_execution_params

PLAYBOOK_FILES = (
    "AGENTS.md",
    "docs/guide/agents-reference.md",
    "crates/dcc-mcp-gateway/src/gateway/native_resources/agent_workflows.md",
    ".agents/skills/dcc-mcp-core/SKILL.md",
    "llms-full.txt",
)
THIN_HARNESS_PATH = "docs/guide/thin-harness.md"
RESUME_NAVIGATION = "docs/guide/agents-reference.md#start-a-repeatable-sequence-with-workflows_run"


def _read(relative: str) -> str:
    return (REPO_ROOT / relative).read_text(encoding="utf-8")


def _thin_harness_example() -> str:
    text = _read(THIN_HARNESS_PATH)
    blocks = [block.split("\n", 1)[1] for block in text.split("```python\n")[1:] if "def execute_python(" in block]
    assert len(blocks) == 1
    return blocks[0].split("\n```", 1)[0]


def _success_result(message: str, **context) -> dict:
    return {"success": True, "message": message, "context": context}


def _trace_result(message: str, error: str, **context) -> dict:
    raw_call = context.get("underlying_call")
    raw_call = raw_call if isinstance(raw_call, str) else repr(raw_call)
    return {
        "success": False,
        "message": message,
        "error": error,
        "context": context,
        "underlying_call": raw_call,
    }


def _compile_documented_example():
    source = "from __future__ import annotations\n\nfrom dcc_mcp_core import skill_entry\n\n" + _thin_harness_example()
    module = ast.parse(source)
    module.body = [node for node in module.body if not isinstance(node, ast.ImportFrom)]
    for node in module.body:
        decorators = getattr(node, "decorator_list", [])
        node.decorator_list = [
            decorator
            for decorator in decorators
            if not (isinstance(decorator, ast.Name) and decorator.id == "skill_entry")
        ]
    code = compile(ast.fix_missing_locations(module), THIN_HARNESS_PATH, "exec")
    namespace = {
        "Path": Path,
        "__builtins__": __builtins__,
        "normalize_file_backed_script_execution_params": normalize_file_backed_script_execution_params,
        "skill_entry": lambda function: function,
        "skill_error_with_trace": _trace_result,
        "skill_success": _success_result,
    }
    exec(code, namespace)
    return namespace["execute_python"]


@pytest.fixture
def trusted_script_factory(tmp_path):
    root = tmp_path / "studio" / "maya-scripts"
    root.mkdir(parents=True)
    counter = itertools.count()

    def factory(source: str):
        script_path = root / f"script_{next(counter)}.py"
        script_path.write_text(textwrap.dedent(source), encoding="utf-8")
        return script_path

    return factory


def _prepare(monkeypatch, trusted_script_factory, source: str):
    home = trusted_script_factory("pass").parent.parent.parent
    monkeypatch.setattr(Path, "home", staticmethod(lambda: home))
    script_path = trusted_script_factory(source)
    return script_path, hashlib.sha256(script_path.read_text(encoding="utf-8").encode("utf-8")).hexdigest()


SCRIPT_WITH_IMPORT = "import math\n\ndef main(value: float) -> dict:\n    return {'value': math.sqrt(value)}\n"


def test_example_signature_accepts_alias_only_and_rejects_no_path(monkeypatch, trusted_script_factory) -> None:
    tree = ast.parse(_thin_harness_example())
    entrypoints = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "execute_python"]
    assert len(entrypoints) == 1
    arguments = {argument.arg for argument in entrypoints[0].args.args}
    defaults = entrypoints[0].args.defaults
    assert {"file_path", "script_path", "params", "sha256", "timeout_secs"} <= arguments
    assert {argument.arg for argument in entrypoints[0].args.args[-len(defaults) :]} >= {
        "file_path",
        "script_path",
    }

    script_path, sha256 = _prepare(monkeypatch, trusted_script_factory, SCRIPT_WITH_IMPORT)
    execute_python = _compile_documented_example()
    alias_only = execute_python(script_path=str(script_path), sha256=sha256, params={"value": 9.0})
    file_only = execute_python(file_path=str(script_path), sha256=sha256, params={"value": 4.0})
    no_path = execute_python(params={"value": 1.0})
    assert alias_only["success"] and alias_only["context"]["output"] == {"value": 3.0}
    assert file_only["success"] and file_only["context"]["output"] == {"value": 2.0}
    assert no_path["error"] == "script_validation_failed"
    assert "either file_path or script_path is required" in no_path["message"]


def test_example_uses_the_shared_normalizer_and_one_namespace(monkeypatch, trusted_script_factory) -> None:
    script_path, sha256 = _prepare(monkeypatch, trusted_script_factory, SCRIPT_WITH_IMPORT)
    execute_python = _compile_documented_example()
    result = execute_python(file_path=str(script_path), sha256=sha256, params={"value": 9.0})
    assert result["success"], result
    assert result["context"]["output"] == {"value": 3.0}

    source = _thin_harness_example()
    assert "normalize_file_backed_script_execution_params(" in source
    assert "read_text" not in source
    assert "ast.parse" not in source
    assert "script_validation_failed" in source
    assert 'exec(compile(script.code, script.file_path, "exec"), namespace)' in source
    assert source.count("exec(compile") == 1


def test_example_fails_closed_on_hash_mismatch(monkeypatch, trusted_script_factory) -> None:
    source = "def main(value: float) -> dict:\n    return {'value': value}\n"
    script_path, sha256 = _prepare(monkeypatch, trusted_script_factory, source)
    execute_python = _compile_documented_example()
    assert sha256 != "0" * 64
    result = execute_python(file_path=str(script_path), sha256="0" * 64, params={"value": 1.0})
    assert result["error"] == "script_validation_failed"
    assert "sha256 does not match the script file" in result["message"]


def test_example_rejects_undeclared_params_before_execution(monkeypatch, trusted_script_factory) -> None:
    source = "def main(name: str, width: float, height: float, depth: float) -> dict:\n    return {}\n"
    script_path, sha256 = _prepare(monkeypatch, trusted_script_factory, source)
    execute_python = _compile_documented_example()
    result = execute_python(
        file_path=str(script_path),
        sha256=sha256,
        params={"name": "heroCube", "width": 1.0, "height": 1.0, "depth": 1.0, "extra": True},
    )
    assert result["error"] == "script_validation_failed"
    assert "extra" in result["message"] or "additional" in result["message"]


def test_documented_recipe_templates_import_their_dcc_namespace() -> None:
    text = _read(THIN_HARNESS_PATH)
    recipes = text.split("```markdown\n", 1)[1].split("\n```", 1)[0]
    for anchor in ("create_polygon_cube", "set_world_translation"):
        heading = f"## {anchor}\n"
        assert heading in recipes
        snippet = recipes.split(heading, 1)[1].split("\n## ", 1)[0]
        escaped_fence = "\\`" * 3
        prefix = escaped_fence + "python\n"
        assert prefix in snippet
        code = snippet.split(prefix, 1)[1].split("\n" + escaped_fence, 1)[0]
        assert "from maya import cmds" in code
        ast.parse(textwrap.dedent(code))


def test_documented_recipe_schema_rejects_unknown_arguments() -> None:
    source = "def main(name: str, width: float, height: float, depth: float) -> dict:\n    return {}\n"
    schema = derive_script_parameters_schema(source)
    assert schema is not None
    assert schema["additionalProperties"] is False
    assert set(schema["required"]) == {"name", "width", "height", "depth"}


def test_playbook_entry_points_teach_all_four_moves() -> None:
    for relative in PLAYBOOK_FILES:
        text = _read(relative)
        for tool in ("workflows_run", "workflows_get_status", "workflows_resume"):
            assert f"`{tool}`" in text, (relative, tool)
        assert "--json-file" in text, relative


def test_resume_navigation_is_explicit_and_never_restarts_the_run() -> None:
    gateway = _read("crates/dcc-mcp-gateway/src/gateway/native_resources/agent_workflows.md")
    section = gateway.split("## Iteration Playbook", 1)[1].split("\n---\n", 1)[0]
    assert "— fall back to `workflows_run`" not in section
    assert "Do not fall back to `workflows_run`" in section

    assert RESUME_NAVIGATION in _read("AGENTS.md")
    assert RESUME_NAVIGATION in _read(".agents/skills/dcc-mcp-core/SKILL.md")
    agents = _read("AGENTS.md")
    assert "do not start a new `workflows_run`" in agents.lower()

    reference = _read("docs/guide/agents-reference.md")
    run_section = reference.split("### 3. Start a repeatable sequence with `workflows_run`", 1)[1]
    run_section = run_section.split("### 4.", 1)[0]
    assert "WorkflowStorage" in run_section
    assert "job-persist-sqlite" in run_section
    assert "cannot resume" in run_section
    assert "resume navigation in section 4" in run_section


def test_gateway_guidance_uses_the_current_discovery_flow() -> None:
    guide = _read("AGENTS.md")
    assert '`search_tools("materialize script")`' not in guide
    assert "POST /v1/search" in guide
    assert "dccmcp search" in guide
    assert "dccmcp call" in guide


def test_thin_harness_additional_properties_wording_is_accurate() -> None:
    text = _read(THIN_HARNESS_PATH)
    assert "rejects undeclared or extra parameters" in text
    assert "rejects declared parameters" not in text


def test_thin_harness_workflow_keys_use_cannot_annotations() -> None:
    text = _read(THIN_HARNESS_PATH)
    declaration_text = text.split("### tools.yaml\n", 1)[1].split("```yaml\n", 1)[1].split("```", 1)[0]
    declaration = yaml.safe_load(declaration_text)["tools"][0]
    assert declaration["annotations"]["read_only_hint"] is False
    assert "read_only" not in declaration
