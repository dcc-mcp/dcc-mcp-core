"""Regression coverage for ui-control window recovery."""

from __future__ import annotations

import importlib.util
from typing import Any
from typing import ClassVar

from conftest import REPO_ROOT

_BACKEND_PATH = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control" / "scripts" / "_windows_uia_backend.py"


def _load_backend() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_window_recovery", _BACKEND_PATH)
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


def test_process_scoped_host_session_ignores_transient_uia_root(monkeypatch: Any) -> None:
    backend = _load_backend()
    snapshots = iter((_uia_snapshot(500), _uia_snapshot(501)))

    class FakeHostClient:
        instances: ClassVar[list[FakeHostClient]] = []

        def __init__(self, **kwargs: Any) -> None:
            self.kwargs = kwargs
            self.snapshot_count = 0
            self.stopped = False
            self.__class__.instances.append(self)

        @property
        def target(self) -> dict[str, Any]:
            return {"process_id": 1234, "window_handle": 500, "window_title": "Unreal Editor"}

        def snapshot(self, *, max_depth: int, max_nodes: int) -> dict[str, Any]:
            del max_depth, max_nodes
            self.snapshot_count += 1
            uia = next(snapshots)
            return {
                "type": "snapshot",
                "observation_id": f"500:{self.snapshot_count}",
                "accessibility_state_id": f"accessibility:{self.snapshot_count}",
                "target": self.target,
                "observation": {
                    "observation_id": f"500:{self.snapshot_count}",
                    "window_handle": 500,
                    "process_id": 1234,
                    "window_title": "Unreal Editor",
                    "source_rect": [0, 0, 1280, 720],
                },
                "root": uia["root"],
                "focus_runtime_id": "",
                "node_count": 1,
                "image": {"mime_type": "image/png"},
                "image_bytes": b"png",
            }

        def execute(self, _action: dict[str, Any]) -> dict[str, Any]:
            return {
                "type": "action_completed",
                "success": True,
                "policy_tier": "task_grant",
                "message": "completed",
            }

        def stop(self) -> dict[str, Any]:
            self.stopped = True
            return {"type": "session_stopped", "cleanup_pending": False}

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID", "1234")
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE", raising=False)
    monkeypatch.setattr(backend, "_HostClient", FakeHostClient)

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
    assert len(FakeHostClient.instances) == 1
    assert FakeHostClient.instances[0].kwargs["process_id"] == 1234
    assert FakeHostClient.instances[0].kwargs["window_handle"] is None
    assert FakeHostClient.instances[0].stopped is False
