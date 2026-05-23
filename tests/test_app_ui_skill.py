"""Tests for the bundled app-ui mock skill."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any

from conftest import REPO_ROOT

_SKILL_DIR = REPO_ROOT / "python" / "dcc_mcp_core" / "skills" / "app-ui"
_SCRIPTS = _SKILL_DIR / "scripts"


def _load_cdp_runtime_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_app_ui_cdp_runtime", _SCRIPTS / "_cdp_runtime.py")
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
    assert {tool.name for tool in meta.tools} == {"snapshot", "find", "act", "wait_for"}

    registry = ToolRegistry()
    catalog = SkillCatalog(registry)
    catalog.discover(extra_paths=[str(_SKILL_DIR.parent)])
    catalog.load_skill("app-ui")
    action_names = {action["name"] for action in registry.list_actions()}
    assert "app_ui__snapshot" in action_names
    assert "app_ui__wait_for" in action_names


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
    ]


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
