"""capture_ui_state — Comprehensive UI state bundle for debugging."""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess
import time
from typing import Any


def _call_tool(instance: str, tool: str, args: dict, timeout: int = 15) -> dict[str, Any] | None:
    """Call a tool."""
    try:
        result = subprocess.run(
            ["dcc-mcp-cli", "call", f"{instance}.{tool}",
             "--json", json.dumps(args), "--output", "json"],
            capture_output=True, text=True, timeout=timeout,
        )
        if result.returncode == 0:
            return json.loads(result.stdout)
        return None
    except Exception:
        return None


def _find_value(payload: Any, key: str) -> Any:
    """Return the first matching value from a bounded CLI result tree."""
    if isinstance(payload, dict):
        if key in payload:
            return payload[key]
        for value in payload.values():
            found = _find_value(value, key)
            if found is not None:
                return found
    elif isinstance(payload, list):
        for value in payload:
            found = _find_value(value, key)
            if found is not None:
                return found
    return None


def _has_explicit_failure(payload: Any) -> bool:
    if isinstance(payload, dict):
        if payload.get("success") is False:
            return True
        return any(_has_explicit_failure(value) for value in payload.values())
    if isinstance(payload, list):
        return any(_has_explicit_failure(value) for value in payload)
    return False


def _tool_succeeded(payload: Any) -> bool:
    """Treat an explicit nested failure as a failed tool call."""
    return payload is not None and not _has_explicit_failure(payload)


def capture_ui_state(
    instance: str,
    session_id: str,
    window_title: str | None = None,
    output_dir: str | None = None,
    include_raw_tree: bool = False,
) -> dict[str, Any]:
    """Capture comprehensive UI state.

    Args:
        instance: Exact DCC instance short ID or UUID.
        session_id: Stable UI Control session ID for this capture.
        window_title: Narrow the operator-bound target window.
        output_dir: Directory for saved artifacts.
        include_raw_tree: Include unfiltered widget tree.

    Returns:
        Bundle metadata with saved file paths.

    """
    if not instance.strip() or not session_id.strip():
        return {"success": False, "error": "instance and session_id are required."}

    ts = int(time.time())
    output_path = Path(output_dir or os.environ.get("TEMP", "/tmp"))
    if output_dir is None:
        output_path /= "dcc-mcp-ui-state"
    output_path.mkdir(parents=True, exist_ok=True)

    saved: list[str] = []

    snapshot_args: dict[str, Any] = {"session_id": session_id}
    if window_title:
        snapshot_args["window_title"] = window_title
    try:
        snapshot = _call_tool(instance, "ui_control__snapshot", snapshot_args)
        if not _tool_succeeded(snapshot):
            return {"success": False, "error": "Canonical UI Control snapshot failed."}

        tree_args: dict[str, Any] = {} if include_raw_tree else {"max_depth": 10}
        if window_title:
            tree_args["window_title"] = window_title
        tree = _call_tool(instance, "qt_ui_inspector__snapshot_tree", tree_args)
        tree_path = output_path / f"widget-tree-{ts}.json"
        if tree:
            with tree_path.open("w", encoding="utf-8") as f:
                json.dump(tree, f, indent=2)
            saved.append(str(tree_path))

        windows = _call_tool(instance, "qt_ui_inspector__list_windows", {})
        windows_path = output_path / f"windows-{ts}.json"
        if windows:
            with windows_path.open("w", encoding="utf-8") as f:
                json.dump(windows, f, indent=2)
            saved.append(str(windows_path))

        provenance = _find_value(snapshot, "capture_provenance")
        rich = _find_value(snapshot, "__rich__")
        screenshot_path = rich.get("artifact_path") if isinstance(rich, dict) else None
        if screenshot_path and Path(screenshot_path).is_file():
            saved.insert(0, str(screenshot_path))
        return {
            "success": True,
            "instance": instance,
            "session_id": session_id,
            "timestamp": ts,
            "output_dir": str(output_path),
            "saved_files": saved,
            "snapshot": snapshot,
            "capture_provenance": provenance,
            "has_screenshot": bool(rich),
            "has_widget_tree": any("widget-tree" in f for f in saved),
            "has_window_list": any("windows" in f for f in saved),
        }
    except Exception as exc:
        return {"success": False, "error": f"Failed to save UI state: {exc}"}
    finally:
        _call_tool(instance, "ui_control__stop_computer_use", {"session_id": session_id})
