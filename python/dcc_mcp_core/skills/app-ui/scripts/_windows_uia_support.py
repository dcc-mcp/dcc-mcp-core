"""State and pure helpers for the Windows UI Automation backend."""

from __future__ import annotations

from contextlib import suppress
import json
import os
from pathlib import Path
import tempfile
import threading
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Set

from dcc_mcp_core.adapter_contracts import AppUiAuditRecord
from dcc_mcp_core.adapter_contracts import AppUiPolicy
from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiBounds
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.adapter_contracts import UiWaitCondition
from dcc_mcp_core.adapter_contracts import UiWaitConditionKind
from dcc_mcp_core.skill import skill_error


def _key_set(value: str) -> frozenset:
    return frozenset(value.split())


_POLICY_KEYS = _key_set(
    "allow_snapshot allow_find allow_mutating_actions allow_text_entry allow_keyboard_shortcuts "
    "allow_raw_coordinates allowed_window_titles allowed_process_ids audit_sensitive_values"
)
_CONDITION_KEYS = _key_set("kind control_id query role label text value checked timeout_ms interval_ms")
_COMPUTER_USE_SESSIONS: Dict[str, Dict[str, Any]] = {}
_COMPUTER_USE_OBSERVATIONS: Dict[str, Dict[str, str]] = {}
_COMPUTER_USE_INTERRUPTED: Set[str] = set()
_COMPUTER_USE_STOPPING: Dict[str, int] = {}
_SESSION_STOP_GENERATIONS: Dict[str, int] = {}
_SESSION_LOCKS: Dict[str, threading.RLock] = {}
_SESSION_STOP_LOCKS: Dict[str, threading.Lock] = {}
_SESSION_LOCKS_GUARD = threading.Lock()
_CLEANUP_REQUESTED = threading.Event()
_MAX_DRAG_POINTS = 256
_MAX_KEY_TOKENS = 16
_MAX_TEXT_UTF16_UNITS = 4096
_DENIED_PROCESS_NAMES = frozenset(
    {
        "1password",
        "authhost",
        "bitwarden",
        "cmd",
        "conhost",
        "consent",
        "credentialuibroker",
        "dashlane",
        "enpass",
        "keeperpasswordmanager",
        "keepass",
        "keepassxc",
        "lastpass",
        "lockapp",
        "logonui",
        "nordpass",
        "openconsole",
        "powershell",
        "powershell_ise",
        "pwsh",
        "roboform",
        "sechealthui",
        "securityhealthhost",
        "systemsettings",
        "windowsterminal",
        "wt",
    }
)
_DESKTOP_UNAVAILABLE_MESSAGE = (
    "The Windows desktop is locked or disconnected. Unlock it before using app_ui; no UI input was attempted."
)


def _safe_session_id(session_id: Any) -> str:
    text = str(session_id or "default")
    cleaned = "".join(ch if ch.isalnum() or ch in "_.-" else "_" for ch in text)
    return cleaned[:80] or "default"


def _session_lock(session_id: str) -> threading.RLock:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        lock = _SESSION_LOCKS.get(safe_id)
        if lock is None:
            lock = threading.RLock()
            _SESSION_LOCKS[safe_id] = lock
        return lock


def _session_stop_lock(session_id: str) -> threading.Lock:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        lock = _SESSION_STOP_LOCKS.get(safe_id)
        if lock is None:
            lock = threading.Lock()
            _SESSION_STOP_LOCKS[safe_id] = lock
        return lock


def _mark_session_stopping(session_id: str, delta: int) -> None:
    with _SESSION_LOCKS_GUARD:
        count = _COMPUTER_USE_STOPPING.get(session_id, 0) + delta
        if count > 0:
            _COMPUTER_USE_STOPPING[session_id] = count
        else:
            _COMPUTER_USE_STOPPING.pop(session_id, None)


def _session_stop_generation(session_id: str) -> int:
    with _SESSION_LOCKS_GUARD:
        return _SESSION_STOP_GENERATIONS.get(_safe_session_id(session_id), 0)


