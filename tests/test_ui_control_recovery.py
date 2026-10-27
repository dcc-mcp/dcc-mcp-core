"""Recovery guidance through the public bridge and fake Host, without native UI."""

from dataclasses import replace
import json

import pytest
import yaml

from test_cua_mcp_window_frame import frame_runtime
from test_ui_control_skill import _SKILL_DIR
from test_ui_control_skill import _configure_fake_host
from test_ui_control_skill import _FakeHostClient
from test_ui_control_skill import _load_cua_module
from test_ui_control_window_frame import frame_tool


def assert_read_only_recovery(result, *, error=False):
    assert result["success"] is not error
    assert result["context"]["recovery_actions"] == (["get_window_state"] if error else [])
    for action in ("restore_window", "activate_window", "show_window"):
        assert action not in json.dumps(result)
    assert "operator" in result["prompt"]


@pytest.mark.parametrize("error", [False, True])
def test_host_read_only_recovery_respects_policy(monkeypatch, error):
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)
    if error:

        def unavailable(self):
            raise backend.UiControlHostError("invalid_target", "Exact window is not capturable.")

        monkeypatch.setattr(_FakeHostClient, "window_state", unavailable)
    try:
        result = backend.act_tool({"action": "get_window_state", "policy": {"allow_mutating_actions": False}})
        assert_read_only_recovery(result, error=error)
        assert _FakeHostClient.instances[0].window_operations == []
    finally:
        backend.cleanup()


@pytest.mark.parametrize("error_code", [None, "invalid_target", "target_unavailable", "target_minimized"])
def test_pixels_read_only_recovery_respects_policy(frame_tool, error_code):
    call, config, process, _ = frame_tool
    config = replace(config, window_operations=("restore_activate", "activate"))
    if error_code:

        def unavailable(raw):
            raw.clear()
            raw.update(type="error", code=error_code, message="Exact window is unavailable.")

        process.mutate_metadata = unavailable
    result = call({"action": "get_window_state", "policy": {"allow_mutating_actions": False}}, runtime=config)
    assert_read_only_recovery(result, error=bool(error_code))
    assert process.metadata_reads == 1 and process.snapshots == 0
    assert not any(
        r.get("params", {}).get("arguments", {}).get("method") == "change_window_state" for r in process.requests
    )


@pytest.mark.parametrize("operations", [(), ("activate",), ("restore_activate",), ("restore_activate", "activate")])
@pytest.mark.parametrize("minimized", [False, True])
def test_pixels_hidden_recovery_uses_state_and_owner_grant(frame_tool, operations, minimized):
    call, config, process, _ = frame_tool
    config = replace(config, window_operations=operations)
    process.mutate_metadata = lambda raw: raw["state"].update(visible=False, minimized=minimized)
    result = call({"action": "get_window_state"}, runtime=config)
    assert result["success"] is True
    expected = [
        name
        for operation, name in (("restore_activate", "restore_window"), ("activate", "activate_window"))
        if operation in operations
    ]
    assert result["context"]["recovery_actions"] == expected
    if not minimized or "restore_activate" not in operations:
        assert "restore visibility manually" in result["prompt"]
    else:
        assert "restore_window" in result["prompt"]
    assert "show_window" not in json.dumps(result)
    assert process.metadata_reads == 1 and process.snapshots == 0


def test_public_window_actions_exclude_unsupported_show():
    tools = yaml.safe_load((_SKILL_DIR / "tools.yaml").read_text(encoding="utf-8"))["tools"]
    act = next(tool for tool in tools if tool["name"] == "act")
    assert "show_window" not in act["input_schema"]["properties"]["action"]["enum"]


def test_unsupported_show_is_rejected_before_opening_host(monkeypatch):
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)
    try:
        result = backend.act_tool({"action": "show_window"})
        assert result["success"] is False and result["error"] == "unsupported_action"
        assert not _FakeHostClient.instances
    finally:
        backend.cleanup()


@pytest.mark.parametrize("owned", [False, True])
@pytest.mark.parametrize("code", ["approval_required", "hard_denied", "user_interrupted", "desktop_unavailable"])
def test_recovery_does_not_turn_stop_gates_into_window_mutations(frame_runtime, owned, code):
    config, _, launches = frame_runtime
    backend = _load_cua_module()
    params = {"trusted_ui_control_runtime": config} if owned else {}
    result = backend._host_error(backend.UiControlHostError(code, "Stop this operation."), params)
    assert result["success"] is False
    assert "recovery_actions" not in result.get("context", {})
    assert not launches


def test_error_recovery_does_not_recommend_disabled_observation():
    backend = _load_cua_module()
    result = backend._host_error(
        backend.UiControlHostError("invalid_target", "Exact window is unavailable."),
        {"policy": {"allow_snapshot": False, "allow_mutating_actions": False}},
    )
    assert result["context"]["recovery_actions"] == []
    assert "get_window_state" not in result["prompt"] and "snapshot" not in result["prompt"]
