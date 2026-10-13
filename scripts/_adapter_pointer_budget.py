"""One cooperative subprocess deadline for an adapter-pointer batch.

The context is opt-in: other users of the PR helpers retain their per-command
timeouts. Every git/gh command must obtain a fresh timeout immediately before
starting, including commands inside a single repository's PR lookup loop.
"""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from contextvars import ContextVar
import time

_DEADLINE: ContextVar[float | None] = ContextVar("adapter_pointer_deadline", default=None)


class BatchDeadlineExceeded(TimeoutError):
    """No more repository work may start in this adapter-pointer batch."""


def positive_seconds(value: str) -> int:
    seconds = int(value)
    if seconds <= 0:
        raise argparse.ArgumentTypeError("batch timeout must be a positive number of seconds")
    return seconds


@contextmanager
def batch_timeout(seconds: int | None):
    """Scope one monotonic deadline without extending any outer deadline."""
    if seconds is None:
        yield
        return
    if seconds <= 0:
        raise ValueError("batch timeout must be positive")
    deadline = time.monotonic() + seconds
    outer = _DEADLINE.get()
    token = _DEADLINE.set(min(outer, deadline) if outer is not None else deadline)
    try:
        yield
    finally:
        _DEADLINE.reset(token)


def command_timeout(per_operation: int) -> float:
    """Keep the per-command cap and spend only the remaining batch budget."""
    deadline = _DEADLINE.get()
    if deadline is None:
        return per_operation
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise BatchDeadlineExceeded("adapter pointer batch deadline exhausted")
    return min(per_operation, remaining)
