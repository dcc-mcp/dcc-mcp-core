"""Shared UI-thread dispatcher for embedded interactive DCC hosts.

Maya, Blender (UI mode), Houdini desktop, Photoshop, etc. all need the same
shape: enqueue callables onto the host main thread, cooperative cancel,
shutdown that unblocks waiters, and dict outcomes compatible with the MCP
HTTP worker. Subclass :class:`HostUiDispatcherBase` and implement
:meth:`~HostUiDispatcherBase.poke_host_pump` only.

For ``mayapy`` / batch / pytest use :class:`InProcessCallableDispatcher`
instead — it runs inline on the calling thread and does not need a pump.

See ``docs/api/dispatcher.md`` (Host UI dispatcher checklist).

Type annotations use ``typing.Optional`` / ``Dict`` / … so the separate cp37
wheel imports cleanly (PEP 604 ``|`` and ``dict[...]`` are invalid on 3.7).
"""

from __future__ import annotations

from collections import deque
import contextvars
import logging
import threading
import time
from typing import Any
from typing import Callable
from typing import Deque
from typing import Dict
from typing import List
from typing import Optional
from typing import Set
from typing import Tuple

from dcc_mcp_core.cancellation import CancelledError
from dcc_mcp_core.cancellation import reset_current_job
from dcc_mcp_core.cancellation import set_current_job

logger = logging.getLogger(__name__)

__all__ = [
    "DEFAULT_UI_JOB_TIMEOUT_MS",
    "DispatcherErrorCode",
    "HostUiDispatcherBase",
    "HostUiJobEntry",
    "current_host_ui_job",
    "host_ui_outcome",
    "normalize_affinity",
]

#: Default soft timeout when callers omit ``timeout_ms`` (milliseconds).
DEFAULT_UI_JOB_TIMEOUT_MS = 30_000


class DispatcherErrorCode:
    """Stable ``error`` string values on dict outcomes (wire-compatible)."""

    CANCELLED = "Cancelled"
    INTERRUPTED = "Interrupted"
    TIMEOUT = "Timeout"
    HOST_BUSY = "host-busy"
    UNSUPPORTED_AFFINITY = "unsupported-affinity"


def normalize_affinity(affinity: str) -> str:
    """Return lower-case affinity or raise ``ValueError``."""
    value = (affinity or "main").lower()
    if value not in ("any", "main"):
        raise ValueError(f"Unsupported affinity '{affinity}'; expected 'any' or 'main'")
    return value


def host_ui_outcome(
    request_id: str,
    affinity: str,
    *,
    success: bool,
    output: Any = None,
    error: Optional[str] = None,
    job_id: Optional[str] = None,
) -> Dict[str, Any]:
    """Build the standard dict envelope returned by UI dispatchers."""
    payload: Dict[str, Any] = {
        "request_id": request_id,
        "affinity": affinity,
        "success": success,
        "output": output,
        "error": error,
    }
    if job_id is not None:
        payload["job_id"] = job_id
    return payload


#: Public ``ContextVar`` pointing at the running :class:`HostUiJobEntry`.
#:
#: Skill scripts and host dispatcher subclasses read this to obtain the
#: current job's progress token / cancellation flag / request id without
#: threading them through every call.  The variable is set by
#: :meth:`HostUiJobEntry.execute` for the duration of one job.
current_host_ui_job: contextvars.ContextVar[Optional[HostUiJobEntry]] = contextvars.ContextVar(
    "dcc_mcp_core_current_host_ui_job",
    default=None,
)


