"""Typed audio-only WAV/OGG transcoding and full-decode receipts."""

from __future__ import annotations

import hashlib
import json
import math
from pathlib import Path
import subprocess
import time

from _media_audio_process import run_audio_command
from _media_common import MediaToolError
from _media_common import assert_nonempty_file
from _media_common import enum_value
from _media_common import existing_file
from _media_common import int_value
from _media_common import output_file
from _media_common import skill_success
from _media_common import vx_command

_FORMATS = {"wav": ("pcm_s16le", ".wav", "pcm_s16le"), "ogg": ("libvorbis", ".ogg", "vorbis")}


def build_audio_command(input_path, output_path, container, audio_codec, audio_quality=None):
    """Validate the narrow codec/container contract before any process starts."""
    if not container or not audio_codec:
        raise MediaToolError("Container and audio codec must be explicit.", "invalid_input")
    source = existing_file("input_path", input_path).resolve()
    container = enum_value("container", container, _FORMATS, "wav")
    codec, suffix, _ = _FORMATS[container]
    audio_codec = enum_value("audio_codec", audio_codec, (codec,), codec)
    output = output_file("output_path", output_path, overwrite=False, suffixes=(suffix,)).resolve()
    if source == output:
        raise MediaToolError("Input and output must differ.", "invalid_output")
    args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-n",
        "-i",
        str(source),
        "-map",
        "0:a:0",
        "-vn",
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-c:a",
        audio_codec,
    ]
    if audio_quality is not None:
        if container != "ogg" or isinstance(audio_quality, bool):
            raise MediaToolError("Audio quality applies only to OGG/Vorbis.", "invalid_input")
        try:
            quality = float(audio_quality)
        except (ValueError, TypeError):
            raise MediaToolError("Audio quality must be numeric.", "invalid_input") from None
        if not math.isfinite(quality) or not -1 <= quality <= 10:
            raise MediaToolError("Audio quality must be finite and between -1 and 10.", "invalid_input")
        args += ["-q:a", str(quality)]
    elif container == "ogg":
        args += ["-q:a", "5"]
    args += ["-f", container, str(output)]
    return vx_command("ffmpeg", args), source, output


def _hash_file(path, deadline):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while True:
            if time.monotonic() >= deadline:
                raise MediaToolError("Audio operation timed out while hashing.", "timeout")
            chunk = stream.read(1024 * 1024)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def _resolve_binaries(execute, deadline):
    """Pin existing vx-managed executables; reject system/global fallback."""
    binaries = {}
    launcher = vx_command("ffmpeg", [])[0]
    command = [launcher, "--output-format", "json", "where", "ffmpeg"]
    try:
        result = json.loads(execute(command)["stdout"])
        encoder = Path(result["path"])
        if result["source"] != "vx" or result["tool"] != "ffmpeg" or not encoder.is_absolute() or not encoder.is_file():
            raise ValueError("Not an existing vx-managed executable")
        encoder = encoder.resolve()
    except (ValueError, TypeError, KeyError) as exc:
        raise MediaToolError(
            "Cannot resolve a managed audio executable; no PATH fallback is allowed.",
            "untrusted_executable",
            context={"tool": "ffmpeg"},
        ) from exc
    # The official vx ffmpeg provider bundles ffprobe in the same bin directory.
    # `where ffmpeg::ffprobe` in vx 0.9.37 can resolve the primary executable,
    # so never treat that query or a PATH lookup as proof of ffprobe provenance.
    probe = encoder.with_name("ffprobe.exe" if encoder.suffix.lower() == ".exe" else "ffprobe").resolve()
    if not probe.is_file() or probe.parent != encoder.parent:
        raise MediaToolError("FFprobe is missing from the managed FFmpeg bundle.", "untrusted_executable")
    for tool, path in (("ffmpeg", encoder), ("ffprobe", probe)):
        version = execute([str(path), "-version"])["stdout"]
        if not version.startswith(tool + " version "):
            raise MediaToolError("Cannot identify the resolved media executable.", "invalid_encoder_version")
        binaries[tool] = {
            "path": str(path),
            "sha256": _hash_file(path, deadline),
            "source": "vx" if tool == "ffmpeg" else "vx_bundle_sibling",
            "version": version.splitlines()[0],
            "resolution_command": command,
        }
    return binaries


def _inspect(path, execute):
    command = vx_command(
        "ffprobe", ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", str(path)]
    )
    try:
        data = json.loads(execute(command)["stdout"])
        streams = data["streams"]
        stream = next(item for item in streams if item.get("codec_type") == "audio")
        rate, channels = int(stream["sample_rate"]), int(stream["channels"])
        if rate <= 0 or channels <= 0:
            raise ValueError("Invalid audio dimensions")
        return {
            "codec": stream["codec_name"],
            "sample_rate": rate,
            "channels": channels,
            "channel_layout": stream.get("channel_layout"),
            "container": data["format"]["format_name"],
            "stream_count": len(streams),
        }
    except (ValueError, KeyError, TypeError, AttributeError, StopIteration) as exc:
        raise MediaToolError("No valid audio metadata returned.", "invalid_audio_metadata") from exc


