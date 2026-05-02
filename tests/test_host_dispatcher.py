"""Integration tests for :mod:`dcc_mcp_core.host`.

Exercises the Rust-backed :class:`QueueDispatcher` / :class:`BlockingDispatcher`
through the Python facade plus the pure-Python :class:`StandaloneHost`
driver. No DCC dependencies — everything runs in a plain CPython process.

Contract tested (matches SOLID design notes in the plan):

* Main-thread affinity: the posted callable executes on the StandaloneHost
  driver thread, never on the poster thread. Substitute StandaloneHost for
  a DCC-native timer and the same contract holds (LSP/OCP).
* FIFO ordering inside a single tick batch.
* Error taxonomy: callable raising a Python exception propagates that
  exception through ``wait``; shutdown and timeout surface as
  ``DispatchError``.
* Context-manager lifecycle is idempotent.
* Concurrent posters don't deadlock or lose jobs.
"""

from __future__ import annotations

# Import built-in modules
import threading
import time
from typing import Any

# Import third-party modules
import pytest

# Import local modules — exercise the public package surface.
from dcc_mcp_core.host import BlockingDispatcher
from dcc_mcp_core.host import DispatchError
from dcc_mcp_core.host import QueueDispatcher
from dcc_mcp_core.host import StandaloneHost
from dcc_mcp_core.host import TickOutcome

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def dispatcher() -> QueueDispatcher:
    """Fresh dispatcher per test so pending state can't leak."""
    d = QueueDispatcher()
    yield d
    if not d.is_shutdown():
        d.shutdown()


@pytest.fixture
def blocking_dispatcher() -> BlockingDispatcher:
    d = BlockingDispatcher()
    yield d
    if not d.is_shutdown():
        d.shutdown()


@pytest.fixture
def host(dispatcher: QueueDispatcher) -> StandaloneHost:
    """Start a StandaloneHost; auto-stop on fixture teardown."""
    h = StandaloneHost(dispatcher)
    h.start()
    try:
        yield h
    finally:
        h.stop()


# ---------------------------------------------------------------------------
# Post → tick → wait round-trip
# ---------------------------------------------------------------------------


