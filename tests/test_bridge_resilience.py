"""Tests for bridge resilience, fallback, and reverse-session helpers."""

from __future__ import annotations

import threading

import pytest

import dcc_mcp_core
from dcc_mcp_core import BridgeConnectionError
from dcc_mcp_core import BridgeFallbackClient
from dcc_mcp_core import BridgeRetryPolicy
from dcc_mcp_core import BridgeRpcError
from dcc_mcp_core import BridgeTransportStrategy
from dcc_mcp_core import ReverseBridgeSession


class _Strategy(BridgeTransportStrategy):
    def __init__(self, name: str, *, connect_failures: int = 0, call_failure: bool = False) -> None:
        self.name = name
        self.connect_failures = connect_failures
        self.call_failure = call_failure
        self.connect_attempts = 0
        self.connected = False
        self.calls: list[tuple[str, dict]] = []

    def connect(self) -> None:
        self.connect_attempts += 1
        if self.connect_failures > 0:
            self.connect_failures -= 1
            raise BridgeConnectionError(f"{self.name} unavailable")
        self.connected = True

    def disconnect(self) -> None:
        self.connected = False

    def is_connected(self) -> bool:
        return self.connected

    def call(self, method: str, **params):
        self.calls.append((method, params))
        if self.call_failure:
            self.connected = False
            self.call_failure = False
            raise BridgeConnectionError("lost connection")
        return {"strategy": self.name, "method": method, "params": params}


def test_bridge_resilience_symbols_exported() -> None:
    for name in (
        "BridgeFallbackClient",
        "BridgeRetryPolicy",
        "BridgeTransportStrategy",
        "ReverseBridgeRequest",
        "ReverseBridgeSession",
    ):
        assert hasattr(dcc_mcp_core, name)
        assert name in dcc_mcp_core.__all__


def test_retry_policy_retries_operation() -> None:
    attempts = {"count": 0}
    policy = BridgeRetryPolicy(attempts=3, initial_delay_secs=0)

    def operation():
        attempts["count"] += 1
        if attempts["count"] < 3:
            raise BridgeConnectionError("not yet")
        return "ok"

    assert policy.run(operation) == "ok"
    assert attempts["count"] == 3


def test_fallback_client_uses_next_strategy_after_failure() -> None:
    primary = _Strategy("primary", connect_failures=1)
    secondary = _Strategy("secondary")
    client = BridgeFallbackClient([primary, secondary], retry_policy=BridgeRetryPolicy(attempts=1))

    active = client.connect()

    assert active is secondary
    assert client.call("scene.info")["strategy"] == "secondary"


def test_fallback_client_reconnects_when_active_call_loses_connection() -> None:
    primary = _Strategy("primary", call_failure=True)
    secondary = _Strategy("secondary")
    client = BridgeFallbackClient([primary, secondary], retry_policy=BridgeRetryPolicy(attempts=1))
    client.connect()

    result = client.call("scene.info", verbose=True)

    assert result["strategy"] == "secondary"
    assert result["params"] == {"verbose": True}


def test_reverse_bridge_session_round_trips_request() -> None:
    session = ReverseBridgeSession(timeout=1.0)
    results: list[object] = []

    def host_call() -> None:
        results.append(session.call("ps.document.info", include_layers=True))

    thread = threading.Thread(target=host_call)
    thread.start()

    request = session.next_request(timeout=1.0)
    assert request is not None
    assert request.to_jsonrpc()["method"] == "ps.document.info"
    assert request.to_jsonrpc()["params"] == {"include_layers": True}
    assert session.submit_response(request.id, result={"name": "hero.psd"}) is True

    thread.join(timeout=1.0)
    assert results == [{"name": "hero.psd"}]


def test_reverse_bridge_session_maps_rpc_errors() -> None:
    session = ReverseBridgeSession(timeout=1.0)
    errors: list[BaseException] = []

    def host_call() -> None:
        try:
            session.call("danger")
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=host_call)
    thread.start()
    request = session.next_request(timeout=1.0)
    assert request is not None
    assert session.submit_response(request.id, error={"code": -32000, "message": "blocked"})
    thread.join(timeout=1.0)

    assert isinstance(errors[0], BridgeRpcError)
    assert str(errors[0]) == "[-32000] blocked"


def test_reverse_bridge_session_close_fails_pending_calls() -> None:
    session = ReverseBridgeSession(timeout=1.0)
    errors: list[BaseException] = []

    def host_call() -> None:
        try:
            session.call("long.running")
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=host_call)
    thread.start()
    assert session.next_request(timeout=1.0) is not None
    session.close("shutdown")
    thread.join(timeout=1.0)

    assert isinstance(errors[0], BridgeConnectionError)


def test_retry_policy_validates_config() -> None:
    with pytest.raises(ValueError, match="attempts"):
        BridgeRetryPolicy(attempts=0)
