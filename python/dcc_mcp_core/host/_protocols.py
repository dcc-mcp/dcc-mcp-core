"""Shared structural protocols for host dispatcher adapters."""

# Import future modules
from __future__ import annotations

# `typing.Protocol` and `typing.runtime_checkable` are 3.8+. For Python 3.7
# (Maya 2022, Blender 2.83), expose a duck-typed base class there.
from typing import Protocol
from typing import runtime_checkable

# Import local modules
try:
    from dcc_mcp_core._core import TickOutcome
except ImportError:
    # py37-lite wheel: pure-Python fallback when _core is absent.
    class TickOutcome:  # type: ignore[no-redef]
        """Duck-typed fallback for TickOutcome (py37-lite)."""

        more_pending: bool


@runtime_checkable
class TickableDispatcher(Protocol):
    """Minimum dispatcher surface required by host tick drivers."""

    def tick(self, max_jobs: int = ...) -> TickOutcome: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...
