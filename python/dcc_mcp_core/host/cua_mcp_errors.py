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
        "visible",
        "win32_read_after_dwm_rejection",
        "zero_area_status_mismatch",
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
        "proof_target_root_window_handle",
        "win32_os_error",
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
_OBJECTS = frozenset(
    {
        "details",
        "capture",
        "root_bounds_failure",
        "capture_preparation",
        "native_outcome",
        "delivery",
        "native_instance",
        "target",
        "task_context",
    }
)
_RECTS = frozenset({"target_bounds", "blocker_bounds", "dwm_raw_rect_edges", "win32_raw_rect_edges"})
_ROOT_CLASSES = frozenset({"positive", "zero_area", "inverted", "overflow"})
_ROOT_ENUMS = {
    "root_role": frozenset({"target_root", "above_target_root"}),
    "dwm_classification": _ROOT_CLASSES,
    "win32_classification": _ROOT_CLASSES,
}
_ROOT_NULLABLE = frozenset(
    {"win32_raw_rect_edges", "win32_classification", "win32_os_error", "zero_area_status_mismatch", "cloaked"}
)
_TOKEN = re.compile(r"[A-Za-z0-9_.:+-]{1,128}\Z")
_INPUT_FENCE_FIELDS = frozenset(
    {
        "schema_version",
        "authorizes_input",
        "target_process_id",
        "target_window_handle",
        "point",
        "unobscured",
        "point_proof_complete",
        "point_visible",
        "hit_window_handle",
        "hit_process_id",
        "hit_root_window_handle",
        "hit_is_target_or_child",
        "redacted_rectangles",
        "redaction_count",
        "redactions_complete",
    }
)


def _unsigned(value: Any, bits: int, *, positive: bool = False) -> bool:
    return type(value) is int and int(positive) <= value < 2**bits


def _signed_coordinates(value: Any, length: int) -> bool:
    return (
        isinstance(value, list)
        and len(value) == length
        and all(type(n) is int and -(2**31) <= n < 2**31 for n in value)
    )


def _input_fence_evidence(value: Any) -> dict[str, Any] | None:
    """Accept the complete content-free failure DTO; it never grants input."""
    if not isinstance(value, dict) or value.keys() != _INPUT_FENCE_FIELDS:
        return None
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        return None
    if value["authorizes_input"] is not False:
        return None
    for field, bits in (("target_process_id", 32), ("target_window_handle", 64)):
        if not _unsigned(value[field], bits, positive=True):
            return None
    for field, bits in (("hit_process_id", 32), ("hit_window_handle", 64), ("hit_root_window_handle", 64)):
        if value[field] is not None and not _unsigned(value[field], bits):
            return None
    for field in ("unobscured", "point_proof_complete", "redactions_complete"):
        if type(value[field]) is not bool:
            return None
    for field in ("point_visible", "hit_is_target_or_child"):
        if value[field] is not None and type(value[field]) is not bool:
            return None
    point, rectangles = value["point"], value["redacted_rectangles"]
    if point is not None and not _signed_coordinates(point, 2):
        return None
    if (
        not isinstance(rectangles, list)
        or len(rectangles) > 16
        or not all(_signed_coordinates(rect, 4) for rect in rectangles)
        or not _unsigned(value["redaction_count"], 32)
        or value["redaction_count"] < len(rectangles)
        or (value["redactions_complete"] and value["redaction_count"] != len(rectangles))
    ):
        return None
    return dict(
        value, point=list(point) if point is not None else None, redacted_rectangles=[list(r) for r in rectangles]
    )


def safe_failure_evidence(payload: dict[str, Any]) -> dict[str, Any]:
    """Keep typed diagnostic fields, never text, paths, secrets, or image data."""
    budget = [128]

    def project(value: Any, depth: int = 0, parent: str | None = None) -> dict[str, Any]:
        result = {}
        if not isinstance(value, dict) or depth > 5:
            return result
        for key, item in value.items():
            if budget[0] <= 0:
                break
            if key == "native_input_fence" and depth == 1 and parent == "details":
                diagnostic = _input_fence_evidence(item)
                if diagnostic is None or budget[0] <= len(diagnostic):
                    continue
                result[key] = diagnostic
                budget[0] -= len(diagnostic)
            elif (
                (key in _BOOLEANS and type(item) is bool)
                or (key in _INTEGERS and type(item) is int and -(2**63) <= item < 2**64)
                or (key in _ENUMS and isinstance(item, str) and _TOKEN.fullmatch(item))
                or (key in _ROOT_ENUMS and isinstance(item, str) and item in _ROOT_ENUMS[key])
                or (key in _ROOT_NULLABLE and item is None)
            ):
                result[key] = item
            elif key in _OBJECTS and isinstance(item, dict):
                result[key] = project(item, depth + 1, key)
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
