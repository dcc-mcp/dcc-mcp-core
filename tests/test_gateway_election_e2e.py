"""E2E-style coverage for :class:`~dcc_mcp_core.gateway_election.DccGatewayElection`.

Uses a real background ``HTTPServer`` answering ``GET /health`` so the probe
path matches production (``urllib`` → HTTP), then tears the listener down to
simulate gateway death and asserts the election loop eventually invokes the
promotion hook.

Pure mocks in ``test_dcc_adapter_base`` cannot catch regressions where the
probe loop never reaches ``_attempt_election`` or the port-free check disagrees
with the HTTP stack.
"""

from __future__ import annotations

from contextlib import suppress
from http.server import BaseHTTPRequestHandler
from http.server import HTTPServer
import threading
import time
from typing import Any


class _ReuseHTTPServer(HTTPServer):
    allow_reuse_address = True


class _HealthHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        if self.path == "/health" or self.path.startswith("/health?"):
            self.send_response(200)
            self.end_headers()
            self.wfile.write(b'{"ok":true}')
            return
        self.send_response(404)
        self.end_headers()

    def log_message(self, fmt: str, *args: Any) -> None:
        return


def test_live_health_probe_then_death_triggers_promotion() -> None:
    """Health stays green, server stops answering, promotion runs once."""
    from dcc_mcp_core.gateway_election import DccGatewayElection

    httpd = _ReuseHTTPServer(("127.0.0.1", 0), _HealthHandler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, name="fake-gateway-health", daemon=True)
    thread.start()
    promoted = threading.Event()

    class _Srv:
        is_gateway = False
        is_running = True

        def _upgrade_to_gateway(self) -> bool:
            promoted.set()
            return True

    srv = _Srv()
    election = DccGatewayElection(
        dcc_name="pytest-dcc",
        server=srv,
        gateway_port=port,
        probe_interval=0.05,
        probe_timeout=1.0,
        probe_failures=2,
    )
    try:
        time.sleep(0.05)
        assert election._probe_gateway() is True

        election.start()
        time.sleep(0.12)
        assert election._probe_gateway() is True

        httpd.shutdown()
        httpd.server_close()
        thread.join(timeout=5.0)

        deadline = time.time() + 8.0
        while time.time() < deadline and not promoted.is_set():
            time.sleep(0.05)

        assert promoted.is_set(), "promotion hook not invoked after gateway /health went away"
    finally:
        election.stop()
        if election._thread is not None:
            election._thread.join(timeout=5.0)
        with suppress(Exception):
            httpd.shutdown()
        with suppress(Exception):
            httpd.server_close()
        thread.join(timeout=2.0)


def test_stable_health_never_calls_promotion() -> None:
    """While /health keeps returning 200, the promotion hook must stay cold."""
    from dcc_mcp_core.gateway_election import DccGatewayElection

    httpd = _ReuseHTTPServer(("127.0.0.1", 0), _HealthHandler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, name="fake-gateway-health-2", daemon=True)
    thread.start()

    calls = {"n": 0}

    class _Srv:
        is_gateway = False
        is_running = True

        def _upgrade_to_gateway(self) -> bool:
            calls["n"] += 1
            return True

    srv = _Srv()
    election = DccGatewayElection(
        dcc_name="pytest-dcc",
        server=srv,
        gateway_port=port,
        probe_interval=0.05,
        probe_timeout=1.0,
        probe_failures=2,
    )
    try:
        time.sleep(0.05)
        election.start()
        time.sleep(0.45)
        assert calls["n"] == 0
    finally:
        election.stop()
        if election._thread is not None:
            election._thread.join(timeout=5.0)
        with suppress(Exception):
            httpd.shutdown()
        with suppress(Exception):
            httpd.server_close()
        thread.join(timeout=2.0)


def test_gateway_instance_skips_probe_and_promotion() -> None:
    """When ``server.is_gateway`` is true, failures must reset without promoting."""
    from dcc_mcp_core.gateway_election import DccGatewayElection

    httpd = _ReuseHTTPServer(("127.0.0.1", 0), _HealthHandler)
    port = httpd.server_address[1]
    thread = threading.Thread(target=httpd.serve_forever, name="fake-gateway-health-3", daemon=True)
    thread.start()

    calls = {"n": 0}

    class _Srv:
        is_gateway = True
        is_running = True

        def _upgrade_to_gateway(self) -> bool:
            calls["n"] += 1
            return True

    srv = _Srv()
    election = DccGatewayElection(
        dcc_name="pytest-dcc",
        server=srv,
        gateway_port=port,
        probe_interval=0.05,
        probe_timeout=1.0,
        probe_failures=1,
    )
    try:
        time.sleep(0.05)
        election.start()
        election._consecutive_failures = 7
        deadline = time.time() + 3.0
        while time.time() < deadline and election._consecutive_failures != 0:
            time.sleep(0.05)
        assert election._consecutive_failures == 0
        assert calls["n"] == 0
    finally:
        election.stop()
        if election._thread is not None:
            election._thread.join(timeout=5.0)
        with suppress(Exception):
            httpd.shutdown()
        with suppress(Exception):
            httpd.server_close()
        thread.join(timeout=2.0)
