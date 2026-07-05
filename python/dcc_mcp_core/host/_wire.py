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
    if _normalize_tool_arguments is None:
        raise ImportError("normalize_tool_arguments requires the _core extension (not available in py37-lite)")
    return _normalize_tool_arguments(arguments)


def normalize_tool_meta(meta: Any = None) -> dict[str, Any] | None:
    """Normalize raw tool ``_meta`` to a dict or ``None``."""
    if _normalize_tool_meta is None:
        raise ImportError("normalize_tool_meta requires the _core extension (not available in py37-lite)")
    return _normalize_tool_meta(meta)
