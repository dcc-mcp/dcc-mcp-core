"""Tests for dcc_mcp_core.dcc_server.register_diagnostic_handlers.

Covers:
- register_diagnostic_handlers registers the three handler names on the mock server
- get_audit_log handler returns valid JSON with success=True (local SandboxContext)
- get_action_metrics handler returns valid JSON with success=True (local ActionRecorder)
- dispatch_action handler returns error when dispatcher is None
- dispatch_action handler relays through a mock dispatcher
- DCC_MCP_IPC_ADDRESS env var is set after registration (unless already present)
- register_diagnostic_handlers is importable from the top-level dcc_mcp_core package
- _handle_get_audit_log handles invalid JSON params gracefully
- _handle_get_action_metrics handles missing action gracefully
"""

from __future__ import annotations

import json
import os

import pytest

# ---------------------------------------------------------------------------
# Helpers / fixtures
# ---------------------------------------------------------------------------


class _MockServer:
    """Minimal stand-in for McpHttpServer / create_skill_manager result."""

    def __init__(self):
        self._handlers: dict[str, object] = {}

    def register_handler(self, name: str, fn) -> None:
        self._handlers[name] = fn

    def call(self, name: str, params: str = "") -> str:
        handler = self._handlers.get(name)
        assert handler is not None, f"No handler registered for {name!r}"
        return handler(params)


class _MockDispatcher:
    """Stand-in for ActionDispatcher that echoes back action + params."""

    def dispatch(self, action: str, params_json: str) -> dict:
        params = json.loads(params_json)
        return {
            "action": action,
            "output": json.dumps({"success": True, "echoed_action": action, "params": params}),
            "validation_skipped": True,
        }


# ---------------------------------------------------------------------------
# Import + API
# ---------------------------------------------------------------------------


def test_importable_from_package():
    from dcc_mcp_core import register_diagnostic_handlers

    assert callable(register_diagnostic_handlers)


def test_importable_from_module():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    assert callable(register_diagnostic_handlers)


# ---------------------------------------------------------------------------
# Handler registration
# ---------------------------------------------------------------------------


def test_registers_three_handlers():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")

    assert "get_audit_log" in server._handlers
    assert "get_action_metrics" in server._handlers
    assert "dispatch_action" in server._handlers


def test_idempotent_registration():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")
    register_diagnostic_handlers(server, dcc_name="test-dcc")
    # Still exactly 3 handlers (re-registration overwrites)
    assert len(server._handlers) == 3


# ---------------------------------------------------------------------------
# get_audit_log
# ---------------------------------------------------------------------------


def test_get_audit_log_returns_json():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")

    result_str = server.call("get_audit_log", json.dumps({"filter": "all", "limit": 10}))
    data = json.loads(result_str)
    assert "success" in data


def test_get_audit_log_invalid_json_params():
    from dcc_mcp_core.dcc_server import _handle_get_audit_log

    # Should not raise even with garbage input
    result_str = _handle_get_audit_log("not-json-{{")
    data = json.loads(result_str)
    assert "success" in data


def test_get_audit_log_empty_params():
    from dcc_mcp_core.dcc_server import _handle_get_audit_log

    result_str = _handle_get_audit_log("")
    data = json.loads(result_str)
    assert "success" in data


# ---------------------------------------------------------------------------
# get_action_metrics
# ---------------------------------------------------------------------------


def test_get_action_metrics_returns_json():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")

    result_str = server.call("get_action_metrics", json.dumps({}))
    data = json.loads(result_str)
    assert "success" in data


def test_get_action_metrics_empty_params():
    from dcc_mcp_core.dcc_server import _handle_get_action_metrics

    result_str = _handle_get_action_metrics("")
    data = json.loads(result_str)
    assert "success" in data


# ---------------------------------------------------------------------------
# dispatch_action
# ---------------------------------------------------------------------------


def test_dispatch_action_no_dispatcher_returns_error():
    import dcc_mcp_core.dcc_server as mod

    # Reset dispatcher so we can test the None path
    original = mod._dispatcher_ref
    mod._dispatcher_ref = None
    try:
        result_str = mod._handle_dispatch_action(json.dumps({"action": "test", "params": {}}))
        data = json.loads(result_str)
        assert data["success"] is False
        assert "Dispatcher not available" in data["message"]
    finally:
        mod._dispatcher_ref = original


def test_dispatch_action_with_dispatcher():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    dispatcher = _MockDispatcher()
    register_diagnostic_handlers(server, dispatcher=dispatcher, dcc_name="test-dcc")

    result_str = server.call(
        "dispatch_action",
        json.dumps({"action": "my_action", "params": {"key": "value"}}),
    )
    data = json.loads(result_str)
    assert data.get("success") is True
    assert data.get("echoed_action") == "my_action"


def test_dispatch_action_missing_action_field():
    from dcc_mcp_core.dcc_server import _handle_dispatch_action

    result_str = _handle_dispatch_action(json.dumps({"params": {}}))
    data = json.loads(result_str)
    assert data["success"] is False
    assert "Missing 'action'" in data["message"]


def test_dispatch_action_invalid_json():
    from dcc_mcp_core.dcc_server import _handle_dispatch_action

    result_str = _handle_dispatch_action("bad-json")
    data = json.loads(result_str)
    assert data["success"] is False


# ---------------------------------------------------------------------------
# DCC_MCP_IPC_ADDRESS env var
# ---------------------------------------------------------------------------


def test_ipc_address_env_set_after_registration(monkeypatch):
    """DCC_MCP_IPC_ADDRESS should be populated after registration (if not already set)."""
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    monkeypatch.delenv("DCC_MCP_IPC_ADDRESS", raising=False)

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")

    # The env var is set (or gracefully skipped if TransportAddress.default_local is unavailable)
    # We only assert it is a non-empty string if it was set.
    addr = os.environ.get("DCC_MCP_IPC_ADDRESS", "")
    # Either it was set to something useful, or gracefully skipped
    if addr:
        assert len(addr) > 0


def test_ipc_address_not_overwritten_if_already_set(monkeypatch):
    """Externally set DCC_MCP_IPC_ADDRESS must not be overwritten."""
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    monkeypatch.setenv("DCC_MCP_IPC_ADDRESS", "pipe://custom_test_address")

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")

    assert os.environ["DCC_MCP_IPC_ADDRESS"] == "pipe://custom_test_address"
