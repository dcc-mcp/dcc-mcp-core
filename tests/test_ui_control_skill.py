"""Tests for the bundled ui-control mock skill."""

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

_SKILL_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control"
_SCRIPTS = _SKILL_DIR / "scripts"


def _load_cdp_runtime_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_cdp_runtime", _SCRIPTS / "_cdp_runtime.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_windows_uia_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_windows_uia", _SCRIPTS / "_windows_uia_backend.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_entrypoint_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_entrypoint", _SCRIPTS / "_entrypoint.py")
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
    env["DCC_MCP_UI_CONTROL_BACKEND"] = "mock"
    env["DCC_MCP_UI_CONTROL_MOCK_STATE_DIR"] = str(state_dir)
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


def test_ui_control_windows_uia_spec_loads_isolated_host_client_state() -> None:
    first = _load_windows_uia_module()
    second = _load_windows_uia_module()

    first._CLIENTS["isolated"] = {"client": object()}

    assert "isolated" not in second._CLIENTS


def test_ui_control_skill_metadata_and_tool_names() -> None:
    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry
    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(_SKILL_DIR))
    assert meta is not None
    assert meta.name == "ui-control"
    assert {tool.name for tool in meta.tools} == {
        "snapshot",
        "find",
        "act",
        "record_clip",
        "system_operation",
        "stop_computer_use",
        "wait_for",
    }
    assert all(tool.requires_in_process for tool in meta.tools)

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])
    catalog.set_in_process_executor(lambda *_args, **_kwargs: {"success": True})
    catalog.load_skill("ui-control")
    action_names = {action["name"] for action in registry.list_actions()}
    assert "ui_control__snapshot" in action_names
    assert "ui_control__record_clip" in action_names
    assert "ui_control__system_operation" in action_names
    assert "ui_control__wait_for" in action_names
    assert "ui_control__stop_computer_use" in action_names


def test_ui_control_load_fails_loudly_without_persistent_executor() -> None:
    import pytest

    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])

    with pytest.raises(ValueError, match="persistent in-process executor"):
        catalog.load_skill("ui-control")


def test_ui_control_tool_schema_supports_computer_use_actions() -> None:
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
        "game_navigation",
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
    keys_description = schema["properties"]["keys"]["description"]
    assert "pointer actions" in keys_description
    assert "navigation/control/function" in keys_description
    assert "exactly one unmodified W, A, S, or D" in keys_description
    assert all(modifier in keys_description for modifier in ("Ctrl", "Shift", "Alt"))
    assert "latest screenshot" in schema["properties"]["path"]["description"]
    assert "semantic lookup fails" in schema["properties"]["path"]["description"]
    assert schema["properties"]["text"]["maxLength"] == 4096
    assert "Windows hard-denies raw type" in tools["act"].description
    assert "exact control_id" in schema["properties"]["text"]["description"]
    assert "secure user/host hand-off" in schema["properties"]["text"]["description"]
    assert schema["properties"]["scroll_x"]["type"] == "integer"
    assert schema["properties"]["scroll_y"]["type"] == "integer"
    assert "pointer-effect dwell" in schema["properties"]["duration_ms"]["description"]
    assert "0 and 500 ms" in schema["properties"]["duration_ms"]["description"]
    assert "stale_observation" in schema["properties"]["snapshot_id"]["description"]
    assert tools["snapshot"].timeout_hint_secs is None
    assert tools["act"].timeout_hint_secs is None
    assert tools["find"].timeout_hint_secs == 2
    assert tools["wait_for"].timeout_hint_secs == 65
    record_schema = json.loads(tools["record_clip"].input_schema)
    assert record_schema["required"] == ["duration_ms"]
    assert record_schema["additionalProperties"] is False
    assert record_schema["properties"]["duration_ms"]["minimum"] == 1_000
    assert record_schema["properties"]["duration_ms"]["maximum"] == 180_000
    assert record_schema["properties"]["frames_per_second"]["minimum"] == 1
    assert record_schema["properties"]["frames_per_second"]["maximum"] == 60
    assert record_schema["properties"]["jpeg_quality"]["minimum"] == 70
    assert record_schema["properties"]["jpeg_quality"]["maximum"] == 100
    assert not {"output", "output_path", "directory", "path"} & set(record_schema["properties"])
    assert tools["record_clip"].timeout_hint_secs == 185
    assert tools["record_clip"].read_only is False
    assert tools["record_clip"].destructive is False
    assert tools["record_clip"].requires_in_process is True
    assert not (tools["act"].next_tools or {}).get("on_failure")
    assert not (tools["wait_for"].next_tools or {}).get("on_failure")
    wait_schema = json.loads(tools["wait_for"].input_schema)
    assert wait_schema["properties"]["condition"]["properties"]["timeout_ms"]["maximum"] == 60_000
    assert tools["stop_computer_use"].requires_in_process is True
    system_schema = json.loads(tools["system_operation"].input_schema)
    assert system_schema["required"] == ["operation_id"]
    assert system_schema["additionalProperties"] is False
    operation_id_schema = system_schema["properties"]["operation_id"]
    assert operation_id_schema["type"] == "string"
    assert operation_id_schema["minLength"] == 1
    assert operation_id_schema["maxLength"] == 256
    assert not {"operation", "command", "allowlist", "system_grant_id", "value", "path"} & set(
        system_schema["properties"]
    )
    assert "Values, paths, commands, grants" in operation_id_schema["description"]
    assert tools["system_operation"].idempotent is True
    assert tools["system_operation"].requires_in_process is True


