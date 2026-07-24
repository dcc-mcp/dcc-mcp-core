"""Windows ui-control proxy for the isolated native UI Control host."""

from __future__ import annotations

import base64
from contextlib import suppress
import importlib.util
import os
from pathlib import Path
import threading
import time
from typing import Any
from typing import Callable
from typing import Dict
from typing import Optional
from typing import Tuple
import uuid

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionRequest
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiControlAuditRecord
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success


def _load_sibling(name: str) -> Any:
    path = Path(__file__).with_name(f"{name}.py")
    spec = importlib.util.spec_from_file_location(f"{__name__}_{name}", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_SUPPORT = _load_sibling("_windows_uia_support")
_HOST = _load_sibling("_ui_control_host_client")
UiControlHostError = _HOST.UiControlHostError
_HostClient = _HOST.UiControlHostClient
_execute_system_operation = _HOST.execute_system_operation

_policy_from_params = _SUPPORT._policy_from_params
_scope_from_params = _SUPPORT._scope_from_params
_scope_is_trusted_native_target = _SUPPORT._scope_is_trusted_native_target
_node_from_uia_dict = _SUPPORT._node_from_uia_dict
_find_by_id = _SUPPORT._find_by_id
_find_controls = _SUPPORT._find_controls
_validate_action_limits = _SUPPORT._validate_action_limits
_is_native_action = _SUPPORT._is_native_action
_condition_from_params = _SUPPORT._condition_from_params
_condition_matches = _SUPPORT._condition_matches
_safe_session_id = _SUPPORT._safe_session_id
_session_lock = _SUPPORT._session_lock

_CLIENTS: Dict[str, Dict[str, Any]] = {}
_STOP_EVENT = threading.Event()
_CLIENTS_LOCK = threading.RLock()
_ACTIVE_CALLS: set[str] = set()
_IDLE_LEASE_SECONDS = max(
    1.0,
    float(os.environ.get("DCC_MCP_UI_CONTROL_IDLE_LEASE_SECONDS", "300")),
)
_MAX_WAIT_MS = 30_000
_INTENTS = {
    "observe",
    "activate",
    "navigate",
    "ordinary_edit",
    "login_or_permission",
    "upload",
    "move_or_rename",
    "transmit_sensitive_data",
    "delete_or_overwrite",
    "install_or_execute_download",
    "financial_transaction",
    "account_or_access_change",
    "external_communication",
    "terminal_or_run_dialog",
    "credential_or_authentication",
    "windows_security_or_privacy",
    "safety_bypass",
    "password_change",
    "escape_scope",
}
_WINDOW_STATE_OPERATIONS = {
    UiActionKind.RESTORE_WINDOW: "restore",
    UiActionKind.SHOW_WINDOW: "show",
    UiActionKind.ACTIVATE_WINDOW: "activate",
}


def _serialize_session_call(func: Callable[..., Dict[str, Any]]) -> Callable[..., Dict[str, Any]]:
    def wrapped(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        raw = dict(params or {})
        session_id = _safe_session_id(raw.get("session_id"))
        _prune_idle_clients(time.monotonic())
        with _session_lock(session_id):
            with _CLIENTS_LOCK:
                _ACTIVE_CALLS.add(session_id)
            try:
                if _STOP_EVENT.is_set():
                    return skill_error(
                        "ui_control Windows host proxy is stopping.",
                        UiErrorCode.BACKEND_UNAVAILABLE,
                        backend="windows-ui-control-host",
                    )
                return func(raw)
            finally:
                with _CLIENTS_LOCK:
                    _ACTIVE_CALLS.discard(session_id)
                    entry = _CLIENTS.get(session_id)
                    if entry is not None:
                        entry["last_activity"] = time.monotonic()

    return wrapped


def _prune_idle_clients(now: float) -> None:
    with _CLIENTS_LOCK:
        expired = [
            (session_id, entry)
            for session_id, entry in _CLIENTS.items()
            if session_id not in _ACTIVE_CALLS and now - float(entry.get("last_activity", now)) >= _IDLE_LEASE_SECONDS
        ]
    for session_id, entry in expired:
        try:
            stopped = entry["client"].stop()
        except (UiControlHostError, OSError, ValueError):
            continue
        with _CLIENTS_LOCK:
            if _CLIENTS.get(session_id) is not entry:
                continue
            if bool(stopped.get("cleanup_pending")):
                entry["last_activity"] = now
            else:
                _CLIENTS.pop(session_id, None)


def _scope_error(scope: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if scope.get("invalid_reason"):
        return skill_error(str(scope["invalid_reason"]), UiErrorCode.INVALID_TARGET)
    if not _scope_is_trusted_native_target(scope):
        return skill_error(
            (
                "Isolated DCC UI Control requires an operator-bound process id or window handle. "
                "Set DCC_MCP_UI_CONTROL_UIA_PROCESS_ID or DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE "
                "in the adapter environment."
            ),
            UiErrorCode.PERMISSION_DENIED,
        )
    if scope.get("process_names"):
        return skill_error(
            "Process-name and title-only scopes cannot mint native UI Control capabilities.",
            UiErrorCode.INVALID_TARGET,
        )
    return None


def _client_spec(session_id: str, params: Dict[str, Any], policy: UiControlPolicy) -> Dict[str, Any]:
    scope = _scope_from_params(params, policy)
    failure = _scope_error(scope)
    if failure is not None:
        raise UiControlHostError(str(failure.get("error") or "invalid_target"), str(failure.get("message") or ""))
    process_ids = scope.get("process_ids") or []
    window_handles = scope.get("window_handles") or []
    process_id = int(process_ids[0]) if len(process_ids) == 1 else None
    window_handle = int(window_handles[0]) if len(window_handles) == 1 else None
    allow_raw_input = str(os.environ.get("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT") or "").strip().lower() in {
        "1",
        "true",
        "yes",
        "on",
    }
    dcc_type = str(os.environ.get("DCC_MCP_UI_CONTROL_DCC_TYPE") or os.environ.get("DCC_MCP_DCC_TYPE") or "custom")
    task_grant_id = f"adapter:{dcc_type}:{session_id}:{process_id or 0}:{window_handle or 0}"
    return {
        "session_id": session_id,
        "task_grant_id": task_grant_id,
        "dcc_type": dcc_type,
        "process_id": process_id,
        "window_handle": window_handle,
        "allow_raw_input": allow_raw_input,
        "scope": scope,
    }


def _client_for(session_id: str, params: Dict[str, Any], policy: UiControlPolicy) -> Tuple[Any, Dict[str, Any]]:
    spec = _client_spec(session_id, params, policy)
    identity = tuple(spec[key] for key in ("dcc_type", "process_id", "window_handle", "allow_raw_input"))
    with _CLIENTS_LOCK:
        entry = _CLIENTS.get(session_id)
        if entry is not None and entry["identity"] != identity:
            with suppress(Exception):
                entry["client"].stop()
            _CLIENTS.pop(session_id, None)
            entry = None
        if entry is None:
            client = _HostClient(
                session_id=session_id,
                task_grant_id=spec["task_grant_id"],
                dcc_type=spec["dcc_type"],
                process_id=spec["process_id"],
                window_handle=spec["window_handle"],
                allow_raw_input=spec["allow_raw_input"],
            )
            entry = {
                "client": client,
                "identity": identity,
                "snapshot_id": None,
                "snapshot": None,
                "scope": spec["scope"],
                "last_activity": 0.0,
            }
            _CLIENTS[session_id] = entry
        return entry["client"], entry


def _host_error(exc: Exception) -> Dict[str, Any]:
    message = str(exc)
    code = str(getattr(exc, "code", None) or UiErrorCode.BACKEND_UNAVAILABLE)
    mapping = {
        "approval_required": "approval_required",
        "hard_denied": UiErrorCode.PERMISSION_DENIED,
        "invalid_target": UiErrorCode.INVALID_TARGET,
        "desktop_unavailable": UiErrorCode.DESKTOP_UNAVAILABLE,
        "capture_failed": "capture_failed",
        "user_interrupted": UiErrorCode.USER_INTERRUPTED,
        "stale_observation": UiErrorCode.STALE_OBSERVATION,
    }
    mapped_code = mapping.get(code, code)
    recovery: Dict[str, Any] = {}
    if mapped_code == UiErrorCode.INVALID_TARGET and "protected system ui" in message.lower():
        recovery = {
            "prompt": (
                "Protected Windows UI is covering the requested point. Call ui_control__stop for this "
                "session, then ask the operator to close or move that protected system surface manually. "
                "Do not hide, override, click through, or ignore protected system UI. After the obstruction "
                "is clear, take a fresh ui_control__snapshot for the same exact authorized PID/HWND before "
                "retrying the action."
            ),
            "possible_solutions": [
                "Stop this UI Control session so its native overlays are cleaned up.",
                "Have the operator close or move the protected Windows surface, then take a fresh snapshot.",
            ],
            "recovery_actions": ["stop", "snapshot"],
            "recovery_scope": "same_exact_pid_hwnd",
        }
    elif mapped_code == UiErrorCode.INVALID_TARGET:
        recovery = {
            "prompt": (
                "If this exact PID/HWND is still valid but minimized or hidden, call ui_control__act with "
                "get_window_state, then restore_window or show_window, optionally activate_window, and retry "
                "ui_control__snapshot. These operations cannot change the authorized PID/HWND scope."
            ),
            "possible_solutions": [
                "Read the authorized window with ui_control__act(action='get_window_state').",
                "Restore or show only that same window, then take a fresh ui_control__snapshot.",
            ],
            "recovery_actions": ["get_window_state", "restore_window", "show_window", "activate_window"],
            "recovery_scope": "same_exact_pid_hwnd",
        }
    return skill_error(
        message,
        mapped_code,
        error_code=mapped_code,
        backend="windows-ui-control-host",
        **recovery,
    )


def _system_operation_id(params: Dict[str, Any]) -> str:
    operation_id = params.get("operation_id")
    if (
        not isinstance(operation_id, str)
        or not operation_id.strip()
        or len(operation_id.encode("utf-8")) > 256
        or not operation_id.isprintable()
    ):
        raise UiControlHostError("invalid_request", "operation_id must be an explicit non-sensitive identifier")
    return operation_id


@_serialize_session_call
def system_operation_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Execute one exact typed operation from an operator-owned host grant."""
    params = dict(params or {})
    if set(params) - {"session_id", "operation_id"}:
        return skill_error(
            "The typed system operation request contains unsupported fields.",
            "invalid_request",
            backend="windows-ui-control-host",
        )
    logical_session_id = _safe_session_id(params.get("session_id"))
    system_grant_id = str(os.environ.get("DCC_MCP_UI_CONTROL_SYSTEM_GRANT_ID") or "").strip()
    if not system_grant_id:
        return skill_error(
            "No operator-owned UI Control system grant ID is configured.",
            "system_operation_not_granted",
            backend="windows-ui-control-host",
        )
    try:
        operation_id = _system_operation_id(params)
        raw = _execute_system_operation(
            session_id=f"system:{uuid.uuid4().hex}",
            system_grant_id=system_grant_id,
            operation_id=operation_id,
        )
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    return skill_success(
        "Completed an operator-granted typed system operation.",
        session_id=logical_session_id,
        operation_type=str(raw.get("operation_type") or ""),
        outcome=str(raw.get("outcome") or ""),
        policy_tier=str(raw.get("policy_tier") or ""),
        host_message=str(raw.get("message") or ""),
    )


def _capture_snapshot(
    session_id: str,
    policy: UiControlPolicy,
    params: Dict[str, Any],
) -> Dict[str, Any]:
    try:
        client, entry = _client_for(session_id, params, policy)
        if params.get("resume_computer_use"):
            client.resume()
        max_depth = max(1, min(12, int(os.environ.get("DCC_MCP_UI_CONTROL_UIA_MAX_DEPTH", "5"))))
        max_nodes = max(1, min(2_000, int(os.environ.get("DCC_MCP_UI_CONTROL_UIA_MAX_NODES", "250"))))
        raw = client.snapshot(max_depth=max_depth, max_nodes=max_nodes)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)

    snapshot_id = str(raw["accessibility_state_id"])
    root = _node_from_uia_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"uia:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= max_nodes,
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "ui_control": {
                "backend": "windows-ui-control-host",
                "scope": entry["scope"],
                "target": raw.get("target") or client.target,
                "max_depth": max_depth,
                "max_nodes": max_nodes,
            },
            "computer_use": raw.get("observation") or {},
        },
    ).to_dict()
    entry["snapshot_id"] = snapshot_id
    entry["snapshot"] = snapshot
    return {
        "success": True,
        "snapshot_id": snapshot_id,
        "snapshot": snapshot,
        "image": raw["image_bytes"],
        "mime_type": str((raw.get("image") or {}).get("mime_type") or "image/png"),
        "observation": raw.get("observation") or {},
        "target": raw.get("target") or client.target,
    }


def _capture_accessibility_snapshot(
    session_id: str,
    policy: UiControlPolicy,
    params: Dict[str, Any],
) -> Dict[str, Any]:
    try:
        client, entry = _client_for(session_id, params, policy)
        max_depth = max(1, min(12, int(os.environ.get("DCC_MCP_UI_CONTROL_UIA_MAX_DEPTH", "5"))))
        max_nodes = max(1, min(2_000, int(os.environ.get("DCC_MCP_UI_CONTROL_UIA_MAX_NODES", "250"))))
        raw = client.accessibility_snapshot(max_depth=max_depth, max_nodes=max_nodes)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    snapshot_id = str(raw["accessibility_state_id"])
    root = _node_from_uia_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"uia:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= max_nodes,
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "ui_control": {
                "backend": "windows-ui-control-host",
                "scope": entry["scope"],
                "target": raw.get("target") or client.target,
                "max_depth": max_depth,
                "max_nodes": max_nodes,
                "pixels_captured": False,
            },
        },
    ).to_dict()
    entry["snapshot_id"] = snapshot_id
    entry["snapshot"] = snapshot
    return {"success": True, "snapshot_id": snapshot_id, "snapshot": snapshot}


@_serialize_session_call
def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control snapshot disabled by policy", UiErrorCode.POLICY_DISABLED)
    capture = _capture_snapshot(session_id, policy, params)
    if not capture.get("success"):
        return capture
    return skill_success(
        "Captured isolated Windows UI Control snapshot.",
        prompt="Use ui_control__find or perform one scoped ui_control__act with this snapshot_id, then snapshot again.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        observation=capture["observation"],
        policy=policy.to_dict(),
        __rich__={
            "kind": "image",
            "data": base64.b64encode(capture["image"]).decode("ascii"),
            "mime": capture["mime_type"],
            "alt": "{} UI Control screenshot".format(params.get("app_name") or "DCC"),
        },
    )


def _recording_integer(
    params: Dict[str, Any],
    name: str,
    *,
    minimum: int,
    maximum: int,
    default: Optional[int] = None,
) -> int:
    value = params.get(name, default)
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise UiControlHostError(
            "invalid_request",
            f"{name} must be an integer in {minimum}..={maximum}.",
        )
    return value


@_serialize_session_call
def record_clip_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Record a bounded host-owned frame sequence from the exact Windows target."""
    params = dict(params or {})
    allowed = {
        "session_id",
        "process_id",
        "window_handle",
        "window_title",
        "process_name",
        "duration_ms",
        "frames_per_second",
        "jpeg_quality",
        "policy",
    }
    if set(params) - allowed:
        return skill_error(
            "The exact-window recording request contains unsupported fields.",
            "invalid_request",
            backend="windows-ui-control-host",
        )
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control recording disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        duration_ms = _recording_integer(params, "duration_ms", minimum=1_000, maximum=180_000)
        frames_per_second = _recording_integer(
            params,
            "frames_per_second",
            minimum=1,
            maximum=60,
            default=30,
        )
        jpeg_quality = _recording_integer(
            params,
            "jpeg_quality",
            minimum=70,
            maximum=100,
            default=92,
        )
        client, entry = _client_for(session_id, params, policy)
        raw = client.record_clip(
            duration_ms=duration_ms,
            frames_per_second=frames_per_second,
            jpeg_quality=jpeg_quality,
        )
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    finally:
        entry = _CLIENTS.get(session_id)
        if entry is not None:
            entry["snapshot_id"] = None
    return skill_success(
        "Recorded an exact-window JPEG sequence through the isolated UI Control host.",
        prompt=(
            "Copy and verify the host-owned artifact through a recording workflow, then call "
            "ui_control__stop_computer_use when this exact-window capture session is complete."
        ),
        session_id=session_id,
        target=raw.get("target") or client.target,
        artifact=raw.get("artifact") or {},
        audio_captured=False,
        capture_scope="exact_window",
        policy=policy.to_dict(),
    )


@_serialize_session_call
def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_find:
        return skill_error("ui_control find disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        _client, entry = _client_for(session_id, params, policy)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    cached_snapshot = entry.get("snapshot")
    cached_snapshot_id = entry.get("snapshot_id")
    capture = (
        {"success": True, "snapshot": cached_snapshot, "snapshot_id": cached_snapshot_id}
        if isinstance(cached_snapshot, dict) and cached_snapshot_id
        else _capture_snapshot(session_id, policy, params)
    )
    if not capture.get("success"):
        return capture
    matches = _find_controls(capture["snapshot"], params)
    return skill_success(
        f"Found {len(matches)} isolated Windows UI Control(s).",
        prompt="Use ui_control__act with a returned control id and snapshot_id.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def _intent(params: Dict[str, Any]) -> str:
    requested = str(params.get("intent") or "ordinary_edit").strip().lower()
    return requested if requested in _INTENTS else "ordinary_edit"


def _action_payload(params: Dict[str, Any], native: bool) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    payload = {
        "action": action,
        "control_id": str(params.get("control_id") or "") or None,
        "input_kind": "raw_input" if native else "semantic",
        "intent": _intent(params),
        "x": params.get("x"),
        "y": params.get("y"),
        "button": params.get("button"),
        "scroll_x": params.get("scroll_x"),
        "scroll_y": params.get("scroll_y"),
        "path": params.get("path") or [],
        "text": params.get("text"),
        "keys": params.get("keys") or [],
        "checked": params.get("checked"),
        "duration_ms": params.get("duration_ms"),
    }
    return {key: value for key, value in payload.items() if value is not None}


def _audit_record(
    action: str,
    success: bool,
    control: Optional[Dict[str, Any]],
    session_id: str,
    policy: UiControlPolicy,
    error_code: Optional[str],
    message: str,
) -> Dict[str, Any]:
    redacted = ["text"] if action in {UiActionKind.SET_TEXT, UiActionKind.TYPE} else []
    return UiControlAuditRecord(
        action_kind=action,
        success=success,
        target_control_id=control.get("id") if control else None,
        target_role=control.get("role") if control else None,
        before_focus_id=None,
        after_focus_id=None,
        error_code=error_code,
        message=message,
        session_id=session_id,
        redacted_fields=redacted,
        metadata={"backend": "windows-ui-control-host", "host_enforced": True},
    ).to_dict()


@_serialize_session_call
def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    action = str(params.get("action") or "")
    limit_error = _validate_action_limits(params)
    if limit_error is not None:
        return limit_error
    request = UiActionRequest(
        control_id=str(params.get("control_id") or "") or None,
        action=action,
        x=params.get("x"),
        y=params.get("y"),
    )
    if not policy.allows_request(request):
        return skill_error(f"ui_control action {action!r} disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        client, entry = _client_for(session_id, params, policy)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    if action == UiActionKind.GET_WINDOW_STATE:
        try:
            raw = client.window_state()
        except (UiControlHostError, OSError, ValueError) as exc:
            return _host_error(exc)
        message = "Read exact scoped window state from the isolated UI Control host."
        return skill_success(
            message,
            prompt=(
                "If minimized, call ui_control__act with restore_window; if hidden, use show_window; "
                "then activate_window and take a fresh snapshot."
            ),
            session_id=session_id,
            window_state=raw.get("state") or {},
            audit=_audit_record(action, True, None, session_id, policy, None, message),
        )
    if action in _WINDOW_STATE_OPERATIONS:
        try:
            raw = client.change_window_state(_WINDOW_STATE_OPERATIONS[action])
        except (UiControlHostError, OSError, ValueError) as exc:
            entry["snapshot_id"] = None
            return _host_error(exc)
        entry["snapshot_id"] = None
        message = f"Completed exact scoped window operation {action!r}."
        return skill_success(
            message,
            prompt="Take a fresh ui_control__snapshot before any content interaction.",
            session_id=session_id,
            window_state=raw.get("state") or {},
            audit=_audit_record(action, True, None, session_id, policy, None, message),
        )
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    current_snapshot_id = str(entry.get("snapshot_id") or "")
    if not requested_snapshot_id or requested_snapshot_id != current_snapshot_id:
        return skill_error(
            "The ui_control snapshot is stale; take a new snapshot before acting.",
            UiErrorCode.STALE_OBSERVATION,
            requested_snapshot_id=requested_snapshot_id,
            current_snapshot_id=current_snapshot_id,
        )
    control = None
    if not _is_native_action(action, params):
        # The host resolves and validates the real UIA node again. This local lookup
        # only improves the portable audit/result envelope.
        control_id = str(params.get("control_id") or "")
        if not control_id:
            return skill_error("control_id is required for semantic actions", UiErrorCode.INVALID_ACTION)
    try:
        raw = client.execute(_action_payload(params, _is_native_action(action, params)))
    except (UiControlHostError, OSError, ValueError) as exc:
        entry["snapshot_id"] = None
        return _host_error(exc)
    entry["snapshot_id"] = None
    success = bool(raw.get("success"))
    target_closed = bool(raw.get("target_closed"))
    if target_closed:
        with _CLIENTS_LOCK:
            current = _CLIENTS.get(session_id)
            if current is not None and current.get("client") is client:
                _CLIENTS.pop(session_id, None)
    error_code = str(raw.get("error")) if raw.get("error") else None
    message = str(raw.get("message") or "DCC UI Control action completed.")
    result = UiActionResult(
        success=success,
        control_id=str(params.get("control_id") or ""),
        error_code=error_code,
        message=message,
        metadata={
            "requires_new_screenshot": not target_closed,
            "policy_tier": raw.get("policy_tier"),
            "target_closed": target_closed,
        },
    ).to_dict()
    audit = _audit_record(action, success, control, session_id, policy, error_code, message)
    if target_closed:
        audit["metadata"]["target_closed"] = True
    if not success:
        return skill_error(message, error_code or UiErrorCode.BACKEND_ERROR, result=result, audit=audit)
    return skill_success(
        f"Completed isolated Windows UI Control action {action!r}.",
        prompt=(
            "The exact target window closed after the completed action. Explicitly bind the intended new PID/HWND "
            "before starting another UI Control session; no replacement window was followed."
            if target_closed
            else "Take a new ui_control__snapshot before the next action."
        ),
        session_id=session_id,
        session_active=not target_closed,
        target_closed=target_closed,
        result=result,
        audit=audit,
    )


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Stop one host-owned window session and invalidate every capability."""
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    with _CLIENTS_LOCK:
        entry = _CLIENTS.get(session_id)
    if entry is None:
        return skill_success(
            "No isolated UI Control session was active.",
            session_id=session_id,
            active=False,
            cleanup_pending=False,
        )
    try:
        stopped = entry["client"].stop()
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    cleanup_pending = bool(stopped.get("cleanup_pending"))
    if cleanup_pending:
        return skill_error(
            "UI Control stopped, but native overlay cleanup is still completing.",
            UiErrorCode.BACKEND_UNAVAILABLE,
            cleanup_pending=True,
        )
    with _CLIENTS_LOCK:
        if _CLIENTS.get(session_id) is entry:
            _CLIENTS.pop(session_id, None)
    return skill_success(
        "Stopped the isolated UI Control session.",
        session_id=session_id,
        active=False,
        cleanup_pending=False,
    )


@_serialize_session_call
def wait_for_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    condition_raw = params.get("condition") or {}
    if not isinstance(condition_raw, dict):
        return skill_error("condition must be an object", UiErrorCode.INVALID_ACTION)
    condition = _condition_from_params(condition_raw)
    timeout_ms = max(0, min(_MAX_WAIT_MS, int(condition.timeout_ms)))
    interval_ms = max(10, int(condition.interval_ms))
    deadline = time.monotonic() + timeout_ms / 1000.0
    last_snapshot_id = None
    while True:
        if _STOP_EVENT.is_set():
            return skill_error(
                "ui_control wait cancelled because the backend is stopping.", UiErrorCode.BACKEND_UNAVAILABLE
            )
        capture = _capture_accessibility_snapshot(session_id, policy, params)
        if not capture.get("success"):
            return capture
        last_snapshot_id = capture["snapshot_id"]
        if _condition_matches(capture["snapshot"], condition):
            return skill_success(
                "Windows UI Control wait condition satisfied.",
                session_id=session_id,
                snapshot_id=last_snapshot_id,
                condition=condition.to_dict(),
            )
        if time.monotonic() >= deadline:
            return skill_error(
                "Timed out waiting for the Windows UI Control condition.",
                UiErrorCode.TIMEOUT,
                session_id=session_id,
                snapshot_id=last_snapshot_id,
                condition=condition.to_dict(),
            )
        time.sleep(min(interval_ms / 1000.0, max(0.0, deadline - time.monotonic())))


@_serialize_session_call
def observe_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return root-level controls from the Windows UIA backend without subtree expansion."""
    _progressive = _load_sibling("_progressive_query")

    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    max_roots = max(1, min(100, int(params.get("max_roots") or 20)))

    if not policy.allow_snapshot and not policy.allow_find:
        return skill_error(
            "ui_control observation disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )

    capture = _capture_accessibility_snapshot(session_id, policy, params)
    if not capture.get("success"):
        return capture

    result = _progressive.observe_from_snapshot(capture["snapshot"], max_roots)

    return skill_success(
        f"Observed {len(result['roots'])} root-level control(s).",
        prompt="Use ui_control__expand to drill into a node, or ui_control__inspect for details.",
        session_id=session_id,
        snapshot_id=capture.get("snapshot_id"),
        roots=result["roots"],
        total_roots=result["total_roots"],
        truncated=result["truncated"],
    )


@_serialize_session_call
def expand_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return direct children of a specific control from the Windows UIA backend."""
    _progressive = _load_sibling("_progressive_query")

    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")
    max_children = max(1, min(200, int(params.get("max_children") or 50)))

    if not policy.allow_find:
        return skill_error(
            "ui_control find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not control_id:
        return skill_error(
            "control_id is required for expand",
            UiErrorCode.INVALID_ACTION,
            error_code=UiErrorCode.INVALID_ACTION,
        )

    capture = _capture_accessibility_snapshot(session_id, policy, params)
    if not capture.get("success"):
        return capture

    result = _progressive.expand_from_snapshot(capture["snapshot"], control_id, max_children)
    if result is None:
        return skill_error(
            f"control {control_id!r} not found; refresh observation",
            UiErrorCode.NOT_FOUND,
            error_code=UiErrorCode.NOT_FOUND,
            session_id=session_id,
        )

    return skill_success(
        f"Expanded {control_id!r}: {len(result['children'])} direct child(ren).",
        prompt="Use ui_control__expand again to drill deeper, or ui_control__inspect for details on a child.",
        session_id=session_id,
        snapshot_id=capture.get("snapshot_id"),
        control_id=result["control_id"],
        children=result["children"],
        total_children=result["total_children"],
        truncated=result["truncated"],
    )


@_serialize_session_call
def inspect_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return detailed properties of a specific control from the Windows UIA backend."""
    _progressive = _load_sibling("_progressive_query")

    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")

    if not policy.allow_find:
        return skill_error(
            "ui_control find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not control_id:
        return skill_error(
            "control_id is required for inspect",
            UiErrorCode.INVALID_ACTION,
            error_code=UiErrorCode.INVALID_ACTION,
        )

    capture = _capture_accessibility_snapshot(session_id, policy, params)
    if not capture.get("success"):
        return capture

    node = _progressive._find_by_id(capture["snapshot"], control_id)
    if not node:
        return skill_error(
            f"control {control_id!r} not found; refresh observation",
            UiErrorCode.NOT_FOUND,
            error_code=UiErrorCode.NOT_FOUND,
            session_id=session_id,
        )

    override = {
        "label_key": "name",
        "object_name_key": "automation_id",
        "tooltip_key": "help_text",
        "role_key": "control_type",
    }
    detail = _progressive.build_control_detail(
        node, capture["snapshot"], "windows-ui-control-host", override
    )

    return skill_success(
        f"Inspected control {control_id!r} ({detail['role']}).",
        prompt="Use ui_control__act with this control_id, or ui_control__expand to see its children.",
        session_id=session_id,
        snapshot_id=capture.get("snapshot_id"),
        control=detail,
    )


def request_stop() -> None:
    """Interrupt package waits and request immediate host-session stops."""
    _STOP_EVENT.set()
    with _CLIENTS_LOCK:
        entries = list(_CLIENTS.values())
    for entry in entries:
        with suppress(Exception):
            entry["client"].stop()


def cleanup() -> None:
    """Stop all host sessions during skill unload."""
    request_stop()
    with _CLIENTS_LOCK:
        _CLIENTS.clear()


def _dedent_for_tests() -> str:
    """Return the UIA script embedded by the native host for contract tests."""
    return Path(__file__).with_name("_windows_uia_backend.ps1").read_text(encoding="utf-8")
