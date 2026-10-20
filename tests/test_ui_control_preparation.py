"""Formal foreground preparation must finish before a new input token is usable."""

from __future__ import annotations

from dataclasses import replace
import json
from typing import Any

import jsonschema
import pytest
import yaml

from conftest import REPO_ROOT
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import runtime


@pytest.fixture(params=[False, True], ids=["ambient-clean", "ambient-conflict"])
def _ambient_scope(request, monkeypatch):
    if request.param:
        for name, value in {
            "PROCESS_ID": "43",
            "WINDOW_HANDLE": "84",
            "WINDOW_TITLE": "Different Test Target",
            "PROCESS_NAME": "powershell.exe",
            "DCC_TYPE": "different-test-host",
        }.items():
            monkeypatch.setenv("DCC_MCP_UI_CONTROL_" + name, value)


@pytest.fixture
def _isolated_scope(monkeypatch, _ambient_scope):
    # Production operator scope still takes precedence. This fake route must
    # explicitly isolate its target from the inherited worker environment.
    for name in ("PROCESS_ID", "WINDOW_HANDLE", "WINDOW_TITLE", "PROCESS_NAME", "DCC_TYPE"):
        monkeypatch.delenv("DCC_MCP_UI_CONTROL_" + name, raising=False)


@pytest.fixture
def route(runtime, monkeypatch, _isolated_scope):
    config, process, launches = runtime
    config = replace(config, window_operations=("activate", "restore_activate"))
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    bridge = HostExecutionBridge()

    def call(name, arguments=None, *, owner=config, scope=TARGET, cancel_token=None):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "prepare-test", **TARGET, **(arguments or {})},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **scope},
            trusted_ui_control_runtime=owner,
            cancel_token=cancel_token,
        )

    yield call, process, launches, config
    call("stop_computer_use")
    bridge.shutdown_script_execution()


def methods(process) -> list[str]:
    return [
        request["params"]["arguments"]["method"]
        for request in process.requests
        if request.get("params", {}).get("name") == "dcc_cua_task_call"
    ]


def _ui_control_tool_schemas() -> dict[str, dict]:
    """Load the exposed ``ui-control`` tool input schemas keyed by bare tool name.

    Read straight from the skill's ``tools.yaml`` (the source of truth for the
    exposed schema) so the assertion holds regardless of whether the runtime
    resolved ``parse_skill_md`` through the compiled ``_core`` extension or the
    pure-Python lite fallback, whose tool representations differ.
    """
    tools_yaml = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "tools.yaml"
    data = yaml.safe_load(tools_yaml.read_text(encoding="utf-8"))
    schemas: dict[str, dict] = {}
    for tool in data["tools"]:
        schema = tool["input_schema"]
        if isinstance(schema, str):
            schema = json.loads(schema)
        schemas[tool["name"]] = schema
    return schemas


@pytest.mark.parametrize("operation", ["activate", "restore_activate"])
def test_prepare_returns_new_observation_before_input(route, operation):
    call, process, launches, _ = route
    old = call("snapshot")["context"]["snapshot_id"]
    ready = call("prepare_foreground", {"operation": operation})
    assert ready["success"] is True, ready
    context = ready["context"]
    assert methods(process) == ["snapshot", "change_window_state", "get_window_state", "snapshot"]
    assert len(launches) == 1
    assert context["snapshot_id"] != old
    assert context["foreground_preparation"]["stage"] == "ready"
    assert context["foreground_preparation"]["window_state"]["foreground"] is True
    assert context["capture_provenance"]["pixels_captured"] is True
    assert context["observation"]["window_handle"] == TARGET["window_handle"]
    stale = call("act", {"action": "click", "x": 2, "y": 3, "snapshot_id": old})
    assert stale["success"] is False and stale["error"] == "stale_observation"
    acted = call("act", {"action": "click", "x": 2, "y": 3, "snapshot_id": context["snapshot_id"]})
    assert acted["success"] is True, acted


@pytest.mark.parametrize("denial", ["window_grant", "snapshot_policy", "mutation_policy"])
def test_missing_authority_refuses_before_launch_or_mutation(route, denial):
    call, process, launches, config = route
    arguments: dict[str, Any] = {"operation": "restore_activate"}
    if denial == "window_grant":
        config = replace(config, window_operations=())
    else:
        arguments["policy"] = {"allow_snapshot" if denial == "snapshot_policy" else "allow_mutating_actions": False}
    result = call("prepare_foreground", arguments, owner=config)
    assert result["success"] is False
    assert result["context"]["foreground_preparation"]["stage"] == "authorization"
    assert launches == [] and methods(process) == []


