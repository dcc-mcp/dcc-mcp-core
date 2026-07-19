"""Regression coverage for app-ui window recovery."""

from __future__ import annotations

import importlib.util
import json
from pathlib import Path
from typing import Any

from conftest import REPO_ROOT

_BACKEND_PATH = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "app-ui" / "scripts" / "_windows_uia_backend.py"


def _load_backend() -> Any:
    spec = importlib.util.spec_from_file_location("_test_app_ui_window_recovery", _BACKEND_PATH)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _uia_snapshot(window_handle: int) -> dict[str, Any]:
    return {
        "ok": True,
        "focus_runtime_id": "",
        "node_count": 1,
        "root": {
            "runtime_id": f"42.{window_handle}",
            "fallback_path": "0",
            "name": "Unreal Editor",
            "automation_id": "",
            "class_name": "UnrealWindow",
            "control_type": "ControlType.Window",
            "process_id": 1234,
            "native_window_handle": window_handle,
            "enabled": True,
            "offscreen": False,
            "bounds": {"x": 0, "y": 0, "width": 1280, "height": 720},
            "children": [],
        },
    }


def test_process_scoped_native_session_ignores_transient_uia_root(tmp_path: Path, monkeypatch: Any) -> None:
    backend = _load_backend()
    created_handles = []
    stopped_handles = []
    snapshots = iter((_uia_snapshot(500), _uia_snapshot(501)))

    class FakeComputerUseSession:
        def __init__(self, **kwargs: Any) -> None:
            self.window_handle = kwargs["window_handle"]
            created_handles.append(self.window_handle)

        def start(self) -> str:
            return json.dumps({"success": True, "active": True})

        def status(self) -> str:
            return json.dumps({"success": True, "active": True})

        def screenshot(self) -> tuple[str, bytes | None]:
            if self.window_handle == 501:
                return json.dumps({"success": False, "error": "missing_window"}), None
            return (
                json.dumps(
                    {
                        "success": True,
                        "mime_type": "image/png",
                        "observation": {
                            "observation_id": "500:1",
                            "window_handle": 500,
                            "process_id": 1234,
                            "window_title": "Unreal Editor",
                        },
                    }
                ),
                b"png",
            )

        def act(self, _request_json: str) -> str:
            return json.dumps({"success": True, "requires_new_screenshot": True})

        def stop(self) -> str:
            stopped_handles.append(self.window_handle)
            return json.dumps({"success": True})

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", "1234")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: next(snapshots))

    policy = {"allow_keyboard_shortcuts": True}
    first = backend.snapshot_tool({"session_id": "unreal", "process_id": 1234, "policy": policy})
    assert first["success"] is True

    acted = backend.act_tool(
        {
            "session_id": "unreal",
            "process_id": 1234,
            "action": "keypress",
            "keys": ["1"],
            "snapshot_id": first["context"]["snapshot_id"],
            "policy": policy,
        }
    )
    assert acted["success"] is True

    refreshed = backend.snapshot_tool({"session_id": "unreal", "process_id": 1234, "policy": policy})

    assert refreshed["success"] is True
    assert refreshed["context"]["observation"]["window_handle"] == 500
    assert created_handles == [500]
    assert stopped_handles == []


def test_explicit_window_scope_keeps_session_spec_strict() -> None:
    backend = _load_backend()
    matches_scope = backend._SUPPORT._computer_use_session_matches_scope

    assert not matches_scope(
        (1234, 500, "Unreal Editor", "Unreal Editor"),
        (1234, 501, "Unreal Editor", "Unreal Editor"),
        {"process_ids": [1234], "window_handles": [500]},
    )
