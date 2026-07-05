"""Shared structural protocols for host dispatcher adapters."""

# Import future modules
from __future__ import annotations

# `typing.Protocol` and `typing.runtime_checkable` are 3.8+. For Python 3.7
# (Maya 2022, Blender 2.83), expose duck-typed fallbacks with the same
# attribute contracts; concrete dispatchers do not need to inherit either way.
from dcc_mcp_core._typing_compat import Protocol
from dcc_mcp_core._typing_compat import runtime_checkable

try:
    from dcc_mcp_core._core import TickOutcome
except ImportError:
    from dcc_mcp_core.host._fallback import TickOutcome


@runtime_checkable
class TickableDispatcher(Protocol):
    """Minimum dispatcher surface required by host tick drivers."""

    def tick(self, max_jobs: int = ...) -> TickOutcome: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...
