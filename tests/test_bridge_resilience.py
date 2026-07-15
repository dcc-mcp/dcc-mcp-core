"""Tests for bridge resilience, fallback, and reverse-session helpers."""

from __future__ import annotations

import asyncio
import json
import logging
import sys
import threading
from types import SimpleNamespace

import pytest

import dcc_mcp_core
from dcc_mcp_core import BridgeConnectionError
from dcc_mcp_core import BridgeFallbackClient
from dcc_mcp_core import BridgeRetryPolicy
from dcc_mcp_core import BridgeRpcError
from dcc_mcp_core import BridgeTransportStrategy
from dcc_mcp_core import DccBridge
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


class _ServerContext:
    def __init__(self) -> None:
        self.closed = threading.Event()

    async def __aenter__(self):
        return self

    async def __aexit__(self, *_args):
        self.closed.set()


class _MemoryWebSocket:
    _CLOSED = object()

    def __init__(self) -> None:
        self.remote_address = ("memory", 0)
        self._incoming: asyncio.Queue = asyncio.Queue()
        self._outgoing: asyncio.Queue = asyncio.Queue()

    def __aiter__(self):
        return self

    async def __anext__(self):
        message = await self._incoming.get()
        if message is self._CLOSED:
            raise StopAsyncIteration
        return message

    async def send(self, message: str) -> None:
        await self._outgoing.put(message)

    async def client_send(self, message: dict) -> None:
        await self._incoming.put(json.dumps(message))

    async def client_recv(self) -> dict:
        return json.loads(await self._outgoing.get())

    async def close(self) -> None:
        await self._incoming.put(self._CLOSED)


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


def test_dcc_bridge_disconnect_closes_server_without_crashing_event_loop(monkeypatch, caplog) -> None:
    server = _ServerContext()
    monkeypatch.setitem(sys.modules, "websockets", SimpleNamespace(serve=lambda *_args, **_kwargs: server))
    bridge = DccBridge(host="127.0.0.1", port=0)

    with caplog.at_level(logging.ERROR, logger="dcc_mcp_core.bridge"):
        bridge.connect()
        loop = bridge._loop
        bridge.disconnect()

    assert server.closed.wait(timeout=1.0)
    assert loop is not None and loop.is_closed()
    assert "DccBridge event loop crashed" not in caplog.text


def test_dcc_bridge_falls_back_when_newer_plugin_disconnects() -> None:
    async def scenario() -> None:
        bridge = DccBridge(timeout=1.0)
        bridge._loop = asyncio.get_running_loop()
        primary = _MemoryWebSocket()
        secondary = _MemoryWebSocket()
        primary_task = asyncio.create_task(bridge._handle_dcc(primary))
        secondary_task = None
        try:
            await primary.client_send({"type": "hello", "client": "primary", "version": "1"})
            assert (await primary.client_recv())["type"] == "hello_ack"

            secondary_task = asyncio.create_task(bridge._handle_dcc(secondary))
            await secondary.client_send({"type": "hello", "client": "temporary", "version": "1"})
            assert (await secondary.client_recv())["type"] == "hello_ack"
            await secondary.close()
            await secondary_task

            assert bridge.is_connected()
            call = asyncio.get_running_loop().run_in_executor(None, bridge.call, "project.inspect")
            request = await asyncio.wait_for(primary.client_recv(), timeout=1.0)
            await primary.client_send({"type": "response", "id": request["id"], "result": "primary"})
            assert await call == "primary"
        finally:
            if secondary_task is not None and not secondary_task.done():
                await secondary.close()
            await primary.close()
            await primary_task
            bridge.disconnect()

    asyncio.run(scenario())


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
