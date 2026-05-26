"""Regression tests for HostUiDispatcherBase (shared UI-thread dispatch)."""

from __future__ import annotations

import threading
import time

import pytest

from dcc_mcp_core._server.host_ui_dispatcher import DEFAULT_UI_JOB_TIMEOUT_MS
from dcc_mcp_core._server.host_ui_dispatcher import DispatcherErrorCode
from dcc_mcp_core._server.host_ui_dispatcher import HostUiDispatcherBase
from dcc_mcp_core._server.host_ui_dispatcher import HostUiJobEntry
from dcc_mcp_core._server.host_ui_dispatcher import host_ui_outcome
from dcc_mcp_core._server.host_ui_dispatcher import normalize_affinity
from dcc_mcp_core.cancellation import CancelledError
from dcc_mcp_core.cancellation import check_dcc_cancelled


class _SyncPumpDispatcher(HostUiDispatcherBase):
    """Test double: drain the queue inline when poked."""

    def poke_host_pump(self) -> None:
        self.drain_queue(budget_ms=DEFAULT_UI_JOB_TIMEOUT_MS)


class _NoopPumpDispatcher(HostUiDispatcherBase):
    """Test double: leave queued jobs pending until tests inspect them."""

    def poke_host_pump(self) -> None:
        return None


def test_normalize_affinity_rejects_unknown():
    with pytest.raises(ValueError):
        normalize_affinity("worker")


def test_run_on_any_thread_success():
    out = HostUiDispatcherBase.run_on_any_thread("r1", lambda: 42, "any")
    assert out["success"] is True
    assert out["output"] == 42


def test_submit_main_runs_on_pump():
    disp = _SyncPumpDispatcher()
    out = disp.submit_callable("req-1", lambda: {"ok": True}, affinity="main")
    assert out["success"] is True
    assert out["output"] == {"ok": True}


def test_main_exception_uses_formatter_hook():
    class _FormattedDispatcher(_SyncPumpDispatcher):
        def format_exception_error(self, exc: BaseException) -> str:
            return f"formatted:{type(exc).__name__}:{exc}"

    disp = _FormattedDispatcher()
    out = disp.submit_callable("boom", lambda: (_ for _ in ()).throw(ValueError("bad")), affinity="main")
    assert out["success"] is False
    assert out["error"] == "formatted:ValueError:bad"


def test_any_thread_exception_uses_formatter_hook():
    class _FormattedDispatcher(_SyncPumpDispatcher):
        def format_exception_error(self, exc: BaseException) -> str:
            return f"any:{type(exc).__name__}:{exc}"

    disp = _FormattedDispatcher()
    out = disp.submit_callable("boom-any", lambda: (_ for _ in ()).throw(RuntimeError("down")), affinity="any")
    assert out["success"] is False
    assert out["error"] == "any:RuntimeError:down"


def test_failing_formatter_hook_falls_back_to_exception_string():
    class _FailingFormatterDispatcher(_SyncPumpDispatcher):
        def format_exception_error(self, exc: BaseException) -> str:
            raise RuntimeError("formatter failed")

    disp = _FailingFormatterDispatcher()
    out = disp.submit_callable("boom-any", lambda: (_ for _ in ()).throw(ValueError("plain")), affinity="any")
    assert out["success"] is False
    assert out["error"] == "plain"


def test_main_timeout_uses_formatter_hook():
    class _TimeoutDispatcher(_NoopPumpDispatcher):
        def format_timeout_error(self, request_id: str, affinity: str, timeout_sec: float) -> str:
            return f"timeout:{request_id}:{affinity}:{timeout_sec:.3f}"

    disp = _TimeoutDispatcher()
    out = disp.submit_callable("slow", lambda: "never", affinity="main", timeout_ms=1)
    assert out["success"] is False
    assert out["error"] == "timeout:slow:main:0.001"


def test_failing_timeout_hook_falls_back_to_default_error():
    class _FailingTimeoutDispatcher(_NoopPumpDispatcher):
        def format_timeout_error(self, request_id: str, affinity: str, timeout_sec: float) -> str:
            raise RuntimeError("timeout formatter failed")

    disp = _FailingTimeoutDispatcher()
    out = disp.submit_callable("slow", lambda: "never", affinity="main", timeout_ms=1)
    assert out["success"] is False
    assert out["error"] == "Timeout (0.0s) waiting for main-thread execution"


def test_cancel_queued_job():
    disp = HostUiDispatcherBase.__new__(HostUiDispatcherBase)
    HostUiDispatcherBase.__init__(disp)
    disp.poke_host_pump = lambda: None  # type: ignore[method-assign]

    job = HostUiJobEntry("block", "main", lambda: time.sleep(5))
    with disp._lock:
        disp._main_queue.append(job)

    assert disp.cancel("block") is True
    assert job.outcome is not None
    assert job.outcome["error"] == DispatcherErrorCode.CANCELLED


