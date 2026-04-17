"""Cross-platform tests for the window-target capture API surface.

These tests cover the Python API contract of ``Capturer.new_window_auto``
and ``Capturer.capture_window`` in a way that works on any platform — the
Windows-specific backend behaviour lives in
:mod:`tests.test_capture_hwnd_backend`.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import sys

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── new_window_auto backend selection ─────────────────────────────────────────


class TestNewWindowAutoBackend:
    def test_returns_capturer_instance(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        assert isinstance(cap, dcc_mcp_core.Capturer)

    def test_backend_kind_is_hwnd_on_windows(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        if sys.platform == "win32":
            assert cap.backend_kind() == dcc_mcp_core.CaptureBackendKind.HwndPrintWindow
        else:
            assert cap.backend_kind() == dcc_mcp_core.CaptureBackendKind.Mock

    def test_backend_name_nonempty(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        assert len(cap.backend_name()) > 0


# ── capture_window argument validation ────────────────────────────────────────


class TestCaptureWindowArgValidation:
    def test_requires_at_least_one_target(self) -> None:
        """capture_window() with no target kwargs must raise ValueError."""
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises(ValueError):
            cap.capture_window()

    def test_all_target_params_are_keyword_only(self) -> None:
        """Positional calls must fail — the signature is keyword-only."""
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises(TypeError):
            # process_id is keyword-only
            cap.capture_window(1234)  # type: ignore[misc]

    @pytest.mark.skipif(
        sys.platform != "win32",
        reason="window-target lookup only enforced by HwndBackend; Mock accepts any target",
    )
    def test_accepts_process_id_keyword(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises((RuntimeError, ValueError)):
            cap.capture_window(process_id=0x7FFFFFFF, timeout_ms=200)

    @pytest.mark.skipif(
        sys.platform != "win32",
        reason="window-target lookup only enforced by HwndBackend; Mock accepts any target",
    )
    def test_accepts_window_handle_keyword(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises((RuntimeError, ValueError)):
            cap.capture_window(window_handle=0x7FFFFFFE, timeout_ms=200)

    @pytest.mark.skipif(
        sys.platform != "win32",
        reason="window-target lookup only enforced by HwndBackend; Mock accepts any target",
    )
    def test_accepts_window_title_keyword(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises((RuntimeError, ValueError)):
            cap.capture_window(
                window_title="__nonexistent-window-title-xyz__",
                timeout_ms=200,
            )


# ── CaptureFrame optional window fields ───────────────────────────────────────


class TestCaptureFrameOptionalFields:
    """Full-screen captures must report ``window_rect``/``window_title`` as None."""

    def test_mock_frame_window_rect_is_none(self) -> None:
        cap = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = cap.capture()
        assert frame.window_rect is None

    def test_mock_frame_window_title_is_none(self) -> None:
        cap = dcc_mcp_core.Capturer.new_mock(width=64, height=64)
        frame = cap.capture()
        assert frame.window_title is None


# ── WindowFinder cross-platform shape ─────────────────────────────────────────


class TestWindowFinderCrossPlatform:
    def test_construct_window_finder(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        assert finder is not None

    def test_enumerate_returns_list(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        result = finder.enumerate()
        assert isinstance(result, list)

    def test_find_unknown_pid_returns_none(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        result = finder.find(dcc_mcp_core.CaptureTarget.process_id(0x7FFFFFFF))
        assert result is None


# ── capture_window_png / capture_region_png sugar API (#212) ──────────────────


class TestCaptureWindowPngStatic:
    """Covers the issue #212 ergonomic wrappers for bytes-or-None captures."""

    def test_is_static_method(self) -> None:
        """capture_window_png is callable on the class itself (no instance)."""
        assert callable(dcc_mcp_core.Capturer.capture_window_png)

    def test_capture_region_png_is_static_method(self) -> None:
        assert callable(dcc_mcp_core.Capturer.capture_region_png)

    @pytest.mark.skipif(
        sys.platform != "win32",
        reason="unknown-PID => None semantics only enforced by HwndBackend; Mock backend has no PID awareness",
    )
    def test_unknown_pid_returns_none(self) -> None:
        """On HwndBackend, an unresolvable PID must return ``None`` (not raise)."""
        result = dcc_mcp_core.Capturer.capture_window_png(pid=0x7FFFFFFF, timeout_ms=200)
        assert result is None

    @pytest.mark.skipif(
        sys.platform != "win32",
        reason="unknown-PID => None semantics only enforced by HwndBackend; Mock backend has no PID awareness",
    )
    def test_region_unknown_pid_returns_none(self) -> None:
        result = dcc_mcp_core.Capturer.capture_region_png(pid=0x7FFFFFFF, x=0, y=0, w=10, h=10, timeout_ms=200)
        assert result is None

    def test_region_zero_width_returns_none(self) -> None:
        """Zero-width/height regions are rejected cheaply as ``None`` on every backend."""
        result = dcc_mcp_core.Capturer.capture_region_png(pid=0x7FFFFFFF, x=0, y=0, w=0, h=100, timeout_ms=200)
        assert result is None

    def test_region_zero_height_returns_none(self) -> None:
        result = dcc_mcp_core.Capturer.capture_region_png(pid=0x7FFFFFFF, x=0, y=0, w=100, h=0, timeout_ms=200)
        assert result is None

    def test_timeout_is_keyword_only(self) -> None:
        """timeout_ms must be a keyword argument — positional call raises TypeError."""
        with pytest.raises(TypeError):
            dcc_mcp_core.Capturer.capture_window_png(0x7FFFFFFF, 200)  # type: ignore[misc]

    def test_region_coords_are_positional(self) -> None:
        """Region coords (x, y, w, h) may be passed positionally — the call must not raise."""
        result = dcc_mcp_core.Capturer.capture_region_png(0x7FFFFFFF, 0, 0, 10, 10, timeout_ms=200)
        # HwndBackend => None (unknown PID); Mock backend => synthetic PNG bytes.
        assert result is None or isinstance(result, bytes)
