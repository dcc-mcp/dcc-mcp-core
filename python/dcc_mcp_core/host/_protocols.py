"""Shared structural protocols for host dispatcher adapters."""

# Import future modules
from __future__ import annotations

# `typing.Protocol` and `typing.runtime_checkable` are 3.8+. For Python 3.7
# (Maya 2022, Blender 2.83), expose a duck-typed base class there.
from typing import Protocol
from typing import runtime_checkable

# Import local modules
from dcc_mcp_core._core import TickOutcome


@runtime_checkable
class TickableDispatcher(Protocol):
    """Minimum dispatcher surface required by host tick drivers."""

    def tick(self, max_jobs: int = ...) -> TickOutcome: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...
