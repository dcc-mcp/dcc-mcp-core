"""Desktop automation test harness for UI Control.

Tests the scenario scripting, snapshot diff, action replay, policy matrix,
and CI integration of the ui-control skill's deterministic mock backend.
No real Windows desktop or UI Automation host is required.
"""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiControlPolicy

# The ui-control skill directory contains a hyphen, which prevents standard
# Python imports.  Use importlib to load the script modules by file path.
_SCRIPTS_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "scripts"


def _load_backend() -> Any:
    """Dynamically load _backend.py."""
    module_name = "dcc_mcp_core_ui_control_backend"
    spec = importlib.util.spec_from_file_location(
        module_name,
        _SCRIPTS_DIR / "_backend.py",
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


def _load_scenario() -> Any:
    """Dynamically load _scenario.py."""
    module_name = "dcc_mcp_core_ui_control_scenario"
    spec = importlib.util.spec_from_file_location(
        module_name,
        _SCRIPTS_DIR / "_scenario.py",
    )
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


_backend_mod = _load_backend()
_scenario_mod = _load_scenario()

# Extract commonly-used symbols for convenience
_snapshot_dict = _backend_mod._snapshot_dict
_load_state = _backend_mod._load_state
_save_state = _backend_mod._save_state

ActionRecorder = _scenario_mod.ActionRecorder
ActionReplayer = _scenario_mod.ActionReplayer
ReplayTrace = _scenario_mod.ReplayTrace
ScenarioRunner = _scenario_mod.ScenarioRunner
ScenarioScript = _scenario_mod.ScenarioScript
ScenarioStep = _scenario_mod.ScenarioStep
build_policy_test_cases = _scenario_mod.build_policy_test_cases
builtin_scenarios = _scenario_mod.builtin_scenarios
check_policy_flag_coverage = _scenario_mod.check_policy_flag_coverage
diff_snapshots = _scenario_mod.diff_snapshots
POLICY_BOOLEAN_FLAGS = _scenario_mod.POLICY_BOOLEAN_FLAGS

_JUSTFILE = REPO_ROOT / "justfile"


# ── Helpers ──────────────────────────────────────────────────────────────────


class _MockState:
    """Context manager that sets DCC_MCP_UI_CONTROL_MOCK_STATE_DIR for the
    duration of the block and cleans it up on exit.

    Usage::

        with _MockState(tmp_path) as state_dir:
            state = _load_state("session")
            ...
    """

    def __init__(self, state_dir: Path):
        self._state_dir = state_dir
        self._state_dir.mkdir(parents=True, exist_ok=True)
        self._old = os.environ.get("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR")

    def __enter__(self) -> Path:
        os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(self._state_dir)
        return self._state_dir

    def __exit__(self, *args: Any) -> None:
        if self._old is None:
            os.environ.pop("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", None)
        else:
            os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = self._old


def _setup_state(state_dir: Path, session_id: str, **overrides: Any) -> None:
    """Initialize mock state in the given directory. Requires env var to be set."""
    state = _load_state(session_id)
    state.update(overrides)
    state["session_id"] = session_id
    _save_state(state)


def _run_tool(
    name: str,
    payload: dict[str, Any],
    state_dir: Path,
    extra_env: dict[str, str] | None = None,
) -> dict[str, Any]:
    """Run a ui-control tool subprocess against the mock backend."""
    scripts = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "scripts"
    env = dict(os.environ)
    env["DCC_MCP_UI_CONTROL_BACKEND"] = "mock"
    env["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(state_dir)
    if extra_env:
        env.update(extra_env)
    python_path = str(REPO_ROOT / "python")
    if env.get("PYTHONPATH"):
        python_path = python_path + os.pathsep + env["PYTHONPATH"]
    env["PYTHONPATH"] = python_path
    result = subprocess.run(
        [sys.executable, str(scripts / f"{name}.py")],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip(), result.stderr
    return json.loads(result.stdout)


# ═══════════════════════════════════════════════════════════════════════════════
# 1. Mock backend scenario script tests
# ═══════════════════════════════════════════════════════════════════════════════


class TestScenarioScript:
    """Tests for ScenarioScript JSON definition, serialization, and loading."""

    def test_scenario_script_from_json_definition(self) -> None:
        """ScenarioScript can be constructed from a JSON-like dict."""
        script = ScenarioScript(
            name="test",
            description="A test scenario",
            steps=[
                ScenarioStep(tool="snapshot", label="initial", expect_success=True),
                ScenarioStep(
                    tool="act",
                    params={"control_id": "apply", "action": "click"},
                    label="click",
                    expect_success=True,
                ),
            ],
        )
        assert script.name == "test"
        assert len(script.steps) == 2
        assert script.steps[0].tool == "snapshot"

    def test_scenario_script_json_roundtrip(self) -> None:
        """ScenarioScript survives JSON serialization roundtrip."""
        original = ScenarioScript(
            name="roundtrip",
            description="Test roundtrip",
            steps=[
                ScenarioStep(tool="snapshot", label="s1", expect_success=True),
                ScenarioStep(
                    tool="act",
                    params={"control_id": "apply", "action": "click"},
                    label="click",
                    expect_success=True,
                ),
            ],
            metadata={"version": 1},
        )
        json_str = original.to_json()
        restored = ScenarioScript.from_json(json_str)
        assert restored.name == original.name
        assert restored.description == original.description
        assert len(restored.steps) == len(original.steps)
        assert restored.steps[0].tool == original.steps[0].tool
        assert restored.steps[1].params == original.steps[1].params
        assert restored.metadata == original.metadata

    def test_scenario_step_expect_error_code(self) -> None:
        """ScenarioStep can assert on error_code."""
        step = ScenarioStep(
            tool="act",
            params={"control_id": "missing", "action": "click"},
            expect_success=False,
            expect_error_code="not_found",
        )
        assert step.expect_error_code == "not_found"

    def test_scenario_step_expect_message_contains(self) -> None:
        """ScenarioStep can assert on message content."""
        step = ScenarioStep(
            tool="act",
            params={"control_id": "apply", "action": "click"},
            expect_message_contains="disabled by policy",
        )
        assert step.expect_message_contains == "disabled by policy"


class TestScenarioRunner:
    """Tests for ScenarioRunner executing scenarios against the mock backend."""

    def test_runner_executes_text_field_update_scenario(self, tmp_path: Path) -> None:
        """ScenarioRunner executes the text_field_update built-in scenario."""
        scenarios = builtin_scenarios()
        script = scenarios["text_field_update"]
        runner = ScenarioRunner(session_id="s-text-field", state_dir=tmp_path / "state")
        result = runner.run(script)
        runner.cleanup()
        assert result["passed"], f"Failed steps: {[s for s in result['step_results'] if not s['passed']]}"
        assert result["total"] == 5
        assert result["passed_count"] == 5

    def test_runner_executes_button_click_scenario(self, tmp_path: Path) -> None:
        """ScenarioRunner executes the button_click built-in scenario."""
        scenarios = builtin_scenarios()
        script = scenarios["button_click"]
        runner = ScenarioRunner(session_id="s-btn-click", state_dir=tmp_path / "state")
        result = runner.run(script)
        runner.cleanup()
        assert result["passed"], f"Failed steps: {[s for s in result['step_results'] if not s['passed']]}"
        assert result["total"] == 5
        assert result["passed_count"] == 5

    def test_runner_executes_checkbox_toggle_scenario(self, tmp_path: Path) -> None:
        """ScenarioRunner executes the checkbox_toggle built-in scenario."""
        scenarios = builtin_scenarios()
        script = scenarios["checkbox_toggle"]
        runner = ScenarioRunner(session_id="s-chk-toggle", state_dir=tmp_path / "state")
        result = runner.run(script)
        runner.cleanup()
        assert result["passed"], f"Failed steps: {[s for s in result['step_results'] if not s['passed']]}"
        assert result["total"] == 5
        assert result["passed_count"] == 5

    def test_runner_executes_policy_denied_scenario(self, tmp_path: Path) -> None:
        """ScenarioRunner executes the policy_denied built-in scenario."""
        scenarios = builtin_scenarios()
        script = scenarios["policy_denied"]
        runner = ScenarioRunner(session_id="s-policy-deny", state_dir=tmp_path / "state")
        result = runner.run(script)
        runner.cleanup()
        assert result["passed"], f"Failed steps: {[s for s in result['step_results'] if not s['passed']]}"
        assert result["total"] == 2
        assert result["passed_count"] == 2

    def test_runner_context_manager(self, tmp_path: Path) -> None:
        """ScenarioRunner works as a context manager."""
        script = ScenarioScript(
            name="ctx-test",
            steps=[ScenarioStep(tool="snapshot", label="s", expect_success=True)],
        )
        with ScenarioRunner(session_id="ctx-manager", state_dir=tmp_path / "ctx") as runner:
            result = runner.run(script)
        assert result["passed"]

    def test_runner_reports_failure_for_unexpected_success(self, tmp_path: Path) -> None:
        """ScenarioRunner reports failure when a step expected to fail succeeds."""
        script = ScenarioScript(
            name="expected-fail",
            steps=[
                ScenarioStep(
                    tool="act",
                    params={"control_id": "apply", "action": "click"},
                    expect_success=False,  # This will actually succeed
                ),
            ],
        )
        state_dir = tmp_path / "fail-state"
        runner = ScenarioRunner(session_id="fail-test", state_dir=state_dir)
        result = runner.run(script)
        runner.cleanup()
        assert not result["passed"]
        assert result["failed_count"] == 1


# ═══════════════════════════════════════════════════════════════════════════════
# 2. Snapshot diff tests
# ═══════════════════════════════════════════════════════════════════════════════


class TestSnapshotDiff:
    """Tests for diff_snapshots comparing UI states before/after actions."""

    def test_diff_detects_text_change(self, tmp_path: Path) -> None:
        """diff_snapshots detects text field value change."""
        session = "diff-text"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session, project_name="Before")

            before = _snapshot_dict(_load_state(session))
            state = _load_state(session)
            state["project_name"] = "After"
            state["revision"] = state["revision"] + 1
            _save_state(state)
            after = _snapshot_dict(_load_state(session))

        diffs = diff_snapshots(before, after)
        text_changes = [d for d in diffs if d["control_id"] == "project-name" and d["field"] == "text"]
        assert len(text_changes) == 1
        assert text_changes[0]["before"] == "Before"
        assert text_changes[0]["after"] == "After"
        assert text_changes[0]["change_type"] == "changed"

    def test_diff_detects_status_change(self, tmp_path: Path) -> None:
        """diff_snapshots detects status label text change."""
        session = "diff-status"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session, status="Idle")

            before = _snapshot_dict(_load_state(session))
            state = _load_state(session)
            state["status"] = "Applied"
            state["revision"] = state["revision"] + 1
            _save_state(state)
            after = _snapshot_dict(_load_state(session))

        diffs = diff_snapshots(before, after)
        status_changes = [d for d in diffs if d["control_id"] == "status" and d["field"] == "text"]
        assert len(status_changes) == 1
        assert status_changes[0]["before"] == "Idle"
        assert status_changes[0]["after"] == "Applied"

    def test_diff_detects_checkbox_toggle(self, tmp_path: Path) -> None:
        """diff_snapshots detects checkbox checked state toggle."""
        session = "diff-checkbox"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session, cache_enabled=False)

            before = _snapshot_dict(_load_state(session))
            state = _load_state(session)
            state["cache_enabled"] = True
            state["revision"] = state["revision"] + 1
            _save_state(state)
            after = _snapshot_dict(_load_state(session))

        diffs = diff_snapshots(before, after)
        checkbox_changes = [d for d in diffs if d["control_id"] == "enable-cache" and d["field"] == "checked"]
        assert len(checkbox_changes) == 1
        assert checkbox_changes[0]["before"] is False
        assert checkbox_changes[0]["after"] is True

    def test_diff_returns_empty_for_identical_snapshots(self, tmp_path: Path) -> None:
        """diff_snapshots returns empty list for identical snapshots."""
        session = "diff-identical"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session)

            snapshot = _snapshot_dict(_load_state(session))

        diffs = diff_snapshots(snapshot, snapshot)
        assert diffs == []

    def test_diff_detects_focus_id_change(self, tmp_path: Path) -> None:
        """diff_snapshots detects focus_id change at snapshot level."""
        session = "diff-focus"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session, focus_id="project-name")

            before = _snapshot_dict(_load_state(session))
            state = _load_state(session)
            state["focus_id"] = "apply"
            state["revision"] = state["revision"] + 1
            _save_state(state)
            after = _snapshot_dict(_load_state(session))

        diffs = diff_snapshots(before, after)
        focus_changes = [d for d in diffs if d["field"] == "focus_id"]
        assert len(focus_changes) == 1

    def test_diff_handles_missing_node(self, tmp_path: Path) -> None:
        """diff_snapshots reports an added node when one snapshot has fewer nodes."""
        session = "diff-add"
        with _MockState(tmp_path / "state") as state_dir:
            _setup_state(state_dir, session)

            before = _snapshot_dict(_load_state(session))
            # Create a snapshot with an extra node by manipulating the dict directly
            after = _snapshot_dict(_load_state(session))
            extra_node = {
                "id": "extra-node",
                "role": "button",
                "enabled": True,
                "visible": True,
            }
            after["root"]["children"].append(extra_node)
            after["node_count"] = after["node_count"] + 1

        diffs = diff_snapshots(before, after)
        added = [d for d in diffs if d["change_type"] == "added"]
        assert len(added) == 1