def _bump_session_stop_generation(session_id: str) -> None:
    safe_id = _safe_session_id(session_id)
    with _SESSION_LOCKS_GUARD:
        _SESSION_STOP_GENERATIONS[safe_id] = _SESSION_STOP_GENERATIONS.get(safe_id, 0) + 1


def _desktop_unavailable_result(session_id: str) -> Dict[str, Any]:
    _COMPUTER_USE_OBSERVATIONS.pop(_safe_session_id(session_id), None)
    return skill_error(
        _DESKTOP_UNAVAILABLE_MESSAGE,
        UiErrorCode.DESKTOP_UNAVAILABLE,
        error_code=UiErrorCode.DESKTOP_UNAVAILABLE,
        backend="windows-uia",
    )


def _state_dir() -> Path:
    root = os.environ.get("DCC_MCP_APP_UI_UIA_STATE_DIR")
    path = Path(root) if root else Path(tempfile.gettempdir()) / "dcc-mcp-app-ui-uia" / f"process-{os.getpid()}"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _state_path(session_id: str) -> Path:
    return _state_dir() / f"{_safe_session_id(session_id)}.json"


def _load_state(session_id: str) -> Dict[str, Any]:
    path = _state_path(session_id)
    if not path.exists():
        return {"session_id": session_id, "revision": 0, "last_snapshot_id": ""}
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        data = {}
    state = {"session_id": session_id, "revision": 0, "last_snapshot_id": ""}
    if isinstance(data, dict):
        state.update(data)
    return state


def _save_state(state: Dict[str, Any]) -> None:
    path = _state_path(str(state.get("session_id") or "default"))
    tmp = path.with_suffix(".tmp")
    tmp.write_text(json.dumps(state, sort_keys=True), encoding="utf-8")
    tmp.replace(path)


def _snapshot_id(state: Dict[str, Any]) -> str:
    return f"{state['session_id']}:{state['revision']}"


def _policy_from_params(params: Dict[str, Any]) -> AppUiPolicy:
    raw = params.get("policy") or {}
    if not isinstance(raw, dict):
        raw = {}
    ceiling = _env_flag("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT")
    return AppUiPolicy(
        allow_raw_coordinates=ceiling,
        allow_keyboard_shortcuts=ceiling,
    ).narrowed({key: raw[key] for key in _POLICY_KEYS if key in raw})


def _env_flag(name: str) -> bool:
    return str(os.environ.get(name) or "").strip().lower() in {"1", "true", "yes", "on"}


def _positive_int(value: Any) -> Optional[int]:
    if value is None or value == "":
        return None
    try:
        parsed = int(value)
    except (TypeError, ValueError):
        return None
    return parsed if parsed > 0 else None


def _intersect_title_constraints(left: str, right: str) -> Optional[str]:
    if not left:
        return right or None
    if not right:
        return left
    left_folded = left.casefold()
    right_folded = right.casefold()
    if left_folded in right_folded:
        return right
    if right_folded in left_folded:
        return left
    return None


def _process_name_key(value: str) -> str:
    return Path(value.strip()).stem.casefold()


