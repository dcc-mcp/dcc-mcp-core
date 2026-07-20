"""CDP backend for the bundled ui_control skill."""

from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import tempfile
import threading
import time
from typing import Any
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from urllib.parse import quote
from urllib.parse import quote_plus

if __package__:
    from ._cdp_runtime import CdpBackendError as ChromeBackendError
    from ._cdp_runtime import CdpClient as _CdpClient
    from ._cdp_runtime import ensure_cdp_target as _ensure_cdp_target
else:
    from _cdp_runtime import CdpBackendError as ChromeBackendError
    from _cdp_runtime import CdpClient as _CdpClient
    from _cdp_runtime import ensure_cdp_target as _ensure_cdp_target

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
    cleaned = "".join(ch if ch.isalnum() or ch in "_.-" else "_" for ch in text)
    return cleaned[:80] or "default"


def _state_dir() -> Path:
    root = os.environ.get("DCC_MCP_UI_CONTROL_CHROME_STATE_DIR")
    path = Path(root) if root else Path(tempfile.gettempdir()) / "dcc-mcp-ui-control-chrome"
    path.mkdir(parents=True, exist_ok=True)
    return path


def _state_path(session_id: str) -> Path:
    return _state_dir() / f"{_safe_session_id(session_id)}.json"


def _default_state(session_id: str) -> Dict[str, Any]:
    return {
        "session_id": session_id,
        "revision": 1,
        "focus_id": "address-search",
        "query": "",
        "status": "Idle",
        "current_url": "about:blank",
        "title": "Chrome",
        "body_text": "",
        "port": 0,
        "pid": 0,
        "web_socket_url": "",
        "cdp_endpoint": "",
        "preset": "",
        "launch_mode": "",
        "user_data_dir": "",
    }


def _load_state(session_id: str) -> Dict[str, Any]:
    path = _state_path(session_id)
    if not path.exists():
        return _default_state(session_id)
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        data = {}
    state = _default_state(session_id)
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


def _policy_from_params(params: Dict[str, Any]) -> UiControlPolicy:
    raw = params.get("policy") or {}
    if not isinstance(raw, dict):
        raw = {}
    return UiControlPolicy().narrowed({key: raw[key] for key in _POLICY_KEYS if key in raw})


def _ensure_chrome(state: Dict[str, Any]) -> Dict[str, Any]:
    return _ensure_cdp_target(state)


def _with_cdp(state: Dict[str, Any]) -> _CdpClient:
    state = _ensure_chrome(state)
    return _CdpClient(str(state["web_socket_url"]))


def _refresh_from_browser(state: Dict[str, Any]) -> Dict[str, Any]:
    try:
        with _with_cdp(state) as cdp:
            value = cdp.call(
                "Runtime.evaluate",
                {
                    "expression": (
                        "JSON.stringify({url: location.href, title: document.title, "
                        "text: document.body ? document.body.innerText.slice(0, 4000) : ''})"
                    ),
                    "returnByValue": True,
                },
            )
    except Exception:
        return state
    raw = value.get("result", {}).get("value")
    try:
        page = json.loads(raw) if isinstance(raw, str) else {}
    except Exception:
        page = {}
    state["current_url"] = page.get("url") or state.get("current_url") or "about:blank"
    state["title"] = page.get("title") or state.get("title") or "Chrome"
    state["body_text"] = page.get("text") or ""
    query = str(state.get("query") or "").lower()
    haystack = " ".join(str(state.get(key) or "").lower() for key in ("current_url", "title", "body_text"))
    if query and query in haystack:
        state["status"] = "Search complete"
    return state