def test_ui_control_windows_game_navigation_contract_is_fail_closed() -> None:
    backend = _load_windows_uia_module()

    for key in ("W", "a", "S", "d"):
        assert backend._validate_action_limits({"action": "game_navigation", "keys": [key], "duration_ms": 500}) is None
        assert backend._is_native_action("game_navigation", {"keys": [key]}) is True

    for payload in (
        {"action": "game_navigation", "keys": []},
        {"action": "game_navigation", "keys": ["W", "D"]},
        {"action": "game_navigation", "keys": ["SHIFT+W"]},
        {"action": "game_navigation", "keys": ["LEFT"]},
        {"action": "game_navigation", "keys": ["W"], "duration_ms": -1},
        {"action": "game_navigation", "keys": ["W"], "duration_ms": 501},
        {"action": "game_navigation", "keys": ["W"], "duration_ms": True},
    ):
        result = backend._validate_action_limits(payload)
        assert result is not None
        assert result["success"] is False
        assert result["error"] == "invalid_action"


def test_ui_control_entrypoints_accept_inprocess_parameters(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    """Sidecar hosts must not require subprocess stdin for bundled ui-control."""
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", str(tmp_path))
    log_dir = tmp_path / "logs"
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(log_dir))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_DCC_TYPE", "unreal")

    snapshot = run_skill_script(
        str(_SCRIPTS / "snapshot.py"),
        {"session_id": "inprocess"},
    )
    snapshot_id = snapshot["context"]["snapshot_id"]
    assert snapshot["context"]["capture_provenance"] == {
        "tool": "ui_control__snapshot",
        "backend": "mock",
        "session_id": "inprocess",
        "pixels_captured": False,
        "snapshot_id": snapshot_id,
    }

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
    changed_snapshot_id = changed["context"]["snapshot_id"]
    assert changed_snapshot_id != snapshot_id

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

    log_text = next(log_dir.glob("dcc-mcp-ui-control.*.log")).read_text(encoding="utf-8")
    audit_rows = [json.loads(line.split(": ", 1)[1]) for line in log_text.splitlines()]
    assert [row["tool"] for row in audit_rows] == [
        "ui_control__snapshot",
        "ui_control__find",
        "ui_control__act",
        "ui_control__wait_for",
        "ui_control__stop_computer_use",
    ]
    assert all(row["event"] == "ui_control_operation" for row in audit_rows)
    assert all(row["dcc_type"] == "unreal" for row in audit_rows)
    assert audit_rows[0]["snapshot_id"] == snapshot_id
    assert audit_rows[0]["backend"] == "mock"
    assert audit_rows[0]["pixels_captured"] is False
    assert audit_rows[2]["snapshot_id"] == changed_snapshot_id
    assert "Signal Forge" not in log_text


def test_ui_control_entrypoint_reports_real_snapshot_provenance(monkeypatch: Any) -> None:
    entrypoint = _load_entrypoint_module()

    class Backend:
        @staticmethod
        def snapshot_tool(_params: dict[str, Any]) -> dict[str, Any]:
            return {
                "success": True,
                "message": "Captured isolated Windows UI Control snapshot.",
                "context": {
                    "session_id": "evidence",
                    "snapshot_id": "accessibility:1",
                    "snapshot": {
                        "metadata": {
                            "ui_control": {"backend": "windows-ui-control-host"},
                        }
                    },
                    "observation": {
                        "observation_id": "obs-1",
                        "process_id": 1234,
                        "window_handle": 500,
                        "width": 1600,
                        "height": 900,
                        "source_rect": [20, 30, 1920, 1080],
                        "capture_backend": "windows-graphics-capture",
                    },
                    "__rich__": {"kind": "image", "data": "png"},
                },
            }

        @staticmethod
        def record_clip_tool(_params: dict[str, Any]) -> dict[str, Any]:
            return {
                "success": True,
                "message": "Recorded an exact-window JPEG sequence.",
                "context": {
                    "session_id": "evidence",
                    "target": {"process_id": 1234, "window_handle": 500},
                    "artifact": {
                        "recording_id": "clip-1",
                        "frame_count": 90,
                        "width": 1280,
                        "height": 720,
                        "manifest_sha256": "a" * 64,
                    },
                },
            }

    monkeypatch.setattr(entrypoint, "_load_backend", lambda: Backend)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "windows-uia")
    monkeypatch.setenv("DCC_MCP_DISABLE_FILE_LOGGING", "1")

    result = entrypoint.snapshot_tool({"session_id": "evidence"})

    provenance = result["context"]["capture_provenance"]
    assert provenance == {
        "tool": "ui_control__snapshot",
        "backend": "windows-ui-control-host",
        "session_id": "evidence",
        "snapshot_id": "accessibility:1",
        "observation_id": "obs-1",
        "process_id": 1234,
        "window_handle": 500,
        "capture_backend": "windows-graphics-capture",
        "pixels_captured": True,
        "width": 1600,
        "height": 900,
        "source_width": 1920,
        "source_height": 1080,
        "downscaled": True,
    }
    assert "windows-ui-control-host" in result["message"]
    assert "1600x900" in result["message"]
    assert "downscaled from 1920x1080" in result["message"]

    clip = entrypoint.record_clip_tool({"session_id": "evidence"})
    assert clip["context"]["capture_provenance"] == {
        "tool": "ui_control__record_clip",
        "backend": "windows-ui-control-host",
        "session_id": "evidence",
        "pixels_captured": True,
        "process_id": 1234,
        "window_handle": 500,
        "recording_id": "clip-1",
        "frame_count": 90,
        "width": 1280,
        "height": 720,
        "manifest_sha256": "a" * 64,
    }