def _scope_from_params(params: Dict[str, Any], policy: AppUiPolicy) -> Dict[str, Any]:
    invalid_reason = None
    trusted_title = str(os.environ.get("DCC_MCP_APP_UI_UIA_WINDOW_TITLE") or "").strip()
    requested_title = str(params.get("window_title") or "").strip()
    effective_title = _intersect_title_constraints(trusted_title, requested_title)
    if trusted_title and requested_title and effective_title is None:
        invalid_reason = "the requested window title does not intersect the runtime DCC title scope"
    allowed_titles = [str(item).strip() for item in policy.allowed_window_titles if str(item).strip()]
    if policy.scope_denied:
        invalid_reason = "the requested policy scope does not intersect the runtime allowlist"
    elif allowed_titles and effective_title:
        compatible = []
        for allowed in allowed_titles:
            narrowed = _intersect_title_constraints(effective_title, allowed)
            if narrowed is not None:
                compatible.append(narrowed)
        if not compatible:
            invalid_reason = "the requested window title is outside the policy allowlist"
        else:
            effective_title = max(compatible, key=len)
    titles = [effective_title] if effective_title else allowed_titles

    allowed_process_ids = {int(item) for item in policy.allowed_process_ids if int(item) > 0}
    raw_trusted_pid = os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_ID")
    trusted_process_id = _positive_int(raw_trusted_pid)
    if raw_trusted_pid and trusted_process_id is None:
        invalid_reason = invalid_reason or "the runtime DCC process id scope is invalid"
    raw_requested_pid = params.get("process_id")
    requested_process_id = _positive_int(raw_requested_pid)
    if raw_requested_pid is not None and requested_process_id is None:
        invalid_reason = invalid_reason or "the requested process id is invalid"
    if trusted_process_id and requested_process_id and trusted_process_id != requested_process_id:
        invalid_reason = invalid_reason or "the requested process id is outside the runtime DCC scope"
    effective_process_id = requested_process_id or trusted_process_id
    if effective_process_id and allowed_process_ids and effective_process_id not in allowed_process_ids:
        invalid_reason = invalid_reason or "the requested process id is outside the policy allowlist"
    process_ids = [effective_process_id] if effective_process_id else sorted(allowed_process_ids)

    trusted_process_name = str(os.environ.get("DCC_MCP_APP_UI_UIA_PROCESS_NAME") or "").strip()
    requested_process_name = str(params.get("process_name") or "").strip()
    if (
        trusted_process_name
        and requested_process_name
        and _process_name_key(trusted_process_name) != _process_name_key(requested_process_name)
    ):
        invalid_reason = invalid_reason or "the requested process name is outside the runtime DCC scope"
    effective_process_name = requested_process_name or trusted_process_name
    if effective_process_name and _process_name_key(effective_process_name) in _DENIED_PROCESS_NAMES:
        invalid_reason = invalid_reason or (
            "system, terminal, authentication, and password-manager processes are not allowed app_ui targets"
        )
    process_names = [effective_process_name] if effective_process_name else []

    raw_trusted_handle = os.environ.get("DCC_MCP_APP_UI_UIA_WINDOW_HANDLE")
    trusted_window_handle = _positive_int(raw_trusted_handle)
    if raw_trusted_handle and trusted_window_handle is None:
        invalid_reason = invalid_reason or "the runtime DCC window handle scope is invalid"
    raw_requested_handle = params.get("window_handle")
    requested_window_handle = _positive_int(raw_requested_handle)
    if raw_requested_handle is not None and requested_window_handle is None:
        invalid_reason = invalid_reason or "the requested window handle is invalid"
    if trusted_window_handle and requested_window_handle and trusted_window_handle != requested_window_handle:
        invalid_reason = invalid_reason or "the requested window handle is outside the runtime DCC scope"
    effective_window_handle = requested_window_handle or trusted_window_handle
    window_handles = [effective_window_handle] if effective_window_handle else []

    explicit_scope = bool(titles or process_ids or process_names or window_handles)
    return {
        "window_titles": [item for item in titles if str(item).strip()],
        "process_ids": [item for item in process_ids if int(item) > 0],
        "process_names": [item for item in process_names if str(item).strip()],
        "window_handles": [item for item in window_handles if item > 0],
        "excluded_process_ids": [] if explicit_scope else [os.getpid()],
        "require_process_match": bool(process_ids or process_names),
        "native_scope_trusted": bool(trusted_process_id or trusted_window_handle),
        "invalid_reason": invalid_reason,
    }


def _scope_is_explicit(scope: Dict[str, Any]) -> bool:
    return bool(scope["window_titles"] or scope["process_ids"] or scope["process_names"] or scope["window_handles"])


def _scope_is_trusted_native_target(scope: Dict[str, Any]) -> bool:
    return bool(
        not scope.get("invalid_reason")
        and scope.get("native_scope_trusted")
        and not scope.get("process_names")
        and (len(scope.get("process_ids") or []) == 1 or len(scope.get("window_handles") or []) == 1)
    )


