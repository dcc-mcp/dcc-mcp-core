"""Generic skill hot-reload support for any DCC adapter.

Monitors skill directories for changes and automatically reloads affected skills
without requiring a server restart.

Uses the ``SkillWatcher`` from dcc-mcp-core (v0.12.24+), which:
- Monitors directories with platform-native APIs (inotify/FSEvents/ReadDirectoryChangesW)
- Debounces rapid events (default 300ms) to avoid excessive reloads
- Runs on a background thread, never blocking the DCC application

This module is DCC-agnostic and works for Maya, Blender, Unreal, ZBrush, etc.
DCC-specific adapters should use :class:`DccSkillHotReloader` directly instead
of writing their own hot-reload classes.

Usage example::

    from dcc_mcp_core.hotreload import DccSkillHotReloader

    class BlenderMcpServer:
        def __init__(self, ...):
            ...
            self._hot_reloader = DccSkillHotReloader(dcc_name="blender", server=self)

        def enable_hot_reload(self, debounce_ms=300) -> bool:
            skill_paths = self._collect_skill_paths()
            return self._hot_reloader.enable(skill_paths, debounce_ms=debounce_ms)

        def disable_hot_reload(self) -> None:
            self._hot_reloader.disable()
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import logging
import threading
from typing import Any

logger = logging.getLogger(__name__)


class DccSkillHotReloader:
    """Generic skill hot-reload manager for any DCC adapter.

    Wraps dcc-mcp-core's ``SkillWatcher`` and handles the full lifecycle:
    watching directories, debouncing events, and reloading skills.

    This class is intentionally DCC-agnostic. Each DCC adapter instantiates
    it with its own ``dcc_name`` and server reference.

    Example (Blender adapter)::

        reloader = DccSkillHotReloader(dcc_name="blender", server=self)
        reloader.enable(["/path/to/skills"], debounce_ms=300)
        # ... files are now monitored, skills reload automatically ...
        reloader.disable()

    Args:
        dcc_name: Short DCC identifier used for log messages (e.g. ``"blender"``).
        server: The DCC MCP server instance. Must expose ``_server`` (the inner
            ``McpHttpServer``) with ``list_skills()`` and ``load_skill()`` methods.

    """

    def __init__(self, dcc_name: str, server: Any) -> None:
        self._dcc_name = dcc_name
        self._server = server
        self._watcher: Any | None = None
        self._watched_paths: list[str] = []
        self._lock = threading.Lock()
        self._reload_count = 0
        self._enabled = False

    # ── properties ────────────────────────────────────────────────────────────

    @property
    def is_enabled(self) -> bool:
        """Whether hot-reload is currently active."""
        return self._enabled

    @property
    def reload_count(self) -> int:
        """Total number of reload events triggered."""
        return self._reload_count

    @property
    def watched_paths(self) -> list[str]:
        """List of directories currently being monitored."""
        with self._lock:
            return list(self._watched_paths)

    # ── public API ────────────────────────────────────────────────────────────

    def enable(
        self,
        skill_paths: list[str] | None = None,
        debounce_ms: int = 300,
    ) -> bool:
        """Enable hot-reload for the given skill directories.

        Starts a ``SkillWatcher`` to monitor the provided directories. When
        SKILL.md or script files are modified, affected skills are automatically
        unloaded and reloaded.

        Args:
            skill_paths: List of directories to watch. If ``None``, the caller
                must supply paths; passing an empty list is a no-op.
            debounce_ms: Milliseconds to wait after the last change event before
                triggering a reload. Default 300ms.

        Returns:
            ``True`` if hot-reload was successfully enabled, ``False`` on error.

        """
        with self._lock:
            if self._enabled:
                logger.warning("[%s] Hot-reload already enabled", self._dcc_name)
                return True

            try:
                from dcc_mcp_core import SkillWatcher
            except ImportError:
                logger.error(
                    "[%s] SkillWatcher not available (requires dcc-mcp-core >= 0.12.24)",
                    self._dcc_name,
                )
                return False

            paths_to_watch: list[str] = list(skill_paths or [])
            if not paths_to_watch:
                logger.warning("[%s] No skill paths supplied; hot-reload not enabled", self._dcc_name)
                return False

            try:
                self._watcher = SkillWatcher(debounce_ms=debounce_ms)
                successfully_watched: list[str] = []

                for path in paths_to_watch:
                    try:
                        self._watcher.watch(path)
                        successfully_watched.append(path)
                        logger.debug("[%s] Hot-reload watching: %s", self._dcc_name, path)
                    except Exception as exc:
                        logger.warning("[%s] Failed to watch %r: %s", self._dcc_name, path, exc)

                if not successfully_watched:
                    logger.warning("[%s] No paths were successfully watched", self._dcc_name)
                    self._watcher = None
                    return False

                self._watched_paths = successfully_watched
                self._enabled = True
                logger.info(
                    "[%s] Hot-reload enabled for %d path(s)",
                    self._dcc_name,
                    len(self._watched_paths),
                )
                return True

            except Exception as exc:
                logger.error("[%s] Failed to enable hot-reload: %s", self._dcc_name, exc)
                self._watcher = None
                self._enabled = False
                return False

    def disable(self) -> None:
        """Disable hot-reload and clean up the SkillWatcher."""
        with self._lock:
            was_enabled = self._enabled
            self._watcher = None
            self._watched_paths.clear()
            self._enabled = False
        if was_enabled:
            logger.info("[%s] Hot-reload disabled", self._dcc_name)

    def reload_now(self) -> int:
        """Manually trigger a reload of all monitored skills.

        Useful for debugging or when a change occurred outside the watcher loop.

        Returns:
            Number of skills successfully reloaded.

        """
        if not self._enabled or self._watcher is None:
            logger.warning("[%s] Hot-reload is not enabled", self._dcc_name)
            return 0

        # Snapshot the watcher reference outside the lock before doing I/O.
        with self._lock:
            watcher = self._watcher
            if watcher is None:
                return 0

        try:
            watcher.reload()
            self._reload_count += 1

            reloaded = 0
            inner = getattr(self._server, "_server", None)
            if inner is not None:
                try:
                    for summary in inner.list_skills():
                        skill_name = summary.name if hasattr(summary, "name") else summary.get("name")
                        if skill_name:
                            try:
                                inner.load_skill(skill_name)
                                reloaded += 1
                            except Exception as exc:
                                logger.debug(
                                    "[%s] Failed to reload skill %r: %s",
                                    self._dcc_name,
                                    skill_name,
                                    exc,
                                )
                except Exception as exc:
                    logger.warning("[%s] Error listing skills during reload: %s", self._dcc_name, exc)

            logger.info("[%s] Manual reload triggered: %d skills reloaded", self._dcc_name, reloaded)
            return reloaded

        except Exception as exc:
            logger.error("[%s] Manual reload failed: %s", self._dcc_name, exc)
            return 0

    def get_stats(self) -> dict:
        """Return hot-reload statistics.

        Returns:
            Dict with keys ``enabled``, ``watched_paths``, ``reload_count``.

        """
        return {
            "enabled": self._enabled,
            "watched_paths": self.watched_paths,
            "reload_count": self._reload_count,
        }

    def __repr__(self) -> str:
        status = "enabled" if self._enabled else "disabled"
        return (
            f"DccSkillHotReloader(dcc={self._dcc_name!r}, "
            f"status={status}, "
            f"watched={len(self._watched_paths)}, "
            f"reloads={self._reload_count})"
        )
