from __future__ import annotations

import time

import pytest

from dcc_mcp_core import CancelToken
from dcc_mcp_core import ContinuationOutcome
from dcc_mcp_core import HostExecutionBridge
from dcc_mcp_core import SplitPhaseOutcome


def test_split_phase_releases_main_segment_before_continuation() -> None:
    segments: list[float] = []

    class MainDispatcher:
        def dispatch_callable(self, func, **kwargs):
            started = time.perf_counter()
            value = func()
            segments.append(time.perf_counter() - started)
            return value

    bridge = HostExecutionBridge(dispatcher=MainDispatcher())

    def main_phase():
        return SplitPhaseOutcome(lambda: (time.sleep(0.075), {"ok": True})[1])

    started = time.perf_counter()
    result = bridge.dispatch_callable(main_phase, thread_affinity="main")
    elapsed = time.perf_counter() - started

    assert result == {"ok": True}
    assert elapsed >= 0.075
    assert segments and segments[0] < 0.05


def test_split_phase_rejects_nested_outcome() -> None:
    bridge = HostExecutionBridge()
    result = bridge.dispatch_callable(
        lambda: ContinuationOutcome(lambda: ContinuationOutcome(lambda: {"bad": True})),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "nested" in result["message"].lower()


def test_split_phase_non_serializable_output_fails_closed() -> None:
    bridge = HostExecutionBridge()
    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(lambda: object()),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "serial" in result["message"].lower()


def test_split_phase_exception_is_structured() -> None:
    bridge = HostExecutionBridge()

    def fail():
        raise RuntimeError("encode failed")

    result = bridge.dispatch_callable(lambda: SplitPhaseOutcome(fail), thread_affinity="main")
    assert result["success"] is False
    assert result["error"] == "RuntimeError"


def test_split_phase_requires_callable() -> None:
    with pytest.raises(TypeError):
        SplitPhaseOutcome(1)  # type: ignore[arg-type]


def test_cancelled_token_blocks_commit() -> None:
    token = CancelToken()
    bridge = HostExecutionBridge()

    def continuation(_probe):
        token.cancel()
        return {"published": True}

    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(continuation),
        thread_affinity="main",
        cancel_token=token,
    )
    assert result["success"] is False
    assert "cancel" in result["message"].lower()


@pytest.mark.parametrize("timeout", [float("nan"), float("inf"), 1e300, 301.0])
def test_split_phase_timeout_is_finite_and_bounded(timeout: float) -> None:
    with pytest.raises(ValueError):
        SplitPhaseOutcome(lambda: {"ok": True}, timeout_secs=timeout)


def test_split_phase_rejects_uncancellable_callback_before_side_effect() -> None:
    published = []
    token = CancelToken()
    bridge = HostExecutionBridge()

    def continuation():
        published.append(True)
        return {"published": True}

    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(continuation),
        thread_affinity="main",
        cancel_token=token,
    )
    assert result["success"] is False
    assert published == []
    assert "uncancellable" in result["message"].lower()


def test_split_phase_resolution_fails_closed_on_host_thread() -> None:
    class HostDispatcher:
        def is_host_thread(self):
            return True

        def dispatch_callable(self, func, **kwargs):
            return func()

    bridge = HostExecutionBridge(dispatcher=HostDispatcher())
    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(lambda: {"ok": True}),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "host thread" in result["message"].lower()
