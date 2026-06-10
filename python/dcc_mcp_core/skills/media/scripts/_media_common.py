"""Shared helpers for the bundled media skill.

The module intentionally uses only the Python standard library. Tool scripts
can run in a plain subprocess, in an embedded DCC Python, or under tests without
importing the compiled dcc_mcp_core extension.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys
from typing import Any
from typing import Callable
from typing import Dict
from typing import Iterable
from typing import List
from typing import Optional
from typing import Sequence
from typing import Set
from typing import Tuple

_SCRIPT_DIR = Path(__file__).resolve().parent
if str(_SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPT_DIR))

import _vx_bootstrap  # noqa: E402

VIDEO_CODECS = ("libx264", "libx265", "mpeg4", "prores_ks", "copy")
SEQUENCE_CODECS = ("libx264", "libx265", "mpeg4")
AUDIO_CODECS = ("aac", "copy", "none")
PIX_FMTS = ("yuv420p", "yuv422p", "yuv444p", "rgb24")
SEQUENCE_PIX_FMTS = ("yuv420p", "yuv422p", "yuv444p")
IMAGE_SUFFIXES = (".png", ".jpg", ".jpeg", ".bmp", ".tif", ".tiff", ".ppm")
VIDEO_SUFFIXES = (".mp4", ".mov", ".mkv", ".avi", ".webm", ".m4v")


class MediaToolError(Exception):
    """Structured media tool failure."""

    def __init__(
        self,
        message: str,
        code: str,
        *,
        prompt: Optional[str] = None,
        possible_solutions: Optional[List[str]] = None,
        context: Optional[Dict[str, Any]] = None,
    ) -> None:
        super().__init__(message)
        self.message = message
        self.code = code
        self.prompt = prompt
        self.possible_solutions = possible_solutions or []
        self.context = context or {}


def skill_success(message: str, **context: Any) -> Dict[str, Any]:
    """Return a dcc-mcp-core-compatible success envelope."""
    return {
        "success": True,
        "message": message,
        "prompt": None,
        "error": None,
        "context": context,
    }


def skill_error(error: MediaToolError) -> Dict[str, Any]:
    """Return a dcc-mcp-core-compatible error envelope."""
    context = dict(error.context)
    if error.possible_solutions:
        context["possible_solutions"] = error.possible_solutions
    return {
        "success": False,
        "message": error.message,
        "prompt": error.prompt or "Fix the media inputs and run the typed media tool again.",
        "error": error.code,
        "context": context,
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


def _read_stdin_json() -> Dict[str, Any]:
    if sys.stdin is None or sys.stdin.isatty():
        return {}
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    try:
        data = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise MediaToolError(
            "Invalid JSON parameters on stdin.",
            "invalid_json",
            context={"detail": str(exc)},
        ) from exc
    if not isinstance(data, dict):
        raise MediaToolError(
            "Tool parameters must be a JSON object.",
            "invalid_input",
            context={"received_type": type(data).__name__},
        )
    return data


def _read_cli_params(argv: Optional[Sequence[str]] = None) -> Dict[str, Any]:
    args = list(sys.argv[1:] if argv is None else argv)
    params: Dict[str, Any] = {}
    index = 0
    while index < len(args):
        key = args[index]
        if not key.startswith("--"):
            raise MediaToolError(
                f"Unexpected positional argument {key!r}.",
                "invalid_cli",
                context={"argv": args},
            )
        name = key[2:].replace("-", "_")
        if index + 1 >= len(args) or args[index + 1].startswith("--"):
            params[name] = True
            index += 1
            continue
        params[name] = _parse_cli_value(args[index + 1])
        index += 2
    return params


def read_params(argv: Optional[Sequence[str]] = None) -> Dict[str, Any]:
    """Read params from stdin JSON, falling back to generic CLI flags."""
    stdin_params = _read_stdin_json()
    if stdin_params:
        return stdin_params
    return _read_cli_params(argv)


def run_tool(func: Callable[..., Dict[str, Any]], params: Dict[str, Any]) -> Dict[str, Any]:
    """Run a tool implementation and convert exceptions to result envelopes."""
    try:
        return func(**params)
    except MediaToolError as exc:
        return skill_error(exc)
    except TypeError as exc:
        return skill_error(
            MediaToolError(
                "Invalid media tool arguments.",
                "invalid_input",
                context={"detail": str(exc), "received_keys": sorted(params.keys())},
            )
        )
    except Exception as exc:  # pragma: no cover - defensive envelope.
        return skill_error(
            MediaToolError(
                "Unexpected media tool failure.",
                "unexpected_error",
                context={"detail": repr(exc)},
            )
        )


def _require_string(name: str, value: Any) -> str:
    if value is None or value == "":
        raise MediaToolError(f"`{name}` is required.", "invalid_input", context={"field": name})
    if not isinstance(value, str):
        raise MediaToolError(
            f"`{name}` must be a string.",
            "invalid_input",
            context={"field": name, "received_type": type(value).__name__},
        )
    if "\x00" in value:
        raise MediaToolError(f"`{name}` contains a NUL byte.", "invalid_path", context={"field": name})
    if value.startswith("-"):
        raise MediaToolError(
            f"`{name}` must not start with '-'.",
            "invalid_path",
            context={"field": name, "value": value},
        )
    return value


def _path(name: str, value: Any) -> Path:
    return Path(_require_string(name, value)).expanduser()


def existing_file(name: str, value: Any) -> Path:
    path = _path(name, value)
    if not path.is_file():
        raise MediaToolError(
            f"`{name}` does not point to an existing file.",
            "input_not_found",
            context={"field": name, "path": str(path)},
        )
    return path


def output_file(
    name: str,
    value: Any,
    *,
    overwrite: bool,
    suffixes: Optional[Tuple[str, ...]] = None,
) -> Path:
    path = _path(name, value)
    if suffixes and path.suffix.lower() not in suffixes:
        raise MediaToolError(
            f"`{name}` must use one of these extensions: {', '.join(suffixes)}.",
            "invalid_output",
            context={"field": name, "path": str(path), "allowed_suffixes": list(suffixes)},
        )
    parent = path.parent
    if parent and not parent.exists():
        raise MediaToolError(
            f"Parent directory for `{name}` does not exist.",
            "output_parent_missing",
            context={"field": name, "parent": str(parent)},
        )
    if path.exists() and not overwrite:
        raise MediaToolError(
            f"Output file already exists: {path}",
            "output_exists",
            prompt="Set overwrite=true only when replacing this media artifact is intended.",
            context={"field": name, "path": str(path)},
        )
    return path


def ensure_output_dir(value: Any, *, create: bool) -> Path:
    path = _path("output_dir", value)
    if path.exists() and not path.is_dir():
        raise MediaToolError(
            "`output_dir` exists but is not a directory.",
            "invalid_output",
            context={"path": str(path)},
        )
    if not path.exists():
        parent = path.parent
        if parent and not parent.exists():
            raise MediaToolError(
                "Parent directory for `output_dir` does not exist.",
                "output_parent_missing",
                context={"parent": str(parent)},
            )
        if not create:
            raise MediaToolError(
                "`output_dir` does not exist.",
                "output_dir_missing",
                context={"path": str(path)},
            )
        path.mkdir()
    return path


def enum_value(name: str, value: Any, allowed: Iterable[str], default: str) -> str:
    selected = default if value in (None, "") else _require_string(name, value)
    allowed_tuple = tuple(allowed)
    if selected not in allowed_tuple:
        raise MediaToolError(
            f"`{name}` must be one of: {', '.join(allowed_tuple)}.",
            "invalid_enum",
            context={"field": name, "value": selected, "allowed": list(allowed_tuple)},
        )
    return selected


def bool_value(name: str, value: Any, default: bool = False) -> bool:
    if value is None:
        return default
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        lowered = value.strip().lower()
        if lowered in {"1", "true", "yes", "on"}:
            return True
        if lowered in {"0", "false", "no", "off"}:
            return False
    raise MediaToolError(
        f"`{name}` must be a boolean.",
        "invalid_input",
        context={"field": name, "value": value},
    )


def number_value(
    name: str,
    value: Any,
    default: Optional[float],
    *,
    minimum: Optional[float] = None,
    maximum: Optional[float] = None,
) -> float:
    if value is None:
        if default is None:
            raise MediaToolError(f"`{name}` is required.", "invalid_input", context={"field": name})
        number = float(default)
    else:
        try:
            number = float(value)
        except (TypeError, ValueError):
            raise MediaToolError(
                f"`{name}` must be numeric.",
                "invalid_input",
                context={"field": name, "value": value},
            ) from None
    if minimum is not None and number < minimum:
        raise MediaToolError(
            f"`{name}` must be >= {minimum}.",
            "invalid_input",
            context={"field": name, "value": number, "minimum": minimum},
        )
    if maximum is not None and number > maximum:
        raise MediaToolError(
            f"`{name}` must be <= {maximum}.",
            "invalid_input",
            context={"field": name, "value": number, "maximum": maximum},
        )
    return number


def int_value(
    name: str,
    value: Any,
    default: int,
    *,
    minimum: int,
    maximum: int,
) -> int:
    number = number_value(name, value, float(default), minimum=float(minimum), maximum=float(maximum))
    if int(number) != number:
        raise MediaToolError(
            f"`{name}` must be an integer.",
            "invalid_input",
            context={"field": name, "value": number},
        )
    return int(number)


def _validate_basename_pattern(name: str, value: Any, suffixes: Tuple[str, ...]) -> str:
    pattern = _require_string(name, value)
    if "/" in pattern or "\\" in pattern:
        raise MediaToolError(
            f"`{name}` must be a basename, not a path.",
            "invalid_path",
            context={"field": name, "value": pattern},
        )
    if "%" not in pattern:
        raise MediaToolError(
            f"`{name}` must include a printf-style frame token such as %04d.",
            "invalid_pattern",
            context={"field": name, "value": pattern},
        )
    if Path(pattern).suffix.lower() not in suffixes:
        raise MediaToolError(
            f"`{name}` must use one of these extensions: {', '.join(suffixes)}.",
            "invalid_pattern",
            context={"field": name, "value": pattern, "allowed_suffixes": list(suffixes)},
        )
    return pattern


def _validate_frame_glob(value: Any) -> str:
    pattern = _require_string("frame_glob", value)
    if "/" in pattern or "\\" in pattern:
        raise MediaToolError(
            "`frame_glob` must be a basename glob, not a path.",
            "invalid_pattern",
            context={"frame_glob": pattern},
        )
    return pattern


def resolve_sequence_source(
    *,
    input_pattern: Optional[str] = None,
    input_dir: Optional[str] = None,
    frame_glob: Optional[str] = None,
) -> Tuple[str, bool]:
    """Return an ffmpeg input pattern and whether it requires glob mode."""
    has_pattern = bool(input_pattern)
    has_dir = bool(input_dir)
    if has_pattern == has_dir:
        raise MediaToolError(
            "Provide exactly one of `input_pattern` or `input_dir`.",
            "invalid_input",
            context={"has_input_pattern": has_pattern, "has_input_dir": has_dir},
        )
    if has_pattern:
        pattern = _path("input_pattern", input_pattern)
        parent = pattern.parent
        if parent and not parent.exists():
            raise MediaToolError(
                "Parent directory for `input_pattern` does not exist.",
                "input_not_found",
                context={"parent": str(parent), "input_pattern": str(pattern)},
            )
        return str(pattern), any(ch in str(pattern) for ch in "*?[")

    directory = _path("input_dir", input_dir)
    if not directory.is_dir():
        raise MediaToolError(
            "`input_dir` does not point to an existing directory.",
            "input_not_found",
            context={"input_dir": str(directory)},
        )
    glob_pattern = _validate_frame_glob(frame_glob or "*.png")
    if not any(directory.glob(glob_pattern)):
        raise MediaToolError(
            "No frames matched `input_dir` plus `frame_glob`.",
            "input_not_found",
            context={"input_dir": str(directory), "frame_glob": glob_pattern},
        )
    return str(directory / glob_pattern), True


def vx_command(tool: str, args: Sequence[str]) -> List[str]:
    """Build a fixed vx-managed media command."""
    if tool not in {"ffmpeg", "ffprobe"}:
        raise MediaToolError(
            "Internal media tool attempted to use an unsupported vx provider.",
            "invalid_tool",
            context={"tool": tool},
        )
    vx_bin = os.environ.get("DCC_MCP_MEDIA_VX_BIN", "vx")
    return [vx_bin, tool, *list(args)]


def _auto_install_vx_enabled() -> bool:
    return _vx_bootstrap.auto_install_vx_enabled()


def _is_default_vx_command(command: Sequence[str]) -> bool:
    return _vx_bootstrap.is_default_vx_command(tuple(command))


def _download_and_install_vx() -> str:
    return _vx_bootstrap.download_and_install_vx(MediaToolError)


def _bootstrap_vx_or_error(original_error: Exception, command: Sequence[str]) -> str:
    if not _auto_install_vx_enabled():
        raise MediaToolError(
            "vx is not available on PATH and automatic vx bootstrap is disabled.",
            "vx_not_found",
            prompt="Install vx, set DCC_MCP_MEDIA_VX_BIN, or unset DCC_MCP_MEDIA_AUTO_INSTALL_VX.",
            possible_solutions=[
                "Install vx and ensure `vx --version` works.",
                "Set DCC_MCP_MEDIA_VX_BIN to the absolute path of the vx executable.",
                "Remove DCC_MCP_MEDIA_AUTO_INSTALL_VX=0 to allow the media skill to download vx.",
            ],
            context={"detail": str(original_error), "command": list(command)},
        ) from original_error
    try:
        return _download_and_install_vx()
    except MediaToolError:
        raise
    except Exception as exc:
        raise MediaToolError(
            "Automatic vx bootstrap failed.",
            "vx_bootstrap_failed",
            prompt="Install vx manually or set DCC_MCP_MEDIA_VX_BIN to a known executable.",
            context={"detail": repr(exc), "command": list(command)},
        ) from exc


def _format_number(number: float) -> str:
    if int(number) == number:
        return str(int(number))
    return str(number)


def _video_quality_args(codec: str, quality: Any, default: int = 18) -> List[str]:
    value = int_value("quality", quality, default, minimum=0, maximum=51)
    if codec in {"mpeg4", "prores_ks"}:
        q_value = min(31, max(1, value or 1))
        return ["-q:v", str(q_value)]
    return ["-crf", str(value)]


def build_probe_command(input_path: Any) -> List[str]:
    input_file = existing_file("input_path", input_path)
    return vx_command(
        "ffprobe",
        [
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            str(input_file),
        ],
    )


def build_sequence_to_mp4_command(
    *,
    output_path: Any,
    input_pattern: Optional[str] = None,
    input_dir: Optional[str] = None,
    frame_glob: Optional[str] = None,
    framerate: Any = 24,
    codec: Any = None,
    pix_fmt: Any = None,
    quality: Any = 18,
    overwrite: Any = False,
) -> Tuple[List[str], Path, str]:
    overwrite_bool = bool_value("overwrite", overwrite)
    output = output_file("output_path", output_path, overwrite=overwrite_bool, suffixes=(".mp4",))
    source, is_glob = resolve_sequence_source(
        input_pattern=input_pattern,
        input_dir=input_dir,
        frame_glob=frame_glob,
    )
    fps = number_value("framerate", framerate, 24, minimum=0.001, maximum=240)
    selected_codec = enum_value("codec", codec, SEQUENCE_CODECS, "mpeg4")
    selected_pix_fmt = enum_value("pix_fmt", pix_fmt, SEQUENCE_PIX_FMTS, "yuv420p")
    args = ["-hide_banner", "-loglevel", "error", "-y" if overwrite_bool else "-n"]
    args += ["-framerate", _format_number(fps)]
    if is_glob:
        args += ["-pattern_type", "glob"]
    args += ["-i", source, "-c:v", selected_codec, "-pix_fmt", selected_pix_fmt]
    args += _video_quality_args(selected_codec, quality)
    args += ["-movflags", "+faststart", str(output)]
    return vx_command("ffmpeg", args), output, source


def build_transcode_command(
    *,
    input_path: Any,
    output_path: Any,
    video_codec: Any = None,
    audio_codec: Any = None,
    pix_fmt: Any = None,
    quality: Any = 18,
    overwrite: Any = False,
) -> Tuple[List[str], Path]:
    input_file = existing_file("input_path", input_path)
    overwrite_bool = bool_value("overwrite", overwrite)
    output = output_file("output_path", output_path, overwrite=overwrite_bool, suffixes=VIDEO_SUFFIXES)
    selected_video = enum_value("video_codec", video_codec, VIDEO_CODECS, "mpeg4")
    selected_audio = enum_value("audio_codec", audio_codec, AUDIO_CODECS, "aac")
    selected_pix_fmt = enum_value("pix_fmt", pix_fmt, PIX_FMTS, "yuv420p")
    args = ["-hide_banner", "-loglevel", "error", "-y" if overwrite_bool else "-n"]
    args += ["-i", str(input_file), "-c:v", selected_video]
    if selected_video != "copy":
        args += ["-pix_fmt", selected_pix_fmt]
        args += _video_quality_args(selected_video, quality)
    if selected_audio == "none":
        args += ["-an"]
    else:
        args += ["-c:a", selected_audio]
    args.append(str(output))
    return vx_command("ffmpeg", args), output


def build_extract_frames_command(
    *,
    input_path: Any,
    output_dir: Any,
    frame_pattern: Any = "frame_%04d.png",
    fps: Any = None,
    overwrite: Any = False,
    create_output_dir: Any = True,
) -> Tuple[List[str], Path, str]:
    input_file = existing_file("input_path", input_path)
    overwrite_bool = bool_value("overwrite", overwrite)
    create_dir = bool_value("create_output_dir", create_output_dir, True)
    out_dir = ensure_output_dir(output_dir, create=create_dir)
    pattern = _validate_basename_pattern("frame_pattern", frame_pattern, IMAGE_SUFFIXES)
    existing_outputs = _matching_outputs(out_dir, pattern)
    if existing_outputs and not overwrite_bool:
        raise MediaToolError(
            f"Output frames already exist for pattern: {pattern}",
            "output_exists",
            prompt="Set overwrite=true only when replacing extracted frames is intended.",
            context={"sample": str(sorted(existing_outputs)[0]), "frame_count": len(existing_outputs)},
        )
    output_pattern = out_dir / pattern
    args = ["-hide_banner", "-loglevel", "error", "-y" if overwrite_bool else "-n"]
    args += ["-i", str(input_file)]
    if fps is not None:
        fps_value = number_value("fps", fps, None, minimum=0.001, maximum=240)
        args += ["-vf", f"fps={_format_number(fps_value)}"]
    args.append(str(output_pattern))
    return vx_command("ffmpeg", args), out_dir, pattern


def build_thumbnail_command(
    *,
    input_path: Any,
    output_path: Any,
    time_seconds: Any = 0,
    width: Any = None,
    quality: Any = 2,
    overwrite: Any = False,
) -> Tuple[List[str], Path]:
    input_file = existing_file("input_path", input_path)
    overwrite_bool = bool_value("overwrite", overwrite)
    output = output_file("output_path", output_path, overwrite=overwrite_bool, suffixes=IMAGE_SUFFIXES)
    seconds = number_value("time_seconds", time_seconds, 0, minimum=0, maximum=None)
    q_value = int_value("quality", quality, 2, minimum=2, maximum=31)
    args = ["-hide_banner", "-loglevel", "error", "-y" if overwrite_bool else "-n"]
    args += ["-ss", _format_number(seconds), "-i", str(input_file), "-frames:v", "1"]
    if width is not None:
        width_value = int_value("width", width, 0, minimum=16, maximum=8192)
        args += ["-vf", f"scale={width_value}:-1"]
    args += ["-q:v", str(q_value), str(output)]
    return vx_command("ffmpeg", args), output


def run_command(command: Sequence[str], timeout_secs: Any, *, allow_auto_install: bool = True) -> str:
    timeout = number_value("timeout_secs", timeout_secs, 30, minimum=1, maximum=3600)
    command_list = list(command)
    try:
        result = subprocess.run(
            command_list,
            capture_output=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
        )
    except FileNotFoundError as exc:
        if _is_default_vx_command(command_list):
            if not allow_auto_install:
                raise MediaToolError(
                    "vx is not available on PATH; this read-only media command will not install it automatically.",
                    "vx_not_found",
                    prompt=(
                        "Install vx manually, set DCC_MCP_MEDIA_VX_BIN, or run a non-read-only media tool "
                        "when installation is intended."
                    ),
                    possible_solutions=[
                        "Install vx and ensure `vx --version` works.",
                        "Set DCC_MCP_MEDIA_VX_BIN to the absolute path of the vx executable.",
                        "Use a non-read-only media tool only when automatic vx installation is acceptable.",
                    ],
                    context={"detail": str(exc), "command": command_list, "allow_auto_install": False},
                ) from exc
            vx_bin = _bootstrap_vx_or_error(exc, command_list)
            command_list[0] = vx_bin
            try:
                result = subprocess.run(
                    command_list,
                    capture_output=True,
                    encoding="utf-8",
                    errors="replace",
                    timeout=timeout,
                )
            except FileNotFoundError as retry_exc:
                raise MediaToolError(
                    "vx bootstrap completed but the downloaded vx binary could not be executed.",
                    "vx_bootstrap_failed",
                    context={"detail": str(retry_exc), "command": command_list},
                ) from retry_exc
            except subprocess.TimeoutExpired as retry_exc:
                raise MediaToolError(
                    "Media command timed out.",
                    "timeout",
                    context={
                        "timeout_secs": timeout,
                        "command": command_list,
                        "stdout": retry_exc.stdout,
                        "stderr": retry_exc.stderr,
                    },
                ) from retry_exc
        else:
            raise MediaToolError(
                "vx is not available on PATH, so the media tool cannot provision FFmpeg.",
                "vx_not_found",
                prompt="Install vx or set DCC_MCP_MEDIA_VX_BIN to the vx executable path, then retry.",
                possible_solutions=[
                    "Install vx and ensure `vx --version` works.",
                    "Set DCC_MCP_MEDIA_VX_BIN to the absolute path of the vx executable.",
                ],
                context={"detail": str(exc), "command": command_list},
            ) from exc
    except subprocess.TimeoutExpired as exc:
        raise MediaToolError(
            "Media command timed out.",
            "timeout",
            context={"timeout_secs": timeout, "command": command_list, "stdout": exc.stdout, "stderr": exc.stderr},
        ) from exc
    if result.returncode != 0:
        stderr_tail = (result.stderr or result.stdout or "").strip()[-1200:]
        raise MediaToolError(
            "Media command failed.",
            "command_failed",
            context={"returncode": result.returncode, "stderr": stderr_tail, "command": command_list},
        )
    return result.stdout or ""


def assert_nonempty_file(path: Path, field: str) -> None:
    if not path.is_file() or path.stat().st_size == 0:
        raise MediaToolError(
            "Media command completed but did not create a non-empty output file.",
            "output_missing",
            context={"field": field, "path": str(path)},
        )


def parse_probe(stdout: str) -> Dict[str, Any]:
    try:
        data = json.loads(stdout)
    except json.JSONDecodeError as exc:
        raise MediaToolError(
            "ffprobe returned invalid JSON.",
            "invalid_probe_output",
            context={"detail": str(exc), "stdout": stdout[:1200]},
        ) from exc
    streams = data.get("streams") or []
    fmt = data.get("format") or {}
    video = next((stream for stream in streams if stream.get("codec_type") == "video"), None)
    audio = next((stream for stream in streams if stream.get("codec_type") == "audio"), None)
    summary: Dict[str, Any] = {
        "filename": fmt.get("filename"),
        "format_name": fmt.get("format_name"),
        "duration": _safe_float(fmt.get("duration")),
        "size_bytes": _safe_int(fmt.get("size")),
        "bit_rate": _safe_int(fmt.get("bit_rate")),
        "streams": streams,
    }
    if video:
        summary["video"] = {
            "codec": video.get("codec_name"),
            "width": video.get("width"),
            "height": video.get("height"),
            "fps": video.get("r_frame_rate"),
            "pix_fmt": video.get("pix_fmt"),
        }
    if audio:
        summary["audio"] = {
            "codec": audio.get("codec_name"),
            "sample_rate": audio.get("sample_rate"),
            "channels": audio.get("channels"),
        }
    return summary


def _safe_float(value: Any) -> Optional[float]:
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def _safe_int(value: Any) -> Optional[int]:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def probe(input_path: Any, timeout_secs: Any = 30) -> Dict[str, Any]:
    command = build_probe_command(input_path)
    stdout = run_command(command, timeout_secs, allow_auto_install=False)
    info = parse_probe(stdout)
    return skill_success(
        "Media metadata probed.",
        input_path=str(existing_file("input_path", input_path)),
        command=command,
        media=info,
    )


def sequence_to_mp4(
    output_path: Any,
    input_pattern: Optional[str] = None,
    input_dir: Optional[str] = None,
    frame_glob: Optional[str] = None,
    framerate: Any = 24,
    codec: Any = None,
    pix_fmt: Any = None,
    quality: Any = 18,
    overwrite: Any = False,
    timeout_secs: Any = 300,
) -> Dict[str, Any]:
    command, output, source = build_sequence_to_mp4_command(
        output_path=output_path,
        input_pattern=input_pattern,
        input_dir=input_dir,
        frame_glob=frame_glob,
        framerate=framerate,
        codec=codec,
        pix_fmt=pix_fmt,
        quality=quality,
        overwrite=overwrite,
    )
    run_command(command, timeout_secs)
    assert_nonempty_file(output, "output_path")
    return skill_success(
        "Image sequence converted to MP4.",
        input=source,
        output_path=str(output),
        command=command,
    )


def transcode(
    input_path: Any,
    output_path: Any,
    video_codec: Any = None,
    audio_codec: Any = None,
    pix_fmt: Any = None,
    quality: Any = 18,
    overwrite: Any = False,
    timeout_secs: Any = 300,
) -> Dict[str, Any]:
    command, output = build_transcode_command(
        input_path=input_path,
        output_path=output_path,
        video_codec=video_codec,
        audio_codec=audio_codec,
        pix_fmt=pix_fmt,
        quality=quality,
        overwrite=overwrite,
    )
    run_command(command, timeout_secs)
    assert_nonempty_file(output, "output_path")
    return skill_success(
        "Media file transcoded.",
        input_path=str(existing_file("input_path", input_path)),
        output_path=str(output),
        command=command,
    )


def extract_frames(
    input_path: Any,
    output_dir: Any,
    frame_pattern: Any = "frame_%04d.png",
    fps: Any = None,
    overwrite: Any = False,
    create_output_dir: Any = True,
    timeout_secs: Any = 300,
) -> Dict[str, Any]:
    command, out_dir, pattern = build_extract_frames_command(
        input_path=input_path,
        output_dir=output_dir,
        frame_pattern=frame_pattern,
        fps=fps,
        overwrite=overwrite,
        create_output_dir=create_output_dir,
    )
    before = _matching_outputs(out_dir, pattern)
    run_command(command, timeout_secs)
    after = _matching_outputs(out_dir, pattern)
    created = sorted(str(path) for path in after - before)
    if not created and not after:
        raise MediaToolError(
            "Media command completed but no extracted frames were found.",
            "output_missing",
            context={"output_dir": str(out_dir), "frame_pattern": pattern},
        )
    return skill_success(
        "Frames extracted from media file.",
        input_path=str(existing_file("input_path", input_path)),
        output_dir=str(out_dir),
        frame_pattern=pattern,
        created_frames=created,
        frame_count=len(after),
        command=command,
    )


def thumbnail(
    input_path: Any,
    output_path: Any,
    time_seconds: Any = 0,
    width: Any = None,
    quality: Any = 2,
    overwrite: Any = False,
    timeout_secs: Any = 30,
) -> Dict[str, Any]:
    command, output = build_thumbnail_command(
        input_path=input_path,
        output_path=output_path,
        time_seconds=time_seconds,
        width=width,
        quality=quality,
        overwrite=overwrite,
    )
    run_command(command, timeout_secs)
    assert_nonempty_file(output, "output_path")
    return skill_success(
        "Thumbnail extracted from media file.",
        input_path=str(existing_file("input_path", input_path)),
        output_path=str(output),
        command=command,
    )


def _matching_outputs(directory: Path, pattern: str) -> Set[Path]:
    glob_pattern = re.sub(r"%0?\d*d", "*", pattern)
    return {path for path in directory.glob(glob_pattern) if path.is_file()}
