"""interact_widget — Click, type, or select on a found widget."""
from __future__ import annotations

import json
import subprocess
from typing import Any


def _call_tool(
    instance: str,
    tool: str,
    args: dict[str, Any],
    timeout: int = 20,
) -> tuple[dict[str, Any] | None, str | None]:
    """Call one instance-bound tool and preserve structured failures."""
    try:
        result = subprocess.run(
            [
                "dcc-mcp-cli",
                "call",
                f"{instance}.{tool}",
                "--json",
                json.dumps(args),
                "--output",
                "json",
            ],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        if result.returncode != 0:
            return None, result.stderr.strip() or f"{tool} failed"
        return json.loads(result.stdout), None
    except Exception as exc:
        return None, str(exc)


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


def _tool_succeeded(payload: dict[str, Any] | None) -> bool:
    """Treat an explicit nested failure as a failed tool call."""
    return payload is not None and not _has_explicit_failure(payload)


def interact_widget(
    widget: dict[str, Any],
    instance: str,
    session_id: str,
    action: str = "click",
    value: str | None = None,
) -> dict[str, Any]:
    """Interact with a widget.

    Args:
        widget: Widget descriptor containing a semantic control_id.
        instance: Exact DCC instance short ID or UUID.
        session_id: Stable UI Control session ID for this action.
        action: click, double_click, type, select, toggle, right_click.
        value: Text to type or item to select.

    Returns:
        Action result.

    """
    if not instance.strip() or not session_id.strip():
        return {"success": False, "error": "instance and session_id are required."}

    control_id = widget.get("control_id", "")
    window_title = widget.get("window_title", "")
    if not control_id:
        return {
            "success": False,
            "error": (
                "A semantic control_id is required. Use ui_control__snapshot/find; "
                "call ui_control__act directly for raw coordinates."
            ),
        }

    action_map = {
        "click": "click",
        "double_click": "double_click",
        "type": "set_text",
        "select": "select_option",
        "toggle": "toggle",
        "right_click": "click",
    }
    mapped_action = action_map.get(action)
    if mapped_action is None:
        return {"success": False, "action": action, "error": f"Unsupported action: {action}"}
    if action in ("type", "select") and value is None:
        return {"success": False, "action": action, "error": f"value is required for {action}."}

    snapshot_args: dict[str, Any] = {"session_id": session_id}
    if window_title:
        snapshot_args["window_title"] = window_title

    try:
        before, error = _call_tool(instance, "ui_control__snapshot", snapshot_args)
        if error or not _tool_succeeded(before):
            return {"success": False, "action": action, "error": error or "UI snapshot failed.", "result": before}

        snapshot_id = _find_value(before, "snapshot_id")
        if not snapshot_id:
            return {"success": False, "action": action, "error": "UI snapshot returned no snapshot_id."}

        act_args: dict[str, Any] = {
            "session_id": session_id,
            "snapshot_id": snapshot_id,
            "control_id": control_id,
            "action": mapped_action,
        }
        if window_title:
            act_args["window_title"] = window_title
        if action in ("type", "select"):
            act_args["text"] = value
        if action == "right_click":
            act_args["button"] = "right"

        act_result, error = _call_tool(instance, "ui_control__act", act_args)
        if error or not _tool_succeeded(act_result):
            return {"success": False, "action": action, "error": error or "UI action failed.", "result": act_result}

        after, error = _call_tool(instance, "ui_control__snapshot", snapshot_args)
        if error or not _tool_succeeded(after):
            return {
                "success": False,
                "action": action,
                "error": error or "UI action completed but verification snapshot failed.",
                "result": act_result,
            }

        return {
            "success": True,
            "instance": instance,
            "session_id": session_id,
            "action": action,
            "control_id": control_id,
            "result": act_result,
            "verification": after,
        }
    finally:
        _call_tool(instance, "ui_control__stop_computer_use", {"session_id": session_id})