class HostUiJobEntry:
    """Per-submission state for a main-thread (or async) UI job."""

    __slots__ = (
        "affinity",
        "cancel_flag",
        "event",
        "exception_formatter",
        "job_id",
        "on_complete",
        "outcome",
        "progress_token",
        "request_id",
        "task",
        "timeout_ms",
    )

    def __init__(
        self,
        request_id: str,
        affinity: str,
        task: Callable[[], Any],
        timeout_ms: Optional[int] = None,
        *,
        job_id: Optional[str] = None,
        progress_token: Optional[str] = None,
        on_complete: Optional[Callable[[Dict[str, Any]], None]] = None,
        exception_formatter: Optional[Callable[[BaseException], str]] = None,
    ) -> None:
        self.request_id = request_id
        self.affinity = affinity
        self.task = task
        self.timeout_ms = timeout_ms or DEFAULT_UI_JOB_TIMEOUT_MS
        self.event = threading.Event()
        self.outcome: Optional[Dict[str, Any]] = None
        self.cancel_flag = threading.Event()
        self.job_id = job_id
        self.progress_token = progress_token
        self.on_complete = on_complete
        self.exception_formatter = exception_formatter

    def cancel(self) -> None:
        """Signal cooperative cancellation — idempotent."""
        self.cancel_flag.set()

    @property
    def cancelled(self) -> bool:
        return self.cancel_flag.is_set()

    def execute(self) -> Dict[str, Any]:
        """Run ``task`` on the host thread and populate ``outcome``."""
        token_job = set_current_job(self)
        token_ui = current_host_ui_job.set(self)
        try:
            output = self.task()
            self.outcome = host_ui_outcome(
                self.request_id,
                self.affinity,
                success=True,
                output=output,
                job_id=self.job_id,
            )
        except CancelledError:
            self.outcome = host_ui_outcome(
                self.request_id,
                self.affinity,
                success=False,
                error=DispatcherErrorCode.CANCELLED,
                job_id=self.job_id,
            )
        except Exception as exc:
            self.outcome = host_ui_outcome(
                self.request_id,
                self.affinity,
                success=False,
                error=self._format_exception(exc),
                job_id=self.job_id,
            )
        finally:
            reset_current_job(token_job)
            current_host_ui_job.reset(token_ui)
        self.event.set()
        if self.on_complete is not None:
            try:
                self.on_complete(self.outcome)
            except Exception as cb_exc:  # pragma: no cover
                logger.warning("HostUiJobEntry.on_complete raised: %s", cb_exc)
        return self.outcome or host_ui_outcome(
            self.request_id,
            self.affinity,
            success=False,
            error="Job completed but outcome was not set",
            job_id=self.job_id,
        )

    def _format_exception(self, exc: BaseException) -> str:
        formatter = self.exception_formatter
        if formatter is None:
            return str(exc)
        try:
            return formatter(exc)
        except Exception as formatter_exc:  # pragma: no cover - defensive
            logger.warning("HostUiJobEntry.exception_formatter raised: %s", formatter_exc)
            return str(exc)


