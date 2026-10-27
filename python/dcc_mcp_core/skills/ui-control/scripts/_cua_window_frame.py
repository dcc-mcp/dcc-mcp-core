"""Pure request validation and projection for exact native window metadata."""

from __future__ import annotations

from typing import Any

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.host.cua_mcp_client import validate_window_frame
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

STATE_MESSAGE = "Read exact scoped application state from the CUA Host."
FRAME_MESSAGE = "Completed exact scoped window frame change without activation."
WINDOW_OPERATIONS = {
    UiActionKind.RESTORE_WINDOW: "restore",
    UiActionKind.ACTIVATE_WINDOW: "activate",
    UiActionKind.MINIMIZE_WINDOW: "minimize",
}


def recovery_actions(params: dict[str, Any], policy: UiControlPolicy) -> list[str]:
    """Intersect Host capabilities or owner grants with the effective policy."""
    runtime = params.get("trusted_ui_control_runtime")
    operations = (
        runtime.window_operations if isinstance(runtime, UiControlRuntimeOptions) else ("restore_activate", "activate")
    )
    return [
        action
        for operation, action in (
            (None, "get_window_state"),
            ("restore_activate", "restore_window"),
            ("activate", "activate_window"),
        )
        if (operation is None or operation in operations) and policy.allows_action(action)
    ]


def recovery_prompt(actions: list[str], state: dict[str, Any] | None = None) -> str:
    """Never imply that a restore/activate grant can show a hidden window."""
    steps = []
    if state is None and "get_window_state" in actions:
        steps.append("Read the same exact PID/HWND with get_window_state.")
    if state is not None and state.get("visible") is False and state.get("minimized") is False:
        steps.append("Ask the operator to restore visibility manually before any activation.")
    elif "restore_window" in actions:
        steps.append(
            "If minimized, call restore_window; if hidden but not minimized, "
            "ask the operator to restore visibility manually."
        )
    else:
        steps.append("Ask the operator to restore visibility manually if needed.")
    if "activate_window" in actions:
        steps.append("After visibility is restored, call activate_window if needed.")
    if "get_window_state" in actions:
        steps.append("Take a fresh ui_control__snapshot before input.")
    steps.append(
        "These operations cannot change the authorized PID/HWND scope; never rebind or retry input automatically."
    )
    return " ".join(steps)


def invalid_target_recovery(actions: list[str]) -> dict[str, Any]:
    """Use the same policy- and capability-bounded guidance on failure."""
    return {
        "prompt": recovery_prompt(actions),
        "possible_solutions": ["Ask the operator to restore this exact window manually if no listed recovery applies."],
        "recovery_actions": actions,
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


def state_result(raw: dict[str, Any], session_id: str, audit: dict[str, Any], actions: list[str]) -> dict[str, Any]:
    """Project a validated read-only state without minting a mutation token."""
    state = raw.get("state") or {}
    return skill_success(
        STATE_MESSAGE,
        prompt=recovery_prompt(actions, state),
        session_id=session_id,
        window_state=state,
        audit=audit,
        recovery_actions=[action for action in actions if action != "get_window_state"],
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
