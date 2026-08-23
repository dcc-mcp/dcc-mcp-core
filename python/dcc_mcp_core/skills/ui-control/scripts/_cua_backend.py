"""Agent-friendly ui-control policy wrapper for the standalone CUA Host."""

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


_SUPPORT = _load_sibling("_cua_support")
_HOST = _load_sibling("_cua_cli_host_client")
UiControlHostError = _HOST.UiControlHostError
_HostClient = _HOST.UiControlHostClient

_policy_from_params = _SUPPORT._policy_from_params
_scope_from_params = _SUPPORT._scope_from_params
_scope_is_trusted_native_target = _SUPPORT._scope_is_trusted_native_target
_node_from_cua_dict = _SUPPORT._node_from_cua_dict
_find_by_id = _SUPPORT._find_by_id
_find_controls = _SUPPORT._find_controls
_validate_action_limits = _SUPPORT._validate_action_limits
_is_native_action = _SUPPORT._is_native_action
_condition_from_params = _SUPPORT._condition_from_params
_condition_matches = _SUPPORT._condition_matches
_safe_session_id = _SUPPORT._safe_session_id
_session_lock = _SUPPORT._session_lock
_raw_input_enabled = _SUPPORT._raw_input_enabled

_CLIENTS: Dict[str, Dict[str, Any]] = {}
_STOP_EVENT = threading.Event()
_CLIENTS_LOCK = threading.RLock()
_ACTIVE_CALLS: set[str] = set()
_IDLE_REAPER: Optional[threading.Thread] = None
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
                        backend="dcc-cua",
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


def _ensure_idle_reaper() -> None:
    global _IDLE_REAPER
    with _CLIENTS_LOCK:
        if _IDLE_REAPER is not None and _IDLE_REAPER.is_alive():
            return
        _IDLE_REAPER = threading.Thread(
            target=_reap_idle_clients,
            name="dcc-mcp-ui-control-idle-reaper",
            daemon=True,
        )
        _IDLE_REAPER.start()


def _reap_idle_clients() -> None:
    global _IDLE_REAPER
    while not _STOP_EVENT.wait(min(_IDLE_LEASE_SECONDS, 1.0)):
        _prune_idle_clients(time.monotonic())
        with _CLIENTS_LOCK:
            if not _CLIENTS:
                _IDLE_REAPER = None
                return


def _scope_error(scope: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    if scope.get("invalid_reason"):
        return skill_error(str(scope["invalid_reason"]), UiErrorCode.INVALID_TARGET)
    if not _scope_is_trusted_native_target(scope):
        return skill_error(
            (
                "Isolated DCC UI Control requires an operator-bound process id or window handle. "
                "Set DCC_MCP_UI_CONTROL_PROCESS_ID or DCC_MCP_UI_CONTROL_WINDOW_HANDLE "
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
    window_titles = scope.get("window_titles") or []
    process_id = int(process_ids[0]) if len(process_ids) == 1 else None
    window_handle = int(window_handles[0]) if len(window_handles) == 1 else None
    window_title = str(window_titles[0]) if len(window_titles) == 1 else None
    allow_raw_input = _raw_input_enabled()
    dcc_type = str(
        os.environ.get("DCC_MCP_UI_CONTROL_DCC_TYPE")
        or scope.get("dcc_type")
        or os.environ.get("DCC_MCP_DCC_TYPE")
        or "custom"
    )
    task_grant_id = f"adapter:{dcc_type}:{session_id}:{process_id or 0}:{window_handle or 0}"
    return {
        "session_id": session_id,
        "task_grant_id": task_grant_id,
        "dcc_type": dcc_type,
        "process_id": process_id,
        "window_handle": window_handle,
        "window_title": window_title,
        "allow_raw_input": allow_raw_input,
        "allow_menu_invoke": policy.allow_mutating_actions,
        "scope": scope,
    }


def _client_for(session_id: str, params: Dict[str, Any], policy: UiControlPolicy) -> Tuple[Any, Dict[str, Any]]:
    spec = _client_spec(session_id, params, policy)
    identity = tuple(
        spec[key]
        for key in (
            "dcc_type",
            "process_id",
            "window_handle",
            "window_title",
            "allow_raw_input",
            "allow_menu_invoke",
        )
    )
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
                window_title=spec["window_title"],
                allow_raw_input=spec["allow_raw_input"],
                allow_menu_invoke=spec["allow_menu_invoke"],
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
            _ensure_idle_reaper()
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
        backend="dcc-cua",
        **recovery,
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
        max_depth = max(1, min(12, int(os.environ.get("DCC_MCP_CUA_MAX_DEPTH", "5"))))
        max_nodes = max(1, min(2_000, int(os.environ.get("DCC_MCP_CUA_MAX_NODES", "250"))))
        raw = client.snapshot(max_depth=max_depth, max_nodes=max_nodes)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)

    snapshot_id = str(raw["accessibility_state_id"])
    accessibility_available = int(raw.get("node_count") or 0) > 0
    state_delta = _state_delta_event(raw, snapshot_id)
    root = _node_from_cua_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"cua:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= max_nodes,
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "ui_control": {
                "backend": "dcc-cua",
                "scope": entry["scope"],
                "target": raw.get("target") or client.target,
                "max_depth": max_depth,
                "max_nodes": max_nodes,
                "accessibility_available": accessibility_available,
                "accessibility_error": (None if accessibility_available else "backend_unavailable"),
            },
            "computer_use": raw.get("observation") or {},
            "state_delta": state_delta,
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
        "state_delta": state_delta,
        "accessibility_available": accessibility_available,
    }


def _capture_accessibility_snapshot(
    session_id: str,
    policy: UiControlPolicy,
    params: Dict[str, Any],
) -> Dict[str, Any]:
    try:
        client, entry = _client_for(session_id, params, policy)
        max_depth = max(1, min(12, int(os.environ.get("DCC_MCP_CUA_MAX_DEPTH", "5"))))
        max_nodes = max(1, min(2_000, int(os.environ.get("DCC_MCP_CUA_MAX_NODES", "250"))))
        raw = client.accessibility_snapshot(max_depth=max_depth, max_nodes=max_nodes)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    snapshot_id = str(raw["accessibility_state_id"])
    state_delta = _state_delta_event(raw, snapshot_id)
    root = _node_from_cua_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"cua:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= max_nodes,
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "ui_control": {
                "backend": "dcc-cua",
                "scope": entry["scope"],
                "target": raw.get("target") or client.target,
                "max_depth": max_depth,
                "max_nodes": max_nodes,
                "pixels_captured": False,
            },
            "state_delta": state_delta,
        },
    ).to_dict()
    entry["snapshot_id"] = snapshot_id
    entry["snapshot"] = snapshot
    return {
        "success": True,
        "snapshot_id": snapshot_id,
        "snapshot": snapshot,
        "state_delta": state_delta,
    }


