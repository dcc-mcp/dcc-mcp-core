"""Tests for py37-lite sidecar server factory and pure-Python McpHttpConfig."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class
from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig
from dcc_mcp_core._runtime.server_factory import create_adapter_server
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer


@dataclass(frozen=True)
class _SidecarStub:
    host_rpc: str
    adapter_version: str | None = None
    display_name: str | None = None
    wait_ready_timeout_secs: float = 15.0
    server_bin: str | None = None
    extra_args: tuple[str, ...] = ()


@dataclass(frozen=True)
class _DiagnosticsStub:
    dcc_pid: int | None = None


@dataclass(frozen=True)
class _OptionsStub:
    dcc_name: str
    sidecar: _SidecarStub
    diagnostics: _DiagnosticsStub = _DiagnosticsStub()


class TestPureMcpHttpConfig:
    def test_defaults_match_rust_surface(self):
        cfg = PureMcpHttpConfig()
        assert cfg.port == 8765
        assert cfg.server_name == "dcc-mcp"
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0
        assert cfg.gateway_port == 9765
        assert cfg.session_ttl_secs == 3600

    def test_repr_contains_name_and_port(self):
        cfg = PureMcpHttpConfig(port=1234, server_name="maya-mcp")
        text = repr(cfg)
        assert "McpHttpConfig" in text
        assert "1234" in text
        assert "maya-mcp" in text


class TestServerFactoryRouting:
    def test_uses_sidecar_backend_when_core_unavailable(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
            lambda: False,
        )
        options = _OptionsStub(
            dcc_name="maya",
            sidecar=_SidecarStub(
                host_rpc="commandport://127.0.0.1:6000",
                server_bin="dcc-mcp-server-test",
                extra_args=("--ppid-poll-ms", "50"),
            ),
        )
        config = PureMcpHttpConfig(port=8765, server_name="maya-mcp")
        server = create_adapter_server("maya", config, options)
        assert isinstance(server, SidecarBackedSkillServer)
        assert server.backend == "sidecar"
        assert server._server_bin == "dcc-mcp-server-test"
        assert server._extra_args == ("--ppid-poll-ms", "50")

    def test_uses_create_skill_server_when_core_available(self, monkeypatch: pytest.MonkeyPatch):
        import sys
        import types

        fake_server = MagicMock(name="embedded-server")
        fake_core = types.ModuleType("dcc_mcp_core._core")

        def _fake_create_skill_server(app_name, config):
            assert app_name == "maya"
            assert config is not None
            return fake_server

        fake_core.create_skill_server = _fake_create_skill_server
        monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", fake_core)
        monkeypatch.setattr(
            "dcc_mcp_core._runtime.server_factory.is_core_extension_available",
            lambda: True,
        )
        options = SimpleNamespace(sidecar=_SidecarStub(host_rpc="commandport://127.0.0.1:6000"))
        config = PureMcpHttpConfig()
        server = create_adapter_server("maya", config, options)
        assert server is fake_server


class TestSidecarBackedSkillServer:
    def test_discover_records_paths_without_core(self):
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(),
            host_rpc="commandport://127.0.0.1:6000",
        )
        count = server.discover(extra_paths=["/tmp/skills"])
        assert count == 1

    def test_start_launches_sidecar_and_returns_handle(self, monkeypatch: pytest.MonkeyPatch):
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(port=8765),
            host_rpc="commandport://127.0.0.1:6000",
        )

        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            lambda **kwargs: {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": MagicMock(),
            },
        )

        handle = server.start()
        assert handle.port == 9876
        assert handle.mcp_url() == "http://127.0.0.1:9876/mcp"

    def test_start_requires_host_rpc(self):
        server = SidecarBackedSkillServer("maya", PureMcpHttpConfig(), host_rpc="")
        with pytest.raises(RuntimeError, match="host_rpc"):
            server.start()

    def test_start_forwards_server_bin_and_extra_args(self, monkeypatch: pytest.MonkeyPatch):
        captured: dict = {}

        def _fake_launch(**kwargs):
            captured.update(kwargs)
            return {
                "success": True,
                "readiness": {"mcp_url": "http://127.0.0.1:9876/mcp"},
                "process": MagicMock(),
            }

        monkeypatch.setattr(
            "dcc_mcp_core._runtime.sidecar_skill_server.launch_sidecar",
            _fake_launch,
        )
        server = SidecarBackedSkillServer(
            "maya",
            PureMcpHttpConfig(port=8765),
            host_rpc="commandport://127.0.0.1:6000",
            server_bin="dcc-mcp-server-test",
            extra_args=("--ppid-poll-ms", "50"),
        )
        server.start()
        assert captured.get("server_bin") == "dcc-mcp-server-test"
        assert captured.get("extra_args") == ("--ppid-poll-ms", "50")


class TestExecutionBridgeLazyCore:
    def test_sandbox_context_skips_core_when_unavailable(self, monkeypatch: pytest.MonkeyPatch):
        monkeypatch.setattr(
            "dcc_mcp_core._server.execution_bridge.is_core_extension_available",
            lambda: False,
        )
        from dcc_mcp_core._server.execution_bridge import _sandbox_context

        assert _sandbox_context(MagicMock()) is None


@pytest.mark.skipif(is_core_extension_available(), reason="only meaningful on py37-lite profile")
class TestServerBaseImportLight:
    def test_server_base_imports_fallbacks_without_core(self, monkeypatch: pytest.MonkeyPatch):
        import importlib

        module = importlib.import_module("dcc_mcp_core.server_base")

        assert "create_skill_server" in module.__dict__
        assert "create_adapter_server" in module.__dict__
        assert module._PKG_VERSION == "0.0.0-dev"


@pytest.mark.skipif(is_core_extension_available(), reason="only meaningful on py37-lite profile")
def test_resolve_mcp_http_config_class_uses_pure_python():
    cls = resolve_mcp_http_config_class()
    assert cls is PureMcpHttpConfig
