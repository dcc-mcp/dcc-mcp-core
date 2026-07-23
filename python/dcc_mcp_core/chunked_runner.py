"""Tick-driven main-affinity work with cooperative cancellation."""

from __future__ import annotations

from functools import wraps
import logging
import time
from typing import Any
from typing import Callable
from typing import Iterable
from typing import Iterator

from dcc_mcp_core.cancellation import CancelledError
from dcc_mcp_core.cancellation import CancelToken
from dcc_mcp_core.cancellation import current_cancel_token

logger = logging.getLogger(__name__)

__all__ = [
    "ChunkedOutcome",
    "ChunkedProgress",
    "ChunkedRunner",
    "ChunkedStep",
    "chunked_job",
]


class ChunkedStep:
    """One bounded unit of host-main work."""

    __slots__ = ("fn", "step")

    def __init__(self, step: int, fn: Callable[[], Any]) -> None:
        self.step = step
        self.fn = fn


class ChunkedProgress:
    """Progress published after an acknowledged step."""

    __slots__ = ("completed", "last_step_at", "message", "total")

    def __init__(
        self,
        completed: int = 0,
        total: int | None = None,
        last_step_at: float = 0.0,
        message: str | None = None,
    ) -> None:
        self.completed = completed
        self.total = total
        self.last_step_at = last_step_at
        self.message = message

    def __repr__(self) -> str:  # pragma: no cover
        return (
            f"ChunkedProgress(completed={self.completed}, total={self.total}, "
            f"last_step_at={self.last_step_at:.3f}, message={self.message!r})"
        )


class ChunkedOutcome:
    """Exactly-once terminal outcome."""

    __slots__ = ("error", "progress", "status")

    def __init__(
        self,
        status: str,
        progress: ChunkedProgress,
        error: str | None = None,
    ) -> None:
        self.status = status
        self.progress = progress
        self.error = error

    def __repr__(self) -> str:  # pragma: no cover
        return f"ChunkedOutcome(status={self.status!r}, progress={self.progress!r}, error={self.error!r})"


class ChunkedRunner:
    """Execute at most one iterator step per :meth:`step` call.

    Call ``step()`` once from each host event-loop tick. Iterables are consumed
    lazily, so generator bodies and yielded callables both remain on the host
    thread. Cancellation is requested immediately but becomes terminal only
    when the next checkpoint observes it.
    """

    def __init__(
        self,
        steps: Iterable[ChunkedStep] | Iterable[Callable[[], Any]],
        *,
        total: int | None = None,
        cancel_token: Any | None = None,
        clock: Callable[[], float] = time.monotonic,
        on_progress: Callable[[ChunkedProgress], None] | None = None,
        on_terminal: Callable[[ChunkedOutcome], None] | None = None,
    ) -> None:
        inferred_total = len(steps) if total is None and hasattr(steps, "__len__") else total
        if inferred_total is not None and inferred_total < 0:
            raise ValueError("total must be >= 0 or None")
        self._steps: Iterator[Any] = iter(steps)
        self._cancel_token = cancel_token if cancel_token is not None else CancelToken()
        self._cancel_requested = False
        self._clock = clock
        self._on_progress = on_progress
        self._on_terminal = on_terminal
        self._outcome: ChunkedOutcome | None = None
        self._progress = ChunkedProgress(total=inferred_total)

    @property
    def progress(self) -> ChunkedProgress:
        """Current progress snapshot."""
        return self._progress

    @property
    def outcome(self) -> ChunkedOutcome | None:
        """Terminal outcome, or ``None`` while active."""
        return self._outcome

    @property
    def is_terminal(self) -> bool:
        """Whether a terminal outcome has been published."""
        return self._outcome is not None

    def step(self) -> bool:
        """Run one bounded step; return whether another tick is required."""
        if self._outcome is not None:
            return False
        if self._is_cancelled():
            self._publish_terminal("cancelled")
            return False

        try:
            item = next(self._steps)
            fn = item.fn if isinstance(item, ChunkedStep) else item
            if not callable(fn):
                raise TypeError(f"Expected ChunkedStep or callable, got {type(item).__name__}")
            message = fn()
        except StopIteration:
            self._publish_terminal("completed")
            return False
        except CancelledError:
            self._publish_terminal("cancelled")
            return False
        except Exception as exc:
            self._publish_terminal("failed", f"{type(exc).__name__}: {exc}")
            return False

        self._progress.completed += 1
        self._progress.last_step_at = self._clock()
        self._progress.message = message if isinstance(message, str) else None
        if self._on_progress is not None:
            try:
                self._on_progress(self._progress)
            except Exception as exc:
                self._publish_terminal("failed", f"{type(exc).__name__}: {exc}")
                return False

        if self._is_cancelled():
            self._publish_terminal("cancelled")
            return False
        if self._progress.total is not None and self._progress.completed >= self._progress.total:
            self._publish_terminal("completed")
            return False
        return True

    def cancel(self) -> None:
        """Request cancellation; the next checkpoint acknowledges it."""
        self._cancel_requested = True
        cancel = getattr(self._cancel_token, "cancel", None)
        if callable(cancel):
            cancel()

    def _is_cancelled(self) -> bool:
        return self._cancel_requested or bool(getattr(self._cancel_token, "cancelled", False))

    def _publish_terminal(self, status: str, error: str | None = None) -> None:
        if self._outcome is not None:
            return
        self._outcome = ChunkedOutcome(
            status,
            ChunkedProgress(
                self._progress.completed,
                self._progress.total,
                self._progress.last_step_at,
                self._progress.message,
            ),
            error,
        )
        if self._on_terminal is not None:
            try:
                self._on_terminal(self._outcome)
            except Exception as exc:  # pragma: no cover - defensive callback isolation
                logger.warning("ChunkedRunner.on_terminal raised: %s", exc)


def chunked_job(
    func: Callable[..., Iterable[Any]] | None = None,
    *,
    total: int | None = None,
) -> Callable[..., Any]:
    """Decorate a generator/iterable factory as a :class:`ChunkedRunner`."""

    def decorate(factory: Callable[..., Iterable[Any]]) -> Callable[..., ChunkedRunner]:
        @wraps(factory)
        def create_runner(*args: Any, **kwargs: Any) -> ChunkedRunner:
            return ChunkedRunner(
                factory(*args, **kwargs),
                total=total,
                cancel_token=current_cancel_token(),
            )

        return create_runner

    return decorate(func) if func is not None else decorate
