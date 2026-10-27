"""Pure request validation and projection for exact native window metadata."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.host.cua_mcp_client import validate_window_frame
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

STATE_MESSAGE = "Read exact scoped application state from the CUA Host."
FRAME_MESSAGE = "Completed exact scoped window frame change without activation."
WINDOW_OPERATIONS = {
    UiActionKind.RESTORE_WINDOW: "restore",
    UiActionKind.SHOW_WINDOW: "show",
    UiActionKind.ACTIVATE_WINDOW: "activate",
    UiActionKind.MINIMIZE_WINDOW: "minimize",
}


def host_invalid_target_recovery() -> dict[str, Any]:
    """Return recovery guidance supported by the default Host route."""
    return {
        "prompt": (
            "Read the same exact PID/HWND with get_window_state. If minimized, call restore_window; "
            "if hidden but not minimized, ask the operator to restore visibility manually. Then call "
            "activate_window and retry ui_control__snapshot. These operations cannot change the "
            "authorized PID/HWND scope."
        ),
        "possible_solutions": [
            "Read the authorized window with ui_control__act(action='get_window_state').",
            "Restore the same exact window, or ask the operator to restore hidden visibility manually.",
        ],
        "recovery_actions": ["get_window_state", "restore_window", "activate_window"],
        "recovery_scope": "same_exact_pid_hwnd",
    }


def owner_invalid_target_recovery(recovery_actions: list[str]) -> dict[str, Any]:
    """Return recovery guidance bounded by the owner-selected pixels grant."""
    return {
        "prompt": (
            "Read the same exact PID/HWND with get_window_state, then use only the owner-granted "
            "window operation listed in recovery_actions. Take a fresh ui_control__snapshot afterward."
        ),
        "possible_solutions": [
            "Read the authorized window with ui_control__act(action='get_window_state').",
            "Use only the listed owner-granted operation, then take a fresh ui_control__snapshot.",
        ],
        "recovery_actions": recovery_actions,
        "recovery_scope": "same_exact_pid_hwnd",
    }


def prepare_request(params: dict[str, Any]) -> tuple[dict[str, int] | None, dict[str, Any] | None]:
    """Validate the owner ceiling and metadata-only shape without opening a client."""
    runtime = params.get("trusted_ui_control_runtime")
    if not isinstance(runtime, UiControlRuntimeOptions) or "set_frame" not in runtime.window_operations:
        return None, skill_error(
            "Set frame requires an explicit owner-selected pixels window grant.", "unsupported_action"
        )
    if any(
        params.get(key) is not None
        for key in (
            "control_id",
            "snapshot_id",
            "accessibility_state_id",
            "element_token",
            "element_index",
            "secret_handle",
        )
    ):
        return None, skill_error(
            "Set frame accepts native metadata only, without pixel or semantic tokens.", "unsupported_action"
        )
    if not isinstance(params.get("window_state_id"), str) or not params["window_state_id"]:
        return None, skill_error(
            "Read fresh get_window_state metadata before setting the frame.", UiErrorCode.STALE_OBSERVATION
        )
    return validate_window_frame(params.get("frame")), None


def state_result(
    raw: dict[str, Any],
    session_id: str,
    prompt: str | None,
    audit: dict[str, Any],
    recovery_actions: list[str] | None = None,
) -> dict[str, Any]:
    """Project a validated read-only state without minting a mutation token."""
    context: dict[str, Any] = {}
    if recovery_actions is not None:
        context["recovery_actions"] = recovery_actions
    return skill_success(
        STATE_MESSAGE,
        prompt=prompt
        or (
            "If minimized, call ui_control__act with restore_window. If hidden but not minimized, "
            "ask the operator to restore visibility manually. Then call activate_window and take a fresh snapshot."
        ),
        session_id=session_id,
        window_state=raw.get("state") or {},
        audit=audit,
        **context,
    )


def frame_result(raw: dict[str, Any], session_id: str, audit: dict[str, Any]) -> dict[str, Any]:
    """Retain the native exact completion and fresh-evidence requirement."""
    return skill_success(
        FRAME_MESSAGE,
        prompt=(
            "Read fresh get_window_state metadata before another frame change; take fresh pixels before content input."
        ),
        session_id=session_id,
        window_state=raw.get("state") or {},
        native_outcome=raw.get("result"),
        task_context=raw.get("task_context"),
        fresh_observation_required=True,
        audit=audit,
    )
