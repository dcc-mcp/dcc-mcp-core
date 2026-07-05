"""Typing backports for Python 3.7 (Maya 2022 / Blender 2.83)."""

from __future__ import annotations

try:
    from typing import Literal
    from typing import Protocol
    from typing import runtime_checkable
except ImportError:  # pragma: no cover - Python 3.7 only

    def runtime_checkable(cls):  # type: ignore[misc]
        return cls

    class Protocol:  # type: ignore[no-redef]
        """Duck-typed Protocol stand-in for Python 3.7."""

    def Literal(*_args):  # type: ignore[misc]
        """Literal stand-in for Python 3.7 static analysis only."""
        return str


__all__ = ["Literal", "Protocol", "runtime_checkable"]
