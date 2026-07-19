"""Tests for the bundled app-ui mock skill."""

from __future__ import annotations

import base64
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import threading
import time
from typing import Any
from typing import ClassVar

import pytest

from conftest import REPO_ROOT
from dcc_mcp_core._server.inprocess_executor import run_skill_script

_SKILL_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "app-ui"
_SCRIPTS = _SKILL_DIR / "scripts"


def _load_cdp_runtime_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_app_ui_cdp_runtime", _SCRIPTS / "_cdp_runtime.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_windows_uia_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_app_ui_windows_uia", _SCRIPTS / "_windows_uia_backend.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _run_tool(
    name: str,
    payload: dict[str, Any],
    state_dir: Path,
    extra_env: dict[str, str] | None = None,
) -> dict[str, Any]:
    env = dict(os.environ)
    env["DCC_MCP_APP_UI_MOCK_STATE_DIR"] = str(state_dir)
    if extra_env:
        env.update(extra_env)
    python_path = str(REPO_ROOT / "python")
    if env.get("PYTHONPATH"):
        python_path = python_path + os.pathsep + env["PYTHONPATH"]
    env["PYTHONPATH"] = python_path
    result = subprocess.run(
        [sys.executable, str(_SCRIPTS / f"{name}.py")],
        input=json.dumps(payload),
        capture_output=True,
        text=True,
        timeout=10,
        env=env,
    )
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip(), result.stderr
    return json.loads(result.stdout)


def test_app_ui_windows_uia_spec_loads_isolated_host_client_state() -> None:
    first = _load_windows_uia_module()
    second = _load_windows_uia_module()

    first._CLIENTS["isolated"] = {"client": object()}

    assert "isolated" not in second._CLIENTS


def test_app_ui_skill_metadata_and_tool_names() -> None:
    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    assert meta.name == "app-ui"
    assert {tool.name for tool in meta.tools} == {
        "snapshot",
        "find",
        "act",
        "stop_computer_use",
        "wait_for",
    }
    assert all(tool.requires_in_process for tool in meta.tools)

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])
    catalog.set_in_process_executor(lambda *_args, **_kwargs: {"success": True})
    catalog.load_skill("app-ui")
    action_names = {action["name"] for action in registry.list_actions()}
    assert "app_ui__snapshot" in action_names
    assert "app_ui__wait_for" in action_names
    assert "app_ui__stop_computer_use" in action_names


def test_app_ui_load_fails_loudly_without_persistent_executor() -> None:
    import pytest

    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])

    with pytest.raises(ValueError, match="persistent in-process executor"):
        catalog.load_skill("app-ui")


def test_app_ui_tool_schema_supports_computer_use_actions() -> None:
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    tools = {tool.name: tool for tool in meta.tools}
    schema = json.loads(tools["act"].input_schema)

    assert schema["required"] == ["action"]
    assert set(schema["properties"]["action"]["enum"]) == {
        "click",
        "move",
        "double_click",
        "scroll",
        "drag",
        "raw_coordinate_click",
        "type",
        "keypress",
        "set_text",
        "toggle",
        "set_checked",
        "select_option",
        "focus",
        "keyboard_shortcut",
        "get_window_state",
        "restore_window",
        "show_window",
        "activate_window",
    }
    assert {
        "control_id",
        "text",
        "checked",
        "x",
        "y",
        "button",
        "scroll_x",
        "scroll_y",
        "path",
        "keys",
        "snapshot_id",
    }.issubset(schema["properties"])
    assert schema["properties"]["path"]["items"]["required"] == ["x", "y"]
    assert schema["properties"]["path"]["maxItems"] == 256
    assert schema["properties"]["keys"]["maxItems"] == 16
    assert schema["properties"]["text"]["maxLength"] == 4096
    assert schema["properties"]["scroll_x"]["type"] == "integer"
    assert schema["properties"]["scroll_y"]["type"] == "integer"
    assert "stale_observation" in schema["properties"]["snapshot_id"]["description"]
    assert tools["snapshot"].timeout_hint_secs is None
    assert tools["act"].timeout_hint_secs is None
    assert tools["find"].timeout_hint_secs == 2
    assert tools["wait_for"].timeout_hint_secs == 65
    assert not (tools["act"].next_tools or {}).get("on_failure")
    assert not (tools["wait_for"].next_tools or {}).get("on_failure")
    wait_schema = json.loads(tools["wait_for"].input_schema)
    assert wait_schema["properties"]["condition"]["properties"]["timeout_ms"]["maximum"] == 60_000
    assert tools["stop_computer_use"].requires_in_process is True


