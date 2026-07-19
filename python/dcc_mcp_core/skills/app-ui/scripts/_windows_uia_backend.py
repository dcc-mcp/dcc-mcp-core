"""Windows UI Automation backend for the bundled app_ui skill.

The backend is intentionally optional and Windows-only. It uses PowerShell's
standard UIAutomationClient assembly instead of adding a Python dependency.
"""

from __future__ import annotations

import atexit
import base64
from contextlib import suppress
from functools import wraps
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
from typing import Any
from typing import Callable
from typing import Dict
from typing import Optional

from dcc_mcp_core.adapter_contracts import AppUiPolicy
from dcc_mcp_core.adapter_contracts import UiActionRequest
from dcc_mcp_core.adapter_contracts import UiActionResult
from dcc_mcp_core.adapter_contracts import UiErrorCode
from dcc_mcp_core.adapter_contracts import UiSnapshot
from dcc_mcp_core.adapter_contracts import UiWaitResult
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_success

try:
    from dcc_mcp_core import ComputerUseSession as _ComputerUseSession
except (AttributeError, ImportError):
    _ComputerUseSession = None


def _load_support_module() -> Any:
    path = Path(__file__).with_name("_windows_uia_support.py")
    spec = importlib.util.spec_from_file_location(f"{__name__}._support", path)
    if spec is None or spec.loader is None:
        raise ImportError(f"Unable to load Windows UIA support module from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


_SUPPORT = _load_support_module()
_POLICY_KEYS = _SUPPORT._POLICY_KEYS
_CONDITION_KEYS = _SUPPORT._CONDITION_KEYS
_COMPUTER_USE_SESSIONS = _SUPPORT._COMPUTER_USE_SESSIONS
_COMPUTER_USE_OBSERVATIONS = _SUPPORT._COMPUTER_USE_OBSERVATIONS
_COMPUTER_USE_INTERRUPTED = _SUPPORT._COMPUTER_USE_INTERRUPTED
_COMPUTER_USE_STOPPING = _SUPPORT._COMPUTER_USE_STOPPING
_SESSION_STOP_GENERATIONS = _SUPPORT._SESSION_STOP_GENERATIONS
_SESSION_LOCKS = _SUPPORT._SESSION_LOCKS
_SESSION_STOP_LOCKS = _SUPPORT._SESSION_STOP_LOCKS
_SESSION_LOCKS_GUARD = _SUPPORT._SESSION_LOCKS_GUARD
_CLEANUP_REQUESTED = _SUPPORT._CLEANUP_REQUESTED
_MAX_DRAG_POINTS = _SUPPORT._MAX_DRAG_POINTS
_MAX_KEY_TOKENS = _SUPPORT._MAX_KEY_TOKENS
_MAX_TEXT_UTF16_UNITS = _SUPPORT._MAX_TEXT_UTF16_UNITS
_DENIED_PROCESS_NAMES = _SUPPORT._DENIED_PROCESS_NAMES
_DESKTOP_UNAVAILABLE_MESSAGE = _SUPPORT._DESKTOP_UNAVAILABLE_MESSAGE

_safe_session_id = _SUPPORT._safe_session_id
_session_lock = _SUPPORT._session_lock
_session_stop_lock = _SUPPORT._session_stop_lock
_mark_session_stopping = _SUPPORT._mark_session_stopping
_session_stop_generation = _SUPPORT._session_stop_generation
_bump_session_stop_generation = _SUPPORT._bump_session_stop_generation
_desktop_unavailable_result = _SUPPORT._desktop_unavailable_result
_state_dir = _SUPPORT._state_dir
_state_path = _SUPPORT._state_path
_load_state = _SUPPORT._load_state
_save_state = _SUPPORT._save_state
_snapshot_id = _SUPPORT._snapshot_id
_policy_from_params = _SUPPORT._policy_from_params
_env_flag = _SUPPORT._env_flag
_positive_int = _SUPPORT._positive_int
_intersect_title_constraints = _SUPPORT._intersect_title_constraints
_process_name_key = _SUPPORT._process_name_key
_scope_from_params = _SUPPORT._scope_from_params
_scope_is_explicit = _SUPPORT._scope_is_explicit
_scope_is_trusted_native_target = _SUPPORT._scope_is_trusted_native_target
_json_object = _SUPPORT._json_object
_backend_unavailable = _SUPPORT._backend_unavailable
_role_from_control_type = _SUPPORT._role_from_control_type
_bounds_from_raw = _SUPPORT._bounds_from_raw
_control_id = _SUPPORT._control_id
_node_from_uia_dict = _SUPPORT._node_from_uia_dict
_iter_nodes = _SUPPORT._iter_nodes
_find_by_id = _SUPPORT._find_by_id
_find_controls = _SUPPORT._find_controls
_error_from_capture = _SUPPORT._error_from_capture
_native_fallback_capture = _SUPPORT._native_fallback_capture
_audit_record = _SUPPORT._audit_record
_stale_result = _SUPPORT._stale_result
_stale_observation_result = _SUPPORT._stale_observation_result
_is_native_action = _SUPPORT._is_native_action
_native_action_request = _SUPPORT._native_action_request
_validate_action_limits = _SUPPORT._validate_action_limits
_condition_from_params = _SUPPORT._condition_from_params
_resolve_condition_control = _SUPPORT._resolve_condition_control
_condition_matches = _SUPPORT._condition_matches
_node_from_dict = _SUPPORT._node_from_dict

_UIA_SCRIPT = Path(__file__).with_name("_windows_uia_backend.ps1").read_text(encoding="utf-8")

_UIA_HELPERS = Path(__file__).with_name("_windows_uia_helpers.ps1").read_text(encoding="utf-8")
_UIA_SCRIPT = _UIA_SCRIPT.replace("# DCC_MCP_UIA_HELPERS", _UIA_HELPERS)


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


def _native_desktop_interactive() -> bool:
    if _ComputerUseSession is None:
        return True
    checker = getattr(_ComputerUseSession, "desktop_interactive", None)
    if not callable(checker):
        return True
    try:
        return bool(checker())
    except Exception:
        return False


def _serialize_session_call(
    function: Callable[[Optional[Dict[str, Any]]], Dict[str, Any]],
) -> Callable[[Optional[Dict[str, Any]]], Dict[str, Any]]:
    @wraps(function)
    def wrapped(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        resolved = dict(params) if params is not None else _read_params()
        session_id = _safe_session_id(resolved.get("session_id"))
        if session_id in _COMPUTER_USE_STOPPING:
            return skill_error(
                "DCC UI Control is stopping; retry after stop completes.",
                UiErrorCode.BACKEND_UNAVAILABLE,
            )
        with _session_lock(session_id):
            if session_id in _COMPUTER_USE_STOPPING:
                return skill_error(
                    "DCC UI Control is stopping; retry after stop completes.",
                    UiErrorCode.BACKEND_UNAVAILABLE,
                )
            if not _native_desktop_interactive():
                return _desktop_unavailable_result(session_id)
            return function(resolved)

    return wrapped


def _request_stop_computer_use_session(session_id: str) -> bool:
    entry = _COMPUTER_USE_SESSIONS.get(_safe_session_id(session_id))
    if not entry:
        return False
    request_stop = getattr(entry["session"], "request_stop", None)
    if callable(request_stop):
        with suppress(Exception):
            request_stop()
    return True


def _stop_computer_use_session(session_id: str) -> Dict[str, Any]:
    safe_id = _safe_session_id(session_id)
    _request_stop_computer_use_session(safe_id)
    entry = _COMPUTER_USE_SESSIONS.get(safe_id)
    _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
    if not entry:
        return {"success": True, "active": False, "cleanup_pending": False}
    try:
        raw = _json_object(entry["session"].stop())
    except Exception as exc:
        return {
            "success": False,
            "active": False,
            "cleanup_pending": True,
            "message": f"DCC UI Control cleanup could not be confirmed: {exc}",
        }
    if raw.get("cleanup_pending"):
        return {
            **raw,
            "success": False,
            "active": False,
            "cleanup_pending": True,
        }
    if _COMPUTER_USE_SESSIONS.get(safe_id) is entry:
        _COMPUTER_USE_SESSIONS.pop(safe_id, None)
    return {
        **raw,
        "success": bool(raw.get("success", True)),
        "active": False,
        "cleanup_pending": False,
    }


def _latch_user_interrupt(session_id: str) -> None:
    safe_id = _safe_session_id(session_id)
    _COMPUTER_USE_INTERRUPTED.add(safe_id)
    _stop_computer_use_session(safe_id)


def _user_interrupted_capture() -> Dict[str, Any]:
    return {
        "success": False,
        "error": UiErrorCode.USER_INTERRUPTED,
        "message": (
            "The user pressed Ctrl+Alt+Esc; DCC UI Control remains stopped. "
            "Only resume after explicit user approval with resume_computer_use=true."
        ),
    }


def _native_process_user_interrupted() -> bool:
    if _ComputerUseSession is None:
        return False
    checker = getattr(_ComputerUseSession, "process_user_interrupted", None)
    if not callable(checker):
        return False
    try:
        return bool(checker())
    except Exception:
        return False


def _stop_all_computer_use_sessions() -> None:
    for session_id in list(_COMPUTER_USE_SESSIONS):
        _stop_computer_use_session(session_id)


def request_stop() -> None:
    """Cooperatively cancel active native input before package cleanup waits."""
    _CLEANUP_REQUESTED.set()
    for session_id in list(_COMPUTER_USE_SESSIONS):
        _request_stop_computer_use_session(session_id)


def cleanup() -> None:
    """Release backend-owned input and overlays before package unload."""
    _CLEANUP_REQUESTED.set()
    _stop_all_computer_use_sessions()
    with suppress(Exception):
        atexit.unregister(_stop_all_computer_use_sessions)


atexit.register(_stop_all_computer_use_sessions)


def _is_windows() -> bool:
    return os.name == "nt"


def _powershell_bin() -> Optional[str]:
    return (
        shutil.which("powershell.exe") or shutil.which("pwsh.exe") or shutil.which("powershell") or shutil.which("pwsh")
    )


def _uia_guard_failure(session_id: str) -> Optional[Dict[str, Any]]:
    safe_id = _safe_session_id(session_id)
    if safe_id in _COMPUTER_USE_INTERRUPTED:
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if _native_process_user_interrupted():
        _latch_user_interrupt(safe_id)
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if not _native_desktop_interactive():
        _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
        return {
            "ok": False,
            "error": UiErrorCode.DESKTOP_UNAVAILABLE,
            "message": _DESKTOP_UNAVAILABLE_MESSAGE,
        }
    if safe_id in _COMPUTER_USE_STOPPING:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_UNAVAILABLE,
            "message": "DCC UI Control was stopped while the Windows UIA action was running.",
        }
    entry = _COMPUTER_USE_SESSIONS.get(safe_id)
    if not entry or not hasattr(entry["session"], "status"):
        return None
    try:
        status = _json_object(entry["session"].status())
    except Exception:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_ERROR,
            "message": "The DCC UI Control safety monitor could not verify the active session.",
        }
    if status.get("user_interrupted"):
        _latch_user_interrupt(safe_id)
        return {
            "ok": False,
            "error": UiErrorCode.USER_INTERRUPTED,
            "message": _user_interrupted_capture()["message"],
        }
    if status.get("desktop_interactive") is False:
        _COMPUTER_USE_OBSERVATIONS.pop(safe_id, None)
        return {
            "ok": False,
            "error": UiErrorCode.DESKTOP_UNAVAILABLE,
            "message": _DESKTOP_UNAVAILABLE_MESSAGE,
        }
    if status.get("active") is False:
        return {
            "ok": False,
            "error": UiErrorCode.BACKEND_UNAVAILABLE,
            "message": "DCC UI Control was stopped while the Windows UIA action was running.",
        }
    return None


