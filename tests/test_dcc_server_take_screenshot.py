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


def _skip_if_screen_capture_unavailable(data: dict) -> None:
    if data.get("success") is True:
        return
    message = str(data.get("message", ""))
    if message.startswith("platform error:") or message.startswith("Capture failed:"):
        pytest.skip(f"screen capture unavailable in this session: {message}")


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
    _skip_if_screen_capture_unavailable(data)
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
    _skip_if_screen_capture_unavailable(data)
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
    _skip_if_screen_capture_unavailable(data)
    assert data["success"] is True
    # Full-screen / mock captures report no window metadata.
    assert data["window_rect"] is None
    assert data["window_title"] is None


class _Frame:
    data = b"png"
    width = 2
    height = 2
    format = "png"
    mime_type = "image/png"
    window_rect = (0, 0, 2, 2)
    window_title = "Houdini FX"

    def byte_len(self) -> int:
        return len(self.data)


class _WindowCapturer:
    def __init__(self, *, health: str = "unverified") -> None:
        self.health = health
        self.arguments = None

    def capture_window(self, **kwargs):
        self.arguments = kwargs
        frame = _Frame()
        frame.capture_health = self.health
        return frame

    def backend_name(self) -> str:
        return "GDI PrintWindow"


def test_take_screenshot_exposes_exact_window_and_fallback_metadata(monkeypatch):
    import dcc_mcp_core.dcc_server as mod

    capturer = _WindowCapturer(health="degraded")
    monkeypatch.setattr(mod, "_get_window_capturer", lambda: capturer)

    result = mod._handle_take_screenshot(json.dumps({"window_handle": 0x1234, "process_id": 42}))
    data = json.loads(result)

    assert data["success"] is True
    assert data["window_handle"] == 0x1234
    assert data["window_title"] == "Houdini FX"
    assert data["capture_backend"] == "GDI PrintWindow"
    assert data["fallback_from"] == "Windows.Graphics.Capture"
    assert data["capture_health"] == "degraded"
    assert capturer.arguments["window_handle"] == 0x1234


def test_take_screenshot_rejects_backend_marked_unusable_frame(monkeypatch):
    import dcc_mcp_core.dcc_server as mod

    monkeypatch.setattr(
        mod,
        "_get_window_capturer",
        lambda: _WindowCapturer(health="unusable"),
    )

    data = json.loads(mod._handle_take_screenshot(json.dumps({"window_handle": 0x1234})))

    assert data["success"] is False
    assert data["error_kind"] == "unusable_capture"
    assert data["capture_health"] == "unusable"


def test_take_screenshot_preserves_desktop_unavailable_error(monkeypatch):
    import dcc_mcp_core.dcc_server as mod

    class _UnavailableCapturer:
        def capture(self, **_kwargs):
            raise RuntimeError(
                "desktop_unavailable: backend=DXGI Desktop Duplication; native_error=Access is denied (0x80070005)"
            )

        def backend_name(self) -> str:
            return "DXGI Desktop Duplication"

    monkeypatch.setattr(mod, "_get_full_capturer", _UnavailableCapturer)

    data = json.loads(mod._handle_take_screenshot('{"full_screen": true}'))

    assert data["success"] is False
    assert data["error_kind"] == "desktop_unavailable"
    assert data["capture_backend"] == "DXGI Desktop Duplication"
    assert data["native_error"].endswith("0x80070005)")
