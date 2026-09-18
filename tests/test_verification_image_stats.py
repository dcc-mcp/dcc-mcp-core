"""Tests for the deterministic image-statistics contract (issue #2261)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import ImageStatsFlags
from dcc_mcp_core import classify_image_stats
from dcc_mcp_core import compute_image_stats
from dcc_mcp_core import decode_ppm


def _rgb(width: int, height: int, r: int, g: int, b: int) -> bytes:
    return bytes([r, g, b] * (width * height))


def _gray(width: int, height: int, value: int) -> bytes:
    return bytes([value] * (width * height))


def test_solid_white_is_flagged_as_white_and_blank() -> None:
    stats = compute_image_stats(8, 8, _rgb(8, 8, 255, 255, 255))
    flags = classify_image_stats(stats)

    assert stats.mean_luma == pytest.approx(1.0, abs=1e-6)
    assert stats.stddev_luma == pytest.approx(0.0, abs=1e-6)
    assert stats.uniformity == pytest.approx(1.0, abs=1e-6)
    assert flags.is_white is True
    assert flags.is_uniform is True
    assert flags.is_blank is True
    assert flags.is_near_black is False


def test_solid_black_is_flagged_as_near_black() -> None:
    stats = compute_image_stats(8, 8, _rgb(8, 8, 0, 0, 0))
    flags = classify_image_stats(stats)

    assert stats.mean_luma == pytest.approx(0.0, abs=1e-6)
    assert flags.is_near_black is True
    assert flags.is_blank is True


def test_mid_gray_low_contrast_is_linear_candidate() -> None:
    stats = compute_image_stats(8, 8, _rgb(8, 8, 128, 128, 128))
    flags = classify_image_stats(stats)

    assert stats.mean_luma == pytest.approx(128 / 255, abs=1e-6)
    assert flags.is_low_contrast is True
    assert flags.is_white is False
    assert flags.is_near_black is False
    assert flags.is_linear_candidate is True


def test_checkerboard_has_contrast_and_is_not_blank() -> None:
    pixels = bytearray()
    for y in range(8):
        for x in range(8):
            value = 255 if (x + y) % 2 == 0 else 0
            pixels += bytes([value, value, value])
    stats = compute_image_stats(8, 8, bytes(pixels))
    flags = classify_image_stats(stats)

    assert stats.stddev_luma > 0.2
    assert flags.is_blank is False
    assert flags.is_white is False
    assert flags.is_near_black is False


def test_gray_channel_mode_matches_rgb_for_gray_input() -> None:
    rgb = compute_image_stats(4, 4, _rgb(4, 4, 100, 100, 100))
    gray = compute_image_stats(4, 4, _gray(4, 4, 100), channels=1)

    assert rgb.mean_luma == pytest.approx(gray.mean_luma, abs=1e-6)
    assert rgb.uniformity == pytest.approx(gray.uniformity, abs=1e-6)


def test_histogram_has_requested_bins_and_sums_to_pixel_count() -> None:
    stats = compute_image_stats(8, 8, _rgb(8, 8, 10, 20, 30), bins=16)
    assert len(stats.histogram) == 16
    assert sum(stats.histogram) == 64


def test_rejects_inconsistent_buffer_length() -> None:
    with pytest.raises(ValueError, match="length"):
        compute_image_stats(8, 8, b"\x00" * 10, channels=3)
    with pytest.raises(ValueError, match="positive"):
        compute_image_stats(0, 8, b"", channels=3)
    with pytest.raises(ValueError, match="channels"):
        compute_image_stats(2, 2, b"\x00" * 16, channels=4)


def test_flags_to_dict_is_json_safe() -> None:
    flags = ImageStatsFlags(True, False, False, True, True, False)
    assert flags.to_dict()["is_white"] is True
    assert flags.to_dict()["is_blank"] is True


def test_decode_ppm_p6_binary_roundtrip() -> None:
    # 2x2 P6 PPM, solid red.
    payload = b"P6\n2 2\n255\n" + bytes([255, 0, 0] * 4)
    width, height, channels, pixels = decode_ppm(payload)
    assert (width, height, channels) == (2, 2, 3)
    assert pixels == bytes([255, 0, 0] * 4)


def test_decode_ppm_p5_binary_gray() -> None:
    payload = b"P5\n2 2\n255\n" + bytes([0, 128, 255, 64])
    width, height, channels, pixels = decode_ppm(payload)
    assert (width, height, channels) == (2, 2, 1)
    assert pixels == bytes([0, 128, 255, 64])


def test_decode_ppm_rejects_unsupported_magic() -> None:
    with pytest.raises(ValueError, match="P2/P3/P5/P6"):
        decode_ppm(b"XX\n2 2\n255\n" + bytes(12))