def _state_delta_event(raw: Dict[str, Any], state_id: str) -> Optional[Dict[str, Any]]:
    delta = raw.get("state_delta")
    if not isinstance(delta, dict):
        return None
    event = {"source": "cua-accessibility", "state_id": state_id, "delta": delta}
    if raw.get("cause_action_id"):
        event["cause_action_id"] = str(raw["cause_action_id"])
    return event


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
    accessibility_available = bool(capture.get("accessibility_available", True))
    return skill_success(
        (
            "Captured scoped CUA application snapshot."
            if accessibility_available
            else "Captured screenshot-only CUA application observation."
        ),
        prompt=(
            "Use ui_control__find or perform one scoped ui_control__act with this snapshot_id, then snapshot again."
            if accessibility_available
            else (
                "CUA accessibility was unavailable for this frame. Inspect the pixels, but do not act "
                "until a fresh snapshot returns accessibility_available=true."
            )
        ),
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        observation=capture["observation"],
        state_delta=capture.get("state_delta"),
        accessibility_available=accessibility_available,
        policy=policy.to_dict(),
        __rich__={
            "kind": "image",
            "data": base64.b64encode(capture["image"]).decode("ascii"),
            "mime": capture["mime_type"],
            "alt": "{} UI Control screenshot".format(params.get("app_name") or "DCC"),
        },
    )


@_serialize_session_call
def recording_start_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Start CUA trajectory recording for the exact target session."""
    params = dict(params or {})
    allowed = {
        "session_id",
        "process_id",
        "window_handle",
        "window_title",
        "process_name",
        "output_dir",
        "record_video",
        "policy",
    }
    if set(params) - allowed:
        return skill_error(
            "The trajectory recording request contains unsupported fields.",
            "invalid_request",
            backend="dcc-cua",
        )
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control recording disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        output_dir = str(params.get("output_dir") or "").strip()
        output_path = Path(output_dir).expanduser()
        if not output_dir or not output_path.is_absolute():
            raise UiControlHostError("invalid_request", "output_dir must be an absolute path.")
        output_dir = str(output_path.resolve())
        record_video = params.get("record_video", False)
        if type(record_video) is not bool:
            raise UiControlHostError("invalid_request", "record_video must be a boolean.")
        client, entry = _client_for(session_id, params, policy)
        recording = client.recording_start(output_dir=output_dir, record_video=record_video)
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    finally:
        entry = _CLIENTS.get(session_id)
        if entry is not None:
            entry["snapshot_id"] = None
    return skill_success(
        "Started CUA trajectory recording for the exact target.",
        prompt=(
            "Perform scoped ui_control actions, inspect ui_control__recording_state when needed, "
            "then call ui_control__recording_stop to finalize CUA-owned artifacts."
        ),
        session_id=session_id,
        target=client.target,
        output_dir=output_dir,
        recording=recording,
        policy=policy.to_dict(),
    )


@_serialize_session_call
def recording_stop_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Finalize CUA trajectory recording for the exact target session."""
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control recording disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        client, entry = _client_for(session_id, params, policy)
        recording = client.recording_stop()
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    finally:
        entry = _CLIENTS.get(session_id)
        if entry is not None:
            entry["snapshot_id"] = None
    return skill_success(
        "Finalized CUA trajectory recording.",
        prompt="Preserve the finalized CUA output directory and structured recording state as evidence.",
        session_id=session_id,
        target=client.target,
        recording=recording,
        policy=policy.to_dict(),
    )


