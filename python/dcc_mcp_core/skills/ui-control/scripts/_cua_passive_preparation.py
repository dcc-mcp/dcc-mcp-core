"""Canonical session orchestration for explicitly granted passive preparation."""

from __future__ import annotations

import base64
from typing import Any

from dcc_mcp_core.cancellation import DccMcpCancelledError
from dcc_mcp_core.cancellation import check_dcc_cancelled
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_client import PixelsMcpHostClient
from dcc_mcp_core.host.ui_control_options import UiControlRuntimeOptions
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def run(params: dict, policy: Any, *, session_id: str, resolve: Any, host_error: Any) -> dict:
    """Use the retained client under its existing lock; never rebind or resume input."""
    client = None
    entry = None
    operation = params.get("operation")
    dispatched = False
    try:
        allowed = {
            "session_id",
            "process_id",
            "window_handle",
            "window_title",
            "process_name",
            "policy",
            "operation",
            "window_state_id",
            "lifetime_ms",
            "trusted_adapter_scope",
            "trusted_ui_control_runtime",
        }
        if set(params) - allowed or params.get("session_id") != session_id or not params.get("session_id"):
            raise CuaCliError("invalid_request", "Use only declared arguments and an explicit retained session_id.")
        if not isinstance(operation, str) or operation not in {"begin", "state", "stop", "snapshot"}:
            raise CuaCliError("invalid_request", "Choose begin, state, stop or snapshot.")
        if operation != "begin" and {"window_state_id", "lifetime_ms"}.intersection(params):
            raise CuaCliError("invalid_request", "Only begin accepts window_state_id and lifetime_ms.")
        target = {key: params.get(key) for key in ("process_id", "window_handle")}
        if any(type(value) is not int or value <= 0 for value in target.values()):
            raise CuaCliError("invalid_target", "Supply the exact positive PID and HWND.")
        options = params.get("trusted_ui_control_runtime")
        if not isinstance(options, UiControlRuntimeOptions) or options.capture_preparation is None:
            raise CuaCliError("permission_denied", "The trusted owner did not grant passive preparation.")
        # Cleanup and status must remain reachable even when observation or mutation policy is narrowed.
        if operation in {"begin", "snapshot"} and not policy.allow_snapshot:
            return skill_error("Passive capture is disabled by policy.", "policy_disabled")
        if operation == "begin" and not policy.allow_mutating_actions:
            return skill_error("Temporary window preparation is disabled by policy.", "policy_disabled")
        client, entry = resolve()
        if not isinstance(client, PixelsMcpHostClient) or any(
            client.target.get(key) != value for key, value in target.items()
        ):
            raise CuaCliError("invalid_target", "Passive preparation requires the retained exact pixels task.")
        entry["snapshot_id"] = None
        entry["snapshot"] = None
        if operation != "begin":
            client.invalidate_action_evidence()
        if operation != "stop":
            check_dcc_cancelled()
        dispatched = True
        raw = client.preparation.call(
            operation, window_state_id=params.get("window_state_id"), lifetime_ms=params.get("lifetime_ms")
        )
        if operation != "stop":
            check_dcc_cancelled()
        status = client.preparation.status
        if operation == "stop" and not status["cleanup_verified"]:
            return skill_error(
                "Native restoration is still pending or unknown.",
                "cleanup_unknown",
                capture_preparation=status,
                cleanup_pending=True,
                input_authorized=False,
            )
        context = {
            "session_id": session_id,
            "target": client.target,
            "task_context": raw["task_context"],
            "capture_preparation": status,
            "input_authorized": False,
            "fresh_observation_required": True,
            "backend": "dcc-cua",
        }
        if operation == "snapshot":
            context["passive_evidence"] = raw["metadata"]
            context["__rich__"] = {
                "kind": "image",
                "data": base64.b64encode(raw["image_bytes"]).decode("ascii"),
                "mime": "image/png",
                "alt": "Passive prepared target pixels; input is not authorized",
            }
        return skill_success(
            "Read the native passive preparation receipt.",
            prompt="Preserve restoration state. These pixels cannot authorize input. Stop preparation when finished.",
            **context,
        )
    except DccMcpCancelledError:
        result = skill_error("Passive preparation was cancelled.", "cancelled")
    except (CuaCliError, OSError, ValueError) as exc:
        result = host_error(exc)
    if entry is not None:
        entry["snapshot_id"] = None
        entry["snapshot"] = None
    context = result.setdefault("context", {})
    context.update(input_authorized=False, fresh_observation_required=True, blind_retry=False)
    if isinstance(client, PixelsMcpHostClient):
        client.invalidate_action_evidence()
        if client.preparation.attempted:
            # One explicit best-effort revoke, never retry a stop already dispatched.
            if not (operation == "stop" and dispatched):
                try:
                    client.preparation.call("stop")
                except (CuaCliError, OSError, ValueError) as exc:
                    context["restoration_error"] = host_error(exc)
            context["capture_preparation"] = client.preparation.status
            context["cleanup_pending"] = (
                client.preparation.status is None
                or not client.preparation.status["cleanup_verified"]
                or "restoration_error" in context
            )
    return result
