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
| `new_auto()` | `Capturer` | Create capturer with best available backend |
| `new_mock(width=1920, height=1080)` | `Capturer` | Create capturer with mock backend (for testing/CI) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `capture(format="png", jpeg_quality=85, scale=1.0, timeout_ms=5000, process_id=None, window_title=None)` | `CaptureFrame` | Capture a frame |
| `backend_name()` | `str` | Name of the active backend (e.g. `"DXGI Desktop Duplication"`) |
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

## Backends

| Backend | Platform | Description |
|---------|----------|-------------|
| `dxgi` | Windows | DXGI Desktop Duplication API |
| `x11` | Linux | X11 XShmGetImage |
| `mock` | All | Synthetic checkerboard for testing |

Backend selection is automatic via `new_auto()`.

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
