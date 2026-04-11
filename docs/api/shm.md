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
| `AnimationCache` | Animation cache data |
| `Screenshot` | Screenshot/image data |
| `Arbitrary` | Arbitrary binary data |

## PyBufferPool

Pre-allocated buffer pool for high-performance scenarios.

### Constructor

```python
from dcc_mcp_core import PyBufferPool

pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)
```

Parameters:
- `capacity` — number of buffer slots in the pool
- `buffer_size` — size in bytes of each individual buffer

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `acquire()` | `PySharedBuffer` | Get a buffer from the pool (raises `RuntimeError` if all slots in use) |
| `available()` | `int` | Number of currently free slots |
| `capacity()` | `int` | Total pool capacity |
| `buffer_size()` | `int` | Per-buffer size in bytes |

### Example

```python
pool = PyBufferPool(capacity=4, buffer_size=1024 * 1024)
buf = pool.acquire()
buf.write(b"scene snapshot")
# Buffer is returned to the pool when `buf` is garbage-collected
print(pool.available())  # 3
```

## PySharedBuffer

Direct access to named memory-mapped shared buffers.

### create()

```python
from dcc_mcp_core import PySharedBuffer

buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB
buf_id = buf.id   # str property (not a method)
buf_path = buf.path()  # file path of the backing mmap file
```

### open()

```python
# Cross-process handoff: reconstruct from path + id
buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
assert buf2.read() == buf.read()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `write(data)` | `int` | Write bytes; returns bytes written. Raises `RuntimeError` if data exceeds capacity |
| `read()` | `bytes` | Read current data |
| `data_len()` | `int` | Bytes currently stored |
| `capacity()` | `int` | Maximum bytes this buffer can hold |
| `clear()` | `None` | Reset data_len to 0 |
| `path()` | `str` | File path of backing mmap file |
| `descriptor_json()` | `str` | JSON descriptor for cross-process handoff |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `id` | `str` | Unique buffer identifier |

## Performance Notes

- Zero-copy: Data is never copied when using memory-mapped files
- LZ4 compression: Optional compression reduces transfer time at CPU cost
- Pool reuse: Buffer pools eliminate allocation overhead for repeated transfers