# ═══════════════════════════════════════════════════════════════════════════════
# 3. Action replay tests
# ═══════════════════════════════════════════════════════════════════════════════


class TestActionRecorderReplayer:
    """Tests for ActionRecorder, ActionReplayer, and ReplayTrace."""

    def test_recorder_records_snapshot_find_act_sequence(self, tmp_path: Path) -> None:
        """ActionRecorder records a full snapshot → find → act sequence."""
        state_dir = tmp_path / "recorder-state"
        state_dir.mkdir()
        recorder = ActionRecorder(session_id="rec-seq", state_dir=state_dir)
        snapshot = recorder.snapshot()
        assert snapshot["success"] is True
        found = recorder.find(label="Project name")
        assert found["success"] is True
        acted = recorder.act(control_id="project-name", action=UiActionKind.SET_TEXT, text="Recorded")
        assert acted["success"] is True
        trace = recorder.finish()
        assert len(trace.steps) == 3
        assert trace.steps[0]["tool"] == "snapshot"
        assert trace.steps[1]["tool"] == "find"
        assert trace.steps[2]["tool"] == "act"

    def test_replayer_replays_trace_deterministically(self, tmp_path: Path) -> None:
        """ActionReplayer replays a recorded trace with matching results."""
        state_dir = tmp_path / "replay-state"
        state_dir.mkdir()
        recorder = ActionRecorder(session_id="replay-det", state_dir=state_dir)
        recorder.snapshot()
        recorder.act(control_id="project-name", action=UiActionKind.SET_TEXT, text="Replay")
        trace = recorder.finish()

        replay_dir = tmp_path / "replay-replay"
        replay_dir.mkdir()
        replayer = ActionReplayer(state_dir=replay_dir)
        result = replayer.replay(trace)
        replayer.cleanup()
        assert result["passed"], f"Replay failed: {result['step_results']}"
        assert result["total"] == 2
        assert result["passed_count"] == 2

    def test_replay_trace_json_roundtrip(self, tmp_path: Path) -> None:
        """ReplayTrace survives JSON serialization roundtrip."""
        trace = ReplayTrace(
            trace_id="test-trace",
            session_id="roundtrip",
            steps=[
                {
                    "index": 1,
                    "tool": "snapshot",
                    "params": {"session_id": "roundtrip"},
                    "result_success": True,
                    "result_error": None,
                    "result_message": "Captured mock ui_control snapshot.",
                },
            ],
            metadata={"version": 1},
        )
        path = tmp_path / "trace.json"
        trace.to_json(path)
        assert path.exists()
        restored = ReplayTrace.from_file(path)
        assert restored.trace_id == trace.trace_id
        assert restored.session_id == trace.session_id
        assert len(restored.steps) == 1
        assert restored.steps[0]["tool"] == "snapshot"
        assert restored.metadata == trace.metadata

    def test_file_loaded_trace_replays_correctly(self, tmp_path: Path) -> None:
        """A ReplayTrace loaded from a file replays deterministically."""
        state_dir = tmp_path / "file-state"
        state_dir.mkdir()
        recorder = ActionRecorder(session_id="file-replay", state_dir=state_dir)
        recorder.snapshot()
        recorder.act(control_id="apply", action=UiActionKind.CLICK)
        trace = recorder.finish()

        trace_path = tmp_path / "file-trace.json"
        trace.to_json(trace_path)

        loaded = ReplayTrace.from_file(trace_path)
        assert loaded.trace_id == trace.trace_id

        replay_dir = tmp_path / "file-replay"
        replay_dir.mkdir()
        replayer = ActionReplayer(state_dir=replay_dir)
        result = replayer.replay(loaded)
        replayer.cleanup()
        assert result["passed"]

    def test_replay_with_mismatched_trace_reports_failure(self, tmp_path: Path) -> None:
        """Replaying a trace with altered expected success fails."""
        trace = ReplayTrace(
            trace_id="mismatch",
            session_id="bad",
            steps=[
                {
                    "index": 1,
                    "tool": "snapshot",
                    "params": {"session_id": "bad"},
                    "result_success": False,  # Wrong expectation
                    "result_error": None,
                    "result_message": "should not match",
                },
            ],
        )
        replay_dir = tmp_path / "mismatch-state"
        replay_dir.mkdir()
        replayer = ActionReplayer(state_dir=replay_dir)
        result = replayer.replay(trace)
        replayer.cleanup()
        assert not result["passed"]
        assert result["failed_count"] == 1

    def test_traces_from_different_sessions_are_independent(self, tmp_path: Path) -> None:
        """Traces recorded in different sessions replay independently."""
        # Session A
        state_a = tmp_path / "state-a"
        state_a.mkdir()
        recorder_a = ActionRecorder(session_id="session-a", state_dir=state_a)
        recorder_a.snapshot()
        recorder_a.act(control_id="project-name", action=UiActionKind.SET_TEXT, text="A")
        trace_a = recorder_a.finish()

        # Session B
        state_b = tmp_path / "state-b"
        state_b.mkdir()
        recorder_b = ActionRecorder(session_id="session-b", state_dir=state_b)
        recorder_b.snapshot()
        recorder_b.act(control_id="project-name", action=UiActionKind.SET_TEXT, text="B")
        trace_b = recorder_b.finish()

        assert trace_a.steps != trace_b.steps  # Different params
        assert len(trace_a.steps) == len(trace_b.steps)

        # Replay A
        replay_a = tmp_path / "replay-a"
        replay_a.mkdir()
        replayer_a = ActionReplayer(state_dir=replay_a)
        result_a = replayer_a.replay(trace_a)
        replayer_a.cleanup()
        assert result_a["passed"]

        # Replay B
        replay_b = tmp_path / "replay-b"
        replay_b.mkdir()
        replayer_b = ActionReplayer(state_dir=replay_b)
        result_b = replayer_b.replay(trace_b)
        replayer_b.cleanup()
        assert result_b["passed"]


