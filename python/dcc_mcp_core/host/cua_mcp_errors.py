"""Content-free, bounded native failure evidence for the owned MCP transport."""

from __future__ import annotations

import re
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError

_BOOLEANS = frozenset(
    {
        "timed_out",
        "action_attempted",
        "focus_mutation_attempted",
        "effect_unknown",
        "local_session_invalidated",
        "session_remains_active",
        "automatic_input",
        "blind_retry",
        "fresh_observation_required",
        "exact_target_revalidation_required",
        "automatic_rebind",
        "explicit_rebind_required",
        "background_delivery_viable",
        "mutation_attempted",
        "cursor_move_attempted",
        "cursor_move_succeeded",
        "delivery_completed",
        "post_dispatch_validated",
        "success",
        "verification_required",
        "native_action_popups",
        "cloaked",
    }
)
_INTEGERS = frozenset(
    {
        "process_id",
        "window_handle",
        "target_process_id",
        "target_window_handle",
        "blocker_process_id",
        "blocker_window_handle",
        "process_creation_time_100ns",
        "window_thread_id",
        "window_class_hash",
        "owner_window_handle",
        "requested_events",
        "inserted_events",
        "cleanup_requested_events",
        "cleanup_inserted_events",
        "os_error",
        "cloaked",
    }
)
_ENUMS = frozenset(
    {
        "phase",
        "input_sent",
        "completion",
        "suggested_delivery_mode",
        "stage",
        "reason",
        "route",
        "effect",
        "operation",
        "provider",
        "runtime_version",
        "task_id",
    }
)
_OBJECTS = frozenset({"details", "capture", "native_outcome", "delivery", "native_instance", "target", "task_context"})
_RECTS = frozenset({"target_bounds", "blocker_bounds"})
_TOKEN = re.compile(r"[A-Za-z0-9_.:+-]{1,128}\Z")


def safe_failure_evidence(payload: dict[str, Any]) -> dict[str, Any]:
    """Keep typed diagnostic fields, never text, paths, secrets, or image data."""
    budget = [128]

    def project(value: Any, depth: int = 0) -> dict[str, Any]:
        result = {}
        if not isinstance(value, dict) or depth > 5:
            return result
        for key, item in value.items():
            if budget[0] <= 0:
                break
            if (
                (key in _BOOLEANS and type(item) is bool)
                or (key in _INTEGERS and type(item) is int and -(2**63) <= item < 2**64)
                or (key in _ENUMS and isinstance(item, str) and _TOKEN.fullmatch(item))
            ):
                result[key] = item
            elif key in _OBJECTS and isinstance(item, dict):
                result[key] = project(item, depth + 1)
            elif (
                key in _RECTS
                and isinstance(item, list)
                and len(item) == 4
                and all(type(n) is int and -(2**31) <= n < 2**31 for n in item)
            ):
                result[key] = list(item)
            else:
                continue
            budget[0] -= 1
        return result

    return project({key: payload[key] for key in ("details", "native_outcome", "task_context") if key in payload})


class OwnedCuaMcpError(CuaCliError):
    """A rejected owned MCP call with bounded safe native evidence."""

    def __init__(self, code: str, message: str, payload: dict[str, Any]) -> None:
        super().__init__(code[:128], message[:2048])
        self.native_evidence = safe_failure_evidence(payload)
        self.fresh_observation_required = False
