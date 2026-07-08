"""Python wrappers for host-facing MCP wire normalization helpers."""

from __future__ import annotations

from typing import Any

try:
    from dcc_mcp_core._core import normalize_tool_arguments as _normalize_tool_arguments
    from dcc_mcp_core._core import normalize_tool_meta as _normalize_tool_meta
except ImportError:
    from dcc_mcp_core.host._fallback import normalize_tool_arguments as _normalize_tool_arguments
    from dcc_mcp_core.host._fallback import normalize_tool_meta as _normalize_tool_meta


def normalize_tool_arguments(arguments: Any = None) -> dict[str, Any]:
    """Normalize raw tool ``arguments`` to a JSON-object-shaped dict."""
    return _normalize_tool_arguments(arguments)


def normalize_tool_meta(meta: Any = None) -> dict[str, Any] | None:
    """Normalize raw tool ``_meta`` to a dict or ``None``."""
    return _normalize_tool_meta(meta)
