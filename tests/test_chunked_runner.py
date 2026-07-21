"""Contract tests for :mod:`dcc_mcp_core.chunked_runner`.

Covers:
- Fake host pump contract: pending cancellation, mid-sequence cancellation,
  generator failure, terminal idempotence.
- Adapter label tests proving the contract is not Houdini-specific.
"""

from __future__ import annotations

# Import built-in modules
import time

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import CancelToken
from dcc_mcp_core import CancelledError
from dcc_mcp_core import ChunkedOutcome
from dcc_mcp_core import ChunkedProgress
from dcc_mcp_core import ChunkedRunner
from dcc_mcp_core import ChunkedStep
from dcc_mcp_core import set_cancel_token
from dcc_mcp_core import reset_cancel_token


# ---------------------------------------------------------------------------
# Top-level exports
# ---------------------------------------------------------------------------


def test_exports_available() -> None:
    """All public symbols must be importable from the top-level package."""
    import dcc_mcp_core

    for name in ("ChunkedRunner", "ChunkedProgress", "ChunkedOutcome", "ChunkedStep"):
        assert hasattr(dcc_mcp_core, name), name
        assert name in dcc_mcp_core.__all__


# ---------------------------------------------------------------------------
# ChunkedStep
# ---------------------------------------------------------------------------


def test_chunked_step_slots() -> None:
    """ChunkedStep is a slotted, lightweight value object."""
    fn = lambda: None
    s = ChunkedStep(3, fn)
    assert s.step == 3
    assert s.fn is fn


def test_chunked_step_accepts_callable_in_runner() -> None:
    """ChunkedRunner wraps plain callables into ChunkedStep."""
    results: list[int] = []

    def a() -> None:
        results.append(1)

    def b() -> None:
        results.append(2)

    runner = ChunkedRunner([a, b])
    assert runner.step() is True
    assert runner.step() is False
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
    assert results == [1, 2]


def test_chunked_runner_rejects_non_callable() -> None:
    """Non-callable items raise TypeError at construction."""
    with pytest.raises(TypeError, match="Expected ChunkedStep or callable"):
        ChunkedRunner([42])  # type: ignore[list-item]


# ---------------------------------------------------------------------------
# ChunkedProgress
# ---------------------------------------------------------------------------


def test_chunked_progress_defaults() -> None:
    """Default progress starts at zero."""
    p = ChunkedProgress()
    assert p.completed == 0
    assert p.total is None
    assert p.last_step_at == 0.0


# ---------------------------------------------------------------------------
# Fake host pump contract tests
# ---------------------------------------------------------------------------


class _FakeHostPump:
    """Minimal fake host pump that calls runner.step() per tick.

    This simulates a DCC main-loop tick callback (Maya ``scriptJob``,
    Blender ``app.timers``, Houdini ``addEventLoopCallback``, etc.).
    """

    def __init__(self, runner: ChunkedRunner, max_ticks: int = 1000) -> None:
        self.runner = runner
        self.max_ticks = max_ticks
        self.ticks = 0

    def run(self) -> ChunkedOutcome:
        """Drain the runner one step per tick until terminal or max_ticks."""
        while self.runner.step():
            self.ticks += 1
            if self.ticks >= self.max_ticks:
                raise RuntimeError("Fake host pump exceeded max ticks")
        outcome = self.runner.outcome
        assert outcome is not None
        return outcome


def _make_steps(n: int) -> list[ChunkedStep]:
    """Create N identity steps that record invocation."""
    calls: list[int] = []

    def _step(i: int) -> None:
        calls.append(i)

    steps = [ChunkedStep(i, lambda i=i: _step(i)) for i in range(n)]
    return steps, calls


def test_pending_cancellation() -> None:
    """Cancel while idle → next step() returns False, outcome is "cancelled"."""
    token = CancelToken()
    token.cancel()  # cancelled before any step
    runner = ChunkedRunner([lambda: None], cancel_token=token)

    assert runner.step() is False
    assert runner.is_terminal is True
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"
    assert runner.outcome.progress.completed == 0
    assert runner.outcome.error is None


