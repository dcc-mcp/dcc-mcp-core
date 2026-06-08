"""Shared lazy-import helpers for modules that defer expensive imports.

This module is intentionally dependency-free so that it can be imported
early during package initialisation without triggering any compiled
extension loads.  It provides the common ``__getattr__`` and ``__dir__``
logic that is otherwise duplicated across ``dcc_mcp_core`` and adapter
sub-packages.

Usage in a package ``__init__.py``::

    from dcc_mcp_core._lazy import resolve_lazy_symbol

    def __getattr__(name: str) -> object:
        if name == "__version__":
            return _resolve_metadata(name)
        return resolve_lazy_symbol(name, _LAZY, module_name=__name__)

Usage in a sub-module that defines its own lazy map::

    from dcc_mcp_core._lazy import lazy_dir, resolve_lazy_symbol

    def __getattr__(name: str) -> object:
        return resolve_lazy_symbol(name, _LAZY_EXPORTS, module_name=__name__)

    def __dir__() -> list[str]:
        return lazy_dir(_LAZY_EXPORTS)
"""

from __future__ import annotations

import importlib
import sys
from typing import Any


def resolve_lazy_symbol(
    name: str,
    exports: dict[str, str],
    *,
    module_name: str,
    optional: frozenset[str] | None = None,
) -> Any:
    """Resolve a lazy symbol by importing its source module and caching the result.

    Call this from a module's ``__getattr__`` to lazily load symbols on first
    access.  The resolved value is cached on the calling module so subsequent
    accesses skip ``__getattr__`` entirely.

    Parameters
    ----------
    name:
        The attribute name being looked up (passed through from ``__getattr__``).
    exports:
        A mapping from attribute name to fully-qualified source module path
        (e.g. ``{"Foo": "dcc_mcp_core._core"}``).
    module_name:
        The fully-qualified name of the module whose ``__getattr__`` delegates
        to this function.  Used for error messages and for caching the result
        back on the module object via ``sys.modules[module_name]``.
    optional:
        When provided, symbols in this set that are absent from their source
        module return ``None`` instead of raising :class:`AttributeError`.
        Useful for symbols backed by optional compiled extensions.

    Returns
    -------
    Any
        The imported symbol value, or ``None`` for optional symbols whose
        source module does not expose them.

    Raises
    ------
    AttributeError
        If *name* is not in *exports*, or if the source module does not expose
        *name* and it is not listed in *optional*.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core._lazy import resolve_lazy_symbol

        _LAZY = {"DccInfo": "dcc_mcp_core._core"}
        _OPTIONAL = frozenset({"PromptHandle"})

        def __getattr__(name: str) -> object:
            return resolve_lazy_symbol(
                name, _LAZY,
                module_name=__name__,
                optional=_OPTIONAL,
            )

    """
    module_path = exports.get(name)
    if module_path is None:
        raise AttributeError(f"module {module_name!r} has no attribute {name!r}")

    mod = importlib.import_module(module_path)
    value = getattr(mod, name, None)

    if value is None and (optional is None or name not in optional):
        raise AttributeError(f"module {module_path!r} has no attribute {name!r}")

    # Cache on the calling module so subsequent accesses skip __getattr__.
    caller_mod = sys.modules[module_name]
    setattr(caller_mod, name, value)
    return value


def lazy_dir(exports: dict[str, str]) -> list[str]:
    """Return a sorted list of lazy-export names for use as ``__dir__``.

    Parameters
    ----------
    exports:
        A mapping from attribute name to source module path.

    Returns
    -------
    list[str]
        Sorted list of lazy-export attribute names.

    Example
    -------
    .. code-block:: python

        from dcc_mcp_core._lazy import lazy_dir

        def __dir__() -> list[str]:
            return lazy_dir(_LAZY_EXPORTS)

    """
    return sorted(exports)
