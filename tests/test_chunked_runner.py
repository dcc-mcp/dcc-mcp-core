"""Contract tests for tick-driven main-affinity jobs."""

from __future__ import annotations

import threading

import pytest

from dcc_mcp_core import CancelToken
from dcc_mcp_core import ChunkedRunner
from dcc_mcp_core import ChunkedStep
from dcc_mcp_core import chunked_job
from dcc_mcp_core._server.host_ui_dispatcher import HostUiDispatcherBase


class _ManualDispatcher(HostUiDispatcherBase):
    def __init__(self, label: str = "fake-host") -> None:
        super().__init__(label=label)
        self.pokes = 0

    def poke_host_pump(self) -> None:
        self.pokes += 1


def test_public_exports() -> None:
    import dcc_mcp_core

    for name in ("ChunkedRunner", "ChunkedProgress", "ChunkedOutcome", "ChunkedStep", "chunked_job"):
        assert name in dcc_mcp_core.__all__


def test_host_pump_runs_one_step_per_tick_and_unrelated_work_continues() -> None:
    dispatcher = _ManualDispatcher()
    events = []
    runner = ChunkedRunner([lambda: events.append("chunk-1"), lambda: events.append("chunk-2")])
    dispatcher.submit_chunked_runner("chunked", runner)
    dispatcher.submit_async_callable("unrelated", lambda: events.append("unrelated"), affinity="main")

    dispatcher.drain_queue(100)
    assert events == ["chunk-1", "unrelated"]
    assert runner.progress.completed == 1
    assert not runner.is_terminal

    dispatcher.drain_queue(100)
    assert events == ["chunk-1", "unrelated", "chunk-2"]
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"


def test_pending_cancel_is_acknowledged_on_pump_checkpoint() -> None:
    dispatcher = _ManualDispatcher()
    ran = []
    runner = ChunkedRunner([lambda: ran.append(True)])
    dispatcher.submit_chunked_runner("cancel-pending", runner)

    assert dispatcher.cancel("cancel-pending")
    assert runner.outcome is None
    dispatcher.drain_queue(100)
    assert ran == []
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"


def test_mid_sequence_cancel_stops_before_later_step() -> None:
    token = CancelToken()
    ran = []
    runner = ChunkedRunner(
        [lambda: ran.append(1) or token.cancel(), lambda: ran.append(2)],
        cancel_token=token,
    )

    assert runner.step() is False
    assert ran == [1]
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"
    assert runner.outcome.progress.completed == 1


def test_dispatcher_can_cancel_an_active_chunk() -> None:
    dispatcher = _ManualDispatcher()
    entered = threading.Event()
    release = threading.Event()

    def blocking_step() -> None:
        entered.set()
        assert release.wait(timeout=2)

    runner = ChunkedRunner([blocking_step, lambda: pytest.fail("later step ran")])
    dispatcher.submit_chunked_runner("active-chunk", runner)
    pump = threading.Thread(target=lambda: dispatcher.drain_queue(100))
    pump.start()

    assert entered.wait(timeout=2)
    assert dispatcher.active_count() == 1
    assert dispatcher.cancel("active-chunk")
    assert runner.outcome is None
    release.set()
    pump.join(timeout=2)

    assert not pump.is_alive()
    assert runner.outcome is not None
    assert runner.outcome.status == "cancelled"
    assert runner.outcome.progress.completed == 1
    assert dispatcher.active_count() == 0


def test_progress_is_monotonic_and_published_after_each_step() -> None:
    timestamps = iter([10.0, 20.0, 30.0])
    observed = []
    runner = ChunkedRunner(
        [lambda: "one", lambda: "two", lambda: "three"],
        clock=lambda: next(timestamps),
        on_progress=lambda progress: observed.append(
            (progress.completed, progress.total, progress.message, progress.last_step_at)
        ),
    )

    while runner.step():
        pass

    assert observed == [
        (1, 3, "one", 10.0),
        (2, 3, "two", 20.0),
        (3, 3, "three", 30.0),
    ]


def test_progress_callback_failure_becomes_terminal_failure() -> None:
    def fail_progress(_progress) -> None:
        raise RuntimeError("progress store unavailable")

    runner = ChunkedRunner([lambda: None], on_progress=fail_progress)
    assert not runner.step()
    assert runner.outcome is not None
    assert runner.outcome.status == "failed"
    assert runner.outcome.error == "RuntimeError: progress store unavailable"


def test_lazy_generator_failure_is_terminal() -> None:
    def steps():
        yield lambda: None
        raise ValueError("generator failed")

    runner = ChunkedRunner(steps())
    assert runner.step()
    assert not runner.step()
    assert runner.outcome is not None
    assert runner.outcome.status == "failed"
    assert runner.outcome.error == "ValueError: generator failed"


def test_terminal_callback_and_state_are_idempotent() -> None:
    terminal = []
    runner = ChunkedRunner([lambda: None], on_terminal=terminal.append)

    assert not runner.step()
    first = runner.outcome
    for _ in range(3):
        assert not runner.step()
    assert runner.outcome is first
    assert terminal == [first]


def test_unknown_total_iterator_finishes_on_following_checkpoint() -> None:
    runner = ChunkedRunner(iter([lambda: None]))
    assert runner.step()
    assert runner.progress.total is None
    assert not runner.step()
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"


def test_invalid_iterator_item_fails_at_its_tick() -> None:
    runner = ChunkedRunner(iter([42]))
    assert not runner.step()
    assert runner.outcome is not None
    assert runner.outcome.error == "TypeError: Expected ChunkedStep or callable, got int"


def test_chunked_step_index_is_adapter_metadata_only() -> None:
    called = []
    step = ChunkedStep(7, lambda: called.append(True))
    runner = ChunkedRunner([step])
    assert not runner.step()
    assert step.step == 7
    assert called == [True]


@pytest.mark.parametrize("label", ["maya-ui", "photoshop-bridge"])
def test_same_core_runner_is_host_neutral(label: str) -> None:
    dispatcher = _ManualDispatcher(label)
    runner = ChunkedRunner([lambda: None])
    dispatcher.submit_chunked_runner(label, runner)
    dispatcher.drain_queue(100)
    assert dispatcher.dispatcher_label == label
    assert runner.outcome is not None
    assert runner.outcome.status == "completed"


def test_chunked_job_decorator_builds_lazy_runner() -> None:
    built = []

    @chunked_job(total=2)
    def build():
        built.append("factory")
        yield lambda: built.append(1)
        yield lambda: built.append(2)

    runner = build()
    assert built == []
    assert isinstance(runner, ChunkedRunner)
    assert runner.step()
    assert not runner.step()
    assert built == ["factory", 1, 2]


def test_chunked_runner_rejects_negative_total() -> None:
    with pytest.raises(ValueError, match="total"):
        ChunkedRunner([], total=-1)


def test_chunked_completion_callback_uses_wire_safe_progress() -> None:
    dispatcher = _ManualDispatcher()
    completed = []
    runner = ChunkedRunner([lambda: "done"])
    dispatcher.submit_chunked_runner("wire", runner, job_id="job-1", on_complete=completed.append)
    dispatcher.drain_queue(100)

    assert completed == [
        {
            "request_id": "wire",
            "affinity": "main",
            "success": True,
            "output": {"current": 1, "total": 1, "message": "done"},
            "error": None,
            "job_id": "job-1",
        }
    ]
