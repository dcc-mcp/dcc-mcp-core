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
from dcc_mcp_core import parse_skill_md
from dcc_mcp_core._server.inprocess_executor import run_skill_script

_SKILL_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "ui-control"
_SCRIPTS = _SKILL_DIR / "scripts"


def test_ui_control_skill_triggers_on_cua_ui_control_and_dcc_cua() -> None:
    metadata = parse_skill_md(str(_SKILL_DIR))
    assert metadata is not None
    description = (metadata.description or "").lower()
    search_hint = (metadata.search_hint or "").lower()
    for phrase in ("cua", "ui control", "dcc-cua"):
        assert phrase in description
        assert phrase in search_hint
    assert "dcc-cua 0.4.0+" in (metadata.compatibility or "")

    from dcc_mcp_core import SkillCatalog
    from dcc_mcp_core import ToolRegistry

    catalog = SkillCatalog(ToolRegistry())
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])
    for phrase in ("cua", "ui control", "dcc-cua"):
        names = {result.name for result in catalog.search_skills(phrase)}
        assert "ui-control" in names


def _load_cdp_runtime_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_cdp_runtime", _SCRIPTS / "_cdp_runtime.py")
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_cua_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_ui_control_cua", _SCRIPTS / "_cua_backend.py")
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


def test_ui_control_defaults_to_dcc_cua_and_keeps_mock_explicit(monkeypatch: Any) -> None:
    entrypoint = _load_entrypoint_module()
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_BACKEND", raising=False)
    assert entrypoint._selected_backend() == "cua"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "")
    assert entrypoint._selected_backend() == "cua"

    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "mock")
    assert entrypoint._selected_backend() == "mock"


def test_ui_control_entrypoint_imports_without_native_core(monkeypatch: Any) -> None:
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", None)
    entrypoint = _load_entrypoint_module()

    class Backend:
        @staticmethod
        def snapshot_tool(_params: dict[str, Any]) -> dict[str, Any]:
            return {
                "success": True,
                "message": "Captured mock snapshot.",
                "context": {
                    "session_id": "mock",
                    "snapshot": {"metadata": {"ui_control": {"backend": "mock"}}},
                },
            }

    monkeypatch.setattr(entrypoint, "_load_backend", lambda: Backend)

    result = entrypoint.snapshot_tool({"session_id": "mock"})
    assert result["success"] is True
    assert result["context"]["snapshot"]["metadata"]["ui_control"]["backend"] == "mock"


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


def test_ui_control_cua_spec_loads_isolated_host_client_state() -> None:
    first = _load_cua_module()
    second = _load_cua_module()

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
        "recording_start",
        "recording_state",
        "recording_stop",
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
    assert "ui_control__recording_start" in action_names
    assert "ui_control__recording_state" in action_names
    assert "ui_control__recording_stop" in action_names
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
    assert "up to four simultaneous canvas keys" in keys_description
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
    record_schema = json.loads(tools["recording_start"].input_schema)
    assert record_schema["required"] == ["output_dir"]
    assert record_schema["additionalProperties"] is False
    assert record_schema["properties"]["output_dir"]["minLength"] == 1
    assert record_schema["properties"]["output_dir"]["maxLength"] == 4096
    assert record_schema["properties"]["record_video"]["type"] == "boolean"
    assert tools["recording_start"].read_only is False
    assert tools["recording_state"].read_only is True
    assert tools["recording_state"].idempotent is True
    assert tools["recording_stop"].read_only is False
    assert all(tools[name].requires_in_process for name in ("recording_start", "recording_state", "recording_stop"))
    assert not (tools["act"].next_tools or {}).get("on_failure")
    assert not (tools["wait_for"].next_tools or {}).get("on_failure")
    wait_schema = json.loads(tools["wait_for"].input_schema)
    assert wait_schema["properties"]["condition"]["properties"]["timeout_ms"]["maximum"] == 60_000
    assert tools["stop_computer_use"].requires_in_process is True