def test_shutdown_unblocks_waiter():
    disp = _SyncPumpDispatcher()

    def _block():
        time.sleep(0.05)
        return "done"

    waiter = threading.Thread(
        target=lambda: disp.submit_callable("slow", _block, affinity="main", timeout_ms=5000),
        daemon=True,
    )
    waiter.start()
    time.sleep(0.01)
    disp.shutdown("Interrupted")
    waiter.join(timeout=2.0)
    assert not waiter.is_alive()


def test_fail_fast_host_busy():
    disp = _SyncPumpDispatcher(fail_fast_on_main_queue_busy=True)
    with disp._lock:
        disp._main_queue.append(
            HostUiJobEntry("ahead", "main", lambda: None),
        )
    out = disp.submit_callable("behind", lambda: None, affinity="main")
    assert out["success"] is False
    assert out["error"] == DispatcherErrorCode.HOST_BUSY


def test_async_main_fail_fast_host_busy():
    disp = _NoopPumpDispatcher(fail_fast_on_main_queue_busy=True)
    with disp._lock:
        disp._main_queue.append(
            HostUiJobEntry("ahead", "main", lambda: None),
        )

    out = disp.submit_async_callable(
        "behind",
        lambda: None,
        affinity="main",
        job_id="job-2",
    )

    assert out["success"] is False
    assert out["status"] == "failed"
    assert out["error"] == DispatcherErrorCode.HOST_BUSY
    assert out["job_id"] == "job-2"
    assert disp.pending_count() == 1


def test_host_ui_job_honours_check_dcc_cancelled():
    entry = HostUiJobEntry("j", "main", lambda: None)

    def _task():
        entry.cancel()
        check_dcc_cancelled()

    entry.task = _task
    outcome = entry.execute()
    assert outcome["success"] is False
    assert outcome["error"] == DispatcherErrorCode.CANCELLED


def test_active_count_visible_while_main_job_runs():
    disp = _NoopPumpDispatcher()
    seen = []

    disp.submit_async_callable(
        "visible",
        lambda: seen.append(disp.active_count()) or "ok",
        affinity="main",
    )

    assert disp.queue_size() == 1
    assert disp.active_count() == 0
    disp.drain_queue(budget_ms=DEFAULT_UI_JOB_TIMEOUT_MS)
    assert seen == [1]
    assert disp.active_count() == 0


def test_lifecycle_hooks_observe_main_job_order():
    class _HookedDispatcher(_SyncPumpDispatcher):
        def __init__(self) -> None:
            super().__init__(label="hooked-host")
            self.events = []

        def on_job_queued(self, job: HostUiJobEntry) -> None:
            self.events.append(("queued", job.request_id, self.queue_size()))

        def on_job_started(self, job: HostUiJobEntry) -> None:
            self.events.append(("started", job.request_id, self.active_count()))

        def on_job_finished(self, job: HostUiJobEntry) -> None:
            self.events.append(("finished", job.request_id, self.active_count()))

    disp = _HookedDispatcher()
    out = disp.submit_callable("hooked", lambda: "ok", affinity="main")

    assert out["success"] is True
    assert disp.dispatcher_label == "hooked-host"
    assert disp.events == [
        ("queued", "hooked", 1),
        ("started", "hooked", 1),
        ("finished", "hooked", 0),
    ]


def test_lifecycle_hook_failures_do_not_interrupt_job():
    class _FailingHookDispatcher(_SyncPumpDispatcher):
        def on_job_queued(self, job: HostUiJobEntry) -> None:
            raise RuntimeError(f"queued:{job.request_id}")

        def on_job_started(self, job: HostUiJobEntry) -> None:
            raise RuntimeError(f"started:{job.request_id}")

        def on_job_finished(self, job: HostUiJobEntry) -> None:
            raise RuntimeError(f"finished:{job.request_id}")

    disp = _FailingHookDispatcher()
    out = disp.submit_callable("hooked", lambda: "ok", affinity="main")
    assert out["success"] is True
    assert out["output"] == "ok"


def test_async_any_affinity_completes():
    disp = _SyncPumpDispatcher()
    done = threading.Event()
    results = []

    def _on_complete(outcome):
        results.append(outcome)
        done.set()

    disp.submit_async_callable(
        "async-any",
        lambda: 7,
        affinity="any",
        on_complete=_on_complete,
    )
    assert done.wait(timeout=2.0)
    assert results[0]["success"] is True
    assert results[0]["output"] == 7


def test_host_ui_outcome_shape():
    payload = host_ui_outcome("id", "main", success=True, output=1, job_id="jid")
    assert payload["request_id"] == "id"
    assert payload["job_id"] == "jid"