def test_ui_control_subprocess_forwards_action_to_windows_backend_without_host(tmp_path: Path) -> None:
    """Standalone-server stdin transport must preserve schema key ``action``."""
    result = _run_tool(
        "act",
        {
            "session_id": "subprocess-action-transport",
            "process_id": 424242,
            "window_handle": 31337,
            "window_title": "transport-probe",
            "action": "keyboard_shortcut",
            "intent": "navigate",
            "keys": ["ALT", "F4"],
            "snapshot_id": "accessibility:probe",
            "policy": {"allow_keyboard_shortcuts": False},
        },
        tmp_path,
        {
            "DCC_MCP_UI_CONTROL_BACKEND": "windows-uia",
            "DCC_MCP_DISABLE_FILE_LOGGING": "1",
        },
    )

    assert result["success"] is False
    assert result["error"] == "policy_disabled"
    assert result["message"] == "ui_control action 'keyboard_shortcut' disabled by policy"


def test_ui_control_admin_audit_records_rejection_without_sensitive_text(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "mock")
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR", str(tmp_path / "state"))
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
    log_text = next((tmp_path / "logs").glob("dcc-mcp-ui-control.*.log")).read_text(encoding="utf-8")
    assert "never-log-this-secret" not in log_text
    row = json.loads(log_text.splitlines()[-1].split(": ", 1)[1])
    assert row["tool"] == "ui_control__act"
    assert row["success"] is False
    assert row["error"] == "policy_disabled"


def test_ui_control_mock_observe_act_wait_verify_loop(tmp_path: Path) -> None:
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


def test_ui_control_mock_reports_stale_and_policy_denied_paths(tmp_path: Path) -> None:
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


def test_ui_control_mock_policy_scopes_wait_and_audits_timeout(tmp_path: Path) -> None:
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


def test_ui_control_policy_can_leave_observation_enabled_while_actions_disabled(tmp_path: Path) -> None:
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


def test_ui_control_backend_router_reports_unknown_backend(tmp_path: Path) -> None:
    result = _run_tool(
        "snapshot",
        {"session_id": "bad-backend"},
        tmp_path,
        extra_env={"DCC_MCP_UI_CONTROL_BACKEND": "definitely-not-a-backend"},
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


@pytest.mark.parametrize(
    ("backend", "reported_backend"),
    [("mock", "mock"), ("chrome", "chrome-cdp")],
)
def test_ui_control_non_native_backends_reject_exact_window_recording(
    tmp_path: Path,
    backend: str,
    reported_backend: str,
) -> None:
    result = _run_tool(
        "record_clip",
        {"session_id": "no-recording-fallback", "duration_ms": 1_000},
        tmp_path,
        extra_env={"DCC_MCP_UI_CONTROL_BACKEND": backend},
    )

    assert result["success"] is False
    assert result["error"] == "unsupported_action"
    assert result["context"]["backend"] == reported_backend


class _FakeHostClient:
    instances: ClassVar[list[_FakeHostClient]] = []

    def __init__(self, **kwargs: Any) -> None:
        self.kwargs = kwargs
        self.executed: list[dict[str, Any]] = []
        self.window_operations: list[str] = []
        self.recordings: list[dict[str, int]] = []
        self.snapshot_calls = 0
        self.accessibility_snapshot_calls = 0
        self.resumed = False
        self.stopped = False
        self.__class__.instances.append(self)

    @property
    def target(self) -> dict[str, Any]:
        return {"process_id": 1234, "window_handle": 500, "window_title": "Godot"}

    def snapshot(self, *, max_depth: int, max_nodes: int) -> dict[str, Any]:
        del max_depth, max_nodes
        self.snapshot_calls += 1
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

    def accessibility_snapshot(self, *, max_depth: int, max_nodes: int) -> dict[str, Any]:
        previous_snapshot_calls = self.snapshot_calls
        snapshot = self.snapshot(max_depth=max_depth, max_nodes=max_nodes)
        self.snapshot_calls = previous_snapshot_calls
        self.accessibility_snapshot_calls += 1
        return {
            "type": "accessibility_snapshot",
            "accessibility_state_id": snapshot["accessibility_state_id"],
            "target": snapshot["target"],
            "root": snapshot["root"],
            "focus_runtime_id": snapshot["focus_runtime_id"],
            "node_count": snapshot["node_count"],
        }

    def execute(self, action: dict[str, Any]) -> dict[str, Any]:
        self.executed.append(action)
        return {
            "type": "action_completed",
            "success": True,
            "policy_tier": "task_grant",
            "message": "completed",
        }

    def record_clip(self, *, duration_ms: int, frames_per_second: int, jpeg_quality: int) -> dict[str, Any]:
        self.recordings.append(
            {
                "duration_ms": duration_ms,
                "frames_per_second": frames_per_second,
                "jpeg_quality": jpeg_quality,
            }
        )
        return {
            "type": "clip_recorded",
            "target": self.target,
            "artifact": {
                "recording_id": "clip-test",
                "directory": "C:/host-owned/clip-test",
                "manifest_path": "C:/host-owned/clip-test/manifest.json",
                "frame_pattern": "frame-%06d.jpg",
                "frame_count": 36,
                "width": 1280,
                "height": 720,
                "frames_per_second": frames_per_second,
                "started_at_ms": 1000,
                "ended_at_ms": 2200,
                "manifest_sha256": "a" * 64,
            },
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
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID", "1234")
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE", raising=False)
    if raw:
        monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "1")
    else:
        monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)


