"""Regression coverage for the bundled dcc-diagnostics screenshot skill."""

from __future__ import annotations

import base64
import importlib.util
from pathlib import Path
import sys
from typing import Any

import dcc_mcp_core

SCRIPT = (
    Path(__file__).resolve().parents[1]
    / "python"
    / "dcc_mcp_core"
    / "skills"
    / "dcc-diagnostics"
    / "scripts"
    / "screenshot.py"
)


def _load_screenshot_module() -> Any:
    spec = importlib.util.spec_from_file_location("_test_dcc_diagnostics_screenshot", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    try:
        spec.loader.exec_module(module)
        return module
    finally:
        sys.modules.pop(spec.name, None)


class _Frame:
    data = b"png-bytes"
    width = 32
    height = 16
    format = "png"
    mime_type = "image/png"
    timestamp_ms = 123
    dpi_scale = 1.0

    def byte_len(self) -> int:
        return len(self.data)


class _Capturer:
    @staticmethod
    def new_auto() -> _Capturer:
        return _Capturer()

    @staticmethod
    def new_window_auto() -> _Capturer:
        return _Capturer()

    @staticmethod
    def new_mock(_width: int, _height: int) -> _Capturer:
        return _Capturer()

    def capture(self, **_kwargs: Any) -> _Frame:
        return _Frame()

    def capture_window(self, **_kwargs: Any) -> _Frame:
        return _Frame()

    def backend_name(self) -> str:
        return "Mock"


def test_screenshot_main_returns_dict_for_inprocess_executor(monkeypatch) -> None:
    module = _load_screenshot_module()
    monkeypatch.setattr(module, "_try_ipc_capture", lambda _params: None)
    monkeypatch.setattr(dcc_mcp_core, "Capturer", _Capturer)

    result = module.main()

    assert isinstance(result, dict)
    assert result["success"] is True
    assert result["context"]["image_base64"] == base64.b64encode(_Frame.data).decode("ascii")


def test_screenshot_main_returns_ipc_payload_dict(monkeypatch, tmp_path: Path) -> None:
    module = _load_screenshot_module()
    image_bytes = b"ipc-png"
    encoded = base64.b64encode(image_bytes).decode("ascii")
    monkeypatch.setattr(
        module,
        "_try_ipc_capture",
        lambda _params: {
            "success": True,
            "message": "captured via test IPC",
            "width": 4,
            "height": 3,
            "format": "png",
            "mime_type": "image/png",
            "byte_len": len(image_bytes),
            "timestamp_ms": 321,
            "image_base64": encoded,
        },
    )

    out = tmp_path / "screen.png"
    result = module.main(save_path=str(out))

    assert result["success"] is True
    assert result["context"]["source"] == "dcc-ipc"
    assert result["context"]["saved_path"] == str(out)
    assert out.read_bytes() == image_bytes


def test_screenshot_main_preserves_structured_ipc_capture_failure(monkeypatch) -> None:
    module = _load_screenshot_module()
    monkeypatch.setattr(
        module,
        "_try_ipc_capture",
        lambda _params: {
            "success": False,
            "message": (
                "desktop_unavailable: backend=DXGI Desktop Duplication; native_error=Access is denied (0x80070005)"
            ),
            "error_kind": "desktop_unavailable",
            "capture_backend": "DXGI Desktop Duplication",
            "native_error": "Access is denied (0x80070005)",
        },
    )

    result = module.main()

    assert result["success"] is False
    assert result["error_kind"] == "desktop_unavailable"
    assert result["context"]["capture_backend"] == "DXGI Desktop Duplication"
    assert result["context"]["native_error"].endswith("0x80070005)")


def test_screenshot_main_does_not_mock_desktop_unavailable(monkeypatch) -> None:
    class _UnavailableCapturer(_Capturer):
        @staticmethod
        def new_auto() -> _Capturer:
            raise RuntimeError(
                "desktop_unavailable: backend=DXGI Desktop Duplication; native_error=Access is denied (0x80070005)"
            )

    module = _load_screenshot_module()
    monkeypatch.setattr(module, "_try_ipc_capture", lambda _params: None)
    monkeypatch.setattr(dcc_mcp_core, "Capturer", _UnavailableCapturer)

    result = module.main()

    assert result["success"] is False
    assert result["error_kind"] == "desktop_unavailable"


def test_screenshot_main_forwards_exact_window_target_to_ipc(monkeypatch) -> None:
    module = _load_screenshot_module()
    observed = {}

    def _capture(params):
        observed.update(params)
        return {
            "success": True,
            "image_base64": base64.b64encode(b"png").decode("ascii"),
            "window_handle": 0x1234,
            "capture_backend": "Windows.Graphics.Capture",
            "capture_health": "unverified",
        }

    monkeypatch.setattr(module, "_try_ipc_capture", _capture)

    result = module.main(process_id=42, window_handle=0x1234, window_title="Houdini")

    assert observed["process_id"] == 42
    assert observed["window_handle"] == 0x1234
    assert observed["window_title"] == "Houdini"
    assert result["context"]["window_handle"] == 0x1234
