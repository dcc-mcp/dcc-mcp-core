"""Generic DCC MCP server base class.

``DccServerBase`` centralises the shared boilerplate for DCC adapters:
skill-path discovery, MCP server wiring, hot reload, gateway failover, and
server lifecycle management. Adapters usually only construct
``DccServerOptions`` and optionally override ``_version_string`` or
``_upgrade_to_gateway``.

The class delegates to four seam controllers (PIP-688):
- :class:`~dcc_mcp_core._server.skill_discovery.SkillDiscoveryController`
- :class:`~dcc_mcp_core._server.execution_bridge.ExecutionBridgeBinder`
- :class:`~dcc_mcp_core._server.lifecycle_controller.LifecycleController`
- :class:`~dcc_mcp_core._server.observability_facade.ObservabilityFacade`
"""

from __future__ import annotations

import logging
import sys
from typing import Any
from typing import Callable

from dcc_mcp_core import _core
from dcc_mcp_core._core import create_skill_server
from dcc_mcp_core._lifecycle_events import LifecycleEventDispatcher
from dcc_mcp_core._server import ExecutionBridgeBinder
from dcc_mcp_core._server import LifecycleController
from dcc_mcp_core._server import ObservabilityFacade
from dcc_mcp_core._server import SkillDiscoveryController
from dcc_mcp_core._server import SkillQueryClient
from dcc_mcp_core._server import WindowResolver
from dcc_mcp_core._server import build_mcp_http_config
from dcc_mcp_core._server import collect_context_metadata_from_env
from dcc_mcp_core._server import resolve_diagnostics_state
from dcc_mcp_core._server import resolve_execution_binding
from dcc_mcp_core._server import resolve_observability_flags
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.options import DccServerOptions

_PKG_VERSION: str = getattr(_core, "__version__", "0.0.0-dev")

logger = logging.getLogger(__name__)