def _navigate_search(state: Dict[str, Any]) -> Dict[str, Any]:
    query = str(state.get("query") or "").strip()
    if not query:
        raise ChromeBackendError("No search query has been set")
    preset = str(state.get("preset") or "reuse")
    template = os.environ.get("DCC_MCP_UI_CONTROL_SEARCH_URL")
    if not template and preset == "auroraview":
        template = os.environ.get("DCC_MCP_UI_CONTROL_AURORAVIEW_SEARCH_URL")
    if not template:
        template = os.environ.get(
            "DCC_MCP_UI_CONTROL_CHROME_SEARCH_URL",
            "https://www.google.com/search?q={query}",
        )
    url = template.format(query=quote_plus(query), raw_query=quote(query))
    with _with_cdp(state) as cdp:
        cdp.call("Page.enable")
        cdp.call("Page.navigate", {"url": url})
    time.sleep(float(os.environ.get("DCC_MCP_UI_CONTROL_CHROME_NAV_WAIT_SECS", "2.0")))
    state["current_url"] = url
    state["status"] = "Search submitted"
    return _refresh_from_browser(state)


def _node(
    node_id: str,
    role: str,
    label: Optional[str] = None,
    text: Optional[str] = None,
    object_name: Optional[str] = None,
    value: Optional[str] = None,
    bounds: Optional[UiBounds] = None,
    snapshot_id: Optional[str] = None,
) -> UiControlNode:
    metadata: Dict[str, Any] = {"ui_control": {"backend": "chrome-cdp"}}
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
        children=[],
        metadata=metadata,
    )


def _app_label(state: Dict[str, Any]) -> str:
    preset = str(state.get("preset") or "")
    if preset == "auroraview":
        return "AuroraView"
    if preset == "edge":
        return "Edge"
    if preset == "agent-browser":
        return "Agent Browser"
    return "Chrome"


def _app_window_object_name(state: Dict[str, Any]) -> str:
    preset = str(state.get("preset") or "")
    if preset == "auroraview":
        return "auroraviewAppWindow"
    if preset == "edge":
        return "edgeAppWindow"
    if preset == "agent-browser":
        return "agentBrowserAppWindow"
    return "chromeAppWindow"


def _snapshot_from_state(state: Dict[str, Any]) -> UiSnapshot:
    sid = _snapshot_id(state)
    app_label = _app_label(state)
    children = [
        _node(
            "address-search",
            "text_field",
            label="Address and search bar",
            object_name="chromeOmnibox",
            text=str(state.get("query") or ""),
            value=str(state.get("query") or ""),
            bounds=UiBounds(x=120, y=48, width=620, height=34),
            snapshot_id=sid,
        ),
        _node(
            "search-button",
            "button",
            label="Search",
            object_name="chromeSearchSubmit",
            bounds=UiBounds(x=752, y=48, width=88, height=34),
            snapshot_id=sid,
        ),
        _node(
            "status",
            "label",
            label="Status",
            object_name="chromeSearchStatus",
            text=str(state.get("status") or ""),
            bounds=UiBounds(x=24, y=92, width=420, height=28),
            snapshot_id=sid,
        ),
        _node(
            "page-title",
            "label",
            label="Page title",
            object_name="chromePageTitle",
            text=str(state.get("title") or ""),
            bounds=UiBounds(x=24, y=124, width=560, height=28),
            snapshot_id=sid,
        ),
        _node(
            "current-url",
            "label",
            label="Current URL",
            object_name="chromeCurrentUrl",
            text=str(state.get("current_url") or ""),
            bounds=UiBounds(x=24, y=156, width=760, height=28),
            snapshot_id=sid,
        ),
    ]
    root = _node(
        "chrome-window",
        "window",
        label=str(state.get("title") or app_label),
        object_name=_app_window_object_name(state),
        bounds=UiBounds(x=80, y=80, width=960, height=720),
        snapshot_id=sid,
    )
    root.children = children
    return UiSnapshot(
        root=root,
        session_id=str(state["session_id"]),
        focus_id=str(state.get("focus_id") or ""),
        truncated=False,
        node_count=1 + len(children),
        metadata={
            "snapshot_id": sid,
            "ui_control": {
                "backend": "chrome-cdp",
                "preset": state.get("preset"),
                "launch_mode": state.get("launch_mode"),
                "browser_pid": state.get("pid"),
                "debug_port": state.get("port"),
                "cdp_endpoint": state.get("cdp_endpoint"),
                "current_url": state.get("current_url"),
            },
        },
    )


