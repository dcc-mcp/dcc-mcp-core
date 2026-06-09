"""Runtime collaborator for :class:`dcc_mcp_core.server_base.DccServerBase`."""

from __future__ import annotations

import logging
import os
import threading
import time
from typing import Any

from dcc_mcp_core._server.gateway_guardian import GatewayDaemonGuardian
from dcc_mcp_core._server.gateway_guardian import ensure_gateway_daemon
from dcc_mcp_core.gateway_election import DccGatewayElection

logger = logging.getLogger(__name__)


class ServerRuntimeController:
    """Owns start/stop helpers that are not part of the public interface."""

    _WATCHDOG_INTERVAL_SECS: float = 60.0

    def __init__(self, owner: Any) -> None:
        self._owner = owner
        self._guardian_watchdog_stop = threading.Event()
        self._guardian_watchdog_thread: threading.Thread | None = None

    def ensure_gateway_daemon_if_needed(self) -> bool:
        owner = self._owner
        gateway_port = int(getattr(owner._config, "gateway_port", 0) or 0)
        if gateway_port <= 0:
            owner._gateway_runtime_mode = "not_configured"
            return False
        if not bool(getattr(owner, "_enable_gateway_failover", False)):
            owner._gateway_runtime_mode = "failover_disabled_by_adapter"
            return False

        strict_gateway = os.environ.get("DCC_MCP_STRICT_GATEWAY", "").strip().lower() in {"1", "true", "yes", "on"}

        dcc_type = str(getattr(owner, "_dcc_name", "dcc"))
        registry_dir = getattr(owner._config, "registry_dir", None)
        retry_count = 2  # additional retries after first attempt
        retry_interval_secs = 2.0

        result: dict[str, Any] = {}
        for attempt in range(1 + retry_count):
            result = ensure_gateway_daemon(
                gateway_host="127.0.0.1",
                gateway_port=gateway_port,
                registry_dir=registry_dir,
                dcc_type=dcc_type,
            )
            owner._gateway_daemon_status = dict(result)
            if result.get("ok"):
                owner._gateway_runtime_mode = "daemon-backed"
                if attempt > 0:
                    logger.info(
                        "[%s] Gateway daemon ensure succeeded on retry %d/%d",
                        owner._dcc_name,
                        attempt,
                        retry_count,
                    )
                return True
            if attempt < retry_count:
                logger.warning(
                    "[%s] Gateway daemon ensure attempt %d/%d failed (%s), retrying in %.1fs...",
                    owner._dcc_name,
                    attempt + 1,
                    1 + retry_count,
                    result.get("reason", "unknown"),
                    retry_interval_secs,
                )
                time.sleep(retry_interval_secs)

        # All attempts failed.
        reason = result.get("reason", "unknown") if result else "unknown"
        gateway_daemon_status = dict(result) if result else {}

        if strict_gateway:
            raise RuntimeError(
                f"[{owner._dcc_name}] DCC_MCP_STRICT_GATEWAY is enabled and "
                f"gateway daemon ensure failed after {1 + retry_count} attempts: "
                f"{reason}. gateway_daemon_status={gateway_daemon_status!r}"
            )

        owner._gateway_runtime_mode = "embedded-fallback"
        owner._gateway_daemon_status = gateway_daemon_status
        logger.warning(
            "[%s] Gateway daemon ensure failed after %d attempts (%s), falling back to embedded election",
            owner._dcc_name,
            1 + retry_count,
            reason,
        )
        return False

    def start_gateway_guardian_if_needed(self) -> None:
        owner = self._owner
        gateway_port = int(getattr(owner._config, "gateway_port", 0) or 0)
        if gateway_port <= 0:
            return
        if not bool(getattr(owner, "_enable_gateway_failover", False)):
            return
        if getattr(owner, "_gateway_runtime_mode", "") != "daemon-backed":
            return

        existing = getattr(owner, "_gateway_guardian", None)
        if existing is not None:
            if existing.status().get("guardian_running", False):
                return  # Already healthy
            logger.warning(
                "[%s] Gateway guardian thread is dead, replacing...",
                owner._dcc_name,
            )
            owner._gateway_guardian = None

        def _record_status(status: dict[str, Any]) -> None:
            owner._gateway_daemon_status = dict(status)

        guardian = GatewayDaemonGuardian(
            gateway_host="127.0.0.1",
            gateway_port=gateway_port,
            registry_dir=getattr(owner._config, "registry_dir", None),
            dcc_type=str(getattr(owner, "_dcc_name", "dcc")),
            status_callback=_record_status,
        )
        if guardian.start():
            owner._gateway_guardian = guardian
            owner._gateway_daemon_status = guardian.status()
            owner._publish_gateway_runtime_metadata()
            logger.info("[%s] Gateway daemon guardian enabled", owner._dcc_name)
            self._start_guardian_watchdog()

    def start_gateway_election_if_needed(self) -> None:
        owner = self._owner
        gateway_port = getattr(owner._config, "gateway_port", 0)
        if not (owner._enable_gateway_failover and gateway_port and gateway_port > 0):
            return
        if getattr(owner, "_gateway_runtime_mode", "") == "daemon-backed":
            return
        if owner._gateway_election is not None:
            return
        election = None
        try:
            election = DccGatewayElection(
                dcc_name=owner._dcc_name,
                server=owner,
                gateway_port=gateway_port,
            )
            election.start()
            owner._gateway_election = election
            logger.info("[%s] Gateway failover election enabled", owner._dcc_name)
        except Exception as exc:
            owner._gateway_election = None
            logger.warning("[%s] Failed to start gateway election: %s", owner._dcc_name, exc)

    def stop_gateway_election(self) -> None:
        owner = self._owner
        if owner._gateway_election is None:
            return
        try:
            owner._gateway_election.stop()
        except Exception as exc:
            logger.warning("[%s] Error stopping gateway election: %s", owner._dcc_name, exc)
        finally:
            owner._gateway_election = None

    def stop_gateway_guardian(self) -> None:
        self._stop_guardian_watchdog()
        owner = self._owner
        guardian = getattr(owner, "_gateway_guardian", None)
        if guardian is None:
            return
        try:
            guardian.stop()
        except Exception as exc:
            logger.warning("[%s] Error stopping gateway guardian: %s", owner._dcc_name, exc)
        finally:
            owner._gateway_guardian = None
            owner._publish_gateway_runtime_metadata()

    def _guardian_watchdog_loop(self) -> None:
        """Periodically check guardian liveness and restart if crashed."""
        while not self._guardian_watchdog_stop.wait(self._WATCHDOG_INTERVAL_SECS):
            try:
                owner = self._owner
                guardian = getattr(owner, "_gateway_guardian", None)
                if guardian is None:
                    continue
                status = guardian.status()
                if not status.get("guardian_running", False):
                    logger.warning(
                        "[%s] Guardian watchdog detected dead guardian, restarting...",
                        owner._dcc_name,
                    )
                    self.start_gateway_guardian_if_needed()
            except Exception:
                logger.exception("[%s] Guardian watchdog check failed", owner._dcc_name)

    def _start_guardian_watchdog(self) -> None:
        if self._guardian_watchdog_thread is not None and self._guardian_watchdog_thread.is_alive():
            return
        self._guardian_watchdog_stop.clear()
        self._guardian_watchdog_thread = threading.Thread(
            target=self._guardian_watchdog_loop,
            name=f"dcc-mcp-guardian-watchdog-{self._owner._dcc_name}",
            daemon=True,
        )
        self._guardian_watchdog_thread.start()

    def _stop_guardian_watchdog(self) -> None:
        self._guardian_watchdog_stop.set()
        if self._guardian_watchdog_thread is not None:
            self._guardian_watchdog_thread.join(timeout=1.0)
            self._guardian_watchdog_thread = None

    def shutdown_server_handle(self) -> None:
        owner = self._owner
        if owner._handle is None:
            return
        try:
            owner._handle.shutdown()
        except Exception as exc:
            logger.warning("[%s] Error stopping server: %s", owner._dcc_name, exc)
        finally:
            owner._handle = None
        logger.info("[%s] MCP server stopped", owner._dcc_name)
