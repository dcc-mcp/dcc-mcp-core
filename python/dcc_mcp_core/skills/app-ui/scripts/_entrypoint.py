"""Backend selector for the bundled app_ui skill entry points."""

from __future__ import annotations

import json
import os
from typing import Any
from typing import Callable
from typing import Dict

from dcc_mcp_core.skill import skill_error


def emit(result: Dict[str, Any]) -> None:
    """Emit a JSON tool result."""
    print(json.dumps(result, sort_keys=True))


def _load_backend() -> Any:
    backend = os.environ.get("DCC_MCP_APP_UI_BACKEND", "mock").strip().lower()
    if backend in {"", "mock"}:
        import _backend

        return _backend
    if backend in {"chrome", "chrome-cdp", "cdp"}:
        import _chrome_backend

        return _chrome_backend
    if backend in {"edge", "msedge", "microsoft-edge"}:
        os.environ.setdefault("DCC_MCP_APP_UI_CDP_PRESET", "edge")
        import _chrome_backend

        return _chrome_backend
    if backend in {"agent-browser", "agent_browser", "agentbrowser"}:
        os.environ.setdefault("DCC_MCP_APP_UI_CDP_PRESET", "agent-browser")
        import _chrome_backend

        return _chrome_backend
    if backend in {"windows-uia", "windows_uia", "uia", "win-uia", "win32-uia"}:
        import _windows_uia_backend

        return _windows_uia_backend
    return None


def _call(name: str) -> Dict[str, Any]:
    backend = _load_backend()
    if backend is None:
        selected = os.environ.get("DCC_MCP_APP_UI_BACKEND", "mock")
        return skill_error(
            f"Unsupported app_ui backend {selected!r}.",
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
    func: Callable[[], Dict[str, Any]] = getattr(backend, name)
    return func()


def snapshot_tool() -> Dict[str, Any]:
    """Dispatch app_ui__snapshot to the selected backend."""
    return _call("snapshot_tool")


def find_tool() -> Dict[str, Any]:
    """Dispatch app_ui__find to the selected backend."""
    return _call("find_tool")


def act_tool() -> Dict[str, Any]:
    """Dispatch app_ui__act to the selected backend."""
    return _call("act_tool")


def wait_for_tool() -> Dict[str, Any]:
    """Dispatch app_ui__wait_for to the selected backend."""
    return _call("wait_for_tool")
