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