def test_ui_control_windows_host_maps_snapshot_and_shared_image(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "godot", "app_name": "Godot"})

    assert result["success"] is True
    context = result["context"]
    assert context["snapshot_id"] == "accessibility:1"
    assert context["snapshot"]["root"]["id"] == "uia:42.1"
    assert context["snapshot"]["root"]["children"][0]["role"] == "button"
    assert context["snapshot"]["metadata"]["ui_control"]["backend"] == "windows-ui-control-host"
    assert context["snapshot"]["metadata"]["computer_use"]["observation_id"] == "obs-1"
    assert base64.b64decode(context["__rich__"]["data"]) == b"png"
    assert _FakeHostClient.instances[0].kwargs["allow_raw_input"] is False


def test_ui_control_windows_host_records_exact_window_clip_without_output_path(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.record_clip_tool(
        {
            "session_id": "pv",
            "duration_ms": 1_200,
            "frames_per_second": 30,
            "jpeg_quality": 92,
        }
    )

    assert result["success"] is True
    assert result["context"]["target"]["window_handle"] == 500
    assert result["context"]["artifact"]["manifest_sha256"] == "a" * 64
    assert _FakeHostClient.instances[0].recordings == [
        {"duration_ms": 1_200, "frames_per_second": 30, "jpeg_quality": 92}
    ]


def test_ui_control_windows_host_requires_operator_bound_scope(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _FakeHostClient.instances.clear()
    monkeypatch.setattr(backend, "_HostClient", _FakeHostClient)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE", raising=False)

    result = backend.snapshot_tool({"session_id": "untrusted", "process_id": 1234})

    assert result["success"] is False
    assert result["error"] == "permission_denied"
    assert not _FakeHostClient.instances


def test_ui_control_windows_host_scope_cannot_be_widened(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "wrong", "process_id": 7})

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert not _FakeHostClient.instances


def test_ui_control_windows_host_raw_input_is_runtime_ceiling(monkeypatch: Any) -> None:
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


def test_ui_control_windows_host_semantic_action_is_thin_proxy(monkeypatch: Any) -> None:
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


def test_ui_control_windows_uia_post_state_is_optional_after_semantic_success() -> None:
    source = (_SCRIPTS / "_windows_uia_backend.ps1").read_text(encoding="utf-8")
    invoke = source.index("$actionResult = Invoke-Action $target")
    reject_failure = source.index("if (-not [bool]$actionResult.ok)", invoke)
    optional_control = source.index("$control = $null", reject_failure)
    success = source.index("ok = $true", optional_control)
    post_read = source[optional_control:success]

    assert invoke < reject_failure < optional_control < success
    assert "ok = $false" in source[reject_failure:optional_control]
    assert 'try {\n      $control = Element-Raw $target 0 "target"\n    } catch {}' in post_read
    assert "control = $control" in source[success:]


def test_ui_control_windows_host_native_action_requires_fresh_snapshot(monkeypatch: Any) -> None:
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


def test_ui_control_windows_host_restores_minimized_exact_window_without_snapshot(monkeypatch: Any) -> None:
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


def test_ui_control_windows_host_reports_closed_target_success_and_requires_explicit_rebind(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()

    class ClosingTargetHost(_FakeHostClient):
        def execute(self, action: dict[str, Any]) -> dict[str, Any]:
            self.executed.append(action)
            return {
                "type": "action_completed",
                "success": True,
                "target_closed": True,
                "policy_tier": "task_grant",
                "message": "completed; the exact target window closed",
            }

    _configure_fake_host(backend, monkeypatch)
    monkeypatch.setattr(backend, "_HostClient", ClosingTargetHost)
    snapshot = backend.snapshot_tool({"session_id": "transition"})
    result = backend.act_tool(
        {
            "session_id": "transition",
            "snapshot_id": snapshot["context"]["snapshot_id"],
            "control_id": "uia:42.2",
            "action": "click",
        }
    )

    assert result["success"] is True
    assert result["context"]["target_closed"] is True
    assert result["context"]["session_active"] is False
    assert result["context"]["result"]["metadata"]["target_closed"] is True
    assert result["context"]["result"]["metadata"]["requires_new_screenshot"] is False
    assert result["context"]["audit"]["metadata"]["target_closed"] is True
    assert "Explicitly bind the intended new PID/HWND" in result["prompt"]
    assert backend._CLIENTS == {}


def test_ui_control_windows_host_invalid_snapshot_target_exposes_scoped_recovery() -> None:
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


def test_ui_control_windows_host_protected_system_ui_requires_manual_operator_recovery() -> None:
    backend = _load_windows_uia_module()

    result = backend._host_error(
        backend.UiControlHostError(
            "invalid_target",
            "the requested pointer coordinate remains blocked by protected system UI: PickerHost / Shell_SystemDim",
        )
    )

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert result["context"]["recovery_scope"] == "same_exact_pid_hwnd"
    assert result["context"]["recovery_actions"] == ["stop", "snapshot"]
    assert "ask the operator to close or move" in result["prompt"]
    assert "Do not hide, override, click through, or ignore" in result["prompt"]
    assert "fresh ui_control__snapshot" in result["prompt"]


def test_ui_control_windows_host_propagates_trusted_confirmation_denial(monkeypatch: Any) -> None:
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


def test_ui_control_windows_host_find_wait_stop_and_cleanup(monkeypatch: Any) -> None:
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
    assert _FakeHostClient.instances[0].accessibility_snapshot_calls == 1

    stopped = backend.stop_computer_use_tool({"session_id": "workflow"})
    assert stopped["success"] is True
    assert _FakeHostClient.instances[0].stopped is True
    assert backend._CLIENTS == {}

    backend.cleanup()
    assert backend._STOP_EVENT.is_set()


def test_ui_control_windows_find_reuses_latest_unconsumed_snapshot(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    snapshot = backend.snapshot_tool({"session_id": "cached"})
    found = backend.find_tool({"session_id": "cached", "role": "button"})

    assert snapshot["success"] is True
    assert found["success"] is True
    assert found["context"]["snapshot_id"] == snapshot["context"]["snapshot_id"]
    assert _FakeHostClient.instances[0].snapshot_calls == 1


def test_ui_control_windows_expires_idle_session_without_touching_active_call(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)
    clock = iter([0.0, 0.0, 10.0, 10.0])
    monkeypatch.setattr(backend, "_IDLE_LEASE_SECONDS", 5.0)
    monkeypatch.setattr(backend.time, "monotonic", lambda: next(clock))

    assert backend.snapshot_tool({"session_id": "idle"})["success"] is True
    idle_client = _FakeHostClient.instances[0]
    assert backend.snapshot_tool({"session_id": "active"})["success"] is True

    assert idle_client.stopped is True
    assert "idle" not in backend._CLIENTS
    assert "active" in backend._CLIENTS


def test_ui_control_windows_host_retries_pending_cleanup(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()

    class PendingCleanupHost(_FakeHostClient):
        def __init__(self, **kwargs: Any) -> None:
            super().__init__(**kwargs)
            self.stop_calls = 0

        def stop(self) -> dict[str, Any]:
            self.stop_calls += 1
            return {
                "type": "session_stopped",
                "cleanup_pending": self.stop_calls == 1,
            }

    _configure_fake_host(backend, monkeypatch)
    monkeypatch.setattr(backend, "_HostClient", PendingCleanupHost)
    assert backend.snapshot_tool({"session_id": "cleanup"})["success"] is True

    pending = backend.stop_computer_use_tool({"session_id": "cleanup"})
    assert pending["success"] is False
    assert pending["context"]["cleanup_pending"] is True
    assert "cleanup" in backend._CLIENTS

    completed = backend.stop_computer_use_tool({"session_id": "cleanup"})
    assert completed["success"] is True
    assert backend._CLIENTS == {}


def test_ui_control_windows_host_resume_always_round_trips_to_trusted_surface(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "resume", "resume_computer_use": True})

    assert result["success"] is True
    assert _FakeHostClient.instances[0].resumed is True


def test_ui_control_windows_uia_script_retains_hard_target_boundaries() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "Denied-Target-Reason" in script
    assert "Denied-Action-Target-Reason" in script
    assert "$currentInfo.IsPassword" in script
    assert "is_password = [bool]$current.IsPassword" in script
    assert "Cross-process descendant controls are not allowed" in script
    assert "Windows Run dialog is not an allowed" in script
    assert "value_pattern_available = $valuePattern.available" in script
    assert "text_pattern_available = $textPatternAvailable" in script
    assert "return $null" in script


def test_ui_control_windows_uia_script_limits_owned_standard_menu_popups() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()
    helpers = (_SCRIPTS / "_windows_uia_helpers.ps1").read_text(encoding="utf-8")

    assert "Find-Owned-Standard-Menu-Popup $handleMatches[0]" in script
    assert "$authorizedRoot.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)" in helpers
    assert "$matches.Count -ne 1" in helpers
    assert '([string]$popupInfo.ClassName) -cne "#32768"' in helpers
    assert "[uint32]$popupInfo.ProcessId -ne $rootProcessId" in helpers
    assert "IsActiveOwnedStandardMenuPopup" in helpers
    assert "popupThreadId != rootThreadId" in helpers
    assert "rootProcessId != expectedProcessId || popupProcessId != expectedProcessId" in helpers
    assert "!IsWindowVisible(popup) || GetWindow(popup, GetWindowOwner) != authorizedRoot" in helpers
    assert "(info.Flags & GuiInMenuMode) != 0" in helpers
    assert "info.ActiveWindow == authorizedRoot" in helpers
    assert "info.MenuOwnerWindow == authorizedRoot" in helpers


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
                frame({"type": "hello", "protocol_version": 3, "capabilities": []})
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


def test_ui_control_host_client_uses_versioned_binary_identity_pipe(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    client_module = backend._HOST
    digest = "a" * 64
    monkeypatch.setattr(client_module, "_windows_session_id", lambda: 42)
    monkeypatch.setattr(client_module, "_host_version", lambda: "0.19.65")
    monkeypatch.setattr(client_module, "_host_binary", lambda: Path("host.exe"))
    monkeypatch.setattr(client_module, "_host_identity", lambda _binary: digest)

    assert client_module._PROTOCOL_VERSION == 3
    assert client_module._pipe_path() == (
        rf"\\.\pipe\dcc-mcp-ui-control-host-v3-version-0.19.65-sha256-{digest}-session-42"
    )


def test_ui_control_host_client_recording_wire_has_no_output_path_and_consumes_observation() -> None:
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
                frame(
                    {
                        "type": "hello",
                        "protocol_version": 3,
                        "capabilities": ["exact_window_recording"],
                    }
                )
                + frame(
                    {
                        "type": "session_opened",
                        "session_id": "pv",
                        "window_capability": "window:opaque",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "Game"},
                    }
                )
                + frame(
                    {
                        "type": "clip_recorded",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "Game"},
                        "artifact": {
                            "recording_id": "clip-1",
                            "directory": "C:/host-owned/clip-1",
                            "manifest_path": "C:/host-owned/clip-1/manifest.json",
                            "frame_pattern": "frame-%06d.jpg",
                            "frame_count": 30,
                            "width": 1280,
                            "height": 720,
                            "frames_per_second": 30,
                            "started_at_ms": 1000,
                            "ended_at_ms": 2000,
                            "manifest_sha256": "a" * 64,
                        },
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
        session_id="pv",
        task_grant_id="grant",
        dcc_type="unity",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        stream=stream,
    )
    client._latest_observation_id = "obs-before-recording"
    client._latest_accessibility_state_id = "accessibility-before-recording"

    response = client.record_clip(duration_ms=1_000, frames_per_second=30, jpeg_quality=92)

    assert response["artifact"]["frame_count"] == 30
    assert client._latest_observation_id is None
    assert client._latest_accessibility_state_id is None
    raw = bytes(stream.requests)
    requests = []
    offset = 0
    while offset < len(raw):
        length = struct.unpack(">I", raw[offset : offset + 4])[0]
        offset += 4
        requests.append(json.loads(raw[offset : offset + length]))
        offset += length
    recording = next(request for request in requests if request["method"] == "record_clip")
    assert set(recording["params"]) == {
        "session_id",
        "task_grant_id",
        "window_capability",
        "duration_ms",
        "frames_per_second",
        "format",
        "jpeg_quality",
    }
    assert recording["params"]["format"] == "jpeg_sequence"
    assert not any("path" in key or "directory" in key for key in recording["params"])


def test_ui_control_host_client_rejects_recording_on_an_older_host() -> None:
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
                frame({"type": "hello", "protocol_version": 3, "capabilities": []})
                + frame(
                    {
                        "type": "session_opened",
                        "session_id": "old",
                        "window_capability": "window:old",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "Game"},
                    }
                )
            )

        def read(self, length: int) -> bytes:
            return self.responses.read(length)

        def write(self, data: bytes) -> int:
            return len(data)

        def close(self) -> None:
            return None

    stream = ScriptedPipe()
    client = client_module.UiControlHostClient(
        session_id="old",
        task_grant_id="grant",
        dcc_type="unity",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        stream=stream,
    )

    with pytest.raises(client_module.UiControlHostError) as failure:
        client.record_clip(duration_ms=1_000, frames_per_second=30, jpeg_quality=92)
    assert failure.value.code == "unsupported"


def test_ui_control_host_client_retains_capability_until_cleanup_completes() -> None:
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
                frame({"type": "hello", "protocol_version": 3, "capabilities": []})
                + frame(
                    {
                        "type": "session_opened",
                        "session_id": "cleanup",
                        "window_capability": "window:opaque",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "DCC"},
                    }
                )
                + frame({"type": "session_stopped", "session_id": "cleanup", "cleanup_pending": True})
                + frame({"type": "session_stopped", "session_id": "cleanup", "cleanup_pending": False})
            )
            self.closed = False

        def read(self, length: int) -> bytes:
            return self.responses.read(length)

        def write(self, data: bytes) -> int:
            return len(data)

        def close(self) -> None:
            self.closed = True

    stream = ScriptedPipe()
    client = client_module.UiControlHostClient(
        session_id="cleanup",
        task_grant_id="grant",
        dcc_type="maya",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        stream=stream,
    )

    assert client.stop()["cleanup_pending"] is True
    assert client._window_capability == "window:opaque"
    assert stream.closed is False

    assert client.stop()["cleanup_pending"] is False
    assert client._window_capability is None
    assert stream.closed is True