def test_app_ui_entrypoints_accept_inprocess_parameters(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    """Sidecar hosts must not require subprocess stdin for bundled app-ui."""
    monkeypatch.setenv("DCC_MCP_APP_UI_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_APP_UI_MOCK_STATE_DIR", str(tmp_path))
    log_dir = tmp_path / "logs"
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(log_dir))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_DCC_TYPE", "unreal")

    snapshot = run_skill_script(
        str(_SCRIPTS / "snapshot.py"),
        {"session_id": "inprocess"},
    )
    snapshot_id = snapshot["context"]["snapshot_id"]

    found = run_skill_script(
        str(_SCRIPTS / "find.py"),
        {"session_id": "inprocess", "label": "Project name"},
    )
    assert found["context"]["matches"][0]["id"] == "project-name"

    changed = run_skill_script(
        str(_SCRIPTS / "act.py"),
        {
            "session_id": "inprocess",
            "control_id": "project-name",
            "action": "set_text",
            "text": "Signal Forge",
            "snapshot_id": snapshot_id,
        },
    )
    assert changed["success"] is True

    waited = run_skill_script(
        str(_SCRIPTS / "wait_for.py"),
        {
            "session_id": "inprocess",
            "condition": {
                "kind": "value_equals",
                "control_id": "project-name",
                "value": "Signal Forge",
                "timeout_ms": 200,
                "interval_ms": 10,
            },
        },
    )
    assert waited["success"] is True

    stopped = run_skill_script(
        str(_SCRIPTS / "stop_computer_use.py"),
        {"session_id": "inprocess"},
    )
    assert stopped["success"] is True
    assert stopped["context"]["active"] is False

    log_text = next(log_dir.glob("dcc-mcp-app-ui.*.log")).read_text(encoding="utf-8")
    audit_rows = [json.loads(line.split(": ", 1)[1]) for line in log_text.splitlines()]
    assert [row["tool"] for row in audit_rows] == [
        "app_ui__snapshot",
        "app_ui__find",
        "app_ui__act",
        "app_ui__wait_for",
        "app_ui__stop_computer_use",
    ]
    assert all(row["event"] == "ui_control_operation" for row in audit_rows)
    assert all(row["dcc_type"] == "unreal" for row in audit_rows)
    assert "Signal Forge" not in log_text


def test_app_ui_admin_audit_records_rejection_without_sensitive_text(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    monkeypatch.setenv("DCC_MCP_APP_UI_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_APP_UI_MOCK_STATE_DIR", str(tmp_path / "state"))
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(tmp_path / "logs"))

    snapshot = run_skill_script(str(_SCRIPTS / "snapshot.py"), {"session_id": "denied"})
    result = run_skill_script(
        str(_SCRIPTS / "act.py"),
        {
            "session_id": "denied",
            "control_id": "project-name",
            "action": "set_text",
            "text": "never-log-this-secret",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "policy": {"allow_text_entry": False},
        },
    )

    assert result["success"] is False
    log_text = next((tmp_path / "logs").glob("dcc-mcp-app-ui.*.log")).read_text(encoding="utf-8")
    assert "never-log-this-secret" not in log_text
    row = json.loads(log_text.splitlines()[-1].split(": ", 1)[1])
    assert row["tool"] == "app_ui__act"
    assert row["success"] is False
    assert row["error"] == "policy_disabled"


