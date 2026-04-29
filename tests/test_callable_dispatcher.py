"""Tests for ``BaseDccCallableDispatcherFull`` + ``InProcessCallableDispatcher`` (issue #520)."""

# Import built-in modules
from __future__ import annotations

import threading
import time

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._server.callable_dispatcher import BaseDccCallableDispatcherFull
from dcc_mcp_core._server.callable_dispatcher import BaseDccPump
from dcc_mcp_core._server.callable_dispatcher import DrainStats
from dcc_mcp_core._server.callable_dispatcher import InProcessCallableDispatcher
from dcc_mcp_core._server.callable_dispatcher import JobEntry
from dcc_mcp_core._server.callable_dispatcher import JobOutcome
from dcc_mcp_core._server.callable_dispatcher import PendingEnvelope
from dcc_mcp_core._server.callable_dispatcher import PumpStats
from dcc_mcp_core._server.callable_dispatcher import current_callable_job

# ── public surface ───────────────────────────────────────────────────────────


def test_top_level_exports() -> None:
    for name in (
        "BaseDccCallableDispatcherFull",
        "BaseDccPump",
        "InProcessCallableDispatcher",
        "JobEntry",
        "JobOutcome",
        "PendingEnvelope",
        "current_callable_job",
    ):
        assert hasattr(dcc_mcp_core, name), name
        assert name in dcc_mcp_core.__all__


def test_protocols_runtime_checkable() -> None:
    assert isinstance(InProcessCallableDispatcher(), BaseDccCallableDispatcherFull)


# ── JobEntry ────────────────────────────────────────────────────────────────


def test_job_entry_defaults() -> None:
    entry = JobEntry(request_id="r1", task=lambda: 42)
    assert entry.request_id == "r1"
    assert entry.cancelled is False
    assert entry.outcome is None
    assert isinstance(entry.submitted_at, float)


def test_job_entry_signal_done_unblocks_wait() -> None:
    entry = JobEntry(request_id="r1", task=lambda: None)
    flipped: list[bool] = []

    def waiter() -> None:
        flipped.append(entry.wait(timeout=2.0))

    t = threading.Thread(target=waiter)
    t.start()
    time.sleep(0.05)
    entry.signal_done()
    t.join(timeout=2.0)
    assert flipped == [True]


# ── synchronous submit_callable ─────────────────────────────────────────────


def test_submit_callable_returns_value() -> None:
    d = InProcessCallableDispatcher()
    outcome = d.submit_callable("r1", lambda: 5 + 7)
    assert outcome.ok is True
    assert outcome.value == 12
    assert outcome.error is None
    assert outcome.elapsed_ms >= 0


def test_submit_callable_captures_exception() -> None:
    d = InProcessCallableDispatcher()

    def boom() -> None:
        raise ValueError("nope")

    outcome = d.submit_callable("r1", boom)
    assert outcome.ok is False
    assert "ValueError" in outcome.error
    assert "nope" in outcome.error


def test_submit_callable_publishes_current_job_during_run() -> None:
    d = InProcessCallableDispatcher()
    seen: list[JobEntry | None] = []

    def task() -> str:
        seen.append(current_callable_job.get())
        return "ok"

    outcome = d.submit_callable("r1", task)
    assert outcome.ok is True
    assert seen[0] is not None
    assert seen[0].request_id == "r1"
    # Outside the call → contextvar must be reset to None.
    assert current_callable_job.get() is None


def test_submit_callable_after_shutdown_returns_error() -> None:
    d = InProcessCallableDispatcher()
    d.shutdown()
    outcome = d.submit_callable("r1", lambda: 1)
    assert outcome.ok is False
    assert outcome.error == "dispatcher is shut down"


# ── async submit + cancel ───────────────────────────────────────────────────


def test_submit_async_returns_pending_envelope_then_completes() -> None:
    d = InProcessCallableDispatcher()
    completed: list[JobOutcome] = []
    barrier = threading.Event()

    def task() -> int:
        barrier.wait(timeout=2.0)
        return 99

    pending = d.submit_async_callable("r1", task, on_complete=completed.append, progress_token="p1")
    assert isinstance(pending, PendingEnvelope)
    assert pending.request_id == "r1"
    assert pending.job_id  # uuid hex
    assert pending.progress_token == "p1"

    barrier.set()
    deadline = time.monotonic() + 2.0
    while not completed and time.monotonic() < deadline:
        time.sleep(0.01)
    assert completed and completed[0].ok is True
    assert completed[0].value == 99


def test_cancel_unknown_returns_false() -> None:
    d = InProcessCallableDispatcher()
    assert d.cancel("ghost") is False


def test_cancel_in_flight_returns_true_and_marks_entry() -> None:
    d = InProcessCallableDispatcher()
    proceed = threading.Event()

    def task() -> int:
        proceed.wait(timeout=2.0)
        return 1

    pending = d.submit_async_callable("r1", task)
    # Allow the worker thread to register the entry.
    time.sleep(0.05)
    cancelled = d.cancel("r1")
    proceed.set()
    assert cancelled is True
    assert pending.request_id == "r1"


def test_shutdown_returns_count_of_active_jobs() -> None:
    d = InProcessCallableDispatcher()
    proceed = threading.Event()

    def task() -> None:
        proceed.wait(timeout=2.0)

    d.submit_async_callable("a", task)
    d.submit_async_callable("b", task)
    time.sleep(0.05)
    n = d.shutdown(reason="test")
    proceed.set()
    assert n == 2


# ── on_complete error swallowing ────────────────────────────────────────────


def test_on_complete_callback_errors_are_swallowed() -> None:
    d = InProcessCallableDispatcher()
    seen: list[str] = []

    def cb(_: JobOutcome) -> None:
        seen.append("called")
        raise RuntimeError("callback boom")

    pending = d.submit_async_callable("r1", lambda: None, on_complete=cb)
    assert isinstance(pending, PendingEnvelope)
    deadline = time.monotonic() + 2.0
    while not seen and time.monotonic() < deadline:
        time.sleep(0.01)
    assert seen == ["called"]
    # Subsequent submits must still work.
    out = d.submit_callable("r2", lambda: 7)
    assert out.ok is True


# ── BaseDccPump / DrainStats / PumpStats containers ─────────────────────────


def test_drain_stats_defaults() -> None:
    s = DrainStats()
    assert s.drained == 0
    assert s.elapsed_ms == 0.0
    assert s.overrun is False


def test_pump_stats_defaults() -> None:
    s = PumpStats()
    assert s.ticks == 0
    assert s.drained == 0
    assert s.overrun_cycles == 0


def test_base_pump_protocol_is_runtime_checkable() -> None:
    class _P:
        @property
        def stats(self) -> PumpStats:
            return PumpStats()

        def drain_queue(self, budget_ms: int) -> DrainStats:
            return DrainStats(drained=0, elapsed_ms=0.0)

    assert isinstance(_P(), BaseDccPump)
