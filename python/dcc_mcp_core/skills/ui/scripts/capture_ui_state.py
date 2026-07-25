"""capture_ui_state — Comprehensive UI state bundle for debugging."""
from __future__ import annotations

import json
import os
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


def _call_tool(instance: str, tool: str, args: dict, timeout: int = 15) -> dict[str, Any] | None:
    """Call a tool."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", "{}.{}".format(instance, tool),
             "--json", json.dumps(args), "--output", "json"],
            capture_output=True, text=True, timeout=timeout,
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
        return None
    except Exception:
        return None


def capture_ui_state(
    window_title: str | None = None,
    output_dir: str | None = None,
    include_raw_tree: bool = False,
) -> dict[str, Any]:
    """Capture comprehensive UI state.

    Args:
        window_title: Focus on a specific window.
        output_dir: Directory for saved artifacts.
        include_raw_tree: Include unfiltered widget tree.

    Returns:
        Bundle metadata with saved file paths.
    """
    instance = _get_first_instance()
    if not instance:
        return {"success": False, "error": "No ready DCC instance found."}

    ts = int(time.time())
    output_dir = output_dir or os.path.join(os.environ.get("TEMP", "/tmp"), "dcc-mcp-ui-state")
    os.makedirs(output_dir, exist_ok=True)

    saved: list[str] = []

    # Screenshot
    ss_path = os.path.join(output_dir, "screenshot-{}.png".format(ts))
    ss = _call_tool(instance, "dcc_diagnostics__screenshot",
                    {"format": "png", "save_path": ss_path})
    if ss and os.path.isfile(ss_path):
        saved.append(ss_path)

    # Widget tree
    tree_args: dict[str, Any] = {} if include_raw_tree else {"max_depth": 10}
    if window_title:
        tree_args["window_title"] = window_title
    tree = _call_tool(instance, "qt_ui_inspector__snapshot_tree", tree_args)
    tree_path = os.path.join(output_dir, "widget-tree-{}.json".format(ts))
    if tree:
        with open(tree_path, "w", encoding="utf-8") as f:
            json.dump(tree, f, indent=2)
        saved.append(tree_path)

    # Window list
    windows = _call_tool(instance, "qt_ui_inspector__list_windows", {})
    windows_path = os.path.join(output_dir, "windows-{}.json".format(ts))
    if windows:
        with open(windows_path, "w", encoding="utf-8") as f:
            json.dump(windows, f, indent=2)
        saved.append(windows_path)

    return {
        "success": True,
        "instance": instance,
        "timestamp": ts,
        "output_dir": output_dir,
        "saved_files": saved,
        "has_screenshot": any("screenshot" in f for f in saved),
        "has_widget_tree": any("widget-tree" in f for f in saved),
        "has_window_list": any("windows" in f for f in saved),
    }
