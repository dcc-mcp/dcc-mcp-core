"""Chunked main-affinity runner with cooperative cancellation.

A ``ChunkedRunner`` drains a sequence of bounded work steps one at a time
per host event-loop tick. Each step runs on the calling (main) thread —
no worker threads are spawned for DCC API calls. A shared cancellation
token is checked before every step, and the runner publishes a terminal
outcome exactly once (completed / failed / cancelled).

Intended integration::

    runner = ChunkedRunner(steps, cancel_token=token)
    # In the host pump tick callback:
    has_more = runner.step()
    if not has_more:
        outcome = runner.outcome  # terminal state published exactly once
"""

from __future__ import annotations

# Import built-in modules
import time
from typing import TYPE_CHECKING
from typing import Any
from typing import Callable
from typing import Sequence

if TYPE_CHECKING:
    pass

from dcc_mcp_core.cancellation import CancelToken
from dcc_mcp_core.cancellation import CancelledError

__all__ = [
    "ChunkedOutcome",
    "ChunkedProgress",
    "ChunkedRunner",
    "ChunkedStep",
]


class ChunkedStep:
    """One bounded unit of work executed on the main/affinity thread.

    Attributes:
        step: Zero-based step index (set by the runner).
        fn: A zero-argument callable that performs one bounded unit of
            work (e.g. process a single mesh, bake one frame). Must
            return quickly — a typical budget is 8-16 ms per step.
    """

    __slots__ = ("step", "fn")

    def __init__(self, step: int, fn: Callable[[], Any]) -> None:
        self.step = step
        self.fn = fn


class ChunkedProgress:
    """Monotonic progress snapshot published after each confirmed step.

    Attributes:
        completed: Number of steps that have finished successfully.
        total: Total number of steps (may be ``None`` for unbounded
            sequences).
        last_step_at: ``time.monotonic`` timestamp of the most recent
            completed step.
    """

    __slots__ = ("completed", "total", "last_step_at")

    def __init__(
        self,
        completed: int = 0,
        total: int | None = None,
        last_step_at: float = 0.0,
    ) -> None:
        self.completed = completed
        self.total = total
        self.last_step_at = last_step_at

    def __repr__(self) -> str:  # pragma: no cover
        return (
            f"ChunkedProgress(completed={self.completed}, "
            f"total={self.total}, last_step_at={self.last_step_at:.3f})"
        )


class ChunkedOutcome:
    """Terminal outcome published exactly once when the runner finishes.

    Attributes:
        status: One of ``"completed"``, ``"failed"``, or ``"cancelled"``.
        progress: The final progress snapshot.
        error: Human-readable error message when ``status`` is
            ``"failed"``, ``None`` otherwise.
    """

    __slots__ = ("status", "progress", "error")

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
        return (
            f"ChunkedOutcome(status={self.status!r}, "
            f"progress={self.progress!r}, error={self.error!r})"
        )


class ChunkedRunner:
    """Drain a sequence of bounded work steps one tick at a time.

    Each call to :meth:`step` executes one chunk on the calling thread,
    checks the optional :class:`~dcc_mcp_core.cancellation.CancelToken`
    before executing, updates the monotonic progress counter, and returns
    ``True`` while more work remains. Once the sequence is exhausted or a
    terminal condition is reached (failure / cancellation), the runner
    publishes a :class:`ChunkedOutcome` exactly once and :meth:`step`
    becomes a no-op.

    Typical usage inside a host pump tick callback::

        def _on_tick() -> float | None:
            has_more = runner.step()
            return 0.05 if has_more else None
    """

    def __init__(
        self,
        steps: Sequence[ChunkedStep] | Sequence[Callable[[], Any]],
        *,
        cancel_token: CancelToken | None = None,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        self._steps: list[ChunkedStep] = []
        for i, item in enumerate(steps):
            if isinstance(item, ChunkedStep):
                step = item
                step.step = i
                self._steps.append(step)
            elif callable(item):
                self._steps.append(ChunkedStep(i, item))
            else:
                raise TypeError(
                    f"Expected ChunkedStep or callable, got {type(item).__name__}"
                )
        self._cancel_token = cancel_token
        self._clock = clock
        self._index: int = 0
        self._outcome: ChunkedOutcome | None = None
        self._progress = ChunkedProgress(
            completed=0,
            total=len(self._steps) if self._steps else None,
            last_step_at=0.0,
        )
        self._terminal_published: bool = False

    # -- Public properties ---------------------------------------------------

    @property
    def progress(self) -> ChunkedProgress:
        """Current progress snapshot."""
        return self._progress

    @property
    def outcome(self) -> ChunkedOutcome | None:
        """Terminal outcome, or ``None`` while the runner is still active."""
        return self._outcome

    @property
    def is_terminal(self) -> bool:
        """``True`` once the runner has reached a terminal state."""
        return self._outcome is not None

    # -- Public methods ------------------------------------------------------

    def step(self) -> bool:
        """Execute one chunk and return ``True`` if more work remains.

        On each call:
        1. If already terminal, return ``False`` (idempotent no-op).
        2. Check the cancellation token; if cancelled, publish
           ``"cancelled"`` outcome and return ``False``.
        3. Execute the next chunk. On success, advance the progress
           counter and timestamp.
        4. If the chunk raises :exc:`CancelledError`, publish
           ``"cancelled"`` outcome and return ``False``.
        5. If the chunk raises any other exception, publish
           ``"failed"`` outcome and return ``False``.
        6. If the sequence is exhausted, publish ``"completed"``
           outcome and return ``False``.

        Returns:
            ``True`` when at least one more step is available;
            ``False`` when the runner is terminal (sequence exhausted,
            failed, or cancelled).

        """
        # Idempotent no-op after terminal state.
        if self._outcome is not None:
            return False

        # Check cancellation token before executing.
        if self._cancel_token is not None and self._cancel_token.cancelled:
            self._publish_terminal("cancelled")
            return False

        # Sequence exhausted — completed.
        if self._index >= len(self._steps):
            self._publish_terminal("completed")
            return False

        step = self._steps[self._index]
        try:
            step.fn()
        except CancelledError:
            self._publish_terminal("cancelled")
            return False
        except Exception as exc:
            error_msg = f"{type(exc).__name__}: {exc}"
            self._publish_terminal("failed", error=error_msg)
            return False

        # Success — advance progress.
        self._index += 1
        self._progress.completed = self._index
        self._progress.last_step_at = self._clock()

        # Check if sequence is now exhausted.
        if self._index >= len(self._steps):
            self._publish_terminal("completed")
            return False

        return True

    def cancel(self) -> None:
        """Signal cancellation via the associated token.

        If no token was provided at construction this is a no-op.
        The actual cancellation check occurs on the next :meth:`step`
        call.
        """
        if self._cancel_token is not None:
            self._cancel_token.cancel()

    # -- Internal helpers ----------------------------------------------------

    def _publish_terminal(self, status: str, error: str | None = None) -> None:
        """Publish the terminal outcome exactly once."""
        if self._terminal_published:
            return
        self._outcome = ChunkedOutcome(
            status=status,
            progress=ChunkedProgress(
                completed=self._progress.completed,
                total=self._progress.total,
                last_step_at=self._progress.last_step_at,
            ),
            error=error,
        )
        self._terminal_published = True
