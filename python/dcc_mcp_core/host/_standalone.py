"""StandaloneHost — drive a dispatcher from a dedicated Python thread.

This is the escape hatch for environments without a real DCC main loop:
tests, CI jobs, CLI automation, and any plain Python process that wants to
exercise the dispatcher API end-to-end.

Responsibility (SRP): :class:`StandaloneHost` only *drives* the tick loop.
It does not own the dispatcher (callers keep that reference for posting
jobs) and it does not implement the dispatcher contract itself. Real DCC
adapters replace this class with a thin wrapper over their native idle
primitive (e.g. ``bpy.app.timers.register``) — that substitution is
friction-free because every adapter satisfies the same dispatcher contract
(LSP/OCP).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import contextlib
import threading
from typing import TYPE_CHECKING
from typing import Protocol
from typing import runtime_checkable

if TYPE_CHECKING:
    # Import local modules
    from dcc_mcp_core._core import BlockingDispatcher
    from dcc_mcp_core._core import QueueDispatcher
    from dcc_mcp_core._core import TickOutcome


@runtime_checkable
class _TickableDispatcher(Protocol):
    """Minimum surface :class:`StandaloneHost` needs from its dispatcher.

    Keeps the class independent of the concrete dispatcher type (DIP)
    and lets callers swap in test doubles without subclassing.
    """

    def tick(self, max_jobs: int = ...) -> TickOutcome: ...
    def has_pending(self) -> bool: ...
    def pending(self) -> int: ...
    def shutdown(self) -> None: ...
    def is_shutdown(self) -> bool: ...


class StandaloneHost:
    """Background driver that ticks a dispatcher on a dedicated thread.

    :param dispatcher: a :class:`~dcc_mcp_core._core.QueueDispatcher` or
        :class:`~dcc_mcp_core._core.BlockingDispatcher`.
    :param tick_interval: seconds between ``tick()`` calls on a non-blocking
        dispatcher. Ignored when ``dispatcher`` exposes ``tick_blocking``
        (the blocking path self-paces via its own timeout).
    :param max_jobs_per_tick: fairness cap passed into each ``tick`` call.
    :param thread_name: debug name for the driver thread.

    Typical usage as a context manager::

        with StandaloneHost(dispatcher):
            handle = dispatcher.post(lambda: ...)
            result = handle.wait(timeout=5.0)

    ``start()`` / ``stop()`` are also available for non-context-manager
    lifecycles (Blender addons, long-lived daemons).
    """

    # Sentinel for the BlockingDispatcher's tick_blocking timeout_ms argument.
    _BLOCKING_TIMEOUT_MS = 50

    def __init__(
        self,
        dispatcher: QueueDispatcher | BlockingDispatcher | _TickableDispatcher,
        *,
        tick_interval: float = 0.01,
        max_jobs_per_tick: int = 16,
        thread_name: str = "dcc-mcp-host-standalone",
    ) -> None:
        if tick_interval <= 0:
            raise ValueError(f"tick_interval must be > 0, got {tick_interval!r}")
        if max_jobs_per_tick <= 0:
            raise ValueError(f"max_jobs_per_tick must be > 0, got {max_jobs_per_tick!r}")
        self._dispatcher = dispatcher
        self._tick_interval = float(tick_interval)
        self._max_jobs = int(max_jobs_per_tick)
        self._thread_name = thread_name
        self._stop_event = threading.Event()
        self._thread: threading.Thread | None = None
        # Cached capability flag: BlockingDispatcher offers tick_blocking.
        self._use_blocking = hasattr(dispatcher, "tick_blocking")

    # ── Lifecycle ──────────────────────────────────────────────────────

    def start(self) -> None:
        """Spawn the driver thread.

        Raises :class:`RuntimeError` if already running.
        """
        if self._thread is not None and self._thread.is_alive():
            raise RuntimeError("StandaloneHost is already running")
        self._stop_event.clear()
        t = threading.Thread(
            target=self._run,
            name=self._thread_name,
            daemon=True,
        )
        t.start()
        self._thread = t

    def stop(self, timeout: float = 5.0) -> None:
        """Stop the driver thread and shut down the dispatcher.

        Idempotent — safe to call multiple times or after a failed ``start``.
        ``timeout`` is the upper bound on how long to wait for the driver
        thread to exit; tick loops return within ``tick_interval`` (or
        ``_BLOCKING_TIMEOUT_MS``) so the default 5 s is usually ample.
        """
        self._stop_event.set()
        # Swallow shutdown failures during teardown so a stale dispatcher
        # can't mask cleanup on the way out.
        with contextlib.suppress(Exception):
            self._dispatcher.shutdown()
        if self._thread is not None:
            self._thread.join(timeout=timeout)
            if self._thread.is_alive():  # pragma: no cover - diagnostic only
                raise RuntimeError(f"StandaloneHost thread {self._thread_name!r} did not stop within {timeout}s")
            self._thread = None

    @property
    def is_running(self) -> bool:
        """``True`` while the driver thread is alive."""
        return self._thread is not None and self._thread.is_alive()

    # ── Context-manager sugar ─────────────────────────────────────────

    def __enter__(self) -> StandaloneHost:
        self.start()
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        self.stop()

    # ── Internals ──────────────────────────────────────────────────────

    def _run(self) -> None:
        """Drive the dispatcher until ``stop()`` is called."""
        if self._use_blocking:
            self._run_blocking()
        else:
            self._run_polling()

    def _run_blocking(self) -> None:
        """Drive the BlockingDispatcher via ``tick_blocking``.

        Self-paced — each iteration blocks up to ``_BLOCKING_TIMEOUT_MS``
        waiting for work, so there is no explicit sleep.
        """
        dispatcher = self._dispatcher
        max_jobs = self._max_jobs
        timeout_ms = self._BLOCKING_TIMEOUT_MS
        while not self._stop_event.is_set():
            # mypy: BlockingDispatcher.tick_blocking exists by
            # virtue of the `_use_blocking` feature flag we set in __init__.
            dispatcher.tick_blocking(max_jobs, timeout_ms)  # type: ignore[attr-defined]

    def _run_polling(self) -> None:
        """Fallback loop for ``QueueDispatcher`` — poll + sleep."""
        dispatcher = self._dispatcher
        max_jobs = self._max_jobs
        interval = self._tick_interval
        while not self._stop_event.is_set():
            outcome = dispatcher.tick(max_jobs)
            if outcome.more_pending:
                # Hot queue — don't sleep, keep ticking.
                continue
            # Cold queue — short sleep. `Event.wait` returns early if
            # `stop` is called, giving tear-down sub-interval latency.
            self._stop_event.wait(interval)