def test_mid_sequence_cancellation() -> None:
    """Cancel during execution → next step catches, outcome is "cancelled"."""
    token = CancelToken()
    steps = [lambda: token.cancel(), lambda: None]
    runner = ChunkedRunner(steps, cancel_token=token)

    # First step executes and cancels the token.
    assert runner.step() is True
    assert runner.progress.completed == 1

    # Second step sees the cancelled token and becomes terminal.
    assert runner.step() is False
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"
    # Progress should not advance for the cancelled step.
    assert runner.outcome.progress.completed == 1


def test_generator_failure() -> None:
    """Chunk raises exception → outcome is "failed" with error message."""
    def _bad() -> None:
        raise ValueError("mesh data corrupted")

    runner = ChunkedRunner([_bad])
    assert runner.step() is False
    assert runner.outcome is not None
    assert runner.outcome.status == "failed"
    assert "ValueError" in (runner.outcome.error or "")
    assert "mesh data corrupted" in (runner.outcome.error or "")
    assert runner.outcome.progress.completed == 0


def test_terminal_idempotence() -> None:
    """Calling step() after terminal state is a no-op."""
    def _ok() -> None:
        pass

    runner = ChunkedRunner([_ok])
    assert runner.step() is False  # completed
    assert runner.outcome is not None
    first_outcome = runner.outcome

    # Subsequent step() calls are idempotent.
    for _ in range(5):
        assert runner.step() is False
        assert runner.outcome is first_outcome  # same object, not re-created


def test_progress_monotonic() -> None:
    """Progress counter only increases; timestamp updates after each step."""
    steps: list[float] = []

    def _record_step() -> None:
        steps.append(time.monotonic())

    runner = ChunkedRunner([_record_step, _record_step, _record_step])

    for i in range(2):
        assert runner.progress.completed == i
        assert runner.step() is True  # more work remains
        assert runner.progress.completed == i + 1
        assert runner.progress.last_step_at > 0.0
        assert len(steps) == i + 1

    # Third step — sequence exhausted after this one.
    assert runner.step() is False
    assert runner.progress.completed == 3


def test_no_token_noop() -> None:
    """Without cancel token, the runner completes normally."""
    runner = ChunkedRunner([lambda: None, lambda: None])
    while runner.step():
        pass
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
    assert runner.outcome.progress.completed == 2


def test_multiple_steps() -> None:
    """Multiple step() calls drain all chunks in order."""
    order: list[int] = []

    def _make(i: int):
        def _fn() -> None:
            order.append(i)

        return _fn

    runner = ChunkedRunner([_make(i) for i in range(10)])

    while runner.step():
        pass

    assert order == list(range(10))
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
    assert runner.outcome.progress.completed == 10


def test_empty_sequence() -> None:
    """Runner with no chunks is terminal immediately."""
    runner = ChunkedRunner([])
    assert runner.is_terminal is False  # not terminal until first step
    assert runner.step() is False
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
    assert runner.outcome.progress.completed == 0


def test_chunked_runner_with_chunked_step_objects() -> None:
    """Runner accepts pre-built ChunkedStep objects."""
    results: list[str] = []

    def a() -> None:
        results.append("a")

    def b() -> None:
        results.append("b")

    steps = [ChunkedStep(0, a), ChunkedStep(1, b)]
    runner = ChunkedRunner(steps)
    assert runner.step() is True
    assert runner.step() is False
    assert results == ["a", "b"]


def test_cancel_method_triggers_token() -> None:
    """Runner.cancel() sets the token; next step() picks it up."""
    token = CancelToken()
    runner = ChunkedRunner([lambda: None, lambda: None], cancel_token=token)

    assert runner.step() is True  # first step completes
    runner.cancel()
    assert token.cancelled is True
    assert runner.step() is False  # cancelled
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"


