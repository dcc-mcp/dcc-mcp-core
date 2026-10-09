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


def state_result(raw: dict[str, Any], session_id: str, prompt: str | None, audit: dict[str, Any]) -> dict[str, Any]:
    """Project a validated read-only state without minting a mutation token."""
    return skill_success(
        STATE_MESSAGE,
        prompt=prompt
        or (
            "If minimized, call ui_control__act with restore_window; if hidden, use show_window; "
            "then activate_window and take a fresh snapshot."
        ),
        session_id=session_id,
        window_state=raw.get("state") or {},
        audit=audit,
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