class HostUiDispatcherBase:
    """Base class for interactive DCC UI-thread dispatchers.

    Subclasses must implement :meth:`poke_host_pump` to nudge the host event
    loop (Maya ``executeDeferred``, Blender ``bpy.app.timers``, …).
    """

    def __init__(
        self,
        *,
        fail_fast_on_main_queue_busy: bool = False,
        label: Optional[str] = None,
    ) -> None:
        self._main_queue: Deque[HostUiJobEntry] = deque()
        self._lock = threading.Lock()
        self._cancelled: Set[str] = set()
        self._active: Dict[str, HostUiJobEntry] = {}
        self._shutdown = False
        self._fail_fast_on_main_queue_busy = fail_fast_on_main_queue_busy
        self._label = label or type(self).__name__

    # ── Host hook ─────────────────────────────────────────────────────────

    def poke_host_pump(self) -> None:
        """Nudge the host to drain :meth:`drain_queue` soon."""
        raise NotImplementedError

    @property
    def dispatcher_label(self) -> str:
        """Human-readable label for adapter logs and diagnostics."""
        return self._label

    def format_exception_error(self, exc: BaseException) -> str:
        """Convert a task exception into the dispatcher error string."""
        return str(exc)

    def format_timeout_error(self, request_id: str, affinity: str, timeout_sec: float) -> str:
        """Convert a sync main-thread timeout into the dispatcher error string."""
        _ = request_id, affinity
        return f"Timeout ({timeout_sec:.1f}s) waiting for main-thread execution"

    def on_job_queued(self, job: HostUiJobEntry) -> None:
        """Observe a job after it is queued for the host pump."""
        _ = job

    def on_job_started(self, job: HostUiJobEntry) -> None:
        """Observe a job immediately before executing on the host thread."""
        _ = job

    def on_job_finished(self, job: HostUiJobEntry) -> None:
        """Observe a job immediately after execution or cancellation."""
        _ = job

    # ── Public API (shared across DCC adapters) ─────────────────────────────

    def submit(
        self,
        action_name: str,
        payload: Optional[str] = None,
        affinity: str = "any",
        timeout_ms: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Submit a static payload job (legacy IPC-style surface)."""

        def _task():
            return payload

        return self.submit_callable(action_name, _task, affinity=affinity, timeout_ms=timeout_ms)

    def submit_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        affinity: str = "main",
        timeout_ms: Optional[int] = None,
    ) -> Dict[str, Any]:
        try:
            affinity_norm = normalize_affinity(affinity)
        except ValueError as exc:
            return host_ui_outcome(
                request_id,
                affinity,
                success=False,
                error=str(exc),
            )
        if affinity_norm == "any":
            return self._run_on_any_thread(request_id, task, affinity_norm)
        return self._submit_main_sync(request_id, task, affinity_norm, timeout_ms)

    def submit_async_callable(
        self,
        request_id: str,
        task: Callable[[], Any],
        *,
        job_id: Optional[str] = None,
        progress_token: Optional[str] = None,
        on_complete: Optional[Callable[[Dict[str, Any]], None]] = None,
        affinity: str = "main",
        timeout_ms: Optional[int] = None,
    ) -> Dict[str, Any]:
        try:
            affinity_norm = normalize_affinity(affinity)
        except ValueError as exc:
            return {
                "request_id": request_id,
                "job_id": job_id,
                "status": "failed",
                "success": False,
                "error": str(exc),
            }

        if self._shutdown:
            return {
                "request_id": request_id,
                "job_id": job_id,
                "status": "interrupted",
                "success": False,
                "error": DispatcherErrorCode.INTERRUPTED,
            }

        if affinity_norm == "any":

            def _bg() -> None:
                result = self.run_on_any_thread(request_id, task, affinity_norm)
                result["job_id"] = job_id
                if on_complete is not None:
                    try:
                        on_complete(result)
                    except Exception as exc:  # pragma: no cover
                        logger.warning("submit_async_callable on_complete raised: %s", exc)

            threading.Thread(target=_bg, daemon=True, name=f"host-ui-async-{request_id}").start()
        else:
            job = HostUiJobEntry(
                request_id,
                affinity_norm,
                task,
                timeout_ms,
                job_id=job_id,
                progress_token=progress_token,
                on_complete=on_complete,
                exception_formatter=self._format_exception_error,
            )
            with self._lock:
                if self._fail_fast_on_main_queue_busy and len(self._main_queue) > 0:
                    return {
                        "request_id": request_id,
                        "job_id": job_id,
                        "status": "failed",
                        "success": False,
                        "error": DispatcherErrorCode.HOST_BUSY,
                    }
                self._main_queue.append(job)
            self._notify_job_queued(job)
            self.poke_host_pump()

        return {
            "request_id": request_id,
            "job_id": job_id,
            "status": "pending",
            "success": True,
            "error": None,
        }

    def cancel(self, request_id: str) -> bool:
        with self._lock:
            self._cancelled.add(request_id)
            for job in self._main_queue:
                if job.request_id == request_id:
                    job.cancel()
                    job.outcome = host_ui_outcome(
                        request_id,
                        job.affinity,
                        success=False,
                        error=DispatcherErrorCode.CANCELLED,
                        job_id=job.job_id,
                    )
                    job.event.set()
                    return True
            active_job = self._active.get(request_id)
            if active_job is not None:
                active_job.cancel()
                return True
        return False

    def pending_count(self) -> int:
        return self.queue_size()

    def queue_size(self) -> int:
        """Return the number of queued main-thread jobs."""
        with self._lock:
            return len(self._main_queue)

    def active_count(self) -> int:
        """Return the number of currently executing main-thread jobs."""
        with self._lock:
            return len(self._active)

    def has_pending(self) -> bool:
        return self.pending_count() > 0

    def shutdown(self, reason: str = DispatcherErrorCode.INTERRUPTED) -> int:
        signalled = 0
        with self._lock:
            self._shutdown = True
            while self._main_queue:
                job = self._main_queue.popleft()
                job.cancel()
                if job.outcome is None:
                    job.outcome = host_ui_outcome(
                        job.request_id,
                        job.affinity,
                        success=False,
                        error=reason,
                        job_id=job.job_id,
                    )
                job.event.set()
                signalled += 1
            for job in list(self._active.values()):
                job.cancel()
                signalled += 1
        if signalled:
            logger.info(
                "%s.shutdown: signalled %d job(s) with reason=%r",
                self.dispatcher_label,
                signalled,
                reason,
            )
        return signalled

    @property
    def is_shutdown(self) -> bool:
        return self._shutdown

    def supported(self) -> List[str]:
        return ["any", "main"]

    def capabilities(self) -> Dict[str, bool]:
        return {
            "supports_main_thread": True,
            "supports_named_threads": False,
            "supports_any_thread": True,
            "supports_time_slicing": True,
        }

    def drain_queue(self, budget_ms: float) -> Tuple[int, int]:
        """Drain the main-thread queue for up to *budget_ms* milliseconds."""
        executed = 0
        start = time.monotonic()
        deadline = start + (budget_ms / 1000.0)

        while time.monotonic() < deadline:
            job = self._dequeue()
            if job is None:
                break

            with self._lock:
                if job.request_id in self._cancelled:
                    self._cancelled.discard(job.request_id)
                    if not job.event.is_set():
                        job.outcome = host_ui_outcome(
                            job.request_id,
                            job.affinity,
                            success=False,
                            error=DispatcherErrorCode.CANCELLED,
                            job_id=job.job_id,
                        )
                        job.event.set()
                    continue
                self._active[job.request_id] = job

            try:
                self._notify_job_started(job)
                job.execute()
            finally:
                with self._lock:
                    self._active.pop(job.request_id, None)
                self._notify_job_finished(job)
            executed += 1

        return executed, len(self._main_queue)

    @staticmethod
    def run_on_any_thread(request_id: str, task: Callable[[], Any], affinity: str) -> Dict[str, Any]:
        try:
            return host_ui_outcome(request_id, affinity, success=True, output=task())
        except Exception as exc:
            return host_ui_outcome(request_id, affinity, success=False, error=str(exc))

    def _run_on_any_thread(self, request_id: str, task: Callable[[], Any], affinity: str) -> Dict[str, Any]:
        try:
            return host_ui_outcome(request_id, affinity, success=True, output=task())
        except Exception as exc:
            return host_ui_outcome(
                request_id,
                affinity,
                success=False,
                error=self._format_exception_error(exc),
            )

    # ── Internal ────────────────────────────────────────────────────────────

    def _submit_main_sync(
        self,
        request_id: str,
        task: Callable[[], Any],
        affinity: str,
        timeout_ms: Optional[int],
    ) -> Dict[str, Any]:
        with self._lock:
            if self._shutdown:
                return host_ui_outcome(
                    request_id,
                    affinity,
                    success=False,
                    error=DispatcherErrorCode.INTERRUPTED,
                )
            if self._fail_fast_on_main_queue_busy and len(self._main_queue) > 0:
                return host_ui_outcome(
                    request_id,
                    affinity,
                    success=False,
                    error=DispatcherErrorCode.HOST_BUSY,
                )
            job = HostUiJobEntry(
                request_id,
                affinity,
                task,
                timeout_ms,
                exception_formatter=self._format_exception_error,
            )
            self._main_queue.append(job)

        self._notify_job_queued(job)
        self.poke_host_pump()

        timeout_sec = (timeout_ms or DEFAULT_UI_JOB_TIMEOUT_MS) / 1000.0
        if not job.event.wait(timeout=timeout_sec):
            return host_ui_outcome(
                request_id,
                affinity,
                success=False,
                error=self._format_timeout_error(request_id, affinity, timeout_sec),
            )
        return job.outcome or host_ui_outcome(
            request_id,
            affinity,
            success=False,
            error="Job completed but outcome was not set",
        )

    def _dequeue(self) -> Optional[HostUiJobEntry]:
        with self._lock:
            if self._main_queue:
                return self._main_queue.popleft()
        return None

    def _format_exception_error(self, exc: BaseException) -> str:
        try:
            return self.format_exception_error(exc)
        except Exception as formatter_exc:  # pragma: no cover - defensive
            logger.warning(
                "%s.format_exception_error raised: %s",
                self.dispatcher_label,
                formatter_exc,
            )
            return str(exc)

    def _format_timeout_error(self, request_id: str, affinity: str, timeout_sec: float) -> str:
        try:
            return self.format_timeout_error(request_id, affinity, timeout_sec)
        except Exception as formatter_exc:  # pragma: no cover - defensive
            logger.warning(
                "%s.format_timeout_error raised: %s",
                self.dispatcher_label,
                formatter_exc,
            )
            return f"Timeout ({timeout_sec:.1f}s) waiting for main-thread execution"

    def _notify_job_queued(self, job: HostUiJobEntry) -> None:
        try:
            self.on_job_queued(job)
        except Exception as hook_exc:  # pragma: no cover - defensive
            logger.warning("%s.on_job_queued raised: %s", self.dispatcher_label, hook_exc)

    def _notify_job_started(self, job: HostUiJobEntry) -> None:
        try:
            self.on_job_started(job)
        except Exception as hook_exc:  # pragma: no cover - defensive
            logger.warning("%s.on_job_started raised: %s", self.dispatcher_label, hook_exc)

    def _notify_job_finished(self, job: HostUiJobEntry) -> None:
        try:
            self.on_job_finished(job)
        except Exception as hook_exc:  # pragma: no cover - defensive
            logger.warning("%s.on_job_finished raised: %s", self.dispatcher_label, hook_exc)
