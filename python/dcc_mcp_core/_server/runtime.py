"""Runtime collaborator for :class:`dcc_mcp_core.server_base.DccServerBase`."""

from __future__ import annotations

import logging
from typing import Any

from dcc_mcp_core.dcc_server import register_diagnostic_handlers
from dcc_mcp_core.dcc_server import register_diagnostic_mcp_tools
from dcc_mcp_core.gateway_election import DccGatewayElection

logger = logging.getLogger(__name__)


class ServerRuntimeController:
    """Owns start/stop helpers that are not part of the public interface."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    def start_gateway_election_if_needed(self) -> None:
        owner = self._owner
        gateway_port = getattr(owner._config, "gateway_port", 0)
        if not (owner._enable_gateway_failover and gateway_port and gateway_port > 0):
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
