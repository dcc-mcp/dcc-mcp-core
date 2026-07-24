"""Deterministic mock backend for the bundled ui-control skill."""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import sys
import tempfile
import threading
import time
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiBounds
from dcc_mcp_core.adapter_contracts import UiControlAuditRecord
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.adapter_contracts import UiWaitCondition
from dcc_mcp_core.adapter_contracts import UiWaitConditionKind
from dcc_mcp_core.adapter_contracts import UiWaitResult
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

_POLICY_KEYS = {
    "allow_snapshot",
    "allow_find",
    "allow_mutating_actions",
    "allow_text_entry",
    "allow_keyboard_shortcuts",
    "allow_raw_coordinates",
    "require_scoped_window",
    "allowed_window_titles",
    "allowed_process_ids",
    "audit_sensitive_values",
}
_CONDITION_KEYS = {
    "kind",
    "control_id",
    "query",
    "role",
    "label",
    "text",
    "value",
    "checked",
    "timeout_ms",
    "interval_ms",
}
_CLEANUP_REQUESTED = threading.Event()


def request_stop() -> None:
    """Wake in-flight waits before this private skill package is unloaded."""
    _CLEANUP_REQUESTED.set()


def cleanup() -> None:
    request_stop()


def _read_params() -> Dict[str, Any]:
    raw = ""
    try:
        if not sys.stdin.isatty():
            raw = sys.stdin.read()
    except Exception:
        raw = ""
    if raw.strip():
        try:
            parsed = json.loads(raw)
            return parsed if isinstance(parsed, dict) else {}
        except json.JSONDecodeError:
            return {}
    return {}


def _safe_session_id(session_id: Any) -> str:
    text = str(session_id or "default")
    return re.sub(r"[^A-Za-z0-9_.-]+", "_", text)[:80] or "default"


def _state_dir() -> Path:
    root = os.environ.get("DCC_MCP_UI_CONTROL_MOCK_STATE_DIR")
    path = Path(root) if root else Path(tempfile.gettempdir()) / "dcc-mcp-ui-control-mock"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _state_path(session_id: str) -> Path:
    return _state_dir() / f"{_safe_session_id(session_id)}.json"


def _default_state(session_id: str) -> Dict[str, Any]:
    return {
        "session_id": session_id,
        "revision": 1,
        "focus_id": "project-name",
        "project_name": "",
        "cache_enabled": False,
        "status": "Idle",
        "window_title": "DCC Mock Settings",
        "process_id": 0,
    }


def _load_state(session_id: str) -> Dict[str, Any]:
    path = _state_path(session_id)
    if not path.exists():
        state = _default_state(session_id)
        _save_state(state)
        return state
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        data = _default_state(session_id)
    default = _default_state(session_id)
    default.update(data if isinstance(data, dict) else {})
    return default


def _save_state(state: Dict[str, Any]) -> None:
    path = _state_path(str(state.get("session_id") or "default"))
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")
    tmp.replace(path)


def _snapshot_id(state: Dict[str, Any]) -> str:
    return f"{state['session_id']}:{state['revision']}"


def _policy_from_params(params: Dict[str, Any]) -> UiControlPolicy:
    raw = params.get("policy") or {}
    if not isinstance(raw, dict):
        raw = {}
    return UiControlPolicy().narrowed({k: raw[k] for k in _POLICY_KEYS if k in raw})


def _has_scoped_window(state: Dict[str, Any]) -> bool:
    title = str(state.get("window_title") or state.get("title") or "").strip()
    try:
        process_id = int(state.get("process_id") or state.get("pid") or 0)
    except Exception:
        process_id = 0
    return bool(title) or process_id > 0


def _window_allowed(state: Dict[str, Any], policy: UiControlPolicy) -> bool:
    if policy.scope_denied:
        return False
    if policy.require_scoped_window and not _has_scoped_window(state):
        return False
    if policy.allowed_window_titles:
        title = str(state.get("window_title") or "").lower()
        allowed = [str(item).lower() for item in policy.allowed_window_titles]
        if not any(item in title for item in allowed):
            return False
    if policy.allowed_process_ids:
        try:
            process_id = int(state.get("process_id") or 0)
        except Exception:
            process_id = 0
        if process_id not in policy.allowed_process_ids:
            return False
    return True


