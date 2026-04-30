"""Callable-payload dispatch protocols for embedded DCCs (issue #520).

EPIC #495 shipped `PyPumpedDispatcher` / `PyStandaloneDispatcher` for
*string-payload* (IPC-style) dispatch. The Python in-process executor
path in `dcc-mcp-maya` (and the upcoming Unreal / Houdini plugins)
needs *callable-payload* dispatch — submitting a zero-arg
``Callable[[], Any]`` to be run on the host's UI thread.

This module ships:

* :class:`BaseDccCallableDispatcher` — full submit / cancel / shutdown
  protocol, the contract every host dispatcher must satisfy.
* :class:`BaseDccPump` — protocol for the cooperative idle-tick that
  drains the queue (Maya ``cmds.scriptJob(event=['idle', pump])``,
  Unreal ``FTickerDelegate`` …).
* :class:`JobEntry` — slotted dataclass used by reference dispatchers
  to track per-submission state.
* :class:`InProcessCallableDispatcher` — reference single-thread impl
  suitable for ``mayapy``, headless Houdini, pytest. Production DCC
  dispatchers (Maya UI thread …) compose or subclass this.

The :class:`BaseDccCallableDispatcher` exported from
``dcc_mcp_core._server.inprocess_executor`` (#521) is a *subset* of
the protocol here — kept intentionally narrow so simple use sites can
still satisfy it with a single ``dispatch_callable`` method. Hosts
that need the full submit/cancel/shutdown surface should implement
this protocol instead.
"""

# Import built-in modules
from __future__ import annotations

import contextvars
from dataclasses import dataclass
from dataclasses import field
import logging
import sys
import threading
import time
from typing import TYPE_CHECKING
from typing import Any
from typing import Callable
import uuid

if TYPE_CHECKING:
    pass

# `typing.Protocol`, `typing.runtime_checkable` and `typing.Literal` are
# 3.8+. The package still claims `requires-python = ">=3.7"`, so on 3.7
# we expose `BaseDccCallableDispatcherFull` / `BaseDccPump` as plain
# duck-typed classes with the same attribute contracts; concrete
# dispatchers do not need to inherit from them either way.
if sys.version_info >= (3, 8):
    from typing import Literal
    from typing import Protocol
    from typing import runtime_checkable
else:  # pragma: no cover - py3.7 only

    def runtime_checkable(cls):
        return cls

    class Protocol:  # type: ignore[no-redef]
        pass

    class _LiteralFallback:
        def __getitem__(self, _item):
            return str

    Literal = _LiteralFallback()  # type: ignore[assignment,misc]

logger = logging.getLogger(__name__)

__all__ = [
    "AdaptivePumpPolicy",
    "AdaptivePumpStats",
    "Affinity",
    "BaseDccCallableDispatcherFull",
    "BaseDccPump",
    "DrainStats",
    "InProcessCallableDispatcher",
    "JobEntry",
    "JobOutcome",
    "PendingEnvelope",
    "PumpStats",
    "current_callable_job",
]


Affinity = Literal["main", "any"]


@dataclass
class JobOutcome:
    """Synchronous-submit result envelope."""

    request_id: str
    ok: bool
    value: Any = None
    error: str | None = None
    elapsed_ms: float = 0.0


@dataclass
class PendingEnvelope:
    """Async-submit result envelope (job is still in-flight)."""

    request_id: str
    job_id: str
    progress_token: str | None = None


@dataclass
class DrainStats:
    """Counters returned by a single :meth:`BaseDccPump.drain_queue` call."""

    drained: int = 0
    elapsed_ms: float = 0.0
    overrun: bool = False


@dataclass
class PumpStats:
    """Cumulative counters exposed by :attr:`BaseDccPump.stats`."""

    ticks: int = 0
    drained: int = 0
    overrun_cycles: int = 0


@dataclass
class AdaptivePumpStats:
    """Cumulative counters for :class:`AdaptivePumpPolicy`."""

    ticks: int = 0
    drained_jobs: int = 0
    overrun_cycles: int = 0
    active_transitions: int = 0
    idle_transitions: int = 0
    mode: str = "active"
    last_interval_secs: float = 0.0


