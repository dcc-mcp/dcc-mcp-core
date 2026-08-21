"""Import-light path coercion helpers."""

# ruff: noqa: UP045

from __future__ import annotations

from pathlib import Path
from typing import Any
from typing import Optional


def to_resolved_path(value: Any) -> Optional[Path]:
    """Return an expanded absolute path, or ``None`` for an empty value.

    ``Path.resolve`` can fail for inaccessible or transient filesystem
    entries. In that case the lexical absolute path remains useful to
    lifecycle diagnostics and subprocess configuration.
    """
    if value in (None, ""):
        return None
    path = Path(str(value)).expanduser()
    try:
        return path.resolve()
    except OSError:
        return path.absolute()