def _snapshot_dict(state: Dict[str, Any]) -> Dict[str, Any]:
    return _snapshot_from_state(state).to_dict()


def _iter_nodes(node: Dict[str, Any]) -> Iterable[Dict[str, Any]]:
    yield node
    for child in node.get("children", []) or []:
        if isinstance(child, dict):
            yield from _iter_nodes(child)


def _find_controls(snapshot: Dict[str, Any], params: Dict[str, Any]) -> List[Dict[str, Any]]:
    query = str(params.get("query") or "").lower()
    role = str(params.get("role") or "").lower()
    label = str(params.get("label") or "").lower()
    object_name = str(params.get("object_name") or "").lower()
    limit = int(params.get("limit") or 10)
    matches: List[Dict[str, Any]] = []
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


def _has_scoped_window(state: Dict[str, Any]) -> bool:
    title = str(state.get("title") or state.get("window_title") or "").strip()
    try:
        process_id = int(state.get("pid") or state.get("process_id") or 0)
    except Exception:
        process_id = 0
    return bool(title) or process_id > 0


def _window_allowed(state: Dict[str, Any], policy: UiControlPolicy) -> bool:
    if policy.scope_denied:
        return False
    if policy.require_scoped_window and not _has_scoped_window(state):
        return False
    if policy.allowed_window_titles:
        title = str(state.get("title") or "Chrome").lower()
        allowed = [str(item).lower() for item in policy.allowed_window_titles]
        if not any(item in title for item in allowed):
            return False
    if policy.allowed_process_ids:
        try:
            process_id = int(state.get("pid") or 0)
        except Exception:
            process_id = 0
        if process_id not in policy.allowed_process_ids:
            return False
    return True


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
    redacted = ["text"] if action == UiActionKind.SET_TEXT and not policy.audit_sensitive_values else []
    audit_metadata = {
        "backend": "chrome-cdp",
        "preset": state.get("preset"),
        "launch_mode": state.get("launch_mode"),
        "snapshot_id": _snapshot_id(state),
        "current_url": state.get("current_url"),
    }
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