def _json_object(value: Any) -> Dict[str, Any]:
    try:
        parsed = json.loads(value)
    except (TypeError, ValueError):
        return {}
    return parsed if isinstance(parsed, dict) else {}


def _backend_unavailable(message: str) -> Dict[str, Any]:
    return skill_error(
        "Windows UI Automation backend is unavailable. "
        f"{message} Install PowerShell with UIAutomationClient support or use DCC_MCP_APP_UI_BACKEND=mock.",
        UiErrorCode.BACKEND_UNAVAILABLE,
        backend="windows-uia",
        hint=(
            "Set DCC_MCP_APP_UI_BACKEND=mock for deterministic testing, or run the adapter in an interactive "
            "Windows desktop session."
        ),
    )


def _role_from_control_type(control_type: Any) -> str:
    name = str(control_type or "").split(".")[-1].lower()
    return {
        "button": "button",
        "calendar": "calendar",
        "checkbox": "checkbox",
        "combobox": "combo_box",
        "custom": "custom",
        "dataitem": "row",
        "edit": "text_field",
        "group": "group",
        "header": "header",
        "hyperlink": "link",
        "image": "image",
        "list": "list",
        "listitem": "list_item",
        "menu": "menu",
        "menuitem": "menu_item",
        "pane": "pane",
        "progressbar": "progress_bar",
        "radiobutton": "radio_button",
        "scrollbar": "scroll_bar",
        "slider": "slider",
        "splitbutton": "button",
        "tab": "tab",
        "tabitem": "tab_item",
        "text": "label",
        "thumb": "thumb",
        "titlebar": "title_bar",
        "toolbar": "tool_bar",
        "tree": "tree",
        "treeitem": "tree_item",
        "window": "window",
    }.get(name, name or "control")


def _bounds_from_raw(raw: Dict[str, Any]) -> Optional[UiBounds]:
    bounds = raw.get("bounds")
    if not isinstance(bounds, dict):
        return None
    try:
        return UiBounds(
            x=float(bounds.get("x") or 0),
            y=float(bounds.get("y") or 0),
            width=float(bounds.get("width") or 0),
            height=float(bounds.get("height") or 0),
        )
    except (TypeError, ValueError):
        return None


def _control_id(raw: Dict[str, Any]) -> str:
    runtime_id = str(raw.get("runtime_id") or "").strip()
    if runtime_id:
        return f"uia:{runtime_id}"
    return f"uia:path:{raw.get('fallback_path') or '0'}"


def _node_from_uia_dict(raw: Dict[str, Any], snapshot_id: str) -> UiControlNode:
    children = [
        _node_from_uia_dict(child, snapshot_id) for child in raw.get("children", []) or [] if isinstance(child, dict)
    ]
    runtime_id = str(raw.get("runtime_id") or "")
    metadata = {
        "app_ui": {
            "backend": "windows-uia",
            "snapshot_id": snapshot_id,
            "runtime_id": runtime_id,
            "fallback_path": raw.get("fallback_path"),
            "process_id": raw.get("process_id"),
            "class_name": raw.get("class_name"),
            "native_window_handle": raw.get("native_window_handle"),
            "control_type": raw.get("control_type"),
        }
    }
    value = raw.get("value")
    checked = raw.get("checked")
    name = str(raw.get("name") or "")
    role = _role_from_control_type(raw.get("control_type"))
    text = name if role == "label" else None
    return UiControlNode(
        id=_control_id(raw),
        role=role,
        label=name or None,
        text=text,
        object_name=str(raw.get("automation_id") or "") or None,
        enabled=bool(raw.get("enabled", True)),
        visible=not bool(raw.get("offscreen", False)),
        bounds=_bounds_from_raw(raw),
        value=str(value) if value is not None else None,
        checked=bool(checked) if checked is not None else None,
        children=children,
        metadata=metadata,
    )


def _iter_nodes(node: Dict[str, Any]) -> Iterable[Dict[str, Any]]:
    yield node
    for child in node.get("children", []) or []:
        if isinstance(child, dict):
            yield from _iter_nodes(child)


