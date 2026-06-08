"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem.

This package is powered by a Rust core via PyO3. The native extension module
``dcc_mcp_core._core`` is compiled by maturin and provides most public APIs.

The pure-Python ``dcc_mcp_core.skill`` sub-module provides lightweight helpers
for skill script authors — no compiled extension required::

    from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

Public symbols are lazily imported from their source modules on first access so
that ``import dcc_mcp_core`` does not eagerly load the PyO3 extension from an
adapter install root. This keeps installer/uninstaller scripts free to inspect
and remove bundled native artifacts before opting into Rust-backed APIs.
"""

# Import future modules
from __future__ import annotations

from dcc_mcp_core._exports import _LAZY
from dcc_mcp_core._exports import _OPTIONAL
from dcc_mcp_core._exports import PUBLIC_EXPORTS as __all__
from dcc_mcp_core._lazy import resolve_lazy_symbol


def _metadata_value(name: str, default: str) -> str:
    """Return package metadata without importing the native extension."""
    import sys

    core = sys.modules.get(f"{__name__}._core")
    if core is not None:
        return str(getattr(core, name, default))

    try:
        from importlib import metadata as importlib_metadata
    except ImportError:
        try:
            import importlib_metadata  # type: ignore[import-not-found]
        except ImportError:
            return default

    try:
        if name == "__version__":
            return importlib_metadata.version("dcc-mcp-core")
        if name == "__author__":
            meta = importlib_metadata.metadata("dcc-mcp-core")
            author = meta.get("Author") or "unknown"
            email = meta.get("Author-email")
            return f"{author} <{email}>" if email else author
    except importlib_metadata.PackageNotFoundError:
        return default
    return default


def __getattr__(name: str) -> object:
    """Lazily import public symbols on first access.

    This keeps ``import dcc_mcp_core`` import-light: the PyO3 extension is only
    loaded when a Rust-backed symbol or the ``_core`` submodule is requested.
    """
    if name == "_core":
        import importlib

        mod = importlib.import_module(f"{__name__}._core")
        globals()[name] = mod
        return mod

    if name == "__version__":
        value = _metadata_value(name, "0.0.0-dev")
        globals()[name] = value
        return value

    if name == "__author__":
        value = _metadata_value(name, "unknown")
        globals()[name] = value
        return value

    return resolve_lazy_symbol(name, _LAZY, module_name=__name__, optional=_OPTIONAL)