def test_ui_control_windows_game_navigation_contract_is_fail_closed() -> None:
    backend = _load_cua_module()

    for keys in (
        ["W"],
        ["W", "D"],
        ["W", "D", "J", "K"],
        ["J", "K"],
        ["SHIFT", "E"],
        ["CTRL+Z"],
        ["SPACE"],
        ["F5"],
        ["LEFT", "UP"],
    ):
        assert backend._validate_action_limits({"action": "game_navigation", "keys": keys, "duration_ms": 500}) is None
        assert backend._is_native_action("game_navigation", {"keys": keys}) is True

    for payload in (
        {"action": "game_navigation", "keys": []},
        {"action": "game_navigation", "keys": ["W", "W"]},
        {"action": "game_navigation", "keys": ["W", "A", "S", "D", "J"]},
        {"action": "game_navigation", "keys": ["WIN", "R"]},
        {"action": "game_navigation", "keys": ["NOT_A_KEY"]},
        {"action": "game_navigation", "keys": ["CTRL+"]},
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


def test_ui_control_entrypoint_reports_real_snapshot_provenance(tmp_path: Path, monkeypatch: Any) -> None:
    entrypoint = _load_entrypoint_module()
    stored: list[dict[str, Any]] = []

    class FileRef:
        def __init__(self, *, mime: str, display_name: str, session_id: str, correlation_id: str) -> None:
            self.uri = f"artefact://sha256/{'b' * 64}"
            self.mime = mime
            self.size_bytes = 3
            self.display_name = display_name
            self.digest = f"sha256:{'b' * 64}"
            self.session_id = session_id
            self.correlation_id = correlation_id
            self.created_at = "2026-07-24T00:00:00Z"
            self.expires_at = "2026-07-25T00:00:00Z"

    def put_bytes(_data: bytes, **kwargs: Any) -> FileRef:
        stored.append(kwargs)
        return FileRef(**{key: kwargs[key] for key in ("mime", "display_name", "session_id", "correlation_id")})

    class Backend:
        @staticmethod
        def snapshot_tool(_params: dict[str, Any]) -> dict[str, Any]:
            return {
                "success": True,
                "message": "Captured scoped CUA application snapshot.",
                "context": {
                    "session_id": "evidence",
                    "snapshot_id": "accessibility:1",
                    "snapshot": {
                        "metadata": {
                            "ui_control": {"backend": "dcc-cua"},
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
                    "__rich__": {
                        "kind": "image",
                        "data": base64.b64encode(b"png").decode("ascii"),
                        "mime": "image/png",
                    },
                },
            }

    monkeypatch.setattr(entrypoint, "_load_backend", lambda: Backend)
    monkeypatch.setattr(entrypoint, "_artefact_put_bytes", put_bytes)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_BACKEND", "cua")
    monkeypatch.setenv("DCC_MCP_DISABLE_FILE_LOGGING", "1")

    result = entrypoint.snapshot_tool({"session_id": "evidence"})

    provenance = result["context"]["capture_provenance"]
    assert provenance == {
        "tool": "ui_control__snapshot",
        "backend": "dcc-cua",
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
    assert "dcc-cua" in result["message"]
    assert "1600x900" in result["message"]
    assert "downscaled from 1920x1080" in result["message"]
    screenshot = result["context"]["artifacts"][0]
    assert screenshot["kind"] == "ui_control_snapshot"
    assert screenshot["display_name"] == "ui-control-snapshot-evidence-accessibility-1.png"
    assert screenshot["session_id"] == "evidence"
    assert result["context"]["__rich__"]["artifact_uri"] == screenshot["uri"]

    assert [item["ttl_secs"] for item in stored] == [86_400]


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
            "DCC_MCP_UI_CONTROL_BACKEND": "cua",
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


def test_ui_control_admin_audit_links_state_delta_to_action(tmp_path: Path, monkeypatch: Any) -> None:
    entrypoint = _load_entrypoint_module()
    monkeypatch.setenv("DCC_MCP_LOG_DIR", str(tmp_path))
    monkeypatch.delenv("DCC_MCP_DISABLE_FILE_LOGGING", raising=False)

    entrypoint._record_operation(
        "snapshot_tool",
        {"session_id": "delta"},
        {
            "success": True,
            "context": {
                "session_id": "delta",
                "snapshot_id": "accessibility:2",
                "state_delta": {
                    "source": "cua-accessibility",
                    "state_id": "accessibility:2",
                    "cause_action_id": "action:test",
                    "delta": {
                        "baseline": False,
                        "changes": [{"path": "/root/name", "kind": "changed"}],
                        "truncated": False,
                    },
                },
            },
        },
    )

    log_text = next(tmp_path.glob("dcc-mcp-ui-control.*.log")).read_text(encoding="utf-8")
    row = json.loads(log_text.splitlines()[-1].split(": ", 1)[1])
    assert row["action_id"] == "action:test"
    assert row["state_delta"]["paths"] == ["/root/name"]
    assert "state_changes=1" in row["detail"]


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
        "cua",
    ]


@pytest.mark.parametrize(
    ("backend", "reported_backend"),
    [("mock", "mock"), ("chrome", "chrome-cdp")],
)
def test_ui_control_non_cua_backends_reject_trajectory_recording(
    tmp_path: Path,
    backend: str,
    reported_backend: str,
) -> None:
    result = _run_tool(
        "recording_start",
        {"session_id": "no-recording-fallback", "output_dir": str(tmp_path)},
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
        self.recordings: list[dict[str, Any]] = []
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
                        "element_index": 1,
                        "element_token": "dcc-wuia:snapshot:1",
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
            "state_delta": {
                "schema_version": 1,
                "baseline": self.snapshot_calls == 1,
                "changes": [] if self.snapshot_calls == 1 else [{"path": "/focus", "kind": "changed"}],
                "truncated": False,
            },
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
            "state_delta": snapshot["state_delta"],
            "cause_action_id": "action:test",
        }

    def execute(self, action: dict[str, Any]) -> dict[str, Any]:
        self.executed.append(action)
        return {
            "type": "action_completed",
            "success": True,
            "action_id": "action:test",
            "policy_tier": "task_grant",
            "message": "completed",
        }

    def recording_start(self, *, output_dir: str, record_video: bool) -> dict[str, Any]:
        self.recordings.append({"operation": "start", "output_dir": output_dir, "record_video": record_video})
        return {"structuredContent": {"enabled": True, "next_turn": 1}}

    def recording_state(self) -> dict[str, Any]:
        self.recordings.append({"operation": "state"})
        return {"structuredContent": {"enabled": True, "next_turn": 2}}

    def recording_stop(self) -> dict[str, Any]:
        self.recordings.append({"operation": "stop"})
        return {"structuredContent": {"enabled": False, "next_turn": 2}}

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


def _configure_fake_host(backend: Any, monkeypatch: Any, *, raw: Optional[bool] = None) -> None:
    _FakeHostClient.instances.clear()
    monkeypatch.setattr(backend, "_HostClient", _FakeHostClient)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_PROCESS_ID", "1234")
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_WINDOW_HANDLE", raising=False)
    if raw is None:
        monkeypatch.delenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", raising=False)
    else:
        monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "true" if raw else "false")


def test_ui_control_cua_host_maps_snapshot_and_shared_image(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "godot", "app_name": "Godot"})

    assert result["success"] is True
    context = result["context"]
    assert context["snapshot_id"] == "accessibility:1"
    assert context["snapshot"]["root"]["id"] == "cua:42.1"
    assert context["snapshot"]["root"]["children"][0]["role"] == "button"
    assert context["snapshot"]["metadata"]["ui_control"]["backend"] == "dcc-cua"
    assert context["snapshot"]["metadata"]["computer_use"]["observation_id"] == "obs-1"
    assert context["state_delta"]["source"] == "cua-accessibility"
    assert context["state_delta"]["delta"]["baseline"] is True
    assert base64.b64decode(context["__rich__"]["data"]) == b"png"
    assert _FakeHostClient.instances[0].kwargs["allow_raw_input"] is True


def test_ui_control_cua_host_controls_trajectory_recording(tmp_path: Path, monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    started = backend.recording_start_tool(
        {
            "session_id": "pv",
            "output_dir": str(tmp_path),
            "record_video": True,
        }
    )
    state = backend.recording_state_tool({"session_id": "pv"})
    stopped = backend.recording_stop_tool({"session_id": "pv"})

    assert started["success"] is True
    assert started["context"]["target"]["window_handle"] == 500
    assert started["context"]["recording"]["structuredContent"]["enabled"] is True
    assert state["context"]["recording"]["structuredContent"]["next_turn"] == 2
    assert stopped["context"]["recording"]["structuredContent"]["enabled"] is False
    assert _FakeHostClient.instances[0].recordings == [
        {"operation": "start", "output_dir": str(tmp_path), "record_video": True},
        {"operation": "state"},
        {"operation": "stop"},
    ]


def test_ui_control_cua_host_requires_operator_bound_scope(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _FakeHostClient.instances.clear()
    monkeypatch.setattr(backend, "_HostClient", _FakeHostClient)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_WINDOW_HANDLE", raising=False)

    result = backend.snapshot_tool({"session_id": "untrusted", "process_id": 1234})

    assert result["success"] is False
    assert result["error"] == "permission_denied"
    assert not _FakeHostClient.instances


def test_ui_control_cua_host_scope_cannot_be_widened(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "wrong", "process_id": 7})

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert not _FakeHostClient.instances


def test_ui_control_cua_host_raw_input_is_runtime_ceiling(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch, raw=False)

    denied = backend.snapshot_tool(
        {
            "session_id": "denied",
            "policy": {"allow_raw_coordinates": True, "allow_keyboard_shortcuts": True},
        }
    )
    assert denied["success"] is True
    assert denied["context"]["policy"]["allow_raw_coordinates"] is False
    assert _FakeHostClient.instances[-1].kwargs["allow_raw_input"] is False

    enabled_backend = _load_cua_module()
    _configure_fake_host(enabled_backend, monkeypatch)
    enabled = enabled_backend.snapshot_tool({"session_id": "enabled"})
    assert enabled["context"]["policy"]["allow_raw_coordinates"] is True
    assert _FakeHostClient.instances[-1].kwargs["allow_raw_input"] is True


def test_ui_control_cua_host_semantic_action_is_thin_proxy(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)
    snapshot = backend.snapshot_tool({"session_id": "semantic"})
    snapshot_id = snapshot["context"]["snapshot_id"]

    result = backend.act_tool(
        {
            "session_id": "semantic",
            "snapshot_id": snapshot_id,
            "control_id": "cua:42.2",
            "action": "click",
            "intent": "external_communication",
        }
    )

    assert result["success"] is True
    assert result["context"]["action_id"] == "action:test"
    payload = _FakeHostClient.instances[0].executed[0]
    assert payload["input_kind"] == "semantic"
    assert payload["element_token"] == "dcc-wuia:snapshot:1"
    assert "control_id" not in payload
    assert payload["intent"] == "external_communication"
    source = (_SCRIPTS / "_cua_backend.py").read_text(encoding="utf-8")
    assert "ComputerUseSession" not in source
    assert "subprocess" not in source
    assert "powershell" not in source.lower()


def test_ui_control_cua_host_native_action_requires_fresh_snapshot(monkeypatch: Any) -> None:
    backend = _load_cua_module()
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


def test_ui_control_cua_host_restores_minimized_exact_window_without_snapshot(monkeypatch: Any) -> None:
    backend = _load_cua_module()
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


def test_ui_control_cua_host_reports_closed_target_success_and_requires_explicit_rebind(monkeypatch: Any) -> None:
    backend = _load_cua_module()

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
            "control_id": "cua:42.2",
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


def test_ui_control_cua_host_invalid_snapshot_target_exposes_scoped_recovery() -> None:
    backend = _load_cua_module()

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


def test_ui_control_cua_host_protected_system_ui_requires_manual_operator_recovery() -> None:
    backend = _load_cua_module()

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


def test_ui_control_cua_host_propagates_trusted_confirmation_denial(monkeypatch: Any) -> None:
    backend = _load_cua_module()

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
            "control_id": "cua:42.2",
            "action": "click",
        }
    )

    assert result["success"] is False
    assert result["error"] == "approval_required"
    assert result["context"]["result"]["metadata"]["policy_tier"] == "action_confirmation"


