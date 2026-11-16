"""Windows DLL search setup for embedded Python hosts."""

from __future__ import annotations

import os
from pathlib import Path
import sys
from typing import Any

_DLL_DIRECTORY_HANDLES: dict[str, Any] = {}


def embedded_python_dll_directories() -> tuple[str, ...]:
    """Return bounded, absolute interpreter directories, never PATH or cwd."""
    candidates = [os.environ.get("DCC_MCP_PYTHON3_DLL_DIR"), sys.prefix, sys.base_prefix]
    if sys.executable:
        candidates.append(str(Path(sys.executable).parent))
    directories: dict[str, str] = {}
    for candidate in candidates:
        if not candidate:
            continue
        path = Path(candidate)
        if not path.is_absolute():
            continue
        resolved = str(path.resolve())
        directories.setdefault(os.path.normcase(resolved), resolved)
    return tuple(directories.values())


def prepare_embedded_python_dll_search() -> tuple[str, ...]:
    """Retain DLL-directory handles for the lifetime of the embedded process."""
    if sys.platform != "win32" or not hasattr(os, "add_dll_directory"):
        return ()
    registered = []
    for directory in embedded_python_dll_directories():
        if not Path(directory).is_dir():
            continue
        key = os.path.normcase(directory)
        if key not in _DLL_DIRECTORY_HANDLES:
            try:
                _DLL_DIRECTORY_HANDLES[key] = os.add_dll_directory(directory)
            except OSError:
                continue
        registered.append(directory)
    return tuple(registered)
