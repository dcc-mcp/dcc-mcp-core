"""State and pure helpers for the Windows UI Automation backend."""

from __future__ import annotations

import os
from pathlib import Path
import threading
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional

from dcc_mcp_core.adapter_contracts import UiActionKind
from dcc_mcp_core.adapter_contracts import UiBounds
from dcc_mcp_core.adapter_contracts import UiControlNode
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.adapter_contracts import UiErrorCode
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
_SESSION_LOCKS: Dict[str, threading.RLock] = {}
_SESSION_LOCKS_GUARD = threading.Lock()
_MAX_DRAG_POINTS = 256
_MAX_KEY_TOKENS = 16
_MAX_GAME_NAVIGATION_KEYS = 4
_MAX_TEXT_UTF16_UNITS = 4096
_GAME_NAVIGATION_NAMED_KEYS = _key_set(
    "CTRL CONTROL LCTRL LEFTCTRL LEFT_CTRL CTRL_L CONTROL_L RCTRL RIGHTCTRL RIGHT_CTRL CTRL_R CONTROL_R "
    "SHIFT LSHIFT LEFTSHIFT LEFT_SHIFT SHIFT_L RSHIFT RIGHTSHIFT RIGHT_SHIFT SHIFT_R "
    "ALT LALT LEFTALT LEFT_ALT ALT_L RALT RIGHTALT RIGHT_ALT ALT_R ALTGR "
    "ESC ESCAPE TAB SPACE PRINTSCREEN PRINT_SCREEN PRTSC ENTER RETURN BACKSPACE DELETE DEL INSERT INS "
    "LEFT ARROWLEFT ARROW_LEFT UP ARROWUP ARROW_UP RIGHT ARROWRIGHT ARROW_RIGHT DOWN ARROWDOWN ARROW_DOWN "
    "HOME END PAGEUP PAGE_UP PGUP PAGEDOWN PAGE_DOWN PGDN CAPSLOCK CAPS_LOCK NUMLOCK NUM_LOCK "
    "SCROLLLOCK SCROLL_LOCK PAUSE KP_DECIMAL KPDECIMAL NUMPAD_DECIMAL "
    "SEMICOLON EQUAL EQUALS COMMA MINUS PERIOD DOT SLASH GRAVE BACKTICK LEFTBRACKET BRACKETLEFT "
    "BACKSLASH RIGHTBRACKET BRACKETRIGHT APOSTROPHE QUOTE"
)
_GAME_NAVIGATION_PUNCTUATION = frozenset(";=,-./`[]\\'")
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


def _policy_from_params(params: Dict[str, Any]) -> UiControlPolicy:
    raw = params.get("policy") or {}
    if not isinstance(raw, dict):
        raw = {}
    ceiling = _env_flag("DCC_MCP_COMPUTER_USE_ALLOW_RAW_INPUT")
    return UiControlPolicy(
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


def _scope_from_params(params: Dict[str, Any], policy: UiControlPolicy) -> Dict[str, Any]:
    invalid_reason = None
    trusted_title = str(os.environ.get("DCC_MCP_UI_CONTROL_UIA_WINDOW_TITLE") or "").strip()
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
    raw_trusted_pid = os.environ.get("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID")
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

    trusted_process_name = str(os.environ.get("DCC_MCP_UI_CONTROL_UIA_PROCESS_NAME") or "").strip()
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
            "system, terminal, authentication, and password-manager processes are not allowed ui_control targets"
        )
    process_names = [effective_process_name] if effective_process_name else []

    raw_trusted_handle = os.environ.get("DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE")
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


def _scope_is_trusted_native_target(scope: Dict[str, Any]) -> bool:
    return bool(
        not scope.get("invalid_reason")
        and scope.get("native_scope_trusted")
        and not scope.get("process_names")
        and (len(scope.get("process_ids") or []) == 1 or len(scope.get("window_handles") or []) == 1)
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
        "ui_control": {
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
        UiActionKind.GAME_NAVIGATION,
        UiActionKind.KEYBOARD_SHORTCUT,
    }


def _is_game_navigation_key(token: str) -> bool:
    upper = token.upper()
    if len(upper) == 1 and upper.isascii() and (upper.isalnum() or upper in _GAME_NAVIGATION_PUNCTUATION):
        return True
    if upper.startswith("F") and upper[1:].isdigit():
        return 1 <= int(upper[1:]) <= 24
    if upper.startswith("KP_") and len(upper) == 4 and upper[-1].isdigit():
        return True
    return upper in _GAME_NAVIGATION_NAMED_KEYS


def _game_navigation_keys(keys: List[Any]) -> Optional[List[str]]:
    tokens: List[str] = []
    for item in keys:
        if not isinstance(item, str):
            return None
        parts = [token.strip() for token in item.split("+")]
        if not parts or any(not token for token in parts):
            return None
        tokens.extend(parts)
    normalized = [token.upper() for token in tokens]
    if (
        not 1 <= len(normalized) <= _MAX_GAME_NAVIGATION_KEYS
        or len(set(normalized)) != len(normalized)
        or not all(_is_game_navigation_key(token) for token in normalized)
    ):
        return None
    return normalized


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
    if params.get("action") == UiActionKind.GAME_NAVIGATION:
        if _game_navigation_keys(keys) is None:
            return skill_error(
                "game_navigation requires one to four distinct supported canvas keys",
                UiErrorCode.INVALID_ACTION,
            )
        duration_ms = params.get("duration_ms")
        if duration_ms is not None and (type(duration_ms) is not int or not 0 <= duration_ms <= 500):
            return skill_error(
                "game_navigation duration_ms must be an integer from 0 through 500",
                UiErrorCode.INVALID_ACTION,
            )
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
