# Shared Memory Guide

Zero-copy shared memory transport for large DCC scene data.

## Overview

DCC scene data (geometry, animation caches, framebuffers) can easily reach gigabytes. Traditional Python-only DCC MCP integrations transmit this data by serializing and sending over TCP, which can take 10–30 seconds for a 1 GB scene.

`dcc-mcp-shm` provides a **zero-copy** alternative: the DCC side writes data directly into a memory-mapped file; the consumer reads from the same mapped region without any copying or serialization.

## Architecture

```
DCC Process                          Agent Process
     │                                     │
     ▼                                     │
┌─────────────┐                      ┌─────────────┐
│ SharedBuffer│◄───── mmap file ─────►│ SharedBuffer│
│   (write)  │     (no copying)      │   (read)    │
└─────────────┘                      └─────────────┘
```

## Quick Start

### Writing Scene Data (DCC Side)

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

# Large vertex data
vertices = open("scene.fbx", "rb").read()

# Write to shared memory
ssb = PySharedSceneBuffer.write(
    vertices,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",
    use_compression=True,  # LZ4 compression
)

# Send the descriptor JSON to agent via IPC
descriptor = ssb.descriptor_json()
send_to_agent(descriptor)
```

### Reading Scene Data (Agent Side)

```python
from dcc_mcp_core import PySharedSceneBuffer

# Receive descriptor from DCC
descriptor = receive_from_dcc()

# Reconstruct from JSON descriptor
# (Note: the buffer must already exist, this reopens the same memory mapping)
ssb = PySharedSceneBuffer.open(...)  # Use path/id from descriptor
data = ssb.read()
```

## PySharedSceneBuffer

High-level shared scene buffer for zero-copy DCC ↔ Agent data exchange.

Automatically selects inline vs chunked storage based on data size. Data larger than 256 MiB is split into chunks.

### write()

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

ssb = PySharedSceneBuffer.write(
    data=vertex_bytes,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",
    use_compression=True,
)
```

| Parameter | Type | Description |
|----------|------|-------------|
| `data` | `bytes` | Raw payload to store |
| `kind` | `PySceneDataKind` | Semantic kind: `Geometry`, `AnimationCache`, `Screenshot`, `Arbitrary` |
| `source_dcc` | `str \| None` | Originating DCC application name |
| `use_compression` | `bool` | Apply LZ4 compression before writing |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `str` | Transfer UUID string |
| `total_bytes` | `int` | Total original byte count |
| `is_inline` | `bool` | `True` if data is in a single buffer |
| `is_chunked` | `bool` | `True` if data spans multiple chunks |

### descriptor_json()

Returns a JSON descriptor string for cross-process handoff:

```python
desc = ssb.descriptor_json()
# Send desc to consumer via IPC
```

### read()

Reads stored data back (decompresses automatically if needed):

```python
data = ssb.read()
assert data == original_bytes
```

## PySceneDataKind

Categorize data for appropriate handling:

| Kind | Use Case |
|------|---------|
| `Geometry` | Mesh/vertex data |
| `AnimationCache` | Animation curves |
| `Screenshot` | Captured framebuffers |
| `Arbitrary` | Any other data (default) |

## Chunked Transfer

For data larger than 256 MiB, automatic chunking is used:

```python
# Large scene is automatically chunked
ssb = PySharedSceneBuffer.write(
    large_scene_data,
    kind=PySceneDataKind.Geometry,
)

# Check if chunked
print(f"Inline: {ssb.is_inline}, Chunked: {ssb.is_chunked}")
print(f"Total bytes: {ssb.total_bytes}")
```

## PyBufferPool

Pre-allocated buffer pool for high-performance scenarios (e.g. 30 fps scene snapshots).

### Creating a Pool

```python
from dcc_mcp_core import PyBufferPool

# 4 buffers of 256 MiB each
pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)
```

### Using Buffers

```python
# Acquire a buffer
buf = pool.acquire()
buf.write(b"scene snapshot")

# ... use buffer ...

# Release back to pool (GC calls __del__ automatically)
# Note: no explicit release() needed — buf is returned when garbage-collected
```

### Pool Properties

```python
print(f"Available: {pool.available()}")
print(f"Total capacity: {pool.capacity()}")
print(f"Buffer size: {pool.buffer_size()}")
```

## PySharedBuffer (Low-Level)

Direct access to memory-mapped shared buffers.

### Creating a Buffer

```python
from dcc_mcp_core import PySharedBuffer

# Create a new shared buffer
buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB

# Open an existing buffer by path and id
buf2 = PySharedBuffer.open(path="/path/to/mmap/file", id="buffer-uuid")
```

### Reading and Writing

```python
# Write bytes
n = buf.write(b"vertex data")
assert n == len(b"vertex data")

# Read back
data = buf.read()
assert data == b"vertex data"

# Check data length
print(f"Data length: {buf.data_len()} bytes")
print(f"Capacity: {buf.capacity()} bytes")

# Clear
buf.clear()
assert buf.data_len() == 0
```

### Properties

```python
print(f"ID: {buf.id}")
print(f"Path: {buf.path()}")  # Memory-mapped file path
print(f"Descriptor: {buf.descriptor_json()}")
```

## Compression

LZ4 compression is optional. Reduces transfer time at CPU cost.

```python
# With compression (slower write, faster network transfer for large data)
ssb = PySharedSceneBuffer.write(
    data, PySceneDataKind.Geometry, use_compression=True
)

# Without compression (faster write, slower network transfer)
ssb = PySharedSceneBuffer.write(
    data, PySceneDataKind.Geometry, use_compression=False
)
```

### When to Use Compression

| Scenario | Recommendation |
|----------|----------------|
| Large geometry (>100 MB) | Use compression |
| Already compressed data (images, etc.) | Skip compression |
| Time-critical capture | Skip compression |
| Low-bandwidth links | Use compression |

## Performance Comparison

| Method | 100 MB Transfer | 1 GB Transfer |
|--------|-----------------|----------------|
| TCP serialization | ~10–15s | ~100–300s |
| Shared memory | ~0.1s | ~0.5–2s |

## Error Handling

```python
from dcc_mcp_core import PySharedBuffer

try:
    buf = PySharedBuffer.create(capacity=-1)  # Invalid
except RuntimeError as e:
    print(f"Shared memory error: {e}")
```

## Use Cases

### Maya Geometry Export

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind
import maya.cmds as cmds

def export_selection_to_agent():
    # Get selected geometry
    selection = cmds.ls(selection=True, type="mesh")
    if not selection:
        return None

    # Export to binary
    data = export_meshes_binary(selection)

    # Write to shared memory
    ssb = PySharedSceneBuffer.write(
        data,
        kind=PySceneDataKind.Geometry,
        source_dcc="Maya",
        use_compression=True,
    )

    return ssb.descriptor_json()
```

### Houdini Scene Transfer

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind
import hou

def share_houdini_scene():
    scene_data = hou.exportScene("/tmp/scene.bin", binary=True)

    ssb = PySharedSceneBuffer.write(
        scene_data,
        kind=PySceneDataKind.Geometry,
        source_dcc="Houdini",
        use_compression=False,
    )

    return ssb.descriptor_json()
```

## Platform Notes

### Windows

- Uses `CreateFileMapping` / `MapViewOfFile`
- Named pipes for cross-process communication
- Maximum file size: 256 TB (theoretical)

### Linux

- Uses `shm_open` / `mmap`
- POSIX shared memory objects
- Maximum file size: limited by filesystem

### macOS

- Uses `mmap` with anonymous memory
- Cross-process via POSIX shared memory