@pytest.mark.parametrize("stage", ["activation", "foreground_readback"])
def test_failed_activation_or_readback_never_captures(route, stage):
    call, process, _, _ = route
    old = call("snapshot")["context"]["snapshot_id"]

    def refuse(raw):
        if (raw["type"] == "window_state_changed") == (stage == "activation"):
            raw["state"]["foreground"] = False

    process.mutate_window = refuse
    result = call("prepare_foreground", {"operation": "restore_activate"})
    assert result["success"] is False
    assert result["context"]["foreground_preparation"]["stage"] == stage
    assert process.snapshots == 1
    assert "execute_action" not in methods(process)
    # A rejected preparation must not leave the old backend snapshot usable.
    if process.returncode is None:
        stale = call("act", {"action": "click", "x": 2, "y": 3, "snapshot_id": old})
        assert stale["success"] is False


def test_same_pid_different_hwnd_cannot_rebind_existing_session(route):
    call, process, launches, _ = route
    assert call("snapshot")["success"] is True
    result = call("prepare_foreground", {"operation": "activate", "window_handle": 501}, scope={"process_id": 42})
    assert result["success"] is False
    assert result["context"]["foreground_preparation"]["stage"] == "binding"
    assert methods(process) == ["snapshot"] and len(launches) == 1


@pytest.mark.parametrize("field", ["window_handle", "observation_id"])
def test_changed_target_or_replayed_capture_is_not_ready(route, field):
    call, process, _, _ = route
    assert call("snapshot")["success"] is True

    def corrupt(raw):
        if field == "window_handle":
            raw["observation"][field] = 501
        else:
            raw["observation_id"] = "native-obs-1"
            raw["observation"][field] = "native-obs-1"

    process.mutate_snapshot = corrupt
    result = call("prepare_foreground", {"operation": "activate"})
    assert result["success"] is False
    assert result["context"]["foreground_preparation"]["stage"] == "capture"
    assert result["context"].get("snapshot_id") is None
    assert "execute_action" not in methods(process)


def test_read_only_snapshot_does_not_prepare_foreground(route):
    call, process, _, _ = route
    assert call("snapshot")["success"] is True
    assert methods(process) == ["snapshot"]


@pytest.mark.parametrize(
    "invalid",
    [
        {"operation": []},
        {"process_id": True},
        {"window_handle": 0},
        {"session_id": "wrong session"},
        {"session_id": ""},
        {"resume_computer_use": True},
    ],
)
def test_invalid_preparation_never_launches_or_mutates(route, invalid):
    call, process, launches, _ = route
    result = call("prepare_foreground", {"operation": "restore_activate", **invalid})
    assert result["success"] is False
    assert result["context"]["foreground_preparation"]["stage"] == "binding"
    assert launches == [] and methods(process) == []


def test_uia_timeout_is_capture_failure_after_successful_activation(monkeypatch, _isolated_scope):
    from test_cua_mcp_pixels import window_response
    from test_ui_control_window_recovery import _load_backend

    backend = _load_backend()
    calls = []

    class SemanticClient:
        def __init__(self, **kwargs):
            self.target = dict(TARGET)

        def change_window_state(self, operation):
            calls.append(operation)
            return window_response(operation)

        def window_state(self):
            calls.append("readback")
            return {"state": window_response()["state"]}

        def snapshot(self, **kwargs):
            calls.append("snapshot")
            raise backend.UiControlHostError("input_failed", "get_window_state timed out after 4s: UIA unresponsive")

        def stop(self):
            return {"active": False}

    monkeypatch.setattr(backend, "_HostClient", SemanticClient)
    monkeypatch.setattr(backend, "_ensure_idle_reaper", lambda: None)
    try:
        result = backend.prepare_foreground_tool(
            {"session_id": "uia-test", **TARGET, "operation": "restore_activate", "trusted_adapter_scope": TARGET}
        )
        assert result["success"] is False and result["error"] == "input_failed"
        assert "UIA unresponsive" in result["message"]
        preparation = result["context"]["foreground_preparation"]
        assert preparation["stage"] == "capture" and preparation["capture_mode"] == "semantic"
        assert preparation["activation"]["state"]["foreground"] is True
        assert preparation["window_state"]["foreground"] is True
        assert preparation["fresh_observation_ready"] is False
        assert backend._CLIENTS["uia-test"]["snapshot_id"] is None
        assert calls == ["restore_activate", "readback", "snapshot"]
    finally:
        backend.cleanup()


