"""Core wire coverage; mocked native delivery does not certify button release."""

from __future__ import annotations

from dataclasses import replace

import pytest

from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_pointer import pointer_payload
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import runtime


@pytest.mark.parametrize(
    "action",
    [
        {"action": "move", "x": 20.5, "y": 30.25, "duration_ms": 50},
        {"action": "drag", "path": [{"x": 1, "y": 2}, {"x": 639, "y": 479}], "button": "right", "duration_ms": 400},
    ],
)
def test_pointer_uses_one_exact_observation_and_preserves_screenshot_coordinates(runtime, action):
    config, process, _ = runtime
    owned = client(replace(config, allowed_actions=(action["action"],)))
    snapshot = owned.snapshot(max_depth=1, max_nodes=1)
    assert snapshot["observation"]["source_rect"][0] == -640
    assert snapshot["observation"]["capture_provenance"]["window_dpi"] == 144
    result = owned.execute({**action, "input_kind": "raw_input", "intent": "navigate"})
    params = process.requests[-1]["params"]["arguments"]["params"]
    assert params["observation_id"] == "native-obs-1"
    for key, value in action.items():
        assert params["action"][key] == value
    assert result["effect"] == "unverifiable" and result["verification_required"]
    with pytest.raises(CuaCliError, match="fresh pixels"):
        owned.execute({**action, "input_kind": "raw_input"})
    owned.stop()
    with pytest.raises(CuaCliError):
        owned.execute({**action, "input_kind": "raw_input"})


@pytest.mark.parametrize(
    "point",
    [
        {"x": -1, "y": 0},
        {"x": 640, "y": 0},
        {"x": 0, "y": 480},
        {"x": float("nan"), "y": 1},
        {"x": float("inf"), "y": 1},
        {"x": True, "y": 1},
        {"x": 10**400, "y": 1},
        {"x": 0, "y": 0, "screen_x": 1},
    ],
)
def test_every_drag_point_is_bounded_in_physical_screenshot_pixels(point):
    with pytest.raises(CuaCliError):
        pointer_payload({"action": "drag", "path": [{"x": 0, "y": 0}, point]}, (640, 480))


@pytest.mark.parametrize(
    "extra",
    [
        {"duration_ms": 0},
        {"duration_ms": 1001},
        {"duration_ms": True},
        {"path": [{"x": 1, "y": 1}]},
        {"path": [{"x": 1, "y": 1}] * 257},
        {"button": "unknown"},
        {"button": []},
        {"keys": ["W"]},
        {"text": "hidden"},
        {"x": 0},
    ],
)
def test_drag_rejects_unbounded_or_unrelated_fields(extra):
    with pytest.raises(CuaCliError):
        pointer_payload({"action": "drag", "path": [{"x": 0, "y": 0}, {"x": 1, "y": 1}], **extra}, (640, 480))


def test_disabled_owner_grant_never_sends_pointer_input(runtime):
    config, process, _ = runtime
    owned = client(replace(config, allowed_actions=()))
    owned.snapshot(max_depth=1, max_nodes=1)
    with pytest.raises(CuaCliError, match="authorize"):
        owned.execute({"action": "move", "input_kind": "raw_input", "x": 0, "y": 0})
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)
    owned.stop()


def test_formal_ui_control_drag_preserves_path_and_rejects_replay(runtime, monkeypatch):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    config = replace(config, allowed_actions=("drag",))
    for name, value in {
        "DCC_MCP_UI_CONTROL_PROCESS_ID": str(TARGET["process_id"]),
        "DCC_MCP_UI_CONTROL_WINDOW_HANDLE": str(TARGET["window_handle"]),
        "DCC_MCP_UI_CONTROL_WINDOW_TITLE": TARGET["window_title"],
        "DCC_MCP_CUA_ALLOW_RAW_INPUT": "true",
    }.items():
        monkeypatch.setenv(name, value)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_PROCESS_NAME", raising=False)
    bridge = HostExecutionBridge()

    def call(name, arguments):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "formal-pointer", **arguments},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    try:
        snapshot = call("snapshot", {})
        assert snapshot["success"] is True
        arguments = {
            "action": "drag",
            "button": "right",
            "duration_ms": 250,
            "path": [{"x": 20, "y": 30}, {"x": 40, "y": 50}],
            "snapshot_id": snapshot["context"]["snapshot_id"],
        }
        assert call("act", arguments)["success"] is True
        sent = process.requests[-1]["params"]["arguments"]["params"]["action"]
        assert sent["path"] == arguments["path"] and sent["duration_ms"] == 250
        assert call("act", arguments)["error"] == "stale_observation"
    finally:
        bridge.shutdown_script_execution()


def test_timed_out_drag_is_not_replayed_and_does_not_claim_release(runtime):
    config, process, _ = runtime
    config = replace(config, allowed_actions=("drag",))
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    object.__setattr__(config, "timeout_seconds", 0.01)
    process.drop_response = True
    action = {"action": "drag", "input_kind": "raw_input", "path": [{"x": 1, "y": 1}, {"x": 2, "y": 2}]}
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


def test_changed_native_identity_after_drag_closes_owned_task(runtime):
    config, process, _ = runtime
    owned = client(replace(config, allowed_actions=("drag",)))
    owned.snapshot(max_depth=1, max_nodes=1)

    def change_identity(raw):
        raw["result"]["native_instance"]["process_creation_time_100ns"] += 1

    process.mutate_action = change_identity
    with pytest.raises(CuaCliError, match="native fence"):
        owned.execute({"action": "drag", "input_kind": "raw_input", "path": [{"x": 1, "y": 1}, {"x": 2, "y": 2}]})
    assert process.returncode == 0
    assert sum(r.get("params", {}).get("name") == "stop_task" for r in process.requests) == 1
