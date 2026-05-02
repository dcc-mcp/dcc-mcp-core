"""Contract tests for :class:`dcc_mcp_core.host.HostAdapter`.

Verify the lifecycle + hook contract without any DCC dependency. A
minimal in-memory subclass (:class:`_TimerHost`) wires the tick
callback to a Python :class:`threading.Timer` so the test proves the
base class machinery actually drives a dispatcher end-to-end.

External DCC adapter repos (``dcc-mcp-blender``, ``dcc-mcp-maya``, …)
are encouraged to copy
:func:`test_subclass_overriding_hooks_drives_dispatcher` into their
own test suite as a contract gate — if your subclass passes it, it
integrates correctly with :class:`HostAdapter`.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import threading
import time

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core.host import BlockingDispatcher
from dcc_mcp_core.host import HostAdapter
from dcc_mcp_core.host import QueueDispatcher

# ── Fakes ────────────────────────────────────────────────────────────


class _TimerHost(HostAdapter):
    """Minimal subclass that stands in for a real DCC's idle primitive
    with a Python :class:`threading.Timer`.

    Proves the base class orchestrates the 3 hooks correctly. Never
    uses any DCC API, so runs in the regular pytest matrix.
    """

    def __init__(self, dispatcher, *, background: bool = False, **kw):
        super().__init__(dispatcher, **kw)
        self._background = background
        self._timer: threading.Timer | None = None
        self._tick_fn = None

    def is_background(self) -> bool:
        return self._background

    def attach_tick(self, tick_fn) -> None:
        self._tick_fn = tick_fn
        self._schedule_next(0.0)

    def detach_tick(self) -> None:
        if self._timer is not None:
            self._timer.cancel()
            self._timer = None
        self._tick_fn = None

    def _schedule_next(self, interval: float) -> None:
        # Tight-loop fake: when tick_fn returns None, stop.
        def _fire():
            fn = self._tick_fn
            if fn is None:
                return
            next_interval = fn()
            if next_interval is None:
                return
            self._schedule_next(max(float(next_interval), 0.001))

        t = threading.Timer(interval, _fire)
        t.daemon = True
        t.start()
        self._timer = t


class _BareHost(HostAdapter):
    """Does nothing beyond what the base provides — used to prove the
    base rejects missing hooks.
    """

    # Inherits the base `is_background()` (returns True by default so a
    # bare instance would try run_headless; override to False so we can
    # exercise the interactive path and prove attach_tick raises).
    def is_background(self) -> bool:
        return False


# ── Validation ───────────────────────────────────────────────────────


def test_init_rejects_invalid_tick_interval_active() -> None:
    with pytest.raises(ValueError, match="tick_interval_active"):
        _TimerHost(QueueDispatcher(), tick_interval_active=-0.1)


def test_init_rejects_invalid_tick_interval_idle() -> None:
    with pytest.raises(ValueError, match="tick_interval_idle"):
        _TimerHost(QueueDispatcher(), tick_interval_idle=0)


def test_init_rejects_invalid_max_jobs() -> None:
    with pytest.raises(ValueError, match="max_jobs_per_tick"):
        _TimerHost(QueueDispatcher(), max_jobs_per_tick=0)


# ── Hook contract ────────────────────────────────────────────────────


def test_attach_tick_not_overridden_raises() -> None:
    """Bare HostAdapter + interactive mode → attach_tick raises the
    documented NotImplementedError on start().
    """
    host = _BareHost(QueueDispatcher())
    with pytest.raises(NotImplementedError, match="attach_tick"):
        host.start()


def test_is_background_default_is_true() -> None:
    """Safe default — subclasses that forget to override still work
    via the headless path, rather than trying to attach a timer that
    will never fire.
    """

    class _NoOverride(HostAdapter):
        # Minimal overrides only — we want the base default for
        # is_background but can't let run_headless actually spin, so
        # provide cheap attach/detach no-ops that will never be used
        # (because is_background() is True → run_headless() path).
        def attach_tick(self, tick_fn) -> None:
            raise AssertionError("attach_tick must not run in background mode")

        def detach_tick(self) -> None:
            pass

    host = _NoOverride(QueueDispatcher())
    assert host.is_background() is True


# ── LSP: substitutability across dispatcher types ────────────────────


def test_dispatcher_substitutability_queue() -> None:
    """LSP: a QueueDispatcher works end-to-end via _TimerHost."""
    dispatcher = QueueDispatcher()
    host = _TimerHost(dispatcher, tick_interval_idle=0.01)
    with host:
        result = dispatcher.post(lambda: "queue").wait(timeout=2.0)
    assert result == "queue"


def test_dispatcher_substitutability_blocking() -> None:
    """LSP: a BlockingDispatcher also works — proves HostAdapter
    depends on the TickableDispatcher protocol, not the concrete
    class.
    """
    dispatcher = BlockingDispatcher()
    host = _TimerHost(dispatcher, tick_interval_idle=0.01)
    with host:
        result = dispatcher.post(lambda: "blocking").wait(timeout=2.0)
    assert result == "blocking"


# ── End-to-end round-trip through the base class ─────────────────────


def test_subclass_overriding_hooks_drives_dispatcher() -> None:
    """The canonical contract test — copy this into your downstream
    DCC repo with ``_TimerHost`` replaced by your real adapter.

    If it passes, your subclass correctly integrates with
    :class:`HostAdapter`'s lifecycle.
    """
    dispatcher = QueueDispatcher()
    host = _TimerHost(dispatcher, tick_interval_idle=0.01)
    assert not host.is_running
    host.start()
    try:
        assert host.is_running
        # Round-trip: a post -> wait actually resolves.
        result = dispatcher.post(lambda: 1 + 1).wait(timeout=2.0)
        assert result == 2
    finally:
        host.stop()
    assert not host.is_running


# ── Lifecycle ────────────────────────────────────────────────────────


def test_context_manager() -> None:
    dispatcher = QueueDispatcher()
    with _TimerHost(dispatcher, tick_interval_idle=0.01) as host:
        assert host.is_running
        assert dispatcher.post(lambda: 7).wait(timeout=2.0) == 7
    assert not host.is_running
    # Dispatcher is shut down on exit.
    assert dispatcher.is_shutdown()


def test_start_twice_raises() -> None:
    host = _TimerHost(QueueDispatcher(), tick_interval_idle=0.01)
    host.start()
    try:
        with pytest.raises(RuntimeError, match="already running"):
            host.start()
    finally:
        host.stop()


def test_stop_idempotent() -> None:
    host = _TimerHost(QueueDispatcher(), tick_interval_idle=0.01)
    host.start()
    host.stop()
    host.stop()  # must not raise


def test_run_headless_exits_on_stop_event() -> None:
    """Background-mode loop terminates promptly when the external
    stop_event is set.
    """
    dispatcher = QueueDispatcher()
    host = _TimerHost(dispatcher, background=True, tick_interval_idle=0.02)
    stop_event = threading.Event()

    t = threading.Thread(target=host.run_headless, args=(stop_event,), daemon=True)
    t.start()
    # Let the loop spin once.
    time.sleep(0.05)
    stop_event.set()
    t.join(timeout=2.0)
    assert not t.is_alive()


def test_background_start_runs_headless_thread() -> None:
    """In background mode, ``start()`` spins a daemon thread instead
    of calling attach_tick — which would raise here since our fake
    is configured as background.
    """
    dispatcher = QueueDispatcher()
    host = _TimerHost(dispatcher, background=True, tick_interval_idle=0.01)
    host.start()
    try:
        assert host.is_running
        result = dispatcher.post(lambda: "headless").wait(timeout=2.0)
        assert result == "headless"
    finally:
        host.stop()
    assert not host.is_running
