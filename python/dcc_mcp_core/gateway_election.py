"""Generic gateway failover election for any DCC MCP server.

When the current gateway instance becomes unreachable, non-gateway instances
automatically run a first-wins election to take over and maintain service
availability.

This module is DCC-agnostic. Maya, Blender, Unreal and any future adapter can
use :class:`DccGatewayElection` without writing their own election logic.

Configuration via environment variables
----------------------------------------
- ``DCC_MCP_GATEWAY_PROBE_INTERVAL`` — seconds between health probes (default 5)
- ``DCC_MCP_GATEWAY_PROBE_TIMEOUT``  — timeout per probe in seconds (default 2)
- ``DCC_MCP_GATEWAY_PROBE_FAILURES`` — consecutive failures before election (default 3)

Usage example::

    from dcc_mcp_core.gateway_election import DccGatewayElection

    class BlenderMcpServer:
        def start(self):
            self._handle = self._server.start()
            if self._enable_gateway_failover:
                self._election = DccGatewayElection(dcc_name="blender", server=self)
                self._election.start()
            return self._handle

        def stop(self):
            if self._election:
                self._election.stop()
            self._handle.shutdown()
"""

# Import future modules
from __future__ import annotations

import contextlib

# Import built-in modules
import logging
import os
import socket
import threading
from typing import Any

logger = logging.getLogger(__name__)

_PROBE_INTERVAL = int(os.environ.get("DCC_MCP_GATEWAY_PROBE_INTERVAL", "5"))
_PROBE_TIMEOUT = float(os.environ.get("DCC_MCP_GATEWAY_PROBE_TIMEOUT", "2"))
_PROBE_FAILURES = int(os.environ.get("DCC_MCP_GATEWAY_PROBE_FAILURES", "3"))
_GATEWAY_HOST = "127.0.0.1"
_DEFAULT_GATEWAY_PORT = 9765


