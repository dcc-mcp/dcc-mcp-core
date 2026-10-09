"""Cancellation fences the retained route between native preparation stages."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
import threading

import pytest

from dcc_mcp_core.cancellation import CancelToken
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from test_ui_control_preparation import _ambient_scope
from test_ui_control_preparation import _isolated_scope
from test_ui_control_preparation import methods
from test_ui_control_preparation import route
from test_ui_control_preparation import runtime


@pytest.mark.parametrize("stage", ["after_binding", "after_activation", "after_readback", "after_capture"])
def test_cancelled_preparation_stops_at_next_boundary_and_discards_tokens(route, monkeypatch, stage):
    call, process, launches, _ = route
    token = CancelToken()
    clients = []
    original = PixelsMcpHostClient.__init__

    def track(client, **kwargs):
        original(client, **kwargs)
        clients.append(client)

    monkeypatch.setattr(PixelsMcpHostClient, "__init__", track)
    if stage == "after_binding":
        process.mutate_open = lambda raw: token.cancel()
    else:
        assert call("snapshot")["success"] is True

        def cancel_window(raw):
            if stage != "after_capture" and raw["type"] == (
                "window_state_changed" if stage == "after_activation" else "window_state"
            ):
                token.cancel()

        process.mutate_window = cancel_window
        if stage == "after_capture":
            process.mutate_snapshot = lambda raw: token.cancel()

    result = call("prepare_foreground", {"operation": "restore_activate"}, cancel_token=token)

    assert result["success"] is False, result
    assert result["error"] == "cancelled", result
    context = result["context"]
    preparation = context["foreground_preparation"]
    assert preparation["fresh_observation_ready"] is False
    assert context["blind_retry"] is False
    assert context.get("snapshot_id") is None
    assert context.get("__rich__") is None
    expected = [] if stage == "after_binding" else ["snapshot", "change_window_state"]
    if stage in {"after_readback", "after_capture"}:
        expected.append("get_window_state")
    if stage == "after_capture":
        expected.append("snapshot")
    assert methods(process) == expected
    assert len(launches) == 1
    if stage != "after_binding":
        assert preparation["activation"]["state"]["foreground"] is True
    assert clients[0]._observation_id is None
    assert clients[0]._window_state_id is None
    # Cancellation keeps an acknowledged session available for explicit stop.
    assert call("stop_computer_use")["success"] is True
    assert sum(r.get("params", {}).get("name") == "stop_task" for r in process.requests) == 1


def test_cancellation_while_waiting_for_session_lock_cannot_dispatch_another_activation(route):
    call, process, launches, _ = route
    activation_entered = threading.Event()
    release_activation = threading.Event()
    queued_request_entered = threading.Event()

    class QueuedCancellation(CancelToken):
        @property
        def cancelled(self):
            cancelled = super().cancelled
            queued_request_entered.set()
            return cancelled

    token = QueuedCancellation()

    def block_activation(raw):
        if raw["type"] == "window_state_changed":
            activation_entered.set()
            assert release_activation.wait(5)

    process.mutate_window = block_activation
    with ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(call, "prepare_foreground", {"operation": "restore_activate"})
        try:
            assert activation_entered.wait(5)
            queued = pool.submit(call, "prepare_foreground", {"operation": "restore_activate"}, cancel_token=token)
            assert queued_request_entered.wait(5)
            token.cancel()
        finally:
            release_activation.set()
        assert first.result(timeout=5)["success"] is True
        cancelled = queued.result(timeout=5)
    assert cancelled["success"] is False and cancelled["error"] == "cancelled"
    assert cancelled["context"]["foreground_preparation"]["stage"] == "binding"
    assert methods(process) == ["change_window_state", "get_window_state", "snapshot"]
    assert len(launches) == 1
