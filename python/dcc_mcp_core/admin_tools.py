"""Administrative tools shared by every DCC adapter."""

from __future__ import annotations

from collections.abc import Callable
import json
import logging
from typing import Any

logger = logging.getLogger(__name__)


def register_admin_tools(
    server: Any,
    *,
    dcc_name: str,
    reload_skills: Callable[[], int],
) -> None:
    """Register the adapter-side skill catalog refresh contract."""
    name = "dcc_admin__reload_skills"
    try:
        server.registry.register(
            name=name,
            description="Re-scan configured skill paths without restarting the DCC.",
            input_schema=json.dumps({"type": "object", "properties": {}, "additionalProperties": False}),
            dcc=dcc_name,
            category="admin",
            version="1.0.0",
            source_file="",
        )
        server.register_handler(
            name,
            lambda _params: {
                "success": True,
                "reloaded": True,
                "skill_count": int(reload_skills()),
            },
        )
    except Exception as exc:
        logger.warning("register_admin_tools failed for %s: %s", name, exc)


__all__ = ["register_admin_tools"]