def test_app_ui_mock_observe_act_wait_verify_loop(tmp_path: Path) -> None:
    session_id = "loop"
    snapshot = _run_tool("snapshot", {"session_id": session_id}, tmp_path)
    snapshot_id = snapshot["context"]["snapshot_id"]
    assert snapshot["context"]["snapshot"]["root"]["role"] == "window"

    found = _run_tool("find", {"session_id": session_id, "label": "Project name"}, tmp_path)
    assert found["success"] is True
    assert found["context"]["matches"][0]["id"] == "project-name"

    set_text = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "project-name",
            "action": "set_text",
            "text": "Hero",
            "snapshot_id": snapshot_id,
        },
        tmp_path,
    )
    assert set_text["success"] is True
    assert set_text["context"]["audit"]["redacted_fields"] == ["text"]

    waited_for_text = _run_tool(
        "wait_for",
        {
            "session_id": session_id,
            "condition": {
                "kind": "value_equals",
                "control_id": "project-name",
                "value": "Hero",
                "timeout_ms": 200,
                "interval_ms": 10,
            },
        },
        tmp_path,
    )
    assert waited_for_text["success"] is True

    apply_result = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "apply",
            "action": "click",
            "snapshot_id": set_text["context"]["snapshot_id"],
        },
        tmp_path,
    )
    assert apply_result["success"] is True

    waited_for_apply = _run_tool(
        "wait_for",
        {
            "session_id": session_id,
            "condition": {
                "kind": "text_equals",
                "control_id": "status",
                "text": "Applied",
                "timeout_ms": 200,
                "interval_ms": 10,
            },
        },
        tmp_path,
    )
    assert waited_for_apply["success"] is True

    verified = _run_tool("snapshot", {"session_id": session_id}, tmp_path)
    status = next(node for node in verified["context"]["snapshot"]["root"]["children"] if node["id"] == "status")
    assert status["text"] == "Applied"


def test_app_ui_mock_reports_stale_and_policy_denied_paths(tmp_path: Path) -> None:
    session_id = "stale-policy"
    snapshot = _run_tool("snapshot", {"session_id": session_id}, tmp_path)
    old_snapshot_id = snapshot["context"]["snapshot_id"]

    changed = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "project-name",
            "action": "set_text",
            "text": "First",
            "snapshot_id": old_snapshot_id,
        },
        tmp_path,
    )
    assert changed["success"] is True

    stale = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "enable-cache",
            "action": "toggle",
            "snapshot_id": old_snapshot_id,
        },
        tmp_path,
    )
    assert stale["success"] is False
    assert stale["context"]["result"]["error_code"] == "stale_control"
    assert stale["context"]["audit"]["action_kind"] == "toggle"
    assert stale["context"]["audit"]["error_code"] == "stale_control"

    denied = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "project-name",
            "action": "set_text",
            "text": "Secret",
            "snapshot_id": changed["context"]["snapshot_id"],
            "policy": {"allow_text_entry": False},
        },
        tmp_path,
    )
    assert denied["success"] is False
    assert denied["context"]["result"]["error_code"] == "policy_disabled"
    assert denied["context"]["audit"]["redacted_fields"] == ["text"]

    not_found = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "missing-control",
            "action": "click",
            "snapshot_id": changed["context"]["snapshot_id"],
        },
        tmp_path,
    )
    assert not_found["success"] is False
    assert not_found["context"]["result"]["error_code"] == "not_found"
    assert not_found["context"]["audit"]["action_kind"] == "click"
    assert not_found["context"]["audit"]["error_code"] == "not_found"


def test_app_ui_mock_policy_scopes_wait_and_audits_timeout(tmp_path: Path) -> None:
    session_id = "wait-policy"
    denied = _run_tool(
        "wait_for",
        {
            "session_id": session_id,
            "condition": {
                "kind": "text_equals",
                "control_id": "status",
                "text": "Never",
                "timeout_ms": 10,
                "interval_ms": 10,
            },
            "policy": {"allowed_window_titles": ["Other App"]},
        },
        tmp_path,
    )
    assert denied["success"] is False
    assert denied["error"] == "policy_disabled"
    assert denied["context"]["audit"]["action_kind"] == "wait_for"
    assert denied["context"]["audit"]["error_code"] == "policy_disabled"

    timed_out = _run_tool(
        "wait_for",
        {
            "session_id": session_id,
            "condition": {
                "kind": "text_equals",
                "control_id": "status",
                "text": "Never",
                "timeout_ms": 10,
                "interval_ms": 10,
            },
        },
        tmp_path,
    )
    assert timed_out["success"] is False
    assert timed_out["context"]["result"]["error_code"] == "timeout"
    assert timed_out["context"]["audit"]["action_kind"] == "wait_for"
    assert timed_out["context"]["audit"]["target_control_id"] == "status"
    assert timed_out["context"]["audit"]["target_role"] == "label"
    assert timed_out["context"]["audit"]["error_code"] == "timeout"


