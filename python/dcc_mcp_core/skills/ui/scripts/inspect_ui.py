"""inspect_ui — Full UI state snapshot with widget tree and screenshot."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _call_cli(tool: str, args: dict, timeout: int = 20) -> dict[str, Any] | None:
    """Call a tool via CLI."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", tool, "--json", json.dumps(args), "--output", "json"],
            capture_output=True, text=True, timeout=timeout,
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
        return None
    except Exception:
        return None


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


def inspect_ui(
    window_title: str | None = None,
    include_widget_tree: bool = True,
    include_screenshot: bool = True,
    max_depth: int = 10,
) -> dict[str, Any]:
    """Snapshot current UI state.

    Args:
        window_title: Filter by window title.
        include_widget_tree: Include widget hierarchy.
        include_screenshot: Include visual capture.

    Returns:
        UI state snapshot.
    """
    instance = _get_first_instance()
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    result: dict[str, Any] = {
        "success": True,
        "instance": instance,
        "windows": [],
        "widget_tree": None,
        "screenshot": None,
    }

    # List windows via Qt inspector
    windows = _call_cli("{}.qt_ui_inspector__list_windows".format(instance), {})
    if windows:
        result["windows"] = windows.get("windows", [])

    # Widget tree for matching windows
    if include_widget_tree:
        tree_args: dict[str, Any] = {}
        if window_title:
            tree_args["window_title"] = window_title
        if max_depth >= 0:
            tree_args["max_depth"] = max_depth
        tree = _call_cli("{}.qt_ui_inspector__snapshot_tree".format(instance), tree_args)
        if tree:
            result["widget_tree"] = tree

    # Screenshot
    if include_screenshot:
        ss = _call_cli("{}.dcc_diagnostics__screenshot".format(instance), {"format": "png"})
        if ss:
            result["screenshot"] = ss

    return result
