"""dcc-mcp-core: Foundational library for the DCC Model Context Protocol (MCP) ecosystem.

This package is powered by a Rust core via PyO3. The native extension module
``dcc_mcp_core._core`` is compiled by maturin and provides all public APIs.

The pure-Python ``dcc_mcp_core.skill`` sub-module provides lightweight helpers
for skill script authors — no compiled extension required::

    from dcc_mcp_core.skill import skill_entry, skill_success, skill_error

Public symbols are lazily imported from their source modules on first access
so that ``import dcc_mcp_core`` does not eagerly pull the entire PyO3 surface
(~30 MB of capture/process/sandbox machinery) into every importer's namespace.
All symbols in ``__all__`` are still accessible via ``from dcc_mcp_core import X``.
"""

# Import future modules
from __future__ import annotations

# Import _core when the compiled extension is present, but keep the package
# importable from a source checkout. Skill scripts often run inside embedded DCC
# Python with only the pure-Python helpers on sys.path; `dcc_mcp_core.skill`
# must remain usable there and should not require a maturin-built extension.
try:
    from dcc_mcp_core import _core
except ImportError as _core_import_error:
    _CORE_IMPORT_ERROR: ImportError | None = _core_import_error
    __version__: str = "0.0.0-dev"
    __author__: str = "unknown"
else:
    _CORE_IMPORT_ERROR = None
    __version__ = getattr(_core, "__version__", "0.0.0-dev")
    __author__ = getattr(_core, "__author__", "unknown")

from dcc_mcp_core._exports import _LAZY
from dcc_mcp_core._exports import _OPTIONAL
from dcc_mcp_core._exports import PUBLIC_EXPORTS as __all__


def __getattr__(name: str) -> object:
    """Lazily import public symbols on first access.

    This keeps ``import dcc_mcp_core`` lightweight — the ~30 MB PyO3
    surface is only loaded when a symbol is actually used.
    """
    module_path = _LAZY.get(name)
    if module_path is None:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")

    import importlib

    mod = importlib.import_module(module_path)
    value = getattr(mod, name, None)

    if value is None and name not in _OPTIONAL:
        raise AttributeError(f"module {module_path!r} has no attribute {name!r}")

    # Cache on the module object so subsequent accesses skip __getattr__.
    globals()[name] = value
    return value
