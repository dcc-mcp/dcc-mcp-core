"""Skill query / management collaborator for :class:`DccServerBase` (#486).

Bundles the 11 read-mostly methods that were previously inline on
``DccServerBase``: ``list_actions``, ``list_skills``, ``search_skills``,
``load_skill``, ``unload_skill``, ``search_actions``, ``get_skill_categories``,
``get_skill_tags``, ``unregister_skill``, ``is_skill_loaded``,
``get_skill_info``.

All methods preserve the original tolerant-of-failure semantics: any
underlying exception is logged at DEBUG level and a sensible empty value
is returned. ``DccServerBase`` keeps the existing public methods and
delegates straight through to this client.
"""

from __future__ import annotations

import logging
from typing import Any

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
