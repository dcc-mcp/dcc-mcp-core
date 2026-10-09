"""Timed keypress wire contracts; mocks do not prove physical key release."""

from __future__ import annotations

from dataclasses import replace

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_keyboard import keypress_payload
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import runtime


@pytest.mark.parametrize("duration", [1, 500, 10000])
@pytest.mark.parametrize("keys", [["W"], ["w", "a"], ["UP", "RIGHT"]])
def test_keypress_preserves_native_hold_bounds(duration, keys):
    assert keypress_payload({"keys": keys, "duration_ms": duration}) == {"keys": keys, "duration_ms": duration}


@pytest.mark.parametrize("duration", [0, -1, 10001, True, 1.5, "500", None])
def test_keypress_rejects_invalid_hold_duration(duration):
    with pytest.raises(CuaCliError, match=r"1\.\.10000"):
        keypress_payload({"keys": ["W"], "duration_ms": duration})


@pytest.mark.parametrize("keys", [[], ["W", "w"], ["W", " W "], ["W", "A", "S"], ["CTRL+W"], ["SHIFT"], ["F1"]])
def test_held_keypress_keeps_native_movement_key_ceiling(keys):
    with pytest.raises(CuaCliError):
        keypress_payload({"keys": keys, "duration_ms": 500})


@pytest.mark.parametrize(
    "extra",
    [
        {"dx": 1},
        {"approved": True},
        {"unknown": None},
        {"modifiers": ["SHIFT"]},
        {"text": "x"},
        {"x": 1},
        {"path": [{"x": 1, "y": 1}]},
    ],
)
def test_unknown_and_unrelated_keypress_fields_never_reach_native(runtime, extra):
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    with pytest.raises(CuaCliError):
        owned.execute({"action": "keypress", "input_kind": "raw_input", "keys": ["W"], "duration_ms": 500, **extra})
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)
    owned.stop()


def test_immediate_shortcut_keypress_preserves_omitted_duration():
    assert keypress_payload({"keys": ["CTRL+F"], "path": [], "text": None}) == {"keys": ["CTRL+F"]}


@pytest.mark.parametrize("duration", [None, 1, 10000])
def test_formal_keypress_bridge_preserves_duration_and_exact_fields(runtime, monkeypatch, duration):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    config = replace(config, allowed_actions=("keypress",))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_PROCESS_ID", str(TARGET["process_id"]))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_WINDOW_HANDLE", str(TARGET["window_handle"]))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_WINDOW_TITLE", TARGET["window_title"])
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "true")
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_PROCESS_NAME", raising=False)
    bridge = HostExecutionBridge()

    def call(name, arguments):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "formal-keypress", **arguments},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    try:
        snapshot = call("snapshot", {})
        assert snapshot["success"] is True
        action = {
            "action": "keypress",
            "keys": ["W", "A"] if duration is not None else ["CTRL+F"],
            "snapshot_id": snapshot["context"]["snapshot_id"],
        }
        if duration is not None:
            action["duration_ms"] = duration
        result = call("act", action)
        assert result["success"] is True
        wire = process.requests[-1]["params"]["arguments"]["params"]["action"]
        expected = {"action", "input_kind", "intent", "delivery_mode", "keys"}
        if duration is not None:
            expected.add("duration_ms")
            assert wire["duration_ms"] == duration
        assert set(wire) == expected
        assert wire["keys"] == action["keys"]
        assert result["context"]["result"]["metadata"]["effect"] == "unverifiable"
        assert call("act", action)["error"] == "stale_observation"
    finally:
        bridge.shutdown_script_execution()


def test_timed_keypress_requires_unchanged_owner_grant(runtime):
    config, process, _ = runtime
    owned = client(replace(config, allowed_actions=("click",)))
    owned.snapshot(max_depth=1, max_nodes=1)
    with pytest.raises(CuaCliError, match="authorize"):
        owned.execute({"action": "keypress", "input_kind": "raw_input", "keys": ["W"], "duration_ms": 500})
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)
    owned.stop()


def test_timed_keypress_timeout_is_not_replayed_or_claimed_released(runtime):
    config, process, _ = runtime
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    object.__setattr__(config, "timeout_seconds", 0.01)
    process.drop_response = True
    action = {"action": "keypress", "input_kind": "raw_input", "keys": ["W"], "duration_ms": 10000}
    with pytest.raises(CuaCliError, match="not retried"):
        owned.execute(action)
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute(action)
    attempts = [
        r for r in process.requests if r.get("params", {}).get("arguments", {}).get("method") == "execute_action"
    ]
    assert len(attempts) == 1
    with pytest.raises(CuaCliError):
        owned.stop()
