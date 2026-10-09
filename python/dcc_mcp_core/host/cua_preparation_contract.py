"""Validate the frozen passive-preparation DTO without minting action authority."""

from __future__ import annotations

from copy import deepcopy
from typing import Any

from dcc_mcp_core.cua_cli import CuaCliError

PHASES = frozenset({"pending_promotion", "active", "restore_pending", "cleanup_unknown", "restored", "refused"})
FAILURES = frozenset(
    [
        "invalid_binding",
        "authorization_denied",
        "parent_unavailable",
        "supervisor_not_independent",
        "target_unavailable",
        "target_outside_desktop",
        "identity_changed",
        "affected_scope_changed",
        "enumeration_incomplete",
        "desktop_unavailable",
        "geometry_changed",
        "anchor_changed",
        "mutation_failed",
        "readback_failed",
        "journal_failed",
        "gate_busy",
        "expired",
        "stopped",
        "parent_died",
        "disconnected",
        "worker_lost",
        "protocol_mismatch",
        "not_active",
        "capture_failed",
    ]
)
FORBIDDEN_TOKENS = frozenset({"observation_id", "accessibility_state_id", "element_token", "window_state_id"})


def require(condition: bool, message: str = "Malformed native capture preparation receipt.") -> None:
    """Reject a violated native receipt invariant."""
    if not condition:
        raise CuaCliError("protocol_mismatch", message)


def integer(value: Any, bits: int = 64, *, signed: bool = False) -> bool:
    """Check exact integer representation and native width, excluding booleans."""
    return type(value) is int and (-(2 ** (bits - 1)) if signed else 0) <= value < 2 ** (bits - int(signed))


def byte_id(value: Any) -> bool:
    """Recognize the native nonzero 16-byte identity representation."""
    return isinstance(value, list) and len(value) == 16 and all(integer(n, 8) for n in value) and any(value)


def no_action_tokens(value: Any, depth: int = 0) -> None:
    """Reject action authority anywhere in a passive receipt."""
    require(depth <= 16)
    if isinstance(value, dict):
        require(not FORBIDDEN_TOKENS.intersection(value), "Passive capture cannot return action evidence tokens.")
        for item in value.values():
            no_action_tokens(item, depth + 1)
    elif isinstance(value, list):
        for item in value:
            no_action_tokens(item, depth + 1)


def failure(value: Any) -> None:
    """Validate the finite nullable native failure DTO."""
    if value is None:
        return
    require(isinstance(value, dict) and set(value) == {"reason", "os_error"})
    require(isinstance(value["reason"], str) and value["reason"] in FAILURES)
    require(value["os_error"] is None or integer(value["os_error"], 32, signed=True))


def native_instance(value: Any) -> None:
    """Validate creation, thread, class and owner identity widths."""
    require(
        isinstance(value, dict)
        and set(value)
        == {"process_creation_time_100ns", "window_thread_id", "window_class_hash", "owner_window_handle"}
    )
    for key, bits in (
        ("process_creation_time_100ns", 64),
        ("window_thread_id", 32),
        ("window_class_hash", 64),
        ("owner_window_handle", 64),
    ):
        require(integer(value[key], bits) and (key == "owner_window_handle" or value[key] > 0))


def identity(value: Any) -> None:
    """Validate one exact native window and executable identity."""
    require(isinstance(value, dict) and set(value) == {"process_id", "window_handle", "native_instance", "executable"})
    require(integer(value["process_id"], 32) and value["process_id"] > 0)
    require(integer(value["window_handle"]) and value["window_handle"] > 0)
    native_instance(value["native_instance"])
    executable = value["executable"]
    require(
        isinstance(executable, dict) and set(executable) == {"canonical_image_path", "volume_serial_number", "file_id"}
    )
    require(isinstance(executable["canonical_image_path"], str) and 0 < len(executable["canonical_image_path"]) <= 4096)
    require(integer(executable["volume_serial_number"]) and byte_id(executable["file_id"]))


