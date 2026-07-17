"""Tests for the Windows HWND PrintWindow capture backend.

All tests are Windows-only. On other platforms ``Capturer.new_window_auto``
falls back to the mock backend (see :mod:`tests.test_capture_window_api`).
"""

# Import future modules
from __future__ import annotations

# Import built-in modules
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import textwrap

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core

pytestmark = pytest.mark.skipif(sys.platform != "win32", reason="HwndBackend is Windows-only")


_TIMEOUT_PROBE = textwrap.dedent(
    r"""
    import ctypes
    from collections import Counter
    import os
    import subprocess
    import sys
    import time
    from ctypes import wintypes

    import dcc_mcp_core

    user32 = ctypes.WinDLL("user32", use_last_error=True)
    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    gdi32 = ctypes.WinDLL("gdi32", use_last_error=True)
    mode = sys.argv[1]
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
    kernel32.GetCurrentProcess.restype = wintypes.HANDLE
    user32.GetGuiResources.argtypes = [wintypes.HANDLE, wintypes.DWORD]
    user32.GetGuiResources.restype = wintypes.DWORD
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
    user32.GetWindowRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]
    user32.GetWindowRect.restype = wintypes.BOOL
    user32.GetDC.argtypes = [wintypes.HWND]
    user32.GetDC.restype = wintypes.HDC
    user32.ReleaseDC.argtypes = [wintypes.HWND, wintypes.HDC]
    user32.ReleaseDC.restype = ctypes.c_int
    user32.PrintWindow.argtypes = [wintypes.HWND, wintypes.HDC, wintypes.UINT]
    user32.PrintWindow.restype = wintypes.BOOL
    gdi32.CreateCompatibleDC.argtypes = [wintypes.HDC]
    gdi32.CreateCompatibleDC.restype = wintypes.HDC
    gdi32.CreateCompatibleBitmap.argtypes = [wintypes.HDC, ctypes.c_int, ctypes.c_int]
    gdi32.CreateCompatibleBitmap.restype = wintypes.HANDLE
    gdi32.SelectObject.argtypes = [wintypes.HDC, wintypes.HANDLE]
    gdi32.SelectObject.restype = wintypes.HANDLE
    gdi32.DeleteObject.argtypes = [wintypes.HANDLE]
    gdi32.DeleteObject.restype = wintypes.BOOL
    gdi32.DeleteDC.argtypes = [wintypes.HDC]
    gdi32.DeleteDC.restype = wintypes.BOOL

    class BitmapInfoHeader(ctypes.Structure):
        _fields_ = [
            ("biSize", wintypes.DWORD),
            ("biWidth", wintypes.LONG),
            ("biHeight", wintypes.LONG),
            ("biPlanes", wintypes.WORD),
            ("biBitCount", wintypes.WORD),
            ("biCompression", wintypes.DWORD),
            ("biSizeImage", wintypes.DWORD),
            ("biXPelsPerMeter", wintypes.LONG),
            ("biYPelsPerMeter", wintypes.LONG),
            ("biClrUsed", wintypes.DWORD),
            ("biClrImportant", wintypes.DWORD),
        ]

    class RgbQuad(ctypes.Structure):
        _fields_ = [
            ("rgbBlue", wintypes.BYTE),
            ("rgbGreen", wintypes.BYTE),
            ("rgbRed", wintypes.BYTE),
            ("rgbReserved", wintypes.BYTE),
        ]

    class BitmapInfo(ctypes.Structure):
        _fields_ = [("bmiHeader", BitmapInfoHeader), ("bmiColors", RgbQuad * 1)]

    gdi32.GetDIBits.argtypes = [
        wintypes.HDC,
        wintypes.HANDLE,
        wintypes.UINT,
        wintypes.UINT,
        ctypes.c_void_p,
        ctypes.POINTER(BitmapInfo),
        wintypes.UINT,
    ]
    gdi32.GetDIBits.restype = ctypes.c_int

    print_calls = ctypes.c_int(0)

    @wndproc_type
    def wndproc(hwnd, message, wparam, lparam):
        if message == 0x0317:  # WM_PRINT
            print_calls.value += 1
            if mode in {"child", "same-thread"}:
                time.sleep(5)
                return 0
        return user32.DefWindowProcW(hwnd, message, wparam, lparam)

    def create_window():
        instance = kernel32.GetModuleHandleW(None)
        class_name = f"DccMcpCaptureTimeoutProbe{os.getpid()}"
        spec = WndClass()
        spec.lpfnWndProc = wndproc
        spec.hInstance = instance
        spec.lpszClassName = class_name
        if not user32.RegisterClassW(ctypes.byref(spec)):
            raise ctypes.WinError(ctypes.get_last_error())
        hwnd = user32.CreateWindowExW(
            0,
            class_name,
            "DCC MCP capture timeout probe",
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
            raise ctypes.WinError(ctypes.get_last_error())
        return hwnd

    def literal_print_window(hwnd):
        rect = wintypes.RECT()
        assert user32.GetWindowRect(hwnd, ctypes.byref(rect))
        width = rect.right - rect.left
        height = rect.bottom - rect.top
        window_dc = user32.GetDC(hwnd)
        assert window_dc
        memory_dc = gdi32.CreateCompatibleDC(window_dc)
        assert memory_dc
        bitmap = gdi32.CreateCompatibleBitmap(window_dc, width, height)
        assert bitmap
        old = gdi32.SelectObject(memory_dc, bitmap)
        assert old
        try:
            printed = user32.PrintWindow(hwnd, memory_dc, 0x00000002)  # PW_RENDERFULLCONTENT
            assert printed
            info = BitmapInfo()
            info.bmiHeader.biSize = ctypes.sizeof(BitmapInfoHeader)
            info.bmiHeader.biWidth = width
            info.bmiHeader.biHeight = -height
            info.bmiHeader.biPlanes = 1
            info.bmiHeader.biBitCount = 32
            pixels = (ctypes.c_ubyte * (width * height * 4))()
            rows = gdi32.GetDIBits(memory_dc, bitmap, 0, height, pixels, ctypes.byref(info), 0)
            assert rows == height
            return bytes(pixels)
        finally:
            gdi32.SelectObject(memory_dc, old)
            gdi32.DeleteObject(bitmap)
            gdi32.DeleteDC(memory_dc)
            user32.ReleaseDC(hwnd, window_dc)

    def child_main():
        hwnd = create_window()
        print(int(hwnd), flush=True)
        message = wintypes.MSG()
        while user32.GetMessageW(ctypes.byref(message), None, 0, 0) > 0:
            user32.TranslateMessage(ctypes.byref(message))
            user32.DispatchMessageW(ctypes.byref(message))

    def external_main():
        child = subprocess.Popen(
            [sys.executable, "-u", __file__, "child"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            assert child.stdout is not None
            hwnd = int(child.stdout.readline().strip())
            cap = dcc_mcp_core.Capturer.new_window_auto()
            before = user32.GetGuiResources(kernel32.GetCurrentProcess(), 0)
            for _ in range(8):
                started = time.perf_counter()
                try:
                    cap.capture_window(window_handle=hwnd, timeout_ms=100)
                except RuntimeError as exc:
                    assert "timed out after 100ms" in str(exc)
                else:
                    raise AssertionError("hung external HWND capture unexpectedly succeeded")
                assert time.perf_counter() - started < 1.0
            after = user32.GetGuiResources(kernel32.GetCurrentProcess(), 0)
            assert after <= before + 2, (before, after)
            tasklist = subprocess.run(
                ["tasklist", "/FI", "IMAGENAME eq dcc-mcp-capture-helper.exe", "/FO", "CSV", "/NH"],
                capture_output=True,
                text=True,
                timeout=3,
                check=False,
            )
            assert "dcc-mcp-capture-helper.exe" not in tasklist.stdout.lower(), tasklist.stdout
        finally:
            child.kill()
            child.wait(timeout=3)

    def same_thread_main():
        hwnd = create_window()
        started = time.perf_counter()
        frame = dcc_mcp_core.Capturer.new_window_auto().capture_window(
            window_handle=hwnd,
            timeout_ms=100,
        )
        assert time.perf_counter() - started < 1.0
        assert frame.byte_len() > 0
        assert print_calls.value == 0
        user32.DestroyWindow(hwnd)

    def responsive_main():
        child = subprocess.Popen(
            [sys.executable, "-u", __file__, "paint-child"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            assert child.stdout is not None
            hwnd = int(child.stdout.readline().strip())
            frame = dcc_mcp_core.Capturer.new_window_auto().capture_window(
                window_handle=hwnd,
                format="raw_bgra",
                timeout_ms=1000,
            )
            data = bytes(frame.data)
            baseline = literal_print_window(hwnd)
            assert data == baseline
            pixels = Counter(data[offset : offset + 4] for offset in range(0, len(data), 4))
            assert len(pixels) > 8, pixels.most_common(8)
        finally:
            child.kill()
            child.wait(timeout=3)

    def missing_helper_main():
        child = subprocess.Popen(
            [sys.executable, "-u", __file__, "paint-child"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        try:
            assert child.stdout is not None
            hwnd = int(child.stdout.readline().strip())
            os.environ["DCC_MCP_CAPTURE_HELPER"] = os.path.join(
                os.path.dirname(__file__), "definitely-missing-capture-helper.exe"
            )
            try:
                dcc_mcp_core.Capturer.new_window_auto().capture_window(
                    window_handle=hwnd,
                    timeout_ms=500,
                )
            except RuntimeError as exc:
                assert "capture helper" in str(exc).lower()
                assert "was not found" in str(exc)
            else:
                raise AssertionError("capture unexpectedly succeeded without helper")
        finally:
            child.kill()
            child.wait(timeout=3)

    if mode in {"child", "paint-child"}:
        child_main()
    elif mode == "external":
        external_main()
    elif mode == "responsive":
        responsive_main()
    elif mode == "missing-helper":
        missing_helper_main()
    else:
        same_thread_main()
    """
)


def _run_timeout_probe(mode: str, *, timeout: float) -> subprocess.CompletedProcess[str]:
    with tempfile.TemporaryDirectory() as temp_dir:
        script = Path(temp_dir, "capture_timeout_probe.py")
        script.write_text(_TIMEOUT_PROBE, encoding="utf-8")
        return subprocess.run(
            [sys.executable, str(script), mode],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )


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

    def test_hung_external_window_honors_timeout_without_gdi_leak(self) -> None:
        result = _run_timeout_probe("external", timeout=15)
        assert result.returncode == 0, result.stderr or result.stdout

    def test_same_thread_window_uses_non_message_fallback(self) -> None:
        result = _run_timeout_probe("same-thread", timeout=3)
        assert result.returncode == 0, result.stderr or result.stdout

    def test_responsive_window_preserves_print_quality_path(self) -> None:
        result = _run_timeout_probe("responsive", timeout=8)
        assert result.returncode == 0, result.stderr or result.stdout

    def test_missing_helper_override_fails_actionably(self) -> None:
        result = _run_timeout_probe("missing-helper", timeout=5)
        assert result.returncode == 0, result.stderr or result.stdout


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
