"""Windows DLL search setup for embedded Python hosts."""

from __future__ import annotations

import os
from pathlib import Path
import sys
from typing import Any

_DLL_DIRECTORY_HANDLES: list[Any] = []


def prepare_embedded_python_dll_search() -> None:
    """Expose an embedded Python prefix to ABI3 extension dependencies."""
    if sys.platform != "win32" or not hasattr(os, "add_dll_directory"):
        return
    prefix = Path(sys.prefix).resolve()
    if not prefix.is_dir():
        return
    try:
        _DLL_DIRECTORY_HANDLES.append(os.add_dll_directory(str(prefix)))
    except OSError:
        return
