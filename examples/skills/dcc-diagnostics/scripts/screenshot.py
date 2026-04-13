"""Capture a screenshot using dcc_mcp_core.Capturer.

Works on Windows (DXGI), Linux (X11), and falls back to a mock backend
in headless/CI environments. Returns the image as base64-encoded bytes.
"""

from __future__ import annotations

import argparse
import base64
import json
from pathlib import Path
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description="Capture a screenshot.")
    parser.add_argument("--format", default="png", choices=["png", "jpeg", "raw_bgra"])
    parser.add_argument("--scale", type=float, default=1.0)
    parser.add_argument("--jpeg-quality", type=int, default=85, dest="jpeg_quality")
    parser.add_argument("--window-title", default=None, dest="window_title")
    parser.add_argument("--save-path", default=None, dest="save_path")
    parser.add_argument("--timeout-ms", type=int, default=5000, dest="timeout_ms")
    args = parser.parse_args()

    try:
        from dcc_mcp_core import Capturer
    except ImportError:
        print(json.dumps({"success": False, "message": "dcc_mcp_core not available. Install the package first."}))
        sys.exit(1)

    try:
        capturer = Capturer.new_auto()
    except Exception:
        # Fall back to mock backend in headless environments
        capturer = Capturer.new_mock(1920, 1080)

    try:
        frame = capturer.capture(
            format=args.format,
            jpeg_quality=args.jpeg_quality,
            scale=args.scale,
            timeout_ms=args.timeout_ms,
            window_title=args.window_title,
        )
    except Exception as exc:
        print(json.dumps({"success": False, "message": f"Capture failed: {exc}"}))
        sys.exit(1)

    # Optionally save to disk
    saved_path = None
    if args.save_path:
        try:
            with Path(args.save_path).open("wb") as f:
                f.write(frame.data)
            saved_path = args.save_path
        except OSError as exc:
            # Non-fatal: still return the base64 data
            saved_path = f"SAVE_FAILED: {exc}"

    b64_data = base64.b64encode(frame.data).decode("ascii")

    print(
        json.dumps(
            {
                "success": True,
                "message": (f"Captured {frame.width}x{frame.height} {frame.format} ({frame.byte_len()} bytes)"),
                "prompt": (
                    "Screenshot captured. You can view the image data in the 'image_base64' field. "
                    "If you see an error on screen, use dcc_diagnostics__audit_log to check recent "
                    "action history, or dcc_diagnostics__action_metrics to find failing tools."
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
        )
    )


if __name__ == "__main__":
    main()