def rectangle(value: Any) -> bool:
    """Check a nonempty physical rectangle and signed Win32 extents."""
    return (
        isinstance(value, list)
        and len(value) == 4
        and all(integer(n, 32, signed=True) for n in value)
        and value[2] > 0
        and value[3] > 0
        and all(integer(value[i] + value[i + 2], 32, signed=True) for i in (0, 1))
    )


def window(value: Any) -> None:
    """Validate native state and its optional exact neighboring identities."""
    require(
        isinstance(value, dict)
        and set(value)
        == {"identity", "topmost", "bounds", "visible_bounds", "dpi", "visible", "minimized", "foreground", "anchors"}
    )
    identity(value["identity"])
    require(all(type(value[key]) is bool for key in ("topmost", "visible", "minimized", "foreground")))
    require(rectangle(value["bounds"]) and rectangle(value["visible_bounds"]))
    require(integer(value["dpi"], 32) and value["dpi"] > 0)
    anchors = value["anchors"]
    require(isinstance(anchors, dict) and set(anchors) == {"above", "below"})
    for item in anchors.values():
        if item is not None:
            identity(item)


def validate_status(value: Any) -> dict[str, Any]:
    """Preserve all typed status, restoration, failure and native-call fields."""
    require(
        isinstance(value, dict)
        and set(value)
        == {
            "preparation_id",
            "phase",
            "deadline_ms",
            "pending_sequence",
            "capture_revoked",
            "cleanup_verified",
            "original",
            "last_mutation",
            "failure",
            "journal_path",
            "affected_readback",
            "last_completed_sequence",
        }
    )
    require(byte_id(value["preparation_id"]) and isinstance(value["phase"], str) and value["phase"] in PHASES)
    require(integer(value["deadline_ms"]))
    require(all(value[key] is None or integer(value[key]) for key in ("pending_sequence", "last_completed_sequence")))
    require(type(value["capture_revoked"]) is bool and type(value["cleanup_verified"]) is bool)
    require(isinstance(value["journal_path"], str) and len(value["journal_path"]) <= 4096)
    failure(value["failure"])
    groups = [value["original"], value["affected_readback"]]
    mutation = value["last_mutation"]
    if mutation is not None:
        require(
            isinstance(mutation, dict)
            and set(mutation)
            == {"sequence", "kind", "returned_at_ms", "api_success", "os_error", "readback", "failure", "native_calls"}
        )
        require(integer(mutation["sequence"]) and integer(mutation["returned_at_ms"]))
        require(mutation["kind"] in ("promote", "restore") and type(mutation["api_success"]) is bool)
        require(mutation["os_error"] is None or integer(mutation["os_error"], 32, signed=True))
        failure(mutation["failure"])
        groups.append(mutation["readback"])
        calls = mutation["native_calls"]
        require(isinstance(calls, list) and len(calls) <= 32)
        for call in calls:
            require(
                isinstance(call, dict)
                and set(call) == {"window_handle", "insert_after", "api_success", "os_error", "returned_at_ms"}
            )
            require(integer(call["window_handle"]) and integer(call["insert_after"], signed=True))
            require(integer(call["returned_at_ms"]) and type(call["api_success"]) is bool)
            require(call["os_error"] is None or integer(call["os_error"], 32, signed=True))
    for group in groups:
        require(isinstance(group, list) and len(group) <= 32)
        for item in group:
            window(item)
    if value["cleanup_verified"]:
        require(
            value["capture_revoked"] and value["pending_sequence"] is None and value["phase"] in {"restored", "refused"}
        )
    if value["phase"] == "active":
        require(not value["capture_revoked"] and not value["cleanup_verified"] and value["pending_sequence"] is None)
    if not value["journal_path"]:
        require(
            value["phase"] == "pending_promotion"
            and value["pending_sequence"] == 1
            and not value["cleanup_verified"]
            and not value["original"]
            and not value["affected_readback"]
            and value["last_mutation"] is None
            and value["last_completed_sequence"] is None,
            "Only uninitialized pending preparation can omit its journal path.",
        )
    return deepcopy(value)