class AdaptivePumpPolicy:
    """Reusable active/idle timing policy for embedded DCC pump callbacks.

    Host adapters still own the actual timer primitive (Maya ``scriptJob``,
    Blender ``bpy.app.timers``, Photoshop event-loop hook, etc.). This policy
    only decides the next interval and records shared counters.
    """

    def __init__(
        self,
        active_interval_secs: float = 0.05,
        idle_interval_secs: float = 1.0,
        idle_delay_secs: float = 5.0,
        max_client_idle_secs: float | None = 10.0,
        *,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        if active_interval_secs <= 0:
            raise ValueError("active_interval_secs must be > 0")
        if idle_interval_secs <= 0:
            raise ValueError("idle_interval_secs must be > 0")
        if idle_delay_secs < 0:
            raise ValueError("idle_delay_secs must be >= 0")
        if max_client_idle_secs is not None and max_client_idle_secs < 0:
            raise ValueError("max_client_idle_secs must be >= 0 or None")

        self.active_interval_secs = active_interval_secs
        self.idle_interval_secs = idle_interval_secs
        self.idle_delay_secs = idle_delay_secs
        self.max_client_idle_secs = max_client_idle_secs
        self._clock = clock
        now = clock()
        self._last_work_at = now
        self._last_client_activity_at = now
        self._mode = "active"
        self._stats = AdaptivePumpStats(mode=self._mode, last_interval_secs=active_interval_secs)

    @property
    def stats(self) -> AdaptivePumpStats:
        """Return cumulative pump-policy counters."""
        return self._stats

    @property
    def mode(self) -> str:
        """Current policy mode: ``"active"`` or ``"idle"``."""
        return self._mode

    def mark_client_activity(self) -> None:
        """Record that an MCP client or adapter submitted work recently."""
        self._last_client_activity_at = self._clock()

    def mark_work_done(
        self,
        drained: int = 1,
        *,
        elapsed_ms: float = 0.0,
        overrun: bool = False,
    ) -> None:
        """Record completed pump work and keep the policy in active mode."""
        self.record_tick(drained=drained, elapsed_ms=elapsed_ms, overrun=overrun)

    def record_tick(
        self,
        drained: int = 0,
        *,
        elapsed_ms: float = 0.0,
        overrun: bool = False,
    ) -> None:
        """Record one host pump tick.

        Args:
            drained: Number of jobs/callables drained by this tick.
            elapsed_ms: Tick duration for adapter metrics. Currently retained
                only through the overrun flag so adapters can compute their own
                host-specific timings.
            overrun: Whether the tick exceeded the adapter's budget.

        """
        if drained < 0:
            raise ValueError("drained must be >= 0")
        self._stats.ticks += 1
        self._stats.drained_jobs += drained
        if overrun:
            self._stats.overrun_cycles += 1
        if drained > 0:
            self._last_work_at = self._clock()
        _ = elapsed_ms

    def next_interval(
        self,
        *,
        has_pending: bool = False,
        deferred_pending: bool = False,
    ) -> float:
        """Return the next host timer interval in seconds.

        ``has_pending`` represents queued dispatcher work; ``deferred_pending``
        represents host-native operations that are still waiting for completion.
        Either one keeps the pump active.
        """
        now = self._clock()
        client_recent = (
            self.max_client_idle_secs is None or now - self._last_client_activity_at <= self.max_client_idle_secs
        )
        should_stay_active = (
            has_pending or deferred_pending or (client_recent and now - self._last_work_at < self.idle_delay_secs)
        )
        mode = "active" if should_stay_active else "idle"
        if mode != self._mode:
            if mode == "active":
                self._stats.active_transitions += 1
            else:
                self._stats.idle_transitions += 1
            self._mode = mode
            self._stats.mode = mode

        interval = self.active_interval_secs if mode == "active" else self.idle_interval_secs
        self._stats.last_interval_secs = interval
        return interval


@dataclass
class JobEntry:
    """Per-submission bookkeeping; mirrors Maya's `_JobEntry`."""

    request_id: str
    task: Callable[[], Any]
    timeout_ms: int | None = None
    on_complete: Callable[[JobOutcome], None] | None = None
    cancel_flag: bool = False
    outcome: JobOutcome | None = None
    submitted_at: float = field(default_factory=time.monotonic)
    _done_event: threading.Event = field(default_factory=threading.Event, repr=False)

    @property
    def cancelled(self) -> bool:
        return self.cancel_flag

    def wait(self, timeout: float | None = None) -> bool:
        return self._done_event.wait(timeout)

    def signal_done(self) -> None:
        self._done_event.set()


# Per-job ContextVar — lets cooperative-cancel probes reach the active
# job from inside skill scripts without a request context.
current_callable_job: contextvars.ContextVar[JobEntry | None] = contextvars.ContextVar(
    "dcc_mcp_core_current_callable_job",
    default=None,
)


@runtime_checkable
class BaseDccCallableDispatcherFull(Protocol):
    """Full submit / cancel / shutdown contract for callable dispatch."""

    def submit_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        affinity: Affinity = "main",
        timeout_ms: int | None = None,
    ) -> JobOutcome: ...

    def submit_async_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        *,
        affinity: Affinity = "main",
        timeout_ms: int | None = None,
        progress_token: str | None = None,
        on_complete: Callable[[JobOutcome], None] | None = None,
    ) -> PendingEnvelope: ...

    def cancel(self, request_id: str) -> bool: ...

    def shutdown(self, reason: str = "Interrupted") -> int: ...


