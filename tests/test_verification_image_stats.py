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


@pytest.mark.parametrize(("magic", "channels"), [(b"P5", 1), (b"P6", 3)])
@pytest.mark.parametrize("first", [9, 10, 11, 12, 13, 32, 35])
@pytest.mark.parametrize("maxval", [255, 65535])
def test_decode_ppm_preserves_raster_leading_whitespace_and_hash(magic, channels, first, maxval) -> None:
    """Header whitespace/comments must never consume the first raster sample."""
    raw = bytes([first]) * channels * (2 if maxval == 65535 else 1)
    header = magic + f"\n1 1\n{maxval}\n".encode("ascii")
    assert decode_ppm(header + raw) == (1, 1, channels, bytes([first]) * channels)


@pytest.mark.parametrize(("magic", "channels"), [(b"P5", 1), (b"P6", 3)])
@pytest.mark.parametrize("separator", [b" ", b"\t", b"\r", b"\n", b"\v", b"\f"])
def test_decode_ppm_consumes_exactly_one_raster_separator(magic, channels, separator) -> None:
    # In particular, CR followed by an LF sample is not a two-byte delimiter.
    raw = bytes([10]) * channels
    payload = magic + b"\n1 1\n255" + separator + raw + b"unused trailing bytes"
    assert decode_ppm(payload) == (1, 1, channels, raw)


@pytest.mark.parametrize(("magic", "channels"), [(b"P5", 1), (b"P6", 3)])
@pytest.mark.parametrize(
    ("maxval", "raw", "expected"),
    [
        (1, b"\x00\x01\x01", b"\x00\xff\xff"),
        (15, b"\x00\x07\x0f", b"\x00\x77\xff"),
        (255, b"\x00\x80\xff", b"\x00\x80\xff"),
        (256, b"\x00\x00\x00\x80\x01\x00", b"\x00\x7f\xff"),
        (65535, b"\x01\x00\x01\x01\xff\xff", b"\x00\x01\xff"),
    ],
)
def test_decode_ppm_scales_complete_samples_at_maxval_boundaries(magic, channels, maxval, raw, expected) -> None:
    """Use both big-endian bytes and normalize every valid Maxval to 8-bit."""
    payload = magic + f"\n3 1\n{maxval}\n".encode("ascii") + raw * channels
    assert decode_ppm(payload) == (3, 1, channels, expected * channels)


@pytest.mark.parametrize("magic", [b"P5", b"P6"])
@pytest.mark.parametrize("maxval", [0, -1, 65536])
def test_decode_ppm_rejects_out_of_range_maxval(magic, maxval) -> None:
    with pytest.raises(ValueError, match=r"maxval must be in 1\.\.65535"):
        decode_ppm(magic + f"\n1 1\n{maxval}\n".encode("ascii") + bytes(6))


@pytest.mark.parametrize(("magic", "channels"), [(b"P5", 1), (b"P6", 3)])
@pytest.mark.parametrize(("maxval", "sample_bytes"), [(255, 1), (256, 2), (65535, 2)])
@pytest.mark.parametrize("empty", [False, True])
def test_decode_ppm_rejects_truncated_binary_raster(magic, channels, maxval, sample_bytes, empty) -> None:
    # A non-empty 16-bit raster ends with a high byte missing its low byte.
    size = 0 if empty else 2 * channels * sample_bytes - 1
    payload = magic + f"\n2 1\n{maxval}\n".encode("ascii") + bytes(size)
    with pytest.raises(ValueError, match="truncated PPM pixel data"):
        decode_ppm(payload)


@pytest.mark.parametrize("magic", [b"P5", b"P6"])
def test_decode_ppm_requires_a_raster_separator(magic) -> None:
    with pytest.raises(ValueError, match="whitespace"):
        decode_ppm(magic + b"\n1 1\n255")


@pytest.mark.parametrize(("magic", "channels"), [(b"P5", 1), (b"P6", 3)])
def test_decoded_16bit_white_is_not_classified_as_black(magic, channels) -> None:
    width, height, decoded_channels, pixels = decode_ppm(magic + b"\n1 1\n65535\n" + b"\xff\xff" * channels)
    stats = compute_image_stats(width, height, pixels, channels=decoded_channels)
    flags = classify_image_stats(stats)
    assert stats.mean_luma == pytest.approx(1.0)
    assert flags.is_white is True
    assert flags.is_near_black is False


def test_decoded_pgm_keeps_count_histogram_and_dominant_bin_uniformity() -> None:
    width, height, channels, pixels = decode_ppm(b"P5\n2 1\n1\n\x00\x01")
    stats = compute_image_stats(width, height, pixels, channels=channels)
    assert stats.histogram[0] == stats.histogram[-1] == 1
    assert sum(stats.histogram) == 2
    assert stats.uniformity == 0.5


@pytest.mark.parametrize(("magic", "channels"), [(b"P2", 1), (b"P3", 3)])
def test_decode_ppm_plain_samples_still_scale_with_header_comments(magic, channels) -> None:
    payload = magic + b"\n# header comment\n3 1\n15\n" + b"0 7 15 " * channels
    assert decode_ppm(payload) == (3, 1, channels, b"\x00\x77\xff" * channels)


@pytest.mark.parametrize("container", [bytearray, memoryview])
def test_decode_ppm_accepts_mutable_byte_containers(container) -> None:
    assert decode_ppm(container(b"P5\n1 1\n255\n\x20")) == (1, 1, 1, b"\x20")
