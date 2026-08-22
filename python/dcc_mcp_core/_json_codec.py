"""Private backend selection for the package JSON helpers.

The native extension is preferred when available.  Python 3.7 lite installs
fall back to the standard library here so public callers do not each grow
their own backend-selection logic.

This module is intentionally private.  Skill authors should continue to use
``dcc_mcp_core.skills_helper``; legacy top-level imports remain supported.
"""

from __future__ import annotations

import importlib
import json as _stdlib_json
from typing import Any


def _optional_core_symbol(name: str) -> Any:
    """Return a native JSON symbol, or ``None`` for a lite installation."""
    try:
        core = importlib.import_module("dcc_mcp_core._core")
    except ModuleNotFoundError as exc:
        if exc.name == "dcc_mcp_core._core":
            return None
        raise
    return getattr(core, name)


def json_dumps(obj: Any, *, ensure_ascii: bool = True, indent: int | None = None) -> str:
    """Serialize *obj* with the native codec, or stdlib in py37-lite."""
    dumps = _optional_core_symbol("json_dumps")
    if dumps is not None:
        return dumps(obj, ensure_ascii=ensure_ascii, indent=indent)
    return _stdlib_json.dumps(obj, ensure_ascii=ensure_ascii, indent=indent)


def json_loads(s: str) -> Any:
    """Deserialize *s* with the native codec, or stdlib in py37-lite."""
    loads = _optional_core_symbol("json_loads")
    if loads is not None:
        return loads(s)
    return _stdlib_json.loads(s)


# Preserve the existing stdlib semantics of pure-Python helpers that are not
# part of the public native-first JSON API.  Keeping this callable private also
# avoids expanding the native codec surface in this refactor.
_stdlib_json_loads = _stdlib_json.loads


__all__ = ["json_dumps", "json_loads"]
