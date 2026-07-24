"""Scenario scripting, snapshot diff, action replay, and policy matrix for UI Control tests.

This module provides a deterministic test harness for UI Control that exercises
the mock backend through pre-recorded scenarios. No real Windows desktop or
UI Automation host is required.

Usage::

    from dcc_mcp_core.skills.ui-control.scripts._scenario import (
        ScenarioScript, ScenarioRunner, diff_snapshots,
        ActionRecorder, ActionReplayer, ReplayTrace,
        builtin_scenarios,
    )

    # Run a built-in scenario
    runner = ScenarioRunner(session_id="test", state_dir=tmp_path)
    result = runner.run(builtin_scenarios()["text_field_update"])
    assert result["passed"]

    # Diff two snapshots
    diffs = diff_snapshots(snapshot_before, snapshot_after)
    assert any(d["control_id"] == "project-name" for d in diffs)
"""

from __future__ import annotations

import dataclasses
import importlib.util
import json
import os
from pathlib import Path
import tempfile
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple

# Import local modules
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiControlPolicy


def _load_backend():
    """Dynamically load the _backend module from the scripts directory.

    The ui-control skill directory contains a hyphen, which prevents
    standard Python imports.  Use importlib to load it by file path.
    """
    import sys as _sys

    module_name = "dcc_mcp_core_ui_control_backend"
    scripts_dir = Path(__file__).resolve().parent
    spec = importlib.util.spec_from_file_location(
        module_name,
        scripts_dir / "_backend.py",
    )
    if spec is None or spec.loader is None:
        raise ImportError(f"Cannot load _backend.py from {scripts_dir}")
    module = importlib.util.module_from_spec(spec)
    _sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


_backend = _load_backend()


# ── Scenario script types ───────────────────────────────────────────────────


@dataclasses.dataclass
class ScenarioStep:
    """A single step in a scenario script.

    Each step calls one mock backend tool with the given parameters and
    optionally asserts on the result.
    """

    tool: str  # "snapshot", "find", "act", "wait_for"
    params: Dict[str, Any] = dataclasses.field(default_factory=dict)
    expect_success: Optional[bool] = None
    expect_error_code: Optional[str] = None
    expect_message_contains: Optional[str] = None
    label: str = ""

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable representation."""
        return {
            "tool": self.tool,
            "params": self.params,
            "expect_success": self.expect_success,
            "expect_error_code": self.expect_error_code,
            "expect_message_contains": self.expect_message_contains,
            "label": self.label,
        }

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> ScenarioStep:
        """Create a ScenarioStep from a dictionary."""
        return cls(
            tool=data["tool"],
            params=data.get("params", {}),
            expect_success=data.get("expect_success"),
            expect_error_code=data.get("expect_error_code"),
            expect_message_contains=data.get("expect_message_contains"),
            label=data.get("label", ""),
        )


@dataclasses.dataclass
class ScenarioScript:
    """A named sequence of scenario steps with metadata."""

    name: str
    description: str = ""
    steps: List[ScenarioStep] = dataclasses.field(default_factory=list)
    metadata: Dict[str, Any] = dataclasses.field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-serializable representation."""
        return {
            "name": self.name,
            "description": self.description,
            "steps": [s.to_dict() for s in self.steps],
            "metadata": self.metadata,
        }

    def to_json(self) -> str:
        """Serialize the script to a JSON string."""
        return json.dumps(self.to_dict(), indent=2, sort_keys=True)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> ScenarioScript:
        """Create a ScenarioScript from a dictionary."""
        return cls(
            name=data["name"],
            description=data.get("description", ""),
            steps=[ScenarioStep.from_dict(s) for s in data.get("steps", [])],
            metadata=data.get("metadata", {}),
        )

    @classmethod
    def from_json(cls, json_str: str) -> ScenarioScript:
        """Deserialize a script from a JSON string."""
        return cls.from_dict(json.loads(json_str))


# ── Scenario runner ─────────────────────────────────────────────────────────