def _node(
    node_id: str,
    role: str,
    label: Optional[str] = None,
    text: Optional[str] = None,
    object_name: Optional[str] = None,
    value: Optional[str] = None,
    checked: Optional[bool] = None,
    bounds: Optional[UiBounds] = None,
    children: Optional[List[UiControlNode]] = None,
    snapshot_id: Optional[str] = None,
) -> UiControlNode:
    metadata: Dict[str, Any] = {"ui_control": {"backend": "mock"}}
    if snapshot_id:
        metadata["ui_control"]["snapshot_id"] = snapshot_id
    return UiControlNode(
        id=node_id,
        role=role,
        label=label,
        text=text,
        object_name=object_name,
        enabled=True,
        visible=True,
        bounds=bounds,
        value=value,
        checked=checked,
        children=children or [],
        metadata=metadata,
    )


def _snapshot_from_state(state: Dict[str, Any]) -> UiSnapshot:
    sid = _snapshot_id(state)
    children = [
        _node(
            "project-name",
            "text_field",
            label="Project name",
            text=str(state.get("project_name") or ""),
            object_name="projectNameEdit",
            value=str(state.get("project_name") or ""),
            bounds=UiBounds(x=24, y=48, width=240, height=28),
            snapshot_id=sid,
        ),
        _node(
            "enable-cache",
            "checkbox",
            label="Enable cache",
            object_name="enableCacheCheckBox",
            checked=bool(state.get("cache_enabled")),
            bounds=UiBounds(x=24, y=88, width=160, height=24),
            snapshot_id=sid,
        ),
        _node(
            "apply",
            "button",
            label="Apply",
            object_name="applyButton",
            bounds=UiBounds(x=204, y=132, width=72, height=28),
            snapshot_id=sid,
        ),
        _node(
            "status",
            "label",
            label="Status",
            text=str(state.get("status") or ""),
            object_name="statusLabel",
            bounds=UiBounds(x=24, y=132, width=160, height=28),
            snapshot_id=sid,
        ),
    ]
    root = _node(
        "mock-window",
        "window",
        label=str(state.get("window_title") or "DCC Mock Settings"),
        object_name="dccMockSettingsWindow",
        bounds=UiBounds(x=100, y=100, width=320, height=200),
        children=children,
        snapshot_id=sid,
    )
    return UiSnapshot(
        root=root,
        session_id=str(state["session_id"]),
        focus_id=str(state.get("focus_id") or ""),
        truncated=False,
        node_count=1 + len(children),
        metadata={
            "snapshot_id": sid,
            "ui_control": {
                "backend": "mock",
                "window_title": state.get("window_title"),
                "process_id": state.get("process_id"),
            },
        },
    )


def _iter_nodes(node: Dict[str, Any]) -> Iterable[Dict[str, Any]]:
    yield node
    for child in node.get("children", []) or []:
        if isinstance(child, dict):
            yield from _iter_nodes(child)


def _snapshot_dict(state: Dict[str, Any]) -> Dict[str, Any]:
    return _snapshot_from_state(state).to_dict()


def _find_controls(snapshot: Dict[str, Any], params: Dict[str, Any]) -> List[Dict[str, Any]]:
    query = str(params.get("query") or "").lower()
    role = str(params.get("role") or "").lower()
    label = str(params.get("label") or "").lower()
    object_name = str(params.get("object_name") or "").lower()
    limit = int(params.get("limit") or 10)
    matches = []
    for node in _iter_nodes(snapshot["root"]):
        if role and str(node.get("role") or "").lower() != role:
            continue
        if label and label not in str(node.get("label") or "").lower():
            continue
        if object_name and object_name not in str(node.get("object_name") or "").lower():
            continue
        if query:
            haystack = " ".join(
                str(node.get(key) or "") for key in ("id", "label", "text", "value", "object_name", "role")
            ).lower()
            if query not in haystack:
                continue
        matches.append(node)
        if len(matches) >= limit:
            break
    return matches


