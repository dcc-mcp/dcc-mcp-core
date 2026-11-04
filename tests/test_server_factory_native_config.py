"""Exercise both factories across the Python-to-PyO3 config boundary."""

from __future__ import annotations

from dataclasses import fields
import sys
from types import ModuleType
from types import SimpleNamespace
from unittest.mock import MagicMock

import pytest

from dcc_mcp_core._runtime import server_factory
from dcc_mcp_core._runtime.config_bridge import to_native_mcp_http_config
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer

_FACTORIES = (server_factory.create_adapter_server, server_factory.create_skill_server)


@pytest.fixture
def native_core():
    """Require the real binding, rather than a permissive Python factory stub."""
    return pytest.importorskip("dcc_mcp_core._core", reason="compiled core is required for native conversion tests")


@pytest.mark.parametrize("factory", _FACTORIES)
@pytest.mark.parametrize("kind", ["pure", "duck", "native", "none"])
def test_factories_forward_native_config_and_public_arguments(factory, kind, monkeypatch):
    class NativeConfig(PureMcpHttpConfig):
        """Require conversion even when the compiled extension is absent."""

    native_module = ModuleType("dcc_mcp_core._core")
    native_module.McpHttpConfig = NativeConfig
    backend = MagicMock()
    native_module.create_skill_server = backend
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", native_module)
    monkeypatch.setattr(server_factory, "_core", native_module)
    monkeypatch.setattr(server_factory, "is_core_extension_available", lambda: True)
    if kind == "none":
        config = None
    elif kind == "duck":
        config = SimpleNamespace(port=3456, server_name="config-regression", endpoint_path="/mcp")
    else:
        config_class = NativeConfig if kind == "native" else PureMcpHttpConfig
        config = config_class(port=3456, server_name="config-regression")

    if factory is server_factory.create_skill_server:
        result = factory("app-server", config, extra_paths=["/skills"], dcc_name="houdini", accumulated=False)
        forwarded = backend.call_args.kwargs["config"]
        assert backend.call_args.args == ("app-server",)
        assert backend.call_args.kwargs == {
            "config": forwarded,
            "extra_paths": ["/skills"],
            "dcc_name": "houdini",
            "accumulated": False,
        }
    else:
        result = factory("houdini", config)
        assert backend.call_args.args[0] == "houdini"
        forwarded = backend.call_args.args[1]

    assert result is backend.return_value
    backend.assert_called_once()
    if kind in ("native", "none"):
        assert forwarded is config
    else:
        assert isinstance(forwarded, NativeConfig)
        assert forwarded is not config
        assert forwarded.port == config.port
        assert forwarded.server_name == config.server_name


@pytest.mark.parametrize("factory", _FACTORIES)
@pytest.mark.parametrize("kind", ["pure", "duck", "native", "none"])
def test_factories_accept_configs_at_the_real_native_boundary(factory, kind, native_core):
    if kind == "pure":
        config = PureMcpHttpConfig(port=0, server_name="config-regression", gateway_port=0)
    elif kind == "duck":
        config = SimpleNamespace(port=0, server_name="config-regression", endpoint_path="/mcp")
    elif kind == "native":
        config = native_core.McpHttpConfig(port=0, server_name="config-regression")
    else:
        config = None

    server = factory("blender", config)

    assert isinstance(server, native_core.McpHttpServer)


def test_native_conversion_preserves_all_pure_fields_and_optional_native_settings(native_core, tmp_path):
    policy = object()
    config = PureMcpHttpConfig(
        port=8765,
        server_name="houdini-config-regression",
        server_version="1.2.3",
        host="127.0.0.2",
        enable_cors=True,
        request_timeout_ms=45678,
        session_ttl_secs=90,
        enable_tool_cache=True,
        enable_prometheus=True,
        gateway_port=0,
        registry_dir=str(tmp_path / "registry"),
        dcc_version="21.0",
        scene="regression.hip",
        dcc_type="houdini",
        host_pid=1234,
        instance_metadata={"project": "regression"},
        standalone_main_thread_execution=True,
        exclude_skill_stubs_from_tools_list=True,
        exclude_group_stubs_from_tools_list=True,
        job_storage_path=str(tmp_path / "jobs.sqlite3"),
        job_retention_hours=48,
        sandbox_policy=policy,
        # The Python config also accepts native settings through **kwargs.
        backend_timeout_ms=123456,
        gateway_read_only=True,
        allowed_tool_slugs=["regression__read"],
        instance_extras={"ready": True, "count": 3},
        spawn_mode="dedicated",
    )
    expected = vars(config).copy()

    native = to_native_mcp_http_config(config, owner="test")

    assert isinstance(native, native_core.McpHttpConfig)
    assert native is not config
    assert {field.name for field in fields(PureMcpHttpConfig)} <= set(expected)
    for name, value in expected.items():
        assert getattr(native, name) == value, name
    assert native.sandbox_policy is policy
    assert vars(config) == expected