def test_app_ui_policy_can_leave_observation_enabled_while_actions_disabled(tmp_path: Path) -> None:
    policy = {"allow_mutating_actions": False}
    session_id = "read-only-policy"

    snapshot = _run_tool("snapshot", {"session_id": session_id, "policy": policy}, tmp_path)
    assert snapshot["success"] is True

    found = _run_tool("find", {"session_id": session_id, "label": "Apply", "policy": policy}, tmp_path)
    assert found["success"] is True
    assert found["context"]["matches"][0]["id"] == "apply"

    denied = _run_tool(
        "act",
        {
            "session_id": session_id,
            "control_id": "apply",
            "action": "click",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "policy": policy,
        },
        tmp_path,
    )
    assert denied["success"] is False
    assert denied["context"]["result"]["error_code"] == "policy_disabled"
    assert denied["context"]["audit"]["target_control_id"] == "apply"


def test_app_ui_backend_router_reports_unknown_backend(tmp_path: Path) -> None:
    result = _run_tool(
        "snapshot",
        {"session_id": "bad-backend"},
        tmp_path,
        extra_env={"DCC_MCP_APP_UI_BACKEND": "definitely-not-a-backend"},
    )

    assert result["success"] is False
    assert result["error"] == "backend_unavailable"
    assert result["context"]["supported_backends"] == [
        "mock",
        "chrome",
        "chrome-cdp",
        "cdp",
        "edge",
        "agent-browser",
        "windows-uia",
    ]


class _FakeHostClient:
    instances: ClassVar[list[_FakeHostClient]] = []

    def __init__(self, **kwargs: Any) -> None:
        self.kwargs = kwargs
        self.executed: list[dict[str, Any]] = []
        self.window_operations: list[str] = []
        self.resumed = False
        self.stopped = False
        self.__class__.instances.append(self)

    @property
    def target(self) -> dict[str, Any]:
        return {"process_id": 1234, "window_handle": 500, "window_title": "Godot"}

    def snapshot(self, *, max_depth: int, max_nodes: int) -> dict[str, Any]:
        del max_depth, max_nodes
        return {
            "type": "snapshot",
            "observation_id": "obs-1",
            "accessibility_state_id": "accessibility:1",
            "target": self.target,
            "observation": {
                "observation_id": "obs-1",
                "process_id": 1234,
                "window_handle": 500,
                "window_title": "Godot",
                "source_rect": [0, 0, 640, 480],
            },
            "root": {
                "runtime_id": "42.1",
                "fallback_path": "0",
                "name": "Godot",
                "automation_id": "",
                "class_name": "Godot",
                "control_type": "ControlType.Window",
                "is_password": False,
                "process_id": 1234,
                "native_window_handle": 500,
                "enabled": True,
                "offscreen": False,
                "bounds": {"x": 0, "y": 0, "width": 640, "height": 480},
                "children": [
                    {
                        "runtime_id": "42.2",
                        "fallback_path": "0.0",
                        "name": "Apply",
                        "automation_id": "applyButton",
                        "class_name": "Button",
                        "control_type": "ControlType.Button",
                        "is_password": False,
                        "process_id": 1234,
                        "native_window_handle": 501,
                        "enabled": True,
                        "offscreen": False,
                        "bounds": {"x": 20, "y": 20, "width": 80, "height": 24},
                        "children": [],
                    }
                ],
            },
            "focus_runtime_id": "42.2",
            "node_count": 2,
            "image": {"mime_type": "image/png"},
            "image_bytes": b"png",
        }

    def execute(self, action: dict[str, Any]) -> dict[str, Any]:
        self.executed.append(action)
        return {
            "type": "action_completed",
            "success": True,
            "policy_tier": "task_grant",
            "message": "completed",
        }

    def window_state(self) -> dict[str, Any]:
        return {
            "type": "window_state",
            "state": {
                "process_id": 1234,
                "window_handle": 500,
                "exists": True,
                "visible": True,
                "minimized": True,
                "foreground": False,
            },
        }

    def change_window_state(self, operation: str) -> dict[str, Any]:
        self.window_operations.append(operation)
        return {
            "type": "window_state_changed",
            "operation": operation,
            "state": {
                "process_id": 1234,
                "window_handle": 500,
                "exists": True,
                "visible": True,
                "minimized": False,
                "foreground": operation == "activate",
            },
        }

    def resume(self) -> None:
        self.resumed = True

    def stop(self) -> dict[str, Any]:
        self.stopped = True
        return {"type": "session_stopped", "cleanup_pending": False}


