# Capture Guide

Screen capture for DCC applications.

## Overview

The capture module provides GPU framebuffer screenshot and frame capture functionality for DCC applications. It supports multiple backends depending on the platform:

- **Windows** — DXGI Desktop Duplication API
- **Linux** — X11 XShmGetImage
- **All platforms** — Mock backend (synthetic checkerboard for testing)

## Architecture

```
Capturer (high-level API)
    └── DccCapture trait (backend abstraction)
            ├── DxgiBackend    (Windows)
            ├── X11Backend     (Linux)
            └── MockBackend    (all platforms)
```

## Quick Start

### Capturing the Screen

```python
from dcc_mcp_core import PyCapturer

# Create capturer with best available backend
capturer = PyCapturer.new_auto()

# Capture a frame
frame = capturer.capture()

# Save to file
with open("screenshot.png", "wb") as f:
    f.write(frame.data)
```

### Targeting Specific Windows

```python
from dcc_mcp_core import PyCapturer, CaptureTarget

capturer = PyCapturer.new_auto()

# Capture a specific window by title
target = CaptureTarget.window("Maya")
frame = capturer.capture(target=target, format="png")

# Capture by process name
target = CaptureTarget.process("maya")
frame = capturer.capture(target=target)

# Capture primary monitor
frame = capturer.capture_primary_monitor()
```

## Finding Windows

```python
from dcc_mcp_core import WindowFinder

finder = WindowFinder()

# Find windows by title (partial match)
windows = finder.find_windows("Maya")
for win in windows:
    print(f"Title: {win.title}")
    print(f"Handle: {win.window_id}")
    print(f"Bounds: {win.rect}")

# Find windows by process name
maya_windows = finder.find_by_process("maya")

# Get currently focused window
foreground = finder.get_foreground()
```

## Capture Formats

### PNG (Lossless)

```python
# Best quality, larger file size
frame = capturer.capture(format="png")
```

### JPEG (Lossy)

```python
# Smaller file size, some quality loss
frame = capturer.capture(format="jpg")
```

### Raw RGBA

```python
# Raw pixel data for processing
frame = capturer.capture(format="rgba")
print(f"Size: {frame.width}x{frame.height}x{frame.bytes_per_pixel}")
```

## CaptureFrame Properties

```python
frame = capturer.capture()

# Dimensions
print(f"Width: {frame.width}")
print(f"Height: {frame.height}")

# Pixel format
print(f"Bytes per pixel: {frame.bytes_per_pixel}")

# Raw data
print(f"Data length: {len(frame.data)} bytes")
```

## Use Cases

### Screenshot for AI Analysis

```python
from dcc_mcp_core import PyCapturer

def capture_for_ai():
    capturer = PyCapturer.new_auto()
    frame = capturer.capture(format="png")

    # Send to AI service for analysis
    response = ai_service.analyze(frame.data)
    return response
```

### Real-time Preview Stream

```python
import time
from dcc_mcp_core import PyCapturer

def preview_stream(fps=30):
    capturer = PyCapturer.new_auto()
    interval = 1.0 / fps

    while True:
        frame = capturer.capture()
        # Stream frame...
        time.sleep(interval)
```

### Window Monitoring

```python
from dcc_mcp_core import WindowFinder, PyCapturer

def monitor_window(window_title):
    capturer = PyCapturer.new_auto()
    finder = WindowFinder()

    windows = finder.find_windows(window_title)
    if not windows:
        return None

    target = CaptureTarget.window(window_title)
    return capturer.capture(target=target)
```

## Performance Tips

1. **Use appropriate formats** — PNG for quality, JPEG for speed
2. **Target specific windows** — Avoid full-screen capture when possible
3. **Use RGBA for processing** — Avoid format conversion overhead
4. **Cache WindowFinder results** — Window enumeration is expensive

## Backend Selection

The `new_auto()` method probes backends in priority order:

```python
# Priority order on Windows:
# 1. DXGI (if available and Desktop Duplication supported)
# 2. Mock (fallback for testing)

# Priority order on Linux:
# 1. X11 (if DISPLAY is set)
# 2. Mock (always available)
```

### Manual Backend Selection

```python
from dcc_mcp_core import CaptureBackendKind

# Check available backends
capturer = PyCapturer.new_auto()
stats = capturer.stats()
print(f"Backend: {stats['backend']}")
```

## Error Handling

```python
from dcc_mcp_core import CaptureError

try:
    frame = capturer.capture()
except CaptureError as e:
    print(f"Capture failed: {e}")
    # Handle error (e.g., window not found, backend unavailable)
```

## Platform Notes

### Windows

- Requires Windows 8 or later
- Desktop Duplication must be enabled
- May require running as administrator for some windows

### Linux

- Requires X11 display server
- `xdotool` or similar for window enumeration
- Wayland support is planned

### macOS

- Uses Mock backend for testing
- Production capture requires platform-specific implementation