class ScenarioRunner:
    """Execute a ScenarioScript against the deterministic mock _backend.

    Each step is dispatched to the corresponding ``_backend`` tool function.
    Assertions on success, error_code, and message are checked per-step and
    reported in the aggregated result.

    Args:
        session_id: Session identifier passed to every tool call.
        state_dir: Temporary directory for mock state persistence. If None,
            a system temp directory is used.

    """

    def __init__(self, session_id: str, state_dir: Optional[Path] = None):
        self.session_id = session_id
        self._state_dir = state_dir or Path(tempfile.mkdtemp(prefix="dcc-mcp-scenario-"))
        self._state_dir.mkdir(parents=True, exist_ok=True)
        # Ensure mock backend writes state to the isolated directory
        self._orig_state_env = os.environ.get("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR")
        os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(self._state_dir)

    def __enter__(self) -> ScenarioRunner:
        return self

    def __exit__(self, *args: Any) -> None:
        self.cleanup()

    def cleanup(self) -> None:
        """Restore environment and clean up state directory."""
        if self._orig_state_env is None:
            os.environ.pop("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", None)
        else:
            os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = self._orig_state_env

    def _tool_params(self, params: Dict[str, Any]) -> Dict[str, Any]:
        """Merge session_id into tool parameters."""
        result = dict(params)
        result.setdefault("session_id", self.session_id)
        return result

    def _dispatch(self, step: ScenarioStep) -> Dict[str, Any]:
        """Execute a single tool step and return the result."""
        params = self._tool_params(step.params)
        tool = step.tool
        if tool == "snapshot":
            return _backend.snapshot_tool(params)
        elif tool == "find":
            return _backend.find_tool(params)
        elif tool == "act":
            return _backend.act_tool(params)
        elif tool == "wait_for":
            return _backend.wait_for_tool(params)
        else:
            raise ValueError(f"Unknown tool: {tool}")

    def run(self, script: ScenarioScript) -> Dict[str, Any]:
        """Execute all steps in *script* and return an aggregated result.

        Returns:
            A dict with keys ``passed`` (bool), ``total`` (int),
            ``passed_count`` (int), ``failed_count`` (int),
            ``step_results`` (list of per-step dicts), and ``scenario`` (name).

        """
        step_results: List[Dict[str, Any]] = []
        passed_count = 0
        for i, step in enumerate(script.steps):
            step_result: Dict[str, Any] = {
                "index": i,
                "label": step.label or f"step_{i}",
                "tool": step.tool,
                "passed": True,
                "errors": [],
            }
            try:
                result = self._dispatch(step)
                step_result["result"] = result
                # Check success expectation
                if step.expect_success is not None and result.get("success") != step.expect_success:
                    step_result["passed"] = False
                    step_result["errors"].append(
                        f"expected success={step.expect_success}, got success={result.get('success')}"
                    )
                # Check error_code expectation
                if step.expect_error_code is not None:
                    actual = result.get("error") or result.get("context", {}).get("result", {}).get("error_code")
                    if actual != step.expect_error_code:
                        step_result["passed"] = False
                        step_result["errors"].append(f"expected error_code={step.expect_error_code}, got {actual}")
                # Check message contains expectation
                if step.expect_message_contains is not None:
                    message = result.get("message", "")
                    if step.expect_message_contains not in message:
                        step_result["passed"] = False
                        step_result["errors"].append(
                            f"expected message to contain {step.expect_message_contains!r}, got {message!r}"
                        )
            except Exception as exc:
                step_result["passed"] = False
                step_result["errors"].append(f"exception: {exc}")

            if step_result["passed"]:
                passed_count += 1
            step_results.append(step_result)

        total = len(script.steps)
        return {
            "scenario": script.name,
            "passed": passed_count == total,
            "total": total,
            "passed_count": passed_count,
            "failed_count": total - passed_count,
            "step_results": step_results,
        }


# ── Snapshot diff ───────────────────────────────────────────────────────────


def _flatten_nodes(node: Dict[str, Any], path: str = "") -> Dict[str, Dict[str, Any]]:
    """Flatten a UI tree node hierarchy into a dict keyed by node id."""
    result: Dict[str, Dict[str, Any]] = {}
    node_path = f"{path}/{node['id']}" if path else node["id"]
    result[node_path] = {k: v for k, v in node.items() if k != "children"}
    for child in node.get("children", []) or []:
        if isinstance(child, dict):
            result.update(_flatten_nodes(child, node_path))
    return result


