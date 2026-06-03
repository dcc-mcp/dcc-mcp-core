"""Tests for DccServerOptions and sub-option dataclasses (issue #850).

Verifies:
- All env-var resolution is centralised in DccServerOptions.from_env.
- dispatcher / execution_bridge mutual exclusion is enforced at build time.
- DccServerBase requires a DccServerOptions instance.
"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any
from unittest.mock import MagicMock
from unittest.mock import patch
import warnings

import pytest

from dcc_mcp_core._server.config import build_mcp_http_config
from dcc_mcp_core._server.config import collect_context_metadata_from_env
from dcc_mcp_core._server.config import resolve_diagnostics_state
from dcc_mcp_core._server.config import resolve_execution_binding
from dcc_mcp_core._server.config import resolve_observability_flags
from dcc_mcp_core._server.options import BridgeExecution
from dcc_mcp_core._server.options import DccServerOptions
from dcc_mcp_core._server.options import DiagnosticsOptions
from dcc_mcp_core._server.options import DispatcherExecution
from dcc_mcp_core._server.options import ExecutionOptions
from dcc_mcp_core._server.options import GatewayOptions
from dcc_mcp_core._server.options import InlineExecution
from dcc_mcp_core._server.options import ObservabilityOptions
from dcc_mcp_core._server.options import StandaloneMainThreadExecution

# ── GatewayOptions ────────────────────────────────────────────────────────────


class TestGatewayOptions:
    def test_defaults(self):
        gw = GatewayOptions()
        assert gw.port is None
        assert gw.registry_dir is None
        assert gw.enable_failover is True

    def test_from_env_reads_gateway_port(self, monkeypatch):
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9999")
        gw = GatewayOptions.from_env()
        assert gw.port == 9999

    def test_from_env_invalid_port_preserves_none(self, monkeypatch):
        """Invalid DCC_MCP_GATEWAY_PORT keeps port=None so Rust default is used."""
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "not_a_number")
        gw = GatewayOptions.from_env()
        assert gw.port is None

    def test_from_env_reads_registry_dir(self, monkeypatch):
        monkeypatch.setenv("DCC_MCP_REGISTRY_DIR", "/some/registry")
        gw = GatewayOptions.from_env()
        assert gw.registry_dir == "/some/registry"

    def test_from_env_explicit_port_overrides_env(self, monkeypatch):
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9999")
        gw = GatewayOptions.from_env(port=1234)
        assert gw.port == 1234

    def test_from_env_no_env_port_keeps_none(self, monkeypatch):
        """When DCC_MCP_GATEWAY_PORT is not set, port stays None (use Rust default 9765)."""
        monkeypatch.delenv("DCC_MCP_GATEWAY_PORT", raising=False)
        gw = GatewayOptions.from_env()
        assert gw.port is None

    def test_from_env_explicit_zero_port_disables_gateway(self, monkeypatch):
        """Explicit port=0 should disable gateway through build_mcp_http_config."""
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9999")
        gw = GatewayOptions.from_env(port=0)
        assert gw.port == 0  # explicit 0 wins over env

    def test_from_env_direct_zero_port_disables_gateway(self, monkeypatch):
        """GatewayOptions(port=0) means gateway is explicitly disabled."""
        monkeypatch.delenv("DCC_MCP_GATEWAY_PORT", raising=False)
        gw = GatewayOptions.from_env(port=0)
        assert gw.port == 0

    def test_frozen(self):
        gw = GatewayOptions()
        with pytest.raises((AttributeError, TypeError)):
            gw.port = 1  # type: ignore[misc]


# ── ObservabilityOptions ──────────────────────────────────────────────────────


class TestObservabilityOptions:
    def test_defaults_all_true(self):
        obs = ObservabilityOptions()
        assert obs.enable_file_logging is True
        assert obs.enable_job_persistence is True
        assert obs.enable_telemetry is True

    def test_can_disable_all(self):
        obs = ObservabilityOptions(
            enable_file_logging=False,
            enable_job_persistence=False,
            enable_telemetry=False,
        )
        assert obs.enable_file_logging is False

    def test_frozen(self):
        obs = ObservabilityOptions()
        with pytest.raises((AttributeError, TypeError)):
            obs.enable_file_logging = False  # type: ignore[misc]


class TestResolvedServerConfig:
    def test_observability_flags_honor_runtime_env(self, monkeypatch):
        monkeypatch.setenv("DCC_MCP_DISABLE_FILE_LOGGING", "1")
        monkeypatch.delenv("DCC_MCP_DISABLE_JOB_PERSISTENCE", raising=False)
        monkeypatch.delenv("DCC_MCP_DISABLE_TELEMETRY", raising=False)

        flags = resolve_observability_flags(ObservabilityOptions())

        assert flags.file_logging is False
        assert flags.job_persistence is True
        assert flags.telemetry is True

    def test_diagnostics_state_defaults_pid(self, monkeypatch):
        monkeypatch.setattr(os, "getpid", lambda: 4321)

        state = resolve_diagnostics_state(DiagnosticsOptions(window_title="Houdini"))

        assert state.dcc_pid == 4321
        assert state.window_title == "Houdini"
        assert state.window_handle is None

    def test_execution_binding_resolves_dispatcher(self):
        dispatcher = MagicMock()

        binding = resolve_execution_binding(DispatcherExecution(dispatcher))

        assert binding.bridge is None
        assert binding.dispatcher is dispatcher
        assert binding.standalone_main_thread is False
        assert binding.register_inprocess_executor is True

    def test_execution_binding_resolves_bridge_dispatcher(self):
        dispatcher = MagicMock()
        bridge = MagicMock(dispatcher=dispatcher)

        binding = resolve_execution_binding(BridgeExecution(bridge))

        assert binding.bridge is bridge
        assert binding.dispatcher is dispatcher
        assert binding.standalone_main_thread is False
        assert binding.register_inprocess_executor is True

    def test_execution_binding_resolves_standalone_main_thread(self):
        binding = resolve_execution_binding(StandaloneMainThreadExecution)

        assert binding.bridge is None
        assert binding.dispatcher is None
        assert binding.standalone_main_thread is True
        assert binding.register_inprocess_executor is True

    def test_context_metadata_from_env_includes_dcc_specific_paths(self, monkeypatch):
        monkeypatch.setenv("DCC_MCP_PROJECT", "show-a")
        monkeypatch.setenv("DCC_MCP_HOUDINI_SKILL_PATHS", "C:/show/skills")

        metadata = collect_context_metadata_from_env("houdini")

        assert metadata["project"] == "show-a"
        assert metadata["dcc_skill_paths"] == "C:/show/skills"

    def test_build_mcp_http_config_populates_gateway_contract(self, tmp_path):
        opts = DccServerOptions.from_env(
            "photoshop",
            tmp_path,
            port=0,
            gateway_port=19765,
            registry_dir="C:/registry",
            dcc_version="25.0",
            scene="C:/scene.psd",
        )

        config = build_mcp_http_config(
            opts,
            package_version="9.9.9",
            version_provider=lambda: "unused",
        )

        assert config.port == 0
        assert config.server_name == "photoshop-mcp"
        assert config.server_version == "9.9.9"
        assert config.gateway_port == 19765
        assert config.registry_dir == "C:/registry"
        assert config.dcc_version == "25.0"
        assert config.scene == "C:/scene.psd"
        assert config.dcc_type == "photoshop"
        assert config.standalone_main_thread_execution is False

    def test_build_mcp_http_config_propagates_standalone_main_thread(self, tmp_path):
        opts = DccServerOptions.from_env(
            "maya",
            tmp_path,
            port=0,
            standalone_main_thread=True,
        )

        config = build_mcp_http_config(
            opts,
            package_version="9.9.9",
            version_provider=lambda: "unused",
        )

        assert config.standalone_main_thread_execution is True


# ── DiagnosticsOptions ────────────────────────────────────────────────────────


class TestDiagnosticsOptions:
    def test_defaults(self):
        d = DiagnosticsOptions()
        assert d.dcc_pid is None
        assert d.window_title is None
        assert d.window_handle is None
        assert d.snapshot_provider is None

    def test_explicit_values(self):
        d = DiagnosticsOptions(dcc_pid=1234, window_title="Maya", window_handle=99)
        assert d.dcc_pid == 1234
        assert d.window_title == "Maya"
        assert d.window_handle == 99


# ── ExecutionOptions / tagged union ──────────────────────────────────────────


class TestExecutionMode:
    def test_inline_is_default(self):
        exec_opts = ExecutionOptions()
        assert exec_opts.mode is InlineExecution
        assert InlineExecution.kind == "inline"
        assert StandaloneMainThreadExecution.kind == "standalone-main-thread"

    def test_dispatcher_execution(self):
        dispatcher = MagicMock()
        mode = DispatcherExecution(dispatcher)
        assert mode.kind == "dispatcher"
        assert mode.dispatcher is dispatcher

    def test_bridge_execution(self):
        bridge = MagicMock()
        mode = BridgeExecution(bridge)
        assert mode.kind == "bridge"
        assert mode.bridge is bridge

    def test_frozen_inline(self):
        with pytest.raises((AttributeError, TypeError)):
            InlineExecution.kind = "other"  # type: ignore[misc]


# ── DccServerOptions ──────────────────────────────────────────────────────────


class TestDccServerOptions:
    def test_minimal_construction(self, tmp_path):
        opts = DccServerOptions(dcc_name="test", builtin_skills_dir=tmp_path)
        assert opts.dcc_name == "test"
        assert opts.port == 8765
        assert opts.gateway.enable_failover is True

    def test_frozen(self, tmp_path):
        opts = DccServerOptions(dcc_name="test", builtin_skills_dir=tmp_path)
        with pytest.raises((AttributeError, TypeError)):
            opts.port = 9999  # type: ignore[misc]

    def test_from_env_minimal(self, tmp_path):
        opts = DccServerOptions.from_env("blender", tmp_path)
        assert opts.dcc_name == "blender"
        assert opts.execution.mode is InlineExecution

    def test_from_env_dispatcher(self, tmp_path):
        dispatcher = MagicMock()
        opts = DccServerOptions.from_env("maya", tmp_path, dispatcher=dispatcher)
        assert opts.execution.mode.kind == "dispatcher"
        assert opts.execution.mode.dispatcher is dispatcher

    def test_from_env_bridge(self, tmp_path):
        bridge = MagicMock()
        opts = DccServerOptions.from_env("maya", tmp_path, execution_bridge=bridge)
        assert opts.execution.mode.kind == "bridge"
        assert opts.execution.mode.bridge is bridge

    def test_from_env_standalone_main_thread(self, tmp_path):
        opts = DccServerOptions.from_env("maya", tmp_path, standalone_main_thread=True)
        assert opts.execution.mode is StandaloneMainThreadExecution

    def test_from_env_mutex_raises(self, tmp_path):
        """Passing both dispatcher and execution_bridge must raise ValueError at build time."""
        with pytest.raises(ValueError, match=r"dispatcher.*execution_bridge|execution_bridge.*dispatcher"):
            DccServerOptions.from_env(
                "maya",
                tmp_path,
                dispatcher=MagicMock(),
                execution_bridge=MagicMock(),
            )

    def test_from_env_standalone_mutex_raises(self, tmp_path):
        with pytest.raises(ValueError, match="standalone_main_thread"):
            DccServerOptions.from_env(
                "maya",
                tmp_path,
                dispatcher=MagicMock(),
                standalone_main_thread=True,
            )

    def test_from_env_gateway_port_from_kwarg(self, tmp_path, monkeypatch):
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9999")
        opts = DccServerOptions.from_env("test", tmp_path, gateway_port=1234)
        assert opts.gateway.port == 1234  # explicit kwarg wins

    def test_from_env_env_var_resolution(self, tmp_path, monkeypatch):
        monkeypatch.setenv("DCC_MCP_GATEWAY_PORT", "9999")
        monkeypatch.setenv("DCC_MCP_REGISTRY_DIR", "/reg")
        opts = DccServerOptions.from_env("test", tmp_path)
        assert opts.gateway.port == 9999
        assert opts.gateway.registry_dir == "/reg"

    def test_from_env_dcc_pid_kwarg(self, tmp_path):
        opts = DccServerOptions.from_env("test", tmp_path, dcc_pid=42)
        assert opts.diagnostics.dcc_pid == 42

    def test_from_env_observability_flags(self, tmp_path):
        opts = DccServerOptions.from_env(
            "test",
            tmp_path,
            enable_file_logging=False,
            enable_job_persistence=False,
            enable_telemetry=False,
        )
        assert opts.observability.enable_file_logging is False
        assert opts.observability.enable_job_persistence is False
        assert opts.observability.enable_telemetry is False


# ── DccServerBase — new options path ─────────────────────────────────────────


class _FakeHandle:
    port = 18765
    is_gateway = False

    def mcp_url(self):
        return f"http://127.0.0.1:{self.port}/mcp"

    def shutdown(self):
        pass


class _FakeConfig:
    port = 18765
    server_name = "fake-mcp"
    server_version = "0.1.0"
    gateway_port = 0
    registry_dir = ""
    dcc_version = ""
    scene = ""
    dcc_type = ""
    instance_metadata: dict = None  # type: ignore[assignment]  # mutable default avoided; set in __init__

    def __init__(self):
        self.instance_metadata = {}

    def __setattr__(self, name, value):
        object.__setattr__(self, name, value)


class _FakeDccServer:
    def start(self):
        return _FakeHandle()

    def list_skills(self):
        return []

    def load_skill(self, name):
        return True

    def unload_skill(self, name):
        return True

    def search_skills(self, **kwargs):
        return []

    def is_loaded(self, name):
        return False

    def get_skill_info(self, name):
        return None

    def discover(self, **kwargs):
        return 0


def _make_fake_dcc_mcp(config):
    """Return a SimpleNamespace that fakes the dcc_mcp_core deferred imports."""
    from types import SimpleNamespace

    return SimpleNamespace(
        McpHttpConfig=lambda **_kw: config,
        create_skill_server=lambda *_a, **_kw: _FakeDccServer(),
        __version__="9.9.9",
    )


class TestDccServerBaseOptionsPath:
    """Verify DccServerBase(options) does not emit DeprecationWarning."""

    def _make_server_via_options(self, tmp_path):
        import builtins

        from dcc_mcp_core.server_base import DccServerBase

        skills_dir = tmp_path / "skills"
        skills_dir.mkdir(exist_ok=True)
        fake_cfg = _FakeConfig()
        real_import = __import__

        def fake_import(name, globals=None, locals=None, fromlist=(), level=0):
            if name == "dcc_mcp_core" and fromlist:
                return _make_fake_dcc_mcp(fake_cfg)
            return real_import(name, globals, locals, fromlist, level)

        opts = DccServerOptions.from_env("houdini", skills_dir)
        with patch.object(builtins, "__import__", side_effect=fake_import):
            with warnings.catch_warnings(record=True) as w:
                warnings.simplefilter("always")
                server = DccServerBase(opts)
                dep_warnings = [x for x in w if issubclass(x.category, DeprecationWarning)]
        return server, dep_warnings

    def test_no_deprecation_warning_with_options(self, tmp_path):
        _, dep_warnings = self._make_server_via_options(tmp_path)
        assert dep_warnings == [], f"Unexpected DeprecationWarning: {dep_warnings}"

    def test_options_path_sets_dcc_name(self, tmp_path):
        server, _ = self._make_server_via_options(tmp_path)
        assert server._dcc_name == "houdini"

    def test_options_stored_on_instance(self, tmp_path):
        server, _ = self._make_server_via_options(tmp_path)
        assert isinstance(server._options, DccServerOptions)


def test_dcc_server_base_requires_options_argument() -> None:
    from dcc_mcp_core.server_base import DccServerBase

    with pytest.raises(TypeError, match="options"):
        DccServerBase()


class TestDccServerBasePublicImport:
    """DccServerOptions and sub-options are importable from top-level dcc_mcp_core."""

    def test_options_importable(self):
        from dcc_mcp_core._server.options import DccServerOptions as O1

        assert O1 is not None

    def test_gateway_options_importable(self):
        from dcc_mcp_core._server.options import GatewayOptions as G

        assert G is not None

    def test_observability_options_importable(self):
        from dcc_mcp_core._server.options import ObservabilityOptions as O

        assert O is not None

    def test_diagnostics_options_importable(self):
        from dcc_mcp_core._server.options import DiagnosticsOptions as D

        assert D is not None

    def test_execution_options_importable(self):
        from dcc_mcp_core._server.options import ExecutionOptions as E

        assert E is not None

    def test_execution_constructors_importable(self):
        from dcc_mcp_core._server.options import BridgeExecution
        from dcc_mcp_core._server.options import DispatcherExecution
        from dcc_mcp_core._server.options import InlineExecution
        from dcc_mcp_core._server.options import StandaloneMainThreadExecution

        assert InlineExecution is not None
        assert DispatcherExecution is not None
        assert BridgeExecution is not None
        assert StandaloneMainThreadExecution is not None