def _find_by_id(snapshot: Dict[str, Any], control_id: str) -> Optional[Dict[str, Any]]:
    for node in _iter_nodes(snapshot["root"]):
        if node.get("id") == control_id:
            return node
    return None


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


def _error_from_capture(capture: Dict[str, Any]) -> Dict[str, Any]:
    error = str(capture.get("error") or UiErrorCode.BACKEND_ERROR)
    message = str(capture.get("message") or "Windows UIA backend failed.")
    if error == "backend_unavailable":
        return _backend_unavailable(message)
    return skill_error(message, error, error_code=error, backend="windows-uia")


def _audit_record(
    action: str,
    success: bool,
    control: Optional[Dict[str, Any]],
    session_id: str,
    policy: AppUiPolicy,
    before_focus_id: Optional[str],
    after_focus_id: Optional[str],
    error_code: Optional[str] = None,
    message: Optional[str] = None,
) -> Dict[str, Any]:
    redacted = []
    if action in (UiActionKind.SET_TEXT, UiActionKind.TYPE) and not policy.audit_sensitive_values:
        redacted.append("text")
    return AppUiAuditRecord(
        action_kind=action,
        success=success,
        target_control_id=control.get("id") if control else None,
        target_role=control.get("role") if control else None,
        target_label=control.get("label") if control else None,
        before_focus_id=before_focus_id,
        after_focus_id=after_focus_id,
        error_code=error_code,
        message=message,
        session_id=session_id,
        redacted_fields=redacted,
        metadata={"backend": "windows-uia"},
    ).to_dict()


def _stale_result(control_id: str, session_id: str, requested: str, current: str) -> Dict[str, Any]:
    result = UiActionResult.stale(control_id).to_dict()
    result["metadata"] = {
        "requested_snapshot_id": requested,
        "current_snapshot_id": current,
    }
    audit = AppUiAuditRecord(
        action_kind="unknown",
        success=False,
        target_control_id=control_id,
        error_code=UiErrorCode.STALE_CONTROL,
        message="control is stale; refresh the UI snapshot",
        session_id=session_id,
        metadata=result["metadata"],
    ).to_dict()
    return skill_error(
        "Control is stale; refresh the app_ui snapshot.",
        UiErrorCode.STALE_CONTROL,
        result=result,
        audit=audit,
        current_snapshot_id=current,
    )


def _stale_observation_result(
    action: str,
    control_id: str,
    session_id: str,
    requested: str,
    current: str,
    policy: AppUiPolicy,
) -> Dict[str, Any]:
    message = "Computer-use observation is stale; take a new app_ui snapshot."
    result = UiActionResult(
        success=False,
        control_id=control_id,
        error_code=UiErrorCode.STALE_OBSERVATION,
        message=message,
        metadata={"requested_snapshot_id": requested, "current_snapshot_id": current},
    ).to_dict()
    audit = _audit_record(
        action,
        False,
        None,
        session_id,
        policy,
        None,
        None,
        UiErrorCode.STALE_OBSERVATION,
        message,
    )
    return skill_error(
        message,
        UiErrorCode.STALE_OBSERVATION,
        result=result,
        audit=audit,
        current_snapshot_id=current,
    )


def _is_native_action(action: str, params: Dict[str, Any]) -> bool:
    if action == UiActionKind.CLICK:
        return params.get("x") is not None or params.get("y") is not None
    return action in {
        UiActionKind.MOVE,
        UiActionKind.DOUBLE_CLICK,
        UiActionKind.SCROLL,
        UiActionKind.DRAG,
        UiActionKind.RAW_COORDINATE_CLICK,
        UiActionKind.TYPE,
        UiActionKind.KEYPRESS,
        UiActionKind.KEYBOARD_SHORTCUT,
    }


