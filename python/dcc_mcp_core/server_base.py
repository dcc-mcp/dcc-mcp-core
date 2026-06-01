"""Generic DCC MCP server base class.

``DccServerBase`` centralises the shared boilerplate for DCC adapters:
skill-path discovery, MCP server wiring, hot reload, gateway failover, and
server lifecycle management. Adapters usually only construct
``DccServerOptions`` and optionally override ``_version_string`` or
``_upgrade_to_gateway``.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import atexit
import contextlib
import logging
from pathlib import Path
import sys
from typing import Any
from typing import Callable
import weakref

# Import first-party modules — compiled symbols come from ``dcc_mcp_core._core`` so
# this module never depends on the lazy ``dcc_mcp_core`` package facade during import.
from dcc_mcp_core import _core
from dcc_mcp_core._core import SandboxContext
from dcc_mcp_core._core import create_skill_server
from dcc_mcp_core._core import get_app_skill_paths_from_env
from dcc_mcp_core._core import get_local_skills_dir
from dcc_mcp_core._core import get_skill_paths_from_env
from dcc_mcp_core._core import get_skills_dir
from dcc_mcp_core._lifecycle_events import LifecycleEventDispatcher
from dcc_mcp_core._server import FileLoggingManager
from dcc_mcp_core._server import JobPersistenceManager
from dcc_mcp_core._server import ServerLifecycleController
from dcc_mcp_core._server import ServerRuntimeController
from dcc_mcp_core._server import SkillQueryClient
from dcc_mcp_core._server import TelemetryManager
from dcc_mcp_core._server import WindowResolver
from dcc_mcp_core._server import build_mcp_http_config
from dcc_mcp_core._server import collect_context_metadata_from_env
from dcc_mcp_core._server import resolve_diagnostics_state
from dcc_mcp_core._server import resolve_execution_binding
from dcc_mcp_core._server import resolve_observability_flags
from dcc_mcp_core._server.inprocess_executor import BaseDccCallableDispatcher
from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.minimal_mode import apply_minimal_mode
from dcc_mcp_core._server.options import DccServerOptions
from dcc_mcp_core.adapter_context import append_context_snapshot
from dcc_mcp_core.adapter_context import register_adapter_instruction_resources
from dcc_mcp_core.hotreload import DccSkillHotReloader
from dcc_mcp_core.plugin_manifest import build_plugin_manifest
from dcc_mcp_core.script_execution import allow_script_materialization_root
from dcc_mcp_core.skill import get_bundled_skill_paths
from dcc_mcp_core.skills.builtin import register_all_builtin_skills

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
        self._lifecycle_events = LifecycleEventDispatcher(
            options.dcc_name,
            lambda: getattr(self, "_lifecycle_hooks", None),
        )

        self._inprocess_executor_registered: bool = False
        self._cached_hwnd: int | None = None

        # ── File logging ──────────────────────────────────────────────────────
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

        # ── Job persistence ───────────────────────────────────────────────────
        self._init_job_persistence(options.dcc_name)

        # Create the inner skill manager
        self._server: Any = create_skill_server(options.dcc_name, self._config)
        self._register_builtin_skills(options)

        # Wire execution bridge / dispatcher
        if execution.bridge is not None:
            self.register_host_execution_bridge(execution.bridge)
        elif execution.dispatcher is not None:
            self.register_inprocess_executor(execution.dispatcher)
        elif execution.register_inprocess_executor:
            self.register_inprocess_executor(None)

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
        self._quit_hooks_ran: bool = False
        self._atexit_registered: bool = False
        self._lifecycle = ServerLifecycleController(self)
        self._runtime = ServerRuntimeController(self)

    # ── builtin skill helpers ───────────────────────────────────────────────

    def _register_builtin_skills(self, options: DccServerOptions) -> None:
        """Register standard built-in skills (diagnostics, introspect, etc)."""
        try:
            register_all_builtin_skills(
                self._server,
                dcc_name=options.dcc_name,
                dcc_pid=self._dcc_pid,
                dcc_window_handle=self._dcc_window_handle,
                dcc_window_title=self._dcc_window_title,
                gateway_failover_resolver=self.get_gateway_election_status,
            )
        except Exception as exc:
            logger.warning("[%s] built-in skill registration failed: %s", options.dcc_name, exc)

    def register_adapter_instructions(self, instruction_set: Any) -> list[str]:
        """Register standard adapter instruction/capability resources."""
        return register_adapter_instruction_resources(self._server, instruction_set)

    def set_context_snapshot_provider(self, provider: Any | None) -> None:
        """Set an optional callable used to append post-tool context snapshots."""
        self._snapshot_provider = provider

    def append_context_snapshot(self, result: dict[str, Any], *, policy: Any | None = None) -> dict[str, Any]:
        """Attach the configured post-tool context snapshot to a result envelope."""
        if self._snapshot_provider is None:
            return dict(result)

        return append_context_snapshot(result, self._snapshot_provider, policy=policy)

    # ── MCP resources ────────────────────────────────────────────────────────

    def resources(self) -> Any:
        """Return the shared MCP ``ResourceHandle`` for this server.

        Adapters should use this public surface to publish custom resources
        such as scene snapshots, command documentation, project state, and API
        references. The returned handle is the same registry used by the inner
        ``McpHttpServer``, so registrations made before or after ``start()``
        are reflected by ``resources/list`` and ``resources/read``.
        """
        get_resources = getattr(self._server, "resources", None)
        if not callable(get_resources):
            raise RuntimeError("inner MCP server does not expose resources()")
        return get_resources()

    def register_resource_producer(self, scheme_or_uri: str, producer: Callable[[str], Any]) -> None:
        """Register a Python resource producer on the server resource handle.

        Args:
            scheme_or_uri: Bare scheme (``"maya-cmds"``) or URI prefix
                (``"maya-cmds://"`` / ``"maya-cmds://commands"``).
            producer: Callable accepting ``uri: str`` and returning the
                ResourceHandle producer dict, typically
                ``{"mimeType": "text/plain", "text": "..."}`` or
                ``{"mimeType": "application/octet-stream", "blob": b"..."}``.

        """
        self.resources().register_producer(scheme_or_uri, producer)

    def set_scene_resource(self, snapshot: Any) -> None:
        """Publish ``snapshot`` as ``scene://current``."""
        self.resources().set_scene(snapshot)

    def notify_resource_updated(self, uri: str) -> None:
        """Emit ``notifications/resources/updated`` for ``uri``."""
        self.resources().notify_updated(uri)

    @staticmethod
    def _context_metadata_from_env(dcc_name: str) -> dict[str, str]:
        """Collect Rez-resolved context metadata for gateway discovery."""
        return collect_context_metadata_from_env(dcc_name)

    # ── observability helpers (delegated to collaborators, #486) ──────────────

    def _init_file_logging(self, dcc_name: str) -> str:
        """Initialise rolling file logging for this DCC server.

        Delegates to :class:`FileLoggingManager`. Returns the resolved log
        directory path (empty string on failure or when disabled). Failures
        are non-fatal: the manager logs a warning and the server continues.
        """
        manager = FileLoggingManager(dcc_name, enabled=self._enable_file_logging)
        return manager.init()

    def _init_job_persistence(self, dcc_name: str) -> None:
        """Wire a SQLite job-history database into ``McpHttpConfig``.

        Delegates to :class:`JobPersistenceManager`. The probe step detects
        whether the ``job-persist-sqlite`` Cargo feature is compiled in and
        falls back to the in-memory ``JobManager`` when it is not.
        """
        manager = JobPersistenceManager(dcc_name, enabled=self._enable_job_persistence, log_dir=self._log_dir)
        manager.init(self._config)

    def _init_telemetry(self) -> None:
        """Initialise in-process metrics so ``dcc_diagnostics__tool_metrics`` has data.

        Uses the noop exporter (no network traffic) — metrics stay in memory
        and are served exclusively through the ``dcc_diagnostics__tool_metrics``
        MCP tool. Call this once, just before ``server.start()``. Delegates
        to :class:`TelemetryManager`.
        """
        if not self._enable_telemetry:
            return
        TelemetryManager(self._dcc_name, self._dcc_pid, enabled=True).init()

    # ── readiness publication (#1206) ────────────────────────────────────────

    def set_readiness_probe(self, probe: Any) -> bool:
        """Publish a shared readiness probe to MCP and REST call surfaces.

        Adapters usually call this through
        :class:`dcc_mcp_core.AdapterReadinessBinder` before ``start()``. The
        inner server then uses the same probe for MCP ``tools/call`` gating,
        REST ``/v1/readyz`` reporting, and REST ``/v1/call`` gating.

        Returns:
            ``True`` when the inner server accepted the probe.

        """
        setter = getattr(self._server, "set_readiness_probe", None)
        if not callable(setter):
            logger.debug("[%s] set_readiness_probe unavailable on inner server", self._dcc_name)
            return False
        try:
            setter(probe)
            return True
        except Exception as exc:
            logger.debug("[%s] set_readiness_probe failed: %s", self._dcc_name, exc)
            return False

    # ── skill search path helpers ─────────────────────────────────────────────

    def collect_skill_search_paths(
        self,
        extra_paths: list[str] | None = None,
        include_bundled: bool = True,
        filter_existing: bool = False,
        include_admin_custom: bool = True,
    ) -> list[str]:
        """Build the ordered skill search path list for this DCC.

        Priority (highest → lowest):
        1. ``extra_paths`` from the caller
        2. Bundled skills in ``builtin_skills_dir``
        3. ``DCC_MCP_{DCC_NAME}_SKILL_PATHS`` env var (DCC-specific)
        4. ``DCC_MCP_SKILL_PATHS`` env var (global fallback)
        5. Local developer skills in ``~/.dcc-mcp/{dcc_name}/skills``
        6. Bundled skills shipped with dcc-mcp-core (when ``include_bundled=True``)
        7. Platform default skills dir
        8. Admin-UI-added skill discovery roots from the gateway SQLite lane
           (when ``include_admin_custom=True``; issue #1400)

        Args:
            extra_paths: Additional directories to prepend.
            include_bundled: Include general-purpose skills from dcc-mcp-core.
            filter_existing: When ``True``, remove paths that do not exist on
                disk and deduplicate the result.  Pass this when feeding paths
                to ``McpHttpServer.discover()`` to avoid warnings on missing dirs.
                Default ``False`` preserves backward-compatible behaviour.
            include_admin_custom: When ``True`` (default), append any custom
                skill paths persisted by the gateway admin UI (#1400) so an
                operator who adds a path via the dashboard sees it picked up
                on the next call to :meth:`reload_skill_paths` (or on the
                next adapter startup). Reads are best-effort — a missing or
                locked SQLite file is treated as zero rows.

        Returns:
            Ordered list of directory paths (strings).

        """
        paths: list[str] = list(extra_paths or [])

        if self._builtin_skills_dir.is_dir():
            paths.append(str(self._builtin_skills_dir))

        paths.extend(get_app_skill_paths_from_env(self._dcc_name))
        paths.extend(get_skill_paths_from_env())

        try:
            local_default_dir = get_local_skills_dir(self._dcc_name)
            Path(local_default_dir).mkdir(parents=True, exist_ok=True)
            if local_default_dir not in paths:
                paths.append(local_default_dir)
        except Exception as exc:
            logger.debug("[%s] Could not initialise local skill path: %s", self._dcc_name, exc)

        if include_bundled:
            try:
                paths.extend(get_bundled_skill_paths(include_bundled=True))
            except Exception as exc:
                logger.debug("[%s] Could not load bundled skill paths: %s", self._dcc_name, exc)

        default_dir = get_skills_dir()
        if default_dir and default_dir not in paths:
            paths.append(default_dir)

        if include_admin_custom:
            try:
                from dcc_mcp_core.admin_sqlite_lane import filter_new_paths
                from dcc_mcp_core.admin_sqlite_lane import read_custom_skill_paths

                custom = read_custom_skill_paths()
                paths.extend(filter_new_paths(paths, custom))
            except Exception as exc:
                logger.debug(
                    "[%s] could not read admin SQLite skill paths: %s",
                    self._dcc_name,
                    exc,
                )

        if filter_existing:
            seen: set[str] = set()
            filtered: list[str] = []
            for p in paths:
                if p not in seen and Path(p).is_dir():
                    seen.add(p)
                    filtered.append(p)
            return filtered

        return paths

    # ── skill registration ────────────────────────────────────────────────────

    def register_builtin_actions(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
        minimal_mode: MinimalModeConfig | None = None,
    ) -> None:
        """Discover and (optionally) progressively load skills.

        Builds the ordered skill search path via
        :meth:`collect_skill_search_paths` and calls
        ``McpHttpServer.discover``. When ``minimal_mode`` is provided,
        only the named skills are loaded eagerly, and the listed tool
        groups inside those skills are deactivated; the remaining
        discovered skills stay as ``__skill__<name>`` stubs until an
        agent calls ``load_skill``.

        Args:
            extra_skill_paths: Additional directories to scan.
            include_bundled: Include dcc-mcp-core bundled skills.
            minimal_mode: Declarative descriptor for progressive
                loading (issue #525). ``None`` (default) preserves the
                pre-existing behaviour of discovering everything and
                leaving every skill as a stub.

        """
        if (
            self._dcc_dispatcher is not None or self._standalone_main_thread
        ) and not self._inprocess_executor_registered:
            self.register_inprocess_executor(self._dcc_dispatcher)

        skill_paths = self.collect_skill_search_paths(
            extra_paths=extra_skill_paths,
            include_bundled=include_bundled,
            filter_existing=True,
        )
        logger.debug("[%s] Registering skills from %d path(s)", self._dcc_name, len(skill_paths))
        try:
            # McpHttpServer.discover() scans the given extra_paths in addition to
            # paths configured in McpHttpConfig; the returned count is informational.
            count = self._server.discover(extra_paths=skill_paths)
            logger.info("[%s] Skills discovered: %d from %d path(s)", self._dcc_name, count, len(skill_paths))
        except Exception as exc:
            logger.warning("[%s] register_builtin_actions failed: %s", self._dcc_name, exc)
            return

        if minimal_mode is not None:
            try:
                loaded = apply_minimal_mode(
                    self._server,
                    minimal_mode,
                    dcc_name=self._dcc_name,
                )
                logger.info(
                    "[%s] Minimal mode: %d skill(s) loaded eagerly",
                    self._dcc_name,
                    loaded,
                )
            except Exception as exc:
                logger.warning("[%s] minimal_mode application failed: %s", self._dcc_name, exc)

    def reload_skill_paths(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
    ) -> int:
        """Re-discover skills after admin-UI skill paths changed (#1400).

        Re-collects the search path list (which by default merges any
        rows in the gateway admin SQLite ``skill_paths_custom`` table —
        see :meth:`collect_skill_search_paths`) and calls
        ``McpHttpServer.discover`` so the adapter's catalog picks up
        skills that an operator added through the admin dashboard
        without restarting the DCC.

        This is the per-adapter counterpart to the standalone
        ``dcc-mcp-server`` binary's ``catalog_discover_hook``: each
        adapter can poll, hot-key, or react to a gateway notification
        and call this method, and the running DCC will then expose any
        new skills on the next ``tools/list`` round-trip.

        Args:
            extra_skill_paths: Additional directories to scan on top of
                the standard path list. Useful for adapters that want
                to inject ephemeral roots (CI / tests).
            include_bundled: Include dcc-mcp-core bundled skills.

        Returns:
            The discovery count as reported by ``McpHttpServer.discover``
            (informational — equals the number of skill directories the
            backend was able to scan, not the delta vs. the previous
            scan). Returns ``0`` on any failure (logged at warning level).

        """
        skill_paths = self.collect_skill_search_paths(
            extra_paths=extra_skill_paths,
            include_bundled=include_bundled,
            filter_existing=True,
        )
        logger.debug(
            "[%s] reload_skill_paths: re-scanning %d path(s)",
            self._dcc_name,
            len(skill_paths),
        )
        try:
            count = self._server.discover(extra_paths=skill_paths)
        except Exception as exc:
            logger.warning("[%s] reload_skill_paths failed: %s", self._dcc_name, exc)
            return 0
        logger.info(
            "[%s] reload_skill_paths: %d skill(s) total from %d path(s)",
            self._dcc_name,
            count,
            len(skill_paths),
        )
        return count

    # ── gateway & is_gateway ──────────────────────────────────────────────────

    # ── observability properties ──────────────────────────────────────────────

    @property
    def log_dir(self) -> str:
        """Directory where rolling log files are written, or ``""`` if disabled."""
        return self._log_dir

    @property
    def observability_summary(self) -> dict[str, Any]:
        """Return a snapshot of the active observability features.

        Useful for ``dcc_diagnostics__process_status`` and support reports.
        """
        return {
            "file_logging": self._enable_file_logging,
            "log_dir": self._log_dir or None,
            "job_persistence": self._enable_job_persistence,
            "job_db": getattr(self._config, "job_storage_path", None),
            "telemetry": self._enable_telemetry,
        }

    # ── host execution bridge / in-process executor wiring (#599, #521) ───────

    def _attach_sandbox_to_bridge(self, bridge: HostExecutionBridge) -> None:
        """Forward ``McpHttpConfig.sandbox_policy`` to the execution bridge (#1001)."""
        policy = getattr(self._config, "sandbox_policy", None)
        if policy is not None:
            try:
                bridge.script_materialization_root = allow_script_materialization_root(
                    policy,
                    root=bridge.script_materialization_root,
                )
            except Exception as exc:
                logger.warning(
                    "[%s] failed to allow script materialization root in sandbox: %s",
                    self._dcc_name,
                    exc,
                )
            bridge.sandbox_context = SandboxContext(policy)

    def _attach_host_dispatcher_to_http(self, dispatcher: Any | None) -> bool:
        """Attach a host queue dispatcher to HTTP ``tools/call`` routing."""
        if dispatcher is None:
            return False
        attach = getattr(self._server, "attach_dispatcher", None)
        if not callable(attach):
            return False
        try:
            attach(dispatcher)
            return True
        except RuntimeError as exc:
            if "already called" in str(exc):
                logger.debug("[%s] host dispatcher already attached: %s", self._dcc_name, exc)
                return False
            logger.warning("[%s] attach_dispatcher failed: %s", self._dcc_name, exc)
            return False
        except TypeError as exc:
            logger.debug("[%s] dispatcher is not an HTTP host dispatcher: %s", self._dcc_name, exc)
            return False
        except Exception as exc:
            logger.warning("[%s] attach_dispatcher failed: %s", self._dcc_name, exc)
            return False

    def register_host_execution_bridge(self, bridge: HostExecutionBridge) -> None:
        """Wire the adapter-facing host execution bridge.

        New embedded adapters should keep a single :class:`HostExecutionBridge`
        for both direct host callables and in-process skill scripts. When the
        bridge carries a Rust-backed host queue dispatcher, this method also
        attaches it to ``McpHttpServer.attach_dispatcher`` so main-affinity
        MCP/REST calls share the same host-thread route.
        """
        self._attach_sandbox_to_bridge(bridge)
        self._execution_bridge = bridge
        self._dcc_dispatcher = bridge.dispatcher
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            self._server.set_in_process_executor(bridge.as_inprocess_executor())
            self._inprocess_executor_registered = True
            host_dispatcher_attached = self._attach_host_dispatcher_to_http(host_dispatcher)
            logger.info(
                "[%s] Host execution bridge registered (dispatcher=%s, host_dispatcher_attached=%s)",
                self._dcc_name,
                type(bridge.dispatcher).__name__ if bridge.dispatcher is not None else "inline",
                host_dispatcher_attached,
            )
        except Exception as exc:
            logger.warning(
                "[%s] register_host_execution_bridge failed: %s",
                self._dcc_name,
                exc,
            )

    def register_inprocess_executor(
        self,
        dispatcher: BaseDccCallableDispatcher | None = None,
    ) -> None:
        """Wire the standard in-process Python skill executor.

        Lifts the ``_wire_in_process_executor`` pattern that
        ``dcc-mcp-maya`` 0.2.19 implements into the core so every
        embedded DCC plugin (Maya, Houdini, Unreal, Blender Python …)
        gets the same in-process execution flow without re-implementing
        ~150 LOC.

        Must be called **before** any
        :meth:`register_builtin_actions` so all subsequently loaded
        skills register their handlers against the in-process path
        (avoids the timing race documented in issue #464/#465).

        Args:
            dispatcher: Optional :class:`BaseDccCallableDispatcher`
                that marshals the script call onto the host's UI /
                main thread. ``None`` (the default — useful for
                ``mayapy``, headless Houdini, pytest) runs the script
                inline on the calling thread.

        """
        self._dcc_dispatcher = dispatcher
        bridge = HostExecutionBridge(dispatcher=dispatcher)
        self._attach_sandbox_to_bridge(bridge)
        self._execution_bridge = bridge
        executor = bridge.as_inprocess_executor()
        host_dispatcher = bridge.resolve_host_dispatcher()
        try:
            self._server.set_in_process_executor(executor)
            self._inprocess_executor_registered = True
            host_dispatcher_attached = self._attach_host_dispatcher_to_http(host_dispatcher)
            logger.info(
                "[%s] In-process executor registered (dispatcher=%s, host_dispatcher_attached=%s)",
                self._dcc_name,
                type(dispatcher).__name__ if dispatcher is not None else "inline",
                host_dispatcher_attached,
            )
        except Exception as exc:
            logger.warning(
                "[%s] register_inprocess_executor failed: %s",
                self._dcc_name,
                exc,
            )

    # ── gateway & is_gateway ──────────────────────────────────────────────────

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

    # ── DCC instance context (PID / window handle / title) ───────────────────

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
        """Resolve the DCC window handle from the available context.

        Delegates to :class:`WindowResolver` (#486). Priority: explicit
        ``dcc_window_handle`` → cached lookup → PID lookup via
        :class:`WindowFinder` → title lookup → ``None``.
        """
        hwnd = self._window_resolver.resolve()
        # Mirror the resolved handle back onto the instance so direct
        # attribute reads (used by historical screenshot helpers) keep
        # observing the cached value.
        if hwnd is not None and self._cached_hwnd is None:
            self._cached_hwnd = hwnd
        return hwnd

    # ── skill query methods (generic — 100% identical across all DCCs) ────────
    # Delegated to SkillQueryClient collaborator (#486).

    @property
    def registry(self) -> Any | None:
        """The underlying ``ToolRegistry``, or ``None`` if unavailable."""
        return self._skill_client.registry

    def list_actions(self, dcc_name: str | None = None) -> list[Any]:
        """List all registered actions for this DCC.

        Args:
            dcc_name: Override the DCC filter (default: this adapter's dcc_name).

        Returns:
            List of ``ActionInfo`` objects.

        """
        return self._skill_client.list_actions(dcc_name)

    def list_skills(self) -> list[Any]:
        """List all discovered skills (loaded and unloaded).

        Returns:
            List of ``SkillSummary`` objects.

        """
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
        """Load a skill by name."""
        return self._skill_client.load_skill(name)

    def get_skill(self, name: str) -> Any | None:
        """Return a detached mutable ``SkillMetadata`` object for a skill."""
        return self._skill_client.get_skill(name)

    def load_skill_object(self, skill: Any) -> bool:
        """Load a caller-supplied ``SkillMetadata`` object through core."""
        return self._skill_client.load_skill_object(skill)

    def set_skill_load_transform(self, transform: Callable[[Any], Any] | None) -> bool:
        """Register an adapter policy hook applied before every skill load.

        The callable receives a detached mutable ``SkillMetadata`` object. It
        may mutate that object and return ``None``, or return a replacement
        ``SkillMetadata``. Raising an exception vetoes the load before any
        tools are registered. Because the hook is installed on the inner
        catalog, direct Python ``load_skill``, MCP ``load_skill``, REST
        ``/v1/load_skill``, and batch/group loads all share the same policy.

        Returns:
            ``True`` when the inner server accepted the hook.

        """
        return self._skill_client.set_skill_load_transform(transform)

    def clear_skill_load_transform(self) -> bool:
        """Remove the adapter skill-load transform, if one is registered."""
        return self._skill_client.clear_skill_load_transform()

    def set_after_load_skill_hook(self, hook: Callable[[Any, list[str]], Any] | None) -> bool:
        """Register an observer called with ``(skill, registered_actions)`` after load."""
        return self._skill_client.set_after_load_skill_hook(hook)

    def clear_after_load_skill_hook(self) -> bool:
        """Remove the after-load skill observer, if one is registered."""
        return self._skill_client.clear_after_load_skill_hook()

    def set_after_unload_skill_hook(self, hook: Callable[[str, list[str]], Any] | None) -> bool:
        """Register an after-unload observer (#1405).

        The callable receives ``(skill_name, unregistered_actions)``.
        Returns ``True`` if the inner server accepted the hook. Used by
        the load-state persistence layer to evict the row from disk.
        """
        return self._skill_client.set_after_unload_skill_hook(hook)

    def clear_after_unload_skill_hook(self) -> bool:
        """Remove the after-unload observer, if one is registered."""
        return self._skill_client.clear_after_unload_skill_hook()

    def set_after_group_change_hook(self, hook: Callable[[str, bool], Any] | None) -> bool:
        """Register an after-group-change observer (#1405).

        The callable receives ``(group_name, activated: bool)``.
        Used by the load-state persistence layer to mirror catalog-wide
        active-group state on disk.
        """
        return self._skill_client.set_after_group_change_hook(hook)

    def clear_after_group_change_hook(self) -> bool:
        """Remove the after-group-change observer, if one is registered."""
        return self._skill_client.clear_after_group_change_hook()

    def enable_skill_load_persistence(
        self,
        *,
        path: Any | None = None,
        sqlite_mirror: bool = True,
        policy: str = "skip_on_drift",
    ) -> dict[str, Any]:
        """Persist + replay ``SkillCatalog.loaded`` across restarts (#1405).

        Loads a :class:`~dcc_mcp_core.loaded_state_store.LoadedStateStore`
        from disk (default path: ``~/.dcc-mcp/<dcc>/loaded.json``), wires
        the catalog's after-load / after-unload / after-group-change
        hooks so every state change is checkpointed, and replays the
        persisted snapshot on the inner skill server.

        Returns the replay report (parsed from JSON) plus a few status
        fields. Safe to call after :meth:`start` — the catalog has
        already finished discovery by then so the replay knows what to
        match against.

        Hooks set previously by the caller are wrapped, not overwritten,
        when they exist — the persistence callback runs in addition to
        anything the adapter had registered.
        """
        from dcc_mcp_core.loaded_state_store import LoadedStateStore

        store = LoadedStateStore(self._dcc_name, path=path, sqlite_mirror=sqlite_mirror)
        self._loaded_state_store = store

        def _on_after_load(skill: Any, registered: list[str]) -> None:
            name = getattr(skill, "name", None)
            if not name:
                return
            version = getattr(skill, "version", None) or None
            skill_path = getattr(skill, "skill_path", None) or None
            store.record_loaded(name, version=version, skill_path=skill_path)

        def _on_after_unload(skill_name: str, _unregistered: list[str]) -> None:
            store.record_unloaded(skill_name)

        def _on_group_change(group_name: str, activated: bool) -> None:
            store.record_group_change(group_name, activated=activated)

        # The before/after-load lifecycle bridge already installed its own
        # observer via set_after_load_skill_hook. Wrap rather than replace
        # so existing callers keep working — we install the persistence
        # hook *after* the lifecycle bridge so adapter policy still runs.
        self.set_after_load_skill_hook(_on_after_load)
        self.set_after_unload_skill_hook(_on_after_unload)
        self.set_after_group_change_hook(_on_group_change)

        snapshot = store.snapshot()
        if not snapshot.skills and not snapshot.active_groups:
            return {
                "store_path": str(store.path),
                "replayed": False,
                "reason": "empty_state",
            }

        import json

        report_json = self._skill_client.replay_loaded_skills(
            json.dumps(snapshot.to_json()),
            policy=policy,
        )
        if report_json is None:
            return {
                "store_path": str(store.path),
                "replayed": False,
                "reason": "binding_unavailable",
            }
        try:
            report = json.loads(report_json)
        except json.JSONDecodeError as exc:
            logger.warning(
                "[%s] enable_skill_load_persistence: failed to parse replay report: %s",
                self._dcc_name,
                exc,
            )
            report = {}
        return {
            "store_path": str(store.path),
            "replayed": True,
            "policy": policy,
            "report": report,
        }

    def register_lifecycle_hooks(self, hooks: Any) -> Any:
        """Bind a :class:`~dcc_mcp_core.lifecycle_hooks.LifecycleHooks` registry.

        The registry's ``BEFORE_SKILL_LOAD`` and ``AFTER_SKILL_LOAD`` handlers
        are bridged to the existing skill-load transform / after-load setters.
        ``search_skills`` emits search hooks, and adapters can use the
        ``dispatch_*`` helpers below to bridge host-owned session and tool-call
        boundaries through the same typed surface (issue #1337).

        Returns the registry, so callers can chain
        ``register_lifecycle_hooks(LifecycleHooks()).on(event, handler)``.
        """
        from dcc_mcp_core.lifecycle_hooks import HookContext
        from dcc_mcp_core.lifecycle_hooks import HookEvent

        self._lifecycle_hooks = hooks

        def _bridge_before_load(skill: Any) -> Any:
            hooks.dispatch(
                HookContext(
                    event=HookEvent.BEFORE_SKILL_LOAD,
                    dcc_name=self._dcc_name,
                    payload={"skill_name": getattr(skill, "name", None)},
                )
            )
            return None

        def _bridge_after_load(skill: Any, registered: list[str]) -> None:
            hooks.dispatch(
                HookContext(
                    event=HookEvent.AFTER_SKILL_LOAD,
                    dcc_name=self._dcc_name,
                    payload={
                        "skill_name": getattr(skill, "name", None),
                        "registered_actions": list(registered),
                    },
                )
            )

        self.set_skill_load_transform(_bridge_before_load)
        self.set_after_load_skill_hook(_bridge_after_load)
        return hooks

    def lifecycle_hooks(self) -> Any | None:
        """Return the :class:`LifecycleHooks` registered by ``register_lifecycle_hooks``."""
        return getattr(self, "_lifecycle_hooks", None)

    def dispatch_lifecycle_event(
        self,
        event: Any,
        payload: dict[str, Any] | None = None,
        *,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        """Dispatch a typed lifecycle event through the registered hooks.

        Hook failures follow :class:`LifecycleHooks` semantics: unexpected
        exceptions are logged and swallowed, while ``HookDeny`` from policy
        events propagates to the caller. The returned dict is the mutable
        payload after ``before_*`` handlers had a chance to enrich it.
        """
        return self._lifecycle_events.dispatch(event, payload=payload, session_id=session_id)

    def dispatch_session_start(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Emit ``on_session_start`` for adapters with explicit agent sessions."""
        return self.dispatch_lifecycle_event("on_session_start", payload, session_id=session_id)

    def dispatch_session_end(
        self,
        *,
        session_id: str,
        payload: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Emit ``on_session_end`` so memory/telemetry consumers can compact state."""
        return self.dispatch_lifecycle_event("on_session_end", payload, session_id=session_id)

    def dispatch_before_tool_call(
        self,
        tool_name: str,
        *,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        """Emit ``before_tool_call`` before adapter-owned tool execution."""
        event_payload = {"tool_name": tool_name, **(payload or {})}
        return self.dispatch_lifecycle_event("before_tool_call", event_payload, session_id=session_id)

    def dispatch_after_tool_call(
        self,
        tool_name: str,
        *,
        ok: bool,
        payload: dict[str, Any] | None = None,
        session_id: str | None = None,
    ) -> dict[str, Any]:
        """Emit ``after_tool_call`` after adapter-owned tool execution."""
        event_payload = {"tool_name": tool_name, "ok": bool(ok), **(payload or {})}
        return self.dispatch_lifecycle_event("after_tool_call", event_payload, session_id=session_id)

    def unload_skill(self, name: str) -> bool:
        """Unload a skill by name."""
        return self._skill_client.unload_skill(name)

    def search_actions(
        self,
        category: str | None = None,
        tags: list[str] | None = None,
        dcc_name: str | None = None,
    ) -> list[Any]:
        """Search registered actions by category and/or tags.

        Delegates to :meth:`ToolRegistry.search_actions` which filters by
        exact category match, all-tags-present, and optional DCC scope.
        """
        return self._skill_client.search_actions(category=category, tags=tags, dcc_name=dcc_name)

    def get_skill_categories(self) -> list[str]:
        """Return all unique action categories."""
        return self._skill_client.get_skill_categories()

    def get_skill_tags(self, dcc_name: str | None = None) -> list[str]:
        """Return all unique tags for this DCC."""
        return self._skill_client.get_skill_tags(dcc_name)

    def unregister_skill(self, name: str, dcc_name: str | None = None) -> None:
        """Unregister a skill from the action registry."""
        self._skill_client.unregister_skill(name, dcc_name)

    def is_skill_loaded(self, name: str) -> bool:
        """Check whether a skill is currently loaded."""
        return self._skill_client.is_skill_loaded(name)

    def get_skill_info(self, name: str) -> Any | None:
        """Return full metadata for a skill."""
        return self._skill_client.get_skill_info(name)

    # ── hot-reload ────────────────────────────────────────────────────────────

    def enable_hot_reload(
        self,
        skill_paths: list[str] | None = None,
        debounce_ms: int = 300,
    ) -> bool:
        """Enable automatic skill hot-reload on file changes.

        Args:
            skill_paths: Directories to monitor. Defaults to the adapter's
                standard skill paths.
            debounce_ms: Wait time after last event before reloading.

        Returns:
            ``True`` on success.

        """
        if self._hot_reloader is None:
            self._hot_reloader = DccSkillHotReloader(dcc_name=self._dcc_name, server=self)

        paths = skill_paths or self.collect_skill_search_paths(include_bundled=False, filter_existing=True)
        return self._hot_reloader.enable(paths, debounce_ms=debounce_ms)

    def disable_hot_reload(self) -> None:
        """Disable skill hot-reload."""
        if self._hot_reloader is not None:
            self._hot_reloader.disable()

    @property
    def is_hot_reload_enabled(self) -> bool:
        """Whether hot-reload is currently active."""
        return self._hot_reloader is not None and self._hot_reloader.is_enabled

    @property
    def hot_reload_stats(self) -> dict:
        """Hot-reload statistics (watched_paths, reload_count)."""
        if self._hot_reloader is None:
            return {"enabled": False, "watched_paths": [], "reload_count": 0}
        return self._hot_reloader.get_stats()

    # ── lifecycle ─────────────────────────────────────────────────────────────

    @staticmethod
    def _stop_from_atexit(ref: weakref.ReferenceType[DccServerBase]) -> None:
        server = ref()
        if server is not None:
            server.stop()

    def _ensure_quit_hook_state(self) -> None:
        self._lifecycle_controller().ensure_state()

    def _lifecycle_controller(self) -> ServerLifecycleController:
        controller = self.__dict__.get("_lifecycle")
        if controller is None:
            controller = ServerLifecycleController(self)
            self._lifecycle = controller
        return controller

    def _runtime_controller(self) -> ServerRuntimeController:
        controller = self.__dict__.get("_runtime")
        if controller is None:
            controller = ServerRuntimeController(self)
            self._runtime = controller
        return controller

    def register_quit_hook(self, callback: Callable[[], Any]) -> Callable[[], Any]:
        """Register a callback to run before the server shuts down.

        Hooks run once per server lifetime in LIFO order. Exceptions are
        logged and swallowed so one broken hook cannot block shutdown.
        """
        return self._lifecycle_controller().register_quit_hook(callback)

    def unregister_quit_hook(self, callback: Callable[[], Any]) -> bool:
        """Remove a previously registered quit hook.

        Returns ``True`` when a hook was removed.
        """
        return self._lifecycle_controller().unregister_quit_hook(callback)

    def _run_quit_hooks(self) -> None:
        """Run registered quit hooks in LIFO order exactly once."""
        self._lifecycle_controller().run_quit_hooks(dcc_name=self._dcc_name)

    def start(self, *, install_atexit_hook: bool = True) -> Any:
        """Start the MCP HTTP server.

        Starts the gateway election thread if ``enable_gateway_failover`` is
        set and a ``gateway_port`` is configured.

        Returns:
            ``McpServerHandle`` with ``.mcp_url()``, ``.port``, ``.shutdown()``.

        """
        if self._handle is not None:
            logger.warning(
                "[%s] Server already running on port %d",
                self._dcc_name,
                self._handle.port,
            )
            return self._handle
        self._lifecycle_controller().prepare_start(
            install_atexit_hook=install_atexit_hook,
            stop_from_atexit=DccServerBase._stop_from_atexit,
            atexit_register=atexit.register,
        )

        # Initialise in-process metrics just before start so the
        # ToolRecorder inside McpHttpServer can accumulate data from the
        # first tool call onward.
        self._init_telemetry()

        self._runtime_controller().ensure_gateway_daemon_if_needed()
        self._handle = self._server.start()
        server_version = getattr(self._config, "server_version", _PKG_VERSION)
        logger.info(
            "[%s] MCP server v%s started at %s",
            self._dcc_name,
            server_version,
            self._handle.mcp_url(),
        )
        self._runtime_controller().start_gateway_guardian_if_needed()
        self._runtime_controller().start_gateway_election_if_needed()

        return self._handle

    def stop(self) -> None:
        """Gracefully stop the server and gateway election thread."""
        self._run_quit_hooks()
        self._runtime_controller().stop_gateway_guardian()
        self._runtime_controller().stop_gateway_election()

        if self._hot_reloader is not None:
            with contextlib.suppress(Exception):
                self._hot_reloader.disable()
        self._runtime_controller().shutdown_server_handle()

    def __enter__(self) -> Any:
        """Start the server and return its handle for ``with`` blocks."""
        return self.start()

    def __exit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
        """Stop the server when leaving a ``with`` block."""
        self.stop()

    @property
    def is_running(self) -> bool:
        """Whether the MCP HTTP server is currently active."""
        return self._handle is not None

    @property
    def mcp_url(self) -> str | None:
        """The MCP endpoint URL, or ``None`` if not running."""
        return self._handle.mcp_url() if self._handle else None

    # ── gateway metadata update ───────────────────────────────────────────────

    def update_gateway_metadata(
        self,
        scene: str | None = None,
        version: str | None = None,
        documents: list[str] | None = None,
        display_name: str | None = None,
    ) -> bool:
        """Update instance metadata in the gateway registry.

        Works for both single-document DCCs (Maya, Blender — pass ``scene``
        only) and multi-document DCCs (Photoshop, After Effects — also pass
        ``documents`` with all open files and optionally ``display_name``).

        Changes propagate to ``FileRegistry`` on the next heartbeat tick
        (≤ 5 s) so the ``gateway://instances`` MCP resource stays current
        without a gateway restart.

        Args:
            scene: Active/focused scene or document path.
                   ``None`` = no change, ``""`` = clear.
            version: DCC application version string.
                     ``None`` = no change, ``""`` = clear.
            documents: Full list of open documents (multi-document DCCs like
                       Photoshop / After Effects).
                       ``None`` = no change, ``[]`` = clear list.
            display_name: Human-readable instance label shown during
                          disambiguation (e.g. ``"PS-Marketing"``).
                          ``None`` = no change, ``""`` = clear.

        Returns:
            ``True`` on success.

        """
        if not self.is_running:
            logger.warning("[%s] Cannot update metadata: server is not running", self._dcc_name)
            return False

        gateway_port = getattr(self._config, "gateway_port", 0)
        if gateway_port <= 0:
            logger.debug("[%s] Gateway not configured; metadata update skipped", self._dcc_name)
            return False

        try:
            if scene is not None:
                self._config.scene = scene
            if version is not None:
                self._config.dcc_version = version
            # Push all fields into the live-metadata store so the next
            # heartbeat tick (≤ 5 s) propagates them to FileRegistry.
            if self._handle is not None:
                try:
                    self._handle.update_scene(scene, version, documents, display_name)
                except Exception as exc_inner:
                    logger.debug("[%s] handle.update_scene failed: %s", self._dcc_name, exc_inner)
            return True
        except Exception as exc:
            logger.error("[%s] Failed to update gateway metadata: %s", self._dcc_name, exc)
            return False

    def get_gateway_election_status(self) -> dict:
        """Return gateway election thread status.

        The returned dict is the canonical source for the
        ``dcc_diagnostics__gateway_failover`` MCP tool (#1355) and admin
        introspection. Shape:

        * ``enabled`` (bool): the adapter opted into automatic gateway
          failover.
        * ``running`` (bool): the election thread is currently alive.
        * ``consecutive_failures`` (int): probe failures since the last
          successful health check (always ``0`` when no thread is running).
        * ``gateway_host`` / ``gateway_port``: target endpoint the election
          bids for. ``gateway_port`` is ``0`` when no port is configured —
          in that case failover cannot run even if ``enabled`` is ``True``.
        * ``is_gateway`` (bool): whether *this* server currently owns the
          gateway port (``True`` when promoted, or when this adapter was
          the first-wins gateway from the start).
        """
        gateway_port = int(getattr(self._config, "gateway_port", 0) or 0)
        is_gateway = bool(getattr(self, "is_gateway", False))
        if self._gateway_election is None:
            return {
                "enabled": bool(self._enable_gateway_failover),
                "running": False,
                "consecutive_failures": 0,
                "gateway_host": None,
                "gateway_port": gateway_port,
                "is_gateway": is_gateway,
                "gateway_runtime_mode": getattr(self, "_gateway_runtime_mode", "unknown"),
                "gateway_daemon_status": dict(getattr(self, "_gateway_daemon_status", {}) or {}),
            }
        status = self._gateway_election.get_status()
        status["enabled"] = bool(self._enable_gateway_failover)
        status.setdefault("gateway_port", gateway_port)
        status["is_gateway"] = is_gateway
        status["gateway_runtime_mode"] = getattr(self, "_gateway_runtime_mode", "unknown")
        status["gateway_daemon_status"] = dict(getattr(self, "_gateway_daemon_status", {}) or {})
        return status

    # ── DCC version hook (override in subclass) ───────────────────────────────

    def _version_string(self) -> str:
        """Return the DCC application version string.

        Override in sub-classes to query the DCC's own version API, e.g.::

            def _version_string(self) -> str:
                import bpy
                return bpy.app.version_string

        Returns:
            Version string, default ``"unknown"``.

        """
        return "unknown"

    # ── gateway promotion hook (invoked by DccGatewayElection) ────────────────

    def _upgrade_to_gateway(self) -> bool:
        """Promote this instance to the active gateway by re-running bind.

        Called by :class:`DccGatewayElection` after it detects that the
        current gateway is unreachable and the gateway port appears free.
        The default implementation tears down the current MCP HTTP server
        handle and starts a new one, which lets the Rust ``GatewayRunner``
        re-run its exclusive first-wins port bind. When that bind succeeds
        the new handle's ``is_gateway`` flag will be ``True`` and
        ``self.is_gateway`` will reflect the promotion without a process
        restart.

        Sub-classes may override this method to plug in DCC-specific
        promotion logic (e.g. clearing caches, re-announcing the endpoint
        to a discovery service).

        Returns:
            ``True`` if the instance is now the active gateway, ``False``
            otherwise (e.g. another process grabbed the port first, or the
            restart failed).

        """
        if self.is_gateway:
            return True

        gateway_port = getattr(self._config, "gateway_port", 0)
        if not gateway_port or gateway_port <= 0:
            logger.debug(
                "[%s] Cannot promote to gateway: gateway_port is not configured",
                self._dcc_name,
            )
            return False

        old_handle = self._handle
        if old_handle is not None:
            with contextlib.suppress(Exception):
                old_handle.shutdown()
            self._handle = None

        try:
            self._handle = self._server.start()
        except Exception as exc:
            logger.error("[%s] Gateway promotion restart failed: %s", self._dcc_name, exc)
            self._handle = None
            return False

        promoted = bool(getattr(self._handle, "is_gateway", False))
        if promoted:
            logger.info("[%s] Gateway promotion succeeded (re-bound on %d)", self._dcc_name, gateway_port)
        else:
            logger.info(
                "[%s] Gateway promotion attempted but another instance won the bind; running as plain instance",
                self._dcc_name,
            )
        return promoted

    # ── Plugin manifest (issue #410) ─────────────────────────────────────────

    def plugin_manifest(
        self,
        *,
        version: str | None = None,
        extra_mcp_servers: list[dict] | None = None,
    ) -> dict:
        """Generate a Claude Code plugin manifest for this server.

        The manifest bundles the running MCP server URL and all currently
        loaded skill paths into a format compatible with Claude Code Plugins
        (https://code.claude.com/docs/en/plugins-reference).

        Requires the server to be running (``is_running == True``).

        Args:
            version: Plugin version string.  Defaults to
                ``dcc_mcp_core.__version__``.
            extra_mcp_servers: Additional MCP server entries to include in
                the manifest alongside this server.

        Returns:
            A JSON-serialisable dict following the Claude Code plugin manifest
            schema.  Save to ``claude_plugin.json`` and distribute alongside
            the server.

        Raises:
            RuntimeError: If the server is not running.

        Example::

            handle = server.start()
            manifest = server.plugin_manifest(version="1.0.0")
            import json, pathlib
            pathlib.Path("claude_plugin.json").write_text(json.dumps(manifest, indent=2))

        """
        if not self.is_running:
            raise RuntimeError(
                f"{self._dcc_name}: Cannot generate plugin manifest — server is not running. Call server.start() first."
            )

        return build_plugin_manifest(
            dcc_name=self._dcc_name,
            mcp_url=self.mcp_url,
            skill_paths=self.collect_skill_search_paths(),
            version=version or _PKG_VERSION,
            extra_mcp_servers=extra_mcp_servers,
        )

    def __repr__(self) -> str:
        status = "running" if self.is_running else "stopped"
        return f"{type(self).__name__}(dcc={self._dcc_name!r}, status={status})"
