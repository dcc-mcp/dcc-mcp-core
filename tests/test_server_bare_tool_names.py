"""Source-only option/config tests; no native extension, server, or network."""

from __future__ import annotations

# Import standard library modules.
from dataclasses import FrozenInstanceError
from dataclasses import fields
import importlib.util
from pathlib import Path
import sys
from types import ModuleType
from types import SimpleNamespace

# Import third-party modules.
import pytest


@pytest.fixture
def source_contract(monkeypatch):
    """Load production Python modules against one controlled config boundary."""
    package = Path(__file__).resolve().parents[1] / "python" / "dcc_mcp_core"

    def module(name, path=None):
        value = ModuleType(name)
        if path is not None:
            value.__path__ = [str(path)]
        monkeypatch.setitem(sys.modules, name, value)
        return value

    def load(name, path):
        spec = importlib.util.spec_from_file_location(name, path)
        value = importlib.util.module_from_spec(spec)
        monkeypatch.setitem(sys.modules, name, value)
        spec.loader.exec_module(value)
        return value

    class Config:
        """Controlled equivalent of the existing public config property."""

        def __init__(self, **kwargs):
            self.__dict__.update(kwargs)
            self._bare_tool_names = True
            self.assignments = []
            self.exclude_skill_stubs_from_tools_list = False
            self.exclude_group_stubs_from_tools_list = False

        @property
        def bare_tool_names(self):
            return self._bare_tool_names

        @bare_tool_names.setter
        def bare_tool_names(self, value):
            assert isinstance(value, bool)
            self.assignments.append(value)
            self._bare_tool_names = value

    module("dcc_mcp_core", package)
    module("dcc_mcp_core._server", package / "_server")
    module("dcc_mcp_core._runtime")
    core = module("dcc_mcp_core._core")
    core.McpHttpConfig = Config
    bridge = module("dcc_mcp_core._runtime.config_bridge")
    bridge.resolve_mcp_http_config_class = lambda: Config
    for name in ("constants", "env"):
        load("dcc_mcp_core." + name, package / (name + ".py"))
    options = load("dcc_mcp_core._server.options", package / "_server" / "options.py")
    load("dcc_mcp_core._server.tools_list_policy", package / "_server" / "tools_list_policy.py")
    config = load("dcc_mcp_core._server.config", package / "_server" / "config.py")
    for name in (
        "DCC_MCP_MAYA_PORT",
        "DCC_MCP_UNITY_PORT",
        "DCC_MCP_MAYA_INSTANCE_TYPE",
        "DCC_MCP_UNITY_INSTANCE_TYPE",
        "DCC_MCP_INSTANCE_TYPE",
        "DCC_MCP_GATEWAY_PORT",
        "DCC_MCP_REGISTRY_DIR",
        "DCC_MCP_STRICT_GATEWAY",
        "DCC_MCP_EXCLUDE_STUBS_FROM_TOOLS_LIST",
        "DCC_MCP_MAYA_EXCLUDE_STUBS_FROM_TOOLS_LIST",
        "DCC_MCP_UNITY_EXCLUDE_STUBS_FROM_TOOLS_LIST",
    ):
        monkeypatch.delenv(name, raising=False)
    return SimpleNamespace(options=options, build=config.build_mcp_http_config, config_type=Config)


@pytest.mark.parametrize("dcc_name", ["maya", "unity"])
@pytest.mark.parametrize("constructor", ["direct", "from_env"])
def test_default_preserves_bare_tool_names(source_contract, tmp_path, dcc_name, constructor):
    options_type = source_contract.options.DccServerOptions
    factory = options_type if constructor == "direct" else options_type.from_env
    options = factory(dcc_name, tmp_path)
    config = source_contract.build(options, package_version="test", version_provider=lambda: "host")

    assert options.bare_tool_names is True
    assert config.bare_tool_names is True
    assert config.assignments == [True]
    assert config.dcc_type == dcc_name


@pytest.mark.parametrize("dcc_name", ["maya", "unity"])
@pytest.mark.parametrize("constructor", ["direct", "from_env"])
def test_qualified_names_reach_config(source_contract, tmp_path, dcc_name, constructor):
    options_type = source_contract.options.DccServerOptions
    factory = options_type if constructor == "direct" else options_type.from_env
    options = factory(dcc_name, tmp_path, bare_tool_names=False)
    config = source_contract.build(options, package_version="test", version_provider=lambda: "host")

    assert options.bare_tool_names is False
    assert config.bare_tool_names is False
    assert config.assignments == [False]
    assert config.dcc_type == dcc_name
    assert config.server_name == dcc_name + "-mcp"


@pytest.mark.parametrize("value", [None, 0, 1, "", "false", "true", 0.0, [], {}])
@pytest.mark.parametrize("constructor", ["direct", "from_env"])
def test_non_boolean_policy_rejected(source_contract, tmp_path, value, constructor):
    options_type = source_contract.options.DccServerOptions
    factory = options_type if constructor == "direct" else options_type.from_env

    with pytest.raises(TypeError, match="bare_tool_names must be a bool"):
        factory("maya", tmp_path, bare_tool_names=value)


def test_historical_positional_order_and_frozen_policy(source_contract, tmp_path):
    api = source_contract.options
    gateway = api.GatewayOptions(port=0)
    observability = api.ObservabilityOptions()
    diagnostics = api.DiagnosticsOptions(dcc_pid=123)
    execution = api.ExecutionOptions()
    sidecar = api.SidecarOptions()
    options = api.DccServerOptions(
        "unity",
        tmp_path,
        0,
        "custom-mcp",
        "test",
        gateway,
        observability,
        diagnostics,
        execution,
        sidecar,
        "standalone",
    )

    assert options.gateway is gateway
    assert options.observability is observability
    assert options.diagnostics is diagnostics
    assert options.execution is execution
    assert options.sidecar is sidecar
    assert options.instance_type == "standalone"
    assert options.bare_tool_names is True
    field_names = [item.name for item in fields(options)]
    assert field_names.index("instance_type") < field_names.index("bare_tool_names")
    with pytest.raises(FrozenInstanceError):
        options.bare_tool_names = False


def test_config_construction_uses_only_controlled_boundary(source_contract, tmp_path):
    options = source_contract.options.DccServerOptions("unity", tmp_path, bare_tool_names=False)
    config = source_contract.build(options, package_version="test", version_provider=lambda: "host")

    assert isinstance(config, source_contract.config_type)
    assert not hasattr(sys.modules["dcc_mcp_core._core"], "__file__")