class DccServerBase:
    """Base MCP server for any DCC application.

    Pass a :class:`~dcc_mcp_core._server.options.DccServerOptions` instance
    (typically from :meth:`DccServerOptions.from_env`). All generic skill
    management, hot-reload, and gateway election logic lives here so DCC
    adapters stay thin.

    Args:
        options: Fully-configured :class:`~dcc_mcp_core._server.options.DccServerOptions`.

    """

    def __init__(self, options: DccServerOptions) -> None:
        self._init_from_options(options)

    def _init_from_options(self, options: DccServerOptions) -> None:
        """Wire all collaborators from a fully-resolved :class:`DccServerOptions`.

        This is the single real constructor path; ``__init__`` delegates here.
        """
        self._options = options
        self._dcc_name = options.dcc_name
        self._builtin_skills_dir = options.builtin_skills_dir
        self._handle: Any | None = None
        self._enable_gateway_failover = options.gateway.enable_failover

        # Observability flags (env var can override at runtime).
        obs = resolve_observability_flags(options.observability)
        self._enable_file_logging: bool = obs.file_logging
        self._enable_job_persistence: bool = obs.job_persistence
        self._enable_telemetry: bool = obs.telemetry

        # DCC diagnostic context
        diag = resolve_diagnostics_state(options.diagnostics)
        self._dcc_pid: int = diag.dcc_pid
        self._dcc_window_title: str | None = diag.window_title
        self._dcc_window_handle: int | None = diag.window_handle

        # Resolve execution mode from the tagged union
        execution = resolve_execution_binding(options.execution.mode)
        self._execution_bridge: HostExecutionBridge | None = execution.bridge
        self._dcc_dispatcher: BaseDccCallableDispatcher | None = execution.dispatcher
        self._standalone_main_thread: bool = execution.standalone_main_thread

        self._inprocess_executor_registered: bool = False
        self._cached_hwnd: int | None = None

        # --- Seam controllers (PIP-688) ------------------------------------------
        self._skill_discovery = SkillDiscoveryController(self)
        self._execution = ExecutionBridgeBinder(self)
        self._lifecycle_ctrl = LifecycleController(self)
        self._observability = ObservabilityFacade(self)

        # --- Lifecycle events dispatcher -----------------------------------------
        self._lifecycle_events = LifecycleEventDispatcher(
            options.dcc_name,
            lambda: getattr(self, "_lifecycle_hooks", None),
        )

        # --- File logging --------------------------------------------------------
        self._log_dir: str = self._init_file_logging(options.dcc_name)

        logger.info(
            "[%s] dcc-mcp-core %s (pid=%d, python=%s, platform=%s)",
            options.dcc_name,
            _PKG_VERSION,
            self._dcc_pid,
            "{}.{}.{}".format(*sys.version_info[:3]),
            sys.platform,
        )

        self._config = build_mcp_http_config(
            options,
            package_version=_PKG_VERSION,
            version_provider=self._version_string,
        )

        # --- Job persistence -----------------------------------------------------
        self._init_job_persistence(options.dcc_name)

        # Create the inner skill manager
        self._server: Any = create_skill_server(options.dcc_name, self._config)
        self._register_builtin_skills(options)

        # Wire execution bridge / dispatcher
        if execution.bridge is not None:
            self._get_execution().register_host_execution_bridge(execution.bridge)
        elif execution.dispatcher is not None:
            self._get_execution().register_inprocess_executor(execution.dispatcher)
        elif execution.register_inprocess_executor:
            self._get_execution().register_inprocess_executor(None)

        # Composed collaborators
        self._skill_client = SkillQueryClient(self._server, options.dcc_name)
        self._window_resolver = WindowResolver(
            dcc_name=options.dcc_name,
            dcc_pid=self._dcc_pid,
            dcc_window_handle=diag.window_handle,
            dcc_window_title=diag.window_title,
        )

        # Lazy-initialised helpers
        self._hot_reloader: Any | None = None
        self._gateway_election: Any | None = None
        self._gateway_guardian: Any | None = None
        self._gateway_runtime_mode: str = "unknown"
        self._gateway_daemon_status: dict[str, Any] = {}
        self._snapshot_provider: Any | None = diag.snapshot_provider
        self._quit_hooks: list[Callable[[], Any]] = []

    # --- seam controller accessors (lazy init for test compatibility, PIP-688) ---

    def _get_skill_discovery(self):
        ctrl = self.__dict__.get("_skill_discovery")
        if ctrl is None:
            ctrl = SkillDiscoveryController(self)
            self._skill_discovery = ctrl
        return ctrl

    def _get_execution(self):
        ctrl = self.__dict__.get("_execution")
        if ctrl is None:
            ctrl = ExecutionBridgeBinder(self)
            self._execution = ctrl
        return ctrl

    def _get_lifecycle_ctrl(self):
        ctrl = self.__dict__.get("_lifecycle_ctrl")
        if ctrl is None:
            ctrl = LifecycleController(self)
            self._lifecycle_ctrl = ctrl
        return ctrl

    def _get_observability(self):
        ctrl = self.__dict__.get("_observability")
        if ctrl is None:
            ctrl = ObservabilityFacade(self)
            self._observability = ctrl
        return ctrl

    def _register_builtin_skills(self, options: DccServerOptions) -> None:
        """Register standard built-in skills (diagnostics, introspect, etc)."""
        self._get_skill_discovery().register_builtin_skills(options)

    def register_adapter_instructions(self, instruction_set: Any) -> list[str]:
        """Register standard adapter instruction/capability resources."""
        return self._get_observability().register_adapter_instructions(instruction_set)

    def set_context_snapshot_provider(self, provider: Any | None) -> None:
        """Set an optional callable used to append post-tool context snapshots."""
        self._get_observability().set_context_snapshot_provider(provider)

    def append_context_snapshot(self, result: dict[str, Any], *, policy: Any | None = None) -> dict[str, Any]:
        """Attach the configured post-tool context snapshot to a result envelope."""
        return self._get_observability().append_context_snapshot(result, policy=policy)

    # --- MCP resources -----------------------------------------------------------

    def resources(self) -> Any:
        """Return the shared MCP ``ResourceHandle`` for this server."""
        return self._get_observability().resources()

    def register_resource_producer(self, scheme_or_uri: str, producer: Callable[[str], Any]) -> None:
        """Register a Python resource producer on the server resource handle."""
        self._get_observability().register_resource_producer(scheme_or_uri, producer)

    def set_scene_resource(self, snapshot: Any) -> None:
        """Publish ``snapshot`` as ``scene://current``."""
        self._get_observability().set_scene_resource(snapshot)

    def notify_resource_updated(self, uri: str) -> None:
        """Emit ``notifications/resources/updated`` for ``uri``."""
        self._get_observability().notify_resource_updated(uri)

    @staticmethod
    def _context_metadata_from_env(dcc_name: str) -> dict[str, str]:
        """Collect Rez-resolved context metadata for gateway discovery."""
        return collect_context_metadata_from_env(dcc_name)

    # --- observability helpers (delegated to ObservabilityFacade, PIP-688) --------

    def _init_file_logging(self, dcc_name: str) -> str:
        return self._get_observability().init_file_logging(dcc_name)

    def _init_job_persistence(self, dcc_name: str) -> None:
        self._get_observability().init_job_persistence(dcc_name)

    def _init_telemetry(self) -> None:
        self._get_observability().init_telemetry()

    # --- readiness publication (#1206) -------------------------------------------

    def set_readiness_probe(self, probe: Any) -> bool:
        """Publish a shared readiness probe to MCP and REST call surfaces."""
        return self._get_observability().set_readiness_probe(probe)

    # --- skill search path helpers -----------------------------------------------

    def collect_skill_search_paths(
        self,
        extra_paths: list[str] | None = None,
        include_bundled: bool = True,
        filter_existing: bool = False,
        include_admin_custom: bool = True,
    ) -> list[str]:
        """Build the ordered skill search path list for this DCC."""
        return self._get_skill_discovery().collect_skill_search_paths(
            extra_paths=extra_paths,
            include_bundled=include_bundled,
            filter_existing=filter_existing,
            include_admin_custom=include_admin_custom,
        )

    # --- skill registration -----------------------------------------------------

    def register_builtin_actions(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
        minimal_mode: MinimalModeConfig | None = None,
    ) -> None:
        """Discover and (optionally) progressively load skills."""
        self._get_skill_discovery().register_builtin_actions(
            extra_skill_paths=extra_skill_paths,
            include_bundled=include_bundled,
            minimal_mode=minimal_mode,
        )

    def reload_skill_paths(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
    ) -> int:
        """Re-discover skills after admin-UI skill paths changed (#1400)."""
        return self._get_skill_discovery().reload_skill_paths(
            extra_skill_paths=extra_skill_paths,
            include_bundled=include_bundled,
        )

    # --- observability properties ------------------------------------------------

    @property
    def log_dir(self) -> str:
        """Directory where rolling log files are written, or ``""`` if disabled."""
        return self._log_dir

    @property
    def observability_summary(self) -> dict[str, Any]:
        """Return a snapshot of the active observability features."""
        return self._get_observability().observability_summary

    # --- host execution bridge / in-process executor wiring (#599, #521) --------

    def register_host_execution_bridge(self, bridge: HostExecutionBridge) -> None:
        """Wire the adapter-facing host execution bridge."""
        self._get_execution().register_host_execution_bridge(bridge)

    def register_inprocess_executor(
        self,
        dispatcher: BaseDccCallableDispatcher | None = None,
    ) -> None:
        """Wire the standard in-process Python skill executor."""
        self._get_execution().register_inprocess_executor(dispatcher)

    # --- gateway & is_gateway ----------------------------------------------------

    @property
    def is_gateway(self) -> bool:
        """Whether this instance is currently the active gateway."""
        if self._handle is None:
            return False
        try:
            return bool(self._handle.is_gateway)
        except Exception:
            return False

    @property
    def gateway_url(self) -> str | None:
        """The gateway URL (e.g. ``http://127.0.0.1:9765/mcp``), or ``None``."""
        if self._handle is None:
            return None
        try:
            port = getattr(self._config, "gateway_port", 0)
            if port > 0 and self.is_gateway:
                return f"http://127.0.0.1:{port}/mcp"
        except Exception:
            pass
        return None

    # --- DCC instance context (PID / window handle / title) ---------------------

    @property
    def dcc_pid(self) -> int:
        """PID of the DCC application process hosting this server."""
        return self._dcc_pid

    @property
    def dcc_window_title(self) -> str | None:
        """Configured DCC window-title substring (``None`` if not set)."""
        return self._dcc_window_title

    @property
    def dcc_window_handle(self) -> int | None:
        """Pre-resolved native DCC window handle (``None`` if not set)."""
        return self._dcc_window_handle

    def _resolve_window_handle(self) -> int | None:
        """Resolve the DCC window handle from the available context."""
        hwnd = self._window_resolver.resolve()
        if hwnd is not None and self._cached_hwnd is None:
            self._cached_hwnd = hwnd
        return hwnd

    # --- skill query methods (delegated to SkillQueryClient, #486) ----------

    @property
    def registry(self) -> Any | None:
        return self._skill_client.registry

    def list_actions(self, dcc_name: str | None = None) -> list[Any]:
        return self._skill_client.list_actions(dcc_name)

    def list_skills(self) -> list[Any]:
        return self._skill_client.list_skills()

    def search_skills(
        self,
        query: str | None = None,
        tags: list[str] | None = None,
        dcc: str | None = None,
        scope: str | None = None,
        limit: int | None = None,
        *,
        session_id: str | None = None,
    ) -> list[Any]:
        """Search for skills by query, tags, DCC, scope, and/or limit."""
        initial_tags = list(tags) if tags is not None else None
        before = self.dispatch_lifecycle_event(
            "before_search",
            {
                "query": query,
                "tags": initial_tags,
                "dcc": dcc,
                "scope": scope,
                "limit": limit,
            },
            session_id=session_id,
        )
        effective_tags = before.get("tags", tags)
        if effective_tags is not None and not isinstance(effective_tags, list):
            effective_tags = list(effective_tags)
        results = self._skill_client.search_skills(
            query=before.get("query", query),
            tags=effective_tags,
            dcc=before.get("dcc", dcc),
            scope=before.get("scope", scope),
            limit=before.get("limit", limit),
        )
        self.dispatch_lifecycle_event(
            "after_search",
            {
                **before,
                "result_count": len(results),
                "zero_results": len(results) == 0,
            },
            session_id=session_id,
        )
        return results

    def load_skill(self, name: str) -> bool:
        return self._skill_client.load_skill(name)

    def get_skill(self, name: str) -> Any | None:
        return self._skill_client.get_skill(name)

    def load_skill_object(self, skill: Any) -> bool:
        return self._skill_client.load_skill_object(skill)

    def set_skill_load_transform(self, transform: Callable[[Any], Any] | None) -> bool:
        return self._skill_client.set_skill_load_transform(transform)

    def clear_skill_load_transform(self) -> bool:
        return self._skill_client.clear_skill_load_transform()

    def set_after_load_skill_hook(self, hook: Callable[[Any, list[str]], Any] | None) -> bool:
        return self._skill_client.set_after_load_skill_hook(hook)

    def clear_after_load_skill_hook(self) -> bool:
        return self._skill_client.clear_after_load_skill_hook()

    def set_after_unload_skill_hook(self, hook: Callable[[str, list[str]], Any] | None) -> bool:
        return self._skill_client.set_after_unload_skill_hook(hook)

    def clear_after_unload_skill_hook(self) -> bool:
        return self._skill_client.clear_after_unload_skill_hook()

    def set_after_group_change_hook(self, hook: Callable[[str, bool], Any] | None) -> bool:
        return self._skill_client.set_after_group_change_hook(hook)

    def clear_after_group_change_hook(self) -> bool:
        return self._skill_client.clear_after_group_change_hook()

    def enable_skill_load_persistence(
        self,
        *,
        path: Any | None = None,
        sqlite_mirror: bool = True,
        policy: str = "skip_on_drift",
    ) -> dict[str, Any]:
        """Persist + replay ``SkillCatalog.loaded`` across restarts (#1405)."""
        return self._get_observability().enable_skill_load_persistence(
            path=path,
            sqlite_mirror=sqlite_mirror,
            policy=policy,
        )

    def register_lifecycle_hooks(self, hooks: Any) -> Any:
        """Bind a :class:`~dcc_mcp_core.lifecycle_hooks.LifecycleHooks` registry."""
        return self._get_observability().register_lifecycle_hooks(hooks)

    def lifecycle_hooks(self) -> Any | None:
        return self._get_observability().lifecycle_hooks()

    def dispatch_lifecycle_event(
        self,
        event: Any,
        payload: dict[str, Any] | None = None,
        *,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        return self._get_observability().dispatch_lifecycle_event(event, payload, session_id=session_id)

    def dispatch_session_start(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return self._get_observability().dispatch_session_start(session_id=session_id, payload=payload)

    def dispatch_session_end(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        return self._get_observability().dispatch_session_end(session_id=session_id, payload=payload)

    def dispatch_before_tool_call(
        self,
        tool_name: str,
        *,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        return self._get_observability().dispatch_before_tool_call(
            tool_name,
            payload=payload,
            session_id=session_id,
        )

    def dispatch_after_tool_call(
        self,
        tool_name: str,
        *,
        ok: bool,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        return self._get_observability().dispatch_after_tool_call(
            tool_name,
            ok=ok,
            payload=payload,
            session_id=session_id,
        )

    def unload_skill(self, name: str) -> bool:
        return self._skill_client.unload_skill(name)

    def search_actions(
        self,
        category: str | None = None,
        tags: list[str] | None = None,
        dcc_name: str | None = None,
    ) -> list[Any]:
        return self._skill_client.search_actions(category=category, tags=tags, dcc_name=dcc_name)

    def get_skill_categories(self) -> list[str]:
        return self._skill_client.get_skill_categories()

    def get_skill_tags(self, dcc_name: str | None = None) -> list[str]:
        return self._skill_client.get_skill_tags(dcc_name)

    def unregister_skill(self, name: str, dcc_name: str | None = None) -> None:
        self._skill_client.unregister_skill(name, dcc_name)

    def is_skill_loaded(self, name: str) -> bool:
        return self._skill_client.is_skill_loaded(name)

    def get_skill_info(self, name: str) -> Any | None:
        return self._skill_client.get_skill_info(name)

    # --- hot-reload -------------------------------------------------------------

    def enable_hot_reload(
        self,
        skill_paths: list[str] | None = None,
        debounce_ms: int = 300,
    ) -> bool:
        """Enable automatic skill hot-reload on file changes."""
        return self._get_skill_discovery().enable_hot_reload(skill_paths=skill_paths, debounce_ms=debounce_ms)

    def disable_hot_reload(self) -> None:
        """Disable skill hot-reload."""
        self._get_skill_discovery().disable_hot_reload()

    @property
    def is_hot_reload_enabled(self) -> bool:
        return self._get_skill_discovery().is_hot_reload_enabled

    @property
    def hot_reload_stats(self) -> dict:
        return self._get_skill_discovery().hot_reload_stats

    # --- lifecycle --------------------------------------------------------------

    def register_quit_hook(self, callback: Callable[[], Any]) -> Callable[[], Any]:
        """Register a callback to run before the server shuts down."""
        return self._get_lifecycle_ctrl().register_quit_hook(callback)

    def unregister_quit_hook(self, callback: Callable[[], Any]) -> bool:
        """Remove a previously registered quit hook."""
        return self._get_lifecycle_ctrl().unregister_quit_hook(callback)

    def start(self, *, install_atexit_hook: bool = True) -> Any:
        """Start the MCP HTTP server."""
        return self._get_lifecycle_ctrl().start(install_atexit_hook=install_atexit_hook)

    def _gateway_runtime_metadata(self) -> dict[str, str]:
        return self._get_lifecycle_ctrl()._gateway_runtime_metadata()

    def _stage_gateway_runtime_metadata(self) -> None:
        self._get_lifecycle_ctrl()._stage_gateway_runtime_metadata()

    def _publish_gateway_runtime_metadata(self) -> None:
        self._get_lifecycle_ctrl()._publish_gateway_runtime_metadata()

    def stop(self) -> None:
        """Gracefully stop the server and gateway election thread."""
        self._get_lifecycle_ctrl().stop()

    def __enter__(self) -> Any:
        return self.start()

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        self.stop()

    @property
    def is_running(self) -> bool:
        """Whether the MCP HTTP server is currently active."""
        return self._handle is not None

    @property
    def mcp_url(self) -> str | None:
        """The MCP endpoint URL, or ``None`` if not running."""
        return self._handle.mcp_url() if self._handle else None

    # --- gateway metadata update ------------------------------------------------

    def update_gateway_metadata(
        self,
        scene: str | None = None,
        version: str | None = None,
        documents: list[str] | None = None,
        display_name: str | None = None,
    ) -> bool:
        """Update instance metadata in the gateway registry."""
        return self._get_lifecycle_ctrl().update_gateway_metadata(
            scene=scene,
            version=version,
            documents=documents,
            display_name=display_name,
        )

    def get_gateway_election_status(self) -> dict:
        """Return gateway election thread status."""
        return self._get_lifecycle_ctrl().get_gateway_election_status()

    # --- DCC version hook (override in subclass) --------------------------------

    def _version_string(self) -> str:
        """Return the DCC application version string. Override in sub-classes."""
        return "unknown"

    # --- gateway promotion hook (invoked by DccGatewayElection) -----------------

    def _upgrade_to_gateway(self) -> bool:
        """Promote this instance to the active gateway by re-running bind."""
        return self._get_lifecycle_ctrl()._upgrade_to_gateway()

    # --- Plugin manifest (issue #410) -------------------------------------------

    def plugin_manifest(
        self,
        *,
        version: str | None = None,
        extra_mcp_servers: list[dict] | None = None,
    ) -> dict:
        """Generate a Claude Code plugin manifest for this server."""
        return self._get_lifecycle_ctrl().plugin_manifest(
            version=version,
            extra_mcp_servers=extra_mcp_servers,
        )

    def __repr__(self) -> str:
        status = "running" if self.is_running else "stopped"
        return f"{type(self).__name__}(dcc={self._dcc_name!r}, status={status})"
