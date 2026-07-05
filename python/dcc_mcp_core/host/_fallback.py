"""Pure-Python host dispatcher fallback used when ``dcc_mcp_core._core`` is absent."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
import json
import threading
from typing import Any
from typing import Callable


class DispatchError(RuntimeError):
    """Fallback dispatch error raised by the Python host dispatcher."""


@dataclass(frozen=True)
class TickOutcome:
    """Result of a dispatcher tick."""

    jobs_executed: int = 0
    jobs_panicked: int = 0
    more_pending: bool = False

    @property
    def drained(self) -> int:
        return self.jobs_executed


class PostHandle:
    """Handle for a posted callable."""

    def __init__(self, dispatcher: "QueueDispatcher", job_id: int) -> None:
        self._dispatcher = dispatcher
        self._job_id = job_id
        self._event = threading.Event()
        self._consumed = False
        self._result: Any = None
        self._exc: BaseException | None = None
        self._cancelled = False

    @property
    def cancelled(self) -> bool:
        return self._cancelled

    def _resolve(self, result: Any = None, exc: BaseException | None = None) -> None:
        self._result = result
        self._exc = exc
        self._event.set()

    def _mark_cancelled(self) -> None:
        self._cancelled = True
        self._resolve(exc=DispatchError("shutdown"))

    def wait(self, timeout: float | None = None) -> Any:
        if self._consumed:
            raise RuntimeError("result already consumed")
        if not self._event.wait(timeout):
            raise DispatchError("timeout")
        self._consumed = True
        if self._exc is not None:
            raise self._exc
        return self._result


class QueueDispatcher:
    """Pure-Python dispatcher that runs callables when ticked."""

    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._condition = threading.Condition(self._lock)
        self._queue: deque[tuple[int, Callable[[], Any], PostHandle]] = deque()
        self._next_job_id = 0
        self._shutdown = False

    def post(self, fn: Callable[[], Any]) -> PostHandle:
        if not callable(fn):
            raise TypeError("post(fn) expects a callable")
        with self._condition:
            if self._shutdown:
                raise DispatchError("shutdown")
            job_id = self._next_job_id
            self._next_job_id += 1
            handle = PostHandle(self, job_id)
            self._queue.append((job_id, fn, handle))
            self._condition.notify_all()
            return handle

    def tick(self, max_jobs: int = 16) -> TickOutcome:
        if max_jobs <= 0:
            return TickOutcome()
        jobs: list[tuple[int, Callable[[], Any], PostHandle]] = []
        with self._condition:
            while self._queue and len(jobs) < max_jobs:
                jobs.append(self._queue.popleft())
        executed = 0
        panicked = 0
        for _job_id, fn, handle in jobs:
            if handle.cancelled:
                continue
            try:
                handle._resolve(result=fn())
            except BaseException as exc:  # noqa: BLE001
                panicked += 1
                handle._resolve(exc=exc)
            executed += 1
        with self._condition:
            more_pending = bool(self._queue)
            if executed or panicked:
                self._condition.notify_all()
        return TickOutcome(jobs_executed=executed, jobs_panicked=panicked, more_pending=more_pending)

    def shutdown(self) -> None:
        with self._condition:
            if self._shutdown:
                return
            self._shutdown = True
            pending = list(self._queue)
            self._queue.clear()
            self._condition.notify_all()
        for _job_id, _fn, handle in pending:
            handle._mark_cancelled()

    def is_shutdown(self) -> bool:
        with self._condition:
            return self._shutdown


class BlockingDispatcher(QueueDispatcher):
    """Queue dispatcher with a blocking tick primitive."""

    def tick_blocking(self, max_jobs: int = 16, timeout_ms: int = 50) -> TickOutcome:
        timeout = max(timeout_ms, 0) / 1000.0
        with self._condition:
            if not self._queue and not self._shutdown:
                self._condition.wait(timeout=timeout)
        return self.tick(max_jobs)


def _normalize_object_root(value: Any, *, allow_none: bool, label: str) -> dict[str, Any] | None:
    if value is None:
        return None if allow_none else {}
    if isinstance(value, dict):
        return value
    if isinstance(value, str):
        text = value.strip()
        if not text:
            return None if allow_none else {}
        try:
            decoded = json.loads(text)
        except json.JSONDecodeError as exc:
            raise ValueError(f"{label}-string-not-json") from exc
        if isinstance(decoded, dict):
            return decoded
        raise ValueError(f"{label}-decoded-not-object")
    raise ValueError(f"{label}-not-object")


def normalize_tool_arguments(arguments: Any = None) -> dict[str, Any]:
    """Normalize tool arguments to an object-shaped dict."""
    normalized = _normalize_object_root(arguments, allow_none=False, label="arguments")
    return normalized if normalized is not None else {}


def normalize_tool_meta(meta: Any = None) -> dict[str, Any] | None:
    """Normalize tool ``_meta`` to a dict or ``None``."""
    return _normalize_object_root(meta, allow_none=True, label="arguments")
