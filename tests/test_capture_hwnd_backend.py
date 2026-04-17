"""Tests for the Windows HWND PrintWindow capture backend.

All tests are Windows-only. On other platforms ``Capturer.new_window_auto``
falls back to the mock backend (see :mod:`tests.test_capture_window_api`).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import os
import sys

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="HwndBackend is Windows-only")


# ── Backend identity ──────────────────────────────────────────────────────────


class TestHwndBackendIdentity:
    def test_new_window_auto_backend_kind_is_hwnd(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        assert cap.backend_kind() == dcc_mcp_core.CaptureBackendKind.HwndPrintWindow

    def test_new_window_auto_backend_name_mentions_printwindow(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        name = cap.backend_name()
        assert "PrintWindow" in name or "GDI" in name


# ── Error paths ───────────────────────────────────────────────────────────────


class TestHwndBackendErrors:
    def test_nonexistent_pid_raises_runtime_error(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises(RuntimeError):
            cap.capture_window(process_id=0x7FFFFFFF, timeout_ms=500)

    def test_nonexistent_handle_raises_runtime_error(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises(RuntimeError):
            cap.capture_window(window_handle=0xDEADBEEF, timeout_ms=500)

    def test_nonexistent_title_raises_runtime_error(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        with pytest.raises(RuntimeError):
            cap.capture_window(
                window_title="__definitely-nonexistent-window-title-xyz__",
                timeout_ms=500,
            )


# ── Smoke test: capture own process's window (if one exists) ─────────────────


class TestHwndBackendSmoke:
    """Best-effort capture using the current Python process's own PID.

    Skipped automatically when no visible top-level window can be resolved
    for the test runner (headless CI).
    """

    def test_capture_own_process_window_populates_fields(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        finder = dcc_mcp_core.WindowFinder()
        info = finder.find(dcc_mcp_core.CaptureTarget.process_id(os.getpid()))
        if info is None:
            pytest.skip("current process has no visible top-level window (headless CI)")
        frame = cap.capture_window(window_handle=info.handle, timeout_ms=2000)
        assert frame.byte_len() > 0
        assert frame.window_rect is not None
        assert frame.window_title is not None
        _x, _y, w, h = frame.window_rect
        assert w > 0 and h > 0


# ── WindowFinder on Windows ───────────────────────────────────────────────────


class TestWindowFinderWindows:
    def test_enumerate_returns_list(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        windows = finder.enumerate()
        assert isinstance(windows, list)

    def test_enumerate_entries_have_handle_and_pid(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        windows = finder.enumerate()
        for w in windows[:5]:  # sample
            assert isinstance(w.handle, int)
            assert w.handle > 0
            assert isinstance(w.pid, int)
            assert isinstance(w.title, str)
            assert isinstance(w.rect, tuple)
            assert len(w.rect) == 4

    def test_find_nonexistent_pid_returns_none(self) -> None:
        finder = dcc_mcp_core.WindowFinder()
        result = finder.find(dcc_mcp_core.CaptureTarget.process_id(0x7FFFFFFF))
        assert result is None
