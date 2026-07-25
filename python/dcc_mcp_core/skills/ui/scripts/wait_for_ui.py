"""wait_for_ui — Poll until a widget appears or disappears."""
from __future__ import annotations

import json
import subprocess
import time
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


def wait_for_ui(
    query: str,
    condition: str = "appears",
    timeout_secs: int = 30,
    poll_interval_ms: int = 500,
    window_title: str | None = None,
) -> dict[str, Any]:
    """Wait for a widget to appear or disappear.

    Args:
        query: Widget text, type, or role.
        condition: 'appears' or 'disappears'.
        timeout_secs: Maximum wait time.
        poll_interval_ms: Polling interval.
        window_title: Window scope.

    Returns:
        Wait result with widget descriptor (for 'appears') or confirmation (for 'disappears').
    """
    instance = _get_first_instance()
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    poll_secs = poll_interval_ms / 1000.0
    deadline = time.time() + timeout_secs
    last_found: list[dict[str, Any]] = []

    while time.time() < deadline:
        try:
            args: dict[str, Any] = {"query": query, "limit": 5}
            if window_title:
                args["window_title"] = window_title

            result = subprocess.run(
                ["dcc-mcp-cli", "call", "{}.qt_ui_inspector__find_widgets".format(instance),
                 "--json", json.dumps(args), "--output", "json"],
                capture_output=True, text=True, timeout=10,
            )
            if result.returncode == 0:
                data = json.loads(result.stdout)
                widgets = data.get("widgets", data.get("results", []))
                found = len(widgets) > 0

                if condition == "appears" and found:
                    return {
                        "success": True,
                        "condition": condition,
                        "found": True,
                        "wait_secs": round(timeout_secs - (deadline - time.time()), 1),
                        "widget": widgets[0],
                    }
                elif condition == "disappears" and not found:
                    return {
                        "success": True,
                        "condition": condition,
                        "found": False,
                        "wait_secs": round(timeout_secs - (deadline - time.time()), 1),
                    }
                elif found:
                    last_found = widgets
        except Exception:
            pass

        time.sleep(poll_secs)

    if condition == "appears":
        return {
            "success": True,
            "condition": condition,
            "found": False,
            "timeout": True,
            "wait_secs": timeout_secs,
            "widget": None,
            "error": "Widget '{}' did not appear within {} seconds.".format(query, timeout_secs),
        }
    else:
        return {
            "success": True,
            "condition": condition,
            "found": True,
            "timeout": True,
            "wait_secs": timeout_secs,
            "remaining_widgets": last_found[:5] if last_found else [],
            "error": "Widget '{}' did not disappear within {} seconds.".format(query, timeout_secs),
        }
