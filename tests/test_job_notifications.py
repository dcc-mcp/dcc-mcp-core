"""End-to-end tests for job lifecycle notifications (issue #326).

Exercises all three SSE channels:

* ``notifications/progress`` — MCP 2025-03-26 standard, fires when
  ``_meta.progressToken`` is supplied.
* ``notifications/$/dcc.jobUpdated`` — vendor extension, fires unconditionally
  while ``enable_job_notifications`` is ``True`` (default).
* ``notifications/$/dcc.workflowUpdated`` — vendor extension, emitted by the
  forthcoming #348 workflow executor; this test only validates the channel
  is reachable through the config flag.
"""

from __future__ import annotations

import contextlib
import json
import threading
import time
from typing import Any
import urllib.request

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _post_json(
    url: str,
    body: dict[str, Any],
    headers: dict[str, str] | None = None,
) -> tuple[int, dict[str, Any]]:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        headers={
            "Content-Type": "application/json",
            "Accept": "application/json",
            **(headers or {}),
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=5) as resp:
        return resp.status, json.loads(resp.read())


class _SseCollector:
    """Background reader for the ``GET /mcp`` SSE stream.

    Parses ``data: <json>`` frames and accumulates them in ``self.frames``
    until ``.stop()`` is called or the server closes the connection.
    """

    def __init__(self, url: str, session_id: str) -> None:
        self._url = url
        self._session_id = session_id
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self.frames: list[dict[str, Any]] = []
        self._lock = threading.Lock()

    def start(self) -> None:
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _run(self) -> None:
        # Use raw sockets so we can do line-by-line reads on a keep-alive
        # SSE stream without blocking on a fixed-size buffer.
        import socket
        from urllib.parse import urlparse

        parsed_url = urlparse(self._url)
        host = parsed_url.hostname or "127.0.0.1"
        port = parsed_url.port or 80
        path = parsed_url.path or "/"

        try:
            sock = socket.create_connection((host, port), timeout=10)
        except OSError:
            return
        try:
            sock.settimeout(0.2)
            request = (
                f"GET {path} HTTP/1.1\r\n"
                f"Host: {host}:{port}\r\n"
                f"Accept: text/event-stream\r\n"
                f"Mcp-Session-Id: {self._session_id}\r\n"
                f"Connection: keep-alive\r\n"
                f"\r\n"
            )
            sock.sendall(request.encode())

            buf = b""
            # Skip HTTP headers
            while b"\r\n\r\n" not in buf and not self._stop.is_set():
                try:
                    chunk = sock.recv(4096)
                except socket.timeout:
                    continue
                if not chunk:
                    return
                buf += chunk
            _, _, buf = buf.partition(b"\r\n\r\n")
            text_buf = buf.decode("utf-8", errors="ignore")

            while not self._stop.is_set():
                try:
                    chunk = sock.recv(4096)
                except socket.timeout:
                    # flush whatever we have so far
                    if "\n\n" in text_buf:
                        self._drain_frames_into_buf(text_buf)
                        text_buf = text_buf.rsplit("\n\n", 1)[-1]
                    continue
                if not chunk:
                    break
                text_buf += chunk.decode("utf-8", errors="ignore")
                while "\n\n" in text_buf:
                    frame, text_buf = text_buf.split("\n\n", 1)
                    self._ingest_frame(frame)
        finally:
            with contextlib.suppress(OSError):
                sock.close()

    def _drain_frames_into_buf(self, text_buf: str) -> None:
        # Split transfer-encoding chunks if present. For simplicity we
        # strip any line that doesn't start with "data:".
        for frame in text_buf.split("\n\n"):
            self._ingest_frame(frame)

    def _ingest_frame(self, frame: str) -> None:
        # Axum's SSE layer wraps the payload in its own ``data: `` framing,
        # so an event we generate as ``data: {json}\n\n`` arrives as
        # ``data: data: {json}\ndata: \ndata: \n\n`` on the wire. Strip
        # any number of leading ``data:`` prefixes until we find JSON.
        for line in frame.splitlines():
            line = line.strip()
            while line.startswith("data:"):
                line = line[len("data:") :].strip()
            if not line or not line.startswith("{"):
                continue
            try:
                parsed = json.loads(line)
            except json.JSONDecodeError:
                continue
            with self._lock:
                self.frames.append(parsed)

    def wait_for(
        self,
        predicate,
        timeout: float = 5.0,
        interval: float = 0.05,
    ) -> dict[str, Any] | None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            with self._lock:
                for frame in self.frames:
                    if predicate(frame):
                        return frame
            time.sleep(interval)
        return None

    def snapshot(self) -> list[dict[str, Any]]:
        with self._lock:
            return list(self.frames)

    def stop(self) -> None:
        self._stop.set()


