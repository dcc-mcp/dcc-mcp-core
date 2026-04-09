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

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `new_auto()` | `Capturer` | Create capturer with best available backend |
| `capture(format="png", jpeg_quality=85, scale=1.0, timeout_ms=5000, process_id=None, window_title=None)` | `CaptureFrame` | Capture a frame |

### CaptureFrame

```python
frame = capturer.capture(format="png")
print(frame.width, frame.height)  # Frame dimensions
print(frame.bytes_per_pixel)      # Bytes per pixel
print(frame.data)                # Raw frame data as bytes
```

### CaptureFrame Properties

| Property | Type | Description |
|----------|------|-------------|
| `width` | `int` | Frame width in pixels |
| `height` | `int` | Frame height in pixels |
| `bytes_per_pixel` | `int` | Bytes per pixel |
| `data` | `bytes` | Raw frame data |

### CaptureFormat

| Format | Description |
|--------|-------------|
| `png` | PNG image format (lossless, larger) |
| `jpeg` / `jpg` | JPEG image format (lossy, smaller) |
| `rgba` | Raw RGBA bytes |

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

```python
from dcc_mcp_core import CaptureError

try:
    frame = capturer.capture(timeout_ms=1000)
except CaptureError as e:
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
