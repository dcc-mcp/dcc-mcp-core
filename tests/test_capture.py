"""Tests for dcc-mcp-capture Python bindings.

Covers Capturer (mock backend) and CaptureFrame attributes.
All tests use the mock backend and require no GPU, display, or DCC.
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── Capturer construction ─────────────────────────────────────────────────────


class TestCapturerConstruction:
    def test_new_mock_returns_instance(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock()
        assert capturer is not None

    def test_new_mock_custom_resolution(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=640, height=480)
        assert capturer is not None

    def test_new_auto_returns_instance(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_auto()
        assert capturer is not None

    def test_backend_name_mock_contains_mock(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock()
        name = capturer.backend_name()
        assert "Mock" in name or "mock" in name

    def test_backend_name_nonempty(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_auto()
        assert len(capturer.backend_name()) > 0

    def test_repr_contains_capturer(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock()
        r = repr(capturer)
        assert "Capturer" in r


# ── Capture PNG ───────────────────────────────────────────────────────────────


class TestCapturePng:
    def test_capture_png_default(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=320, height=240)
        frame = capturer.capture()
        assert frame is not None

    def test_capture_png_format(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=100, height=100)
        frame = capturer.capture(format="png")
        assert frame.format == "png"

    def test_capture_png_mime_type(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=50, height=50)
        frame = capturer.capture(format="png")
        assert frame.mime_type == "image/png"

    def test_capture_png_starts_with_magic_bytes(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = capturer.capture(format="png")
        assert frame.data[:4] == b"\x89PNG"

    def test_capture_png_dimensions_match(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=200, height=150)
        frame = capturer.capture(format="png")
        assert frame.width == 200
        assert frame.height == 150

    def test_capture_png_byte_len_positive(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        frame = capturer.capture(format="png")
        assert frame.byte_len() > 0

    def test_capture_png_data_length_matches_byte_len(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        frame = capturer.capture(format="png")
        assert len(frame.data) == frame.byte_len()


# ── Capture JPEG ──────────────────────────────────────────────────────────────


class TestCaptureJpeg:
    def test_capture_jpeg_format(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=100, height=100)
        frame = capturer.capture(format="jpeg")
        assert frame.format == "jpeg"

    def test_capture_jpeg_mime_type(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = capturer.capture(format="jpeg")
        assert frame.mime_type == "image/jpeg"

    def test_capture_jpeg_starts_with_ff_d8(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = capturer.capture(format="jpeg")
        assert frame.data[:2] == b"\xff\xd8"

    def test_capture_jpeg_custom_quality(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = capturer.capture(format="jpeg", jpeg_quality=50)
        assert frame.format == "jpeg"
        assert frame.byte_len() > 0

    def test_capture_jpg_alias(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        frame = capturer.capture(format="jpg")
        assert frame.format == "jpeg"


# ── Capture raw_bgra ──────────────────────────────────────────────────────────


class TestCaptureRawBgra:
    def test_capture_raw_format(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=16, height=16)
        frame = capturer.capture(format="raw_bgra")
        assert frame.format == "raw_bgra"

    def test_capture_raw_byte_len_equals_width_x_height_x_4(self) -> None:
        w, h = 16, 16
        capturer = dcc_mcp_core.Capturer.new_mock(width=w, height=h)
        frame = capturer.capture(format="raw_bgra")
        assert frame.byte_len() == w * h * 4

    def test_capture_raw_alias(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=8, height=8)
        frame = capturer.capture(format="raw")
        assert frame.format == "raw_bgra"


# ── Scale ─────────────────────────────────────────────────────────────────────


class TestCaptureScale:
    def test_scale_half_reduces_dimensions(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=200, height=100)
        frame = capturer.capture(format="raw_bgra", scale=0.5)
        assert frame.width == 100
        assert frame.height == 50

    def test_scale_native_preserves_dimensions(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=100, height=80)
        frame = capturer.capture(format="raw_bgra", scale=1.0)
        assert frame.width == 100
        assert frame.height == 80


# ── CaptureFrame attributes ───────────────────────────────────────────────────


class TestCaptureFrameAttributes:
    def _frame(self) -> dcc_mcp_core.CaptureFrame:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        return capturer.capture()

    def test_timestamp_ms_positive(self) -> None:
        frame = self._frame()
        assert frame.timestamp_ms > 0

    def test_dpi_scale_positive(self) -> None:
        frame = self._frame()
        assert frame.dpi_scale > 0.0

    def test_repr_contains_dimensions(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=10, height=10)
        frame = capturer.capture()
        r = repr(frame)
        assert "10" in r


# ── Capturer stats ────────────────────────────────────────────────────────────


class TestCapturerStats:
    def test_stats_initial_zeros(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock()
        count, total_bytes, errors = capturer.stats()
        assert count == 0
        assert total_bytes == 0
        assert errors == 0

    def test_stats_accumulate_after_captures(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        for _ in range(5):
            capturer.capture(format="png")
        count, total_bytes, errors = capturer.stats()
        assert count == 5
        assert total_bytes > 0
        assert errors == 0

    def test_stats_no_errors_with_mock(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=16, height=16)
        capturer.capture()
        _, _, errors = capturer.stats()
        assert errors == 0

    def test_multiple_formats_accumulate_in_stats(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=32, height=32)
        capturer.capture(format="png")
        capturer.capture(format="jpeg")
        capturer.capture(format="raw_bgra")
        count, _, _ = capturer.stats()
        assert count == 3


# ── CaptureResult wrapper ─────────────────────────────────────────────────────


class TestCaptureResult:
    """CaptureResult is the inner result type; exercise via capture()."""

    def test_capture_returns_frame_not_none(self) -> None:
        capturer = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = capturer.capture()
        assert frame is not None