def _find_by_id(snapshot: Dict[str, Any], control_id: str) -> Optional[Dict[str, Any]]:
    for node in _iter_nodes(snapshot["root"]):
        if node.get("id") == control_id:
            return node
    return None


def _policy_denied_result(
    action: str,
    control_id: str,
    control: Optional[Dict[str, Any]],
    state: Dict[str, Any],
    policy: UiControlPolicy,
    message: str,
) -> Dict[str, Any]:
    result = UiActionResult(
        success=False,
        control_id=control_id,
        error_code=UiErrorCode.POLICY_DISABLED,
        message=message,
        before_focus_id=state.get("focus_id"),
        after_focus_id=state.get("focus_id"),
    ).to_dict()
    audit = _audit_record(action, False, control, state, policy, UiErrorCode.POLICY_DISABLED, message)
    return skill_error(message, UiErrorCode.POLICY_DISABLED, result=result, audit=audit)


def _audit_record(
    action: str,
    success: bool,
    control: Optional[Dict[str, Any]],
    state: Dict[str, Any],
    policy: UiControlPolicy,
    error_code: Optional[str] = None,
    message: Optional[str] = None,
    before_focus_id: Optional[str] = None,
    after_focus_id: Optional[str] = None,
    metadata: Optional[Dict[str, Any]] = None,
) -> Dict[str, Any]:
    redacted = []
    if action == UiActionKind.SET_TEXT and not policy.audit_sensitive_values:
        redacted.append("text")
    audit_metadata = {"backend": "mock", "snapshot_id": _snapshot_id(state)}
    if metadata:
        audit_metadata.update(metadata)
    return UiControlAuditRecord(
        action_kind=action,
        success=success,
        target_control_id=control.get("id") if control else None,
        target_role=control.get("role") if control else None,
        target_label=control.get("label") if control else None,
        before_focus_id=before_focus_id if before_focus_id is not None else state.get("focus_id"),
        after_focus_id=after_focus_id if after_focus_id is not None else state.get("focus_id"),
        error_code=error_code,
        message=message,
        session_id=state.get("session_id"),
        redacted_fields=redacted,
        metadata=audit_metadata,
    ).to_dict()


def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error(
            "ui_control snapshot disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not _window_allowed(state, policy):
        return skill_error(
            "scoped ui_control window is not allowed by policy",
            UiErrorCode.MISSING_WINDOW,
            error_code=UiErrorCode.MISSING_WINDOW,
        )
    snapshot = _snapshot_dict(state)
    return skill_success(
        "Captured mock ui_control snapshot.",
        prompt="Use ui_control__find to resolve a control, then ui_control__act with the returned snapshot_id.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        snapshot=snapshot,
        policy=policy.to_dict(),
    )


def record_clip_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    del params
    return skill_error(
        "Exact-window recording is available only through the Windows UI Control host.",
        UiErrorCode.UNSUPPORTED_ACTION,
        backend="mock",
    )


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Idempotent no-op for backends without native input ownership."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    return skill_success(
        "No native Computer Use session was active for the mock backend.",
        session_id=session_id,
        active=False,
        was_active=False,
        user_interrupted=False,
    )