def _stop_uia_process(process: Any) -> None:
    with suppress(Exception):
        process.terminate()
    try:
        process.communicate(timeout=0.5)
    except Exception:
        with suppress(Exception):
            process.kill()
        with suppress(Exception):
            process.communicate(timeout=0.5)


def _run_uia(payload: Dict[str, Any]) -> Dict[str, Any]:
    if not _is_windows():
        raise RuntimeError("Windows UIA backend is only available on Windows")
    ps = _powershell_bin()
    if not ps:
        raise RuntimeError("PowerShell executable not found for Windows UIA backend")
    payload = dict(payload)
    guard_session_id = str(payload.pop("_session_id", ""))
    timeout = float(os.environ.get("DCC_MCP_APP_UI_UIA_TIMEOUT_SECS", "12"))
    with tempfile.NamedTemporaryFile("w", suffix=".ps1", delete=False, encoding="utf-8") as handle:
        handle.write(_UIA_SCRIPT)
        script_path = handle.name
    try:
        if guard_session_id:
            guarded_failure = _uia_guard_failure(guard_session_id)
            if guarded_failure is not None:
                return guarded_failure
            process = subprocess.Popen(
                [ps, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", script_path],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            input_text: Optional[str] = json.dumps(payload)
            deadline = time.monotonic() + timeout
            while True:
                guarded_failure = _uia_guard_failure(guard_session_id)
                if guarded_failure is not None:
                    _stop_uia_process(process)
                    return guarded_failure
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    _stop_uia_process(process)
                    raise RuntimeError(f"Windows UIA command timed out after {timeout:g} seconds")
                try:
                    stdout, stderr = process.communicate(input=input_text, timeout=min(0.05, remaining))
                    break
                except subprocess.TimeoutExpired:
                    input_text = None
            guarded_failure = _uia_guard_failure(guard_session_id)
            if guarded_failure is not None:
                return guarded_failure
            returncode = process.returncode
        else:
            try:
                completed = subprocess.run(
                    [ps, "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", script_path],
                    input=json.dumps(payload),
                    capture_output=True,
                    text=True,
                    timeout=timeout,
                )
            except subprocess.TimeoutExpired as exc:
                raise RuntimeError(f"Windows UIA command timed out after {exc.timeout:g} seconds") from exc
            stdout, stderr, returncode = completed.stdout, completed.stderr, completed.returncode
    finally:
        with suppress(OSError):
            Path(script_path).unlink()
    if returncode != 0:
        raise RuntimeError((stderr or stdout or "Windows UIA command failed").strip())
    try:
        parsed = json.loads(stdout or "{}")
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"Windows UIA command returned invalid JSON: {exc}") from exc
    return parsed if isinstance(parsed, dict) else {}


def _capture_snapshot(
    session_id: str,
    policy: AppUiPolicy,
    params: Dict[str, Any],
    *,
    bump_revision: bool,
    guard_session_id: Optional[str] = None,
) -> Dict[str, Any]:
    scope = _scope_from_params(params, policy)
    if scope.get("invalid_reason"):
        return {
            "success": False,
            "error": UiErrorCode.INVALID_TARGET,
            "message": str(scope["invalid_reason"]),
        }
    if not _scope_is_explicit(scope):
        return {
            "success": False,
            "error": UiErrorCode.MISSING_WINDOW,
            "message": (
                "Windows UIA backend requires an explicit scoped window title, "
                "process id, or process name; whole-desktop snapshots are disabled."
            ),
        }
    state = _load_state(session_id)
    if bump_revision:
        _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
        state["revision"] = int(state.get("revision") or 0) + 1
    snapshot_id = _snapshot_id(state)
    payload = {
        "mode": "snapshot",
        "scope": scope,
        "max_depth": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_DEPTH", "5")),
        "max_nodes": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_NODES", "250")),
    }
    if guard_session_id:
        payload["_session_id"] = guard_session_id
    try:
        raw = _run_uia(payload)
    except RuntimeError as exc:
        return {
            "success": False,
            "error": "backend_unavailable",
            "message": str(exc),
            "scope": scope,
            "snapshot_id": snapshot_id,
            "_state": state,
        }
    if not raw.get("ok"):
        return {
            "success": False,
            "error": str(raw.get("error") or UiErrorCode.BACKEND_ERROR),
            "message": str(raw.get("message") or "Windows UIA snapshot failed."),
            "scope": scope,
            "snapshot_id": snapshot_id,
            "_state": state,
        }
    root = _node_from_uia_dict(raw["root"], snapshot_id)
    focus_runtime_id = str(raw.get("focus_runtime_id") or "")
    snapshot = UiSnapshot(
        root=root,
        session_id=session_id,
        focus_id=f"uia:{focus_runtime_id}" if focus_runtime_id else None,
        truncated=int(raw.get("node_count") or 0) >= payload["max_nodes"],
        node_count=int(raw.get("node_count") or 1),
        metadata={
            "snapshot_id": snapshot_id,
            "app_ui": {
                "backend": "windows-uia",
                "scope": scope,
                "max_depth": payload["max_depth"],
                "max_nodes": payload["max_nodes"],
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
        "target": raw["root"],
    }


def _computer_use_screenshot(
    session_id: str,
    capture: Dict[str, Any],
    params: Dict[str, Any],
) -> Dict[str, Any]:
    return _SUPPORT._computer_use_screenshot_impl(
        session_id,
        capture,
        params,
        _ComputerUseSession,
        _stop_computer_use_session,
        _latch_user_interrupt,
        _user_interrupted_capture,
    )


@_serialize_session_call
def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_snapshot:
        return skill_error(
            "app_ui snapshot disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    capture = _capture_snapshot(session_id, policy, params, bump_revision=True)
    if not capture.get("success"):
        fallback = None
        if capture.get("error") in {UiErrorCode.BACKEND_ERROR, UiErrorCode.BACKEND_UNAVAILABLE}:
            fallback = _native_fallback_capture(
                session_id,
                policy,
                params,
                capture,
            )
        if fallback is None:
            return _error_from_capture(capture)
        capture = fallback
    native_fallback = (
        capture["snapshot"].get("metadata", {}).get("app_ui", {}).get("backend") == "windows-native-fallback"
    )
    result = skill_success(
        (
            "Captured native DCC UI Control screenshot after Windows UIA was unavailable."
            if native_fallback
            else "Captured Windows UIA app_ui snapshot."
        ),
        prompt=(
            "Inspect the image, perform one scoped app_ui__act with this snapshot_id, then snapshot again."
            if native_fallback
            else "Use app_ui__find to resolve a control, then app_ui__act with the returned snapshot_id."
        ),
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        snapshot=capture["snapshot"],
        policy=policy.to_dict(),
    )
    raw_input_enabled = policy.allow_raw_coordinates or policy.allow_keyboard_shortcuts
    if raw_input_enabled or _scope_is_trusted_native_target(capture["scope"]):
        computer_use = _computer_use_screenshot(session_id, capture, params)
        if not computer_use.get("success"):
            return _error_from_capture(computer_use)
        observation = computer_use["observation"]
        snapshot_app_ui = capture["snapshot"].get("metadata", {}).get("app_ui", {})
        if native_fallback:
            root = capture["snapshot"]["root"]
            root_app_ui = root.setdefault("metadata", {}).setdefault("app_ui", {})
            resolved_target = {
                "process_id": observation.get("process_id"),
                "native_window_handle": observation.get("window_handle"),
                "window_title": observation.get("window_title"),
            }
            root_app_ui.update(resolved_target)
            snapshot_app_ui.update(resolved_target)
            if observation.get("window_title"):
                root["label"] = str(observation["window_title"])
            source_rect = observation.get("source_rect")
            if isinstance(source_rect, list) and len(source_rect) == 4:
                root["bounds"] = {
                    "x": float(source_rect[0]),
                    "y": float(source_rect[1]),
                    "width": float(source_rect[2]),
                    "height": float(source_rect[3]),
                }
        capture["snapshot"]["metadata"]["computer_use"] = observation
        result["context"]["snapshot"] = capture["snapshot"]
        result["context"]["observation"] = observation
        result["context"]["computer_use"] = computer_use["status"]
        result["context"]["control_hint"] = computer_use["status"].get("hint")
        result["context"]["__rich__"] = {
            "kind": "image",
            "data": base64.b64encode(computer_use["image"]).decode("ascii"),
            "mime": computer_use["mime_type"],
            "alt": f"{params.get('app_name') or 'DCC'} UI Control screenshot",
        }
    return result


@_serialize_session_call
def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    policy = _policy_from_params(params)
    if not policy.allow_find:
        return skill_error(
            "app_ui find disabled by policy",
            UiErrorCode.POLICY_DISABLED,
            error_code=UiErrorCode.POLICY_DISABLED,
        )
    capture = _capture_snapshot(session_id, policy, params, bump_revision=True)
    if not capture.get("success"):
        return _error_from_capture(capture)
    matches = _find_controls(capture["snapshot"], params)
    return skill_success(
        f"Found {len(matches)} Windows UIA app_ui control(s).",
        prompt="Use app_ui__act with a returned control id, then app_ui__wait_for.",
        session_id=session_id,
        snapshot_id=capture["snapshot_id"],
        matches=matches,
        count=len(matches),
    )


def _consume_action_observation(session_id: str, state: Dict[str, Any]) -> str:
    """Invalidate every coordinate binding before a UI dispatch can mutate state."""
    _COMPUTER_USE_OBSERVATIONS.pop(session_id, None)
    state["revision"] = int(state.get("revision") or 0) + 1
    state["last_snapshot_id"] = _snapshot_id(state)
    _save_state(state)
    return str(state["last_snapshot_id"])


def _run_native_action(
    session_id: str,
    state: Dict[str, Any],
    policy: AppUiPolicy,
    params: Dict[str, Any],
) -> Dict[str, Any]:
    action = str(params.get("action") or "")
    control_id = str(params.get("control_id") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    current_snapshot_id = str(state.get("last_snapshot_id") or "")
    binding = _COMPUTER_USE_OBSERVATIONS.get(session_id)
    if (
        not requested_snapshot_id
        or requested_snapshot_id != current_snapshot_id
        or not binding
        or binding.get("snapshot_id") != requested_snapshot_id
    ):
        return _stale_observation_result(
            action,
            control_id,
            session_id,
            requested_snapshot_id,
            current_snapshot_id,
            policy,
        )
    entry = _COMPUTER_USE_SESSIONS.get(session_id)
    if not entry:
        return _backend_unavailable(
            "Native DCC UI Control session is not available in this Python process; take a new snapshot in-process."
        )
    request = _native_action_request(params, binding["observation_id"])
    _consume_action_observation(session_id, state)
    try:
        raw = _json_object(entry["session"].act(json.dumps(request)))
    except Exception as exc:
        raw = {"success": False, "error": UiErrorCode.BACKEND_ERROR, "message": str(exc)}
    if not raw.get("success"):
        error = str(raw.get("error") or UiErrorCode.BACKEND_ERROR)
        message = str(raw.get("message") or "Native DCC UI Control action failed.")
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=error,
            message=message,
            metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, error, message)
        if error == UiErrorCode.USER_INTERRUPTED:
            _latch_user_interrupt(session_id)
        return skill_error(message, error, result=result, audit=audit)

    message = f"Completed native DCC UI Control action {action!r}."
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
    ).to_dict()
    audit = _audit_record(action, True, None, session_id, policy, None, None, None, message)
    return skill_success(
        message,
        prompt="Take a new app_ui__snapshot before the next native action.",
        session_id=session_id,
        snapshot_id=state["last_snapshot_id"],
        result=result,
        audit=audit,
    )


@_serialize_session_call
def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    if _native_process_user_interrupted():
        _COMPUTER_USE_INTERRUPTED.add(session_id)
        return _user_interrupted_capture()
    policy = _policy_from_params(params)
    action = str(params.get("action") or "")
    limit_error = _validate_action_limits(params)
    if limit_error is not None:
        return limit_error
    control_id = str(params.get("control_id") or "")
    requested_snapshot_id = str(params.get("snapshot_id") or "")
    state = _load_state(session_id)
    current_snapshot_id = str(state.get("last_snapshot_id") or "")
    native_action = _is_native_action(action, params)
    if requested_snapshot_id and requested_snapshot_id != current_snapshot_id:
        if native_action:
            return _stale_observation_result(
                action,
                control_id,
                session_id,
                requested_snapshot_id,
                current_snapshot_id,
                policy,
            )
        return _stale_result(control_id, session_id, requested_snapshot_id, current_snapshot_id)
    request = UiActionRequest(
        control_id=control_id or None,
        action=action,
        x=params.get("x"),
        y=params.get("y"),
    )
    if not policy.allows_request(request):
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.POLICY_DISABLED,
            message=f"app_ui action {action!r} disabled by policy",
        ).to_dict()
        audit = _audit_record(action, False, None, session_id, policy, None, None, UiErrorCode.POLICY_DISABLED)
        return skill_error(result["message"], UiErrorCode.POLICY_DISABLED, result=result, audit=audit)
    if native_action:
        return _run_native_action(session_id, state, policy, params)

    capture = _capture_snapshot(session_id, policy, params, bump_revision=False)
    if not capture.get("success"):
        return _error_from_capture(capture)
    control = _find_by_id(capture["snapshot"], control_id)
    if not control:
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=UiErrorCode.NOT_FOUND,
            message="control not found in scoped Windows UIA window",
        ).to_dict()
        return skill_error("Control not found in scoped Windows UIA window.", UiErrorCode.NOT_FOUND, result=result)

    if not _scope_is_trusted_native_target(capture["scope"]):
        return skill_error(
            (
                "Mutating Windows UIA actions require an operator-bound DCC process id or window handle "
                "so the visible DCC UI Control session and user interruption monitor target the same window."
            ),
            UiErrorCode.PERMISSION_DENIED,
        )
    computer_use = _computer_use_screenshot(session_id, capture, params)
    if not computer_use.get("success"):
        return _error_from_capture(computer_use)

    payload = {
        "mode": "act",
        "_session_id": session_id,
        "scope": capture["scope"],
        "max_depth": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_DEPTH", "5")),
        "max_nodes": int(os.environ.get("DCC_MCP_APP_UI_UIA_MAX_NODES", "250")),
        "action": {
            "control_id": control_id,
            "action": action,
            "text": params.get("text") or "",
            "checked": bool(params.get("checked")),
        },
    }
    _consume_action_observation(session_id, state)
    try:
        raw = _run_uia(payload)
    except RuntimeError as exc:
        return _backend_unavailable(str(exc))

    before_focus = f"uia:{raw.get('before_focus_runtime_id')}" if raw.get("before_focus_runtime_id") else None
    after_focus = f"uia:{raw.get('after_focus_runtime_id')}" if raw.get("after_focus_runtime_id") else None
    if not raw.get("ok"):
        error = str(raw.get("error") or UiErrorCode.BACKEND_ERROR)
        message = str(raw.get("message") or "Windows UIA action failed.")
        result = UiActionResult(
            success=False,
            control_id=control_id,
            error_code=error,
            message=message,
            before_focus_id=before_focus,
            after_focus_id=after_focus,
            metadata={"snapshot_id": state["last_snapshot_id"], "requires_new_screenshot": True},
        ).to_dict()
        audit = _audit_record(action, False, control, session_id, policy, before_focus, after_focus, error, message)
        return skill_error(message, error, result=result, audit=audit)

    message = str(raw.get("message") or "Windows UIA action completed")
    result = UiActionResult(
        success=True,
        control_id=control_id,
        message=message,
        before_focus_id=before_focus,
        after_focus_id=after_focus,
        metadata={"snapshot_id": state["last_snapshot_id"]},
    ).to_dict()
    audit = _audit_record(action, True, control, session_id, policy, before_focus, after_focus, None, message)
    return skill_success(
        f"Completed Windows UIA action {action!r} on {control_id}.",
        prompt="Use app_ui__wait_for to poll for the expected UI state, then app_ui__snapshot to verify.",
        session_id=session_id,
        snapshot_id=state["last_snapshot_id"],
        result=result,
        audit=audit,
    )


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Stop one visible DCC UI Control session without clearing the Ctrl+Alt+Esc latch."""
    params = dict(params) if params is not None else _read_params()
    session_id = _safe_session_id(params.get("session_id"))
    _bump_session_stop_generation(session_id)
    _mark_session_stopping(session_id, 1)
    try:
        was_active = _request_stop_computer_use_session(session_id)
        with _session_stop_lock(session_id), _session_lock(session_id):
            was_active = was_active or session_id in _COMPUTER_USE_SESSIONS
            stop_status = _stop_computer_use_session(session_id)
            state = _load_state(session_id)
            state["revision"] = int(state.get("revision") or 0) + 1
            state["last_snapshot_id"] = _snapshot_id(state)
            _save_state(state)
            cleanup_pending = bool(stop_status.get("cleanup_pending"))
            if cleanup_pending:
                return skill_error(
                    (
                        "DCC UI Control stop was requested, but input-owner or visual cleanup is pending. "
                        "retry stop shortly."
                    ),
                    UiErrorCode.BACKEND_UNAVAILABLE,
                    session_id=session_id,
                    active=False,
                    was_active=was_active,
                    cleanup_pending=True,
                    user_interrupted=(session_id in _COMPUTER_USE_INTERRUPTED or _native_process_user_interrupted()),
                )
            return skill_success(
                "Stopped DCC UI Control and removed its visible control effects.",
                session_id=session_id,
                active=False,
                was_active=was_active,
                cleanup_pending=False,
                user_interrupted=(session_id in _COMPUTER_USE_INTERRUPTED or _native_process_user_interrupted()),
            )
    finally:
        _mark_session_stopping(session_id, -1)


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
    stop_generation = _session_stop_generation(session_id)

    def interrupted() -> Optional[Dict[str, Any]]:
        if _session_stop_generation(session_id) != stop_generation:
            return skill_error(
                "app_ui wait cancelled because DCC UI Control was stopped.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        guarded_failure = _uia_guard_failure(session_id)
        if guarded_failure is None:
            return None
        error = str(guarded_failure.get("error") or UiErrorCode.BACKEND_UNAVAILABLE)
        return skill_error(
            str(guarded_failure.get("message") or "app_ui wait was interrupted."),
            error,
            session_id=session_id,
            attempts=attempts,
        )

    while True:
        interruption = interrupted()
        if interruption is not None:
            return interruption
        if _CLEANUP_REQUESTED.is_set():
            return skill_error(
                "app_ui wait cancelled because the backend is stopping.",
                UiErrorCode.BACKEND_UNAVAILABLE,
                session_id=session_id,
                attempts=attempts,
            )
        with _session_lock(session_id):
            interruption = interrupted()
            if interruption is not None:
                return interruption
            capture = _capture_snapshot(
                session_id,
                policy,
                params,
                bump_revision=True,
                guard_session_id=session_id,
            )
        attempts += 1
        if not capture.get("success"):
            return _error_from_capture(capture)
        last_snapshot = capture["snapshot"]
        if _condition_matches(last_snapshot, condition):
            elapsed_ms = round((time.monotonic() - start) * 1000.0, 1)
            result = UiWaitResult(
                success=True,
                condition=condition,
                elapsed_ms=elapsed_ms,
                attempts=attempts,
                snapshot=UiSnapshot(
                    root=_node_from_dict(last_snapshot["root"]),
                    session_id=session_id,
                    focus_id=last_snapshot.get("focus_id"),
                    truncated=bool(last_snapshot.get("truncated")),
                    node_count=int(last_snapshot.get("node_count") or 1),
                    metadata=last_snapshot.get("metadata") or {},
                ),
                message="condition became true",
            ).to_dict()
            return skill_success(
                "app_ui wait condition satisfied.",
                session_id=session_id,
                snapshot_id=capture["snapshot_id"],
                result=result,
            )
        if time.monotonic() >= deadline:
            break
        sleep_deadline = min(deadline, time.monotonic() + interval_ms / 1000.0)
        while time.monotonic() < sleep_deadline:
            interruption = interrupted()
            if interruption is not None:
                return interruption
            if _CLEANUP_REQUESTED.wait(min(0.05, max(0.0, sleep_deadline - time.monotonic()))):
                break

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
    return skill_error(
        "app_ui wait_for timed out.",
        UiErrorCode.TIMEOUT,
        session_id=session_id,
        result=result,
        attempts=attempts,
    )


def _dedent_for_tests() -> str:
    return textwrap.dedent(_UIA_SCRIPT).strip()
