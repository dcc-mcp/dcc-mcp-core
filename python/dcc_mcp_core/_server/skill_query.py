"""Skill query / management collaborator for :class:`DccServerBase` (#486).

Bundles the 11 read-mostly methods that were previously inline on
``DccServerBase``: ``list_actions``, ``list_skills``, ``search_skills``,
``load_skill``, ``get_skill``, ``load_skill_object``, ``unload_skill``,
``search_actions``, ``get_skill_categories``, ``get_skill_tags``,
``unregister_skill``, ``is_skill_loaded``, ``get_skill_info``.

All methods preserve the original tolerant-of-failure semantics: any
underlying exception is logged at DEBUG level and a sensible empty value
is returned. ``DccServerBase`` keeps the existing public methods and
delegates straight through to this client.
"""

from __future__ import annotations

import logging
from typing import Any
from typing import Callable

logger = logging.getLogger(__name__)


class SkillQueryClient:
    """Thin façade over the inner ``McpHttpServer`` and its ``ToolRegistry``."""

    def __init__(self, server: Any, dcc_name: str) -> None:
        self._server = server
        self._dcc_name = dcc_name

    @property
    def registry(self) -> Any | None:
        """The underlying ``ToolRegistry``, or ``None`` if unavailable."""
        try:
            return self._server.registry
        except Exception:
            return None

    def list_actions(self, dcc_name: str | None = None) -> list[Any]:
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
        try:
            return list(self._server.list_skills())
        except Exception as exc:
            logger.debug("[%s] list_skills failed: %s", self._dcc_name, exc)
            return []

    def search_skills(
        self,
        query: str | None = None,
        tags: list[str] | None = None,
        dcc: str | None = None,
        scope: str | None = None,
        limit: int | None = None,
    ) -> list[Any]:
        try:
            return list(self._server.search_skills(query=query, tags=tags or [], dcc=dcc, scope=scope, limit=limit))
        except Exception as exc:
            logger.debug("[%s] search_skills failed: %s", self._dcc_name, exc)
            return []

    def load_skill(self, name: str) -> bool:
        try:
            self._server.load_skill(name)
            return True
        except Exception as exc:
            logger.debug("[%s] load_skill(%r) failed: %s", self._dcc_name, name, exc)
            return False

    def get_skill(self, name: str) -> Any | None:
        """Return a detached mutable ``SkillMetadata`` object, if present."""
        try:
            return self._server.get_skill(name)
        except Exception as exc:
            logger.debug("[%s] get_skill(%r) failed: %s", self._dcc_name, name, exc)
            return None

    def load_skill_object(self, skill: Any) -> bool:
        """Load a caller-supplied ``SkillMetadata`` object through core."""
        try:
            self._server.load_skill_object(skill)
            return True
        except Exception as exc:
            logger.debug("[%s] load_skill_object failed: %s", self._dcc_name, exc)
            return False

    def set_skill_load_transform(self, transform: Callable[[Any], Any] | None) -> bool:
        """Register a pre-load metadata transform on the inner skill catalog."""
        setter = getattr(self._server, "set_skill_load_transform", None)
        if not callable(setter):
            logger.debug("[%s] set_skill_load_transform unavailable on inner server", self._dcc_name)
            return False
        try:
            setter(transform)
            return True
        except Exception as exc:
            logger.debug("[%s] set_skill_load_transform failed: %s", self._dcc_name, exc)
            return False

    def clear_skill_load_transform(self) -> bool:
        """Remove a previously registered pre-load metadata transform."""
        clearer = getattr(self._server, "clear_skill_load_transform", None)
        if callable(clearer):
            try:
                clearer()
                return True
            except Exception as exc:
                logger.debug("[%s] clear_skill_load_transform failed: %s", self._dcc_name, exc)
                return False
        return self.set_skill_load_transform(None)

    def set_after_load_skill_hook(self, hook: Callable[[Any, list[str]], Any] | None) -> bool:
        """Register an after-load observer on the inner skill catalog."""
        setter = getattr(self._server, "set_after_load_skill_hook", None)
        if not callable(setter):
            logger.debug("[%s] set_after_load_skill_hook unavailable on inner server", self._dcc_name)
            return False
        try:
            setter(hook)
            return True
        except Exception as exc:
            logger.debug("[%s] set_after_load_skill_hook failed: %s", self._dcc_name, exc)
            return False

    def clear_after_load_skill_hook(self) -> bool:
        """Remove a previously registered after-load observer."""
        clearer = getattr(self._server, "clear_after_load_skill_hook", None)
        if callable(clearer):
            try:
                clearer()
                return True
            except Exception as exc:
                logger.debug("[%s] clear_after_load_skill_hook failed: %s", self._dcc_name, exc)
                return False
        return self.set_after_load_skill_hook(None)

    def set_after_unload_skill_hook(self, hook: Callable[[str, list[str]], Any] | None) -> bool:
        """Register an after-unload observer on the inner skill catalog (#1405)."""
        setter = getattr(self._server, "set_after_unload_skill_hook", None)
        if not callable(setter):
            logger.debug(
                "[%s] set_after_unload_skill_hook unavailable on inner server",
                self._dcc_name,
            )
            return False
        try:
            setter(hook)
            return True
        except Exception as exc:
            logger.debug("[%s] set_after_unload_skill_hook failed: %s", self._dcc_name, exc)
            return False

    def clear_after_unload_skill_hook(self) -> bool:
        """Remove the after-unload observer, if one is registered."""
        clearer = getattr(self._server, "clear_after_unload_skill_hook", None)
        if callable(clearer):
            try:
                clearer()
                return True
            except Exception as exc:
                logger.debug("[%s] clear_after_unload_skill_hook failed: %s", self._dcc_name, exc)
                return False
        return self.set_after_unload_skill_hook(None)

    def set_after_group_change_hook(self, hook: Callable[[str, bool], Any] | None) -> bool:
        """Register an after-group-change observer (#1405)."""
        setter = getattr(self._server, "set_after_group_change_hook", None)
        if not callable(setter):
            logger.debug(
                "[%s] set_after_group_change_hook unavailable on inner server",
                self._dcc_name,
            )
            return False
        try:
            setter(hook)
            return True
        except Exception as exc:
            logger.debug("[%s] set_after_group_change_hook failed: %s", self._dcc_name, exc)
            return False

    def clear_after_group_change_hook(self) -> bool:
        """Remove the after-group-change observer, if one is registered."""
        clearer = getattr(self._server, "clear_after_group_change_hook", None)
        if callable(clearer):
            try:
                clearer()
                return True
            except Exception as exc:
                logger.debug("[%s] clear_after_group_change_hook failed: %s", self._dcc_name, exc)
                return False
        return self.set_after_group_change_hook(None)

    def replay_loaded_skills(self, state_json: str, *, policy: str = "skip_on_drift") -> str | None:
        """Replay a persisted catalog snapshot on the inner skill server (#1405).

        Returns the ``ReplayReport`` as a JSON string when the inner
        server exposes the method, or ``None`` if the binding is absent
        (older host wheels).
        """
        replay = getattr(self._server, "replay_loaded_skills", None)
        if not callable(replay):
            logger.debug("[%s] replay_loaded_skills unavailable on inner server", self._dcc_name)
            return None
        try:
            return replay(state_json, policy)
        except Exception as exc:
            logger.debug("[%s] replay_loaded_skills failed: %s", self._dcc_name, exc)
            return None

    def unload_skill(self, name: str) -> bool:
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
        registry = self.registry
        if registry is None:
            return []
        try:
            return list(registry.get_categories())
        except Exception as exc:
            logger.debug("[%s] get_categories failed: %s", self._dcc_name, exc)
            return []

    def get_skill_tags(self, dcc_name: str | None = None) -> list[str]:
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
        registry = self.registry
        if registry is None:
            logger.warning("[%s] Registry unavailable; cannot unregister %r", self._dcc_name, name)
            return
        try:
            registry.unregister(name, dcc_name=dcc_name)
        except Exception as exc:
            logger.debug("[%s] unregister(%r) failed: %s", self._dcc_name, name, exc)

    def is_skill_loaded(self, name: str) -> bool:
        try:
            return bool(self._server.is_loaded(name))
        except Exception as exc:
            logger.debug("[%s] is_loaded(%r) failed: %s", self._dcc_name, name, exc)
            return False

    def get_skill_info(self, name: str) -> Any | None:
        try:
            return self._server.get_skill_info(name)
        except Exception as exc:
            logger.debug("[%s] get_skill_info(%r) failed: %s", self._dcc_name, name, exc)
            return None
