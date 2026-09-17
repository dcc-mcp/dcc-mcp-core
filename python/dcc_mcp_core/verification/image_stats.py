
"""In-band image statistics for verification (issue #2261).

Captures are the primary quality signal in DCC work, but shipping pixels to a
model is expensive and the evaluation's worst defects — white Maya playblasts,
near-black Arnold GPU frames, and display-transform-less linear frames — are
visible from a handful of scalar statistics alone.  This module computes those
statistics deterministically from raw decoded pixels so adapters can flag
bad frames before any model (or VLM) ever sees them.

The module is pure Python and standard-library only: it runs on Python 3.7
(Maya 2022 / Blender 2.83) and inside the ``py37-lite`` wheel.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Tuple

__all__ = [
    "ImageStats",
    "ImageStatsFlags",
    "ImageStatsThresholds",
    "classify_image_stats",
    "compute_image_stats",
    "decode_ppm",
]

# Rec. 709 luma coefficients (the same weights ffmpeg's ``signalstats`` uses
# for ``YAVG``), so our numbers line up with the media skill's ffmpeg path.
_LUMA_R = 0.2126
_LUMA_G = 0.7152
_LUMA_B = 0.0722

_HISTOGRAM_BINS = 16


@dataclass(frozen=True)
class ImageStats:
    """Scalar statistics of a decoded frame, without any pixel data."""

    width: int
    height: int
    channels: int
    mean_luma: float
    stddev_luma: float
    min_luma: float
    max_luma: float
    histogram: Tuple[int, ...]
    uniformity: float

    @property
    def pixel_count(self) -> int:
        """Total number of pixels (``width * height``)."""
        return self.width * self.height

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-safe dict form for skill envelopes."""
        return {
            "width": self.width,
            "height": self.height,
            "channels": self.channels,
            "pixel_count": self.pixel_count,
            "mean_luma": round(self.mean_luma, 6),
            "stddev_luma": round(self.stddev_luma, 6),
            "min_luma": round(self.min_luma, 6),
            "max_luma": round(self.max_luma, 6),
            "histogram": list(self.histogram),
            "uniformity": round(self.uniformity, 6),
        }


@dataclass(frozen=True)
class ImageStatsThresholds:
    """Thresholds used by :func:`classify_image_stats`.

    Values are unitless luma fractions in ``[0, 1]`` (mean/stddev) or a
    fraction of pixels (uniformity).  ``bins`` is the histogram bucket count.
    """

    white_mean: float = 0.98
    black_mean: float = 0.02
    low_contrast_stddev: float = 0.02
    blank_uniformity: float = 0.99
    linear_mean_min: float = 0.25
    linear_mean_max: float = 0.75


@dataclass(frozen=True)
class ImageStatsFlags:
    """Machine-detectable bad-frame classifications derived from stats."""

    is_white: bool
    is_near_black: bool
    is_low_contrast: bool
    is_uniform: bool
    is_blank: bool
    is_linear_candidate: bool

    def to_dict(self) -> Dict[str, Any]:
        """Return a JSON-safe dict form for skill envelopes."""
        return {
            "is_white": self.is_white,
            "is_near_black": self.is_near_black,
            "is_low_contrast": self.is_low_contrast,
            "is_uniform": self.is_uniform,
            "is_blank": self.is_blank,
            "is_linear_candidate": self.is_linear_candidate,
        }


def _luma(r: int, g: int, b: int) -> float:
    return (_LUMA_R * r + _LUMA_G * g + _LUMA_B * b) / 255.0


def compute_image_stats(
    width: int,
    height: int,
    pixels: Any,
    channels: int = 3,
    bins: int = _HISTOGRAM_BINS,
) -> ImageStats:
    """Compute scalar statistics over a raw decoded pixel buffer.

    ``pixels`` is a bytes-like buffer (or any indexable sequence of integers)
    of length ``width * height * channels``.  ``channels`` is ``1`` for
    grayscale and ``3`` for interleaved RGB.  Luma is Rec. 709 and always
    normalised to ``[0, 1]``.

    Raises
    ------
    ValueError
        If dimensions, channel count, or buffer length are inconsistent.

    """
    width = int(width)
    height = int(height)
    channels = int(channels)
    bins = int(bins)
    if width <= 0 or height <= 0:
        raise ValueError("width and height must be positive")
    if channels not in (1, 3):
        raise ValueError("channels must be 1 (gray) or 3 (RGB)")
    if bins <= 0:
        raise ValueError("bins must be positive")
    expected = width * height * channels
    if len(pixels) != expected:  # type: ignore[arg-type]
        raise ValueError(f"pixel buffer length {len(pixels)} does not match {width}x{height}x{channels}")

    histogram = [0] * bins
    total = 0.0
    total_sq = 0.0
    min_luma = 1.0
    max_luma = 0.0
    pixel_count = width * height

    if channels == 1:
        for index in range(pixel_count):
            value = int(pixels[index]) & 0xFF
            lum = value / 255.0
            total += lum
            total_sq += lum * lum
            if lum < min_luma:
                min_luma = lum
            if lum > max_luma:
                max_luma = lum
            bucket = int(lum * bins)
            if bucket >= bins:
                bucket = bins - 1
            histogram[bucket] += 1
    else:
        for index in range(pixel_count):
            base = index * 3
            lum = _luma(int(pixels[base]) & 0xFF, int(pixels[base + 1]) & 0xFF, int(pixels[base + 2]) & 0xFF)
            total += lum
            total_sq += lum * lum
            if lum < min_luma:
                min_luma = lum
            if lum > max_luma:
                max_luma = lum
            bucket = int(lum * bins)
            if bucket >= bins:
                bucket = bins - 1
            histogram[bucket] += 1

    mean = total / pixel_count
    variance = (total_sq / pixel_count) - (mean * mean)
    stddev = variance ** 0.5 if variance > 0.0 else 0.0
    uniformity = max(histogram) / float(pixel_count)
    return ImageStats(
        width=width,
        height=height,
        channels=channels,
        mean_luma=mean,
        stddev_luma=stddev,
        min_luma=min_luma,
        max_luma=max_luma,
        histogram=tuple(histogram),
        uniformity=uniformity,
    )


