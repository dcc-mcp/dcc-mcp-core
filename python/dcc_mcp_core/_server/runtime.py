"""Runtime collaborator for :class:`dcc_mcp_core.server_base.DccServerBase`."""

from __future__ import annotations

import logging
from typing import Any

from dcc_mcp_core._server.gateway_guardian import GatewayDaemonGuardian
from dcc_mcp_core._server.gateway_guardian import ensure_gateway_daemon
from dcc_mcp_core.gateway_election import DccGatewayElection

logger = logging.getLogger(__name__)


class ServerRuntimeController:
    """Owns start/stop helpers that are not part of the public interface."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def ensure_gateway_daemon_if_needed(self) -> bool:
        owner = self._owner
        gateway_port = int(getattr(owner._config, "gateway_port", 0) or 0)
        if gateway_port <= 0:
            owner._gateway_runtime_mode = "not_configured"
            return False
        if not bool(getattr(owner, "_enable_gateway_failover", False)):
            owner._gateway_runtime_mode = "failover_disabled_by_adapter"
            return False

        result = ensure_gateway_daemon(
            gateway_host="127.0.0.1",
            gateway_port=gateway_port,
            registry_dir=getattr(owner._config, "registry_dir", None),
            dcc_type=str(getattr(owner, "_dcc_name", "dcc")),
        )
        owner._gateway_daemon_status = dict(result)
        if result.get("ok"):
            owner._gateway_runtime_mode = "daemon-backed"
            return True
        owner._gateway_runtime_mode = "embedded-fallback"
        logger.warning(
            "[%s] Gateway daemon ensure failed (%s), falling back to embedded election",
            owner._dcc_name,
            result.get("reason", "unknown"),
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
        if getattr(owner, "_gateway_guardian", None) is not None:
            return

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
            logger.info("[%s] Gateway daemon guardian enabled", owner._dcc_name)

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