@pytest.mark.parametrize("code,reason", [("capture_failed", "root_overlap"), ("user_interrupted", None)])
def test_native_capture_refusal_retains_stage_and_reason_without_retry(route, code, reason):
    call, process, _, _ = route

    def refuse(raw):
        task_context = raw["task_context"]
        raw.clear()
        raw.update(
            type="error",
            code=code,
            message="Native capture refused.",
            task_context=task_context,
            details={"reason": reason, "fresh_observation_required": True, "blind_retry": False},
        )

    process.mutate_snapshot = refuse
    result = call("prepare_foreground", {"operation": "restore_activate"})
    assert result["success"] is False and result["error"] == code
    preparation = result["context"]["foreground_preparation"]
    assert preparation["stage"] == "capture" and preparation["capture_mode"] == "pixels_only"
    assert preparation["window_state"]["foreground"] is True
    assert preparation["activation"]["state"]["foreground"] is True
    assert preparation["fresh_observation_ready"] is False
    if reason:
        assert result["context"]["details"]["reason"] == reason
    assert methods(process) == ["change_window_state", "get_window_state", "snapshot"]


# The native dcc-cua UIA-timeout error Core passes through; its trailing
# "Fallback options" prose targets dcc-cua's own CLI surface, not the Core
# ``prepare_foreground`` schema (issue #2819).
_UIA_ADVISORY = (
    "get_window_state timed out after 4s (UIA provider unresponsive on hwnd 0x1234, class 'UnrealWindow'). "
    "Fallback options: (a) re-call this tool with a depth-limited scan (`max_elements` / `max_depth`) ... "
    "if the tree stays unusable, act by pixel `click(x, y)` off the screenshot in the response; "
    "(b) if the target is a transient VCL / message-box dialog, send `press_key` with "
    '`delivery_mode:"foreground"` (SendInput) to fire the default accelerator (Esc / Enter / Y / N) '
    "without needing the tree..."
)


def test_uia_capture_failure_is_failed_schema_bound_and_image_free(monkeypatch, _isolated_scope):
    """Acceptance criteria for a semantic UIA capture timeout after activation succeeds.

    (1) overall readiness stays failed and no content input is sent; (2) every
    suggested next call is supported by the exposed schema; (3) the response never
    advertises an image that is absent; (4) no fresh alternate observation is
    fabricated. It also asserts the native reason is retained while dcc-cua's
    tool-specific advisory is not echoed back to the caller.
    """
    from test_cua_mcp_pixels import window_response
    from test_ui_control_window_recovery import _load_backend

    backend = _load_backend()
    calls = []

    class SemanticClient:
        def __init__(self, **kwargs):
            self.target = dict(TARGET)

        def change_window_state(self, operation):
            calls.append(operation)
            return window_response(operation)

        def window_state(self):
            calls.append("readback")
            return {"state": window_response()["state"]}

        def snapshot(self, **kwargs):
            calls.append("snapshot")
            raise backend.UiControlHostError("input_failed", _UIA_ADVISORY)

        def stop(self):
            return {"active": False}

    monkeypatch.setattr(backend, "_HostClient", SemanticClient)
    monkeypatch.setattr(backend, "_ensure_idle_reaper", lambda: None)
    try:
        result = backend.prepare_foreground_tool(
            {"session_id": "uia-test", **TARGET, "operation": "restore_activate", "trusted_adapter_scope": TARGET}
        )
    finally:
        backend.cleanup()

    context = result["context"]
    preparation = context["foreground_preparation"]
    recovery = context["recovery"]

    # (1) Overall readiness stays failed and no content input was sent.
    assert result["success"] is False
    assert result["error"] == "input_failed"
    assert preparation["stage"] == "capture"
    assert preparation["fresh_observation_ready"] is False
    assert preparation["content_input_sent"] is False
    assert recovery["content_input_sent"] is False and recovery["content_input_allowed"] is False
    assert calls == ["restore_activate", "readback", "snapshot"]
    # Activation success is retained, clearly separated from the observation failure.
    assert preparation["activation"]["state"]["foreground"] is True
    assert preparation["window_state"]["foreground"] is True
    assert recovery["activation_succeeded"] is True and recovery["observation_failed"] is True

    # Native failure reason is retained; the vendor advisory is not echoed.
    assert "UIA provider unresponsive" in result["message"]
    assert "Fallback options" not in result["message"]
    assert "UIA provider unresponsive" in recovery["native_reason"]

    # (2) Every suggested next call is supported by the exposed schema.
    schemas = _ui_control_tool_schemas()
    assert recovery["next_steps"], "capture failure must expose at least one real next step"
    for step in recovery["next_steps"]:
        tool = step["tool"]
        assert tool.startswith("ui_control__")
        name = tool[len("ui_control__") :]
        assert name in schemas, f"{tool} is not exposed by ui-control tools.yaml"
        jsonschema.Draft202012Validator(schemas[name]).validate(step["arguments"])
    for action in context["recovery_actions"]:
        assert action in schemas, f"recovery action {action!r} is not a real ui-control tool"

    # Re-observation must stay on the same exact target.
    reobserve = next(step for step in recovery["next_steps"] if step["tool"] == "ui_control__snapshot")
    assert reobserve["arguments"]["process_id"] == TARGET["process_id"]
    assert reobserve["arguments"]["window_handle"] == TARGET["window_handle"]

    # (3) The response never advertises an image that is absent.
    assert recovery["image_available"] is False
    assert "__rich__" not in result and "__rich__" not in context
    for key in ("image", "mime_type", "snapshot_id", "observation", "state_delta"):
        assert key not in context, f"failed capture must not advertise {key}"

    # (4) No successful alternate observation is fabricated, and no pixels route exists.
    assert recovery["fresh_observation_available"] is False
    assert recovery["pixels_only_route_available"] is False
    assert context.get("snapshot_id") is None

    # The response must not surface vendor advisory aimed at a different tool surface.
    serialized = json.dumps(result)
    for token in (
        "max_elements",
        "max_depth",
        "click(x",
        "press_key",
        "delivery_mode",
        "escalate_session",
        "transient",
        "VCL",
        "SendInput",
        "screenshot",
    ):
        assert token not in serialized, f"response must not surface vendor advisory token {token!r}"


