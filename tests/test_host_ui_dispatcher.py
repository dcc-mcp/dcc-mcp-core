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


def test_host_ui_job_honours_check_dcc_cancelled():
    entry = HostUiJobEntry("j", "main", lambda: None)

    def _task():
        entry.cancel()
        check_dcc_cancelled()

    entry.task = _task
    outcome = entry.execute()
    assert outcome["success"] is False
    assert outcome["error"] == DispatcherErrorCode.CANCELLED


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
