"""Pure-Python wire normalization used when ``dcc_mcp_core._core`` is absent."""

from __future__ import annotations

import json
from typing import Any


def _normalize_object_root(value: Any, *, allow_none: bool, label: str) -> dict[str, Any] | None:
    if value is None:
        return None if allow_none else {}
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None if allow_none else {}
        try:
            decoded = json.loads(text)
        except json.JSONDecodeError as exc:
            raise ValueError(f"{label}-string-not-json") from exc
        if isinstance(decoded, dict):
            return decoded
        raise ValueError(f"{label}-decoded-not-object")
    raise ValueError(f"{label}-not-object")


def normalize_tool_arguments(arguments: Any = None) -> dict[str, Any]:
    """Normalize tool arguments to an object-shaped dict."""
    normalized = _normalize_object_root(arguments, allow_none=False, label="arguments")
    return normalized if normalized is not None else {}


def normalize_tool_meta(meta: Any = None) -> dict[str, Any] | None:
    """Normalize tool ``_meta`` to a dict or ``None``."""
    return _normalize_object_root(meta, allow_none=True, label="arguments")