def _start_server(enable_job_notifications: bool = True):
    reg = ToolRegistry()
    reg.register(
        "slow_tool",
        description="Placeholder tool",
        category="test",
        tags=[],
        dcc="test",
        version="1.0.0",
    )
    cfg = McpHttpConfig(port=0, server_name="job-notif-test")
    cfg.enable_job_notifications = enable_job_notifications
    server = McpHttpServer(reg, cfg)
    server.register_handler(
        "slow_tool",
        lambda params: {"status": "ok", "echo": params},
    )
    handle = server.start()
    return server, handle, handle.mcp_url()


def _initialize_session(url: str) -> str:
    code, body = _post_json(
        url,
        {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "pytest", "version": "1.0"},
            },
        },
    )
    assert code == 200
    return body["result"]["__session_id"]


class TestJobNotifications:
    def test_progress_and_job_updated_fire_for_tool_call_with_token(self):
        _, handle, url = _start_server(enable_job_notifications=True)
        try:
            sid = _initialize_session(url)
            collector = _SseCollector(url, sid)
            collector.start()
            # give the SSE GET a moment to hit the broadcast channel
            time.sleep(0.2)

            code, body = _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 42,
                    "method": "tools/call",
                    "params": {
                        "name": "slow_tool",
                        "arguments": {"x": 1},
                        "_meta": {"progressToken": "tok-xyz"},
                    },
                },
                headers={"Mcp-Session-Id": sid},
            )
            assert code == 200
            assert body["result"]["isError"] is False

            # Channel A — notifications/progress with the echoed token
            progress = collector.wait_for(
                lambda f: (
                    f.get("method") == "notifications/progress"
                    and f.get("params", {}).get("progressToken") == "tok-xyz"
                    and f.get("params", {}).get("message") == "completed"
                ),
                timeout=5.0,
            )
            assert progress is not None, f"no terminal progress frame: {collector.snapshot()}"
            assert progress["params"]["progress"] == 100
            assert progress["params"]["total"] == 100

            # Channel B — $/dcc.jobUpdated with completed status
            job_update = collector.wait_for(
                lambda f: (
                    f.get("method") == "notifications/$/dcc.jobUpdated"
                    and f.get("params", {}).get("status") == "completed"
                ),
                timeout=5.0,
            )
            assert job_update is not None, f"no terminal job update: {collector.snapshot()}"
            assert job_update["params"]["tool"] == "slow_tool"
            assert job_update["params"]["completed_at"] is not None
            assert job_update["params"]["error"] is None

            collector.stop()
        finally:
            handle.shutdown()

    def test_job_updated_fires_without_progress_token(self):
        _, handle, url = _start_server(enable_job_notifications=True)
        try:
            sid = _initialize_session(url)
            collector = _SseCollector(url, sid)
            collector.start()
            time.sleep(0.2)

            _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 7,
                    "method": "tools/call",
                    "params": {"name": "slow_tool", "arguments": {}},
                },
                headers={"Mcp-Session-Id": sid},
            )

            job_update = collector.wait_for(
                lambda f: (
                    f.get("method") == "notifications/$/dcc.jobUpdated"
                    and f.get("params", {}).get("status") == "completed"
                ),
                timeout=5.0,
            )
            assert job_update is not None
            # progress channel must NOT fire for a call that had no token
            snap = collector.snapshot()
            progress_frames = [f for f in snap if f.get("method") == "notifications/progress"]
            assert progress_frames == [], progress_frames

            collector.stop()
        finally:
            handle.shutdown()

    def test_disabling_flag_suppresses_dcc_channels_but_keeps_progress(self):
        _, handle, url = _start_server(enable_job_notifications=False)
        try:
            sid = _initialize_session(url)
            collector = _SseCollector(url, sid)
            collector.start()
            time.sleep(0.2)

            _post_json(
                url,
                {
                    "jsonrpc": "2.0",
                    "id": 99,
                    "method": "tools/call",
                    "params": {
                        "name": "slow_tool",
                        "arguments": {},
                        "_meta": {"progressToken": "tok-flag-off"},
                    },
                },
                headers={"Mcp-Session-Id": sid},
            )

            # Progress STILL fires because the client supplied a token
            progress = collector.wait_for(
                lambda f: (
                    f.get("method") == "notifications/progress"
                    and f.get("params", {}).get("progressToken") == "tok-flag-off"
                ),
                timeout=5.0,
            )
            assert progress is not None

            # But $/dcc.jobUpdated / $/dcc.workflowUpdated are silent
            # (small settle delay so late frames are included)
            time.sleep(0.3)
            snap = collector.snapshot()
            offending = [f for f in snap if f.get("method", "").startswith("notifications/$/dcc.")]
            assert offending == [], offending

            collector.stop()
        finally:
            handle.shutdown()

    def test_config_defaults(self):
        cfg = McpHttpConfig(port=0, server_name="defaults")
        assert cfg.enable_job_notifications is True
        cfg.enable_job_notifications = False
        assert cfg.enable_job_notifications is False


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
