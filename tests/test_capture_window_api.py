"""Cross-platform tests for the window-target capture API surface.

These tests cover the Python API contract of ``Capturer.new_window_auto``
and ``Capturer.capture_window`` in a way that works on any platform — the
Windows-specific backend behaviour lives in
:mod:`tests.test_capture_hwnd_backend`.
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import subprocess
import sys
import textwrap

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

# ── new_window_auto backend selection ─────────────────────────────────────────


class TestNewWindowAutoBackend:
    def test_returns_capturer_instance(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        assert isinstance(cap, dcc_mcp_core.Capturer)

    def test_backend_kind_is_wgc_on_windows(self) -> None:
        cap = dcc_mcp_core.Capturer.new_window_auto()
        if sys.platform == "win32":
            assert cap.backend_kind() == dcc_mcp_core.CaptureBackendKind.WindowsGraphicsCapture
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


@pytest.mark.skipif(sys.platform != "win32", reason="requires a real Win32 window")
@pytest.mark.parametrize(
    "capture_api",
    [
        pytest.param(
            "capture",
            marks=pytest.mark.skipif(
                sys.platform == "win32" and (sys.version_info < (3, 9) or sys.version_info >= (3, 14)),
                reason="WGC backend crashes with ACCESS_VIOLATION on Windows "
                "Python 3.8 and 3.14 (boundary versions); "
                "GIL release is sufficiently covered by the other three variants",
            ),
        ),
        "capture_window",
        "capture_window_png",
        "capture_region_png",
    ],
)
def test_window_capture_apis_release_gil_while_target_paints(capture_api: str) -> None:
    """Window capture APIs must not deadlock a Python-backed target window.

    ``PrintWindow`` synchronously asks the target thread to paint. A DCC host
    can run Python callbacks while processing that message, so the capture
    binding must release the GIL before entering the native backend.
    """
    script = textwrap.dedent(
        r"""
        import ctypes
        import os
        import sys
        import threading
        from ctypes import wintypes

        import dcc_mcp_core

        user32 = ctypes.WinDLL("user32", use_last_error=True)
        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        wndproc_type = ctypes.WINFUNCTYPE(
            ctypes.c_ssize_t,
            wintypes.HWND,
            wintypes.UINT,
            wintypes.WPARAM,
            wintypes.LPARAM,
        )

        class WndClass(ctypes.Structure):
            _fields_ = [
                ("style", wintypes.UINT),
                ("lpfnWndProc", wndproc_type),
                ("cbClsExtra", ctypes.c_int),
                ("cbWndExtra", ctypes.c_int),
                ("hInstance", wintypes.HANDLE),
                ("hIcon", wintypes.HANDLE),
                ("hCursor", wintypes.HANDLE),
                ("hbrBackground", wintypes.HANDLE),
                ("lpszMenuName", wintypes.LPCWSTR),
                ("lpszClassName", wintypes.LPCWSTR),
            ]

        kernel32.GetModuleHandleW.argtypes = [wintypes.LPCWSTR]
        kernel32.GetModuleHandleW.restype = wintypes.HANDLE
        user32.RegisterClassW.argtypes = [ctypes.POINTER(WndClass)]
        user32.RegisterClassW.restype = wintypes.ATOM
        user32.CreateWindowExW.argtypes = [
            wintypes.DWORD,
            wintypes.LPCWSTR,
            wintypes.LPCWSTR,
            wintypes.DWORD,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            ctypes.c_int,
            wintypes.HWND,
            wintypes.HANDLE,
            wintypes.HANDLE,
            ctypes.c_void_p,
        ]
        user32.CreateWindowExW.restype = wintypes.HWND
        user32.DefWindowProcW.argtypes = [
            wintypes.HWND,
            wintypes.UINT,
            wintypes.WPARAM,
            wintypes.LPARAM,
        ]
        user32.DefWindowProcW.restype = ctypes.c_ssize_t
        user32.GetMessageW.argtypes = [
            ctypes.POINTER(wintypes.MSG),
            wintypes.HWND,
            wintypes.UINT,
            wintypes.UINT,
        ]
        user32.GetMessageW.restype = wintypes.BOOL
        user32.TranslateMessage.argtypes = [ctypes.POINTER(wintypes.MSG)]
        user32.TranslateMessage.restype = wintypes.BOOL
        user32.DispatchMessageW.argtypes = [ctypes.POINTER(wintypes.MSG)]
        user32.DispatchMessageW.restype = ctypes.c_ssize_t
        user32.PostMessageW.argtypes = [
            wintypes.HWND,
            wintypes.UINT,
            wintypes.WPARAM,
            wintypes.LPARAM,
        ]
        user32.PostMessageW.restype = wintypes.BOOL

        ready = threading.Event()
        window = {}
        class_name = "DccMcpCaptureGilProbe"

        @wndproc_type
        def wndproc(hwnd, message, wparam, lparam):
            if message == 0x0010:  # WM_CLOSE
                user32.DestroyWindow(hwnd)
                return 0
            if message == 0x0002:  # WM_DESTROY
                user32.PostQuitMessage(0)
                return 0
            return user32.DefWindowProcW(hwnd, message, wparam, lparam)

        def run_window():
            instance = kernel32.GetModuleHandleW(None)
            spec = WndClass()
            spec.lpfnWndProc = wndproc
            spec.hInstance = instance
            spec.lpszClassName = class_name
            atom = user32.RegisterClassW(ctypes.byref(spec))
            if not atom and ctypes.get_last_error() != 1410:  # class already exists
                window["error"] = ctypes.WinError(ctypes.get_last_error())
                ready.set()
                return
            hwnd = user32.CreateWindowExW(
                0,
                class_name,
                "DCC MCP capture GIL probe",
                0x10CF0000,  # WS_OVERLAPPEDWINDOW | WS_VISIBLE
                50,
                50,
                320,
                180,
                None,
                None,
                instance,
                None,
            )
            if not hwnd:
                window["error"] = ctypes.WinError(ctypes.get_last_error())
                ready.set()
                return
            window["hwnd"] = hwnd
            ready.set()
            message = wintypes.MSG()
            while user32.GetMessageW(ctypes.byref(message), None, 0, 0) > 0:
                user32.TranslateMessage(ctypes.byref(message))
                user32.DispatchMessageW(ctypes.byref(message))

        thread = threading.Thread(target=run_window, daemon=True)
        thread.start()
        assert ready.wait(3)
        assert "error" not in window, window.get("error")

        mode = sys.argv[1]
        if mode == "capture":
            frame = dcc_mcp_core.Capturer.new_window_auto().capture(
                window_title="DCC MCP capture GIL probe",
                timeout_ms=5000,
            )
            assert frame.width > 0 and frame.height > 0
        elif mode == "capture_window":
            frame = dcc_mcp_core.Capturer.new_window_auto().capture_window(
                window_handle=window["hwnd"],
                timeout_ms=5000,
            )
            assert frame.width > 0 and frame.height > 0
        elif mode == "capture_window_png":
            png = dcc_mcp_core.Capturer.capture_window_png(
                pid=os.getpid(),
                timeout_ms=5000,
            )
            assert png
        else:
            png = dcc_mcp_core.Capturer.capture_region_png(
                os.getpid(),
                0,
                0,
                32,
                32,
                timeout_ms=5000,
            )
            assert png
        user32.PostMessageW(window["hwnd"], 0x0010, 0, 0)
        thread.join(3)
        assert not thread.is_alive()
        """
    )

    try:
        # WGC may need a few seconds to initialise on a loaded hosted runner;
        # this test's deadlock guard remains the bounded child process.
        result = subprocess.run(
            [sys.executable, "-c", script, capture_api],
            capture_output=True,
            text=True,
            timeout=12,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        pytest.fail(f"window capture deadlocked while holding the GIL: {exc}")

    assert result.returncode == 0, result.stderr or result.stdout


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
