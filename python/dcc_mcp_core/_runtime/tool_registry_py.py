"""Minimal pure-Python tool registry for py37-lite sidecar mode."""

from __future__ import annotations

from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional


class PurePythonToolRegistry:
    """Duck-typed registry placeholder while HTTP/MCP runs in the sidecar."""

    def __init__(self) -> None:
        self._actions: Dict[str, Dict[str, Any]] = {}

    def register(
        self,
        name: str,
        *,
        description: str = "",
        category: str = "",
        tags: Optional[Iterable[str]] = None,
        dcc: str = "",
        **metadata: Any,
    ) -> None:
        self._actions[str(name)] = {
            "name": str(name),
            "description": description,
            "category": category,
            "tags": list(tags or []),
            "dcc": dcc,
            **metadata,
        }

    def list_actions(self, dcc_name: Optional[str] = None) -> List[Any]:
        if dcc_name is None:
            return list(self._actions.values())
        return [action for action in self._actions.values() if action.get("dcc") in ("", dcc_name)]