def _configure_fake_host(backend: Any, monkeypatch: Any, *, raw: bool = False) -> None:
    _FakeHostClient.instances.clear()
    monkeypatch.setattr(backend, "_HostClient", _FakeHostClient)
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", "1234")
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)
    if raw:
        monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "1")
    else:
        monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)


def test_app_ui_windows_host_maps_snapshot_and_shared_image(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "godot", "app_name": "Godot"})

    assert result["success"] is True
    context = result["context"]
    assert context["snapshot_id"] == "accessibility:1"
    assert context["snapshot"]["root"]["id"] == "uia:42.1"
    assert context["snapshot"]["root"]["children"][0]["role"] == "button"
    assert context["snapshot"]["metadata"]["app_ui"]["backend"] == "windows-ui-control-host"
    assert context["snapshot"]["metadata"]["computer_use"]["observation_id"] == "obs-1"
    assert base64.b64decode(context["__rich__"]["data"]) == b"png"
    assert _FakeHostClient.instances[0].kwargs["allow_raw_input"] is False


def test_app_ui_windows_host_requires_operator_bound_scope(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _FakeHostClient.instances.clear()
    monkeypatch.setattr(backend, "_HostClient", _FakeHostClient)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)

    result = backend.snapshot_tool({"session_id": "untrusted", "process_id": 1234})

    assert result["success"] is False
    assert result["error"] == "permission_denied"
    assert not _FakeHostClient.instances


def test_app_ui_windows_host_scope_cannot_be_widened(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "wrong", "process_id": 7})

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert not _FakeHostClient.instances