def _decode(path, metadata, execute):
    # f32le gives a fixed four bytes per sample, independent of source sample format.
    # No rate/channel conversion, trimming, padding, or timestamp correction is used.
    command = vx_command(
        "ffmpeg",
        [
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-xerror",
            "-i",
            str(path),
            "-map",
            "0:a:0",
            "-vn",
            "-sn",
            "-dn",
            "-c:a",
            "pcm_f32le",
            "-f",
            "f32le",
            "pipe:1",
        ],
    )
    result = execute(command, pcm=True)
    frame_bytes = 4 * metadata["channels"]
    if not result["decoded_bytes"] or result["decoded_bytes"] % frame_bytes:
        raise MediaToolError("Full audio decode has invalid PCM length.", "invalid_audio_decode")
    frames = result["decoded_bytes"] // frame_bytes
    return {
        "sample_frames": frames,
        "duration_seconds": frames / metadata["sample_rate"],
        "pcm_format": "f32le",
        "pcm_sha256": result["decoded_sha256"],
    }


def transcode_audio(input_path, output_path, container, audio_codec, audio_quality=None, timeout_secs=300):
    """Encode a fresh audio artifact; accept only verified full-decode results."""
    command, source, output = build_audio_command(input_path, output_path, container, audio_codec, audio_quality)
    if isinstance(timeout_secs, bool):
        raise MediaToolError("Timeout must be an integer.", "invalid_input")
    try:
        timeout = int_value("timeout_secs", timeout_secs, 300, minimum=1, maximum=3600)
    except (ValueError, OverflowError):
        raise MediaToolError("Timeout must be finite.", "invalid_input") from None
    deadline = time.monotonic() + timeout
    binaries = {}
    receipt = {
        "input_path": str(source),
        "output_path": str(output),
        "commands": [],
        "accepted": False,
        "output_status": "not_created",
    }

    def execute(argv, *, pcm=False):
        if len(argv) > 1 and argv[1] in binaries:
            argv = [binaries[argv[1]]["path"], *argv[2:]]
        receipt["commands"].append(argv)
        try:
            return run_audio_command(argv, deadline, pcm=pcm)
        except subprocess.TimeoutExpired as exc:
            raise MediaToolError("Audio operation timed out.", "timeout", context={"command": argv}) from exc

    try:
        receipt["input_sha256"] = _hash_file(source, deadline)
        binaries = _resolve_binaries(execute, deadline)
        receipt["executables"] = binaries
        encoders = execute([binaries["ffmpeg"]["path"], "-hide_banner", "-encoders"])["stdout"]
        available = {
            parts[1]
            for line in encoders.splitlines()
            for parts in [line.split()]
            if len(parts) >= 2 and parts[0].startswith("A")
        }
        if not {audio_codec, "pcm_f32le"}.issubset(available):
            raise MediaToolError(
                "Requested audio encoder is unavailable; no fallback was attempted.",
                "encoder_unavailable",
                context={"audio_codec": audio_codec},
            )
        receipt["input_media"] = _inspect(source, execute)
        receipt["input_decode"] = _decode(source, receipt["input_media"], execute)
        execute(command)
        assert_nonempty_file(output, "output_path")
        receipt["output_sha256"] = _hash_file(output, deadline)
        receipt["output_media"] = _inspect(output, execute)
        actual = receipt["output_media"]
        expected_rate = receipt["input_media"]["sample_rate"]
        expected_channels = receipt["input_media"]["channels"]
        if (
            actual["codec"] != _FORMATS[container][2]
            or actual["container"] != container
            or actual["stream_count"] != 1
            or actual["sample_rate"] != expected_rate
            or actual["channels"] != expected_channels
        ):
            raise MediaToolError("Output does not match the requested audio contract.", "audio_contract_mismatch")
        receipt["output_decode"] = _decode(output, actual, execute)
        if receipt["input_decode"]["sample_frames"] != receipt["output_decode"]["sample_frames"]:
            raise MediaToolError("Full decode sample count changed without resampling.", "sample_count_mismatch")
        receipt["input_sha256_after"] = _hash_file(source, deadline)
        if receipt["input_sha256_after"] != receipt["input_sha256"]:
            raise MediaToolError("Source changed during audio transcoding.", "source_changed")
        if _hash_file(output, deadline) != receipt["output_sha256"]:
            raise MediaToolError("Output changed during verification.", "output_changed")
        for binary in binaries.values():
            if _hash_file(Path(binary["path"]), deadline) != binary["sha256"]:
                raise MediaToolError("Resolved executable changed during operation.", "executable_changed")
        receipt.update(accepted=True, output_status="verified")
        return skill_success("Audio transcoded and fully decoded for verification.", **receipt)
    except MediaToolError as exc:
        receipt["output_status"] = "unverified_partial" if output.exists() else "not_created"
        receipt["output_size_bytes"] = output.stat().st_size if output.is_file() else None
        exc.context.update(receipt)
        raise
