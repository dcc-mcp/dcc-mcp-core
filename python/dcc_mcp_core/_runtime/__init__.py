"""Runtime helpers for py37-lite and sidecar-backed HTTP/MCP serving."""

from __future__ import annotations

__all__ = [
    "create_adapter_server",
    "is_core_extension_available",
    "resolve_mcp_http_config_class",
]

from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._runtime.config_bridge import resolve_mcp_http_config_class