def test_ui_control_cua_host_find_wait_stop_and_cleanup(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    found = backend.find_tool({"session_id": "workflow", "role": "button"})
    assert found["success"] is True
    assert found["context"]["matches"][0]["id"] == "cua:42.2"

    waited = backend.wait_for_tool(
        {
            "session_id": "workflow",
            "condition": {"kind": "control_exists", "control_id": "cua:42.2", "timeout_ms": 50},
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
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    snapshot = backend.snapshot_tool({"session_id": "cached"})
    found = backend.find_tool({"session_id": "cached", "role": "button"})

    assert snapshot["success"] is True
    assert found["success"] is True
    assert found["context"]["snapshot_id"] == snapshot["context"]["snapshot_id"]
    assert _FakeHostClient.instances[0].snapshot_calls == 1


def test_ui_control_windows_expires_idle_session_without_touching_active_call(monkeypatch: Any) -> None:
    backend = _load_cua_module()
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


def test_ui_control_windows_expires_idle_session_without_another_tool_call(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)
    monkeypatch.setattr(backend, "_IDLE_LEASE_SECONDS", 0.02)

    assert backend.snapshot_tool({"session_id": "idle"})["success"] is True
    idle_client = _FakeHostClient.instances[0]

    deadline = time.monotonic() + 1.0
    while not idle_client.stopped and time.monotonic() < deadline:
        time.sleep(0.01)

    assert idle_client.stopped is True
    assert "idle" not in backend._CLIENTS


def test_ui_control_cua_host_retries_pending_cleanup(monkeypatch: Any) -> None:
    backend = _load_cua_module()

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


def test_ui_control_cua_host_resume_always_round_trips_to_trusted_surface(monkeypatch: Any) -> None:
    backend = _load_cua_module()
    _configure_fake_host(backend, monkeypatch)

    result = backend.snapshot_tool({"session_id": "resume", "resume_computer_use": True})

    assert result["success"] is True
    assert _FakeHostClient.instances[0].resumed is True


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
