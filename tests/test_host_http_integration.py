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
import urllib.error
import urllib.request

# Import third-party modules
import pytest

from conftest import McpClient

# Import local modules
from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry
from dcc_mcp_core.host import DispatchError
from dcc_mcp_core.host import QueueDispatcher
from dcc_mcp_core.host import StandaloneHost

# ── helpers ─────────────────────────────────────────────────────────


def _call_tool(url: str, tool: str, arguments: dict[str, Any] | None = None) -> dict:
    client = McpClient(url)
    body = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool, "arguments": arguments or {}},
    }
    _, resp = client.post(body)
    return resp


def _rest_base_url(mcp_url: str) -> str:
    return mcp_url.rsplit("/mcp", 1)[0]


def _rest_call(
    mcp_url: str,
    tool_slug: str,
    arguments: dict[str, Any] | None = None,
) -> tuple[int, dict[str, Any]]:
    """POST ``/v1/call`` — same path the gateway and ``dcc-mcp-cli call`` use."""
    payload = json.dumps(
        {"tool_slug": tool_slug, "arguments": arguments or {}},
    ).encode("utf-8")
    req = urllib.request.Request(
        f"{_rest_base_url(mcp_url)}/v1/call",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8")
        return exc.code, json.loads(body) if body else {}


def _start_routed_server(
    server_name: str,
) -> tuple[McpHttpServer, ToolRegistry, StandaloneHost, Any]:
    """Server with ``QueueDispatcher`` + ``StandaloneHost`` (thread-routed REST)."""
    server, reg = _make_server(server_name)
    reg.register(
        "thread_probe",
        description="Return the thread id handling the call.",
        category="test",
        dcc="test",
        version="1.0.0",
        thread_affinity="main",
        enforce_thread_affinity=True,
    )
    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)
    host = StandaloneHost(dispatcher, tick_interval=0.005)
    host.start()
    handle = server.start()
    time.sleep(0.2)
    return server, reg, host, handle


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
    server, reg = _make_server("p2b-routing")

    # Record the thread that runs the handler.
    captured: dict[str, int | None] = {"tid": None}

    def _probe(_params: dict) -> dict:
        captured["tid"] = threading.get_ident()
        return {"tid": captured["tid"]}

    server.register_handler("thread_probe", _probe, thread_affinity="main")
    reg.register(
        "thread_probe",
        description="Return the thread id handling the call.",
        category="test",
        dcc="test",
        version="1.0.0",
        thread_affinity="main",
        enforce_thread_affinity=True,
    )

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

    server.register_handler("thread_probe", _slow, thread_affinity="main")

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


def test_any_affinity_bypasses_dispatcher() -> None:
    """Regression guard for core#716 from the Python side.

    A handler declared ``thread_affinity="any"`` MUST NOT route through
    the attached dispatcher even when one is wired. Instead it runs on
    a tokio worker — which means the captured thread id is **different**
    from the dispatcher tick thread, and specifically not equal to it.

    The pre-#716 bug was that any declared handler would get pulled
    through the UI dispatcher whenever an executor existed, serialising
    pure-compute calls behind scene-mutating ones. This test locks in
    the fixed behaviour.
    """
    server, _reg = _make_server("p2b-any-bypass")

    captured: dict[str, int | None] = {"tid": None}

    def _probe(_params: dict) -> dict:
        captured["tid"] = threading.get_ident()
        return {"tid": captured["tid"]}

    # Explicit `any` — post-#716, this must bypass the attached
    # dispatcher and run on a tokio worker.
    server.register_handler("thread_probe", _probe, thread_affinity="any")

    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)

    host = StandaloneHost(dispatcher, tick_interval=0.005)
    host.start()
    handle = server.start()
    try:
        time.sleep(0.2)
        resp = _call_tool(handle.mcp_url(), "thread_probe")
        assert resp.get("error") is None, resp
        assert captured["tid"] is not None, "handler did not run"

        tick_tid = host._thread.ident  # type: ignore[union-attr]
        assert captured["tid"] != tick_tid, (
            f"`any`-affinity handler unexpectedly ran on the dispatcher tick thread "
            f"({tick_tid}) — the #716 bypass is not engaged"
        )
        # Poster thread is also not a valid landing spot for a tokio
        # worker, but a second equality assertion would couple the
        # test to the poster's identity. The tick-thread inequality is
        # what matters.
    finally:
        handle.shutdown()
        host.stop()