def test_capture_stage_cancellation_keeps_abort_prompt_without_observation_recovery(route):
    """A cancellation during capture is an abort, not an observation failure.

    Issue #2819 acceptance requires cancellation/interruption rules to be
    preserved: cancelling during the native snapshot must keep its
    cancellation-specific prompt and retained receipts, and must not be
    overwritten by the schema-accurate observation-failure recovery Core
    authors for a genuine ``input_failed`` / ``capture_failed`` observation
    miss.
    """
    from dcc_mcp_core.cancellation import CancelToken

    call, process, _, _ = route
    # Establish a session first, then arm cancellation on the capture snapshot.
    assert call("snapshot")["success"] is True
    token = CancelToken()
    process.mutate_snapshot = lambda raw: token.cancel()

    result = call("prepare_foreground", {"operation": "restore_activate"}, cancel_token=token)

    assert result["success"] is False
    assert result["error"] == "cancelled"
    assert "Foreground preparation was cancelled" in result["message"]
    # The abort keeps its own cancellation-specific guidance ...
    assert "Inspect retained native receipts" in result["prompt"]
    assert "explicitly stop the owned session" in result["prompt"]
    # ... and is not rewritten as an observation-failure recovery.
    assert "observation capture failed at the capture stage" not in result["prompt"]
    assert "Foreground activation succeeded" not in result["prompt"]
    context = result["context"]
    assert "recovery" not in context
    assert "recovery_actions" not in context
    assert "possible_solutions" not in context
    assert "recovery_scope" not in context
    assert context["foreground_preparation"]["fresh_observation_ready"] is False


def test_capture_stage_user_interrupted_keeps_abort_without_observation_recovery(route):
    """A user interruption during capture is an abort, not an observation failure.

    ``user_interrupted`` must not be misclassified as a missing-observation
    defect: Core must not author the observation-failure recovery over it, and
    no recovery context may appear in the response.
    """
    call, process, _, _ = route

    def interrupt(raw):
        task_context = raw["task_context"]
        raw.clear()
        raw.update(
            type="error",
            code="user_interrupted",
            message="Operator interrupted the capture.",
            task_context=task_context,
            details={"fresh_observation_required": True, "blind_retry": False},
        )

    process.mutate_snapshot = interrupt
    result = call("prepare_foreground", {"operation": "restore_activate"})

    assert result["success"] is False
    assert result["error"] == "user_interrupted"
    # No observation-failure recovery was authored for this abort.
    assert "observation capture failed at the capture stage" not in result["prompt"]
    assert "Foreground activation succeeded" not in result["prompt"]
    context = result["context"]
    assert "recovery" not in context
    assert "recovery_actions" not in context
    assert "possible_solutions" not in context
    assert "recovery_scope" not in context
    assert context["foreground_preparation"]["fresh_observation_ready"] is False