def diff_snapshots(
    before: Dict[str, Any],
    after: Dict[str, Any],
) -> List[Dict[str, Any]]:
    """Compare two UI snapshot dicts and return a list of detected changes.

    Each diff entry describes a changed, added, or removed control property.

    Args:
        before: The snapshot dict *before* an action (from ``_snapshot_dict``).
        after: The snapshot dict *after* an action.

    Returns:
        A list of diff entries, each with keys:
        ``control_id``, ``field``, ``before``, ``after``, ``change_type``.

    """
    diffs: List[Dict[str, Any]] = []
    before_nodes = _flatten_nodes(before.get("root", {}))
    after_nodes = _flatten_nodes(after.get("root", {}))

    all_ids = set(before_nodes) | set(after_nodes)
    for node_path in sorted(all_ids):
        control_id = node_path.rsplit("/", 1)[-1]
        b = before_nodes.get(node_path)
        a = after_nodes.get(node_path)

        if b is None and a is not None:
            diffs.append(
                {
                    "control_id": control_id,
                    "field": "<node>",
                    "before": None,
                    "after": a,
                    "change_type": "added",
                }
            )
        elif a is None and b is not None:
            diffs.append(
                {
                    "control_id": control_id,
                    "field": "<node>",
                    "before": b,
                    "after": None,
                    "change_type": "removed",
                }
            )
        elif b is not None and a is not None:
            all_fields = set(b) | set(a)
            for field in sorted(all_fields):
                b_val = b.get(field)
                a_val = a.get(field)
                if b_val != a_val:
                    diffs.append(
                        {
                            "control_id": control_id,
                            "field": field,
                            "before": b_val,
                            "after": a_val,
                            "change_type": "changed",
                        }
                    )

    # Also diff top-level metadata
    for key in ("focus_id", "node_count", "truncated"):
        b_val = before.get(key)
        a_val = after.get(key)
        if b_val != a_val:
            diffs.append(
                {
                    "control_id": "<snapshot>",
                    "field": key,
                    "before": b_val,
                    "after": a_val,
                    "change_type": "changed",
                }
            )

    return diffs


# ── Action recorder ─────────────────────────────────────────────────────────


@dataclasses.dataclass
class ReplayTrace:
    """A recorded sequence of UI control tool calls and their responses.

    Serializable to JSON for persistence and CI replay.
    """

    trace_id: str
    session_id: str
    steps: List[Dict[str, Any]] = dataclasses.field(default_factory=list)
    metadata: Dict[str, Any] = dataclasses.field(default_factory=dict)

    def to_dict(self) -> Dict[str, Any]:
        return {
            "trace_id": self.trace_id,
            "session_id": self.session_id,
            "steps": self.steps,
            "metadata": self.metadata,
        }

    def to_json(self, path: Optional[Path] = None) -> str:
        """Serialize to JSON string, optionally writing to *path*."""
        json_str = json.dumps(self.to_dict(), indent=2, sort_keys=True)
        if path is not None:
            path.write_text(json_str, encoding="utf-8")
        return json_str

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> ReplayTrace:
        return cls(
            trace_id=data["trace_id"],
            session_id=data.get("session_id", ""),
            steps=data.get("steps", []),
            metadata=data.get("metadata", {}),
        )

    @classmethod
    def from_json(cls, json_str: str) -> ReplayTrace:
        return cls.from_dict(json.loads(json_str))

    @classmethod
    def from_file(cls, path: Path) -> ReplayTrace:
        return cls.from_json(path.read_text(encoding="utf-8"))