def test_register_handler_rejects_bad_affinity() -> None:
    """Invalid ``thread_affinity`` values surface as ``ValueError``."""
    server, _reg = _make_server("p2b-bad-affinity")
    with pytest.raises(ValueError, match="thread_affinity must be"):
        server.register_handler("thread_probe", lambda _params: {"ok": True}, thread_affinity="sometimes")


def test_v1_call_rejects_invalid_params_with_400() -> None:
    """Thread-routed REST must map ``ValidationFailed`` to HTTP 400, not 502."""
    server, reg, host, handle = _start_routed_server("rest-invalid-params")
    schema = {
        "type": "object",
        "required": ["radius"],
        "properties": {"radius": {"type": "number"}},
    }
    reg.register(
        "typed_probe",
        description="Requires radius",
        category="test",
        dcc="test",
        version="1.0.0",
        thread_affinity="main",
        enforce_thread_affinity=True,
        input_schema=json.dumps(schema),
    )
    server.register_handler(
        "typed_probe",
        lambda params: {"radius": params.get("radius")},
        thread_affinity="main",
    )
    try:
        status, body = _rest_call(handle.mcp_url(), "typed_probe", {})
        assert status == 400, body
        assert body.get("kind") == "invalid-params", body
    finally:
        handle.shutdown()
        host.stop()


def test_v1_call_reports_validation_skipped_for_empty_schema() -> None:
    """Thread-routed REST must preserve ``validation_skipped`` in the response."""
    server, reg, host, handle = _start_routed_server("rest-validation-skipped")
    reg.register(
        "loose_probe",
        description="No schema constraints",
        category="test",
        dcc="test",
        version="1.0.0",
        thread_affinity="main",
        enforce_thread_affinity=True,
        input_schema=json.dumps({}),
    )
    server.register_handler(
        "loose_probe",
        lambda _params: {"ok": True},
        thread_affinity="main",
    )
    try:
        status, body = _rest_call(handle.mcp_url(), "loose_probe", {"anything": "goes"})
        assert status == 200, body
        assert body.get("validation_skipped") is True, body
    finally:
        handle.shutdown()
        host.stop()


def test_v1_call_routes_through_dispatcher() -> None:
    """REST ``POST /v1/call`` must honour ``thread_affinity=main`` like MCP ``tools/call``.

    Gateway and ``dcc-mcp-cli call`` fan out through per-DCC ``/v1/call``. Without
    :class:`~dcc_mcp_http_server.ThreadRoutedInvoker`, handlers run on a Tokio
    worker and ``enforce_thread_affinity`` rejects the call with HTTP 409.
    """
    server, reg = _make_server("rest-thread-routing")
    captured: dict[str, int | None] = {"tid": None}

    def _probe(_params: dict) -> dict:
        captured["tid"] = threading.get_ident()
        return {"tid": captured["tid"]}

    server.register_handler("thread_probe", _probe, thread_affinity="main")
    reg.register(
        "thread_probe",
        description="Return the thread id handling the call.",
        category="test",
        dcc="test",
        version="1.0.0",
        thread_affinity="main",
        enforce_thread_affinity=True,
    )

    dispatcher = QueueDispatcher()
    server.attach_dispatcher(dispatcher)
    host = StandaloneHost(dispatcher, tick_interval=0.005)
    host.start()
    handle = server.start()
    try:
        time.sleep(0.2)
        tick_tid = host._thread.ident  # type: ignore[union-attr]

        status, body = _rest_call(handle.mcp_url(), "thread_probe", {})
        assert status == 200, body
        assert "thread-affinity-violation" not in json.dumps(body).lower(), body
        output = body.get("output", body)
        assert output.get("tid") == captured["tid"]
        assert captured["tid"] == tick_tid, (
            f"REST handler ran on {captured['tid']}, expected dispatcher tick {tick_tid}"
        )

        captured["tid"] = None
        resp = _call_tool(handle.mcp_url(), "thread_probe")
        assert resp.get("error") is None, resp
        assert captured["tid"] == tick_tid
    finally:
        handle.shutdown()
        host.stop()
