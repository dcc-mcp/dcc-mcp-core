"""Generic DCC MCP server base class.

Provides a reusable foundation for embedding a standards-compliant MCP
Streamable HTTP server (2025-03-26 spec) inside any DCC application.

:class:`DccServerBase` bundles all the boilerplate that every DCC adapter
would otherwise copy-paste:

- Skill search path collection (per-app env var → global env var → bundled)
- ``McpHttpConfig`` / ``create_skill_server`` wiring
- All 7 skill query / management methods (find, list, load, unload, …)
- Hot-reload integration via :class:`~dcc_mcp_core.hotreload.DccSkillHotReloader`
- Gateway failover via :class:`~dcc_mcp_core.gateway_election.DccGatewayElection`
- Server lifecycle (start / stop / is_running / mcp_url)
- Module-level singleton helper via :func:`~dcc_mcp_core.factory.create_dcc_server`

Creating a DCC adapter
-----------------------
Subclass :class:`DccServerBase` and supply:

1. ``dcc_name``             — short identifier, e.g. ``"blender"``
2. ``builtin_skills_dir``   — ``Path`` to bundled skills shipped with the adapter

Everything else is inherited::

    from pathlib import Path
    from dcc_mcp_core.server_base import DccServerBase

    class BlenderMcpServer(DccServerBase):
        def __init__(self, port: int = 8765, **kwargs):
            super().__init__(
                dcc_name="blender",
                builtin_skills_dir=Path(__file__).parent / "skills",
                port=port,
                **kwargs,
            )

    # That's it — all skill methods, hot-reload, gateway are ready.

Minimum viable DCC-specific code
----------------------------------
Some DCCs may need to override:

- :meth:`_version_string` — return the DCC application version (default ``"unknown"``)
- :meth:`_upgrade_to_gateway` — DCC-specific gateway promotion (advanced)
"""

# Import future modules
from __future__ import annotations

import contextlib

# Import built-in modules
import logging
import os
from pathlib import Path
from typing import Any

# NOTE: dcc_mcp_core imports (McpHttpConfig, create_skill_server, get_*,
# TransportManager, get_bundled_skill_paths) are deferred inside methods to
# avoid a circular import: __init__.py imports DccServerBase from this module,
# so this module cannot import from dcc_mcp_core at module level.
from dcc_mcp_core.gateway_election import DccGatewayElection
from dcc_mcp_core.hotreload import DccSkillHotReloader

logger = logging.getLogger(__name__)


