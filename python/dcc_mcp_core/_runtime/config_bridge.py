"""Resolve the active ``McpHttpConfig`` implementation for the current wheel."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core._runtime.core_availability import is_core_extension_available


def resolve_mcp_http_config_class() -> type[Any]:
    """Return PyO3 ``McpHttpConfig`` when available, else the pure-Python dataclass."""
    if is_core_extension_available():
        from dcc_mcp_core._core import McpHttpConfig as CoreMcpHttpConfig

        return CoreMcpHttpConfig
    from dcc_mcp_core._runtime.mcp_http_config import McpHttpConfig as PureMcpHttpConfig

    return PureMcpHttpConfig


McpHttpConfig = resolve_mcp_http_config_class()
