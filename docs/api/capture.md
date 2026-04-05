# Capture API

`dcc_mcp_core.PyCapturer`

Screen capture for DCC applications using platform-specific backends.

## Capturer

High-level capturer wrapper with automatic backend selection.

### Constructor

```python
from dcc_mcp_core import PyCapturer
capturer = PyCapturer.new_auto()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `new_auto()` | `PyCapturer` | Create capturer with best available backend |
| `capture(target=None, format="png")` | `PyCaptureFrame` | Capture a frame from the target |
| `capture_window(window_id, format="png")` | `PyCaptureFrame` | Capture a specific window |
| `capture_primary_monitor(format="png")` | `PyCaptureFrame` | Capture the primary monitor |
| `stats()` | `dict` | Get capture statistics |

### CaptureFrame

```python
frame = capturer.capture(format="png")
print(frame.width, frame.height)  # Frame dimensions
print(frame.bytes_per_pixel)      # Bytes per pixel
print(frame.data)                # Raw frame data as bytes
```

### CaptureFormat

| Format | Description |
|--------|-------------|
| `png` | PNG image format (lossless, larger) |
| `jpg` | JPEG image format (lossy, smaller) |
| `rgba` | Raw RGBA bytes |

### CaptureTarget

```python
from dcc_mcp_core import CaptureTarget

# Capture by window title (partial match)
target = CaptureTarget.window("Maya")

# Capture by process name
target = CaptureTarget.process("maya")

# Capture a specific monitor
target = CaptureTarget.monitor(index=0)
```

## WindowFinder

Find windows for capture targeting.

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `find_windows(title_contains)` | `List[WindowInfo]` | Find windows by title |
| `find_by_process(name)` | `List[WindowInfo]` | Find windows by process name |
| `get_foreground()` | `WindowInfo` | Get the currently focused window |

### WindowInfo

```python
finder = WindowFinder()
windows = finder.find_windows("Maya")
for win in windows:
    print(win.window_id)      # Platform-specific window ID
    print(win.title)          # Window title
    print(win.process_name)   # Process name
    print(win.rect)           # Window bounds (x, y, width, height)
```

## Backends

| Backend | Platform | Description |
|---------|----------|-------------|
| `dxgi` | Windows | DXGI Desktop Duplication API |
| `x11` | Linux | X11 XShmGetImage |
| `mock` | All | Synthetic checkerboard for testing |

## Error Handling

```python
from dcc_mcp_core import CaptureError

try:
    frame = capturer.capture()
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
