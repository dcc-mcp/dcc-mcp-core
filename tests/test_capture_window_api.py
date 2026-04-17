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
