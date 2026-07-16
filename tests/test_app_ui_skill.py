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
        "focus",
        "keyboard_shortcut",
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


def test_app_ui_windows_uia_maps_control_tree_to_contract() -> None:
    backend = _load_windows_uia_module()
    raw = {
        "runtime_id": "42.7",
        "fallback_path": "0",
        "name": "Project name",
        "automation_id": "projectNameEdit",
        "class_name": "Edit",
        "control_type": "ControlType.Edit",
        "process_id": 1234,
        "native_window_handle": 500,
        "enabled": True,
        "offscreen": False,
        "focused": True,
        "bounds": {"x": 10, "y": 20, "width": 200, "height": 24},
        "value": "Hero",
        "children": [
            {
                "runtime_id": "42.8",
                "fallback_path": "0.0",
                "name": "Apply",
                "automation_id": "applyButton",
                "class_name": "Button",
                "control_type": "ControlType.Button",
                "process_id": 1234,
                "native_window_handle": 501,
                "enabled": True,
                "offscreen": False,
                "bounds": {"x": 220, "y": 20, "width": 80, "height": 24},
                "children": [],
            }
        ],
    }

    node = backend._node_from_uia_dict(raw, "uia-session:1").to_dict()

    assert node["id"] == "uia:42.7"
    assert node["role"] == "text_field"
    assert node["label"] == "Project name"
    assert node["object_name"] == "projectNameEdit"
    assert node["value"] == "Hero"
    assert node["bounds"] == {"x": 10.0, "y": 20.0, "width": 200.0, "height": 24.0}
    assert node["metadata"]["app_ui"]["backend"] == "windows-uia"
    assert node["metadata"]["app_ui"]["process_id"] == 1234
    assert node["children"][0]["role"] == "button"
    assert node["children"][0]["id"] == "uia:42.8"


def test_app_ui_windows_uia_requires_explicit_scope(tmp_path: Path) -> None:
    result = _run_tool(
        "snapshot",
        {"session_id": "uia-no-scope"},
        tmp_path,
        extra_env={"DCC_MCP_APP_UI_BACKEND": "windows-uia"},
    )

    assert result["success"] is False
    assert result["error"] == "missing_window"
    assert "whole-desktop snapshots are disabled" in result["message"]


def test_app_ui_windows_uia_accepts_process_id_scope() -> None:
    """PowerShell's read-only $PID automatic variable must not be shadowed."""
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "foreach ($pid in" not in script.lower()
    assert "foreach ($processId in As-Array $payload.scope.process_ids)" in script


def test_app_ui_windows_uia_uses_main_window_handle_for_single_process() -> None:
    """Heavy DCC providers should not require a whole-desktop UIA scan."""
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "function Find-Process-Root" in script
    assert "[System.Windows.Automation.AutomationElement]::FromHandle" in script
    assert "$proc.MainWindowHandle" in script
    assert "$requestedHandles = As-Array $payload.scope.window_handles" in script
    assert "foreach ($windowHandle in $requestedHandles)" in script


def test_app_ui_windows_uia_click_has_native_button_fallback() -> None:
    """Native dialogs can expose an InvokePattern that still rejects Invoke()."""
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "function Invoke-NativeButtonClick" in script
    assert "[DccMcpNativeUi]::SendMessage" in script
    assert "0x00F5" in script
    assert "invoked native button" in script
    assert "focused control because InvokePattern is unavailable" not in script
    assert "click requires InvokePattern, TogglePattern, or a native button handle" in script


