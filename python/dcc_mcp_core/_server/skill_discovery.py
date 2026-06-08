"""Skill discovery and registration controller for :class:`DccServerBase`.

Extracted from ``server_base.py`` (PIP-688) to own skill-path
construction, discovery, progressive (minimal) loading, hot-reload,
and builtin-skill registration.

``DccServerBase`` keeps thin public wrappers that delegate here.
"""

from __future__ import annotations

import logging
import os
from pathlib import Path
from typing import Any

from dcc_mcp_core._core import get_app_skill_paths_from_env
from dcc_mcp_core._core import get_local_skills_dir
from dcc_mcp_core._core import get_skill_paths_from_env
from dcc_mcp_core._core import get_skills_dir
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.minimal_mode import apply_minimal_mode
from dcc_mcp_core.hotreload import DccSkillHotReloader
from dcc_mcp_core.skill import get_bundled_skill_paths
from dcc_mcp_core.skills.builtin import register_all_builtin_skills

logger = logging.getLogger(__name__)


class SkillDiscoveryController:
    """Owns skill-path construction, discovery, and hot-reload for one server."""

    def __init__(self, owner: Any) -> None:
        self._owner = owner

    # -- skill search paths ---------------------------------------------------

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
        6. Marketplace-installed skills in ``~/.dcc-mcp/marketplace/{dcc_name}``
        7. Bundled skills shipped with dcc-mcp-core (when ``include_bundled=True``)
        8. Platform default skills dir
        9. Admin-UI-added skill discovery roots from the gateway SQLite lane
           (when ``include_admin_custom=True``; issue #1400)
        """
        owner = self._owner
        paths: list[str] = list(extra_paths or [])

        if owner._builtin_skills_dir.is_dir():
            paths.append(str(owner._builtin_skills_dir))

        paths.extend(get_app_skill_paths_from_env(owner._dcc_name))
        paths.extend(get_skill_paths_from_env())

        try:
            local_default_dir = get_local_skills_dir(owner._dcc_name)
            Path(local_default_dir).mkdir(parents=True, exist_ok=True)
            if local_default_dir not in paths:
                paths.append(local_default_dir)
        except Exception as exc:
            logger.debug("[%s] Could not initialise local skill path: %s", owner._dcc_name, exc)

        try:
            marketplace_root = Path(
                os.environ.get(
                    "DCC_MCP_MARKETPLACE_INSTALL_ROOT",
                    str(Path.home() / ".dcc-mcp" / "marketplace"),
                )
            )
            marketplace_dir = marketplace_root / owner._dcc_name.lower()
            if marketplace_dir.is_dir():
                marketplace_dir_str = str(marketplace_dir)
                if marketplace_dir_str not in paths:
                    paths.append(marketplace_dir_str)
        except Exception as exc:
            logger.debug("[%s] Could not resolve marketplace skill path: %s", owner._dcc_name, exc)

        if include_bundled:
            try:
                paths.extend(get_bundled_skill_paths(include_bundled=True))
            except Exception as exc:
                logger.debug("[%s] Could not load bundled skill paths: %s", owner._dcc_name, exc)

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
                    owner._dcc_name,
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

    # -- skill registration ---------------------------------------------------

    def register_builtin_skills(self, options: Any) -> None:
        """Register standard built-in skills (diagnostics, introspect, etc)."""
        owner = self._owner
        try:
            register_all_builtin_skills(
                owner._server,
                dcc_name=options.dcc_name,
                dcc_pid=owner._dcc_pid,
                dcc_window_handle=owner._dcc_window_handle,
                dcc_window_title=owner._dcc_window_title,
                gateway_failover_resolver=owner.get_gateway_election_status,
            )
        except Exception as exc:
            logger.warning("[%s] built-in skill registration failed: %s", options.dcc_name, exc)

    def register_builtin_actions(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
        minimal_mode: MinimalModeConfig | None = None,
    ) -> None:
        """Discover and (optionally) progressively load skills."""
        owner = self._owner
        if (
            owner._dcc_dispatcher is not None or owner._standalone_main_thread
        ) and not owner._inprocess_executor_registered:
            owner.register_inprocess_executor(owner._dcc_dispatcher)

        skill_paths = self.collect_skill_search_paths(
            extra_paths=extra_skill_paths,
            include_bundled=include_bundled,
            filter_existing=True,
        )
        logger.debug("[%s] Registering skills from %d path(s)", owner._dcc_name, len(skill_paths))
        try:
            count = owner._server.discover(extra_paths=skill_paths)
            logger.info("[%s] Skills discovered: %d from %d path(s)", owner._dcc_name, count, len(skill_paths))
        except Exception as exc:
            logger.warning("[%s] register_builtin_actions failed: %s", owner._dcc_name, exc)
            return

        if minimal_mode is not None:
            try:
                loaded = apply_minimal_mode(
                    owner._server,
                    minimal_mode,
                    dcc_name=owner._dcc_name,
                )
                logger.info(
                    "[%s] Minimal mode: %d skill(s) loaded eagerly",
                    owner._dcc_name,
                    loaded,
                )
            except Exception as exc:
                logger.warning("[%s] minimal_mode application failed: %s", owner._dcc_name, exc)

    def reload_skill_paths(
        self,
        extra_skill_paths: list[str] | None = None,
        include_bundled: bool = True,
    ) -> int:
        """Re-discover skills after admin-UI skill paths changed (#1400)."""
        owner = self._owner
        skill_paths = self.collect_skill_search_paths(
            extra_paths=extra_skill_paths,
            include_bundled=include_bundled,
            filter_existing=True,
        )
        logger.debug(
            "[%s] reload_skill_paths: re-scanning %d path(s)",
            owner._dcc_name,
            len(skill_paths),
        )
        try:
            count = owner._server.discover(extra_paths=skill_paths)
        except Exception as exc:
            logger.warning("[%s] reload_skill_paths failed: %s", owner._dcc_name, exc)
            return 0
        logger.info(
            "[%s] reload_skill_paths: %d skill(s) total from %d path(s)",
            owner._dcc_name,
            count,
            len(skill_paths),
        )
        return count

    # -- hot-reload -----------------------------------------------------------

    def enable_hot_reload(
        self,
        skill_paths: list[str] | None = None,
        debounce_ms: int = 300,
    ) -> bool:
        """Enable automatic skill hot-reload on file changes."""
        owner = self._owner
        if owner._hot_reloader is None:
            owner._hot_reloader = DccSkillHotReloader(dcc_name=owner._dcc_name, server=owner)

        paths = skill_paths or self.collect_skill_search_paths(include_bundled=False, filter_existing=True)
        return owner._hot_reloader.enable(paths, debounce_ms=debounce_ms)

    def disable_hot_reload(self) -> None:
        """Disable skill hot-reload."""
        owner = self._owner
        if owner._hot_reloader is not None:
            owner._hot_reloader.disable()

    @property
    def is_hot_reload_enabled(self) -> bool:
        """Whether hot-reload is currently active."""
        owner = self._owner
        return owner._hot_reloader is not None and owner._hot_reloader.is_enabled

    @property
    def hot_reload_stats(self) -> dict:
        """Hot-reload statistics (watched_paths, reload_count)."""
        owner = self._owner
        if owner._hot_reloader is None:
            return {"enabled": False, "watched_paths": [], "reload_count": 0}
        return owner._hot_reloader.get_stats()