def test_ui_control_host_client_revokes_local_capability_when_exact_target_closes() -> None:
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
                frame({"type": "hello", "protocol_version": 3, "capabilities": []})
                + frame(
                    {
                        "type": "session_opened",
                        "session_id": "transition",
                        "window_capability": "window:opaque",
                        "target": {"process_id": 42, "window_handle": 500, "window_title": "DCC"},
                    }
                )
                + frame(
                    {
                        "type": "action_completed",
                        "success": True,
                        "target_closed": True,
                        "policy_tier": "task_grant",
                        "message": "completed; the exact target window closed",
                    }
                )
            )
            self.requests = bytearray()
            self.closed = False

        def read(self, length: int) -> bytes:
            return self.responses.read(length)

        def write(self, data: bytes) -> int:
            self.requests.extend(data)
            return len(data)

        def close(self) -> None:
            self.closed = True

    stream = ScriptedPipe()
    client = client_module.UiControlHostClient(
        session_id="transition",
        task_grant_id="grant",
        dcc_type="unity",
        process_id=42,
        window_handle=500,
        allow_raw_input=False,
        stream=stream,
    )
    client._latest_observation_id = "obs-1"
    client._latest_accessibility_state_id = "accessibility:1"

    response = client.execute({"action": "click"})

    assert response["target_closed"] is True
    assert client._window_capability is None
    assert stream.closed is True
    with pytest.raises(client_module.UiControlHostError, match="session is closed"):
        client.window_state()


