"""interact_widget — Click, type, or select on a found widget."""
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


def interact_widget(
    widget: dict[str, Any],
    action: str = "click",
    value: str | None = None,
    coordinates: dict[str, int] | None = None,
) -> dict[str, Any]:
    """Interact with a widget.

    Args:
        widget: Widget descriptor from find_widget or inspect_ui.
        action: click, double_click, type, select, toggle, right_click.
        value: Text to type or item to select.
        coordinates: Fallback {x, y} coordinates.

    Returns:
        Action result.
    """
    instance = _get_first_instance()
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    # Build interaction args
    control_id = widget.get("control_id", "")
    widget_path = widget.get("widget_path", "")
    window_title = widget.get("window_title", "")

    # Prefer semantic targeting via ui_control
    # Build a find + act sequence
    target: dict[str, Any] = {}
    if control_id:
        target["control_id"] = control_id
    elif widget_path:
        target["widget_path"] = widget_path
    elif coordinates:
        target["x"] = coordinates.get("x", 0)
        target["y"] = coordinates.get("y", 0)
    else:
        return {"success": False, "error": "No target: provide control_id, widget_path, or coordinates."}

    act_args: dict[str, Any] = {
        "action": action,
        "target": target,
    }
    if value:
        act_args["value"] = value

    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "{}.ui_control__act".format(instance),
             "--json", json.dumps(act_args), "--output", "json"],
            capture_output=True, text=True, timeout=20,
        )
        if result.returncode == 0:
            data = json.loads(result.stdout)
            return {
                "success": True,
                "action": action,
                "target": target,
                "result": data,
            }
        return {
            "success": False,
            "action": action,
            "error": result.stderr,
        }
    except Exception as e:
        return {"success": False, "action": action, "error": str(e)}
