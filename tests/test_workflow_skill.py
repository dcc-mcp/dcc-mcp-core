"""Tests for the bundled workflow skill (run_chain).

Covers the core logic of run_chain.py directly (without subprocess) by
importing and exercising _interpolate, _build_local_dispatcher, and the
full main() function via subprocess.
"""

from __future__ import annotations

import json
from pathlib import Path
import subprocess
import sys
from typing import Any

import pytest

# Path to the bundled skill script
_SKILL_DIR = Path(__file__).parent / ".." / "python" / "dcc_mcp_core" / "skills" / "workflow"
_RUN_CHAIN = str((_SKILL_DIR / "scripts" / "run_chain.py").resolve())


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _run(steps: list, context: dict | None = None, env: dict | None = None) -> dict:
    """Run run_chain.py as a subprocess and return parsed JSON output."""
    import os

    cmd = [
        sys.executable,
        _RUN_CHAIN,
        "--steps",
        json.dumps(steps),
    ]
    if context:
        cmd += ["--context", json.dumps(context)]

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=30,
        env={**os.environ, **(env or {}), "DCC_MCP_IPC_ADDRESS": ""},
    )
    assert result.stdout.strip(), f"No stdout. stderr={result.stderr[:300]}"
    return json.loads(result.stdout.strip())


# ---------------------------------------------------------------------------
# Unit tests — _interpolate (import directly, no subprocess)
# ---------------------------------------------------------------------------


class TestInterpolate:
    """Unit tests for the {key} interpolation helper."""

    @pytest.fixture(autouse=True)
    def _import(self):
        import importlib.util

        spec = importlib.util.spec_from_file_location("run_chain", _RUN_CHAIN)
        self.mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.mod)

    def test_string_replaced(self):
        assert self.mod._interpolate("{name}", {"name": "cube"}) == "cube"

    def test_string_missing_key_kept(self):
        assert self.mod._interpolate("{missing}", {}) == "{missing}"

    def test_nested_dict(self):
        result = self.mod._interpolate({"path": "/tmp/{name}.fbx"}, {"name": "hero"})
        assert result == {"path": "/tmp/hero.fbx"}

    def test_nested_list(self):
        result = self.mod._interpolate(["{a}", "{b}"], {"a": "x", "b": "y"})
        assert result == ["x", "y"]

    def test_non_string_passthrough(self):
        assert self.mod._interpolate(42, {}) == 42
        assert self.mod._interpolate(True, {}) is True
        assert self.mod._interpolate(None, {}) is None

    def test_multiple_placeholders_in_one_string(self):
        result = self.mod._interpolate("{prefix}_{suffix}", {"prefix": "char", "suffix": "001"})
        assert result == "char_001"


# ---------------------------------------------------------------------------
# Integration tests — run_chain.py via subprocess
# ---------------------------------------------------------------------------


class TestRunChainBasic:
    """Basic chain execution without a live DCC server (local dispatcher)."""

    def test_empty_steps_fails(self):
        out = _run([])
        assert out["success"] is False
        assert "'steps' must be a non-empty" in out["message"]

    def test_missing_action_field_aborts(self):
        out = _run([{"label": "step-with-no-action"}])
        assert out["success"] is False
        assert out["context"]["aborted_at"] == 0

    def test_unknown_action_fails_and_aborts(self):
        """An unknown action should fail; chain aborts at step 0."""
        out = _run([{"action": "nonexistent_action__xyz"}])
        assert out["success"] is False
        assert out["context"]["aborted_at"] == 0
        assert out["context"]["completed_steps"] == 1

    def test_stop_on_failure_false_continues(self):
        """stop_on_failure=False: chain continues even after a failing step."""
        steps = [
            {"action": "bad_action_xyz", "stop_on_failure": False},
            {"action": "another_bad_action_xyz", "stop_on_failure": False},
        ]
        out = _run(steps)
        # Both steps attempted
        assert out["context"]["completed_steps"] == 2
        # Both failed
        assert out["context"]["failed_count"] == 2
        # Chain reports failure overall
        assert out["success"] is False

    def test_invalid_json_steps(self):
        import os
        import subprocess

        result = subprocess.run(
            [sys.executable, _RUN_CHAIN, "--steps", "NOT_JSON"],
            capture_output=True,
            text=True,
            timeout=10,
            env={**os.environ, "DCC_MCP_IPC_ADDRESS": ""},
        )
        out = json.loads(result.stdout.strip())
        assert out["success"] is False
        assert "Invalid JSON" in out["message"]


