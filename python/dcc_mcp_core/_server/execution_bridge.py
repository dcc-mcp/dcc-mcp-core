"""Execution bridge binding controller for :class:`DccServerBase`.

Extracted from ``server_base.py`` (PIP-688) to own host-execution-bridge
and in-process-executor wiring, sandbox attachment, and HTTP dispatcher
attachment.

``DccServerBase`` keeps thin public wrappers that delegate here.
"""

from __future__ import annotations

import logging
from typing import Any

from dcc_mcp_core._runtime.core_availability import is_core_extension_available
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core.script_execution import allow_script_materialization_root

try:
    from dcc_mcp_core._core import SandboxContext
except ImportError:

    class SandboxContext:  # type: ignore[no-redef]
        """Fallback stub used when the native sandbox context is unavailable."""

        def __init__(self, *args: Any, **kwargs: Any) -> None:
            raise NotImplementedError("SandboxContext requires dcc_mcp_core._core")


logger = logging.getLogger(__name__)


def _sandbox_context(policy: Any) -> Any:
    if not is_core_extension_available():
        return None
    from dcc_mcp_core._core import SandboxContext

    return SandboxContext(policy)


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
            bridge.sandbox_context = _sandbox_context(policy)

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

    # -- package lifecycle ----------------------------------------------------

    def _unsubscribe_skill_unloaded(self) -> None:
        owner = self._owner
        subscription = getattr(owner, "_inprocess_unload_subscription", None)
        if not subscription:
            return
        event_bus, subscriber_id = subscription
        try:
            event_bus.unsubscribe("skill.unloaded", subscriber_id)
        except Exception as exc:
            logger.debug("[%s] skill.unloaded unsubscribe failed: %s", owner._dcc_name, exc)
        owner._inprocess_unload_subscription = None

    def _bind_package_lifecycle(self, bridge: HostExecutionBridge) -> None:
        owner = self._owner
        previous_hook = getattr(owner, "_inprocess_cleanup_quit_hook", None)
        if previous_hook is not None:
            owner.unregister_quit_hook(previous_hook)

        cleanup_hook = bridge.shutdown_script_execution
        owner._inprocess_cleanup_quit_hook = cleanup_hook
        self.prepare_start()

        self._unsubscribe_skill_unloaded()
        event_bus_getter = getattr(owner._server, "event_bus", None)
        if not callable(event_bus_getter):
            logger.debug("[%s] catalog EventBus is unavailable; unload cleanup is not bound", owner._dcc_name)
            return
        try:
            event_bus = event_bus_getter()

            def _on_skill_unloaded(event: Any) -> None:
                attributes = event.get("attributes", {}) if isinstance(event, dict) else {}
                skill_path = attributes.get("skill_path") if isinstance(attributes, dict) else None
                if skill_path:
                    bridge.clear_script_packages_under(str(skill_path))

            subscriber_id = event_bus.subscribe("skill.unloaded", _on_skill_unloaded)
            owner._inprocess_unload_subscription = (event_bus, subscriber_id)
        except Exception as exc:
            logger.debug("[%s] skill.unloaded cleanup subscription failed: %s", owner._dcc_name, exc)

    def prepare_start(self) -> None:
        """Ensure package cleanup runs once for the next start/stop lifecycle."""
        owner = self._owner
        bridge = getattr(owner, "_execution_bridge", None)
        resume = getattr(bridge, "resume_script_execution", None)
        if callable(resume):
            resume()
        hook = getattr(owner, "_inprocess_cleanup_quit_hook", None)
        if hook is None:
            return
        hooks = getattr(owner, "_quit_hooks", ())
        if not any(registered is hook for registered in hooks):
            owner.register_quit_hook(hook)

    # -- public wiring ---------------------------------------------------------

    def _active_bridge_is_same(self, bridge: HostExecutionBridge) -> bool:
        owner = self._owner
        if not getattr(owner, "_inprocess_executor_registered", False):
            return False
        if getattr(owner, "_execution_bridge", None) is bridge:
            return True
        raise RuntimeError(
            "An in-process execution bridge is already active; create a new "
            "DccServerBase instance to bind a different bridge."
        )

    def register_host_execution_bridge(self, bridge: HostExecutionBridge) -> None:
        """Wire the adapter-facing host execution bridge.

        New embedded adapters should keep a single :class:`HostExecutionBridge`
        for both direct host callables and in-process skill scripts. When the
        bridge carries a Rust-backed host queue dispatcher, this method also
        attaches it to ``McpHttpServer.attach_dispatcher`` so main-affinity
        MCP/REST calls share the same host-thread route.
        """
        owner = self._owner
        if self._active_bridge_is_same(bridge):
            return
        self._attach_sandbox_to_bridge(bridge)
        owner._execution_bridge = bridge
        owner._dcc_dispatcher = bridge.dispatcher
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            owner._server.set_in_process_executor(bridge.as_inprocess_executor())
            owner._inprocess_executor_registered = True
            self._bind_package_lifecycle(bridge)
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
        if getattr(owner, "_inprocess_executor_registered", False):
            active_bridge = getattr(owner, "_execution_bridge", None)
            if isinstance(active_bridge, HostExecutionBridge) and active_bridge.dispatcher is dispatcher:
                return
            raise RuntimeError(
                "An in-process executor is already registered with a different dispatcher; "
                "create a new DccServerBase instance to replace it."
            )
        bridge = HostExecutionBridge(dispatcher=dispatcher)
        owner._dcc_dispatcher = dispatcher
        self._attach_sandbox_to_bridge(bridge)
        owner._execution_bridge = bridge
        executor = bridge.as_inprocess_executor()
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            owner._server.set_in_process_executor(executor)
            owner._inprocess_executor_registered = True
            self._bind_package_lifecycle(bridge)
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
