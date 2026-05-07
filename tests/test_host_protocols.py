"""Tests for host dispatcher protocol exports."""

# Import future modules
from __future__ import annotations

# Import local modules
from dcc_mcp_core.host import BlockingDispatcher
from dcc_mcp_core.host import QueueDispatcher
from dcc_mcp_core.host import TickableDispatcher


class _DuckDispatcher:
    def tick(self, max_jobs: int = 16):
        return None

    def shutdown(self) -> None:
        pass

    def is_shutdown(self) -> bool:
        return False


def test_tickable_dispatcher_accepts_core_dispatchers() -> None:
    assert isinstance(QueueDispatcher(), TickableDispatcher)
    assert isinstance(BlockingDispatcher(), TickableDispatcher)


def test_tickable_dispatcher_accepts_duck_typed_dispatcher() -> None:
    assert isinstance(_DuckDispatcher(), TickableDispatcher)


def test_tickable_dispatcher_protocol_methods_are_noop_stubs() -> None:
    dispatcher = object()
    assert TickableDispatcher.tick(dispatcher) is None
    assert TickableDispatcher.shutdown(dispatcher) is None
    assert TickableDispatcher.is_shutdown(dispatcher) is None