def test_app_ui_windows_host_raw_input_is_runtime_ceiling(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    denied = backend.snapshot_tool(
        {
            "session_id": "denied",
            "policy": {"allow_raw_coordinates": True, "allow_keyboard_shortcuts": True},
        }
    )
    assert denied["success"] is True
    assert denied["context"]["policy"]["allow_raw_coordinates"] is False
    assert _FakeHostClient.instances[-1].kwargs["allow_raw_input"] is False

    enabled_backend = _load_windows_uia_module()
    _configure_fake_host(enabled_backend, monkeypatch, raw=True)
    enabled = enabled_backend.snapshot_tool({"session_id": "enabled"})
    assert enabled["context"]["policy"]["allow_raw_coordinates"] is True
    assert _FakeHostClient.instances[-1].kwargs["allow_raw_input"] is True


def test_app_ui_windows_host_semantic_action_is_thin_proxy(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)
    snapshot = backend.snapshot_tool({"session_id": "semantic"})
    snapshot_id = snapshot["context"]["snapshot_id"]

    result = backend.act_tool(
        {
            "session_id": "semantic",
            "snapshot_id": snapshot_id,
            "control_id": "uia:42.2",
            "action": "click",
            "intent": "external_communication",
        }
    )

    assert result["success"] is True
    payload = _FakeHostClient.instances[0].executed[0]
    assert payload["input_kind"] == "semantic"
    assert payload["control_id"] == "uia:42.2"
    assert payload["intent"] == "external_communication"
    source = (_SCRIPTS / "_windows_uia_backend.py").read_text(encoding="utf-8")
    assert "ComputerUseSession" not in source
    assert "subprocess" not in source
    assert "powershell" not in source.lower()


def test_app_ui_windows_host_native_action_requires_fresh_snapshot(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch, raw=True)

    missing = backend.act_tool({"session_id": "raw", "action": "raw_coordinate_click", "x": 10, "y": 20})
    assert missing["success"] is False
    assert missing["error"] == "stale_observation"

    snapshot = backend.snapshot_tool({"session_id": "raw"})
    completed = backend.act_tool(
        {
            "session_id": "raw",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "action": "raw_coordinate_click",
            "x": 10,
            "y": 20,
        }
    )
    assert completed["success"] is True
    assert _FakeHostClient.instances[0].executed[0]["input_kind"] == "raw_input"

    replayed = backend.act_tool(
        {
            "session_id": "raw",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "action": "raw_coordinate_click",
            "x": 10,
            "y": 20,
        }
    )
    assert replayed["success"] is False
    assert replayed["error"] == "stale_observation"


def test_app_ui_windows_host_restores_minimized_exact_window_without_snapshot(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    state = backend.act_tool({"session_id": "minimized", "action": "get_window_state"})
    restored = backend.act_tool({"session_id": "minimized", "action": "restore_window"})

    assert state["success"] is True
    assert state["context"]["window_state"]["minimized"] is True
    assert restored["success"] is True
    assert restored["context"]["window_state"]["minimized"] is False
    assert _FakeHostClient.instances[0].window_operations == ["restore"]
    assert _FakeHostClient.instances[0].executed == []
    assert state["context"]["audit"]["metadata"]["host_enforced"] is True


def test_app_ui_windows_host_invalid_snapshot_target_exposes_scoped_recovery() -> None:
    backend = _load_windows_uia_module()

    result = backend._host_error(backend.UiControlHostError("invalid_target", "window is not capturable"))

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert result["context"]["recovery_scope"] == "same_exact_pid_hwnd"
    assert result["context"]["recovery_actions"] == [
        "get_window_state",
        "restore_window",
        "show_window",
        "activate_window",
    ]
    assert "cannot change the authorized PID/HWND scope" in result["prompt"]


def test_app_ui_windows_host_propagates_trusted_confirmation_denial(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()

    class DenyingHost(_FakeHostClient):
        def execute(self, action: dict[str, Any]) -> dict[str, Any]:
            self.executed.append(action)
            return {
                "type": "action_completed",
                "success": False,
                "policy_tier": "action_confirmation",
                "error": "approval_required",
                "message": "the user did not approve this action",
            }

    _configure_fake_host(backend, monkeypatch)
    monkeypatch.setattr(backend, "_HostClient", DenyingHost)
    snapshot = backend.snapshot_tool({"session_id": "confirm"})
    result = backend.act_tool(
        {
            "session_id": "confirm",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "control_id": "uia:42.2",
            "action": "click",
        }
    )

    assert result["success"] is False
    assert result["error"] == "approval_required"
    assert result["context"]["result"]["metadata"]["policy_tier"] == "action_confirmation"


def test_app_ui_windows_host_find_wait_stop_and_cleanup(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    found = backend.find_tool({"session_id": "workflow", "role": "button"})
    assert found["success"] is True
    assert found["context"]["matches"][0]["id"] == "uia:42.2"

    waited = backend.wait_for_tool(
        {
            "session_id": "workflow",
            "condition": {"kind": "control_exists", "control_id": "uia:42.2", "timeout_ms": 50},
        }
    )
    assert waited["success"] is True

    stopped = backend.stop_computer_use_tool({"session_id": "workflow"})
    assert stopped["success"] is True
    assert _FakeHostClient.instances[0].stopped is True
    assert backend._CLIENTS == {}

    backend.cleanup()
    assert backend._STOP_EVENT.is_set()


def test_app_ui_windows_host_resume_always_round_trips_to_trusted_surface(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "resume", "resume_computer_use": True})

    assert result["success"] is True
    assert _FakeHostClient.instances[0].resumed is True


def test_app_ui_windows_uia_script_retains_hard_target_boundaries() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "Denied-Target-Reason" in script
    assert "Denied-Action-Target-Reason" in script
    assert "$currentInfo.IsPassword" in script
    assert "is_password = [bool]$current.IsPassword" in script
    assert "Cross-process descendant controls are not allowed" in script
    assert "Windows Run dialog is not an allowed" in script


def test_ui_control_host_client_wire_has_no_approval_boolean() -> None:
    import io
    import struct

    backend = _load_windows_uia_module()
    client_module = backend._HOST

    def frame(value: dict[str, Any]) -> bytes:
        body = json.dumps(value).encode("utf-8")
        return struct.pack(">I", len(body)) + body

    class ScriptedPipe:
        def __init__(self) -> None:
            self.responses = io.BytesIO(
                frame({"type": "hello", "protocol_version": 1, "capabilities": []})
                + frame(
                    {
                        "type": "session_opened",
                        "session_id": "wire",
                        "window_capability": "window:opaque",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "DCC"},
                    }
                )
            )
            self.requests = bytearray()

        def read(self, length: int) -> bytes:
            return self.responses.read(length)

        def write(self, data: bytes) -> int:
            self.requests.extend(data)
            return len(data)

        def close(self) -> None:
            return None

    stream = ScriptedPipe()
    client = client_module.UiControlHostClient(
        session_id="wire",
        task_grant_id="grant",
        dcc_type="unreal",
        process_id=42,
        window_handle=500,
        allow_raw_input=True,
        stream=stream,
    )

    assert client.target["window_handle"] == 500
    wire = bytes(stream.requests).decode("utf-8", errors="ignore").lower()
    assert "approved" not in wire
    assert "confirmed" not in wire
    assert "window:opaque" not in wire


def test_app_ui_chrome_cdp_preset_aliases(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_APP_UI_CDP_PRESET", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_CHROME_PRESET", raising=False)
    assert cdp_runtime.cdp_preset() == "reuse"

    monkeypatch.setenv("DCC_MCP_APP_UI_CDP_PRESET", "aurora")
    assert cdp_runtime.cdp_preset() == "auroraview"

    monkeypatch.setenv("DCC_MCP_APP_UI_CDP_PRESET", "temp")
    assert cdp_runtime.cdp_preset() == "isolated"

    monkeypatch.setenv("DCC_MCP_APP_UI_CDP_PRESET", "msedge")
    assert cdp_runtime.cdp_preset() == "edge"

    monkeypatch.setenv("DCC_MCP_APP_UI_CDP_PRESET", "agent_browser")
    assert cdp_runtime.cdp_preset() == "agent-browser"


def test_app_ui_auroraview_preset_uses_auroraview_port(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_APP_UI_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_CHROME_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_CDP_PORT", raising=False)
    monkeypatch.setenv("AURORAVIEW_CDP_PORT", "9333")

    assert cdp_runtime.endpoint_candidates("auroraview") == [
        "http://127.0.0.1:9333",
        "http://127.0.0.1:9222",
    ]


def test_app_ui_edge_preset_uses_edge_port(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_APP_UI_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_EDGE_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_CDP_PORT", raising=False)
    monkeypatch.setenv("DCC_MCP_APP_UI_EDGE_CDP_PORT", "9444")

    assert cdp_runtime.endpoint_candidates("edge") == [
        "http://127.0.0.1:9444",
        "http://127.0.0.1:9222",
    ]


def test_app_ui_agent_browser_preset_parses_cdp_url(tmp_path: Path, monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()
    script = tmp_path / ("agent-browser.cmd" if os.name == "nt" else "agent-browser")
    if os.name == "nt":
        script.write_text("@echo off\necho ws://127.0.0.1:9777/devtools/page/ci\n", encoding="utf-8")
    else:
        script.write_text("#!/bin/sh\necho ws://127.0.0.1:9777/devtools/page/ci\n", encoding="utf-8")
        script.chmod(0o755)
    monkeypatch.setenv("DCC_MCP_APP_UI_AGENT_BROWSER_BIN", str(script))

    assert cdp_runtime._agent_browser_cdp_url() == "ws://127.0.0.1:9777/devtools/page/ci"
