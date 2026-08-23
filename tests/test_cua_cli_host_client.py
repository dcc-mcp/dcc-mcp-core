from __future__ import annotations

import importlib.util
from pathlib import Path
from types import SimpleNamespace
from typing import Any

from conftest import REPO_ROOT

_CLIENT_PATH = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "scripts" / "_cua_cli_host_client.py"
_BACKEND_PATH = _CLIENT_PATH.with_name("_cua_backend.py")


def _load(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeBridge:
    def __init__(self, capabilities: tuple[str, ...] = ("native_menu_path",)) -> None:
        self.calls: list[tuple[str, dict[str, Any]]] = []
        self.closed = False
        self.contract = SimpleNamespace(capabilities=capabilities)

    def call(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self.calls.append((method, params))
        if method == "open_session":
            return {
                "type": "session_opened",
                "window_capability": "cua-window-1",
                "target": {"process_id": 42, "window_handle": 500, "window_title": "Maya"},
            }
        if method == "snapshot":
            return {
                "type": "snapshot",
                "observation_id": "observation-1",
                "accessibility_state_id": "accessibility-1",
                "target": {"process_id": 42, "window_handle": 500, "window_title": "Maya"},
                "root": {
                    "elements": [
                        {
                            "element_index": 0,
                            "element_token": "dcc-wuia:snapshot:0",
                            "depth": 0,
                            "role": "Window",
                            "name": "Maya",
                            "enabled": True,
                        },
                        {
                            "element_index": 1,
                            "element_token": "dcc-wuia:snapshot:1",
                            "depth": 1,
                            "role": "Button",
                            "name": "Create Cube",
                            "enabled": True,
                            "focused": True,
                        },
                    ]
                },
                "node_count": 2,
                "image": {
                    "encoding": "shared_memory",
                    "name": "cua-image",
                    "id": "image-1",
                    "length": 3,
                    "mime_type": "image/png",
                },
            }
        if method == "execute_action":
            return {
                "type": "action_completed",
                "success": True,
                "action_id": "action-1",
                "target_closed": False,
            }
        if method == "change_window_state":
            return {
                "type": "window_state_changed",
                "state": {"minimized": False, "foreground": True},
            }
        if method == "invoke_menu":
            return {
                "type": "menu_invoked",
                "result": {
                    "success": True,
                    "effect": "unverifiable",
                    "verification_required": True,
                    "observation_required": True,
                    "target": {"process_id": 42, "window_handle": 500},
                },
            }
        if method == "recording_start":
            return {
                "type": "recording_started",
                "result": {"structuredContent": {"enabled": True, "next_turn": 1}},
            }
        if method == "recording_state":
            return {
                "type": "recording_state",
                "result": {"structuredContent": {"enabled": True, "next_turn": 2}},
            }
        if method == "recording_stop":
            return {
                "type": "recording_stopped",
                "result": {"structuredContent": {"enabled": False, "next_turn": 2}},
            }
        if method == "stop_session":
            return {"type": "session_stopped", "session_id": "maya", "cleanup_pending": False}
        raise AssertionError(method)

    def close(self) -> None:
        self.closed = True

    def read_image(self, response: dict[str, Any]) -> bytes:
        assert response["image"]["length"] == 3
        return b"png"


def test_cua_host_adapter_preserves_exact_grant_snapshot_and_action_fences() -> None:
    client_module = _load(_CLIENT_PATH, "_test_cua_cli_host_client")
    bridge = FakeBridge()
    client = client_module.UiControlHostClient(
        session_id="maya",
        task_grant_id="grant-1",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        window_title="Autodesk Maya",
        allow_raw_input=True,
        allow_menu_invoke=True,
        bridge=bridge,
    )

    opened = bridge.calls[0]
    assert opened[0] == "open_session"
    assert opened[1]["grant"] == {
        "task_grant_id": "grant-1",
        "application_label": "maya",
        "process_id": 42,
        "window_handle": 500,
        "window_title": "Autodesk Maya",
        "allow_raw_input": True,
        "allow_recording": True,
        "allow_menu_invoke": True,
    }
    snapshot = client.snapshot(max_depth=5, max_nodes=250)
    button = snapshot["root"]["children"][0]
    assert snapshot["image_bytes"] == b"png"
    assert snapshot["focus_runtime_id"] == "dcc-wuia:snapshot:1"
    assert button["element_token"] == "dcc-wuia:snapshot:1"

    client.execute(
        {
            "action": "click",
            "input_kind": "semantic",
            "intent": "ordinary_edit",
            "element_token": button["element_token"],
        }
    )
    action = bridge.calls[-1]
    assert action[0] == "execute_action"
    assert action[1]["observation_id"] == "observation-1"
    assert action[1]["accessibility_state_id"] == "accessibility-1"

    started = client.recording_start(output_dir="C:/recording", record_video=True)
    assert started["structuredContent"]["enabled"] is True
    assert bridge.calls[-1][1]["request"] == {
        "output_dir": "C:/recording",
        "record_video": True,
    }
    assert client.recording_state()["structuredContent"]["next_turn"] == 2
    assert client.recording_stop()["structuredContent"]["enabled"] is False

    assert client.stop()["cleanup_pending"] is False
    assert bridge.closed is True


def test_restore_window_uses_cua_restore_activate_contract() -> None:
    client_module = _load(_CLIENT_PATH, "_test_cua_restore_activate")
    bridge = FakeBridge()
    client = client_module.UiControlHostClient(
        session_id="maya",
        task_grant_id="grant-1",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        allow_menu_invoke=True,
        bridge=bridge,
    )

    result = client.change_window_state("restore")

    assert result["state"]["foreground"] is True
    assert bridge.calls[-1] == (
        "change_window_state",
        {
            "session_id": "maya",
            "task_grant_id": "grant-1",
            "window_capability": "cua-window-1",
            "operation": "restore_activate",
        },
    )


def test_native_menu_path_uses_explicit_grant_and_invalidates_observation() -> None:
    client_module = _load(_CLIENT_PATH, "_test_cua_native_menu_path")
    bridge = FakeBridge()
    client = client_module.UiControlHostClient(
        session_id="maya",
        task_grant_id="grant-1",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        allow_menu_invoke=True,
        bridge=bridge,
    )
    client.snapshot(max_depth=5, max_nodes=250)

    result = client.invoke_menu(["Window", "Arrange", "Left"])

    assert result["verification_required"] is True
    assert result["observation_required"] is True
    assert bridge.calls[-1] == (
        "invoke_menu",
        {
            "session_id": "maya",
            "task_grant_id": "grant-1",
            "window_capability": "cua-window-1",
            "request": {"path": ["Window", "Arrange", "Left"]},
        },
    )
    try:
        client.execute({"action": "click"})
    except client_module.UiControlHostError as exc:
        assert exc.code == "stale_observation"
    else:
        raise AssertionError("native menu invocation must invalidate the observation")


def test_legacy_host_omits_menu_grant_and_fails_capability_check() -> None:
    client_module = _load(_CLIENT_PATH, "_test_cua_legacy_menu_capability")
    bridge = FakeBridge(capabilities=())
    client = client_module.UiControlHostClient(
        session_id="maya",
        task_grant_id="grant-1",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        allow_menu_invoke=True,
        bridge=bridge,
    )

    assert "allow_menu_invoke" not in bridge.calls[0][1]["grant"]
    try:
        client.invoke_menu(["File"])
    except client_module.UiControlHostError as exc:
        assert exc.code == "unsupported_action"
    else:
        raise AssertionError("native menu invocation must require the negotiated capability")


def test_cua_backend_passes_cua_element_token_not_legacy_control_id() -> None:
    backend = _load(_BACKEND_PATH, "_test_cua_cli_windows_backend")
    control = {
        "metadata": {
            "ui_control": {
                "element_index": 7,
                "element_token": "dcc-wuia:snapshot:7",
            }
        }
    }

    payload = backend._action_payload(
        {"action": "click", "control_id": "uia:legacy"},
        False,
        control,
    )

    assert payload["element_token"] == "dcc-wuia:snapshot:7"
    assert "element_index" not in payload
    assert "control_id" not in payload


def test_cua_tree_uses_parent_indices_and_normalizes_driver_frames() -> None:
    client_module = _load(_CLIENT_PATH, "_test_cua_parent_tree")

    root, _focus = client_module._legacy_accessibility_tree(
        {
            "elements": [
                {
                    "element_index": 0,
                    "element_token": "snapshot:0",
                    "depth": 1,
                    "role": "Window",
                    "label": "Fixture",
                },
                {
                    "element_index": 1,
                    "element_token": "snapshot:1",
                    "parent_index": 0,
                    "depth": 3,
                    "role": "Button",
                    "label": "Increment",
                    "frame": {"x": 10, "y": 20, "w": 30, "h": 40},
                },
            ]
        }
    )

    button = root["children"][0]
    assert button["name"] == "Increment"
    assert button["bounds"] == {"x": 10, "y": 20, "width": 30, "height": 40}