def _native_action_request(params: Dict[str, Any], observation_id: str) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    request = {
        "action": {
            UiActionKind.RAW_COORDINATE_CLICK: UiActionKind.CLICK,
            UiActionKind.KEYBOARD_SHORTCUT: UiActionKind.KEYPRESS,
        }.get(action, action),
        "observation_id": observation_id,
    }
    for key in ("x", "y", "button", "text", "duration_ms"):
        if params.get(key) is not None:
            request[key] = params[key]
    for key in ("scroll_x", "scroll_y"):
        if params.get(key) is not None:
            request[key] = int(params[key])
    if params.get("path") is not None:
        request["path"] = params["path"]
    if params.get("keys") is not None:
        request["keys"] = params["keys"]
    return request


def _validate_action_limits(params: Dict[str, Any]) -> Optional[Dict[str, Any]]:
    path = params.get("path") or []
    if not isinstance(path, list):
        return skill_error("path must be an array", UiErrorCode.INVALID_ACTION)
    if len(path) > _MAX_DRAG_POINTS:
        return skill_error(
            f"drag path exceeds the {_MAX_DRAG_POINTS}-point safety limit",
            UiErrorCode.INVALID_ACTION,
        )

    keys = params.get("keys") or []
    if not isinstance(keys, list):
        return skill_error("keys must be an array", UiErrorCode.INVALID_ACTION)
    key_count = sum(1 for item in keys for token in str(item).split("+") if token.strip())
    if key_count > _MAX_KEY_TOKENS:
        return skill_error(
            f"keypress exceeds the {_MAX_KEY_TOKENS}-key safety limit",
            UiErrorCode.INVALID_ACTION,
        )

    text = params.get("text")
    if text is not None:
        units = len(str(text).encode("utf-16-le")) // 2
        if units > _MAX_TEXT_UTF16_UNITS:
            return skill_error(
                f"text exceeds the {_MAX_TEXT_UTF16_UNITS}-UTF-16-unit safety limit",
                UiErrorCode.INVALID_ACTION,
            )
    return None


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


def _node_from_dict(raw: Dict[str, Any]) -> UiControlNode:
    bounds = raw.get("bounds") or {}
    return UiControlNode(
        id=str(raw.get("id") or ""),
        role=str(raw.get("role") or "control"),
        label=raw.get("label"),
        text=raw.get("text"),
        object_name=raw.get("object_name"),
        enabled=bool(raw.get("enabled", True)),
        visible=bool(raw.get("visible", True)),
        bounds=UiBounds(
            x=float(bounds.get("x") or 0),
            y=float(bounds.get("y") or 0),
            width=float(bounds.get("width") or 0),
            height=float(bounds.get("height") or 0),
        )
        if bounds
        else None,
        value=raw.get("value"),
        checked=raw.get("checked"),
        children=[_node_from_dict(child) for child in raw.get("children", []) or []],
        metadata=raw.get("metadata") or {},
    )


def _native_fallback_capture(
    session_id: str,
    policy: AppUiPolicy,
    params: Dict[str, Any],
    failure: Dict[str, Any],
) -> Optional[Dict[str, Any]]:
    scope = failure.get("scope") or _scope_from_params(params, policy)
    if not _scope_is_trusted_native_target(scope):
        return None

    state = failure.get("_state") or _load_state(session_id)
    _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
    snapshot_id = str(failure.get("snapshot_id") or _snapshot_id(state))
    process_id = scope["process_ids"][0] if scope["process_ids"] else 0
    window_handle = scope["window_handles"][0] if scope["window_handles"] else 0
    window_title = scope["window_titles"][0] if scope["window_titles"] else ""
    label = str(params.get("app_name") or window_title or "DCC application")
    backend_metadata = {
        "backend": "windows-native-fallback",
        "snapshot_id": snapshot_id,
        "process_id": process_id,
        "native_window_handle": window_handle,
        "fallback_reason": str(failure.get("message") or "Windows UIA snapshot failed."),
    }
    root = UiControlNode(
        id=f"native:window:{window_handle}" if window_handle else f"native:process:{process_id}",
        role="window",
        label=label,
        metadata={"app_ui": backend_metadata},
    )
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        metadata={
            "snapshot_id": snapshot_id,
            "app_ui": {
                **backend_metadata,
                "scope": scope,
            },
        },
    ).to_dict()
    state["last_snapshot_id"] = snapshot_id
    _save_state(state)
    return {
        "success": True,
        "snapshot": snapshot,
        "snapshot_id": snapshot_id,
        "scope": scope,
        "target": {
            "name": window_title,
            "process_id": process_id,
            "native_window_handle": window_handle,
        },
    }