def test_native_config_and_none_pass_through_unchanged(native_core):
    config = native_core.McpHttpConfig(port=1234)
    assert to_native_mcp_http_config(config, owner="test") is config
    assert to_native_mcp_http_config(None, owner="test") is None


def test_minimal_duck_config_keeps_unspecified_native_defaults(native_core):
    config = SimpleNamespace(port=1234, server_name="config-regression", endpoint_path="/mcp")
    defaults = native_core.McpHttpConfig(port=config.port, server_name=config.server_name)

    native = to_native_mcp_http_config(config, owner="test")

    for name in ("port", "server_name", "server_version", "host", "request_timeout_ms", "gateway_port"):
        assert getattr(native, name) == getattr(defaults, name), name


@pytest.mark.parametrize("factory", _FACTORIES)
@pytest.mark.parametrize("name,value", [("endpoint_path", "/custom-mcp"), ("max_sessions", 42)])
def test_nondefault_readonly_settings_fail_before_native_factory(factory, name, value, native_core, monkeypatch):
    backend = MagicMock()
    monkeypatch.setattr(native_core, "create_skill_server", backend)
    config = PureMcpHttpConfig(**{name: value})

    with pytest.raises(TypeError, match=rf"{factory.__name__}.*config\.{name}.*read-only"):
        factory("blender", config)

    backend.assert_not_called()
    assert getattr(config, name) == value


@pytest.mark.parametrize("factory", _FACTORIES)
def test_unknown_setting_is_not_silently_dropped(factory, native_core, monkeypatch):
    backend = MagicMock()
    monkeypatch.setattr(native_core, "create_skill_server", backend)

    with pytest.raises(TypeError, match=r"config\.unsupported_setting.*not supported"):
        factory("houdini", PureMcpHttpConfig(unsupported_setting=True))

    backend.assert_not_called()


@pytest.mark.parametrize(
    "name,value",
    [("port", -1), ("request_timeout_ms", -1), ("gateway_port", 70000), ("host", "invalid-host")],
)
def test_invalid_native_values_have_factory_context(name, value, native_core):
    with pytest.raises(TypeError, match=r"create_adapter_server.*config.*invalid"):
        server_factory.create_adapter_server("blender", PureMcpHttpConfig(**{name: value}))


@pytest.mark.parametrize("factory", _FACTORIES)
def test_sidecar_preserves_duck_config_without_native_conversion(factory, monkeypatch):
    monkeypatch.setattr(server_factory, "is_core_extension_available", lambda: False)
    monkeypatch.setattr(server_factory, "to_native_mcp_http_config", MagicMock(side_effect=AssertionError))
    monkeypatch.setattr(SidecarBackedSkillServer, "discover", MagicMock())
    config = SimpleNamespace(port=0, server_name="sidecar-config", endpoint_path="/custom-mcp")

    server = factory("houdini", config)

    assert isinstance(server, SidecarBackedSkillServer)
    assert server._config is config


@pytest.mark.parametrize("has_none_export", [False, True])
def test_missing_native_config_export_is_actionable(monkeypatch, has_none_export):
    partial_core = ModuleType("dcc_mcp_core._core")
    if has_none_export:
        partial_core.McpHttpConfig = None
    monkeypatch.setitem(sys.modules, "dcc_mcp_core._core", partial_core)

    with pytest.raises(RuntimeError, match="embedded backend is missing McpHttpConfig; reinstall"):
        to_native_mcp_http_config(PureMcpHttpConfig(), owner="test")
