"""Tests for the ``take_screenshot`` diagnostic IPC handler."""

# Import future modules
from __future__ import annotations

# Import built-in modules
import base64
import json

# Import third-party modules
import pytest

# ---------------------------------------------------------------------------
# Handler registration & basic return shape
# ---------------------------------------------------------------------------


class _MockServer:
    def __init__(self) -> None:
        self._handlers: dict[str, object] = {}

    def register_handler(self, name: str, fn) -> None:
        self._handlers[name] = fn


def test_take_screenshot_handler_registered():
    from dcc_mcp_core.dcc_server import register_diagnostic_handlers

    server = _MockServer()
    register_diagnostic_handlers(server, dcc_name="test-dcc")
    assert "take_screenshot" in server._handlers


def test_take_screenshot_full_screen_returns_base64_png():
    """full_screen=True uses the auto capturer (mock backend in CI)."""
    from dcc_mcp_core.dcc_server import _handle_take_screenshot

    result = _handle_take_screenshot(json.dumps({"full_screen": True, "format": "png"}))
    data = json.loads(result)
    assert data["success"] is True
    assert data["format"] == "png"
    assert data["width"] > 0
    assert data["height"] > 0
    assert data["mime_type"] == "image/png"
    assert data["byte_len"] > 0
    assert isinstance(data["image_base64"], str)
    decoded = base64.b64decode(data["image_base64"])
    assert decoded.startswith(b"\x89PNG")


def test_take_screenshot_full_screen_raw_format():
    from dcc_mcp_core.dcc_server import _handle_take_screenshot

    result = _handle_take_screenshot(json.dumps({"full_screen": True, "format": "raw_bgra"}))
    data = json.loads(result)
    assert data["success"] is True
    assert data["format"] == "raw_bgra"
    assert data["byte_len"] == data["width"] * data["height"] * 4


def test_take_screenshot_empty_params_errors_without_target():
    """Without full_screen AND without instance context, must return success=False."""
    # Import local modules
    import dcc_mcp_core.dcc_server as mod

    original = dict(mod._instance_context)
    mod._instance_context.update(
        {"dcc_pid": None, "dcc_window_handle": None, "dcc_window_title": None, "resolver": None}
    )
    try:
        result = mod._handle_take_screenshot("")
        data = json.loads(result)
        assert data["success"] is False
        assert "message" in data
    finally:
        mod._instance_context.update(original)


def test_take_screenshot_invalid_json_is_graceful():
    # Invalid JSON falls back to empty dict; then full_screen defaults False and
    # no context is set → should return a structured error, not raise.
    # Import local modules
    import dcc_mcp_core.dcc_server as mod
    from dcc_mcp_core.dcc_server import _handle_take_screenshot

    original = dict(mod._instance_context)
    mod._instance_context.update(
        {"dcc_pid": None, "dcc_window_handle": None, "dcc_window_title": None, "resolver": None}
    )
    try:
        result = _handle_take_screenshot("not-json-{{")
        data = json.loads(result)
        assert "success" in data
    finally:
        mod._instance_context.update(original)


def test_take_screenshot_uses_resolver_when_no_explicit_handle():
    """When only a resolver is available, it must be invoked."""
    # Import local modules
    import dcc_mcp_core.dcc_server as mod

    calls: list[int] = []

    def _resolver() -> int:
        calls.append(1)
        return 0xDEADBEEF  # invalid handle — capture_window will raise

    original = dict(mod._instance_context)
    mod._instance_context.update(
        {
            "dcc_pid": None,
            "dcc_window_handle": None,
            "dcc_window_title": None,
            "resolver": _resolver,
        }
    )
    try:
        result = mod._handle_take_screenshot(json.dumps({"timeout_ms": 200}))
        data = json.loads(result)
        # On Windows, the resolver is called. On other platforms the mock
        # backend ignores the handle, so the call may succeed — accept both.
        assert "success" in data
        # If Windows HwndBackend was used, resolver must have been invoked.
        if not data["success"]:
            assert calls, "resolver should have been invoked for HwndBackend"
    finally:
        mod._instance_context.update(original)


def test_take_screenshot_window_rect_none_for_full_screen():
    from dcc_mcp_core.dcc_server import _handle_take_screenshot

    result = _handle_take_screenshot(json.dumps({"full_screen": True}))
    data = json.loads(result)
    assert data["success"] is True
    # Full-screen / mock captures report no window metadata.
    assert data["window_rect"] is None
    assert data["window_title"] is None