def test_post_then_get_result(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """Happy path: post a lambda, receive its return value through ``wait``."""
    handle = dispatcher.post(lambda: 42)
    assert handle.wait(timeout=2.0) == 42


def test_post_returns_complex_object(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """Return values can be any Python object, not just primitives."""
    payload = {"items": [1, 2, 3], "meta": {"kind": "ok"}}
    handle = dispatcher.post(lambda: payload)
    got = handle.wait(timeout=2.0)
    assert got == payload


def test_tick_outcome_reports_counts(dispatcher: QueueDispatcher) -> None:
    """Direct ``tick`` call (no StandaloneHost) exposes TickOutcome counters."""
    for _ in range(3):
        dispatcher.post(lambda: None)
    # Drain on the calling thread.
    outcome: TickOutcome = dispatcher.tick(max_jobs=16)
    assert outcome.jobs_executed == 3
    assert outcome.jobs_panicked == 0
    assert outcome.more_pending is False


# ---------------------------------------------------------------------------
# Ordering
# ---------------------------------------------------------------------------


def test_fifo_ordering(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """Jobs execute in submission order across a single tick."""
    log: list[int] = []
    lock = threading.Lock()

    def _append(i: int):
        def _fn() -> None:
            with lock:
                log.append(i)

        return _fn

    handles = [dispatcher.post(_append(i)) for i in range(20)]
    for h in handles:
        h.wait(timeout=2.0)
    assert log == list(range(20))


# ---------------------------------------------------------------------------
# Main-thread affinity
# ---------------------------------------------------------------------------


def test_jobs_run_on_host_thread_not_poster_thread(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """The callable runs on the StandaloneHost driver thread, not the poster."""
    poster_thread = threading.current_thread().ident
    captured: dict[str, int | None] = {"tid": None}

    def _capture() -> None:
        captured["tid"] = threading.current_thread().ident

    dispatcher.post(_capture).wait(timeout=2.0)
    assert captured["tid"] is not None
    assert captured["tid"] != poster_thread, "Dispatcher ran the job on the poster thread — main-thread affinity broken"


# ---------------------------------------------------------------------------
# Error taxonomy
# ---------------------------------------------------------------------------


def test_python_exception_propagates_through_wait(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """A Python exception raised in the callable re-raises on ``wait``.

    Exceptions are first-class Python errors — not wrapped in DispatchError —
    so ``except ValueError`` semantics still work for callers.
    """

    def _bang() -> None:
        raise ValueError("bang")

    handle = dispatcher.post(_bang)
    with pytest.raises(ValueError, match="bang"):
        handle.wait(timeout=2.0)


def test_wait_timeout_raises_dispatch_error(dispatcher: QueueDispatcher) -> None:
    """No StandaloneHost — never ticked; ``wait(timeout)`` raises DispatchError."""
    handle = dispatcher.post(lambda: 1)
    with pytest.raises(DispatchError) as excinfo:
        handle.wait(timeout=0.05)
    assert "timeout" in str(excinfo.value)


def test_shutdown_cancels_pending_posts(dispatcher: QueueDispatcher) -> None:
    """Shutdown resolves pending jobs to DispatchError("shutdown") without running."""
    handle = dispatcher.post(lambda: 1)
    dispatcher.shutdown()
    with pytest.raises(DispatchError) as excinfo:
        handle.wait(timeout=1.0)
    assert "shutdown" in str(excinfo.value)


def test_wait_twice_raises_runtime_error(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """The result can only be observed once — second ``wait`` is an error."""
    handle = dispatcher.post(lambda: 7)
    assert handle.wait(timeout=2.0) == 7
    with pytest.raises(RuntimeError, match="already consumed"):
        handle.wait(timeout=0.1)


# ---------------------------------------------------------------------------
# Lifecycle (context manager, start/stop)
# ---------------------------------------------------------------------------


def test_context_manager_drives_and_shuts_down(dispatcher: QueueDispatcher) -> None:
    """`with` block starts and stops the host; dispatcher shuts down on exit."""
    with StandaloneHost(dispatcher) as h:
        assert h.is_running
        assert dispatcher.post(lambda: "ok").wait(timeout=2.0) == "ok"
    assert not h.is_running
    assert dispatcher.is_shutdown()


def test_start_twice_raises(dispatcher: QueueDispatcher) -> None:
    """Calling ``start`` while already running is an error."""
    h = StandaloneHost(dispatcher)
    h.start()
    try:
        with pytest.raises(RuntimeError, match="already running"):
            h.start()
    finally:
        h.stop()


def test_stop_is_idempotent(dispatcher: QueueDispatcher) -> None:
    """Multiple ``stop`` calls are safe."""
    h = StandaloneHost(dispatcher)
    h.start()
    h.stop()
    h.stop()  # no-op
    assert not h.is_running


# ---------------------------------------------------------------------------
# Concurrency
# ---------------------------------------------------------------------------


def test_concurrent_posters(dispatcher: QueueDispatcher, host: StandaloneHost) -> None:
    """Multiple poster threads interleave without deadlock or job loss."""
    results: list[int] = []
    results_lock = threading.Lock()

    def _poster(base: int) -> None:
        local: list[Any] = []
        for i in range(25):
            local.append(dispatcher.post(lambda v=base + i: v))
        for h in local:
            value = h.wait(timeout=5.0)
            with results_lock:
                results.append(value)

    threads = [threading.Thread(target=_poster, args=(b * 100,), name=f"poster-{b}") for b in range(8)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=30)
    # 8 threads x 25 posts = 200 results; exact values depend on interleaving
    # but the set of all values posted must be completely received.
    expected = set()
    for base in range(8):
        expected.update(range(base * 100, base * 100 + 25))
    assert set(results) == expected


# ---------------------------------------------------------------------------
# BlockingDispatcher (headless path)
# ---------------------------------------------------------------------------


def test_blocking_dispatcher_round_trip(
    blocking_dispatcher: BlockingDispatcher,
) -> None:
    """BlockingDispatcher works the same way, driven by StandaloneHost's
    blocking fast-path (uses ``tick_blocking`` under the hood).
    """
    with StandaloneHost(blocking_dispatcher):
        got = blocking_dispatcher.post(lambda: "blocking-ok").wait(timeout=2.0)
    assert got == "blocking-ok"


def test_blocking_dispatcher_exposes_tick_blocking(
    blocking_dispatcher: BlockingDispatcher,
) -> None:
    """tick_blocking(timeout_ms) returns an empty outcome when no job arrives."""
    outcome = blocking_dispatcher.tick_blocking(max_jobs=16, timeout_ms=20)
    assert outcome.jobs_executed == 0
    assert outcome.more_pending is False


# ---------------------------------------------------------------------------
# Driver substitution — SRP / LSP sanity
# ---------------------------------------------------------------------------


def test_standalone_host_accepts_either_dispatcher_type() -> None:
    """StandaloneHost is substitutable across QueueDispatcher and
    BlockingDispatcher (DIP: depends on the shared dispatcher contract,
    not on a concrete type).
    """
    for d in (QueueDispatcher(), BlockingDispatcher()):
        with StandaloneHost(d):
            assert d.post(lambda: 1).wait(timeout=2.0) == 1
        assert d.is_shutdown()


def test_standalone_host_rejects_invalid_config() -> None:
    """Constructor validation — SRP: the driver refuses nonsensical inputs
    up front instead of failing opaquely on ``start``.
    """
    with pytest.raises(ValueError, match="tick_interval"):
        StandaloneHost(QueueDispatcher(), tick_interval=0)
    with pytest.raises(ValueError, match="max_jobs_per_tick"):
        StandaloneHost(QueueDispatcher(), max_jobs_per_tick=0)