@_serialize_session_call
def recording_state_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Read CUA trajectory recording state for the exact target session."""
    params = dict(params or {})
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control recording disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        client, _entry = _client_for(session_id, params, policy)
        recording = client.recording_state()
    except (UiControlHostError, OSError, ValueError) as exc:
        return _host_error(exc)
    return skill_success(
        "Read CUA trajectory recording state.",
        session_id=session_id,
        target=client.target,
        recording=recording,
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
        f"Found {len(matches)} scoped CUA control(s).",
        prompt="Use ui_control__act with a returned control id and snapshot_id.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def _intent(params: Dict[str, Any]) -> str:
    requested = str(params.get("intent") or "ordinary_edit").strip().lower()
    return requested if requested in _INTENTS else "ordinary_edit"


def _action_payload(
    params: Dict[str, Any],
    native: bool,
    control: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    payload = {
        "action": "click" if action == UiActionKind.RAW_COORDINATE_CLICK else action,
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
    metadata = (control or {}).get("metadata") or {}
    locator = metadata.get("ui_control") if isinstance(metadata, dict) else None
    if not native and isinstance(locator, dict):
        if locator.get("element_token"):
            payload["element_token"] = locator["element_token"]
        elif type(locator.get("element_index")) is int:
            payload["element_index"] = locator["element_index"]
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
        metadata={"backend": "dcc-cua", "host_enforced": True},
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
        menu_path=list(params.get("menu_path") or []),
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
        message = "Read exact scoped application state from the CUA Host."
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
    if action == UiActionKind.INVOKE_MENU:
        menu_path = list(params.get("menu_path") or [])
        try:
            raw = client.invoke_menu(menu_path)
        except (UiControlHostError, OSError, ValueError) as exc:
            entry["snapshot_id"] = None
            return _host_error(exc)
        entry["snapshot_id"] = None
        effect = str(raw.get("effect") or "unverifiable")
        verification_required = bool(raw.get("verification_required", effect != "confirmed"))
        observation_required = bool(raw.get("observation_required", True))
        success = bool(raw.get("success"))
        if not success:
            return skill_error(
                "dcc-cua did not invoke the exact native menu path.",
                UiErrorCode.INPUT_FAILED,
                session_id=session_id,
                menu_path=menu_path,
                effect=effect,
                verification_required=True,
                observation_required=observation_required,
            )
        message = (
            "Invoked and confirmed the exact native menu path."
            if not verification_required
            else "Invoked the exact native menu path; delivery requires verification."
        )
        return skill_success(
            message,
            prompt=(
                "Take a fresh ui_control__snapshot and verify the requested menu, popup, or application "
                "state before the next mutation. Native delivery alone is not completion evidence."
            ),
            session_id=session_id,
            menu_path=menu_path,
            effect=effect,
            verification_required=verification_required,
            observation_required=observation_required,
            target=raw.get("target") or client.target,
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
    native = _is_native_action(action, params)
    control_id = str(params.get("control_id") or "")
    control = _find_by_id(entry["snapshot"], control_id) if control_id and entry.get("snapshot") else None
    if not native and control is None:
        return skill_error("control_id is required for semantic actions", UiErrorCode.INVALID_ACTION)
    try:
        raw = client.execute(_action_payload(params, native, control))
    except (UiControlHostError, OSError, ValueError) as exc:
        entry["snapshot_id"] = None
        return _host_error(exc)
    entry["snapshot_id"] = None
    success = bool(raw.get("success"))
    action_id = str(raw.get("action_id") or "") or None
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
            "action_id": action_id,
        },
    ).to_dict()
    audit = _audit_record(action, success, control, session_id, policy, error_code, message)
    if target_closed:
        audit["metadata"]["target_closed"] = True
    if not success:
        return skill_error(
            message,
            error_code or UiErrorCode.BACKEND_ERROR,
            action_id=action_id,
            result=result,
            audit=audit,
        )
    return skill_success(
        f"Completed scoped CUA action {action!r}.",
        prompt=(
            "The exact target window closed after the completed action. Explicitly bind the intended new PID/HWND "
            "before starting another UI Control session; no replacement window was followed."
            if target_closed
            else "Take a new ui_control__snapshot before the next action."
        ),
        session_id=session_id,
        session_active=not target_closed,
        target_closed=target_closed,
        action_id=action_id,
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
                "CUA wait condition satisfied.",
                session_id=session_id,
                snapshot_id=last_snapshot_id,
                condition=condition.to_dict(),
            )
        if time.monotonic() >= deadline:
            return skill_error(
                "Timed out waiting for the CUA condition.",
                UiErrorCode.TIMEOUT,
                session_id=session_id,
                snapshot_id=last_snapshot_id,
                condition=condition.to_dict(),
            )
        time.sleep(min(interval_ms / 1000.0, max(0.0, deadline - time.monotonic())))


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
