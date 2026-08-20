"""Standard typing APIs with the official Python 3.7 backport."""

from __future__ import annotations

try:
    from typing import Literal
    from typing import Protocol
    from typing import runtime_checkable
except ImportError:  # pragma: no cover - Python 3.7 only
    from typing_extensions import Literal
    from typing_extensions import Protocol
    from typing_extensions import runtime_checkable

__all__ = ["Literal", "Protocol", "runtime_checkable"]