def test_ui_control_system_operation_uses_only_operator_grant_and_redacts_result(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID", "operator-plugin-setup")
    calls: list[dict[str, Any]] = []

    def execute(**kwargs: Any) -> dict[str, Any]:
        calls.append(kwargs)
        return {
            "type": "system_operation_completed",
            "operation_type": "ensure_hkcu_registry_string",
            "outcome": "updated",
            "policy_tier": "action_confirmation",
            "message": "Registry value is configured.",
        }

    monkeypatch.setattr(backend, "_execute_system_operation", execute)
    result = backend.system_operation_tool(
        {
            "session_id": "plugin-setup",
            "operation_id": "configure-plugin-mode",
        }
    )

    assert result["success"] is True
    assert calls[0]["system_grant_id"] == "operator-plugin-setup"
    assert calls[0]["session_id"].startswith("system:")
    assert calls[0]["session_id"] != "plugin-setup"
    assert calls[0]["operation_id"] == "configure-plugin-mode"
    serialized = json.dumps(result)
    assert "configure-plugin-mode" not in serialized
    assert "operator-plugin-setup" not in serialized
    assert result["context"]["operation_type"] == "ensure_hkcu_registry_string"
    assert result["context"]["outcome"] == "updated"
    assert result["context"]["policy_tier"] == "action_confirmation"


def test_ui_control_system_operation_rejects_untrusted_fields_and_missing_grant(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID", raising=False)

    missing = backend.system_operation_tool({"operation_id": "enable-plugin"})
    assert missing["error"] == "system_operation_not_granted"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID", "operator-plugin-setup")
    top_level = backend.system_operation_tool(
        {
            "system_grant_id": "agent-grant",
            "operation_id": "enable-plugin",
        }
    )
    assert top_level["error"] == "invalid_request"

    rejected = backend.system_operation_tool({"operation_id": "enable-plugin", "command": "ignored.exe"})
    assert rejected["error"] == "invalid_request"

    value_injection = backend.system_operation_tool(
        {"operation_id": "configure-plugin", "value": "must-not-cross-the-tool-boundary"}
    )
    assert value_injection["error"] == "invalid_request"

    invalid_id = backend.system_operation_tool({"operation_id": "password\nvalue"})
    assert invalid_id["error"] == "invalid_request"


@pytest.mark.parametrize("backend", ["mock", "chrome"])
def test_ui_control_system_operation_is_explicitly_unsupported_outside_windows(
    backend: str,
    tmp_path: Path,
) -> None:
    result = _run_tool(
        "system_operation",
        {"operation_id": "link-plugin"},
        tmp_path,
        extra_env={"DCC_MCP_UI_CONTROL_BACKEND": backend},
    )

    assert result["success"] is False
    assert result["error"] == "unsupported"


def test_ui_control_host_client_negotiates_typed_system_operations() -> None:
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
                frame(
                    {
                        "type": "hello",
                        "protocol_version": 3,
                        "capabilities": ["typed_system_operations"],
                    }
                )
                + frame(
                    {
                        "type": "system_session_opened",
                        "session_id": "system:wire",
                        "system_capability": "system:opaque",
                        "dcc_type": "photoshop",
                    }
                )
                + frame(
                    {
                        "type": "system_operation_completed",
                        "operation_type": "ensure_file_symlink",
                        "outcome": "unchanged",
                        "policy_tier": "action_confirmation",
                        "message": "Symbolic link is configured.",
                    }
                )
                + frame({"type": "system_session_stopped", "session_id": "system:wire"})
            )
            self.requests = bytearray()
            self.closed = False

        def read(self, length: int) -> bytes:
            return self.responses.read(length)

        def write(self, data: bytes) -> int:
            self.requests.extend(data)
            return len(data)

        def close(self) -> None:
            self.closed = True

    stream = ScriptedPipe()
    result = client_module.execute_system_operation(
        session_id="system:wire",
        system_grant_id="operator-grant",
        operation_id="link-plugin",
        stream=stream,
    )

    requests = []
    raw = io.BytesIO(bytes(stream.requests))
    prefix = raw.read(4)
    while prefix:
        requests.append(json.loads(raw.read(struct.unpack(">I", prefix)[0])))
        prefix = raw.read(4)
    assert [request["method"] for request in requests] == [
        "hello",
        "open_system_session",
        "execute_system_operation",
        "stop_system_session",
    ]
    assert requests[0]["params"]["protocol_version"] == 3
    assert set(requests[1]["params"]) == {"session_id", "system_grant_id"}
    assert set(requests[2]["params"]) == {
        "session_id",
        "system_grant_id",
        "system_capability",
        "operation_id",
    }
    wire = json.dumps(requests)
    assert "command" not in wire
    assert "C:/link" not in wire
    assert "Software\\\\Vendor" not in wire
    assert result["outcome"] == "unchanged"
    assert stream.closed is True

    unsupported = ScriptedPipe()
    unsupported.responses = io.BytesIO(frame({"type": "hello", "protocol_version": 3, "capabilities": []}))
    with pytest.raises(client_module.UiControlHostError) as exc_info:
        client_module.execute_system_operation(
            session_id="system:old-host",
            system_grant_id="operator-grant",
            operation_id="enable-plugin",
            stream=unsupported,
        )
    assert exc_info.value.code == "unsupported"
    assert unsupported.closed is True


def test_ui_control_chrome_cdp_preset_aliases(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CDP_PRESET", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CHROME_PRESET", raising=False)
    assert cdp_runtime.cdp_preset() == "reuse"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_CDP_PRESET", "aurora")
    assert cdp_runtime.cdp_preset() == "auroraview"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_CDP_PRESET", "temp")
    assert cdp_runtime.cdp_preset() == "isolated"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_CDP_PRESET", "msedge")
    assert cdp_runtime.cdp_preset() == "edge"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_CDP_PRESET", "agent_browser")
    assert cdp_runtime.cdp_preset() == "agent-browser"


def test_ui_control_auroraview_preset_uses_auroraview_port(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CHROME_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CDP_PORT", raising=False)
    monkeypatch.setenv("AURORAVIEW_CDP_PORT", "9333")

    assert cdp_runtime.endpoint_candidates("auroraview") == [
        "http://127.0.0.1:9333",
        "http://127.0.0.1:9222",
    ]


def test_ui_control_edge_preset_uses_edge_port(monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()

    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_EDGE_CDP_URL", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_CDP_PORT", raising=False)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_EDGE_CDP_PORT", "9444")

    assert cdp_runtime.endpoint_candidates("edge") == [
        "http://127.0.0.1:9444",
        "http://127.0.0.1:9222",
    ]


def test_ui_control_agent_browser_preset_parses_cdp_url(tmp_path: Path, monkeypatch: Any) -> None:
    cdp_runtime = _load_cdp_runtime_module()
    script = tmp_path / ("agent-browser.cmd" if os.name == "nt" else "agent-browser")
    if os.name == "nt":
        script.write_text("@echo off\necho ws://127.0.0.1:9777/devtools/page/ci\n", encoding="utf-8")
    else:
        script.write_text("#!/bin/sh\necho ws://127.0.0.1:9777/devtools/page/ci\n", encoding="utf-8")
        script.chmod(0o755)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_AGENT_BROWSER_BIN", str(script))

    assert cdp_runtime._agent_browser_cdp_url() == "ws://127.0.0.1:9777/devtools/page/ci"
