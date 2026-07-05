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

    class _LiteralMeta(type):
        def __getitem__(cls, item):
            return cls

    class Literal(metaclass=_LiteralMeta):  # type: ignore[no-redef]
        """Literal stand-in for Python 3.7 static analysis only."""


__all__ = ["Literal", "Protocol", "runtime_checkable"]