# ═══════════════════════════════════════════════════════════════════════════════
# 4. Policy matrix tests
# ═══════════════════════════════════════════════════════════════════════════════


class TestPolicyFlagCoverage:
    """Tests that every UiControlPolicy flag has corresponding test coverage."""

    def test_all_boolean_policy_flags_are_listed(self) -> None:
        """POLICY_BOOLEAN_FLAGS covers every boolean UiControlPolicy field."""
        from dataclasses import fields

        expected = {f.name for f in fields(UiControlPolicy) if f.type in (bool, "bool")}
        covered = set(POLICY_BOOLEAN_FLAGS)
        missing = expected - covered
        assert not missing, f"Missing policy flags from POLICY_BOOLEAN_FLAGS: {missing}"

    def test_check_policy_flag_coverage_returns_all_covered(self) -> None:
        """check_policy_flag_coverage returns True for all flags."""
        coverage = check_policy_flag_coverage()
        assert len(coverage) == len(POLICY_BOOLEAN_FLAGS)
        assert all(coverage.values())


class TestPolicyMatrix:
    """Parametrized tests verifying each policy flag blocks its corresponding action."""

    def test_allow_snapshot_denies_snapshot_when_disabled(self, tmp_path: Path) -> None:
        """Snapshot is denied when allow_snapshot=False."""
        session = "policy-snapshot"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        result = _run_tool(
            "snapshot",
            {"session_id": session, "policy": {"allow_snapshot": False}},
            state_dir,
        )
        assert result["success"] is False
        assert "disabled by policy" in result["message"]

    def test_allow_find_denies_find_when_disabled(self, tmp_path: Path) -> None:
        """Find is denied when allow_find=False."""
        session = "policy-find"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        result = _run_tool(
            "find",
            {"session_id": session, "label": "Apply", "policy": {"allow_find": False}},
            state_dir,
        )
        assert result["success"] is False
        assert "disabled by policy" in result["message"]

    def test_allow_mutating_actions_denies_click_when_disabled(self, tmp_path: Path) -> None:
        """Click is denied when allow_mutating_actions=False."""
        session = "policy-mutating"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        snapshot = _run_tool("snapshot", {"session_id": session}, state_dir)
        result = _run_tool(
            "act",
            {
                "session_id": session,
                "control_id": "apply",
                "action": UiActionKind.CLICK,
                "snapshot_id": snapshot["context"]["snapshot_id"],
                "policy": {"allow_mutating_actions": False},
            },
            state_dir,
        )
        assert result["success"] is False
        assert result["context"]["result"]["error_code"] == "policy_disabled"

    def test_allow_text_entry_denies_set_text_when_disabled(self, tmp_path: Path) -> None:
        """set_text is denied when allow_text_entry=False."""
        session = "policy-text"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        snapshot = _run_tool("snapshot", {"session_id": session}, state_dir)
        result = _run_tool(
            "act",
            {
                "session_id": session,
                "control_id": "project-name",
                "action": UiActionKind.SET_TEXT,
                "text": "Secret",
                "snapshot_id": snapshot["context"]["snapshot_id"],
                "policy": {"allow_text_entry": False},
            },
            state_dir,
        )
        assert result["success"] is False
        assert result["context"]["result"]["error_code"] == "policy_disabled"

    def test_allow_keyboard_shortcuts_denies_keyboard_shortcut_when_disabled(self, tmp_path: Path) -> None:
        """keyboard_shortcut is denied when allow_keyboard_shortcuts=False."""
        session = "policy-kb"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        snapshot = _run_tool("snapshot", {"session_id": session}, state_dir)
        result = _run_tool(
            "act",
            {
                "session_id": session,
                "control_id": "apply",
                "action": UiActionKind.KEYBOARD_SHORTCUT,
                "keys": ["Ctrl", "S"],
                "snapshot_id": snapshot["context"]["snapshot_id"],
                "policy": {"allow_keyboard_shortcuts": False},
            },
            state_dir,
        )
        assert result["success"] is False
        assert "disabled by policy" in result["message"]

    def test_allow_raw_coordinates_denies_raw_coordinate_click_when_disabled(self, tmp_path: Path) -> None:
        """raw_coordinate_click is denied when allow_raw_coordinates=False."""
        session = "policy-raw"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session)
        snapshot = _run_tool("snapshot", {"session_id": session}, state_dir)
        result = _run_tool(
            "act",
            {
                "session_id": session,
                "control_id": "mock-window",
                "action": UiActionKind.RAW_COORDINATE_CLICK,
                "x": 10,
                "y": 10,
                "snapshot_id": snapshot["context"]["snapshot_id"],
                "policy": {"allow_raw_coordinates": False},
            },
            state_dir,
        )
        assert result["success"] is False
        assert "disabled by policy" in result["message"]

    def test_allowed_window_titles_blocks_unmatched_title(self, tmp_path: Path) -> None:
        """Operations are denied when window title doesn't match allowed list."""
        session = "policy-title"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session, window_title="DCC Mock Settings")
        result = _run_tool(
            "snapshot",
            {
                "session_id": session,
                "policy": {"allowed_window_titles": ["Other App"]},
            },
            state_dir,
        )
        assert result["success"] is False
        assert "not allowed by policy" in result["message"]

    def test_allowed_process_ids_blocks_unmatched_pid(self, tmp_path: Path) -> None:
        """Operations are denied when process ID doesn't match allowed list."""
        session = "policy-pid"
        state_dir = tmp_path / "state"
        state_dir.mkdir()
        _setup_state(state_dir, session, process_id=0)
        result = _run_tool(
            "snapshot",
            {
                "session_id": session,
                "policy": {"allowed_process_ids": [9999]},
            },
            state_dir,
        )
        assert result["success"] is False
        assert "not allowed by policy" in result["message"]

    def test_scope_denied_blocks_all_actions(self) -> None:
        """All actions are blocked when scope_denied=True (unit test on policy)."""
        policy = UiControlPolicy(scope_denied=True)
        assert not policy.allows_action(UiActionKind.CLICK)
        assert not policy.allows_action(UiActionKind.SET_TEXT)
        assert not policy.allows_action(UiActionKind.FOCUS)
        assert not policy.allows_action(UiActionKind.KEYBOARD_SHORTCUT)
        # Read-only actions are also blocked
        assert not policy.allows_action(UiActionKind.GET_WINDOW_STATE)


