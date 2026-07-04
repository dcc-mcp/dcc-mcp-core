"""Factory for adapter-local MCP servers (embedded _core or sidecar binary)."""

from __future__ import annotations

import os
from typing import Any

from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.sidecar_skill_server import SidecarBackedSkillServer


def create_adapter_server(
    dcc_name: str,
    config: Any,
    options: Any | None = None,
) -> Any:
    """Create the inner server object used by :class:`DccServerBase`."""
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


def _resolve_host_rpc(sidecar: Any) -> str:
    if sidecar is not None:
        value = getattr(sidecar, "host_rpc", None)
        if isinstance(value, str) and value.strip():
            return value.strip()
    return str(os.environ.get("DCC_MCP_HOST_RPC", "")).strip()


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
