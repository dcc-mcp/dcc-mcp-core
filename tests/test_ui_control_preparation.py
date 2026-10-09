"""Formal foreground preparation must finish before a new input token is usable."""

from __future__ import annotations

from dataclasses import replace
from typing import Any

import pytest

from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import runtime


@pytest.fixture
def route(runtime, monkeypatch):
    config, process, launches = runtime
    config = replace(config, window_operations=("activate", "restore_activate"))
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "1")
    bridge = HostExecutionBridge()

    def call(name, arguments=None, *, owner=config, scope=TARGET):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "prepare-test", **TARGET, **(arguments or {})},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **scope},
            trusted_ui_control_runtime=owner,
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


def test_uia_timeout_is_capture_failure_after_successful_activation(monkeypatch):
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