class DccGatewayElection:
    """Manages automatic gateway election when the current gateway fails.

    Runs a background daemon thread that:
    1. Periodically probes the gateway's ``/health`` HTTP endpoint
    2. Counts consecutive failures
    3. Attempts a first-wins socket bind when failures exceed the threshold
    4. Signals the server to upgrade to gateway mode on success

    This class is DCC-agnostic. Pass ``dcc_name`` for log message labelling only.

    Example::

        election = DccGatewayElection(dcc_name="blender", server=blender_server)
        election.start()
        # ... runs in background ...
        election.stop()

    Args:
        dcc_name: Short DCC identifier for log messages (e.g. ``"blender"``).
        server: The DCC server instance. Must expose:
            - ``is_gateway: bool`` property
            - ``is_running: bool`` property
            - ``_handle`` attribute (the McpServerHandle)
        gateway_host: Gateway bind address (default ``"127.0.0.1"``).
        gateway_port: Gateway port to compete for (default ``9765``).
        probe_interval: Seconds between health probes (default from env var).
        probe_timeout: Timeout per probe in seconds (default from env var).
        probe_failures: Consecutive failures before attempting election
            (default from env var).

    """

    def __init__(
        self,
        dcc_name: str,
        server: Any,
        gateway_host: str = _GATEWAY_HOST,
        gateway_port: int = _DEFAULT_GATEWAY_PORT,
        probe_interval: int = _PROBE_INTERVAL,
        probe_timeout: float = _PROBE_TIMEOUT,
        probe_failures: int = _PROBE_FAILURES,
    ) -> None:
        self._dcc_name = dcc_name
        self._server = server
        self._gateway_host = gateway_host
        self._gateway_port = gateway_port
        self._probe_interval = probe_interval
        self._probe_timeout = probe_timeout
        self._probe_failures = probe_failures

        self._thread: threading.Thread | None = None
        self._stop_event = threading.Event()
        self._consecutive_failures = 0
        self._is_running = False
        self._lock = threading.Lock()

    # ── properties ────────────────────────────────────────────────────────────

    @property
    def is_running(self) -> bool:
        """Whether the election thread is active."""
        with self._lock:
            return self._is_running

    @property
    def consecutive_failures(self) -> int:
        """Current consecutive gateway probe failure count."""
        return self._consecutive_failures

    # ── lifecycle ─────────────────────────────────────────────────────────────

    def start(self) -> None:
        """Start the background gateway election thread.

        Safe to call multiple times — will not spawn duplicate threads.
        """
        with self._lock:
            if self._is_running:
                logger.warning("[%s] GatewayElection already running", self._dcc_name)
                return
            self._is_running = True
            self._stop_event.clear()

        self._thread = threading.Thread(
            target=self._run_election_loop,
            daemon=True,
            name=f"dcc-mcp-{self._dcc_name}-gateway-election",
        )
        self._thread.start()
        logger.info("[%s] GatewayElection thread started", self._dcc_name)

    def stop(self) -> None:
        """Gracefully stop the gateway election thread.

        Signals the thread to exit and waits up to 5 seconds.
        Safe to call even if not running.
        """
        with self._lock:
            if not self._is_running:
                return
            self._is_running = False

        self._stop_event.set()

        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=5.0)
            if self._thread.is_alive():
                logger.warning("[%s] GatewayElection thread did not stop gracefully", self._dcc_name)

        logger.info("[%s] GatewayElection thread stopped", self._dcc_name)

    # ── internal loop ─────────────────────────────────────────────────────────

    def _run_election_loop(self) -> None:
        """Run the gateway health probe loop and attempt election on failure."""
        logger.debug(
            "[%s] Election loop started: interval=%ds timeout=%ds failures=%d",
            self._dcc_name,
            self._probe_interval,
            self._probe_timeout,
            self._probe_failures,
        )

        while not self._stop_event.is_set():
            try:
                if self._server.is_gateway:
                    # We are the gateway, nothing to do
                    self._consecutive_failures = 0
                else:
                    if self._probe_gateway():
                        self._consecutive_failures = 0
                    else:
                        self._consecutive_failures += 1
                        logger.debug(
                            "[%s] Gateway probe failed (%d/%d)",
                            self._dcc_name,
                            self._consecutive_failures,
                            self._probe_failures,
                        )

                        if self._consecutive_failures >= self._probe_failures:
                            logger.warning(
                                "[%s] Gateway unreachable for %d probes, attempting election…",
                                self._dcc_name,
                                self._consecutive_failures,
                            )
                            if self._attempt_election():
                                logger.info("[%s] Successfully promoted to gateway!", self._dcc_name)
                                self._consecutive_failures = 0
            except Exception as exc:
                logger.error("[%s] Unexpected error in election loop: %s", self._dcc_name, exc)

            self._stop_event.wait(self._probe_interval)

    def _probe_gateway(self) -> bool:
        """HTTP GET /health probe against the gateway endpoint.

        Returns:
            ``True`` if the gateway responds with HTTP 200.

        """
        try:
            import urllib.request

            url = f"http://{self._gateway_host}:{self._gateway_port}/health"
            req = urllib.request.Request(url, method="GET")
            with urllib.request.urlopen(req, timeout=self._probe_timeout) as resp:
                return resp.status == 200
        except Exception:
            return False

    def _attempt_election(self) -> bool:
        """Try to bind the gateway port (first-wins mutual exclusion).

        Uses a plain TCP socket with ``SO_REUSEADDR`` explicitly disabled so
        only one process can bind at a time.

        Returns:
            ``True`` if this instance won and should upgrade to gateway.

        """
        sock = None
        try:
            sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            # Disable SO_REUSEADDR for exclusive binding (first-wins)
            sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 0)
            sock.bind((self._gateway_host, self._gateway_port))
            sock.listen(1)
            logger.info(
                "[%s] Bound gateway port %s:%d — promoting to gateway",
                self._dcc_name,
                self._gateway_host,
                self._gateway_port,
            )
            sock.close()
            sock = None
            self._upgrade_to_gateway()
            return True
        except OSError:
            # Another process already bound — we lost the election
            return False
        except Exception as exc:
            logger.error("[%s] Unexpected error during election: %s", self._dcc_name, exc)
            return False
        finally:
            if sock is not None:
                with contextlib.suppress(Exception):
                    sock.close()

    def _upgrade_to_gateway(self) -> None:
        """Signal the server to upgrade to gateway mode after winning election.

        Sub-classes or callers can override this hook for DCC-specific promotion
        logic. The default implementation logs the intent.
        """
        logger.info(
            "[%s] Gateway promotion signal sent (server will re-bind on next start)",
            self._dcc_name,
        )

    def get_status(self) -> dict:
        """Return election status information.

        Returns:
            Dict with keys ``running``, ``consecutive_failures``,
            ``gateway_host``, ``gateway_port``.

        """
        return {
            "running": self.is_running,
            "consecutive_failures": self._consecutive_failures,
            "gateway_host": self._gateway_host,
            "gateway_port": self._gateway_port,
        }

    def __repr__(self) -> str:
        status = "running" if self.is_running else "stopped"
        return f"DccGatewayElection(dcc={self._dcc_name!r}, status={status}, failures={self._consecutive_failures})"
