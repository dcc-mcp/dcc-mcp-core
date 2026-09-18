"""Shared helpers for the bundled verification skill.

Standard-library only so the scripts run in a plain subprocess, in an embedded
DCC Python, or under tests without importing the compiled dcc_mcp_core
extension.  Non-PPM image decode and comparison-sheet compositing shell out to
ffmpeg/ffprobe on PATH (or via ``vx``) and fail loudly when unavailable.
"""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple


class VerificationToolError(Exception):
    """Structured verification tool failure."""

    def __init__(self, message: str, code: str, *, context: Optional[Dict[str, Any]] = None) -> None:
        super().__init__(message)
        self.message = message
        self.code = code
        self.context = context or {}


def success(message: str, **context: Any) -> Dict[str, Any]:
    """Return a dcc-mcp-core-compatible success envelope."""
    return {
        "success": True,
        "message": message,
        "prompt": None,
        "error": None,
        "context": context,
    }


def error(err: VerificationToolError) -> Dict[str, Any]:
    """Return a dcc-mcp-core-compatible error envelope."""
    return {
        "success": False,
        "message": err.message,
        "prompt": "Fix the inputs and run the verification tool again.",
        "error": err.code,
        "context": err.context,
    }


def emit(result: Dict[str, Any]) -> None:
    """Print one JSON result envelope."""
    print(json.dumps(result, sort_keys=True))


def _parse_cli_value(value: str) -> Any:
    lowered = value.strip().lower()
    if lowered == "true":
        return True
    if lowered == "false":
        return False
    if lowered == "null":
        return None
    try:
        if "." in value:
            return float(value)
        return int(value)
    except ValueError:
        return value


def read_params(argv: Optional[List[str]] = None) -> Dict[str, Any]:
    """Read tool params from stdin JSON (or ``--key value`` CLI args)."""
    args = list(sys.argv[1:] if argv is None else argv)
    if args:
        params: Dict[str, Any] = {}
        index = 0
        while index < len(args):
            key = args[index]
            if not key.startswith("--"):
                raise VerificationToolError(f"Unexpected positional argument {key!r}.", "invalid_cli")
            name = key[2:].replace("-", "_")
            if index + 1 >= len(args) or args[index + 1].startswith("--"):
                params[name] = True
            else:
                params[name] = _parse_cli_value(args[index + 1])
                index += 1
            index += 1
        return params
    if sys.stdin is not None and not sys.stdin.isatty():
        raw = sys.stdin.read()
        if raw.strip():
            try:
                data = json.loads(raw)
            except json.JSONDecodeError as exc:
                raise VerificationToolError(
                    "Invalid JSON parameters on stdin.",
                    "invalid_json",
                    context={"detail": str(exc)},
                ) from exc
            if not isinstance(data, dict):
                raise VerificationToolError("Tool parameters must be a JSON object.", "invalid_input")
            return data
    return {}


def existing_file(key: str, value: Any) -> Path:
    """Resolve a required existing file path."""
    if not value:
        raise VerificationToolError(f"{key} is required.", "missing_input", context={"field": key})
    path = Path(str(value))
    if not path.is_file():
        raise VerificationToolError(
            f"{key} does not exist: {path}",
            "missing_input",
            context={"field": key, "path": str(path)},
        )
    return path


def _program_argv(prog: str) -> List[str]:
    """Return an argv prefix that runs ``prog`` (ffmpeg/ffprobe), via vx if needed."""
    bare = shutil.which(prog)
    if bare:
        return [bare]
    vx = shutil.which("vx")
    if vx:
        return [vx, prog]
    return [prog]


def run_ffmpeg(argv: List[str], timeout_secs: int) -> bytes:
    """Run ffmpeg/ffprobe and return stdout, or raise a structured error."""
    command = _program_argv(argv[0]) + argv[1:]
    try:
        result = subprocess.run(command, capture_output=True, timeout=timeout_secs, check=False)
    except FileNotFoundError:
        raise VerificationToolError(
            "ffmpeg/ffprobe is not available; install it or vx.",
            "ffmpeg_not_found",
        ) from None
    if result.returncode != 0:
        stderr = (result.stderr or b"").decode("utf-8", "replace").strip()
        raise VerificationToolError(
            f"ffmpeg failed for {argv[0]}.",
            "ffmpeg_failed",
            context={"command": command, "stderr": stderr[-500:]},
        )
    return result.stdout


def _probe_dimensions(path: Path, timeout_secs: int) -> Tuple[int, int]:
    stdout = run_ffmpeg(
        [
            "ffprobe",
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height",
            "-of",
            "csv=p=0",
            str(path),
        ],
        timeout_secs,
    )
    text = stdout.decode("utf-8", "replace").strip()
    parts = text.replace("\n", ",").split(",")
    if len(parts) < 2:
        raise VerificationToolError(
            "Could not determine image dimensions from ffprobe.",
            "probe_failed",
            context={"path": str(path), "output": text},
        )
    try:
        return int(parts[0]), int(parts[1])
    except ValueError as exc:
        raise VerificationToolError(
            "Could not parse image dimensions from ffprobe.",
            "probe_failed",
            context={"path": str(path), "output": text},
        ) from exc


def _gray_to_rgb(gray: bytes) -> bytes:
    out = bytearray(len(gray) * 3)
    for index, value in enumerate(gray):
        out[index * 3] = value
        out[index * 3 + 1] = value
        out[index * 3 + 2] = value
    return bytes(out)


def read_image_rgb(path: Path, timeout_secs: int = 30) -> Tuple[int, int, bytes]:
    """Decode an image to ``(width, height, interleaved RGB24 bytes)``.

    PPM/PGM decode through the standard library; PNG/JPEG/EXR decode through
    ffmpeg (which yields 3-channel RGB24 for any supported input).
    """
    data = path.read_bytes()
    if data[:2] in (b"P6", b"P3", b"P5", b"P2"):
        from dcc_mcp_core.verification import decode_ppm

        width, height, channels, pixels = decode_ppm(data)
        if channels == 1:
            pixels = _gray_to_rgb(pixels)
        return width, height, pixels

    width, height = _probe_dimensions(path, timeout_secs)
    rgb = run_ffmpeg(
        [
            "ffmpeg",
            "-i",
            str(path),
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ],
        timeout_secs,
    )
    expected = width * height * 3
    if len(rgb) < expected:
        raise VerificationToolError(
            f"ffmpeg decoded {len(rgb)} bytes, expected {expected}.",
            "decode_short",
            context={"path": str(path)},
        )
    return width, height, rgb[:expected]


def load_json_object(path: Path) -> Dict[str, Any]:
    """Load a JSON object from a file (used by validate_scene_vs_spec)."""
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise VerificationToolError(
            f"Invalid JSON in {path}.",
            "invalid_json",
            context={"path": str(path), "detail": str(exc)},
        ) from exc
    if not isinstance(data, dict):
        raise VerificationToolError(
            f"Expected a JSON object in {path}.",
            "invalid_input",
            context={"path": str(path)},
        )
    return data


def run_tool(func: Any, params: Dict[str, Any]) -> Dict[str, Any]:
    """Invoke a tool function and normalise its result/exception to an envelope."""
    try:
        return func(**params)
    except VerificationToolError as exc:
        return error(exc)