def _computer_use_session_matches_scope(
    existing_spec: Any,
    resolved_spec: Any,
    scope: Dict[str, Any],
) -> bool:
    """Keep a process-scoped session stable across transient UIA root changes."""
    if existing_spec == resolved_spec:
        return True
    if scope.get("window_handles"):
        return False
    try:
        existing_process_id = _positive_int(existing_spec[0])
        resolved_process_id = _positive_int(resolved_spec[0])
    except (IndexError, TypeError):
        return False
    return existing_process_id is not None and existing_process_id == resolved_process_id


def _computer_use_screenshot_impl(
    session_id: str,
    capture: Dict[str, Any],
    params: Dict[str, Any],
    computer_use_session_type: Any,
    stop_session: Callable[[str], Dict[str, Any]],
    latch_user_interrupt: Callable[[str], None],
    user_interrupted_capture: Callable[[], Dict[str, Any]],
) -> Dict[str, Any]:
    if computer_use_session_type is None:
        return {
            "success": False,
            "error": "backend_unavailable",
            "message": "Native ComputerUseSession is unavailable in this dcc-mcp-core build.",
        }
    scope = capture.get("scope") or {}
    if not scope.get("native_scope_trusted"):
        return {
            "success": False,
            "error": UiErrorCode.PERMISSION_DENIED,
            "message": (
                "Native DCC MCP Computer Use requires an operator-bound DCC scope. "
                "Set DCC_MCP_APP_UI_UIA_PROCESS_ID or DCC_MCP_APP_UI_UIA_WINDOW_HANDLE "
                "in the adapter environment before enabling raw input."
            ),
        }
    if scope.get("process_names"):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": (
                "process_name scopes are observation-only for native Computer Use; "
                "bind the adapter to an exact DCC process id or window handle instead."
            ),
        }
    if len(scope.get("process_ids") or []) != 1 and len(scope.get("window_handles") or []) != 1:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": (
                "Native DCC MCP Computer Use requires one exact process_id or window_handle; "
                "title-only and process-name scopes are observation-only because they can match the wrong app."
            ),
        }

    target = capture.get("target")
    if not isinstance(target, dict):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The UI snapshot did not resolve one native DCC window.",
        }
    target_process_id = _positive_int(target.get("process_id"))
    target_window_handle = _positive_int(target.get("native_window_handle"))
    scoped_process_ids = {int(item) for item in scope.get("process_ids") or []}
    scoped_window_handles = {int(item) for item in scope.get("window_handles") or []}
    target_title = str(target.get("name") or "").strip()
    scoped_titles = [str(item).strip() for item in scope.get("window_titles") or [] if str(item).strip()]
    if scoped_process_ids and target_process_id not in scoped_process_ids:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the operator-bound DCC process scope.",
        }
    if scoped_window_handles and target_window_handle not in scoped_window_handles:
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the operator-bound DCC window scope.",
        }
    if scoped_titles and not any(title.casefold() in target_title.casefold() for title in scoped_titles):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": "The resolved UI window is outside the scoped DCC title allowlist.",
        }

    resume = bool(params.get("resume_computer_use"))
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if entry and hasattr(entry["session"], "status"):
        with suppress(Exception):
            current_status = _json_object(entry["session"].status())
            if current_status.get("user_interrupted"):
                latch_user_interrupt(session_id)
                entry = None
    if session_id in _COMPUTER_USE_INTERRUPTED:
        if not resume:
            return user_interrupted_capture()
        _COMPUTER_USE_INTERRUPTED.discard(session_id)

    process_id = target_process_id
    window_handle = target_window_handle
    window_title = str(target.get("name") or "") or None
    app_name = str(
        params.get("app_name") or os.environ.get("DCC_MCP_COMPUTER_USE_APP_NAME") or window_title or "DCC application"
    )
    spec = (process_id, window_handle, window_title, app_name)
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if entry and not _computer_use_session_matches_scope(entry["spec"], spec, scope):
        stop_status = stop_session(session_id)
        if stop_status.get("cleanup_pending"):
            return {
                "success": False,
                "error": UiErrorCode.BACKEND_UNAVAILABLE,
                "message": (
                    "The previous Computer Use session is still removing its input owner and overlays; retry shortly."
                ),
                "cleanup_pending": True,
            }
        entry = None
    if not entry:
        try:
            session = computer_use_session_type(
                process_id=process_id,
                window_handle=window_handle,
                window_title=window_title,
                app_name=app_name,
            )
        except Exception as exc:
            return {"success": False, "error": "backend_unavailable", "message": str(exc)}
        entry = {"session": session, "spec": spec}
        _COMPUTER_USE_SESSIONS[session_id] = entry
    session = entry["session"]
    if resume:
        try:
            resumed = _json_object(session.resume_after_user_approval())
        except Exception as exc:
            stop_session(session_id)
            return {"success": False, "error": "backend_unavailable", "message": str(exc)}
        if not resumed.get("success"):
            stop_session(session_id)
            return resumed
    try:
        started = _json_object(session.start())
    except Exception as exc:
        stop_session(session_id)
        return {"success": False, "error": "backend_unavailable", "message": str(exc)}
    if not started.get("success"):
        if started.get("error") == UiErrorCode.USER_INTERRUPTED or started.get("user_interrupted"):
            latch_user_interrupt(session_id)
            return user_interrupted_capture()
        if started.get("error") == UiErrorCode.DESKTOP_UNAVAILABLE:
            _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
            return {
                "success": False,
                "error": UiErrorCode.DESKTOP_UNAVAILABLE,
                "message": str(started.get("message") or _DESKTOP_UNAVAILABLE_MESSAGE),
            }
        stop_session(session_id)
        return started
    try:
        metadata_json, image = session.screenshot()
    except Exception as exc:
        status = {}
        if hasattr(session, "status"):
            with suppress(Exception):
                status = _json_object(session.status())
        if status.get("user_interrupted"):
            latch_user_interrupt(session_id)
            return user_interrupted_capture()
        stop_session(session_id)
        return {"success": False, "error": "capture_failed", "message": str(exc)}
    metadata = _json_object(metadata_json)
    if not metadata.get("success") or image is None:
        if metadata.get("error") == UiErrorCode.USER_INTERRUPTED or metadata.get("user_interrupted"):
            latch_user_interrupt(session_id)
            return user_interrupted_capture()
        if metadata.get("error") == UiErrorCode.DESKTOP_UNAVAILABLE:
            _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
            return {
                "success": False,
                "error": UiErrorCode.DESKTOP_UNAVAILABLE,
                "message": str(metadata.get("message") or _DESKTOP_UNAVAILABLE_MESSAGE),
            }
        if metadata.get("error") in {UiErrorCode.MISSING_WINDOW, UiErrorCode.FOCUS_LOST}:
            status = {}
            if hasattr(session, "status"):
                with suppress(Exception):
                    status = _json_object(session.status())
            if status.get("active"):
                _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
                return metadata
        stop_session(session_id)
        return metadata or {
            "success": False,
            "error": "capture_failed",
            "message": "Native computer-use screenshot returned no PNG data.",
        }
    observation = metadata.get("observation")
    if not isinstance(observation, dict) or not observation.get("observation_id"):
        stop_session(session_id)
        return {
            "success": False,
            "error": "capture_failed",
            "message": "Native computer-use screenshot returned no observation id.",
        }
    _COMPUTER_USE_OBSERVATIONS[session_id] = {
        "snapshot_id": capture["snapshot_id"],
        "observation_id": str(observation["observation_id"]),
    }
    return {
        "success": True,
        "image": bytes(image),
        "mime_type": str(metadata.get("mime_type") or "image/png"),
        "observation": observation,
        "status": started,
    }
