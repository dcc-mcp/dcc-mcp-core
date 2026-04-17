"""Tests for ``register_diagnostic_mcp_tools``.

Covers:
- All four ``diagnostics__*`` tools get registered in the server's ToolRegistry.
- Each tool has a handler wired through :class:`McpHttpServer.register_handler`.
- ``diagnostics__process_status`` returns the instance context via its handler.
- Registration is idempotent when called twice.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import create_skill_server
from dcc_mcp_core import register_diagnostic_mcp_tools

# ── fixtures ─────────────────────────────────────────────────────────────────


@pytest.fixture
def server():
    """Return a fresh skills-first server instance (not started)."""
    return create_skill_server("test-dcc", McpHttpConfig(port=0))


# ── tool registration ────────────────────────────────────────────────────────


EXPECTED_TOOLS = [
    "diagnostics__screenshot",
    "diagnostics__audit_log",
    "diagnostics__tool_metrics",
    "diagnostics__process_status",
]


class TestRegisterDiagnosticMcpTools:
    def test_all_four_tools_registered(self, server) -> None:
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        reg = server.registry
        names = {entry["name"] for entry in reg.list_actions()}
        for name in EXPECTED_TOOLS:
            assert name in names, f"{name} not registered"

    def test_handlers_are_wired(self, server) -> None:
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        for name in EXPECTED_TOOLS:
            assert server.has_handler(name), f"{name} handler missing"

    def test_idempotent(self, server) -> None:
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        names = {entry["name"] for entry in server.registry.list_actions()}
        for name in EXPECTED_TOOLS:
            assert name in names

    def test_instance_context_populated(self, server) -> None:
        # Import local modules
        from dcc_mcp_core.dcc_server import _instance_context

        register_diagnostic_mcp_tools(
            server,
            dcc_name="test-dcc",
            dcc_pid=98765,
            dcc_window_title="Test App",
            dcc_window_handle=0xBEEF0001,
        )
        assert _instance_context["dcc_pid"] == 98765
        assert _instance_context["dcc_window_title"] == "Test App"
        assert _instance_context["dcc_window_handle"] == 0xBEEF0001


# ── handler behaviour ────────────────────────────────────────────────────────


class TestProcessStatusHandler:
    def test_reports_context(self) -> None:
        # Import local modules
        from dcc_mcp_core.dcc_server import _handle_process_status
        from dcc_mcp_core.dcc_server import _instance_context

        original = dict(_instance_context)
        _instance_context.update(
            {
                "dcc_name": "maya",
                "dcc_pid": 99999,  # unlikely to be alive
                "dcc_window_handle": None,
                "dcc_window_title": "Maya",
                "resolver": None,
            }
        )
        try:
            payload = json.loads(_handle_process_status("{}"))
            assert payload["success"] is True
            assert payload["dcc_name"] == "maya"
            assert payload["dcc_pid"] == 99999
            assert payload["dcc_window_title"] == "Maya"
            assert isinstance(payload["adapter_pid"], int)
            assert isinstance(payload["dcc_alive"], bool)
            assert isinstance(payload["timestamp_ms"], int)
        finally:
            _instance_context.update(original)


class TestToolCategoryAndMetadata:
    def test_tools_have_diagnostics_category(self, server) -> None:
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        reg = server.registry
        for name in EXPECTED_TOOLS:
            meta = reg.get_action(name)
            assert meta is not None
            assert meta["category"] == "diagnostics"
            assert meta["dcc"] == "test-dcc"
