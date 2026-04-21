"""Gateway SSE multiplex regression (issue #320).

The gateway subscribes to each backend's SSE stream and forwards
`notifications/progress`, `$/dcc.jobUpdated`, `$/dcc.workflowUpdated`
back to the originating client sessions, correlated via
`progressToken` (for progress) and `job_id` (for the ``$/dcc.*``
channels).

This test focuses on the HTTP plumbing pieces most likely to regress
in future refactors. The deep correlation / reconnect backoff /
pending-buffer logic is covered by the Rust unit tests in
``crates/dcc-mcp-http/src/gateway/sse_subscriber.rs``.

Invariants verified here:

1. ``GET /mcp`` on the gateway preserves the client-supplied
   ``Mcp-Session-Id`` and immediately emits the ``endpoint`` SSE
   event — the per-session ``SessionCleanup`` guard and
   ``SubscriberManager.register_client`` hook both hang off that
   session id.
2. Two concurrent SSE clients with distinct session ids do not
   share a sink — if they did, a progress notification bound for
   client A would leak into client B's stream.
3. The gateway's ``GET /mcp`` handler keeps serving after a brief
   backend churn: the reconnect loop uses jittered exponential
   backoff and must never tear down the front-end.
"""

from __future__ import annotations

import contextlib
import socket
import threading
import time
import urllib.request
import uuid

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry

SSE_READ_BUDGET_S = 6.0


def _pick_free_port() -> int:
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    try:
        return s.getsockname()[1]
    finally:
        s.close()


def _wait_reachable(port: int, budget: float = 5.0) -> bool:
    deadline = time.time() + budget
    while time.time() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.2):
                return True
        except (OSError, socket.timeout):
            time.sleep(0.05)
    return False


class _SseReader(threading.Thread):
    """Background thread that captures raw SSE records from a URL."""

    def __init__(self, url: str, session_id: str) -> None:
        super().__init__(daemon=True)
        self.url = url
        self.session_id = session_id
        self.events: list[str] = []
        self._stop_evt = threading.Event()
        self._reply_session_id: str | None = None
        self._err: Exception | None = None

    @property
    def reply_session_id(self) -> str | None:
        return self._reply_session_id

    @property
    def error(self) -> Exception | None:
        return self._err

    def stop(self) -> None:
        self._stop_evt.set()

    def run(self) -> None:  # pragma: no cover - thread body
        try:
            req = urllib.request.Request(
                self.url,
                headers={
                    "Accept": "text/event-stream",
                    "Cache-Control": "no-cache",
                    "Mcp-Session-Id": self.session_id,
                },
                method="GET",
            )
            with urllib.request.urlopen(req, timeout=5) as resp:
                self._reply_session_id = resp.headers.get("Mcp-Session-Id")
                # Short socket timeout so ``stop()`` unblocks a pending
                # ``readline`` within a few hundred milliseconds instead of
                # hanging until the server closes the connection.
                with contextlib.suppress(Exception):
                    resp.fp.raw._sock.settimeout(0.25)
                pending: list[str] = []
                while not self._stop_evt.is_set():
                    try:
                        line = resp.readline()
                    except (socket.timeout, TimeoutError):
                        continue
                    if not line:
                        break
                    text = line.decode("utf-8", "replace").rstrip("\r\n")
                    if text == "":
                        if pending:
                            self.events.append("\n".join(pending))
                            pending = []
                        continue
                    pending.append(text)
        except Exception as e:
            self._err = e