@pytest.mark.parametrize("operation", ["activate", "restore_activate"])
def test_pixels_preparation_success_has_fresh_same_target_observation(route, operation):
    """Criterion (4): a successful pixels-only observation carries fresh, same-target evidence."""
    call, _process, _launches, _config = route
    old = call("snapshot")["context"]["snapshot_id"]
    ready = call("prepare_foreground", {"operation": operation})
    assert ready["success"] is True, ready
    context = ready["context"]
    # Fresh: a new native observation id, not the pre-preparation one.
    assert context["snapshot_id"] != old
    # Same target: bound to the exact same PID/HWND that was authorized.
    assert context["observation"]["window_handle"] == TARGET["window_handle"]
    assert context["observation"]["process_id"] == TARGET["process_id"]
    assert context["capture_provenance"]["pixels_captured"] is True


@pytest.mark.parametrize(
    "code",
    ["input_failed", "capture_failed", "stale_observation", "backend_unavailable", "timeout"],
)
def test_pixels_capture_failure_recovery_never_suggests_doomed_snapshot(route, code):
    """Pixels-only capture-failure recovery must not offer ``ui_control__snapshot``.

    A pixels-only capture failure closes the owned pixels task inside
    ``PixelsMcpHostClient.snapshot`` (it stops the task on any ``CuaCliError``), so
    a follow-up ``ui_control__snapshot`` against the same session always fails with
    ``backend_unavailable`` ("The owned pixels task is closed."). The recovery
    context therefore must not suggest that step and must report no usable
    pixels-only re-observation route. Every whitelisted observation-failure code
    takes this path; the semantic route still offers ``ui_control__snapshot`` and
    is covered separately.
    """
    call, process, _launches, _config = route

    def refuse(raw):
        task_context = raw["task_context"]
        raw.clear()
        raw.update(
            type="error",
            code=code,
            message="Native capture refused.",
            task_context=task_context,
            details={"fresh_observation_required": True, "blind_retry": False},
        )

    process.mutate_snapshot = refuse
    result = call("prepare_foreground", {"operation": "restore_activate"})

    # Overall readiness stays failed at the capture stage, on the pixels route.
    assert result["success"] is False
    assert result["error"] == code
    preparation = result["context"]["foreground_preparation"]
    assert preparation["stage"] == "capture"
    assert preparation["capture_mode"] == "pixels_only"
    assert preparation["fresh_observation_ready"] is False

    context = result["context"]
    recovery = context["recovery"]
    # No usable pixels-only re-observation route is advertised ...
    assert recovery["pixels_only_route_available"] is False
    # ... and the doomed ui_control__snapshot step is not offered ...
    next_step_tools = [step["tool"] for step in recovery["next_steps"]]
    assert "ui_control__snapshot" not in next_step_tools
    assert context["recovery_actions"] == ["stop_computer_use"]
    assert "snapshot" not in context["recovery_actions"]
    # ... while the schema-real release step remains available.
    assert next_step_tools == ["ui_control__stop_computer_use"]
    schemas = _ui_control_tool_schemas()
    for step in recovery["next_steps"]:
        tool = step["tool"]
        assert tool.startswith("ui_control__")
        name = tool[len("ui_control__") :]
        assert name in schemas, f"{tool} is not exposed by ui-control tools.yaml"
        jsonschema.Draft202012Validator(schemas[name]).validate(step["arguments"])

    # The prompt must steer the caller away from the closed task, not toward it.
    assert "do not call ui_control__snapshot against this closed task" in result["prompt"]
    assert "re-authorize a fresh owned pixels task" in result["prompt"]

    # Lock the regression: following the previously-suggested snapshot is in fact
    # unusable against the closed owned pixels task.
    follow_up = call("snapshot")
    assert follow_up["success"] is False
    assert follow_up["error"] == "backend_unavailable"
    assert "owned pixels task is closed" in follow_up["message"]
