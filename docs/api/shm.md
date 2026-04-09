# Shared Memory API

`dcc_mcp_core` (shm module)

Zero-copy shared memory transport for large DCC scene data.

## Overview

DCC scene data (geometry, animation caches, framebuffers) can easily reach gigabytes. `dcc-mcp-shm` provides a **zero-copy** alternative: the DCC side writes data directly into a memory-mapped file; the consumer reads from the same mapped region without any copying or serialization.

## PySharedSceneBuffer

High-level wrapper for shared scene data.

### write()

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

# DCC side: write scene data
vertices = bytes(1024 * 1024)  # 1 MiB of vertex data
ssb = PySharedSceneBuffer.write(
    data=vertices,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",
    use_compression=True
)
```

### read()

```python
# Agent side: read back the original bytes
recovered = ssb.read()
assert recovered == vertices
```

### descriptor_json()

```python
# Send JSON descriptor to the Agent side via IPC
json_descriptor = ssb.descriptor_json()
print(json_descriptor)
```

### from_descriptor_json()

```python
# Reconstruct from JSON descriptor
ssb = PySharedSceneBuffer.from_descriptor_json(json_descriptor)
data = ssb.read()
```

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `kind` | `PySceneDataKind` | Data type enum |
| `source_dcc` | `str` | Source DCC name |
| `use_compression` | `bool` | Whether LZ4 compression is used |
| `size_bytes` | `int` | Size in bytes |

## PySceneDataKind

Enum for classifying data:

| Kind | Description |
|------|-------------|
| `Geometry` | Mesh/vertex data |
| `Texture` | Image/texture data |
| `Animation` | Animation curves |
| `Scene` | Full scene state |
| `Metadata` | Scene metadata |

## PyBufferPool

Pre-allocated buffer pool for high-performance scenarios.

### Constructor

```python
from dcc_mcp_core import PyBufferPool

pool = PyBufferPool(capacity=4, size_bytes=256 * 1024 * 1024)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `acquire()` | `PySharedBuffer` | Get a buffer from the pool |
| `release(buffer)` | `None` | Return buffer to the pool |

### Example

```python
# Acquire a buffer from the pool
buffer = pool.acquire()

# Write data to the buffer
buffer.write(b"data")

# Release back to the pool
pool.release(buffer)
```

## PySharedBuffer

Direct access to memory-mapped shared buffers.

### create()

```python
from dcc_mcp_core import PySharedBuffer

buffer = PySharedBuffer.create(size_bytes=1024 * 1024)
buffer_id = buffer.buffer_id()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `write(data)` | `None` | Write bytes to buffer |
| `read()` | `bytes` | Read all buffer data |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `size_bytes` | `int` | Buffer size |
| `buffer_id()` | `str` | Unique buffer identifier |

## Performance Notes

- Zero-copy: Data is never copied when using memory-mapped files
- LZ4 compression: Optional compression reduces transfer time at CPU cost
- Pool reuse: Buffer pools eliminate allocation overhead for repeated transfers
