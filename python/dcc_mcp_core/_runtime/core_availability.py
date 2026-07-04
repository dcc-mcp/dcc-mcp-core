"""Detect whether the compiled ``dcc_mcp_core._core`` extension is loadable."""

from __future__ import annotations

import importlib
import sys

_CORE_AVAILABLE: bool | None = None


def is_core_extension_available() -> bool:
    """Return ``True`` when the PyO3 ``_core`` extension can be imported."""
    global _CORE_AVAILABLE
    if _CORE_AVAILABLE is not None:
        return _CORE_AVAILABLE

    if sys.version_info < (3, 8):
        _CORE_AVAILABLE = False
        return _CORE_AVAILABLE

    try:
        importlib.import_module("dcc_mcp_core._core")
    except ImportError:
        _CORE_AVAILABLE = False
    else:
        _CORE_AVAILABLE = True
    return _CORE_AVAILABLE