def test_cancel_no_token_is_noop() -> None:
    """cancel() without a token is a safe no-op."""
    runner = ChunkedRunner([lambda: None])
    runner.cancel()  # should not raise
    assert runner.step() is False  # completes normally
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"


def test_chunked_outcome_repr() -> None:
    """Outcome repr is useful for debugging."""
    p = ChunkedProgress(completed=1, total=3, last_step_at=1000.0)
    o = ChunkedOutcome(status="completed", progress=p)
    assert "completed" in repr(o)
    assert "completed=1" in repr(o)


# ---------------------------------------------------------------------------
# Adapter label tests — proving contract is not Houdini-specific
# ---------------------------------------------------------------------------


def test_label_standalone_host_adapter() -> None:
    """ChunkedRunner integrates with HostAdapter via attach_tick pattern.

    This test uses a manual pump to simulate the host event loop —
    no real DCC required. The pattern is the same for all DCCs:
    ``attach_tick`` → call ``runner.step()`` → return interval or None.
    """
    results: list[int] = []

    def _make(i: int):
        def _fn() -> None:
            results.append(i)

        return _fn

    runner = ChunkedRunner([_make(i) for i in range(5)])

    class _ManualPump:
        """Mimics a DCC idle callback that drains the runner one step at a time."""

        def __init__(self) -> None:
            self.tick_count = 0
            self.intervals: list[float | None] = []

        def tick(self) -> float | None:
            self.tick_count += 1
            has_more = runner.step()
            if has_more:
                interval: float | None = 0.05  # active
            else:
                interval = None  # done — detach
            self.intervals.append(interval)
            return interval

    pump = _ManualPump()
    # Simulate the host calling tick() repeatedly until None.
    while True:
        interval = pump.tick()
        if interval is None:
            break

    assert results == list(range(5))
    # 5 ticks: steps 1-4 return interval (active), step 5 returns None (terminal)
    assert pump.tick_count == 5
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
    assert runner.outcome.progress.completed == 5


def test_label_inprocess_callable_dispatcher() -> None:
    """ChunkedRunner works inside an InProcessCallableDispatcher callback.

    The InProcessCallableDispatcher is the reference single-thread impl
    used by mayapy, headless Houdini, and pytest. Submitting a callable
    that drains a ChunkedRunner demonstrates the runner runs correctly
    in the main-thread-affine dispatch path.
    """
    from dcc_mcp_core import InProcessCallableDispatcher

    dispatcher = InProcessCallableDispatcher()
    results: list[int] = []

    def _make(i: int):
        def _fn() -> None:
            results.append(i)

        return _fn

    runner = ChunkedRunner([_make(i) for i in range(4)])

    def _drain_runner() -> int:
        ticks = 0
        while runner.step():
            ticks += 1
        return ticks

    outcome = dispatcher.submit_callable("test-1", _drain_runner, "any", timeout_ms=5000)
    assert outcome.ok, outcome.error
    assert results == list(range(4))
    assert outcome.value == 3  # 3 active ticks before completion
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"


def test_label_manual_host_timer_adapter() -> None:
    """ChunkedRunner integrated with ManualHostTimerAdapter (test harness).

    ManualHostTimerAdapter is the test adapter for HostPumpController.
    This test proves the runner contract works with the host pump
    infrastructure used by real DCC adapters.
    """
    from dcc_mcp_core import ManualHostTimerAdapter

    results: list[int] = []

    def _make(i: int):
        def _fn() -> None:
            results.append(i)

        return _fn

    runner = ChunkedRunner([_make(i) for i in range(3)])

    adapter = ManualHostTimerAdapter()
    adapter.install(lambda: 0.05 if runner.step() else None)

    # Fire ticks until None is returned (runner terminal).
    while True:
        interval = adapter.fire()
        if interval is None:
            break

    assert results == list(range(3))
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"
