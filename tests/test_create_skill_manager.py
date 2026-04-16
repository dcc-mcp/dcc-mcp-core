"""Tests for create_skill_server — Skills-First one-call DCC server factory.

Covers:
- Basic instantiation (no env vars, no skills discovered)
- Returns a McpHttpServer instance
- Custom McpHttpConfig is honoured
- app_name determines which DCC_MCP_{APP}_SKILL_PATHS env var is read (not server name)
- dcc_name override is accepted
- extra_paths parameter is accepted
- Server can be started and returns a handle with valid mcp_url
- Skill discovery/listing methods are accessible on the returned server
"""

from __future__ import annotations

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import create_skill_server


class TestCreateSkillManagerBasic:
    """Basic creation and type checks."""

    def test_returns_mcp_http_server(self):
        server = create_skill_server("maya")
        assert isinstance(server, McpHttpServer)

    def test_default_config_port_is_8765(self):
        server = create_skill_server("maya")
        # repr exposes port
        assert "8765" in repr(server)

    def test_default_server_name_in_repr(self):
        # app_name determines which env var to read for skill paths, NOT the server name.
        # The default server name is the APP_NAME constant ("dcc-mcp").
        server = create_skill_server("blender")
        r = repr(server).lower()
        assert "dcc-mcp" in r

    def test_custom_server_name_in_repr(self):
        cfg = McpHttpConfig(server_name="blender")
        server = create_skill_server("blender", config=cfg)
        assert "blender" in repr(server).lower()

    def test_custom_config_port_honoured(self):
        cfg = McpHttpConfig(port=9999, server_name="test-server")
        server = create_skill_server("maya", config=cfg)
        assert "9999" in repr(server)

    def test_config_exposes_http_runtime_fields(self):
        cfg = McpHttpConfig(port=8765, enable_cors=True, request_timeout_ms=1234)
        assert cfg.host == "127.0.0.1"
        assert cfg.endpoint_path == "/mcp"
        assert cfg.max_sessions == 100
        assert cfg.enable_cors is True
        assert cfg.request_timeout_ms == 1234

    def test_extra_paths_empty_list_accepted(self):
        server = create_skill_server("maya", extra_paths=[])
        assert isinstance(server, McpHttpServer)

    def test_extra_paths_nonexistent_dir_accepted(self):
        # Non-existent dirs are silently filtered by get_app_skill_paths_from_env
        server = create_skill_server("maya", extra_paths=["/nonexistent/path/xyz"])
        assert isinstance(server, McpHttpServer)

    def test_dcc_name_override_accepted(self):
        server = create_skill_server("my-app", dcc_name="maya")
        assert isinstance(server, McpHttpServer)

    def test_no_skills_discovered_without_env(self, monkeypatch):
        monkeypatch.delenv("DCC_MCP_MAYA_SKILL_PATHS", raising=False)
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        server = create_skill_server("maya")
        skills = server.list_skills()
        assert isinstance(skills, list)
        assert len(skills) == 0


class TestCreateSkillManagerServerHandle:
    """Verify the returned server can be started and shut down."""

    def test_start_returns_handle_with_port(self):
        server = create_skill_server("maya", config=McpHttpConfig(port=0))
        handle = server.start()
        try:
            assert handle.port > 0
        finally:
            handle.shutdown()

    def test_start_returns_handle_with_mcp_url(self):
        server = create_skill_server("maya", config=McpHttpConfig(port=0))
        handle = server.start()
        try:
            url = handle.mcp_url()
            assert url.startswith("http://")
            assert url.endswith("/mcp")
        finally:
            handle.shutdown()

    def test_bind_addr_contains_port(self):
        server = create_skill_server("maya", config=McpHttpConfig(port=0))
        handle = server.start()
        try:
            assert str(handle.port) in handle.bind_addr
        finally:
            handle.shutdown()

    def test_double_shutdown_is_safe(self):
        server = create_skill_server("maya", config=McpHttpConfig(port=0))
        handle = server.start()
        handle.shutdown()
        handle.shutdown()  # Second shutdown must not raise


class TestCreateSkillManagerSkillMethods:
    """Verify skill catalog methods are accessible on the returned server."""

    @pytest.fixture
    def server(self):
        return create_skill_server("maya")

    def test_list_skills_returns_list(self, server):
        result = server.list_skills()
        assert isinstance(result, list)

    def test_list_skills_with_status_filter_accepted(self, server):
        result = server.list_skills(status="loaded")
        assert isinstance(result, list)

    def test_find_skills_returns_list(self, server):
        result = server.find_skills()
        assert isinstance(result, list)

    def test_find_skills_with_query(self, server):
        result = server.find_skills(query="nonexistent_skill_xyz")
        assert isinstance(result, list)
        assert len(result) == 0

    def test_loaded_count_returns_int(self, server):
        count = server.loaded_count()
        assert isinstance(count, int)
        assert count >= 0

    def test_is_loaded_returns_bool(self, server):
        result = server.is_loaded("nonexistent_skill")
        assert result is False

    def test_catalog_property_is_string(self, server):
        info = server.catalog
        assert isinstance(info, str)
        assert "SkillCatalog" in info

    def test_discover_returns_int(self, server, monkeypatch):
        monkeypatch.delenv("DCC_MCP_MAYA_SKILL_PATHS", raising=False)
        monkeypatch.delenv("DCC_MCP_SKILL_PATHS", raising=False)
        count = server.discover()
        assert isinstance(count, int)
        assert count >= 0

    def test_load_skill_raises_for_unknown(self, server):
        with pytest.raises((ValueError, Exception)):
            server.load_skill("nonexistent_skill_that_does_not_exist_xyz")

    def test_unload_skill_raises_for_unknown(self, server):
        with pytest.raises((ValueError, Exception)):
            server.unload_skill("nonexistent_skill_that_does_not_exist_xyz")


class TestCreateSkillManagerHandlerRegistration:
    """Verify register_handler and has_handler work on the returned server."""

    @pytest.fixture
    def server(self):
        from dcc_mcp_core import ToolRegistry

        reg = ToolRegistry()
        reg.register(
            "test_action",
            description="Test action",
            category="test",
            dcc="maya",
        )
        return create_skill_server("maya")

    def test_has_handler_false_before_registration(self, server):
        assert server.has_handler("test_action") is False

    def test_register_handler_callable(self, server):
        server.register_handler("test_action", lambda params: "ok")
        assert server.has_handler("test_action") is True

    def test_register_handler_non_callable_raises(self, server):
        with pytest.raises(TypeError):
            server.register_handler("test_action", "not-a-callable")