def system_operation_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Reject Windows-only system configuration on the deterministic mock."""
    del params
    return skill_error(
        "Typed system operations are only available through the Windows UI Control host.",
        "unsupported",
        backend="mock",
    )


def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    if not policy.allow_find:
        return skill_error(
            "ui_control find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not _window_allowed(state, policy):
        return skill_error(
            "scoped ui_control window is not allowed by policy",
            UiErrorCode.MISSING_WINDOW,
            error_code=UiErrorCode.MISSING_WINDOW,
        )
    snapshot = _snapshot_dict(state)
    matches = _find_controls(snapshot, params)
    return skill_success(
        f"Found {len(matches)} ui_control control(s).",
        prompt="Use ui_control__act with a returned control id, then ui_control__wait_for.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def _stale_result(
    control_id: str,
    action: str,
    state: Dict[str, Any],
    policy: UiControlPolicy,
    requested: str,
) -> Dict[str, Any]:
    result = UiActionResult.stale(control_id).to_dict()
    metadata = {
        "requested_snapshot_id": requested,
        "current_snapshot_id": _snapshot_id(state),
    }
    result["metadata"] = metadata
    control = _find_by_id(_snapshot_dict(state), control_id)
    audit = _audit_record(
        action,
        False,
        control or {"id": control_id},
        state,
        policy,
        UiErrorCode.STALE_CONTROL,
        "control is stale; refresh the UI snapshot",
        metadata=metadata,
    )
    return skill_error(
        "Control is stale; refresh the ui_control snapshot.",
        UiErrorCode.STALE_CONTROL,
        result=result,
        audit=audit,
        current_snapshot_id=_snapshot_id(state),
    )


def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")
    action = str(params.get("action") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    if requested_snapshot_id and requested_snapshot_id != _snapshot_id(state):
        return _stale_result(control_id, action, state, policy, requested_snapshot_id)
    snapshot = _snapshot_dict(state)
    control = _find_by_id(snapshot, control_id)
    if not control:
        message = "control not found in scoped ui_control window"
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.NOT_FOUND,
            message=message,
            before_focus_id=state.get("focus_id"),
            after_focus_id=state.get("focus_id"),
        ).to_dict()
        audit = _audit_record(action, False, None, state, policy, UiErrorCode.NOT_FOUND, message)
        return skill_error(
            "Control not found in scoped ui_control window.",
            UiErrorCode.NOT_FOUND,
            result=result,
            audit=audit,
            current_snapshot_id=_snapshot_id(state),
        )
    if not _window_allowed(state, policy):
        return _policy_denied_result(
            action,
            control_id,
            control,
            state,
            policy,
            "scoped ui_control window is not allowed by policy",
        )
    if not policy.allows_action(action):
        return _policy_denied_result(
            action,
            control_id,
            control,
            state,
            policy,
            f"ui_control action {action!r} disabled by policy",
        )

    before_focus = state.get("focus_id")
    message = "ui_control action completed"
    if action == UiActionKind.FOCUS:
        state["focus_id"] = control_id
    elif action == UiActionKind.SET_TEXT:
        if control.get("role") != "text_field":
            return _unsupported_action(action, control, state, policy, "set_text requires a text_field control")
        state["project_name"] = str(params.get("text") or "")
        state["focus_id"] = control_id
    elif action in (UiActionKind.TOGGLE, UiActionKind.SET_CHECKED):
        if control.get("role") != "checkbox":
            return _unsupported_action(action, control, state, policy, f"{action} requires a checkbox control")
        if action == UiActionKind.SET_CHECKED:
            state["cache_enabled"] = bool(params.get("checked"))
        else:
            state["cache_enabled"] = not bool(state.get("cache_enabled"))
        state["focus_id"] = control_id
    elif action == UiActionKind.CLICK:
        state["focus_id"] = control_id
        if control_id == "apply":
            state["status"] = "Applied"
        elif control.get("role") == "checkbox":
            state["cache_enabled"] = not bool(state.get("cache_enabled"))
        elif control.get("role") not in ("button", "text_field"):
            return _unsupported_action(action, control, state, policy, "click is unsupported for this control role")
    else:
        return _unsupported_action(action, control, state, policy, "unsupported ui_control action")

    state["revision"] = int(state.get("revision") or 1) + 1
    _save_state(state)
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        before_focus_id=before_focus,
        after_focus_id=state.get("focus_id"),
        metadata={"snapshot_id": _snapshot_id(state)},
    ).to_dict()
    audit = _audit_record(action, True, control, state, policy, None, message, before_focus_id=before_focus)
    return skill_success(
        f"Completed ui_control action {action!r} on {control_id}.",
        prompt="Use ui_control__wait_for to poll for the expected UI state, then ui_control__snapshot to verify.",
        session_id=session_id,
        snapshot_id=_snapshot_id(state),
        result=result,
        audit=audit,
    )


def _unsupported_action(
    action: str,
    control: Dict[str, Any],
    state: Dict[str, Any],
    policy: UiControlPolicy,
    message: str,
) -> Dict[str, Any]:
    result = UiActionResult(
        success=False,
        control_id=str(control.get("id") or ""),
        error_code=UiErrorCode.UNSUPPORTED_ACTION,
        message=message,
        before_focus_id=state.get("focus_id"),
        after_focus_id=state.get("focus_id"),
    ).to_dict()
    audit = _audit_record(action, False, control, state, policy, UiErrorCode.UNSUPPORTED_ACTION, message)
    return skill_error(message, UiErrorCode.UNSUPPORTED_ACTION, result=result, audit=audit)


def _condition_from_params(raw: Dict[str, Any]) -> UiWaitCondition:
    data = {key: raw[key] for key in _CONDITION_KEYS if key in raw}
    data.setdefault("kind", UiWaitConditionKind.CONTROL_EXISTS)
    return UiWaitCondition(**data)


def _resolve_condition_control(snapshot: Dict[str, Any], condition: UiWaitCondition) -> Optional[Dict[str, Any]]:
    if condition.control_id:
        return _find_by_id(snapshot, condition.control_id)
    matches = _find_controls(snapshot, condition.to_dict())
    return matches[0] if matches else None


def _condition_matches(snapshot: Dict[str, Any], condition: UiWaitCondition) -> bool:
    control = _resolve_condition_control(snapshot, condition)
    if condition.kind == UiWaitConditionKind.CONTROL_MISSING:
        return control is None
    if control is None:
        return False
    if condition.kind == UiWaitConditionKind.CONTROL_EXISTS:
        return True
    if condition.kind == UiWaitConditionKind.TEXT_EQUALS:
        return str(control.get("text") or "") == str(condition.text or "")
    if condition.kind == UiWaitConditionKind.VALUE_EQUALS:
        return str(control.get("value") or "") == str(condition.value or "")
    if condition.kind == UiWaitConditionKind.CHECKED_EQUALS:
        return bool(control.get("checked")) is bool(condition.checked)
    if condition.kind == UiWaitConditionKind.ENABLED:
        return bool(control.get("enabled"))
    if condition.kind == UiWaitConditionKind.DISABLED:
        return not bool(control.get("enabled"))
    if condition.kind == UiWaitConditionKind.FOCUSED:
        return snapshot.get("focus_id") == control.get("id")
    return False


def wait_for_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    condition = _condition_from_params(params.get("condition") or {})
    timeout_ms = min(60_000, max(0, int(condition.timeout_ms)))
    condition.timeout_ms = timeout_ms
    interval_ms = max(10, int(condition.interval_ms))
    deadline = time.monotonic() + (timeout_ms / 1000.0)
    attempts = 0
    last_snapshot = None
    start = time.monotonic()
    while True:
        if _CLEANUP_REQUESTED.is_set():
            return skill_error(
                "ui_control wait cancelled because the backend is stopping.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        state = _load_state(session_id)
        if not _window_allowed(state, policy):
            elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
            message = "scoped ui_control window is not allowed by policy"
            result = UiWaitResult(
                success=False,
                condition=condition,
                elapsed_ms=elapsed_ms,
                attempts=attempts,
                snapshot=None,
                error_code=UiErrorCode.POLICY_DISABLED,
                message=message,
            ).to_dict()
            audit = _audit_record("wait_for", False, None, state, policy, UiErrorCode.POLICY_DISABLED, message)
            return skill_error(message, UiErrorCode.POLICY_DISABLED, session_id=session_id, result=result, audit=audit)
        last_snapshot = _snapshot_dict(state)
        attempts += 1
        if _condition_matches(last_snapshot, condition):
            elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
            control = _resolve_condition_control(last_snapshot, condition)
            result = UiWaitResult(
                success=True,
                condition=condition,
                elapsed_ms=elapsed_ms,
                attempts=attempts,
                snapshot=_snapshot_from_state(state),
                message="condition became true",
            ).to_dict()
            return skill_success(
                "ui_control wait condition satisfied.",
                session_id=session_id,
                snapshot_id=last_snapshot["metadata"]["snapshot_id"],
                result=result,
                audit=_audit_record(
                    "wait_for",
                    True,
                    control,
                    state,
                    policy,
                    None,
                    "condition became true",
                    metadata={"condition": condition.to_dict()},
                ),
            )
        if time.monotonic() >= deadline:
            break
        _CLEANUP_REQUESTED.wait(min(interval_ms / 1000.0, max(0.0, deadline - time.monotonic())))

    elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
    result = UiWaitResult(
        success=False,
        condition=condition,
        elapsed_ms=elapsed_ms,
        attempts=attempts,
        snapshot=None,
        error_code=UiErrorCode.TIMEOUT,
        message="condition did not become true before timeout",
        metadata={"last_snapshot": last_snapshot},
    ).to_dict()
    control = _resolve_condition_control(last_snapshot, condition) if last_snapshot else None
    audit = _audit_record(
        "wait_for",
        False,
        control,
        state,
        policy,
        UiErrorCode.TIMEOUT,
        "condition did not become true before timeout",
        metadata={"condition": condition.to_dict()},
    )
    return skill_error(
        "ui_control wait_for timed out.",
        UiErrorCode.TIMEOUT,
        session_id=session_id,
        result=result,
        audit=audit,
        attempts=attempts,
    )


def _build_detailed_node(
    snapshot: Dict[str, Any],
    node: Dict[str, Any],
    tree_path: str,
) -> Dict[str, Any]:
    """Build a UiControlDetail dict from a snapshot node."""
    role = str(node.get("role") or "unknown")
    is_text_field = role == "text_field"
    is_checkbox = role == "checkbox"
    is_button = role == "button"

    patterns = []
    actions = []
    if is_text_field:
        patterns = ["ValuePattern", "TextPattern"]
        actions = ["set_text", "focus"]
    elif is_checkbox:
        patterns = ["TogglePattern"]
        actions = ["toggle", "set_checked", "click"]
    elif is_button:
        patterns = ["InvokePattern"]
        actions = ["click"]
    else:
        patterns = ["SelectionItemPattern"] if role in ("list_item", "menu_item", "combo_box") else []
        actions = ["click", "focus"]

    return {
        "id": node.get("id", ""),
        "role": role,
        "enabled": bool(node.get("enabled", True)),
        "visible": bool(node.get("visible", True)),
        "focused": snapshot.get("focus_id") == node.get("id"),
        "label": node.get("label"),
        "text": node.get("text"),
        "object_name": node.get("object_name"),
        "tooltip": node.get("tooltip"),
        "bounds": node.get("bounds"),
        "value": node.get("value"),
        "checked": node.get("checked"),
        "child_count": len(node.get("children", []) or []),
        "supported_patterns": patterns,
        "supported_actions": actions,
        "is_keyboard_focusable": role in ("text_field", "button", "checkbox", "combo_box"),
        "is_password": role == "password",
        "tree_path": tree_path,
        "metadata": {"backend": "mock"},
    }


def observe_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return root-level controls without expanding children."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    max_roots = max(1, min(100, int(params.get("max_roots") or 20)))

    if not policy.allow_snapshot:
        return skill_error(
            "ui_control observation disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not _window_allowed(state, policy):
        return skill_error(
            "scoped ui_control window is not allowed by policy",
            UiErrorCode.MISSING_WINDOW,
            error_code=UiErrorCode.MISSING_WINDOW,
        )

    snapshot = _snapshot_dict(state)
    # Return root-level children without their children expanded
    all_roots = list(snapshot["root"].get("children", []) or [])
    total_roots = len(all_roots)
    truncated = total_roots > max_roots

    roots = []
    for i, child in enumerate(all_roots[:max_roots]):
        # Strip children to return only root-level metadata
        stripped = dict(child)
        child_count = len(stripped.get("children", []) or [])
        stripped["children"] = []
        stripped["child_count"] = child_count
        roots.append(stripped)

    return skill_success(
        f"Observed {len(roots)} root-level control(s).",
        prompt="Use ui_control__expand to drill into a node, or ui_control__inspect for details.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        roots=roots,
        total_roots=total_roots,
        truncated=truncated,
    )


def expand_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return direct children of a specific control node."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")
    max_children = max(1, min(200, int(params.get("max_children") or 50)))

    if not policy.allow_find:
        return skill_error(
            "ui_control find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not _window_allowed(state, policy):
        return skill_error(
            "scoped ui_control window is not allowed by policy",
            UiErrorCode.MISSING_WINDOW,
            error_code=UiErrorCode.MISSING_WINDOW,
        )
    if not control_id:
        return skill_error(
            "control_id is required for expand",
            UiErrorCode.INVALID_ACTION,
            error_code=UiErrorCode.INVALID_ACTION,
        )

    snapshot = _snapshot_dict(state)
    parent = _find_by_id(snapshot, control_id)
    if not parent:
        return skill_error(
            f"control {control_id!r} not found; refresh observation",
            UiErrorCode.NOT_FOUND,
            error_code=UiErrorCode.NOT_FOUND,
            session_id=session_id,
        )

    all_children = list(parent.get("children", []) or [])
    total_children = len(all_children)
    truncated = total_children > max_children

    children = []
    for i, child in enumerate(all_children[:max_children]):
        stripped = dict(child)
        grandchild_count = len(stripped.get("children", []) or [])
        stripped["children"] = []
        stripped["child_count"] = grandchild_count
        children.append(stripped)

    return skill_success(
        f"Expanded {control_id!r}: {len(children)} direct child(ren).",
        prompt="Use ui_control__expand again to drill deeper, or ui_control__inspect for details on a child.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        control_id=control_id,
        children=children,
        total_children=total_children,
        truncated=truncated,
    )


def inspect_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Return detailed properties of a specific control."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")

    if not policy.allow_find:
        return skill_error(
            "ui_control find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    if not _window_allowed(state, policy):
        return skill_error(
            "scoped ui_control window is not allowed by policy",
            UiErrorCode.MISSING_WINDOW,
            error_code=UiErrorCode.MISSING_WINDOW,
        )
    if not control_id:
        return skill_error(
            "control_id is required for inspect",
            UiErrorCode.INVALID_ACTION,
            error_code=UiErrorCode.INVALID_ACTION,
        )

    snapshot = _snapshot_dict(state)
    node = _find_by_id(snapshot, control_id)
    if not node:
        return skill_error(
            f"control {control_id!r} not found; refresh observation",
            UiErrorCode.NOT_FOUND,
            error_code=UiErrorCode.NOT_FOUND,
            session_id=session_id,
        )

    tree_path = _compute_tree_path(snapshot["root"], control_id)
    detail = _build_detailed_node(snapshot, node, tree_path)

    return skill_success(
        f"Inspected control {control_id!r} ({detail['role']}).",
        prompt="Use ui_control__act with this control_id, or ui_control__expand to see its children.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        control=detail,
    )


def _compute_tree_path(root: Dict[str, Any], target_id: str) -> str:
    """Compute a dot-separated index path from root to target_id."""
    queue = [(root, "")]
    while queue:
        node, prefix = queue.pop(0)
        children = node.get("children", []) or []
        for i, child in enumerate(children):
            path = f"{prefix}{i}" if prefix else str(i)
            if child.get("id") == target_id:
                return path
            queue.append((child, path + "."))
    return ""


def emit(result: Dict[str, Any]) -> None:
    print(json.dumps(result, sort_keys=True))
