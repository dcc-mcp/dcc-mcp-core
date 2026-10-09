"""Public embedded gateway overrides reach the real native configuration."""

from dataclasses import FrozenInstanceError

import pytest

from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class
from dcc_mcp_core._server.config import build_mcp_http_config
from dcc_mcp_core.constants import ENV_GATEWAY_REMOTE_HOST
from dcc_mcp_core.constants import ENV_GATEWAY_REMOTE_PORT
from dcc_mcp_core.server import DccServerOptions
from dcc_mcp_core.server import GatewayOptions


@pytest.fixture(autouse=True)
def clean_remote_env(monkeypatch):
    monkeypatch.delenv("DCC_MCP_GATEWAY_REMOTE_HOST", raising=False)
    monkeypatch.delenv("DCC_MCP_GATEWAY_REMOTE_PORT", raising=False)


def build(options):
    return build_mcp_http_config(options, package_version="test", version_provider=lambda: "test")


def test_omitted_options_preserve_native_constructor_defaults(tmp_path):
    expected = resolve_mcp_http_config_class()(port=0)
    actual = build(DccServerOptions.from_env("blender", tmp_path))
    assert actual.gateway_remote_host == expected.gateway_remote_host
    assert actual.gateway_remote_port == expected.gateway_remote_port
    assert GatewayOptions().remote_port is None


@pytest.mark.parametrize("remote_port", [0, 59765, 65535])
def test_public_options_reach_config_without_touching_main_port(tmp_path, monkeypatch, remote_port):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", "12345")
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_HOST", "0.0.0.0")
    options = DccServerOptions.from_env(
        "blender",
        tmp_path,
        gateway_port=19765,
        gateway_remote_port=remote_port,
        gateway_remote_host="127.0.0.1",
        enable_gateway_failover=False,
    )
    config = build(options)
    assert config.gateway_port == 19765
    assert config.gateway_remote_port == remote_port
    assert config.gateway_remote_host == "127.0.0.1"
    assert options.gateway.enable_failover is False


def test_existing_remote_env_reaches_embedded_config(tmp_path, monkeypatch):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", "0")
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_HOST", "127.0.0.1")
    actual = build(DccServerOptions.from_env("blender", tmp_path))
    assert actual.gateway_remote_port == 0
    assert actual.gateway_remote_host == "127.0.0.1"


def test_direct_options_do_not_read_environment(tmp_path, monkeypatch):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", "invalid")
    options = GatewayOptions(remote_port=0)
    assert build(DccServerOptions("blender", tmp_path, gateway=options)).gateway_remote_port == 0
    with pytest.raises(FrozenInstanceError):
        options.remote_port = 1


@pytest.mark.parametrize("value", [True, False, 1.5, "0", -1, 65536])
def test_remote_port_rejects_invalid_explicit_values(value):
    with pytest.raises(ValueError, match="remote_port"):
        GatewayOptions(remote_port=value)


@pytest.mark.parametrize("value", ["", "invalid", "1.5", "-1", "65536"])
def test_invalid_env_fails_before_native_construction(monkeypatch, value):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_PORT", value)
    with pytest.raises(ValueError, match=ENV_GATEWAY_REMOTE_PORT):
        GatewayOptions.from_env()
    assert GatewayOptions.from_env(remote_port=0).remote_port == 0


@pytest.mark.parametrize("value", ["", "   "])
def test_invalid_env_host_fails_before_native_construction(monkeypatch, value):
    monkeypatch.setenv("DCC_MCP_GATEWAY_REMOTE_HOST", value)
    with pytest.raises(ValueError, match=ENV_GATEWAY_REMOTE_HOST):
        GatewayOptions.from_env()
    assert GatewayOptions.from_env(remote_host="127.0.0.1").remote_host == "127.0.0.1"


@pytest.mark.parametrize("value", [True, 1, "", "  "])
def test_remote_host_rejects_invalid_explicit_values(value):
    with pytest.raises(ValueError, match="remote_host"):
        GatewayOptions(remote_host=value)


def test_remote_options_append_without_changing_positional_contract():
    options = GatewayOptions(19765, None, None, None, False, True)
    assert options.strict_gateway is True and options.remote_port is None