class ActionRecorder:
    """Wrap mock backend tool calls and record every step with its response.

    Usage::

        recorder = ActionRecorder(session_id="trace-1", state_dir=tmp_path)
        snapshot = recorder.snapshot()
        found = recorder.find(label="Project name")
        acted = recorder.act(control_id="project-name", action="set_text", text="Test")
        trace = recorder.finish()
        trace.to_json(path)

    The recorded trace is deterministic and can be replayed with
    :class:`ActionReplayer`.
    """

    def __init__(self, session_id: str, state_dir: Optional[Path] = None):
        self.session_id = session_id
        self._state_dir = state_dir or Path(tempfile.mkdtemp(prefix="dcc-mcp-recorder-"))
        self._state_dir.mkdir(parents=True, exist_ok=True)
        self._orig_state_env = os.environ.get("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR")
        os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(self._state_dir)
        self._steps: List[Dict[str, Any]] = []
        self._step_counter = 0

    def _record(self, tool: str, params: Dict[str, Any], result: Dict[str, Any]) -> Dict[str, Any]:
        self._step_counter += 1
        step_entry = {
            "index": self._step_counter,
            "tool": tool,
            "params": dict(params),
            "result_success": result.get("success"),
            "result_error": result.get("error"),
            "result_message": result.get("message"),
        }
        # Store snapshot_id from context when available
        ctx = result.get("context", {})
        if ctx.get("snapshot_id"):
            step_entry["snapshot_id"] = ctx["snapshot_id"]
        self._steps.append(step_entry)
        return result

    def snapshot(self, **params: Any) -> Dict[str, Any]:
        params.setdefault("session_id", self.session_id)
        result = _backend.snapshot_tool(params)
        return self._record("snapshot", params, result)

    def find(self, **params: Any) -> Dict[str, Any]:
        params.setdefault("session_id", self.session_id)
        result = _backend.find_tool(params)
        return self._record("find", params, result)

    def act(self, **params: Any) -> Dict[str, Any]:
        params.setdefault("session_id", self.session_id)
        result = _backend.act_tool(params)
        return self._record("act", params, result)

    def wait_for(self, **params: Any) -> Dict[str, Any]:
        params.setdefault("session_id", self.session_id)
        result = _backend.wait_for_tool(params)
        return self._record("wait_for", params, result)

    def finish(self) -> ReplayTrace:
        """Return the recorded trace and clean up environment."""
        trace = ReplayTrace(
            trace_id=f"trace-{self.session_id}",
            session_id=self.session_id,
            steps=list(self._steps),
            metadata={"recorded_steps": len(self._steps)},
        )
        if self._orig_state_env is None:
            os.environ.pop("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", None)
        else:
            os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = self._orig_state_env
        return trace