class TestPolicyBuildMatrix:
    """Tests for the build_policy_test_cases helper."""

    def test_build_policy_test_cases_returns_all_entries(self) -> None:
        """build_policy_test_cases returns test cases for all key policy flags."""
        cases = build_policy_test_cases()
        case_flags = {c[0] for c in cases}
        # Should cover the main behavioral flags
        assert "allow_snapshot" in case_flags
        assert "allow_find" in case_flags
        assert "allow_mutating_actions" in case_flags
        assert "allow_text_entry" in case_flags
        assert "allow_keyboard_shortcuts" in case_flags
        assert "allow_raw_coordinates" in case_flags
        assert "allowed_window_titles" in case_flags
        assert "allowed_process_ids" in case_flags
        assert "scope_denied" in case_flags

    def test_each_policy_case_has_required_fields(self) -> None:
        """Each policy test case tuple has all required fields."""
        cases = build_policy_test_cases()
        for case in cases:
            assert len(case) == 5
            flag_name, policy_overrides, tool, _action, extra_params = case
            assert isinstance(flag_name, str)
            assert isinstance(policy_overrides, dict)
            assert isinstance(tool, str)
            assert isinstance(extra_params, dict)


# ═══════════════════════════════════════════════════════════════════════════════
# 5. CI integration tests
# ═══════════════════════════════════════════════════════════════════════════════


