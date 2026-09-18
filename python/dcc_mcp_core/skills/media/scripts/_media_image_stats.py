"""Image statistics helpers for the bundled media skill.

Split out of ``_media_common.py`` to keep both modules below the repository's
1000-line limit (see AGENTS.md, section "File Size Policy"). The dependency direction
is one-way: this module imports the shared primitives from ``_media_common``
and ``_media_common`` never imports it back, so the import graph stays acyclic.
"""

from __future__ import annotations

import contextlib
import math
import os
from pathlib import Path
import tempfile
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple

from _media_common import MediaToolError
from _media_common import existing_file
from _media_common import int_value
from _media_common import probe
from _media_common import run_command
from _media_common import skill_success
from _media_common import vx_command

# A frame whose luma range (max - min) stays within this many 8-bit steps is
# treated as uniform (blank/black/white/gamma-broken) for image_stats.
_UNIFORM_CONTRAST_THRESHOLD = 2


def compute_image_stats_from_gray(data: bytes, width: int, height: int) -> Dict[str, Any]:
    """Compute luma statistics from raw 8-bit grayscale frame bytes.

    Pure-Python and dependency-free so it is unit-testable without ffmpeg or vx.
    Returns mean/min/max/stddev luma (each in ``[0, 1]``), a 16-bin luma
    histogram (each bin also in ``[0, 1]`` and summing to 1), the dominant-bin
    fraction, and a boolean ``uniform`` flag (near-zero contrast) used to
    detect blank, black, white, and gamma-broken frames in-band.
    """
    total = width * height
    if total <= 0:
        raise MediaToolError(
            "Sample frame must be non-empty.",
            "invalid_image",
            context={"width": width, "height": height},
        )
    if len(data) < total:
        raise MediaToolError(
            "Sample frame is shorter than its declared size.",
            "short_frame",
            context={"expected_bytes": total, "received_bytes": len(data)},
        )
    histogram = [0] * 16
    minimum = 255
    maximum = 0
    total_sum = 0
    total_sq = 0
    for raw in data[:total]:
        value = raw if isinstance(raw, int) else ord(raw)
        histogram[value // 16] += 1
        if value < minimum:
            minimum = value
        if value > maximum:
            maximum = value
        total_sum += value
        total_sq += value * value
    mean = total_sum / float(total)
    variance = (total_sq / float(total)) - mean * mean
    stddev = math.sqrt(max(0.0, variance))
    return {
        "mean_luma": round(mean / 255.0, 6),
        "min_luma": round(minimum / 255.0, 6),
        "max_luma": round(maximum / 255.0, 6),
        "stddev_luma": round(stddev / 255.0, 6),
        "histogram": [round(count / float(total), 6) for count in histogram],
        "dominant_bin_fraction": round(max(histogram) / float(total), 6),
        "uniform": (maximum - minimum) <= _UNIFORM_CONTRAST_THRESHOLD,
    }


def build_image_stats_command(input_path: Any, sample_size: Any = 256) -> Tuple[List[str], Path, int]:
    """Build the ffmpeg command that dumps one downscaled gray frame to a temp file."""
    input_file = existing_file("input_path", input_path)
    size = int_value("sample_size", sample_size, 256, minimum=16, maximum=4096)
    descriptor, tmp_name = tempfile.mkstemp(prefix="dcc_media_stats_", suffix=".gray")
    os.close(descriptor)
    command = vx_command(
        "ffmpeg",
        [
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(input_file),
            "-vf",
            f"scale={size}:{size}:flags=area,format=gray",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "gray",
            tmp_name,
        ],
    )
    return command, Path(tmp_name), size


def _image_dimensions(input_file: Path, timeout_secs: Any) -> Tuple[Optional[int], Optional[int]]:
    probe_result = probe(str(input_file), timeout_secs=timeout_secs)
    media = (probe_result.get("context") or {}).get("media") or {}
    video = media.get("video") or {}
    return video.get("width"), video.get("height")


def image_stats(input_path: Any, timeout_secs: Any = 30, sample_size: Any = 256) -> Dict[str, Any]:
    """Compute in-band image statistics for a media file.

    Decodes the first frame (image or video) with vx-managed ffmpeg, downscales
    it to a square ``sample_size`` grayscale sample, and returns mean/min/max/
    stddev luma plus a 16-bin histogram and a uniform-frame flag. Read-only:
    it never modifies the input and never auto-installs vx.
    """
    input_file = existing_file("input_path", input_path)
    command, tmp_path, size = build_image_stats_command(str(input_file), sample_size=sample_size)
    try:
        run_command(command, timeout_secs, allow_auto_install=False)
        if not tmp_path.is_file() or tmp_path.stat().st_size == 0:
            raise MediaToolError(
                "ffmpeg produced no frame data.",
                "output_missing",
                context={"path": str(tmp_path)},
            )
        data = tmp_path.read_bytes()
    finally:
        with contextlib.suppress(OSError):
            tmp_path.unlink()
    stats = compute_image_stats_from_gray(data, size, size)
    width, height = _image_dimensions(input_file, timeout_secs)
    return skill_success(
        "Image statistics computed.",
        input_path=str(input_file),
        width=width,
        height=height,
        sample_size=size,
        stats=stats,
    )
