"""Factory for adapter-local MCP servers (embedded _core or sidecar binary)."""

from __future__ import annotations

import os
from typing import Any

from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class
from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer
from dcc_mcp_core.constants import ENV_HOST_RPC


def create_adapter_server(
    dcc_name: str,
    config: McpHttpConfig | None = None,
    options: Any | None = None,
) -> Any:
    """Create the inner server object used by :class:`DccServerBase`.

    Args:
        dcc_name: Name of the DCC the server is created for.
        config: HTTP server configuration. ``None`` selects the backend
            default configuration.
        options: Optional :class:`~dcc_mcp_core._server.options.DccServerOptions`
            used to resolve the sidecar binding on the py37-lite profile.

    Raises:
        TypeError: If ``config`` is neither ``None`` nor the active
            ``McpHttpConfig`` type for this wheel.

    """
    config = _validate_config(config)
    if is_core_extension_available():
        from dcc_mcp_core._core import create_skill_server

        return create_skill_server(dcc_name, config)

    sidecar = getattr(options, "sidecar", None) if options is not None else None
    host_rpc = _resolve_host_rpc(sidecar)
    return SidecarBackedSkillServer(
        dcc_name,
        config,
        host_rpc=host_rpc,
        watch_pid=_resolve_watch_pid(options),
        adapter_version=getattr(sidecar, "adapter_version", None) if sidecar is not None else None,
        display_name=getattr(sidecar, "display_name", None) if sidecar is not None else None,
        wait_ready_timeout_secs=_resolve_wait_ready(sidecar),
        server_bin=getattr(sidecar, "server_bin", None) if sidecar is not None else None,
        extra_args=_resolve_extra_args(sidecar),
    )


def _validate_config(config: McpHttpConfig | None) -> McpHttpConfig | None:
    """Reject non-config values with an actionable message.

    The Rust-backed factory surfaces a bare ``TypeError`` from PyO3 when a
    caller passes a plain mapping, which does not explain how to build the
    expected object. Validate up front so the failure names both the accepted
    type and the constructor.
    """
    if config is None:
        return None
    active_config_cls = resolve_mcp_http_config_class()
    if isinstance(config, active_config_cls):
        return config
    raise TypeError(
        "create_adapter_server: 'config' must be an McpHttpConfig instance or None, "
        f"got {type(config).__name__!r}. Build one with "
        "dcc_mcp_core.McpHttpConfig(port=..., server_name=...) or pass None to use defaults."
    )


def _resolve_host_rpc(sidecar: Any) -> str:
    if sidecar is not None:
        value = getattr(sidecar, "host_rpc", None)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return str(os.environ.get(ENV_HOST_RPC, "")).strip()


def _resolve_watch_pid(options: Any | None) -> int | None:
    if options is None:
        return None
    diagnostics = getattr(options, "diagnostics", None)
    if diagnostics is not None and getattr(diagnostics, "dcc_pid", None) is not None:
        return int(diagnostics.dcc_pid)
    return None


def _resolve_wait_ready(sidecar: Any) -> float:
    if sidecar is None:
        return 15.0
    value = getattr(sidecar, "wait_ready_timeout_secs", None)
    if value is None:
        return 15.0
    return float(value)


def _resolve_extra_args(sidecar: Any) -> tuple:
    if sidecar is None:
        return ()
    value = getattr(sidecar, "extra_args", None)
    if not value:
        return ()
    return tuple(str(arg) for arg in value)
