"""Regression tests for issue #303 — gateway/server listener reachability.

These tests exercise the same invariant as
``crates/dcc-mcp-http/tests/gateway_reachability.rs`` but from Python,
where the server is created via :class:`McpHttpServer` — the same path
that fails under PyO3-embedded Maya on Windows.

Key contract:

- When ``handle = server.start()`` returns, a plain TCP connect to
  ``handle.bind_addr`` must succeed within a short deadline.
- When ``handle.is_gateway`` is ``True``, a connect to the gateway port
  must also succeed.
- The Python-side default ``spawn_mode`` is ``"dedicated"`` (listener
  on its own OS thread), which is what makes this work under Maya on
  Windows.
"""

from __future__ import annotations

import socket
import threading
import time

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import ToolRegistry


def _tcp_reachable(host: str, port: int, timeout: float = 0.5) -> bool:
    """Return True if a TCP connect to (host, port) succeeds within timeout."""
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except (OSError, socket.timeout):
        return False


def _wait_reachable(host: str, port: int, budget: float = 2.0) -> bool:
    """Poll until reachable or budget expires."""
    deadline = time.time() + budget
    while time.time() < deadline:
        if _tcp_reachable(host, port, timeout=0.2):
            return True
        time.sleep(0.02)
    return False


def _empty_registry() -> ToolRegistry:
    return ToolRegistry()


class TestInstanceListenerReachable:
    """Instance MCP listener must be reachable whenever ``.start()`` returns."""

    def test_dedicated_default_reachable(self):
        """The Python default (dedicated spawn mode) must produce a reachable listener."""
        config = McpHttpConfig(port=0, server_name="reach-py-dedicated")
        # Issue #303: Python default is "dedicated".
        assert config.spawn_mode == "dedicated"
        server = McpHttpServer(_empty_registry(), config)
        handle = server.start()
        try:
            assert _wait_reachable("127.0.0.1", handle.port, budget=1.0), (
                f"Dedicated listener on port {handle.port} unreachable after .start() returned"
            )
        finally:
            handle.shutdown()

    def test_ambient_opt_out_reachable(self):
        """When the caller explicitly opts out of Dedicated mode it must still work."""
        config = McpHttpConfig(port=0, server_name="reach-py-ambient")
        config.spawn_mode = "ambient"
        server = McpHttpServer(_empty_registry(), config)
        handle = server.start()
        try:
            assert _wait_reachable("127.0.0.1", handle.port, budget=1.0), (
                f"Ambient listener on port {handle.port} unreachable after .start() returned"
            )
        finally:
            handle.shutdown()

    @pytest.mark.parametrize("round_", range(5))
    def test_repeated_start_shutdown_cycles(self, round_):
        """Starting/stopping several times must remain reachable every time."""
        config = McpHttpConfig(port=0, server_name=f"reach-py-cycle-{round_}")
        server = McpHttpServer(_empty_registry(), config)
        handle = server.start()
        try:
            assert _wait_reachable("127.0.0.1", handle.port, budget=1.0), f"cycle {round_}: listener unreachable"
        finally:
            handle.shutdown()


class TestGILPressure:
    """Regression guard: #303 was a scheduling-starvation issue.

    Simulate Python threads holding the GIL in tight loops while the
    server starts. Without the Dedicated spawn mode an ambient
    tokio-spawned accept loop would be starved and the probe would fail.
    """

    def _gil_burner(self, stop: threading.Event):
        """Burn CPU while holding the GIL most of the time."""
        x = 0
        while not stop.is_set():
            for _ in range(1000):
                x = (x * 1_000_003 + 7) & 0xFFFF_FFFF

    def test_dedicated_survives_gil_pressure(self):
        """Dedicated mode must remain reachable even with GIL pressure."""
        stop = threading.Event()
        threads = [threading.Thread(target=self._gil_burner, args=(stop,), daemon=True) for _ in range(4)]
        for t in threads:
            t.start()
        try:
            # Start server *after* GIL burners are active.
            time.sleep(0.05)
            config = McpHttpConfig(port=0, server_name="reach-py-gil")
            server = McpHttpServer(_empty_registry(), config)
            handle = server.start()
            try:
                # Give the probe a generous budget — 2s — because GIL
                # pressure slows everything.
                assert _wait_reachable("127.0.0.1", handle.port, budget=2.0), (
                    "Dedicated listener unreachable under GIL pressure"
                )
            finally:
                handle.shutdown()
        finally:
            stop.set()
            for t in threads:
                t.join(timeout=1.0)


class TestGatewayReachable:
    """Gateway port, when the handle reports is_gateway=true, must be reachable."""

    def test_gateway_listener_reachable_when_won(self, tmp_path):
        """If we win the gateway election, the gateway port must answer."""
        # Use an ephemeral gateway port via a pre-bound-then-released socket.
        # Pre-binding to :0, reading the port, then closing lets us claim a
        # port that almost certainly is free for a moment.
        probe = socket.socket()
        probe.bind(("127.0.0.1", 0))
        gateway_port = probe.getsockname()[1]
        probe.close()
        # Short sleep to let Windows release the port fully.
        time.sleep(0.05)

        config = McpHttpConfig(port=0, server_name="reach-py-gateway")
        config.gateway_port = gateway_port
        config.dcc_type = "reach-test"
        config.registry_dir = str(tmp_path)

        server = McpHttpServer(_empty_registry(), config)
        handle = server.start()
        try:
            # We should have won the gateway election.
            if handle.is_gateway:
                assert _wait_reachable("127.0.0.1", gateway_port, budget=2.0), (
                    f"handle.is_gateway=True but gateway port {gateway_port} "
                    "is unreachable — exactly the #303 symptom this test guards"
                )
            else:
                # Something else grabbed the port between bind probe and
                # McpHttpServer.start(); that's a flaky OS scheduling and
                # still a valid outcome for this invariant.
                pytest.skip(f"Race: port {gateway_port} taken before gateway election")
        finally:
            handle.shutdown()
