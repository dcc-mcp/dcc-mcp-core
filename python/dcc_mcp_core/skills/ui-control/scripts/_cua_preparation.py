"""Explicit, non-retrying foreground preparation on one retained CUA client."""

from __future__ import annotations

from typing import Any
from typing import Callable

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions
from dcc_mcp_core.skill import skill_error


def _exact_target(value: Any, expected: dict[str, int]) -> None:
    if not isinstance(value, dict) or any(
        type(value.get(key)) is not int or value[key] != item for key, item in expected.items()
    ):
        raise CuaCliError("invalid_target", "Foreground preparation requires the same exact PID and HWND throughout.")


def _foreground_state(raw: dict[str, Any], target: dict[str, int]) -> dict[str, Any]:
    state = raw.get("state")
    _exact_target(state, target)
    if any(
        state.get(key) is not value
        for key, value in (("exists", True), ("visible", True), ("minimized", False), ("foreground", True))
    ):
        raise CuaCliError("foreground_activation_refused", "The exact target is not visible in the foreground.")
    return state


def run(
    params: dict[str, Any], policy: Any, *, session_id: str, resolve: Callable, capture: Callable, host_error: Callable
) -> dict[str, Any]:
    """Compose authorized window mutation, readback and capture; never send content input."""
    stage = "binding"
    receipts: dict[str, Any] = {"stage": stage, "content_input_sent": False}
    entry = None

    def failed(result):
        if entry is not None:
            entry["snapshot_id"] = None
            entry["snapshot"] = None
        context = result.setdefault("context", {})
        context.update(
            foreground_preparation={**receipts, "stage": stage, "fresh_observation_ready": False},
            fresh_observation_required=True,
            blind_retry=False,
        )
        return result

    try:
        if (
            not isinstance(params.get("session_id"), str)
            or not params["session_id"].strip()
            or params["session_id"] != session_id
        ):
            raise CuaCliError("invalid_request", "Supply the retained UI session_id explicitly.")
        target = {key: params.get(key) for key in ("process_id", "window_handle")}
        if any(type(value) is not int or value <= 0 for value in target.values()):
            raise CuaCliError("invalid_target", "Supply the exact positive process_id and window_handle.")
        operation = params.get("operation")
        if (
            not isinstance(operation, str)
            or operation not in {"activate", "restore_activate"}
            or params.get("resume_computer_use")
        ):
            raise CuaCliError(
                "invalid_request", "Choose activate or restore_activate; preparation never resumes input."
            )
        stage = "authorization"
        action = UiActionKind.ACTIVATE_WINDOW if operation == "activate" else UiActionKind.RESTORE_WINDOW
        if not policy.allow_snapshot or not policy.allows_action(action):
            return failed(
                skill_error("Foreground preparation requires snapshot and window-mutation policy.", "policy_disabled")
            )
        options = params.get("trusted_ui_control_runtime")
        if isinstance(options, UiControlRuntimeOptions) and operation not in options.window_operations:
            return failed(skill_error("The owner did not grant this window operation.", "permission_denied"))
        stage = "binding"
        client, entry = resolve()
        _exact_target(client.target, target)
        old_snapshot = entry.get("snapshot") or {}
        old_observation = (old_snapshot.get("metadata", {}).get("computer_use") or {}).get("observation_id")
        entry["snapshot_id"] = None
        entry["snapshot"] = None
        stage = "activation"
        activated = client.change_window_state(operation)
        receipts["activation"] = activated
        if activated.get("operation") != operation or (activated.get("result") or {}).get("success") is not True:
            raise CuaCliError("protocol_mismatch", "The exact requested activation did not complete successfully.")
        _foreground_state(activated, target)
        if (activated.get("result") or {}).get("fresh_observation_required") is not True:
            raise CuaCliError("protocol_mismatch", "Activation did not invalidate prior observations.")
        stage = "foreground_readback"
        receipts["window_state"] = _foreground_state(client.window_state(), target)
        stage = "capture"
        receipts["capture_mode"] = getattr(client, "observation_mode", "semantic")
        result = capture(client, entry)
        if not result.get("success"):
            return failed(result)
        context = result.get("context") or {}
        observation = context.get("observation") or {}
        _exact_target(observation, target)
        observation_id = observation.get("observation_id")
        if not isinstance(observation_id, str) or not observation_id or observation_id == old_observation:
            raise CuaCliError("stale_observation", "Preparation did not produce a new native observation.")
        if not context.get("snapshot_id") or not context.get("__rich__"):
            raise CuaCliError("capture_failed", "Preparation did not return a usable fresh screenshot.")
        if context.get("observation_mode") != "pixels_only" and not context.get("accessibility_available"):
            raise CuaCliError("backend_unavailable", "The semantic observation has no usable accessibility state.")
        context["foreground_preparation"] = {
            **receipts,
            "stage": "ready",
            "fresh_observation_ready": True,
            "input_permissions_unchanged": True,
        }
        return result
    except (CuaCliError, OSError, ValueError) as exc:
        return failed(host_error(exc))
