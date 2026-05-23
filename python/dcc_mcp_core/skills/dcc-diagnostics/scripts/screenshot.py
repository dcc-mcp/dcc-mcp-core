"""Capture a screenshot, preferring the owning DCC adapter's IPC handler.

Protocol (in order of preference):

1. If ``DCC_MCP_IPC_ADDRESS`` is set, connect to the adapter's IPC listener
   and delegate to the ``take_screenshot`` handler so the captured image is
   the DCC's own window rather than the entire desktop.
2. Otherwise fall back to the in-process :class:`dcc_mcp_core.Capturer` using
   the auto backend (DXGI on Windows, X11 on Linux, mock in headless CI).
"""

from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import sys


def _try_ipc_capture(params: dict) -> dict | None:
    """Return ``take_screenshot`` payload when the adapter's IPC is reachable.

    Returns ``None`` if no IPC address is configured or the call fails so the
    caller can fall back to the in-process path.
    """
    addr = os.environ.get("DCC_MCP_IPC_ADDRESS") or os.environ.get("DCC_MCP_OWNER_IPC")
    if not addr:
        return None
    try:
        from dcc_mcp_core import connect_ipc

        channel = connect_ipc(addr, timeout_ms=3000)
        result = channel.call(
            "take_screenshot",
            json.dumps(params).encode("utf-8"),
            timeout_ms=int(params.get("timeout_ms", 10000)),
        )
    except Exception as exc:
        print(
            json.dumps({"debug": f"IPC screenshot failed, falling back: {exc}"}),
            file=sys.stderr,
        )
        return None
    if not result.get("success"):
        return None
    try:
        return json.loads(bytes(result["payload"]).decode("utf-8"))
    except Exception as exc:
        print(json.dumps({"debug": f"IPC payload decode failed: {exc}"}), file=sys.stderr)
        return None


def main(
    format: str = "png",
    scale: float = 1.0,
    jpeg_quality: int = 85,
    window_title: str | None = None,
    save_path: str | None = None,
    timeout_ms: int = 5000,
    full_screen: bool = False,
) -> dict:
    """Capture a screenshot and return the standard skill result dict."""
    # Try IPC path first so we capture the DCC's own window.
    ipc_payload = _try_ipc_capture(
        {
            "format": format,
            "jpeg_quality": jpeg_quality,
            "scale": scale,
            "timeout_ms": timeout_ms,
            "full_screen": full_screen,
            "window_title": window_title,
        }
    )
    if ipc_payload is not None and ipc_payload.get("success"):
        saved_path = None
        if save_path:
            try:
                with Path(save_path).open("wb") as f:
                    f.write(base64.b64decode(ipc_payload["image_base64"]))
                saved_path = save_path
            except OSError as exc:
                saved_path = f"SAVE_FAILED: {exc}"
        return {
            "success": True,
            "message": ipc_payload.get("message", "captured via IPC"),
            "prompt": (
                "Screenshot captured from the DCC's own window. "
                "If you see an error on screen, use dcc_diagnostics__audit_log to check "
                "recent tool history, or dcc_diagnostics__tool_metrics to find failing tools."
            ),
            "context": {
                **{
                    k: ipc_payload.get(k)
                    for k in (
                        "width",
                        "height",
                        "format",
                        "mime_type",
                        "byte_len",
                        "timestamp_ms",
                        "window_rect",
                        "window_title",
                        "image_base64",
                    )
                },
                "saved_path": saved_path,
                "source": "dcc-ipc",
            },
        }

    try:
        from dcc_mcp_core import Capturer
    except ImportError:
        return {"success": False, "message": "dcc_mcp_core not available. Install the package first."}

    try:
        capturer = Capturer.new_auto()
    except Exception:
        # Fall back to mock backend in headless environments
        capturer = Capturer.new_mock(1920, 1080)

    try:
        frame = capturer.capture(
            format=format,
            jpeg_quality=jpeg_quality,
            scale=scale,
            timeout_ms=timeout_ms,
            window_title=window_title,
        )
    except Exception as exc:
        return {"success": False, "message": f"Capture failed: {exc}"}

    # Optionally save to disk
    saved_path = None
    if save_path:
        try:
            with Path(save_path).open("wb") as f:
                f.write(frame.data)
            saved_path = save_path
        except OSError as exc:
            # Non-fatal: still return the base64 data
            saved_path = f"SAVE_FAILED: {exc}"

    b64_data = base64.b64encode(frame.data).decode("ascii")

    return {
        "success": True,
        "message": f"Captured {frame.width}x{frame.height} {frame.format} ({frame.byte_len()} bytes)",
        "prompt": (
            "Screenshot captured. You can view the image data in the 'image_base64' field. "
            "If you see an error on screen, use dcc_diagnostics__audit_log to check recent "
            "tool history, or dcc_diagnostics__tool_metrics to find failing tools."
        ),
        "context": {
            "width": frame.width,
            "height": frame.height,
            "format": frame.format,
            "mime_type": frame.mime_type,
            "byte_len": frame.byte_len(),
            "timestamp_ms": frame.timestamp_ms,
            "dpi_scale": frame.dpi_scale,
            "saved_path": saved_path,
            "image_base64": b64_data,
        },
    }


def _main_cli() -> int:
    """CLI shim kept for subprocess execution and manual debugging."""
    import argparse

    parser = argparse.ArgumentParser(description="Capture a screenshot.")
    parser.add_argument("--format", default="png", choices=["png", "jpeg", "raw_bgra"])
    parser.add_argument("--scale", type=float, default=1.0)
    parser.add_argument("--jpeg-quality", type=int, default=85, dest="jpeg_quality")
    parser.add_argument("--window-title", default=None, dest="window_title")
    parser.add_argument("--save-path", default=None, dest="save_path")
    parser.add_argument("--timeout-ms", type=int, default=5000, dest="timeout_ms")
    parser.add_argument("--full-screen", action="store_true", dest="full_screen")
    args = parser.parse_args()

    result = main(
        format=args.format,
        scale=args.scale,
        jpeg_quality=args.jpeg_quality,
        window_title=args.window_title,
        save_path=args.save_path,
        timeout_ms=args.timeout_ms,
        full_screen=args.full_screen,
    )
    print(json.dumps(result))
    return 0 if result.get("success") else 1


if __name__ == "__main__":
    sys.exit(_main_cli())
