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
    def new_mock(_width: int, _height: int) -> _Capturer:
        return _Capturer()

    def capture(self, **_kwargs: Any) -> _Frame:
        return _Frame()


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
