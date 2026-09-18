"""Deterministic pixel metrics for capture comparison (issue #2261).

Zero-token hard/soft gates that run before any VLM is consulted: perceptual
hashes (pHash/dHash/aHash) for duplicate detection, silhouette IoU, structural
similarity (SSIM), CIEDE2000 colour distance, and Sobel edge maps.  All are
pure Python and deterministic — the same pixels always yield the same number.

They are intentionally a first-class Python implementation of the metrics the
issue proposes for the Rust core; the numeric contracts here are the reference
the Rust port must reproduce.
"""

from __future__ import annotations

import math
from typing import Any
from typing import List
from typing import Sequence

__all__ = [
    "ahash_64",
    "delta_e_2000",
    "dhash_64",
    "edge_density",
    "hamming_distance",
    "phash_64",
    "silhouette_iou",
    "sobel_edges",
    "ssim",
]

_LUMA_R = 0.2126
_LUMA_G = 0.7152
_LUMA_B = 0.0722


def _to_gray(width: int, height: int, pixels: Any, channels: int = 3) -> List[float]:
    """Decode an interleaved buffer into per-pixel Rec. 709 luma (0..1)."""
    width = int(width)
    height = int(height)
    channels = int(channels)
    if width <= 0 or height <= 0:
        raise ValueError("width and height must be positive")
    if channels not in (1, 3):
        raise ValueError("channels must be 1 or 3")
    expected = width * height * channels
    if len(pixels) != expected:  # type: ignore[arg-type]
        raise ValueError(f"pixel buffer length {len(pixels)} does not match {width}x{height}x{channels}")
    count = width * height
    gray: List[float] = [0.0] * count
    if channels == 1:
        for index in range(count):
            gray[index] = (int(pixels[index]) & 0xFF) / 255.0
        return gray
    for index in range(count):
        base = index * 3
        gray[index] = (
            _LUMA_R * (int(pixels[base]) & 0xFF)
            + _LUMA_G * (int(pixels[base + 1]) & 0xFF)
            + _LUMA_B * (int(pixels[base + 2]) & 0xFF)
        ) / 255.0
    return gray


def _resize_bilinear(gray: List[float], width: int, height: int, new_width: int, new_height: int) -> List[float]:
    """Bilinear-resample grayscale luma to ``new_width x new_height``."""
    if width == new_width and height == new_height:
        return list(gray)
    out: List[float] = [0.0] * (new_width * new_height)
    x_ratio = width / new_width if new_width > 1 else 1.0
    y_ratio = height / new_height if new_height > 1 else 1.0
    for y in range(new_height):
        src_y = y * y_ratio
        y0 = int(src_y)
        y1 = min(y0 + 1, height - 1)
        fy = src_y - y0
        for x in range(new_width):
            src_x = x * x_ratio
            x0 = int(src_x)
            x1 = min(x0 + 1, width - 1)
            fx = src_x - x0
            top = gray[y0 * width + x0] * (1 - fx) + gray[y0 * width + x1] * fx
            bottom = gray[y1 * width + x0] * (1 - fx) + gray[y1 * width + x1] * fx
            out[y * new_width + x] = top * (1 - fy) + bottom * fy
    return out


def _mean(values: List[float]) -> float:
    if not values:
        return 0.0
    return sum(values) / len(values)