class DccServerBase:
    """Base MCP server for any DCC application.

    Sub-classes only need to supply ``dcc_name`` and ``builtin_skills_dir``.
    All generic skill management, hot-reload, and gateway election logic is
    provided here so DCC adapters stay thin (~100 LOC each).

    Args:
        dcc_name: Short DCC identifier (``"maya"``, ``"blender"``, …).
            Used for logging, env-var names, and gateway labels.
        builtin_skills_dir: Path to the adapter's bundled ``skills/`` directory.
            Typically ``Path(__file__).parent / "skills"``.
        port: TCP port for the MCP HTTP server. ``0`` → OS picks a free port.
        server_name: Name reported in the MCP ``initialize`` response.
        server_version: Version reported in the MCP ``initialize`` response.
            Defaults to the installed ``dcc_mcp_core`` package version.
        gateway_port: Port for the multi-DCC first-wins gateway competition.
            ``None`` reads ``DCC_MCP_GATEWAY_PORT`` env var; ``0`` disables gateway.
        registry_dir: Directory for the shared ``FileRegistry`` JSON file.
        dcc_version: DCC application version string for the gateway registry.
        scene: Currently open scene file path for the gateway registry.
        enable_gateway_failover: Enable automatic gateway failover / election.

    """

    def __init__(
        self,
        dcc_name: str,
        builtin_skills_dir: Path,
        port: int = 8765,
        server_name: str | None = None,
        server_version: str | None = None,
        gateway_port: int | None = None,
        registry_dir: str | None = None,
        dcc_version: str | None = None,
        scene: str | None = None,
        enable_gateway_failover: bool = True,
    ) -> None:
        # Deferred: circular import — __init__.py imports DccServerBase from
        # this module, so we cannot import from dcc_mcp_core at module level.
        from dcc_mcp_core import McpHttpConfig
        from dcc_mcp_core import __version__ as _pkg_version
        from dcc_mcp_core import create_skill_server

        self._dcc_name = dcc_name
        self._builtin_skills_dir = builtin_skills_dir
        self._handle: Any | None = None
        self._enable_gateway_failover = enable_gateway_failover

        # Resolve gateway port
        effective_gateway_port: int = 0
        if gateway_port is not None:
            effective_gateway_port = gateway_port
        else:
            env_val = os.environ.get("DCC_MCP_GATEWAY_PORT", "")
            if env_val.isdigit():
                effective_gateway_port = int(env_val)

        # Build McpHttpConfig — port must be passed at construction time (read-only after init)
        self._config = McpHttpConfig(
            port=port,
            server_name=server_name or f"{dcc_name}-mcp",
            server_version=server_version if server_version is not None else _pkg_version,
        )
        if effective_gateway_port > 0:
            self._config.gateway_port = effective_gateway_port
        # registry_dir: explicit param wins; fall back to DCC_MCP_REGISTRY_DIR env var
        effective_registry_dir = registry_dir or os.environ.get("DCC_MCP_REGISTRY_DIR", "")
        if effective_registry_dir:
            self._config.registry_dir = effective_registry_dir
        resolved_dcc_version = dcc_version if dcc_version is not None else self._version_string()
        if resolved_dcc_version:
            self._config.dcc_version = resolved_dcc_version
        if scene:
            self._config.scene = scene
        # Always stamp the DCC type so gateway registry knows which DCC this is
        self._config.dcc_type = dcc_name

        # Create the inner skill manager (registry + dispatcher + catalog)
        self._server: Any = create_skill_server(dcc_name, self._config)

        # Lazy-initialised helpers
        self._hot_reloader: Any | None = None
        self._gateway_election: Any | None = None

    # ── skill search path helpers ─────────────────────────────────────────────

    def collect_skill_search_paths(
        self,
        extra_paths: list[str] | None = None,
        include_bundled: bool = True,
        filter_existing: bool = False,
    ) -> list[str]:
        """Build the ordered skill search path list for this DCC.

        Priority (highest → lowest):
        1. ``extra_paths`` from the caller
        2. Bundled skills in ``builtin_skills_dir``
        3. ``DCC_MCP_{DCC_NAME}_SKILL_PATHS`` env var (DCC-specific)
        4. ``DCC_MCP_SKILL_PATHS`` env var (global fallback)
        5. Bundled skills shipped with dcc-mcp-core (when ``include_bundled=True``)
        6. Platform default skills dir

        Args:
            extra_paths: Additional directories to prepend.
            include_bundled: Include general-purpose skills from dcc-mcp-core.
            filter_existing: When ``True``, remove paths that do not exist on
                disk and deduplicate the result.  Pass this when feeding paths
                to ``McpHttpServer.discover()`` to avoid warnings on missing dirs.
                Default ``False`` preserves backward-compatible behaviour.

        Returns:
            Ordered list of directory paths (strings).

        """
        from dcc_mcp_core import get_app_skill_paths_from_env
        from dcc_mcp_core import get_skill_paths_from_env
        from dcc_mcp_core import get_skills_dir

        paths: list[str] = list(extra_paths or [])

        if self._builtin_skills_dir.is_dir():
            paths.append(str(self._builtin_skills_dir))

        paths.extend(get_app_skill_paths_from_env(self._dcc_name))
        paths.extend(get_skill_paths_from_env())

        if include_bundled:
            try:
                from dcc_mcp_core.skill import get_bundled_skill_paths

                paths.extend(get_bundled_skill_paths(include_bundled=True))
            except Exception as exc:
                logger.debug("[%s] Could not load bundled skill paths: %s", self._dcc_name, exc)

        default_dir = get_skills_dir()
        if default_dir and default_dir not in paths:
            paths.append(default_dir)

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
    ) -> None:
        """Discover and load all skills from the search path.

        Builds the ordered skill search path via
        :meth:`collect_skill_search_paths` and calls
        ``SkillCatalog.discover_and_load_all``.

        Args:
            extra_skill_paths: Additional directories to scan.
            include_bundled: Include dcc-mcp-core bundled skills.

        """
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

    # ── skill query methods (generic — 100% identical across all DCCs) ────────

    @property
    def registry(self) -> Any | None:
        """The underlying ``ToolRegistry``, or ``None`` if unavailable."""
        try:
            return self._server.registry
        except Exception:
            return None

    def list_actions(self, dcc_name: str | None = None) -> list[Any]:
        """List all registered actions for this DCC.

        Args:
            dcc_name: Override the DCC filter (default: this adapter's dcc_name).

        Returns:
            List of ``ActionInfo`` objects.

        """
        registry = self.registry
        if registry is None:
            return []
        effective_dcc = dcc_name if dcc_name is not None else self._dcc_name
        try:
            return list(registry.list_actions(dcc_name=effective_dcc))
        except Exception as exc:
            logger.debug("[%s] list_actions failed: %s", self._dcc_name, exc)
            return []

    def list_skills(self) -> list[Any]:
        """List all discovered skills (loaded and unloaded).

        Returns:
            List of ``SkillSummary`` objects.

        """
        try:
            return list(self._server.list_skills())
        except Exception as exc:
            logger.debug("[%s] list_skills failed: %s", self._dcc_name, exc)
            return []

    def load_skill(self, name: str) -> bool:
        """Load a skill by name.

        Args:
            name: Skill name as discovered (e.g. ``"maya-scene"``).

        Returns:
            ``True`` on success.

        """
        try:
            self._server.load_skill(name)
            return True
        except Exception as exc:
            logger.debug("[%s] load_skill(%r) failed: %s", self._dcc_name, name, exc)
            return False

    def unload_skill(self, name: str) -> bool:
        """Unload a skill by name.

        Args:
            name: Skill name.

        Returns:
            ``True`` on success.

        """
        try:
            self._server.unload_skill(name)
            return True
        except Exception as exc:
            logger.debug("[%s] unload_skill(%r) failed: %s", self._dcc_name, name, exc)
            return False

    def search_actions(
        self,
        category: str | None = None,
        tags: list[str] | None = None,
        dcc_name: str | None = None,
    ) -> list[Any]:
        """Search registered actions by category and/or tags.

        Delegates to :meth:`ToolRegistry.search_actions` which filters by
        exact category match, all-tags-present, and optional DCC scope.

        Args:
            category: Exact category name to filter by (``None`` = no filter).
            tags: All listed tags must be present on the action (empty = no filter).
            dcc_name: Override the DCC filter.

        Returns:
            List of matching ``ActionInfo`` dicts.

        """
        registry = self.registry
        if registry is None:
            return []
        effective_dcc = dcc_name if dcc_name is not None else self._dcc_name
        try:
            return list(registry.search_actions(category=category, tags=tags or [], dcc_name=effective_dcc))
        except Exception as exc:
            logger.debug("[%s] search_actions failed: %s", self._dcc_name, exc)
            return []

    def get_skill_categories(self) -> list[str]:
        """Return all unique action categories.

        Returns:
            Sorted list of category strings.

        """
        registry = self.registry
        if registry is None:
            return []
        try:
            return list(registry.get_categories())
        except Exception as exc:
            logger.debug("[%s] get_categories failed: %s", self._dcc_name, exc)
            return []

    def get_skill_tags(self, dcc_name: str | None = None) -> list[str]:
        """Return all unique tags for this DCC.

        Args:
            dcc_name: Override the DCC filter.

        Returns:
            Sorted list of tag strings.

        """
        registry = self.registry
        if registry is None:
            return []
        effective_dcc = dcc_name if dcc_name is not None else self._dcc_name
        try:
            return list(registry.get_tags(dcc_name=effective_dcc))
        except Exception as exc:
            logger.debug("[%s] get_tags failed: %s", self._dcc_name, exc)
            return []

    def unregister_skill(self, name: str, dcc_name: str | None = None) -> None:
        """Unregister a skill from the action registry.

        Args:
            name: Canonical action name (e.g. ``"blender_scene__create_cube"``).
            dcc_name: Scope to a specific DCC; ``None`` means global.

        """
        registry = self.registry
        if registry is None:
            logger.warning("[%s] Registry unavailable; cannot unregister %r", self._dcc_name, name)
            return
        try:
            registry.unregister(name, dcc_name=dcc_name)
        except Exception as exc:
            logger.debug("[%s] unregister(%r) failed: %s", self._dcc_name, name, exc)

    def find_skills(
        self,
        query: str | None = None,
        tags: list[str] | None = None,
        dcc: str | None = None,
    ) -> list[Any]:
        """Search the SkillCatalog by query / tags / DCC filter.

        Args:
            query: Free-text matched against name, description, and search_hint.
            tags: All listed tags must be present on the skill.
            dcc: Restrict to skills targeting this DCC.

        Returns:
            List of ``SkillSummary`` objects.

        """
        try:
            return list(self._server.find_skills(query=query, tags=tags, dcc=dcc))
        except Exception as exc:
            logger.debug("[%s] find_skills failed: %s", self._dcc_name, exc)
            return []

    def is_skill_loaded(self, name: str) -> bool:
        """Check whether a skill is currently loaded.

        Args:
            name: Skill name.

        Returns:
            ``True`` if loaded.

        """
        try:
            return bool(self._server.is_loaded(name))
        except Exception as exc:
            logger.debug("[%s] is_loaded(%r) failed: %s", self._dcc_name, name, exc)
            return False

    def get_skill_info(self, name: str) -> Any | None:
        """Return full metadata for a skill.

        Args:
            name: Skill name.

        Returns:
            ``SkillMetadata`` or ``None`` if not found.

        """
        try:
            return self._server.get_skill_info(name)
        except Exception as exc:
            logger.debug("[%s] get_skill_info(%r) failed: %s", self._dcc_name, name, exc)
            return None

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

    def start(self) -> Any:
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

        self._handle = self._server.start()
        logger.info("[%s] MCP server started at %s", self._dcc_name, self._handle.mcp_url())

        # Start gateway election thread if appropriate
        gateway_port = getattr(self._config, "gateway_port", 0)
        if self._enable_gateway_failover and gateway_port and gateway_port > 0 and self._gateway_election is None:
            try:
                self._gateway_election = DccGatewayElection(
                    dcc_name=self._dcc_name,
                    server=self,
                    gateway_port=gateway_port,
                )
                self._gateway_election.start()
                logger.info("[%s] Gateway failover election enabled", self._dcc_name)
            except Exception as exc:
                logger.warning("[%s] Failed to start gateway election: %s", self._dcc_name, exc)

        return self._handle

    def stop(self) -> None:
        """Gracefully stop the server and gateway election thread."""
        if self._gateway_election is not None:
            try:
                self._gateway_election.stop()
            except Exception as exc:
                logger.warning("[%s] Error stopping gateway election: %s", self._dcc_name, exc)
            finally:
                self._gateway_election = None

        if self._hot_reloader is not None:
            with contextlib.suppress(Exception):
                self._hot_reloader.disable()

        if self._handle is not None:
            try:
                self._handle.shutdown()
            except Exception as exc:
                logger.warning("[%s] Error stopping server: %s", self._dcc_name, exc)
            finally:
                self._handle = None
            logger.info("[%s] MCP server stopped", self._dcc_name)

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
    ) -> bool:
        """Update scene / version metadata in the gateway registry.

        Modifies the live registry entry without restarting the server.
        Also updates the in-memory ``McpHttpConfig`` so future heartbeats
        include the new values.

        Args:
            scene: New scene file path.
            version: New DCC application version string.

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
            # Update in-memory config first
            if scene is not None:
                self._config.scene = scene
            if version is not None:
                self._config.dcc_version = version

            from dcc_mcp_core import TransportManager

            registry_dir = getattr(self._config, "registry_dir", "") or os.environ.get("DCC_MCP_REGISTRY_DIR", "")
            if not registry_dir:
                return True  # config updated locally, no registry dir to write

            mgr = TransportManager(registry_dir=registry_dir)
            instance_id = getattr(self._handle, "instance_id", None) if self._handle else None
            if instance_id:
                result = mgr.update_scene(
                    self._dcc_name,
                    instance_id,
                    scene=scene,
                    version=version,
                )
                return bool(result)
            return True

        except Exception as exc:
            logger.error("[%s] Failed to update gateway metadata: %s", self._dcc_name, exc)
            return False

    def get_gateway_election_status(self) -> dict:
        """Return gateway election thread status.

        Returns:
            Dict with ``enabled``, ``running``, ``consecutive_failures``.

        """
        if self._gateway_election is None:
            return {
                "enabled": self._enable_gateway_failover,
                "running": False,
                "consecutive_failures": 0,
            }
        status = self._gateway_election.get_status()
        status["enabled"] = self._enable_gateway_failover
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

    def __repr__(self) -> str:
        status = "running" if self.is_running else "stopped"
        return f"{type(self).__name__}(dcc={self._dcc_name!r}, status={status})"