def classify_image_stats(
    stats: ImageStats,
    thresholds: Optional[ImageStatsThresholds] = None,
) -> ImageStatsFlags:
    """Classify a frame from its :class:`ImageStats`.

    The flags target the evaluation's known-bad captures:

    * ``is_white`` — a washed-out white frame (Maya playblast).
    * ``is_near_black`` — an underexposed black frame (Arnold GPU).
    * ``is_low_contrast`` — a flat frame (missing display transform / blank).
    * ``is_uniform`` — a single flat colour across essentially every pixel.
    * ``is_blank`` — a frame with no usable content.
    * ``is_linear_candidate`` — a low-contrast, mid-luma frame with no
      display transform (linear, not gamma-encoded).  Heuristic: confirm with
      ``view_transform`` metadata when available.
    """
    thresholds = thresholds or ImageStatsThresholds()
    is_white = stats.mean_luma >= thresholds.white_mean
    is_near_black = stats.mean_luma <= thresholds.black_mean
    is_low_contrast = stats.stddev_luma <= thresholds.low_contrast_stddev
    is_uniform = stats.uniformity >= thresholds.blank_uniformity
    is_blank = is_uniform or (is_low_contrast and (is_white or is_near_black))
    is_linear_candidate = (
        is_low_contrast
        and not is_white
        and not is_near_black
        and thresholds.linear_mean_min <= stats.mean_luma <= thresholds.linear_mean_max
    )
    return ImageStatsFlags(
        is_white=is_white,
        is_near_black=is_near_black,
        is_low_contrast=is_low_contrast,
        is_uniform=is_uniform,
        is_blank=is_blank,
        is_linear_candidate=is_linear_candidate,
    )


def decode_ppm(data: bytes) -> Tuple[int, int, int, bytes]:
    """Decode a PPM (P6/P3) or PGM (P5/P2) image into raw pixels.

    Returns ``(width, height, channels, pixels)`` where ``channels`` is ``1``
    for PGM and ``3`` for PPM, and ``pixels`` is interleaved 8-bit bytes.

    This is the standard-library decode path used by tests and by adapters
    that emit portable PPM captures; PNG/JPEG/EXR decoding is handled by the
    verification skill's ffmpeg bridge (see ``skills/verification``).
    """
    if isinstance(data, (bytearray, memoryview)):
        data = bytes(data)
    if not isinstance(data, bytes):
        raise TypeError("decode_ppm expects bytes")
    if data[:2] not in (b"P5", b"P6", b"P2", b"P3"):
        raise ValueError("only P2/P3/P5/P6 (PGM/PPM) inputs are supported")
    magic = data[:2]
    index = 2

    def _next_token() -> str:
        nonlocal index
        while index < len(data) and data[index : index + 1].isspace():
            index += 1
        if index >= len(data):
            raise ValueError("truncated PPM header")
        if data[index : index + 1] == b"#":
            while index < len(data) and data[index : index + 1] != b"\n":
                index += 1
            return _next_token()
        start = index
        while index < len(data) and not data[index : index + 1].isspace():
            index += 1
        return data[start:index].decode("ascii")

    header: List[int] = []
    while len(header) < 3:
        token = _next_token()
        try:
            header.append(int(token))
        except ValueError as exc:
            raise ValueError(f"invalid PPM header token: {token!r}") from exc

    width, height, maxval = header
    if width <= 0 or height <= 0:
        raise ValueError("PPM dimensions must be positive")
    if maxval <= 0 or maxval > 65535:
        raise ValueError("PPM maxval must be in 1..65535")

    binary = magic in (b"P5", b"P6")
    channels = 1 if magic in (b"P5", b"P2") else 3
    pixel_count = width * height * channels

    if binary:
        # Skip exactly one whitespace byte after maxval.
        while index < len(data) and data[index : index + 1].isspace():
            index += 1
        raw = data[index : index + pixel_count * (2 if maxval > 255 else 1)]
        if len(raw) < pixel_count * (2 if maxval > 255 else 1):
            raise ValueError("truncated PPM pixel data")
        if maxval <= 255:
            return width, height, channels, raw
        # 16-bit big-endian: downsample to 8-bit.
        out = bytearray(pixel_count)
        for position in range(pixel_count):
            out[position] = (raw[position * 2] * 255) // maxval
        return width, height, channels, bytes(out)

    # ASCII (P2/P3): gather the remaining integer tokens.
    values: List[int] = []
    while len(values) < pixel_count:
        token = _next_token()
        try:
            values.append(int(token))
        except ValueError as exc:
            raise ValueError(f"invalid PPM pixel token: {token!r}") from exc
    out = bytearray(pixel_count)
    for position in range(pixel_count):
        out[position] = (min(max(values[position], 0), maxval) * 255) // maxval
    return width, height, channels, bytes(out)
