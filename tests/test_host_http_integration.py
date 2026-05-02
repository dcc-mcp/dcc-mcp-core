"""Integration tests for ``McpHttpServer.attach_dispatcher`` (P2b).

Verifies the cross-DCC main-thread dispatcher from
:mod:`dcc_mcp_core.host` actually serves as the tools/call thread
whenever an :class:`McpHttpServer` has one attached. No DCC binary
required — :class:`StandaloneHost` stands in as the driver.

Contract covered:

* Attaching before ``start()`` makes every subsequent ``tools/call``
  execute its handler on the dispatcher-tick thread, not the tokio
  worker.
* Re-attaching is rejected (SRP: hot-swap is future work).
* Running without an attached dispatcher preserves legacy behaviour
  (handlers run on a tokio worker, backward compatibility).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import json
import threading
import time
from typing import Any
import urllib.request

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core.host import DispatchError
from dcc_mcp_core.host import QueueDispatcher
from dcc_mcp_core.host import StandaloneHost

# ── helpers ─────────────────────────────────────────────────────────


def _call_tool(url: str, tool: str, arguments: dict[str, Any] | None = None) -> dict:
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments or {}},
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json", "Accept": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def _make_server(server_name: str) -> tuple[McpHttpServer, ToolRegistry]:
    reg = ToolRegistry()
    reg.register(
        "thread_probe",
        description="Return the thread id handling the call.",
        category="test",
        dcc="test",
        version="1.0.0",
    )
    cfg = McpHttpConfig(port=0, server_name=server_name)
    return McpHttpServer(reg, cfg), reg


# ── tests ───────────────────────────────────────────────────────────


def test_tools_call_routes_through_dispatcher() -> None:
    """Main-thread affinity: the handler runs on the dispatcher tick
    thread, never on a tokio worker or the poster thread.
    """
    server, _reg = _make_server("p2b-routing")

    # Record the thread that runs the handler.
    captured: dict[str, int | None] = {"tid": None}

    def _probe(_params: dict) -> dict:
        captured["tid"] = threading.get_ident()
        return {"tid": captured["tid"]}

    server.register_handler("thread_probe", _probe)

    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)

    host = StandaloneHost(dispatcher, tick_interval=0.005)
    host.start()
    handle = server.start()
    try:
        # Give the server a moment to bind.
        time.sleep(0.2)
        resp = _call_tool(handle.mcp_url(), "thread_probe")
        # tools/call returns a CallToolResult envelope; the handler's
        # return value lives inside ``content[0].text`` as JSON text.
        assert resp.get("error") is None, resp
        text = resp["result"]["content"][0]["text"]
        assert "tid" in text, text

        tick_tid = host._thread.ident  # type: ignore[union-attr]
        assert captured["tid"] == tick_tid, (
            f"handler ran on thread {captured['tid']}, expected dispatcher tick thread {tick_tid}"
        )
        # Poster thread must be different — proves we didn't
        # accidentally bypass the dispatcher.
        assert captured["tid"] != threading.get_ident()
    finally:
        handle.shutdown()
        host.stop()


def test_attach_dispatcher_rejects_second_call() -> None:
    """Re-attaching on the same server is rejected with RuntimeError."""
    server, _reg = _make_server("p2b-reject")
    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)
    with pytest.raises(RuntimeError, match="already called"):
        server.attach_dispatcher(QueueDispatcher())


def test_attach_dispatcher_rejects_non_dispatcher_type() -> None:
    """Passing a random Python object is a TypeError, not a panic."""
    server, _reg = _make_server("p2b-type")
    with pytest.raises(TypeError, match="QueueDispatcher or BlockingDispatcher"):
        server.attach_dispatcher(object())


def test_without_dispatcher_backcompat() -> None:
    """No ``attach_dispatcher`` call → existing tokio-worker path still works."""
    server, _reg = _make_server("p2b-backcompat")
    captured: dict[str, int | None] = {"tid": None}

    def _probe(_params: dict) -> dict:
        captured["tid"] = threading.get_ident()
        return {"ok": True}

    server.register_handler("thread_probe", _probe)
    handle = server.start()
    try:
        time.sleep(0.2)
        resp = _call_tool(handle.mcp_url(), "thread_probe")
        assert resp.get("error") is None, resp
        assert captured["tid"] is not None
    finally:
        handle.shutdown()


def test_dispatcher_shutdown_during_call_surfaces_error() -> None:
    """If the dispatcher is shut down while a call is in flight, the
    HTTP caller receives a clean error envelope, not a hang.
    """
    server, _reg = _make_server("p2b-shutdown")

    def _slow(_params: dict) -> dict:
        time.sleep(0.2)
        return {"ok": True}

    server.register_handler("thread_probe", _slow)

    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)
    host = StandaloneHost(dispatcher, tick_interval=0.005)
    host.start()
    handle = server.start()
    try:
        time.sleep(0.2)
        # Shut down the dispatcher immediately — any later call will
        # be rejected by the dispatcher before even reaching the
        # handler.
        dispatcher.shutdown()
        host.stop()

        resp = _call_tool(handle.mcp_url(), "thread_probe")
        # The response must be well-formed JSON-RPC. Either isError is
        # set on the CallToolResult, or the dispatch error leaked as a
        # JSON-RPC error — both are acceptable "clean error" shapes.
        is_tool_error = resp.get("result", {}).get("isError") is True
        is_rpc_error = resp.get("error") is not None
        assert is_tool_error or is_rpc_error, f"expected a clean error envelope after shutdown, got: {resp}"
    finally:
        handle.shutdown()
