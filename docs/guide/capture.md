# Capture Guide

Screen capture for DCC applications.

## Overview

The capture module provides GPU framebuffer screenshot and frame capture functionality for DCC applications. It supports multiple backends depending on the platform:

- **Windows** — DXGI Desktop Duplication API
- **Linux** — X11 XShmGetImage
- **All platforms** — Mock backend (synthetic checkerboard for testing / CI)

::: tip
The capturer automatically selects the best available backend on the current platform. Use `Capturer.new_mock()` in headless CI environments.
:::

## Quick Start

### Capturing a Frame

```python
from dcc_mcp_core import Capturer

# Create capturer with best available backend
capturer = Capturer.new_auto()

# Capture a frame (default: PNG format)
frame = capturer.capture()

# Save to file
with open("screenshot.png", "wb") as f:
    f.write(frame.data)

print(f"Captured {frame.width}x{frame.height}, {frame.byte_len()} bytes")
print(f"Format: {frame.format}")  # "png", "jpeg", or "raw_bgra"
print(f"Backend: {capturer.backend_name()}")
```

### Capturing by Process ID or Window Title

```python
# Capture a window by process ID
frame = capturer.capture(process_id=1234)

# Capture a window by title substring
frame = capturer.capture(window_title="Maya")
```

### Mock Capturer (Headless / CI)

```python
from dcc_mcp_core import Capturer

# Synthetic backend — always available, no GPU required
capturer = Capturer.new_mock(width=1920, height=1080)
frame = capturer.capture(format="raw_bgra")
print(f"{frame.width}x{frame.height}")
```

## CaptureFrame

Returned by `Capturer.capture()`. Contains the captured image data and metadata.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `data` | `bytes` | Encoded image bytes (PNG/JPEG) or raw BGRA32 |
| `width` | `int` | Frame width in pixels |
| `height` | `int` | Frame height in pixels |
| `format` | `str` | `"png"`, `"jpeg"`, or `"raw_bgra"` |
| `mime_type` | `str` | MIME type, e.g. `"image/png"` |
| `timestamp_ms` | `int` | Unix timestamp in ms at capture time |
| `dpi_scale` | `float` | Display scale factor (1.0 standard, 2.0 HiDPI) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `byte_len()` | `int` | Byte length of the encoded image data |

### Example

```python
capturer = Capturer.new_auto()
frame = capturer.capture(format="png")

print(f"Size: {frame.width}x{frame.height}")
print(f"Format: {frame.format} ({frame.mime_type})")
print(f"Bytes: {frame.byte_len()}")
print(f"DPI scale: {frame.dpi_scale}")
print(f"Captured at: {frame.timestamp_ms}")

# Write PNG to disk
with open("viewport.png", "wb") as f:
    f.write(frame.data)
```

## Capture Formats

### PNG (Lossless)

Best quality, larger file size. Default format.

```python
frame = capturer.capture(format="png")
```

### JPEG (Lossy)

Smaller file size, some quality loss. Adjust quality with `jpeg_quality`:

```python
frame = capturer.capture(format="jpeg", jpeg_quality=90)
```

### Raw BGRA

Raw pixel data for processing. No encoding overhead.

```python
frame = capturer.capture(format="raw_bgra")
# frame.data contains raw BGRA32 bytes
print(f"Size: {frame.width * frame.height * 4} bytes")
```

## Capture Parameters

### scale

Scale factor 0.0–1.0. Reduces resolution for faster capture:

```python
# Half resolution
frame = capturer.capture(scale=0.5)
```

### timeout_ms

Maximum wait time in milliseconds:

```python
# 10 second timeout
frame = capturer.capture(timeout_ms=10000)
```

### process_id / window_title

Target a specific window:

```python
# By process ID
frame = capturer.capture(process_id=os.getpid())

# By window title (partial match)
frame = capturer.capture(window_title="Maya")
```

## Statistics

```python
capturer = Capturer.new_auto()

# ... do some captures ...

count, total_bytes, errors = capturer.stats()
print(f"Capture count: {count}")
print(f"Total bytes: {total_bytes}")
print(f"Errors: {errors}")
print(f"Backend: {capturer.backend_name()}")
```

## Backend Selection

### Automatic (Recommended)

`new_auto()` probes backends in priority order:

| Priority | Windows | Linux | macOS |
|----------|---------|-------|-------|
| 1 | DXGI Desktop Duplication | X11 (if DISPLAY set) | Mock |
| 2 | Mock | Mock | — |

### Manual Mock (CI / Headless)

```python
# Always safe in CI or headless environments
capturer = Capturer.new_mock(width=1280, height=720)
```

## Error Handling

Capture raises `RuntimeError` on failure:

```python
from dcc_mcp_core import Capturer

capturer = Capturer.new_auto()

try:
    frame = capturer.capture(process_id=99999)  # Non-existent PID
except RuntimeError as e:
    print(f"Capture failed: {e}")
    # Handle error (window not found, backend unavailable, etc.)
```

## Platform Notes

### Windows

- Requires Windows 8 or later
- Desktop Duplication must be enabled
- DXGI backend provides GPU framebuffer access (<16ms per frame)
- May require running as administrator for some windows

### Linux

- Requires X11 display server (`DISPLAY` env var)
- X11 XShmGetImage backend
- Wayland support is planned

### macOS

- Uses Mock backend for testing
- Production capture requires platform-specific implementation

## Performance Tips

1. **Use appropriate formats** — PNG for quality, JPEG for speed, raw_bgra for processing
2. **Target specific windows** — Avoid full-screen capture when possible
3. **Use lower scale** for thumbnails/previews
4. **Mock backend in CI** — No GPU required, deterministic output
