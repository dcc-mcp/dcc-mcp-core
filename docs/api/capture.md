# Capture API

`dcc_mcp_core` (capture module)

Screen capture for DCC applications using platform-specific backends.

## Capturer

High-level capturer wrapper with automatic backend selection.

### Constructor

```python
from dcc_mcp_core import Capturer

capturer = Capturer.new_auto()
```

### Static Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `new_auto()` | `Capturer` | Create capturer with best available backend (full-screen / display) |
| `new_window_auto()` | `Capturer` | Create capturer configured for single-window capture (HWND PrintWindow on Windows; Mock elsewhere) |
| `new_mock(width=1920, height=1080)` | `Capturer` | Create capturer with mock backend (for testing/CI) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `capture(format="png", jpeg_quality=85, scale=1.0, timeout_ms=5000, process_id=None, window_title=None)` | `CaptureFrame` | Capture a frame (display or, when `process_id`/`window_title` is set, the matching window) |
| `capture_window(*, process_id=None, window_handle=None, window_title=None, format="png", jpeg_quality=85, scale=1.0, timeout_ms=5000, include_decorations=True)` | `CaptureFrame` | Capture a single window. At least one of `process_id` / `window_handle` / `window_title` must be provided |
| `backend_name()` | `str` | Name of the active backend (e.g. `"DXGI Desktop Duplication"`, `"HWND PrintWindow"`) |
| `backend_kind()` | `CaptureBackendKind` | Enum form of the active backend |
| `stats()` | `tuple[int, int, int]` | Running statistics: `(capture_count, total_bytes, error_count)` |

### CaptureFrame

```python
frame = capturer.capture(format="png")
print(frame.width, frame.height)  # Frame dimensions
print(frame.format)               # Format string: "png", "jpeg", or "raw_bgra"
print(frame.mime_type)            # MIME type, e.g. "image/png"
print(frame.byte_len())           # Byte length of encoded data
print(frame.data)                 # Encoded image bytes
```

### CaptureFrame Properties

| Property | Type | Description |
|----------|------|-------------|
| `width` | `int` | Frame width in pixels |
| `height` | `int` | Frame height in pixels |
| `data` | `bytes` | Encoded image bytes (PNG, JPEG) or raw BGRA32 data |
| `format` | `str` | Format string: `"png"`, `"jpeg"`, or `"raw_bgra"` |
| `mime_type` | `str` | MIME type for the encoded bytes (e.g. `"image/png"`) |
| `timestamp_ms` | `int` | Milliseconds since Unix epoch at capture time |
| `dpi_scale` | `float` | Display scale factor (1.0 standard, 2.0 HiDPI) |
| `window_rect` | `tuple[int, int, int, int] \| None` | `(x, y, width, height)` of the source window in screen coordinates, or `None` for full-screen / display captures |
| `window_title` | `str \| None` | Source window title, or `None` for full-screen / display captures |

### CaptureFrame Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `byte_len()` | `int` | Byte length of the encoded image data |

### CaptureFormat

| Format | Description |
|--------|-------------|
| `png` | PNG image format (lossless, larger) |
| `jpeg` / `jpg` | JPEG image format (lossy, smaller) |
| `raw_bgra` | Raw BGRA32 bytes (no encoding) |

### Capture Parameters

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `format` | `str` | `"png"` | Output format |
| `jpeg_quality` | `int` | `85` | JPEG quality (1-100) |
| `scale` | `float` | `1.0` | Scale factor |
| `timeout_ms` | `int` | `5000` | Capture timeout |
| `process_id` | `int` | `None` | Capture specific process |
| `window_title` | `str` | `None` | Capture specific window |

## Window-Target Capture

Capture a single application window instead of the entire display.

```python
from dcc_mcp_core import Capturer, CaptureTarget, WindowFinder

# High-level: auto-select window backend, capture by PID / title / handle
cap = Capturer.new_window_auto()
frame = cap.capture_window(window_title="Maya 2024", include_decorations=True)
print(frame.window_rect, frame.window_title)   # ((x, y, w, h), "Maya 2024 - ...")

# Low-level: resolve a target to a concrete HWND before capture
finder = WindowFinder()
info = finder.find(CaptureTarget.process_id(12345))
if info is not None:
    print(info.handle, info.pid, info.title, info.rect)
    frame = cap.capture_window(window_handle=info.handle)

# Enumerate every visible top-level window
for info in finder.enumerate():
    print(info.handle, info.title)
```

### `CaptureTarget`

Opaque window / display target descriptor. Construct via the static factories below.

| Factory | Description |
|---------|-------------|
| `CaptureTarget.primary_display()` | The primary display (full-screen capture) |
| `CaptureTarget.monitor_index(index)` | A specific monitor by 0-based index |
| `CaptureTarget.process_id(pid)` | The main window belonging to a process |
| `CaptureTarget.window_title(title)` | The first window whose title contains the substring |
| `CaptureTarget.window_handle(handle)` | A specific HWND / X11 window ID |

### `WindowFinder`

Resolves a `CaptureTarget` to a concrete `WindowInfo` without raising when no match is found.

| Method | Returns | Description |
|--------|---------|-------------|
| `WindowFinder()` | `WindowFinder` | Construct a finder (platform-native enumeration on Windows; stubbed elsewhere) |
| `.find(target)` | `WindowInfo \| None` | Resolve a `CaptureTarget` — returns `None` when no matching window exists |
| `.enumerate()` | `list[WindowInfo]` | Every visible top-level window |

### `WindowInfo`

| Property | Type | Description |
|----------|------|-------------|
| `handle` | `int` | Native window handle (HWND on Windows, X11 window ID on Linux) |
| `pid` | `int` | Owner process ID |
| `title` | `str` | Window title |
| `rect` | `tuple[int, int, int, int]` | `(x, y, width, height)` in screen coordinates |

## Backends

| Backend | Platform | Kind | Description |
|---------|----------|------|-------------|
| `dxgi` | Windows | `DxgiDesktopDuplication` | DXGI Desktop Duplication API — full-screen / display |
| `hwnd` | Windows | `HwndPrintWindow` | GDI `PrintWindow` + `BitBlt` fallback — single window |
| `x11` | Linux | `X11Xshm` | X11 `XShmGetImage` — full-screen |
| `pipewire` | Linux | `PipeWire` | PipeWire screencast (Wayland) — reserved |
| `screencapturekit` | macOS | `ScreenCaptureKit` | ScreenCaptureKit — reserved |
| `mock` | All | `Mock` | Synthetic checkerboard for testing |

Backend selection is automatic:
- `Capturer.new_auto()` — picks the best full-screen / display backend.
- `Capturer.new_window_auto()` — picks the best window-target backend (HWND on Windows; Mock elsewhere).

### `CaptureBackendKind`

Enum exposed as `CaptureBackendKind.<Variant>` class attributes. Useful for
branching on the backend without parsing `backend_name()`:

```python
from dcc_mcp_core import Capturer, CaptureBackendKind

cap = Capturer.new_window_auto()
if cap.backend_kind() == CaptureBackendKind.HwndPrintWindow:
    ...  # Windows window capture path
```

## Error Handling

Capture errors are raised as `RuntimeError`:

```python
try:
    frame = capturer.capture(timeout_ms=1000)
except RuntimeError as e:
    print(f"Capture failed: {e}")
```

## Platform-Specific Notes

### Windows

The DXGI backend requires:
- Windows 8 or later
- DirectX 11 compatible GPU
- Desktop Duplication support

### Linux

The X11 backend requires:
- X11 display server
- Read access to the X server

### macOS

macOS uses the Mock backend for testing. Production capture requires platform-specific implementation.
