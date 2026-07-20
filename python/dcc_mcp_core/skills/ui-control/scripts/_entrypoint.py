"""Backend selector for the bundled ui_control skill entry points."""

from __future__ import annotations

from datetime import datetime
from datetime import timezone
import importlib
import json
import os
from pathlib import Path
import threading
from typing import Any
from typing import Callable
from typing import Dict
from typing import Optional

from dcc_mcp_core.skill import skill_error

_AUDIT_LOCK = threading.Lock()


def emit(result: Dict[str, Any]) -> None:
    """Emit a JSON tool result."""
    print(json.dumps(result, sort_keys=True))


def _import_sibling(name: str) -> Any:
    if __package__:
        return importlib.import_module(f".{name}", __package__)
    return importlib.import_module(name)


def _load_backend() -> Any:
    backend = os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock").strip().lower()
    if backend in {"", "mock"}:
        return _import_sibling("_backend")
    if backend in {"chrome", "chrome-cdp", "cdp"}:
        return _import_sibling("_chrome_backend")
    if backend in {"edge", "msedge", "microsoft-edge"}:
        os.environ.setdefault("DCC_MCP_UI_CONTROL_CDP_PRESET", "edge")
        return _import_sibling("_chrome_backend")
    if backend in {"agent-browser", "agent_browser", "agentbrowser"}:
        os.environ.setdefault("DCC_MCP_UI_CONTROL_CDP_PRESET", "agent-browser")
        return _import_sibling("_chrome_backend")
    if backend in {"windows-uia", "windows_uia", "uia", "win-uia", "win32-uia"}:
        return _import_sibling("_windows_uia_backend")
    return None


def _operation_error(result: Dict[str, Any]) -> Optional[str]:
    error = result.get("error")
    context = result.get("context")
    if not error and isinstance(context, dict):
        audit = context.get("audit")
        nested_result = context.get("result")
        if isinstance(audit, dict):
            error = audit.get("error_code")
        if not error and isinstance(nested_result, dict):
            error = nested_result.get("error_code")
    return str(error) if error else None


def _record_operation(name: str, params: Dict[str, Any], result: Dict[str, Any]) -> None:
    """Append one redacted UI Control event to the Admin log stream."""
    if os.environ.get("DCC_MCP_DISABLE_FILE_LOGGING", "0") == "1":
        return
    try:
        from dcc_mcp_core import get_log_dir

        operation = name[:-5] if name.endswith("_tool") else name
        success = bool(result.get("success", False))
        error = _operation_error(result)
        action = str(params.get("action") or operation)
        condition = params.get("condition")
        condition_kind = condition.get("kind") if isinstance(condition, dict) else None
        detail = [
            f"backend={os.environ.get('DCC_MCP_UI_CONTROL_BACKEND', 'mock')}",
            f"action={action}",
            f"session={params.get('session_id', 'default')}",
        ]
        if condition_kind:
            detail.append(f"condition={condition_kind}")
        if error:
            detail.append(f"error={error}")
        event = {
            "event": "ui_control_operation",
            "tool": f"ui_control__{operation}",
            "dcc_type": os.environ.get("DCC_MCP_UI_CONTROL_DCC_TYPE")
            or os.environ.get("DCC_MCP_DCC_TYPE", "ui-control"),
            "session_id": str(params.get("session_id", "default")),
            "backend": os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock"),
            "action": action,
            "success": success,
            "error": error,
            "message": f"DCC UI Control {operation} {'succeeded' if success else 'rejected'}",
            "detail": " ".join(detail),
        }
        directory = Path(os.environ.get("DCC_MCP_LOG_DIR") or get_log_dir())
        directory.mkdir(parents=True, exist_ok=True)
        timestamp = datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")
        level = "INFO" if success else "WARN"
        line = f"{timestamp} {level} dcc_mcp_core.ui_control.audit: {json.dumps(event, sort_keys=True)}\n"
        with _AUDIT_LOCK, (directory / f"dcc-mcp-ui-control.{os.getpid()}.log").open("a", encoding="utf-8") as stream:
            stream.write(line)
    except Exception:
        # Observability must never block or change a UI operation.
        return


def _call(name: str, params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    call_params = params or {}
    backend = _load_backend()
    if backend is None:
        selected = os.environ.get("DCC_MCP_UI_CONTROL_BACKEND", "mock")
        result = skill_error(
            f"Unsupported ui_control backend {selected!r}.",
            "backend_unavailable",
            backend=selected,
            supported_backends=[
                "mock",
                "chrome",
                "chrome-cdp",
                "cdp",
                "edge",
                "agent-browser",
                "windows-uia",
            ],
        )
        _record_operation(name, call_params, result)
        return result
    func: Callable[[Optional[Dict[str, Any]]], Dict[str, Any]] = getattr(backend, name)
    try:
        result = func(params)
    except Exception as exc:
        _record_operation(name, call_params, {"success": False, "error": type(exc).__name__})
        raise
    _record_operation(name, call_params, result)
    return result


def snapshot_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__snapshot to the selected backend."""
    return _call("snapshot_tool", params)


def find_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__find to the selected backend."""
    return _call("find_tool", params)


def act_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__act to the selected backend."""
    return _call("act_tool", params)


def system_operation_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__system_operation to the selected backend."""
    return _call("system_operation_tool", params)


def wait_for_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__wait_for to the selected backend."""
    return _call("wait_for_tool", params)


def stop_computer_use_tool(params: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """Dispatch ui_control__stop_computer_use to the selected backend."""
    return _call("stop_computer_use_tool", params)
