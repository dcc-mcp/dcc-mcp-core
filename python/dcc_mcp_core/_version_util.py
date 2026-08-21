"""Shared version parsing helpers for Python runtime decisions."""

from __future__ import annotations

import importlib
import re
import sys
from typing import Any


def package_version(*, fallback: str, load_core: bool = False) -> str:
    """Return the core package version without requiring the native module.

    An already-loaded native extension is authoritative. ``load_core=True``
    additionally permits importing it; import-light callers leave that option
    disabled. Distribution metadata is the shared fallback before the caller's
    explicit final value.
    """
    core_version = _core_version(load_core)
    if core_version is not None:
        return core_version

    try:
        importlib_metadata = importlib.import_module("importlib.metadata")
    except ImportError:
        try:
            importlib_metadata = importlib.import_module("importlib_metadata")
        except ImportError:
            return str(fallback)

    try:
        return str(importlib_metadata.version("dcc-mcp-core"))
    except Exception:
        return str(fallback)


def _core_version(load_core: bool) -> str | None:
    core: Any = sys.modules.get("dcc_mcp_core._core")
    if core is None and load_core:
        try:
            core = importlib.import_module("dcc_mcp_core._core")
        except Exception:
            core = None
    version = getattr(core, "__version__", None) if core is not None else None
    return str(version) if version is not None else None


def parse_semver(value: str) -> tuple[int, int, int] | None:
    """Return the numeric SemVer core, or ``None`` for an invalid version."""
    text = str(value).strip().lstrip("vV")
    text = re.split(r"[-+]", text, maxsplit=1)[0]
    parts = text.split(".")
    if not parts or any(not part.isdigit() for part in parts[:3]):
        return None
    padded = [int(part) for part in parts[:3]]
    while len(padded) < 3:
        padded.append(0)
    return tuple(padded[:3])
