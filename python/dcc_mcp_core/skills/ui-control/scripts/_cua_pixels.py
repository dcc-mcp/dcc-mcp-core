"""Projection of an exact pixels observation without an invented semantic tree."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.host.cua_mcp_errors import OwnedCuaMcpError
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions


def capture_snapshot(raw: dict[str, Any], client: Any, entry: dict[str, Any], session_id: str) -> dict[str, Any]:
    """Retain native identity and provenance in the bundled tool's snapshot."""
    snapshot_id = raw["observation_id"]
    snapshot = {
        "session_id": session_id,
        "root": None,
        "node_count": 0,
        "metadata": {
            "snapshot_id": snapshot_id,
            "observation_mode": "pixels_only",
            "accessibility_state_id": None,
            "ui_control": {
                "backend": "dcc-cua",
                "scope": entry["scope"],
                "target": client.target,
                "accessibility_available": False,
            },
            "computer_use": raw["observation"],
        },
    }
    entry["snapshot_id"] = snapshot_id
    entry["snapshot"] = snapshot
    return {
        "success": True,
        "snapshot_id": snapshot_id,
        "snapshot": snapshot,
        "image": raw["image_bytes"],
        "mime_type": "image/png",
        "observation": raw["observation"],
        "target": client.target,
        "task_context": raw["task_context"],
        "observation_mode": "pixels_only",
        "accessibility_state_id": None,
        "accessibility_available": False,
    }


def error_context(exc: Exception, params: dict[str, Any], *, fresh_observation: bool) -> dict[str, Any]:
    """Publish safe native failures and only applicable owner-granted recovery."""
    result = dict(exc.native_evidence) if isinstance(exc, OwnedCuaMcpError) else {}
    if fresh_observation or getattr(exc, "fresh_observation_required", False):
        result.update(fresh_observation_required=True, blind_retry=False)
    options = params.get("trusted_ui_control_runtime")
    if not isinstance(options, UiControlRuntimeOptions):
        return result
    if (
        getattr(exc, "code", None) in {"invalid_target", "target_unavailable", "target_minimized"}
        and "protected system ui" not in str(exc).lower()
    ):
        actions = ["get_window_state"]
        if "restore_activate" in options.window_operations:
            actions.append("restore_window")
        if "activate" in options.window_operations:
            actions.append("activate_window")
        result.update(
            recovery_actions=actions,
            recovery_scope="same_exact_pid_hwnd",
            prompt=(
                "Read only the same exact PID/HWND with get_window_state. "
                + (
                    "The owner permits these explicit window operations: " + ", ".join(actions[1:]) + ". "
                    if len(actions) > 1
                    else "Ask the operator to restore visibility if needed. "
                )
                + "Take fresh pixels before any input; never rebind or retry input automatically."
            ),
            possible_solutions=[
                "Use only the listed owner-granted operations for this exact window.",
                "Take a fresh ui_control__snapshot after visibility is restored.",
            ],
        )
    return result


def window_state_prompt(params: dict[str, Any]) -> str | None:
    """Do not suggest unavailable show/restore operations to pixels callers."""
    options = params.get("trusted_ui_control_runtime")
    if not isinstance(options, UiControlRuntimeOptions):
        return None
    names = [
        name
        for operation, name in (("restore_activate", "restore_window"), ("activate", "activate_window"))
        if operation in options.window_operations
    ]
    return (
        "Only these explicit owner-granted recovery actions are available: " + ", ".join(names) + ". "
        if names
        else "Ask the operator to restore visibility if needed. "
    ) + "Take fresh pixels before input."
