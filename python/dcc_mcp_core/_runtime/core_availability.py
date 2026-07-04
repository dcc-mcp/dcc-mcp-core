"""Detect whether the compiled ``dcc_mcp_core._core`` extension is loadable."""

from __future__ import annotations

import importlib

_CORE_AVAILABLE: bool | None = None


def is_core_extension_available() -> bool:
    """Return ``True`` when the PyO3 ``_core`` extension can be imported."""
    global _CORE_AVAILABLE
    if _CORE_AVAILABLE is not None:
        return _CORE_AVAILABLE


    try:
        importlib.import_module("dcc_mcp_core._core")
    except ImportError:
        _CORE_AVAILABLE = False
    else:
        _CORE_AVAILABLE = True
    return _CORE_AVAILABLE