def _median(values: List[float]) -> float:
    ordered = sorted(values)
    length = len(ordered)
    if length == 0:
        return 0.0
    if length % 2:
        return ordered[length // 2]
    return (ordered[length // 2 - 1] + ordered[length // 2]) / 2.0


def _pack_bits(bits: List[int]) -> int:
    value = 0
    for bit in bits:
        value = (value << 1) | (1 if bit else 0)
    return value


def ahash_64(width: int, height: int, pixels: Any, channels: int = 3) -> int:
    """Average hash: mean-thresholded 8x8 downscale (64-bit int)."""
    gray = _resize_bilinear(_to_gray(width, height, pixels, channels), width, height, 8, 8)
    mean = _mean(gray)
    bits = [1 if value > mean else 0 for value in gray]
    return _pack_bits(bits)


def dhash_64(width: int, height: int, pixels: Any, channels: int = 3) -> int:
    """Difference hash: row-wise gradient over a 9x8 downscale (64-bit int)."""
    gray = _resize_bilinear(_to_gray(width, height, pixels, channels), width, height, 9, 8)
    bits: List[int] = []
    for y in range(8):
        for x in range(8):
            bits.append(1 if gray[y * 9 + x] < gray[y * 9 + x + 1] else 0)
    return _pack_bits(bits)


def _dct_coefficients(gray: List[float], size: int, count: int) -> List[List[float]]:
    """Top-left ``count x count`` DCT-II coefficients (unnormalised)."""
    cos_table = [[math.cos((2 * x + 1) * u * math.pi / (2 * size)) for x in range(size)] for u in range(count)]
    result = [[0.0] * count for _ in range(count)]
    for u in range(count):
        for v in range(count):
            total = 0.0
            for y in range(size):
                row = gray[y * size : y * size + size]
                cos_v = cos_table[v][y]
                for x in range(size):
                    total += row[x] * cos_table[u][x] * cos_v
            result[u][v] = total
    return result


def phash_64(width: int, height: int, pixels: Any, channels: int = 3) -> int:
    """Perceptual hash (DCT): median-thresholded 8x8 low-frequency block.

    Downscales to 32x32 grayscale, computes the 2D DCT-II, keeps the top-left
    8x8 coefficients (DC included), and thresholds against their median.  The
    result is a 64-bit integer stable under mild resize/compression.
    """
    gray = _resize_bilinear(_to_gray(width, height, pixels, channels), width, height, 32, 32)
    dct = _dct_coefficients(gray, 32, 8)
    flat = [dct[u][v] for u in range(8) for v in range(8)]
    median = _median(flat)
    bits = [1 if value > median else 0 for value in flat]
    return _pack_bits(bits)


def hamming_distance(a: int, b: int) -> int:
    """Return the number of differing bits between two 64-bit hashes."""
    return bin(a ^ b).count("1")


def silhouette_iou(a: Sequence[int], b: Sequence[int]) -> float:
    """Intersection-over-union of two binary masks.

    Masks are equal-length sequences of ``0/1`` (or falsy/truthy values).
    Returns ``1.0`` when both masks are empty (trivially identical).
    """
    if len(a) != len(b):
        raise ValueError("masks must be equal length")
    intersection = 0
    union = 0
    for left, right in zip(a, b):
        left_on = bool(left)
        right_on = bool(right)
        if left_on and right_on:
            intersection += 1
        if left_on or right_on:
            union += 1
    if union == 0:
        return 1.0
    return intersection / union


def ssim(a: List[float], b: List[float], width: int, height: int, window: int = 8) -> float:
    """Mean structural similarity over uniform ``window x window`` tiles.

    ``a`` and ``b`` are grayscale luma lists of length ``width * height`` in
    ``[0, 1]``.  Returns ``1.0`` for identical inputs and can go negative for
    strongly anti-correlated ones.  Pixels outside the tiled area (when
    dimensions are not multiples of ``window``) are ignored.
    """
    if len(a) != len(b) or len(a) != width * height:
        raise ValueError("a and b must both be width * height long")
    if width < window or height < window:
        raise ValueError("image smaller than SSIM window")
    k1 = 0.01
    k2 = 0.03
    c1 = k1 * k1
    c2 = k2 * k2
    total = 0.0
    count = 0
    for y in range(0, height - window + 1, window):
        for x in range(0, width - window + 1, window):
            va: List[float] = []
            vb: List[float] = []
            for wy in range(window):
                for wx in range(window):
                    va.append(a[(y + wy) * width + x + wx])
                    vb.append(b[(y + wy) * width + x + wx])
            mu_a = _mean(va)
            mu_b = _mean(vb)
            var_a = sum((v - mu_a) ** 2 for v in va) / (len(va) - 1) if len(va) > 1 else 0.0
            var_b = sum((v - mu_b) ** 2 for v in vb) / (len(vb) - 1) if len(vb) > 1 else 0.0
            cov = sum((va[i] - mu_a) * (vb[i] - mu_b) for i in range(len(va))) / (len(va) - 1) if len(va) > 1 else 0.0
            numerator = (2 * mu_a * mu_b + c1) * (2 * cov + c2)
            denominator = (mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2)
            total += numerator / denominator
            count += 1
    if count == 0:
        return 1.0
    return total / count


def delta_e_2000(lab_a: Sequence[float], lab_b: Sequence[float]) -> float:
    """CIEDE2000 colour difference between two Lab colours ``(L, a, b)``."""
    l1, a1, b1 = float(lab_a[0]), float(lab_a[1]), float(lab_a[2])
    l2, a2, b2 = float(lab_b[0]), float(lab_b[1]), float(lab_b[2])

    c1 = math.hypot(a1, b1)
    c2 = math.hypot(a2, b2)
    c_bar = (c1 + c2) / 2.0
    c_bar_7 = c_bar**7
    g = 0.5 * (1.0 - math.sqrt(c_bar_7 / (c_bar_7 + 25.0**7)))

    a1p = (1.0 + g) * a1
    a2p = (1.0 + g) * a2
    c1p = math.hypot(a1p, b1)
    c2p = math.hypot(a2p, b2)

    def _hue(a: float, b: float) -> float:
        if a == 0.0 and b == 0.0:
            return 0.0
        hue = math.degrees(math.atan2(b, a))
        return hue if hue >= 0 else hue + 360.0

    h1p = _hue(a1p, b1)
    h2p = _hue(a2p, b2)

    dlp = l2 - l1
    dcp = c2p - c1p

    if c1p * c2p == 0.0:
        dhp = 0.0
    elif abs(h2p - h1p) <= 180.0:
        dhp = h2p - h1p
    elif h2p - h1p > 180.0:
        dhp = h2p - h1p - 360.0
    else:
        dhp = h2p - h1p + 360.0
    dhp_deg = 2.0 * math.sqrt(c1p * c2p) * math.sin(math.radians(dhp / 2.0))

    l_bar = (l1 + l2) / 2.0
    cp_bar = (c1p + c2p) / 2.0

    if c1p * c2p == 0.0:
        hp_bar = h1p + h2p
    elif abs(h1p - h2p) <= 180.0:
        hp_bar = (h1p + h2p) / 2.0
    elif h1p + h2p < 360.0:
        hp_bar = (h1p + h2p + 360.0) / 2.0
    else:
        hp_bar = (h1p + h2p - 360.0) / 2.0

    t = (
        1.0
        - 0.17 * math.cos(math.radians(hp_bar - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hp_bar))
        + 0.32 * math.cos(math.radians(3.0 * hp_bar + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hp_bar - 63.0))
    )
    delta_theta = 30.0 * math.exp(-(((hp_bar - 275.0) / 25.0) ** 2))
    cp_bar_7 = cp_bar**7
    rc = 2.0 * math.sqrt(cp_bar_7 / (cp_bar_7 + 25.0**7))
    sl = 1.0 + (0.015 * (l_bar - 50.0) ** 2) / math.sqrt(20.0 + (l_bar - 50.0) ** 2)
    sc = 1.0 + 0.045 * cp_bar
    sh = 1.0 + 0.015 * cp_bar * t
    rt = -math.sin(math.radians(2.0 * delta_theta)) * rc

    return math.sqrt((dlp / sl) ** 2 + (dcp / sc) ** 2 + (dhp_deg / sh) ** 2 + rt * (dcp / sc) * (dhp_deg / sh))


def sobel_edges(width: int, height: int, pixels: Any, channels: int = 3) -> List[float]:
    """Sobel gradient magnitude per pixel (grayscale first)."""
    gray = _to_gray(width, height, pixels, channels)
    if width < 3 or height < 3:
        raise ValueError("image must be at least 3x3 for Sobel")
    magnitudes: List[float] = [0.0] * (width * height)
    for y in range(1, height - 1):
        for x in range(1, width - 1):
            tl = gray[(y - 1) * width + x - 1]
            tc = gray[(y - 1) * width + x]
            tr = gray[(y - 1) * width + x + 1]
            ml = gray[y * width + x - 1]
            mr = gray[y * width + x + 1]
            bl = gray[(y + 1) * width + x - 1]
            bc = gray[(y + 1) * width + x]
            br = gray[(y + 1) * width + x + 1]
            gx = (tr + 2 * mr + br) - (tl + 2 * ml + bl)
            gy = (bl + 2 * bc + br) - (tl + 2 * tc + tr)
            magnitudes[y * width + x] = math.hypot(gx, gy)
    return magnitudes


def edge_density(width: int, height: int, pixels: Any, channels: int = 3, threshold: float = 0.2) -> float:
    """Fraction of pixels whose Sobel magnitude exceeds ``threshold``."""
    magnitudes = sobel_edges(width, height, pixels, channels)
    strong = sum(1 for value in magnitudes if value > threshold)
    return strong / len(magnitudes)
