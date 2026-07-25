"""find_widget — Search widget tree for matching elements."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _get_first_instance() -> str | None:
    """Resolve first ready instance."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "list", "--output", "json"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            return None
        data = json.loads(result.stdout)
        for inst in data.get("instances", []):
            if inst.get("direct_control", {}).get("ready"):
                return inst.get("instance_short") or inst.get("instance_id")
        if data.get("instances"):
            return data["instances"][0].get("instance_short")
    except Exception:
        pass
    return None


def find_widget(
    query: str,
    widget_type: str | None = None,
    window_title: str | None = None,
    limit: int = 20,
) -> dict[str, Any]:
    """Search for widgets matching criteria.

    Args:
        query: Search text (widget text, type, or role).
        widget_type: Filter by widget class.
        window_title: Limit to matching windows.
        limit: Max results.

    Returns:
        Matching widget descriptors.
    """
    instance = _get_first_instance()
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    # Use qt_ui_inspector__find_widgets
    args: dict[str, Any] = {"query": query, "limit": limit}
    if widget_type:
        args["widget_type"] = widget_type
    if window_title:
        args["window_title"] = window_title

    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "{}.qt_ui_inspector__find_widgets".format(instance),
             "--json", json.dumps(args), "--output", "json"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            widgets = data.get("widgets", data.get("results", []))
            return {
                "success": True,
                "query": query,
                "found": len(widgets),
                "widgets": widgets[:limit],
            }
        return {"success": False, "error": result.stderr}
    except Exception as e:
        return {"success": False, "error": str(e)}
