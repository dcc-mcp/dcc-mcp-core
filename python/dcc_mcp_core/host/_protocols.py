"""Shared structural protocols for host dispatcher adapters."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import sys
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    # Import local modules
    from dcc_mcp_core._core import TickOutcome

# `typing.Protocol` and `typing.runtime_checkable` are 3.8+. The package
# still supports Python 3.7 for older embedded DCC runtimes such as Maya 2022
# and Blender 2.83, so expose a duck-typed base class there.
if sys.version_info >= (3, 8):
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    def runtime_checkable(cls):
        return cls

    class Protocol:  # type: ignore[no-redef]
        pass


@runtime_checkable
class TickableDispatcher(Protocol):
    """Minimum dispatcher surface required by host tick drivers."""

    def tick(self, max_jobs: int = ...) -> TickOutcome: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...