class TestCiIntegration:
    """Tests verifying CI justfile targets and infrastructure."""

    def test_justfile_exists(self) -> None:
        """Justfile exists in the repository root."""
        assert _JUSTFILE.exists(), f"justfile not found at {_JUSTFILE}"

    def test_justfile_has_test_target(self) -> None:
        """Justfile has the 'test' target that runs pytest tests/."""
        content = _JUSTFILE.read_text(encoding="utf-8")
        assert "pytest tests/" in content

    def test_justfile_has_test_suite_target(self) -> None:
        """Justfile has the 'test-suite' target for CI."""
        content = _JUSTFILE.read_text(encoding="utf-8")
        assert "test-suite" in content
        assert "--dist loadfile" in content

    def test_all_new_tests_use_deterministic_mock_backend(self) -> None:
        """No test in this module sets the backend to windows-uia or chrome."""
        source = Path(__file__).read_text(encoding="utf-8")
        # Verify no subprocess test sets the backend to a real desktop backend
        lines_with_backend = [line for line in source.splitlines() if "DCC_MCP_UI_CONTROL_BACKEND" in line]
        for line in lines_with_backend:
            assert "windows-uia" not in line, f"Found windows-uia backend: {line.strip()}"
            assert "chrome" not in line, f"Found chrome backend: {line.strip()}"

    def test_existing_ui_control_tests_still_discoverable(self) -> None:
        """Existing UI Control test files still exist and are discoverable."""
        existing_tests = list((REPO_ROOT / "tests").glob("test_ui_control_*.py"))
        assert len(existing_tests) >= 4  # At least 4 existing test files