def _policy_denied(
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


def _backend_unavailable(exc: Exception, state: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    raw_preset = (
        (state or {}).get("preset")
        or os.environ.get("DCC_MCP_UI_CONTROL_CDP_PRESET")
        or os.environ.get("DCC_MCP_UI_CONTROL_CHROME_PRESET")
        or "reuse"
    )
    return skill_error(
        str(exc),
        "backend_unavailable",
        backend="chrome",
        cdp_preset=str(raw_preset),
    )


def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error("ui_control snapshot disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        state = _ensure_chrome(state)
        state = _refresh_from_browser(state)
        _save_state(state)
    except Exception as exc:
        return _backend_unavailable(exc, state)
    if not _window_allowed(state, policy):
        return skill_error("scoped Chrome window is not allowed by policy", UiErrorCode.MISSING_WINDOW)
    snapshot = _snapshot_dict(state)
    return skill_success(
        "Captured Chrome ui_control snapshot.",
        prompt="Use ui_control__find to resolve a control, then ui_control__act with the returned snapshot_id.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        snapshot=snapshot,
        policy=policy.to_dict(),
    )


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Idempotent no-op for CDP, which does not own native input."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    return skill_success(
        "No native Computer Use session was active for the Chrome backend.",
        session_id=session_id,
        active=False,
        was_active=False,
        user_interrupted=False,
    )


def system_operation_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Reject Windows-only system configuration on browser backends."""
    del params
    return skill_error(
        "Typed system operations are only available through the Windows UI Control host.",
        "unsupported",
        backend="chrome-cdp",
    )


def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    policy = _policy_from_params(params)
    if not policy.allow_find:
        return skill_error("ui_control find disabled by policy", UiErrorCode.POLICY_DISABLED)
    try:
        state = _refresh_from_browser(_ensure_chrome(state))
    except Exception as exc:
        return _backend_unavailable(exc, state)
    _save_state(state)
    if not _window_allowed(state, policy):
        return skill_error("scoped Chrome window is not allowed by policy", UiErrorCode.MISSING_WINDOW)
    snapshot = _snapshot_dict(state)
    matches = _find_controls(snapshot, params)
    return skill_success(
        f"Found {len(matches)} Chrome ui_control control(s).",
        prompt="Use ui_control__act with a returned control id, then ui_control__wait_for.",
        session_id=session_id,
        snapshot_id=snapshot["metadata"]["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    state = _load_state(session_id)
    try:
        state = _refresh_from_browser(_ensure_chrome(state))
    except Exception as exc:
        return _backend_unavailable(exc, state)
    policy = _policy_from_params(params)
    control_id = str(params.get("control_id") or "")
    action = str(params.get("action") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    if requested_snapshot_id and requested_snapshot_id != _snapshot_id(state):
        return _stale_result(control_id, action, state, policy, requested_snapshot_id)
    snapshot = _snapshot_dict(state)
    control = _find_by_id(snapshot, control_id)
    if not control:
        message = "control not found in scoped Chrome ui_control window"
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
            "Control not found in scoped Chrome ui_control window.",
            UiErrorCode.NOT_FOUND,
            result=result,
            audit=audit,
        )
    if not _window_allowed(state, policy):
        return _policy_denied(action, control_id, control, state, policy, "scoped Chrome window is not allowed")
    if not policy.allows_action(action):
        return _policy_denied(
            action,
            control_id,
            control,
            state,
            policy,
            f"ui_control action {action!r} disabled by policy",
        )

    before_focus = state.get("focus_id")
    message = "ui_control action completed"
    try:
        if action == UiActionKind.FOCUS:
            state["focus_id"] = control_id
        elif action == UiActionKind.SET_TEXT and control_id == "address-search":
            state["query"] = str(params.get("text") or "")
            state["focus_id"] = control_id
            state["status"] = "Query ready"
        elif action == UiActionKind.CLICK and control_id == "search-button":
            state["focus_id"] = control_id
            state = _navigate_search(state)
            message = "Chrome search submitted"
        else:
            return _unsupported_action(action, control, state, policy, "unsupported Chrome ui_control action")
    except Exception as exc:
        return skill_error(str(exc), "backend_error", backend="chrome")

    state["revision"] = int(state.get("revision") or 1) + 1
    state = _refresh_from_browser(state)
    _save_state(state)
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        before_focus_id=before_focus,
        after_focus_id=state.get("focus_id"),
        metadata={"snapshot_id": _snapshot_id(state), "current_url": state.get("current_url")},
    ).to_dict()
    audit = _audit_record(action, True, control, state, policy, None, message, before_focus_id=before_focus)
    return skill_success(
        f"Completed Chrome ui_control action {action!r} on {control_id}.",
        prompt="Use ui_control__wait_for to poll for the expected UI state, then ui_control__snapshot to verify.",
        session_id=session_id,
        snapshot_id=_snapshot_id(state),
        result=result,
        audit=audit,
    )


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
    if condition.kind == UiWaitConditionKind.FOCUSED:
        return snapshot.get("focus_id") == control.get("id")
    if condition.kind == UiWaitConditionKind.ENABLED:
        return bool(control.get("enabled"))
    if condition.kind == UiWaitConditionKind.DISABLED:
        return not bool(control.get("enabled"))
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
    start = time.monotonic()
    last_snapshot = None
    while True:
        if _CLEANUP_REQUESTED.is_set():
            return skill_error(
                "ui_control wait cancelled because the backend is stopping.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        state = _load_state(session_id)
        try:
            state = _refresh_from_browser(_ensure_chrome(state))
        except Exception as exc:
            return _backend_unavailable(exc, state)
        _save_state(state)
        if not _window_allowed(state, policy):
            elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
            message = "scoped Chrome window is not allowed by policy"
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