class TestRunChainOutput:
    """Verify output structure and fields."""

    def test_result_has_required_keys(self):
        out = _run([{"action": "noop_action"}])
        assert "success" in out
        assert "message" in out
        assert "prompt" in out
        assert "context" in out

    def test_context_has_meta_fields(self):
        out = _run([{"action": "noop_action"}])
        ctx = out["context"]
        assert "completed_steps" in ctx
        assert "total_steps" in ctx
        assert "aborted_at" in ctx
        assert "failed_count" in ctx
        assert "dispatch_source" in ctx
        assert "accumulated_context" in ctx
        assert "results" in ctx

    def test_results_list_has_step_entries(self):
        out = _run([{"action": "step_one"}, {"action": "step_two", "stop_on_failure": False}])
        results = out["context"]["results"]
        assert len(results) >= 1
        entry = results[0]
        assert "step" in entry
        assert "action" in entry
        assert "success" in entry
        assert "duration_ms" in entry

    def test_dispatch_source_is_local_without_ipc(self):
        out = _run([{"action": "any_action"}])
        assert out["context"]["dispatch_source"] == "local"

    def test_prompt_present_on_failure(self):
        out = _run([{"action": "bad_action"}])
        assert out["success"] is False
        assert "prompt" in out
        assert len(out["prompt"]) > 0

    def test_prompt_mentions_diagnostics_on_failure(self):
        out = _run([{"action": "bad_action"}])
        assert "dcc_diagnostics" in out["prompt"]

    def test_total_steps_matches_input(self):
        steps = [{"action": "a", "stop_on_failure": False}, {"action": "b", "stop_on_failure": False}]
        out = _run(steps)
        assert out["context"]["total_steps"] == 2


class TestRunChainContextPropagation:
    """Context interpolation and propagation across steps."""

    def test_initial_context_available_in_interpolation(self):
        """Initial context values should be substitutable in params."""
        steps = [{"action": "some_action", "params": {"path": "{export_path}"}}]
        out = _run(steps, context={"export_path": "/tmp/hero.fbx"})
        # Step ran (even if action unknown); check accumulated context contains initial value
        assert out["context"]["accumulated_context"].get("export_path") == "/tmp/hero.fbx"

    def test_accumulated_context_preserved(self):
        steps = [
            {"action": "step_a", "stop_on_failure": False},
            {"action": "step_b", "stop_on_failure": False},
        ]
        out = _run(steps, context={"key": "value"})
        assert out["context"]["accumulated_context"].get("key") == "value"

    def test_label_used_in_failure_message(self):
        steps = [{"action": "bad_action", "label": "Export FBX"}]
        out = _run(steps)
        assert "Export FBX" in out["message"]


class TestRunChainSkillParsing:
    """Verify the skill is correctly parsed from SKILL.md."""

    def test_skill_md_exists(self):
        assert (_SKILL_DIR / "SKILL.md").exists()

    def test_skill_md_parseable(self):
        from dcc_mcp_core import parse_skill_md

        meta = parse_skill_md(str(_SKILL_DIR))
        assert meta is not None
        assert meta.name == "workflow"
        assert meta.dcc == "python"

    def test_skill_has_run_chain_tool(self):
        from dcc_mcp_core import parse_skill_md

        meta = parse_skill_md(str(_SKILL_DIR))
        tool_names = [t.name for t in meta.tools]
        assert "run_chain" in tool_names

    def test_run_chain_tool_has_steps_schema(self):
        from dcc_mcp_core import parse_skill_md

        meta = parse_skill_md(str(_SKILL_DIR))
        tool = next(t for t in meta.tools if t.name == "run_chain")
        schema = json.loads(tool.input_schema)
        assert "steps" in schema.get("properties", {})
        assert "steps" in schema.get("required", [])

    def test_script_file_exists(self):
        assert (_SKILL_DIR / "scripts" / "run_chain.py").exists()

    def test_skill_loaded_in_catalog(self):
        """Verify workflow skill loads cleanly via SkillCatalog."""
        from dcc_mcp_core import SkillCatalog
        from dcc_mcp_core import ToolRegistry

        registry = ToolRegistry()
        cat = SkillCatalog(registry)
        count = cat.discover(extra_paths=[str(_SKILL_DIR.parent)])
        assert count >= 1
        names = [s.name for s in cat.list_skills()]
        assert "workflow" in names

    def test_workflow_action_registered_after_load(self):
        """After load_skill, workflow__run_chain appears in registry."""
        from dcc_mcp_core import SkillCatalog
        from dcc_mcp_core import ToolRegistry

        registry = ToolRegistry()
        cat = SkillCatalog(registry)
        cat.discover(extra_paths=[str(_SKILL_DIR.parent)])
        cat.load_skill("workflow")
        actions = registry.list_actions()
        action_names = [a["name"] for a in actions]
        assert any("workflow" in n and "run_chain" in n for n in action_names)


class TestRunChainBundledDiscovery:
    """Verify workflow is discoverable via get_bundled_skill_paths()."""

    def test_bundled_dir_contains_workflow(self):
        from dcc_mcp_core import get_bundled_skills_dir

        bundled = Path(get_bundled_skills_dir())
        assert (bundled / "workflow").is_dir()
        assert (bundled / "workflow" / "SKILL.md").exists()

    def test_get_bundled_skill_paths_returns_list(self):
        from dcc_mcp_core import get_bundled_skill_paths

        paths = get_bundled_skill_paths()
        assert isinstance(paths, list)
        assert len(paths) == 1

    def test_get_bundled_skill_paths_opt_out(self):
        from dcc_mcp_core import get_bundled_skill_paths

        paths = get_bundled_skill_paths(include_bundled=False)
        assert paths == []

    def test_workflow_discoverable_via_bundled_path(self):
        from dcc_mcp_core import SkillCatalog
        from dcc_mcp_core import ToolRegistry
        from dcc_mcp_core import get_bundled_skill_paths

        paths = get_bundled_skill_paths()
        registry = ToolRegistry()
        cat = SkillCatalog(registry)
        cat.discover(extra_paths=paths)
        names = [s.name for s in cat.list_skills()]
        assert "workflow" in names
