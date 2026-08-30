from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import threading
import time

import pytest

from dcc_mcp_core import CancelToken
from dcc_mcp_core import ContinuationOutcome
from dcc_mcp_core import HostExecutionBridge
from dcc_mcp_core import SplitPhaseOutcome


def test_continuation_public_import_parity() -> None:
    from dcc_mcp_core._server._continuation_lifecycle import resolve_bridge_result
    from dcc_mcp_core._server._inprocess_results import resolve_execution_result
    from dcc_mcp_core._server.inprocess_executor import ContinuationOutcome as ExecutorOutcome

    assert ExecutorOutcome is ContinuationOutcome
    assert callable(resolve_bridge_result)
    assert callable(resolve_execution_result)


def test_split_phase_releases_main_segment_before_continuation() -> None:
    segments: list[float] = []

    class MainDispatcher:
        def is_host_thread(self):
            return False

        def dispatch_callable(self, func, **kwargs):
            started = time.perf_counter()
            value = func()
            segments.append(time.perf_counter() - started)
            return value

    bridge = HostExecutionBridge(dispatcher=MainDispatcher())

    def main_phase():
        def continuation(_probe):
            deadline = time.perf_counter() + 0.075
            while time.perf_counter() < deadline:
                pass
            return {"ok": True}

        return SplitPhaseOutcome(continuation)

    started = time.perf_counter()
    result = bridge.dispatch_callable(main_phase, thread_affinity="main")
    elapsed = time.perf_counter() - started

    assert result == {"ok": True}
    assert elapsed >= 0.075
    assert segments and segments[0] < 0.05


def test_split_phase_rejects_nested_outcome() -> None:
    bridge = HostExecutionBridge()
    result = bridge.dispatch_callable(
        lambda: ContinuationOutcome(lambda _probe: ContinuationOutcome(lambda _nested_probe: {"bad": True})),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "nested" in result["message"].lower()


def test_split_phase_rejects_forged_mapping_marker() -> None:
    bridge = HostExecutionBridge()
    result = bridge.dispatch_callable(
        lambda: {"_dcc_mcp_split_phase": {"kind": "continuation.v1"}},
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "malformed split-phase marker" in result["message"].lower()


def test_split_phase_non_serializable_output_fails_closed() -> None:
    bridge = HostExecutionBridge()
    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(lambda _probe: object()),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "serial" in result["message"].lower()


def test_split_phase_exception_is_structured() -> None:
    bridge = HostExecutionBridge()

    def fail(_probe):
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


def test_split_phase_rejects_uncancellable_sync_callback_without_token() -> None:
    published = []
    bridge = HostExecutionBridge()

    def continuation():
        published.append(True)
        return {"published": True}

    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(continuation),
        thread_affinity="main",
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
        lambda: SplitPhaseOutcome(lambda _probe: {"ok": True}),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "host thread" in result["message"].lower()


def test_split_phase_resolution_fails_closed_without_thread_identity() -> None:
    class OpaqueDispatcher:
        def dispatch_callable(self, func, **kwargs):
            return func()

    bridge = HostExecutionBridge(dispatcher=OpaqueDispatcher())
    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(lambda _probe: {"ok": True}),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "thread identity" in result["message"].lower()


def test_split_phase_shutdown_during_continuation_blocks_commit() -> None:
    bridge = HostExecutionBridge()

    def continuation(_probe):
        bridge.close_script_admission()
        return {"published": True}

    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(continuation),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert "cancel" in result["message"].lower()


def test_running_cancellation_probe_blocks_durable_side_effect() -> None:
    token = CancelToken()
    started = threading.Event()
    release = threading.Event()
    published: list[bool] = []
    bridge = HostExecutionBridge()

    def continuation(probe):
        started.set()
        release.wait(1)
        if not probe.cancelled:
            published.append(True)
        return {"published": True}

    with ThreadPoolExecutor(max_workers=1) as pool:
        call = pool.submit(
            bridge.dispatch_callable,
            lambda: SplitPhaseOutcome(continuation),
            thread_affinity="main",
            cancel_token=token,
        )
        assert started.wait(1)
        token.cancel()
        release.set()
        result = call.result(timeout=2)
    assert result["success"] is False
    assert published == []


def test_sync_continuation_receives_deadline_probe_without_cancel_token() -> None:
    observed = []
    bridge = HostExecutionBridge()

    def continuation(probe):
        observed.append(probe)
        deadline = time.perf_counter() + 0.02
        while time.perf_counter() < deadline:
            pass
        probe.check()
        return {"published": True}

    result = bridge.dispatch_callable(
        lambda: SplitPhaseOutcome(continuation, timeout_secs=0.001),
        thread_affinity="main",
    )
    assert result["success"] is False
    assert observed and observed[0] is not None
    assert "cancel" in result["message"].lower()