def test_app_ui_windows_uia_raw_input_policy_is_an_environment_ceiling(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()

    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    denied = backend._policy_from_params({"policy": {"allow_raw_coordinates": True, "allow_keyboard_shortcuts": True}})
    assert denied.allow_raw_coordinates is False
    assert denied.allow_keyboard_shortcuts is False

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    enabled = backend._policy_from_params({})
    assert enabled.allow_raw_coordinates is True
    assert enabled.allow_keyboard_shortcuts is True

    narrowed = backend._policy_from_params(
        {"policy": {"allow_raw_coordinates": False, "allow_keyboard_shortcuts": False}}
    )
    assert narrowed.allow_raw_coordinates is False
    assert narrowed.allow_keyboard_shortcuts is False


def test_app_ui_windows_uia_scope_intersects_request_with_policy(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_TITLE", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_NAME", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)

    pid_policy = backend.AppUiPolicy(allowed_process_ids=[42])
    rejected_pid = backend._scope_from_params({"process_id": 7}, pid_policy)
    assert rejected_pid["invalid_reason"]

    title_policy = backend.AppUiPolicy(allowed_window_titles=["Godot Editor"])
    narrowed_title = backend._scope_from_params({"window_title": "Godot"}, title_policy)
    assert narrowed_title["invalid_reason"] is None
    assert narrowed_title["window_titles"] == ["Godot Editor"]
    rejected_title = backend._scope_from_params({"window_title": "Maya"}, title_policy)
    assert rejected_title["invalid_reason"]

    accepted = backend._scope_from_params(
        {"process_id": 42, "window_title": "Project - Godot Editor"},
        backend.AppUiPolicy(allowed_process_ids=[42], allowed_window_titles=["Godot Editor"]),
    )
    assert accepted["invalid_reason"] is None
    assert accepted["process_ids"] == [42]
    assert accepted["window_titles"] == ["Project - Godot Editor"]
    assert os.getpid() not in accepted["excluded_process_ids"]

    embedded = backend._scope_from_params(
        {"process_id": os.getpid()},
        backend.AppUiPolicy(),
    )
    assert embedded["excluded_process_ids"] == []


@pytest.mark.parametrize(
    "process_name",
    [
        "LockApp.exe",
        "SecHealthUI",
        "CredentialUIBroker",
        "PowerShell.exe",
        "powershell_ise.exe",
        "cmd",
        "Bitwarden",
    ],
)
def test_app_ui_windows_uia_rejects_named_system_and_sensitive_targets(
    process_name: str,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_NAME", raising=False)

    scope = backend._scope_from_params({"process_name": process_name}, backend.AppUiPolicy())

    assert "not allowed app_ui targets" in scope["invalid_reason"]


def test_app_ui_windows_uia_resolved_root_enforces_target_denylist() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests().lower()

    assert "function denied-target-reason" in script
    assert 'error = "permission_denied"' in script
    assert '"consolewindowclass"' in script
    assert '"powershell_ise"' in script
    assert '$processname -eq "explorer"' in script
    assert '$classname -eq "#32770"' in script
    assert "-and $title" not in script


def test_app_ui_windows_uia_mutation_rechecks_descendant_security_boundary() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "function Denied-Action-Target-Reason" in script
    assert "$currentProcessId -ne $rootProcessId" in script
    assert "$currentInfo.IsPassword" in script
    assert "$deniedReason = Denied-Target-Reason $current" in script
    assert '"credential dialog xaml host"' in script.lower()
    assert "Cross-process descendant controls are not allowed mutation targets." in script
    assert "Password controls are not allowed mutation targets." in script
    boundary_check = script.index("Denied-Action-Target-Reason $root $target")
    mutation = script.index("$actionResult = Invoke-Action $target")
    assert boundary_check < mutation


def test_app_ui_windows_uia_permission_denial_never_falls_back(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()

    class UnexpectedComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            raise AssertionError("permission denial must not enter native fallback")

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", UnexpectedComputerUseSession)
    monkeypatch.setattr(
        backend,
        "_run_uia",
        lambda _payload: {
            "ok": False,
            "error": "permission_denied",
            "message": "sensitive target denied",
        },
    )

    result = backend.snapshot_tool({"session_id": "denied", "window_handle": 500})

    assert result["success"] is False
    assert result["error"] == "permission_denied"


def test_app_ui_windows_uia_native_fallback_propagates_target_policy_denial(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()

    class DeniedComputerUseSession:
        @staticmethod
        def desktop_interactive() -> bool:
            return True

        @staticmethod
        def process_user_interrupted() -> bool:
            return False

        def __init__(self, **_kwargs: Any) -> None:
            pass

        def start(self) -> str:
            return '{"success":false,"error":"permission_denied","message":"sensitive target denied"}'

        def screenshot(self) -> tuple[str, bytes]:
            raise AssertionError("denied native targets must not be captured")

        def stop(self) -> str:
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", DeniedComputerUseSession)
    monkeypatch.setattr(
        backend,
        "_run_uia",
        lambda _payload: {
            "ok": False,
            "error": "backend_unavailable",
            "message": "UIA unavailable",
        },
    )

    result = backend.snapshot_tool({"session_id": "native-denied", "window_handle": 500})

    assert result["success"] is False
    assert result["error"] == "permission_denied"


def test_app_ui_windows_uia_script_fails_closed_for_unresolved_processes_and_handles() -> None:
    backend = _load_windows_uia_module()
    script = backend._dedent_for_tests()

    assert "$payload.scope.require_process_match -and $processIds.Count -eq 0" in script
    assert "Match-Scope $windowRoot $processIds" in script
    assert "$requestedHandles.Count -gt 0" in script
    assert "The explicit HWND did not satisfy every PID, process-name, and title constraint." in script
    assert "$excludedProcessIds -contains [int]$element.Current.ProcessId" in script
    assert "$matches.Count -gt 1" in script
    assert (
        '$scopeErrorCode = if ($null -eq $script:scopeError) { "missing_window" } else { "invalid_target" }' in script
    )


def test_app_ui_windows_uia_serializes_one_session(tmp_path: Path, monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    active = 0
    max_active = 0
    gate = threading.Lock()

    def slow_snapshot(_payload: dict[str, Any]) -> dict[str, Any]:
        nonlocal active, max_active
        with gate:
            active += 1
            max_active = max(max_active, active)
        time.sleep(0.03)
        with gate:
            active -= 1
        return _uia_snapshot_payload()

    monkeypatch.setattr(backend, "_run_uia", slow_snapshot)
    with ThreadPoolExecutor(max_workers=4) as pool:
        results = list(
            pool.map(
                lambda _: backend.snapshot_tool({"session_id": "serialized", "process_id": 1234}),
                range(4),
            )
        )

    ids = [item["context"]["snapshot_id"] for item in results]
    assert max_active == 1
    assert len(set(ids)) == 4


@pytest.mark.skipif(sys.platform != "win32", reason="Windows UIA backend requires PowerShell on Windows")
def test_app_ui_windows_uia_normalizes_powershell_timeout(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.setattr(backend, "_powershell_bin", lambda: "powershell.exe")

    def time_out(*_args: Any, **_kwargs: Any) -> None:
        raise subprocess.TimeoutExpired("powershell.exe", 0.25)

    monkeypatch.setattr(backend.subprocess, "run", time_out)

    try:
        backend._run_uia({"mode": "snapshot"})
    except RuntimeError as exc:
        assert "timed out after 0.25 seconds" in str(exc)
    else:
        raise AssertionError("PowerShell timeout was not normalized")


@pytest.mark.skipif(sys.platform != "win32", reason="Windows UIA backend requires PowerShell on Windows")
def test_app_ui_windows_uia_inflight_guard_terminates_powershell(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.setattr(backend, "_powershell_bin", lambda: "powershell.exe")
    checks = 0

    class SlowProcess:
        returncode = None
        terminated = False

        def communicate(self, input: Any = None, timeout: Any = None) -> tuple[str, str]:
            del input
            if self.terminated:
                self.returncode = -1
                return "", ""
            raise subprocess.TimeoutExpired("powershell.exe", timeout)

        def terminate(self) -> None:
            self.terminated = True

        def kill(self) -> None:
            self.terminated = True

    process = SlowProcess()

    def guard(_session_id: str) -> Any:
        nonlocal checks
        checks += 1
        if checks < 3:
            return None
        return {
            "ok": False,
            "error": "user_interrupted",
            "message": "Ctrl+Alt+Esc stopped Computer Use",
        }

    monkeypatch.setattr(backend, "_uia_guard_failure", guard)
    monkeypatch.setattr(backend.subprocess, "Popen", lambda *_args, **_kwargs: process)

    result = backend._run_uia({"mode": "act", "_session_id": "guarded"})

    assert result["ok"] is False
    assert result["error"] == "user_interrupted"
    assert process.terminated is True


def test_app_ui_windows_uia_inflight_guard_detects_session_desktop_transition(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()

    class NativeBindings:
        @staticmethod
        def desktop_interactive() -> bool:
            return True

        @staticmethod
        def process_user_interrupted() -> bool:
            return False

    class LockedSession:
        def status(self) -> str:
            return '{"success":true,"active":true,"desktop_interactive":false}'

    monkeypatch.setattr(backend, "_ComputerUseSession", NativeBindings)
    backend._COMPUTER_USE_SESSIONS["guarded"] = {"session": LockedSession()}

    result = backend._uia_guard_failure("guarded")

    assert result is not None
    assert result["error"] == "desktop_unavailable"


def test_app_ui_windows_uia_locked_desktop_blocks_uia_before_snapshot(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()

    class LockedComputerUseSession:
        @staticmethod
        def desktop_interactive() -> bool:
            return False

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", LockedComputerUseSession)
    monkeypatch.setattr(
        backend,
        "_run_uia",
        lambda _payload: (_ for _ in ()).throw(AssertionError("UIA must not inspect LockApp")),
    )

    result = backend.snapshot_tool({"session_id": "locked", "process_id": 1234})

    assert result["success"] is False
    assert result["error"] == "desktop_unavailable"
    assert backend.UiErrorCode.DESKTOP_UNAVAILABLE == "desktop_unavailable"
    assert "unlock" in result["message"].lower()


def test_app_ui_windows_uia_wait_stops_when_desktop_locks_mid_poll(
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    checks = 0
    captures = 0

    def desktop_interactive() -> bool:
        nonlocal checks
        checks += 1
        return checks < 3

    def capture(session_id: str, *_args: Any, **_kwargs: Any) -> dict[str, Any]:
        nonlocal captures
        captures += 1
        return {
            "success": True,
            "snapshot_id": f"snapshot-{captures}",
            "snapshot": {
                "root": {"id": "root", "role": "window", "children": []},
                "session_id": session_id,
                "metadata": {},
            },
        }

    monkeypatch.setattr(backend, "_native_desktop_interactive", desktop_interactive)
    monkeypatch.setattr(backend, "_capture_snapshot", capture)

    result = backend.wait_for_tool(
        {
            "session_id": "lock-during-wait",
            "condition": {
                "kind": "control_exists",
                "control_id": "never",
                "timeout_ms": 100,
                "interval_ms": 10,
            },
        }
    )

    assert result["success"] is False
    assert result["error"] == "desktop_unavailable"
    assert captures == 1


def test_app_ui_windows_uia_wait_stops_immediately_on_process_hotkey(monkeypatch: Any) -> None:
    backend = _load_windows_uia_module()
    interrupted = threading.Event()
    captures = 0

    class NativeBindings:
        @staticmethod
        def desktop_interactive() -> bool:
            return True

        @staticmethod
        def process_user_interrupted() -> bool:
            return interrupted.is_set()

    def capture(session_id: str, *_args: Any, **_kwargs: Any) -> dict[str, Any]:
        nonlocal captures
        captures += 1
        interrupted.set()
        return {
            "success": True,
            "snapshot_id": f"snapshot-{captures}",
            "snapshot": {
                "root": {"id": "root", "role": "window", "children": []},
                "session_id": session_id,
                "metadata": {},
            },
        }

    monkeypatch.setattr(backend, "_ComputerUseSession", NativeBindings)
    monkeypatch.setattr(backend, "_capture_snapshot", capture)

    result = backend.wait_for_tool(
        {
            "session_id": "hotkey-wait",
            "condition": {
                "kind": "control_exists",
                "control_id": "never",
                "timeout_ms": 60_000,
                "interval_ms": 10_000,
            },
        }
    )

    assert result["success"] is False
    assert result["error"] == "user_interrupted"
    assert captures == 1


def test_app_ui_windows_uia_explicit_stop_interrupts_wait_without_lock_delay(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    captured = threading.Event()

    def capture(session_id: str, *_args: Any, **_kwargs: Any) -> dict[str, Any]:
        captured.set()
        return {
            "success": True,
            "snapshot_id": "snapshot-1",
            "snapshot": {
                "root": {"id": "root", "role": "window", "children": []},
                "session_id": session_id,
                "metadata": {},
            },
        }

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_capture_snapshot", capture)

    with ThreadPoolExecutor(max_workers=2) as pool:
        waiting = pool.submit(
            backend.wait_for_tool,
            {
                "session_id": "explicit-stop-wait",
                "condition": {
                    "kind": "control_exists",
                    "control_id": "never",
                    "timeout_ms": 60_000,
                    "interval_ms": 10_000,
                },
            },
        )
        assert captured.wait(1)
        started = time.monotonic()
        stopped = backend.stop_computer_use_tool({"session_id": "explicit-stop-wait"})
        stop_elapsed = time.monotonic() - started
        result = waiting.result(timeout=1)

    assert stop_elapsed < 0.5
    assert stopped["success"] is True
    assert result["success"] is False
    assert result["error"] == "backend_unavailable"
    assert "stopped" in result["message"].lower()


def test_app_ui_windows_uia_desktop_unavailable_capture_retains_session_for_unlock(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    created = []

    class RecoveringComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            self.capture_count = 0
            self.stopped = False
            created.append(self)

        @staticmethod
        def desktop_interactive() -> bool:
            return True

        def start(self) -> str:
            return '{"success":true,"active":true}'

        def screenshot(self) -> tuple[str, bytes | None]:
            self.capture_count += 1
            if self.capture_count == 1:
                return (
                    '{"success":false,"error":"desktop_unavailable","message":"desktop locked"}',
                    None,
                )
            return (
                '{"success":true,"mime_type":"image/png","observation":{"observation_id":"after-unlock"}}',
                b"png",
            )

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", RecoveringComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    unavailable = backend.snapshot_tool({"session_id": "unlock", "window_handle": 500})

    assert unavailable["success"] is False
    assert unavailable["error"] == "desktop_unavailable"
    assert len(created) == 1
    assert created[0].stopped is False
    assert "unlock" in backend._COMPUTER_USE_SESSIONS
    assert "unlock" not in backend._COMPUTER_USE_OBSERVATIONS

    recovered = backend.snapshot_tool({"session_id": "unlock", "window_handle": 500})

    assert recovered["success"] is True
    assert recovered["context"]["observation"]["observation_id"] == "after-unlock"
    assert len(created) == 1


def test_app_ui_windows_uia_desktop_unavailable_start_retains_session_for_unlock(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    created = []

    class RecoveringStartComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            self.start_count = 0
            self.stopped = False
            created.append(self)

        @staticmethod
        def desktop_interactive() -> bool:
            return True

        def start(self) -> str:
            self.start_count += 1
            if self.start_count == 1:
                return '{"success":false,"error":"desktop_unavailable","message":"desktop locked"}'
            return '{"success":true,"active":true}'

        def screenshot(self) -> tuple[str, bytes | None]:
            return (
                '{"success":true,"mime_type":"image/png","observation":{"observation_id":"after-unlock"}}',
                b"png",
            )

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", RecoveringStartComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    unavailable = backend.snapshot_tool({"session_id": "unlock-start", "window_handle": 500})

    assert unavailable["success"] is False
    assert unavailable["error"] == "desktop_unavailable"
    assert len(created) == 1
    assert created[0].stopped is False
    assert "unlock-start" in backend._COMPUTER_USE_SESSIONS

    recovered = backend.snapshot_tool({"session_id": "unlock-start", "window_handle": 500})

    assert recovered["success"] is True
    assert recovered["context"]["observation"]["observation_id"] == "after-unlock"
    assert len(created) == 1


def test_app_ui_windows_uia_temporarily_hidden_target_retains_active_session(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    created = []

    class RecoveringTargetComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            self.capture_count = 0
            self.stopped = False
            created.append(self)

        @staticmethod
        def desktop_interactive() -> bool:
            return True

        def start(self) -> str:
            return '{"success":true,"active":true}'

        def status(self) -> str:
            return '{"success":true,"active":true,"overlay_visible":false}'

        def screenshot(self) -> tuple[str, bytes | None]:
            self.capture_count += 1
            if self.capture_count == 1:
                return (
                    '{"success":false,"error":"missing_window","message":"target temporarily hidden"}',
                    None,
                )
            return (
                '{"success":true,"mime_type":"image/png","observation":{"observation_id":"visible-again"}}',
                b"png",
            )

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", RecoveringTargetComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    unavailable = backend.snapshot_tool({"session_id": "target-return", "window_handle": 500})

    assert unavailable["success"] is False
    assert unavailable["error"] == "missing_window"
    assert len(created) == 1
    assert created[0].stopped is False
    assert "target-return" in backend._COMPUTER_USE_SESSIONS

    recovered = backend.snapshot_tool({"session_id": "target-return", "window_handle": 500})

    assert recovered["success"] is True
    assert recovered["context"]["observation"]["observation_id"] == "visible-again"
    assert len(created) == 1


def test_app_ui_windows_uia_falls_back_to_native_snapshot_for_exact_process(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    created = []

    class FakeComputerUseSession:
        def __init__(self, **kwargs: Any) -> None:
            created.append(kwargs)

        def start(self) -> str:
            return '{"success":true,"hint":"press Ctrl+Alt+Esc to stop"}'

        def screenshot(self) -> tuple[str, bytes]:
            return (
                '{"success":true,"mime_type":"image/png","observation":'
                '{"observation_id":"native-1","window_handle":500,"process_id":1234,'
                '"window_title":"Project - Godot","source_rect":[10,20,640,480]}}',
                b"png",
            )

        def stop(self) -> str:
            return '{"success":true}'

    def unavailable(_payload: dict[str, Any]) -> dict[str, Any]:
        raise RuntimeError("Windows UIA command timed out after 12 seconds")

    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", "1234")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", unavailable)

    result = backend.snapshot_tool({"session_id": "native-fallback", "process_id": 1234, "app_name": "Godot"})

    assert result["success"] is True
    assert "after Windows UIA was unavailable" in result["message"]
    assert "perform one scoped app_ui__act" in result["prompt"]
    assert created == [
        {
            "process_id": 1234,
            "window_handle": None,
            "window_title": None,
            "app_name": "Godot",
        }
    ]
    snapshot = result["context"]["snapshot"]
    assert snapshot["root"]["id"] == "native:process:1234"
    assert snapshot["root"]["label"] == "Project - Godot"
    assert snapshot["root"]["bounds"] == {
        "x": 10.0,
        "y": 20.0,
        "width": 640.0,
        "height": 480.0,
    }
    assert snapshot["root"]["metadata"]["app_ui"]["native_window_handle"] == 500
    assert snapshot["metadata"]["app_ui"]["backend"] == "windows-native-fallback"
    assert snapshot["metadata"]["app_ui"]["native_window_handle"] == 500
    assert "timed out" in snapshot["metadata"]["app_ui"]["fallback_reason"]
    assert result["context"]["observation"]["observation_id"] == "native-1"

    backend._stop_computer_use_session("native-fallback")
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    handle_result = backend.snapshot_tool(
        {"session_id": "native-handle-fallback", "window_handle": 500, "app_name": "Godot"}
    )
    assert handle_result["success"] is True
    assert created[-1] == {
        "process_id": None,
        "window_handle": 500,
        "window_title": None,
        "app_name": "Godot",
    }
    assert handle_result["context"]["snapshot"]["root"]["id"] == "native:window:500"

    backend._stop_computer_use_session("native-handle-fallback")
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", "1234")
    env_scoped = backend.snapshot_tool({"session_id": "native-env-fallback", "app_name": "Godot"})
    assert env_scoped["success"] is True
    assert created[-1]["process_id"] == 1234


def test_app_ui_windows_uia_does_not_fallback_without_exact_raw_scope(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_NAME", raising=False)

    class UnexpectedComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            raise AssertionError("native session must not start")

    def unavailable(_payload: dict[str, Any]) -> dict[str, Any]:
        raise RuntimeError("UIA unavailable")

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", UnexpectedComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", unavailable)

    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    semantic = backend.snapshot_tool({"session_id": "semantic", "process_id": 1234})
    assert semantic["success"] is False
    assert semantic["error"] == "backend_unavailable"

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    semantic_find = backend.find_tool({"session_id": "semantic-find", "process_id": 1234})
    assert semantic_find["success"] is False
    assert semantic_find["error"] == "backend_unavailable"

    for session_id, scope in (
        ("title-only-failure", {"window_title": "Godot"}),
        ("name-only-failure", {"process_name": "godot"}),
        ("name-combination-failure", {"process_id": 1234, "process_name": "godot"}),
    ):
        result = backend.snapshot_tool({"session_id": session_id, **scope})
        assert result["success"] is False
        assert result["error"] == "backend_unavailable"


def test_app_ui_windows_uia_snapshot_returns_native_png_without_raw_input(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    created = []

    class FakeComputerUseSession:
        def __init__(self, **kwargs: Any) -> None:
            self.kwargs = kwargs
            self.stopped = False
            created.append(self)

        def start(self) -> str:
            return json.dumps(
                {
                    "success": True,
                    "active": True,
                    "overlay_visible": True,
                    "hint": "DCC MCP Computer Use is controlling Godot - press Ctrl+Alt+Esc to stop",
                }
            )

        def screenshot(self) -> tuple[str, bytes]:
            return (
                json.dumps(
                    {
                        "success": True,
                        "mime_type": "image/png",
                        "observation": {
                            "observation_id": "500:1",
                            "window_handle": 500,
                            "process_id": 1234,
                            "width": 640,
                            "height": 480,
                        },
                    }
                ),
                b"\x89PNG\r\n\x1a\ncomputer-use",
            )

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    result = backend.snapshot_tool(
        {
            "session_id": "godot",
            "window_handle": 500,
            "app_name": "Godot",
        }
    )

    assert result["success"] is True
    assert created[0].kwargs == {
        "process_id": 1234,
        "window_handle": 500,
        "window_title": "Godot",
        "app_name": "Godot",
    }
    assert result["context"]["observation"]["observation_id"] == "500:1"
    assert result["context"]["snapshot"]["metadata"]["computer_use"]["width"] == 640
    assert result["context"]["control_hint"].endswith("press Ctrl+Alt+Esc to stop")
    rich = result["context"]["__rich__"]
    assert rich["kind"] == "image"
    assert rich["mime"] == "image/png"
    assert base64.b64decode(rich["data"]).startswith(b"\x89PNG")

    stopped = backend.stop_computer_use_tool({"session_id": "godot"})
    assert stopped["success"] is True
    assert stopped["context"]["active"] is False
    assert stopped["context"]["was_active"] is True
    assert created[0].stopped is True


def test_app_ui_windows_uia_semantic_mutation_starts_visible_session(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    calls = []

    class FakeComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            calls.append("created")

        def start(self) -> str:
            calls.append("started")
            return '{"success":true,"active":true,"overlay_visible":true}'

        def screenshot(self) -> tuple[str, bytes]:
            calls.append("screenshot")
            return ('{"success":true,"observation":{"observation_id":"semantic:1"}}', b"png")

        def stop(self) -> str:
            return '{"success":true}'

    def run_uia(payload: dict[str, Any]) -> dict[str, Any]:
        if payload["mode"] == "snapshot":
            return _uia_snapshot_payload()
        calls.append("uia-act")
        return {
            "ok": True,
            "message": "invoked control",
            "before_focus_runtime_id": "",
            "after_focus_runtime_id": "42.1",
        }

    monkeypatch.delenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", raising=False)
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", run_uia)

    result = backend.act_tool(
        {
            "session_id": "semantic-visible",
            "window_handle": 500,
            "control_id": "uia:42.1",
            "action": "click",
        }
    )

    assert result["success"] is True
    assert calls == ["created", "started", "screenshot", "uia-act"]
    backend.stop_computer_use_tool({"session_id": "semantic-visible"})

    calls.clear()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE")
    denied = backend.act_tool(
        {
            "session_id": "semantic-unbound",
            "process_id": 1234,
            "control_id": "uia:42.1",
            "action": "click",
        }
    )
    assert denied["success"] is False
    assert denied["error"] == "permission_denied"
    assert calls == []


def test_app_ui_windows_uia_stop_interrupts_inflight_native_action(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    action_started = threading.Event()
    stop_requested = threading.Event()
    created = []

    class FakeComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            self.stopped = False
            created.append(self)

        def start(self) -> str:
            return '{"success":true,"hint":"press Ctrl+Alt+Esc to stop"}'

        def screenshot(self) -> tuple[str, bytes]:
            return (
                '{"success":true,"mime_type":"image/png","observation":{"observation_id":"obs-stop"}}',
                b"png",
            )

        def act(self, _request_json: str) -> str:
            action_started.set()
            if not stop_requested.wait(5):
                raise AssertionError("request_stop did not interrupt the native action")
            return '{"success":false,"error":"backend_unavailable","message":"Computer Use stopped"}'

        def request_stop(self) -> None:
            stop_requested.set()

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    snapshot = backend.snapshot_tool({"session_id": "stop-race", "window_handle": 500})
    snapshot_id = snapshot["context"]["snapshot_id"]
    with ThreadPoolExecutor(max_workers=2) as pool:
        action = pool.submit(
            backend.act_tool,
            {
                "session_id": "stop-race",
                "action": "drag",
                "path": [{"x": 1, "y": 1}, {"x": 10, "y": 10}],
                "snapshot_id": snapshot_id,
                "duration_ms": 5_000,
            },
        )
        assert action_started.wait(1)
        started = time.monotonic()
        stopped = backend.stop_computer_use_tool({"session_id": "stop-race"})
        elapsed = time.monotonic() - started
        action_result = action.result(timeout=1)

    assert elapsed < 1
    assert stopped["success"] is True
    assert stopped["context"]["was_active"] is True
    assert action_result["error"] == "backend_unavailable"
    assert created[0].stopped is True
    assert "stop-race" not in backend._COMPUTER_USE_SESSIONS


@pytest.mark.skipif(sys.platform != "win32", reason="Windows UIA backend stop/action serialization is Windows-specific")
def test_app_ui_windows_uia_overlapping_stops_keep_actions_blocked(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    stop_entered = threading.Event()
    release_stop = threading.Event()

    class _Session:
        def request_stop(self) -> None:
            pass

        def stop(self) -> None:
            stop_entered.set()
            assert release_stop.wait(2)

    backend._COMPUTER_USE_SESSIONS["same"] = {"session": _Session()}
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(
        backend,
        "_capture_snapshot",
        lambda *_args, **_kwargs: (_ for _ in ()).throw(AssertionError("action bypassed overlapping stops")),
    )

    with ThreadPoolExecutor(max_workers=3) as pool:
        first = pool.submit(backend.stop_computer_use_tool, {"session_id": "same"})
        assert stop_entered.wait(1)
        second = pool.submit(backend.stop_computer_use_tool, {"session_id": "same"})
        blocked = backend.snapshot_tool({"session_id": "same"})
        release_stop.set()
        first.result(timeout=1)
        second.result(timeout=1)

    assert blocked["success"] is False
    assert blocked["error"] == "backend_unavailable"


def test_app_ui_wait_backends_cancel_on_package_stop(tmp_path: Path, monkeypatch: Any) -> None:
    modules = []
    for filename in ("_backend.py", "_chrome_backend.py", "_windows_uia_backend.py"):
        module_name = f"_test_cancel_{filename[:-3].lstrip('_')}"
        spec = importlib.util.spec_from_file_location(module_name, _SCRIPTS / filename)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        sys.path.insert(0, str(_SCRIPTS))
        try:
            spec.loader.exec_module(module)
        finally:
            sys.path.remove(str(_SCRIPTS))
        modules.append(module)

    _mock, chrome, windows = modules
    monkeypatch.setenv("DCC_MCP_APP_UI_MOCK_STATE_DIR", str(tmp_path / "mock"))
    monkeypatch.setenv("DCC_MCP_APP_UI_CHROME_STATE_DIR", str(tmp_path / "chrome"))
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path / "windows"))
    monkeypatch.setattr(chrome, "_ensure_chrome", lambda state: state)
    monkeypatch.setattr(chrome, "_refresh_from_browser", lambda state: state)
    monkeypatch.setattr(
        windows,
        "_capture_snapshot",
        lambda session_id, _policy, _params, **_kwargs: {
            "success": True,
            "snapshot_id": "snapshot",
            "snapshot": {
                "root": {"id": "root", "role": "window", "children": []},
                "session_id": session_id,
                "metadata": {},
            },
        },
    )

    for module in modules:
        with ThreadPoolExecutor(max_workers=1) as pool:
            waiting = pool.submit(
                module.wait_for_tool,
                {
                    "session_id": "cleanup",
                    "condition": {
                        "kind": "control_exists",
                        "control_id": "never",
                        "timeout_ms": 600_000,
                        "interval_ms": 10_000,
                    },
                },
            )
            time.sleep(0.05)
            started = time.monotonic()
            module.request_stop()
            result = waiting.result(timeout=1)
        assert time.monotonic() - started < 1
        assert result["success"] is False
        assert result["error"] == "backend_unavailable"


def test_app_ui_windows_uia_cleanup_stops_all_sessions() -> None:
    backend = _load_windows_uia_module()

    class _Session:
        def __init__(self) -> None:
            self.stop_requested = False
            self.stopped = False

        def request_stop(self) -> None:
            self.stop_requested = True

        def stop(self) -> None:
            self.stopped = True

    session = _Session()
    backend._COMPUTER_USE_SESSIONS["godot"] = {"session": session}
    backend._COMPUTER_USE_OBSERVATIONS["godot"] = {"observation_id": "obs-1"}

    backend.request_stop()
    assert session.stop_requested is True
    backend.cleanup()

    assert session.stopped is True
    assert backend._COMPUTER_USE_SESSIONS == {}
    assert backend._COMPUTER_USE_OBSERVATIONS == {}


def test_app_ui_windows_uia_stop_retains_pending_cleanup_for_retry(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()

    class PendingThenCleanSession:
        def __init__(self) -> None:
            self.stop_calls = 0

        def request_stop(self) -> None:
            pass

        def stop(self) -> str:
            self.stop_calls += 1
            return json.dumps(
                {
                    "success": True,
                    "active": False,
                    "cleanup_pending": self.stop_calls == 1,
                }
            )

    session = PendingThenCleanSession()
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    backend._COMPUTER_USE_SESSIONS["pending"] = {"session": session}

    pending = backend.stop_computer_use_tool({"session_id": "pending"})

    assert pending["success"] is False
    assert pending["context"]["cleanup_pending"] is True
    assert "pending" in backend._COMPUTER_USE_SESSIONS

    cleaned = backend.stop_computer_use_tool({"session_id": "pending"})

    assert cleaned["success"] is True
    assert cleaned["context"]["cleanup_pending"] is False
    assert "pending" not in backend._COMPUTER_USE_SESSIONS
    assert session.stop_calls == 2


def test_app_ui_windows_uia_process_stop_hotkey_blocks_semantic_actions(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()

    class InterruptedComputerUseSession:
        @staticmethod
        def process_user_interrupted() -> bool:
            return True

    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", InterruptedComputerUseSession)
    monkeypatch.setattr(
        backend,
        "_run_uia",
        lambda _payload: (_ for _ in ()).throw(AssertionError("UIA must remain blocked after Ctrl+Alt+Esc")),
    )

    result = backend.act_tool(
        {
            "session_id": "changed-session-id",
            "process_id": 1234,
            "control_id": "uia:42.1",
            "action": "set_text",
            "text": "must not be sent",
        }
    )

    assert result["success"] is False
    assert result["error"] == "user_interrupted"
    assert "changed-session-id" in backend._COMPUTER_USE_INTERRUPTED


def test_app_ui_windows_uia_action_limits_apply_before_backend_dispatch(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    too_much_text = backend.act_tool(
        {
            "session_id": "bounded",
            "process_id": 1234,
            "control_id": "uia:42.1",
            "action": "set_text",
            "text": "x" * 4097,
        }
    )
    assert too_much_text["success"] is False
    assert too_much_text["error"] == "invalid_action"

    too_many_keys = backend.act_tool(
        {
            "session_id": "bounded",
            "process_id": 1234,
            "action": "keypress",
            "keys": ["CTRL+SHIFT+ALT+A+B+C+D+E+F+G+H+I+J+K+L+M+N"],
        }
    )
    assert too_many_keys["success"] is False
    assert too_many_keys["error"] == "invalid_action"


def test_app_ui_windows_uia_native_control_requires_operator_bound_pid_or_hwnd(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)

    class UnexpectedComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            raise AssertionError("native session must not start from title-only scope")

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", UnexpectedComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    result = backend.snapshot_tool({"session_id": "untrusted", "window_handle": 500})

    assert result["success"] is False
    assert result["error"] == "permission_denied"
    assert "operator-bound DCC scope" in result["message"]


@pytest.mark.parametrize(
    ("scope_variable", "scope_value", "scope_label"),
    [
        ("DCC_MCP_APP_UI_UIA_PROCESS_ID", "9999", "process"),
        ("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "999", "window"),
    ],
)
def test_app_ui_windows_uia_rejects_native_target_outside_operator_scope(
    tmp_path: Path,
    monkeypatch: Any,
    scope_variable: str,
    scope_value: str,
    scope_label: str,
) -> None:
    backend = _load_windows_uia_module()
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_PROCESS_ID", raising=False)
    monkeypatch.delenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", raising=False)
    monkeypatch.setenv(scope_variable, scope_value)

    class UnexpectedComputerUseSession:
        def __init__(self, **_kwargs: Any) -> None:
            raise AssertionError("an out-of-scope UIA target must not create a native session")

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", UnexpectedComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    result = backend.snapshot_tool({"session_id": f"wrong-{scope_label}"})

    assert result["success"] is False
    assert result["error"] == "invalid_target"
    assert f"operator-bound DCC {scope_label} scope" in result["message"]


def test_app_ui_windows_uia_failed_semantic_dispatch_consumes_native_observation(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    native_requests = []

    class NativeBindings:
        @staticmethod
        def desktop_interactive() -> bool:
            return True

        @staticmethod
        def process_user_interrupted() -> bool:
            return False

    class NativeSession:
        def act(self, request_json: str) -> str:
            native_requests.append(json.loads(request_json))
            return '{"success":true}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", NativeBindings)
    monkeypatch.setattr(
        backend,
        "_capture_snapshot",
        lambda *_args, **_kwargs: {
            "success": True,
            "scope": {
                "process_ids": [1234],
                "process_names": [],
                "window_handles": [],
                "native_scope_trusted": True,
                "invalid_reason": None,
            },
            "snapshot": {
                "root": {
                    "id": "uia:42.1",
                    "role": "window",
                    "label": "Godot",
                    "children": [],
                }
            },
        },
    )
    monkeypatch.setattr(
        backend,
        "_computer_use_screenshot",
        lambda *_args, **_kwargs: {"success": True},
    )
    monkeypatch.setattr(
        backend,
        "_run_uia",
        lambda _payload: {
            "ok": False,
            "error": "backend_error",
            "message": "UIA action may have partially changed the application",
        },
    )
    state = {"session_id": "consume", "revision": 1, "last_snapshot_id": "consume:1"}
    backend._save_state(state)
    backend._COMPUTER_USE_SESSIONS["consume"] = {"session": NativeSession()}
    backend._COMPUTER_USE_OBSERVATIONS["consume"] = {
        "snapshot_id": "consume:1",
        "observation_id": "native:1",
    }

    failed = backend.act_tool(
        {
            "session_id": "consume",
            "snapshot_id": "consume:1",
            "control_id": "uia:42.1",
            "action": "focus",
        }
    )
    assert failed["success"] is False
    assert backend._load_state("consume")["last_snapshot_id"] == "consume:2"
    assert "consume" not in backend._COMPUTER_USE_OBSERVATIONS

    stale = backend.act_tool(
        {
            "session_id": "consume",
            "snapshot_id": "consume:1",
            "action": "click",
            "x": 10,
            "y": 20,
        }
    )

    assert stale["success"] is False
    assert stale["error"] == "stale_observation"
    assert native_requests == []


def test_app_ui_windows_uia_native_actions_require_latest_snapshot_and_preserve_escape(
    tmp_path: Path,
    monkeypatch: Any,
) -> None:
    backend = _load_windows_uia_module()
    requests = []

    class FakeComputerUseSession:
        interrupted = False

        def __init__(self, **_kwargs: Any) -> None:
            self.stopped = False

        def start(self) -> str:
            return '{"success":true,"hint":"press Ctrl+Alt+Esc to stop"}'

        def status(self) -> str:
            return json.dumps({"success": True, "user_interrupted": self.interrupted})

        def screenshot(self) -> tuple[str, bytes]:
            return (
                '{"success":true,"mime_type":"image/png","observation":{"observation_id":"obs-7"}}',
                b"png",
            )

        def act(self, request_json: str) -> str:
            request = json.loads(request_json)
            requests.append(request)
            return '{"success":true,"requires_new_screenshot":true}'

        def stop(self) -> str:
            self.stopped = True
            return '{"success":true}'

        def resume_after_user_approval(self) -> str:
            self.interrupted = False
            return '{"success":true,"user_interrupted":false}'

    monkeypatch.setenv("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT", "true")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE", "500")
    monkeypatch.setenv("DCC_MCP_APP_UI_UIA_STATE_DIR", str(tmp_path))
    monkeypatch.setattr(backend, "_ComputerUseSession", FakeComputerUseSession)
    monkeypatch.setattr(backend, "_run_uia", lambda _payload: _uia_snapshot_payload())

    snapshot = backend.snapshot_tool({"session_id": "native", "window_handle": 500})
    snapshot_id = snapshot["context"]["snapshot_id"]

    missing = backend.act_tool({"session_id": "native", "action": "click", "x": 10, "y": 20})
    assert missing["success"] is False
    assert missing["error"] == "stale_observation"
    assert requests == []

    clicked = backend.act_tool(
        {
            "session_id": "native",
            "action": "click",
            "x": 10,
            "y": 20,
            "button": "left",
            "duration_ms": 75,
            "snapshot_id": snapshot_id,
        }
    )
    assert clicked["success"] is True
    assert requests[0] == {
        "action": "click",
        "observation_id": "obs-7",
        "x": 10,
        "y": 20,
        "button": "left",
        "duration_ms": 75,
    }

    refreshed = backend.snapshot_tool({"session_id": "native", "window_handle": 500})
    escaped = backend.act_tool(
        {
            "session_id": "native",
            "action": "keypress",
            "keys": ["ESC"],
            "snapshot_id": refreshed["context"]["snapshot_id"],
        }
    )
    assert escaped["success"] is True
    assert requests[1]["action"] == "keypress"
    assert requests[1]["keys"] == ["ESC"]
    assert "native" in backend._COMPUTER_USE_SESSIONS

    FakeComputerUseSession.interrupted = True
    blocked = backend.snapshot_tool({"session_id": "native", "window_handle": 500})
    assert blocked["success"] is False
    assert blocked["error"] == "user_interrupted"
    assert "Ctrl+Alt+Esc" in blocked["message"]
    assert "resume_computer_use=true" in blocked["message"]

    resumed = backend.snapshot_tool({"session_id": "native", "window_handle": 500, "resume_computer_use": True})
    assert resumed["success"] is True


def _uia_snapshot_payload() -> dict[str, Any]:
    return {
        "ok": True,
        "focus_runtime_id": "",
        "node_count": 1,
        "root": {
            "runtime_id": "42.1",
            "fallback_path": "0",
            "name": "Godot",
            "automation_id": "",
            "class_name": "Godot",
            "control_type": "ControlType.Window",
            "process_id": 1234,
            "native_window_handle": 500,
            "enabled": True,
            "offscreen": False,
            "bounds": {"x": 0, "y": 0, "width": 640, "height": 480},
            "children": [],
        },
    }


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