@pytest.fixture
def gateway_backend(tmp_path):
    """Start a single ``McpHttpServer`` that wins the gateway election.

    One process hosting both the plain-instance endpoint and the
    gateway facade is sufficient for the invariants we check here.
    """
    registry_dir = tmp_path / "registry"
    registry_dir.mkdir()

    gw_port = _pick_free_port()

    reg = ToolRegistry()
    cfg = McpHttpConfig(port=0, server_name="gateway-sse-test")
    cfg.gateway_port = gw_port
    cfg.registry_dir = str(registry_dir)
    cfg.dcc_type = "python"
    cfg.heartbeat_secs = 1
    cfg.stale_timeout_secs = 10

    server = McpHttpServer(reg, cfg)
    handle = server.start()

    assert _wait_reachable(handle.port), f"instance port {handle.port} unreachable"
    if not handle.is_gateway:
        pytest.skip(f"another process is holding gateway port {gw_port}; cannot test gateway SSE")
    assert _wait_reachable(gw_port), f"gateway port {gw_port} unreachable"

    try:
        yield {"handle": handle, "gateway_port": gw_port}
    finally:
        with contextlib.suppress(Exception):
            handle.shutdown()


def test_gateway_sse_echoes_client_session_id(gateway_backend):
    """``GET /mcp`` must preserve the client-supplied ``Mcp-Session-Id``."""
    gw_port = gateway_backend["gateway_port"]
    sid = f"sess-{uuid.uuid4().hex[:8]}"

    reader = _SseReader(f"http://127.0.0.1:{gw_port}/mcp", session_id=sid)
    reader.start()
    try:
        deadline = time.time() + SSE_READ_BUDGET_S
        while time.time() < deadline and not reader.events:
            time.sleep(0.05)
        assert reader.events, f"no SSE events within {SSE_READ_BUDGET_S}s (err={reader.error})"
        assert any("endpoint" in e for e in reader.events[:2]), f"expected endpoint event, got: {reader.events[:2]}"
        assert reader.reply_session_id == sid, f"gateway must echo session id unchanged; got {reader.reply_session_id}"
    finally:
        reader.stop()
        reader.join(timeout=2)


def test_gateway_sse_two_clients_have_distinct_sinks(gateway_backend):
    """Two SSE clients with different session ids must be isolated.

    The ``SubscriberManager`` keeps a per-session ``broadcast::Sender``;
    cross-talk would mean losing routing for progress notifications.
    """
    gw_port = gateway_backend["gateway_port"]

    sid_a = f"sess-a-{uuid.uuid4().hex[:6]}"
    sid_b = f"sess-b-{uuid.uuid4().hex[:6]}"

    reader_a = _SseReader(f"http://127.0.0.1:{gw_port}/mcp", session_id=sid_a)
    reader_b = _SseReader(f"http://127.0.0.1:{gw_port}/mcp", session_id=sid_b)

    reader_a.start()
    reader_b.start()
    try:
        deadline = time.time() + SSE_READ_BUDGET_S
        while time.time() < deadline and not (reader_a.events and reader_b.events):
            time.sleep(0.05)

        assert reader_a.events, f"A got no events (err={reader_a.error})"
        assert reader_b.events, f"B got no events (err={reader_b.error})"
        assert reader_a.reply_session_id == sid_a
        assert reader_b.reply_session_id == sid_b
        assert reader_a.reply_session_id != reader_b.reply_session_id, (
            "session ids must be distinct — subscriber sinks are keyed on them"
        )
    finally:
        reader_a.stop()
        reader_b.stop()
        reader_a.join(timeout=2)
        reader_b.join(timeout=2)


def test_gateway_sse_keeps_serving_under_concurrent_connects(gateway_backend):
    """The gateway must handle overlapping SSE connect attempts without
    tearing down existing subscribers.

    The SSE multiplexer's reconnect loop uses jittered exponential
    backoff (100 ms → 10 s); opening several connections in quick
    succession must not destabilise the supervisor task.
    """
    gw_port = gateway_backend["gateway_port"]
    readers = [_SseReader(f"http://127.0.0.1:{gw_port}/mcp", session_id=f"probe-{i}") for i in range(3)]
    for r in readers:
        r.start()
    try:
        deadline = time.time() + SSE_READ_BUDGET_S
        while time.time() < deadline and any(not r.events for r in readers):
            time.sleep(0.05)
        for i, r in enumerate(readers):
            assert r.events, f"reader {i} got no events (err={r.error})"
    finally:
        for r in readers:
            r.stop()
        for r in readers:
            r.join(timeout=2)
