"""Tests for deterministic pixel metrics (issue #2261)."""

from __future__ import annotations

import pytest

from dcc_mcp_core import ahash_64
from dcc_mcp_core import delta_e_2000
from dcc_mcp_core import dhash_64
from dcc_mcp_core import edge_density
from dcc_mcp_core import hamming_distance
from dcc_mcp_core import phash_64
from dcc_mcp_core import silhouette_iou
from dcc_mcp_core import sobel_edges
from dcc_mcp_core import ssim


def _rgb(width: int, height: int, r: int, g: int, b: int) -> bytes:
    return bytes([r, g, b] * (width * height))


def _gray_from_rgb(width: int, height: int, pixels: bytes) -> list:
    from dcc_mcp_core.verification.pixel_metrics import _to_gray

    return _to_gray(width, height, pixels, 3)


def test_hashes_are_deterministic() -> None:
    a = _rgb(16, 16, 10, 20, 30)
    assert phash_64(16, 16, a) == phash_64(16, 16, a)
    assert dhash_64(16, 16, a) == dhash_64(16, 16, a)
    assert ahash_64(16, 16, a) == ahash_64(16, 16, a)


def test_distinct_images_yield_distinct_phash() -> None:
    a = _rgb(32, 32, 0, 0, 0)
    b = _rgb(32, 32, 255, 255, 255)
    assert phash_64(32, 32, a) != phash_64(32, 32, b)


def test_hamming_distance_measures_bit_difference() -> None:
    assert hamming_distance(0, 0) == 0
    assert hamming_distance(0, 1) == 1
    assert hamming_distance(0b1010, 0b1111) == 2


def test_silhouette_iou() -> None:
    a = [1, 1, 0, 0]
    b = [1, 0, 0, 0]
    # intersection = 1 pixel, union = 2 pixels -> IoU = 0.5.
    assert silhouette_iou(a, b) == pytest.approx(0.5)
    assert silhouette_iou(a, a) == pytest.approx(1.0)
    assert silhouette_iou([0, 0], [0, 0]) == pytest.approx(1.0)
    with pytest.raises(ValueError, match="equal length"):
        silhouette_iou([1], [1, 0])


def test_ssim_identical_is_one_and_different_is_lower() -> None:
    a = _rgb(16, 16, 30, 60, 90)
    b = _rgb(16, 16, 0, 0, 0)
    gray_a = _gray_from_rgb(16, 16, a)
    gray_b = _gray_from_rgb(16, 16, b)

    assert ssim(gray_a, gray_a, 16, 16) == pytest.approx(1.0, abs=1e-6)
    assert ssim(gray_a, gray_b, 16, 16) < 0.5


def test_delta_e_2000_reference_pair() -> None:
    # Sharma, Wu & Dalal (2005) test pair #1: expected dE00 = 2.0425.
    lab_a = (50.0000, 2.6772, -79.7751)
    lab_b = (50.0000, 0.0000, -82.7485)
    assert delta_e_2000(lab_a, lab_b) == pytest.approx(2.0425, abs=1e-3)
    assert delta_e_2000(lab_a, lab_a) == pytest.approx(0.0, abs=1e-9)


def test_sobel_edges_flat_image_is_zero() -> None:
    flat = _rgb(8, 8, 40, 40, 40)
    magnitudes = sobel_edges(8, 8, flat)
    assert all(value == 0.0 for value in magnitudes)
    assert edge_density(8, 8, flat) == pytest.approx(0.0)


def test_sobel_edges_detect_an_edge() -> None:
    pixels = bytearray()
    for _y in range(8):
        for x in range(8):
            value = 255 if x >= 4 else 0
            pixels += bytes([value, value, value])
    magnitudes = sobel_edges(8, 8, bytes(pixels))
    assert any(value > 0.0 for value in magnitudes)
    assert edge_density(8, 8, bytes(pixels)) > 0.0


def test_rejects_small_images_for_sobel() -> None:
    with pytest.raises(ValueError, match="3x3"):
        sobel_edges(2, 2, _rgb(2, 2, 0, 0, 0))
