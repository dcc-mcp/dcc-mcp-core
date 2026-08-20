"""Deprecated import shim for the interpreter-neutral lite fallback."""

from __future__ import annotations

from importlib import import_module
import warnings

_impl = import_module("dcc_mcp_core._lite_fallback")

warnings.warn(
    "dcc_mcp_core._py37_fallback is deprecated; import dcc_mcp_core._lite_fallback instead",
    DeprecationWarning,
    stacklevel=2,
)

__all__ = _impl.__all__


def __getattr__(name: str) -> object:
    """Forward legacy named imports without duplicating the implementation."""
    return getattr(_impl, name)