@runtime_checkable
class BaseDccPump(Protocol):
    """Cooperative idle-tick contract for hosts that drain the queue."""

    def drain_queue(self, budget_ms: int) -> DrainStats: ...

    @property
    def stats(self) -> PumpStats: ...


class InProcessCallableDispatcher:
    """Reference single-thread dispatcher for ``mayapy`` / headless / tests.

    Production DCC dispatchers (Maya UI thread …) compose this class or
    subclass and override ``submit_callable`` to push the job onto
    their host's main-thread queue instead of running it inline.
    """

    def __init__(self) -> None:
        self._jobs: dict[str, JobEntry] = {}
        self._jobs_lock = threading.Lock()
        self._shutdown = False

    def submit_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        affinity: Affinity = "main",
        timeout_ms: int | None = None,
    ) -> JobOutcome:
        if self._shutdown:
            return JobOutcome(request_id=request_id, ok=False, error="dispatcher is shut down")
        entry = JobEntry(request_id=request_id, task=task, timeout_ms=timeout_ms)
        with self._jobs_lock:
            self._jobs[request_id] = entry
        try:
            return self._run_entry(entry)
        finally:
            with self._jobs_lock:
                self._jobs.pop(request_id, None)

    def submit_async_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        *,
        affinity: Affinity = "main",
        timeout_ms: int | None = None,
        progress_token: str | None = None,
        on_complete: Callable[[JobOutcome], None] | None = None,
    ) -> PendingEnvelope:
        job_id = uuid.uuid4().hex
        entry = JobEntry(
            request_id=request_id,
            task=task,
            timeout_ms=timeout_ms,
            on_complete=on_complete,
        )
        with self._jobs_lock:
            self._jobs[request_id] = entry

        def _runner() -> None:
            try:
                outcome = self._run_entry(entry)
            finally:
                with self._jobs_lock:
                    self._jobs.pop(request_id, None)
            if on_complete is not None:
                try:
                    on_complete(outcome)
                except Exception as exc:
                    logger.warning("on_complete callback raised: %s", exc)

        threading.Thread(target=_runner, name=f"InProcessJob-{request_id}", daemon=True).start()
        return PendingEnvelope(request_id=request_id, job_id=job_id, progress_token=progress_token)

    def cancel(self, request_id: str) -> bool:
        with self._jobs_lock:
            entry = self._jobs.get(request_id)
        if entry is None:
            return False
        entry.cancel_flag = True
        entry.signal_done()
        return True

    def shutdown(self, reason: str = "Interrupted") -> int:
        self._shutdown = True
        with self._jobs_lock:
            cancelled = list(self._jobs.values())
        for entry in cancelled:
            entry.cancel_flag = True
            entry.signal_done()
        return len(cancelled)

    def _run_entry(self, entry: JobEntry) -> JobOutcome:
        token = current_callable_job.set(entry)
        started = time.monotonic()
        try:
            if entry.cancel_flag:
                outcome = JobOutcome(
                    request_id=entry.request_id,
                    ok=False,
                    error="cancelled before start",
                )
            else:
                value = entry.task()
                outcome = JobOutcome(
                    request_id=entry.request_id,
                    ok=True,
                    value=value,
                )
        except Exception as exc:
            outcome = JobOutcome(
                request_id=entry.request_id,
                ok=False,
                error=f"{type(exc).__name__}: {exc}",
            )
        finally:
            current_callable_job.reset(token)
            entry.outcome = outcome
            entry.signal_done()
        outcome.elapsed_ms = (time.monotonic() - started) * 1000.0
        return outcome
