"""Tests for ``register_diagnostic_mcp_tools``.

Covers:
- All four ``dcc_diagnostics__*`` tools get registered in the server's ToolRegistry.
- Each tool has a handler wired through :class:`McpHttpServer.register_handler`.
- ``dcc_diagnostics__process_status`` returns the instance context via its handler.
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
    "dcc_diagnostics__screenshot",
    "dcc_diagnostics__audit_log",
    "dcc_diagnostics__tool_metrics",
    "dcc_diagnostics__process_status",
    "dcc_diagnostics__gateway_failover",
    "dcc_diagnostics__get_instance_info",
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


class TestGetInstanceInfoHandler:
    def test_reports_context(self) -> None:
        # Import local modules
        from dcc_mcp_core.dcc_server import _handle_get_instance_info
        from dcc_mcp_core.dcc_server import _instance_context

        original = dict(_instance_context)
        _instance_context.update(
            {
                "dcc_name": "maya",
                "dcc_pid": 99999,
                "dcc_window_handle": None,
                "dcc_window_title": "Maya 2024",
                "dcc_version": "2024.2",
                "resolver": None,
                "gateway_failover_resolver": None,
            }
        )
        try:
            payload = json.loads(_handle_get_instance_info("{}"))
            assert payload["success"] is True
            assert payload["dcc_name"] == "maya"
            assert payload["dcc_pid"] == 99999
            assert payload["dcc_version"] == "2024.2"
            assert isinstance(payload["adapter_pid"], int)
            assert isinstance(payload["timestamp_ms"], int)
            # Fields that are None when no server is wired
            assert payload["instance_uuid"] is None
            assert payload["mcp_url"] is None
            assert payload["server_port"] is None
            assert payload["gateway_port"] is None
            # Static fields
            assert payload["dcc_mcp_core_version"] is not None
            assert payload["python_version"] is not None
        finally:
            _instance_context.update(original)

    def test_with_server_ref(self, server) -> None:
        """Handler returns server-attached fields when server is wired."""
        # Import local modules
        from dcc_mcp_core.dcc_server import _handle_get_instance_info
        from dcc_mcp_core.dcc_server import _instance_context
        from dcc_mcp_core.dcc_server import _server_ref

        register_diagnostic_mcp_tools(
            server,
            dcc_name="test-dcc",
            dcc_pid=12345,
            dcc_version="2025.1",
        )

        try:
            payload = json.loads(_handle_get_instance_info("{}"))
            assert payload["success"] is True
            assert payload["dcc_name"] == "test-dcc"
            assert payload["dcc_pid"] == 12345
            assert payload["dcc_version"] == "2025.1"
            # Server ref should be set by register_diagnostic_mcp_tools
            assert _server_ref is not None
        finally:
            _instance_context.update(
                {
                    "dcc_name": None,
                    "dcc_pid": None,
                    "dcc_window_handle": None,
                    "dcc_window_title": None,
                    "dcc_version": None,
                    "resolver": None,
                    "gateway_failover_resolver": None,
                }
            )


class TestToolCategoryAndMetadata:
    def test_tools_have_diagnostics_category(self, server) -> None:
        register_diagnostic_mcp_tools(server, dcc_name="test-dcc")
        reg = server.registry
        for name in EXPECTED_TOOLS:
            meta = reg.get_action(name)
            assert meta is not None
            assert meta["category"] == "diagnostics"
            assert meta["dcc"] == "test-dcc"