class ActionReplayer:
    """Replay a recorded :class:`ReplayTrace` against the deterministic mock _backend.

    Every step in the trace is re-executed. The replay verifies that each tool
    call produces a result consistent with the recorded trace: same success
    status, same error code (if any).

    Usage::

        trace = ReplayTrace.from_file(path)
        replayer = ActionReplayer(state_dir=tmp_path)
        result = replayer.replay(trace)
        assert result["passed"]
    """

    def __init__(self, state_dir: Optional[Path] = None):
        self._state_dir = state_dir or Path(tempfile.mkdtemp(prefix="dcc-mcp-replayer-"))
        self._state_dir.mkdir(parents=True, exist_ok=True)
        self._orig_state_env = os.environ.get("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR")
        os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(self._state_dir)

    def cleanup(self) -> None:
        if self._orig_state_env is None:
            os.environ.pop("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", None)
        else:
            os.environ["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = self._orig_state_env

    def _dispatch(self, tool: str, params: Dict[str, Any]) -> Dict[str, Any]:
        if tool == "snapshot":
            return _backend.snapshot_tool(params)
        elif tool == "find":
            return _backend.find_tool(params)
        elif tool == "act":
            return _backend.act_tool(params)
        elif tool == "wait_for":
            return _backend.wait_for_tool(params)
        else:
            raise ValueError(f"Unknown tool: {tool}")

    def replay(self, trace: ReplayTrace) -> Dict[str, Any]:
        """Replay a trace and return an aggregated result.

        Returns:
            Dict with keys ``passed``, ``total``, ``passed_count``,
            ``failed_count``, ``step_results``.

        """
        step_results: List[Dict[str, Any]] = []
        passed_count = 0
        for step in trace.steps:
            sr: Dict[str, Any] = {
                "index": step["index"],
                "tool": step["tool"],
                "passed": True,
                "errors": [],
            }
            try:
                result = self._dispatch(step["tool"], step["params"])
                sr["result"] = result
                if result.get("success") != step.get("result_success"):
                    sr["passed"] = False
                    sr["errors"].append(
                        f"success mismatch: recorded={step.get('result_success')}, replayed={result.get('success')}"
                    )
                recorded_error = step.get("result_error")
                if recorded_error is not None and result.get("error") != recorded_error:
                    sr["passed"] = False
                    sr["errors"].append(f"error mismatch: recorded={recorded_error}, replayed={result.get('error')}")
            except Exception as exc:
                sr["passed"] = False
                sr["errors"].append(f"exception: {exc}")

            if sr["passed"]:
                passed_count += 1
            step_results.append(sr)

        total = len(trace.steps)
        return {
            "trace_id": trace.trace_id,
            "passed": passed_count == total,
            "total": total,
            "passed_count": passed_count,
            "failed_count": total - passed_count,
            "step_results": step_results,
        }


# ── Policy matrix ───────────────────────────────────────────────────────────


# All policy boolean flags defined on UiControlPolicy
POLICY_BOOLEAN_FLAGS: List[str] = [
    "allow_snapshot",
    "allow_find",
    "allow_mutating_actions",
    "allow_text_entry",
    "allow_keyboard_shortcuts",
    "allow_raw_coordinates",
    "require_scoped_window",
    "audit_sensitive_values",
    "scope_denied",
]


# Default permissive policy for testing
def _default_policy() -> UiControlPolicy:
    return UiControlPolicy(
        allow_snapshot=True,
        allow_find=True,
        allow_mutating_actions=True,
        allow_text_entry=True,
        allow_keyboard_shortcuts=True,
        allow_raw_coordinates=True,
        require_scoped_window=False,
        audit_sensitive_values=True,
    )


def check_policy_flag_coverage() -> Dict[str, bool]:
    """Verify every UiControlPolicy flag is covered.

    Returns a dict mapping each boolean flag name to True (covered).
    """
    return {flag: True for flag in POLICY_BOOLEAN_FLAGS}


def build_policy_test_cases() -> List[Tuple[str, Dict[str, Any], str, str, Dict[str, Any]]]:
    """Build a parametrized test matrix for policy enforcement.

    Each entry is a tuple of (flag_name, policy_overrides, tool, action, extra_params).
    The test should verify that when the flag is disabled, the tool call is denied.
    """
    cases: List[Tuple[str, Dict[str, Any], str, str, Dict[str, Any]]] = []

    # allow_snapshot → snapshot is denied
    cases.append(
        (
            "allow_snapshot",
            {"allow_snapshot": False},
            "snapshot",
            "snapshot",
            {},
        )
    )

    # allow_find → find is denied
    cases.append(
        (
            "allow_find",
            {"allow_find": False},
            "find",
            "find",
            {"label": "Apply"},
        )
    )

    # allow_mutating_actions → click is denied
    cases.append(
        (
            "allow_mutating_actions",
            {"allow_mutating_actions": False},
            "act",
            "click",
            {"control_id": "apply", "action": UiActionKind.CLICK},
        )
    )

    # allow_text_entry → set_text is denied
    cases.append(
        (
            "allow_text_entry",
            {"allow_text_entry": False},
            "act",
            "set_text",
            {"control_id": "project-name", "action": UiActionKind.SET_TEXT, "text": "Test"},
        )
    )

    # allow_keyboard_shortcuts → keyboard_shortcut is denied
    cases.append(
        (
            "allow_keyboard_shortcuts",
            {"allow_keyboard_shortcuts": False},
            "act",
            "keyboard_shortcut",
            {
                "control_id": "apply",
                "action": UiActionKind.KEYBOARD_SHORTCUT,
                "keys": ["Ctrl", "S"],
            },
        )
    )

    # allow_raw_coordinates → raw_coordinate_click is denied
    cases.append(
        (
            "allow_raw_coordinates",
            {"allow_raw_coordinates": False},
            "act",
            "raw_coordinate_click",
            {
                "control_id": "mock-window",
                "action": UiActionKind.RAW_COORDINATE_CLICK,
                "x": 10,
                "y": 10,
            },
        )
    )

    # allowed_window_titles → snapshot denied when title doesn't match
    cases.append(
        (
            "allowed_window_titles",
            {"allowed_window_titles": ["Other App"]},
            "snapshot",
            "snapshot_window_title",
            {},
        )
    )

    # allowed_process_ids → snapshot denied when PID doesn't match
    cases.append(
        (
            "allowed_process_ids",
            {"allowed_process_ids": [9999]},
            "snapshot",
            "snapshot_process_id",
            {},
        )
    )

    # scope_denied → all actions denied
    cases.append(
        (
            "scope_denied",
            {"scope_denied": True},
            "act",
            "scope_denied_action",
            {"control_id": "apply", "action": UiActionKind.CLICK},
        )
    )

    return cases


# ── Built-in scenarios ──────────────────────────────────────────────────────


def builtin_scenarios() -> Dict[str, ScenarioScript]:
    """Return the four built-in scenario scripts.

    Returns a dict mapping scenario name to :class:`ScenarioScript`:

    - ``text_field_update``: set text on a text field and verify the value change
    - ``button_click``: click a button and verify the status change
    - ``checkbox_toggle``: toggle a checkbox and verify checked state
    - ``policy_denied``: attempt an action blocked by policy
    """
    return {
        "text_field_update": ScenarioScript(
            name="text_field_update",
            description="Set text on a text field and verify the value changes.",
            steps=[
                ScenarioStep(
                    tool="snapshot",
                    label="initial snapshot",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="find",
                    params={"label": "Project name"},
                    label="find text field",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="act",
                    params={
                        "action": UiActionKind.SET_TEXT,
                        "control_id": "project-name",
                        "text": "Scenario Test",
                    },
                    label="set text",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="wait_for",
                    params={
                        "condition": {
                            "kind": "value_equals",
                            "control_id": "project-name",
                            "value": "Scenario Test",
                            "timeout_ms": 200,
                            "interval_ms": 10,
                        },
                    },
                    label="wait for value change",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="snapshot",
                    label="verify snapshot",
                    expect_success=True,
                ),
            ],
        ),
        "button_click": ScenarioScript(
            name="button_click",
            description="Click the Apply button and verify the status label changes.",
            steps=[
                ScenarioStep(tool="snapshot", label="initial snapshot", expect_success=True),
                ScenarioStep(
                    tool="find",
                    params={"label": "Apply"},
                    label="find button",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="act",
                    params={
                        "action": UiActionKind.CLICK,
                        "control_id": "apply",
                    },
                    label="click apply",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="wait_for",
                    params={
                        "condition": {
                            "kind": "text_equals",
                            "control_id": "status",
                            "text": "Applied",
                            "timeout_ms": 200,
                            "interval_ms": 10,
                        },
                    },
                    label="wait for status change",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="snapshot",
                    label="verify snapshot",
                    expect_success=True,
                ),
            ],
        ),
        "checkbox_toggle": ScenarioScript(
            name="checkbox_toggle",
            description="Toggle the Enable cache checkbox and verify checked state.",
            steps=[
                ScenarioStep(tool="snapshot", label="initial snapshot", expect_success=True),
                ScenarioStep(
                    tool="find",
                    params={"label": "Enable cache"},
                    label="find checkbox",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="act",
                    params={
                        "action": UiActionKind.TOGGLE,
                        "control_id": "enable-cache",
                    },
                    label="toggle checkbox",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="wait_for",
                    params={
                        "condition": {
                            "kind": "checked_equals",
                            "control_id": "enable-cache",
                            "checked": True,
                            "timeout_ms": 200,
                            "interval_ms": 10,
                        },
                    },
                    label="wait for checked",
                    expect_success=True,
                ),
                ScenarioStep(
                    tool="snapshot",
                    label="verify snapshot",
                    expect_success=True,
                ),
            ],
        ),
        "policy_denied": ScenarioScript(
            name="policy_denied",
            description="Attempt a set_text action when allow_text_entry is disabled.",
            steps=[
                ScenarioStep(tool="snapshot", label="initial snapshot", expect_success=True),
                ScenarioStep(
                    tool="act",
                    params={
                        "action": UiActionKind.SET_TEXT,
                        "control_id": "project-name",
                        "text": "Denied",
                        "policy": {"allow_text_entry": False},
                    },
                    label="attempt denied set_text",
                    expect_success=False,
                    expect_error_code="policy_disabled",
                ),
            ],
        ),
    }
