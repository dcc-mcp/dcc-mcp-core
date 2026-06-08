"""Execution bridge binding controller for :class:`DccServerBase`.

Extracted from ``server_base.py`` (PIP-688) to own host-execution-bridge
and in-process-executor wiring, sandbox attachment, and HTTP dispatcher
attachment.

``DccServerBase`` keeps thin public wrappers that delegate here.
"""

from __future__ import annotations

import logging
from typing import Any

from dcc_mcp_core._core import SandboxContext
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core.script_execution import allow_script_materialization_root

logger = logging.getLogger(__name__)


class ExecutionBridgeBinder:
    """Owns execution-bridge and in-process-executor wiring for one server."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    # -- sandbox ---------------------------------------------------------------

    def _attach_sandbox_to_bridge(self, bridge: HostExecutionBridge) -> None:
        """Forward ``McpHttpConfig.sandbox_policy`` to the execution bridge (#1001)."""
        owner = self._owner
        policy = getattr(owner._config, "sandbox_policy", None)
        if policy is not None:
            try:
                bridge.script_materialization_root = allow_script_materialization_root(
                    policy,
                    root=bridge.script_materialization_root,
                )
            except Exception as exc:
                logger.warning(
                    "[%s] failed to allow script materialization root in sandbox: %s",
                    owner._dcc_name,
                    exc,
                )
            bridge.sandbox_context = SandboxContext(policy)

    # -- HTTP dispatcher -------------------------------------------------------

    def _attach_host_dispatcher_to_http(self, dispatcher: Any | None) -> bool:
        """Attach a host queue dispatcher to HTTP ``tools/call`` routing."""
        owner = self._owner
        if dispatcher is None:
            return False
        attach = getattr(owner._server, "attach_dispatcher", None)
        if not callable(attach):
            return False
        try:
            attach(dispatcher)
            return True
        except RuntimeError as exc:
            if "already called" in str(exc):
                logger.debug("[%s] host dispatcher already attached: %s", owner._dcc_name, exc)
                return False
            logger.warning("[%s] attach_dispatcher failed: %s", owner._dcc_name, exc)
            return False
        except TypeError as exc:
            logger.debug("[%s] dispatcher is not an HTTP host dispatcher: %s", owner._dcc_name, exc)
            return False
        except Exception as exc:
            logger.warning("[%s] attach_dispatcher failed: %s", owner._dcc_name, exc)
            return False

    # -- public wiring ---------------------------------------------------------

    def register_host_execution_bridge(self, bridge: HostExecutionBridge) -> None:
        """Wire the adapter-facing host execution bridge.

        New embedded adapters should keep a single :class:`HostExecutionBridge`
        for both direct host callables and in-process skill scripts. When the
        bridge carries a Rust-backed host queue dispatcher, this method also
        attaches it to ``McpHttpServer.attach_dispatcher`` so main-affinity
        MCP/REST calls share the same host-thread route.
        """
        owner = self._owner
        self._attach_sandbox_to_bridge(bridge)
        owner._execution_bridge = bridge
        owner._dcc_dispatcher = bridge.dispatcher
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            owner._server.set_in_process_executor(bridge.as_inprocess_executor())
            owner._inprocess_executor_registered = True
            host_dispatcher_attached = self._attach_host_dispatcher_to_http(host_dispatcher)
            logger.info(
                "[%s] Host execution bridge registered (dispatcher=%s, host_dispatcher_attached=%s)",
                owner._dcc_name,
                type(bridge.dispatcher).__name__ if bridge.dispatcher is not None else "inline",
                host_dispatcher_attached,
            )
        except Exception as exc:
            logger.warning(
                "[%s] register_host_execution_bridge failed: %s",
                owner._dcc_name,
                exc,
            )

    def register_inprocess_executor(
        self,
        dispatcher: BaseDccCallableDispatcher | None = None,
    ) -> None:
        """Wire the standard in-process Python skill executor.

        Must be called **before** any
        :meth:`register_builtin_actions` so all subsequently loaded
        skills register their handlers against the in-process path
        (avoids the timing race documented in issue #464/#465).
        """
        owner = self._owner
        owner._dcc_dispatcher = dispatcher
        bridge = HostExecutionBridge(dispatcher=dispatcher)
        self._attach_sandbox_to_bridge(bridge)
        owner._execution_bridge = bridge
        executor = bridge.as_inprocess_executor()
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            owner._server.set_in_process_executor(executor)
            owner._inprocess_executor_registered = True
            host_dispatcher_attached = self._attach_host_dispatcher_to_http(host_dispatcher)
            logger.info(
                "[%s] In-process executor registered (dispatcher=%s, host_dispatcher_attached=%s)",
                owner._dcc_name,
                type(dispatcher).__name__ if dispatcher is not None else "inline",
                host_dispatcher_attached,
            )
        except Exception as exc:
            logger.warning(
                "[%s] register_inprocess_executor failed: %s",
                owner._dcc_name,
                exc,
            )
