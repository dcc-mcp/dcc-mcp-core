"""Bound native confirmation waits without weakening response correlation."""

import queue

import pytest

from dcc_mcp_core.cua_cli import CuaCliBridge
from dcc_mcp_core.cua_cli import CuaCliError
from test_cua_cli import _fake_command


@pytest.mark.parametrize(
    "method,expected",
    [("execute_action", 120.0), ("invoke_menu", 120.0), ("snapshot", 35.0), ("ping", 35.0)],
)
def test_confirmation_wait_budget_is_method_scoped(tmp_path, monkeypatch, method, expected):
    with CuaCliBridge(_fake_command(tmp_path)) as bridge:
        original_get = bridge._responses.get
        observed = []

        def get(timeout):
            observed.append(timeout)
            return original_get(timeout=timeout)

        monkeypatch.setattr(bridge._responses, "get", get)
        response = bridge.call(method, {"session_id": "test"}, request_id="confirmation-1")
        assert response["request_id"] == "confirmation-1"
        assert observed == [expected]


def test_explicit_timeout_remains_bounded_and_does_not_replay(tmp_path, monkeypatch):
    bridge = CuaCliBridge(_fake_command(tmp_path))
    observed = []

    def timeout_response(timeout):
        observed.append(timeout)
        raise queue.Empty

    monkeypatch.setattr(bridge._responses, "get", timeout_response)
    try:
        with pytest.raises(CuaCliError) as failure:
            bridge.call("execute_action", timeout=0.25)
        assert failure.value.code == "timeout"
        assert observed == [0.25]
        with pytest.raises(CuaCliError) as closed:
            bridge.call("ping")
        assert closed.value.code == "transport_error"
        assert observed == [0.25]
    finally:
        bridge.close()
