# Shared Memory API

`dcc_mcp_core` (shm module)

Zero-copy shared memory transport for large DCC scene data.

## Overview

DCC scene data (geometry, animation caches, framebuffers) can easily reach gigabytes. `dcc-mcp-shm` provides a **zero-copy** alternative: the DCC side writes data directly into a memory-mapped file; the consumer reads from the same mapped region without any copying or serialization.

## SharedSceneBuffer

High-level wrapper for shared scene data.

### Writing Data

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

# DCC side: write scene data
vertices = bytes(1024 * 1024)  # 1 MiB of vertex data
ssb = PySharedSceneBuffer.write(
    vertices,
    SceneDataKind.GEOMETRY,
    "Maya",  # DCC name
    True     # LZ4 compression
)
```

### Reading Data

```python
# Agent side: read back the original bytes
recovered = ssb.read()
assert recovered == vertices
```

### Serialization

```python
# Send JSON descriptor to the Agent side via IPC
json_descriptor = ssb.to_descriptor_json()
print(json_descriptor)
```

## SceneDataKind

| Kind | Description |
|------|-------------|
| `GEOMETRY` | Mesh/vertex data |
| `TEXTURE` | Image/texture data |
| `ANIMATION` | Animation curves |
| `SCENE` | Full scene state |
| `METADATA` | Scene metadata |

## BufferPool

Pre-allocated buffer pool for high-performance scenarios.

```python
from dcc_mcp_core import PyBufferPool, BufferDescriptor

# Create a pool with 4 buffers of 256 MiB each
pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)

# Acquire a buffer from the pool
buffer = pool.acquire()

# Write data to the buffer
buffer.write(data)

# Release back to the pool
pool.release(buffer)
```

### BufferDescriptor

```python
# Get information about a shared buffer
desc = buffer.descriptor()
print(desc.size)       # Buffer size in bytes
print(desc.path)       # Memory-mapped file path
print(desc.offset)     # Offset within the buffer
```

## Chunked Transfer

For data larger than 256 MiB, chunked transfer is used automatically.

```python
from dcc_mcp_core import ChunkManifest, DEFAULT_CHUNK_SIZE

# Large data is automatically chunked
ssb = PySharedSceneBuffer.write(large_data, SceneDataKind.SCENE)

# Get the chunk manifest for transmission
manifest = ssb.chunk_manifest()
for chunk in manifest.chunks:
    print(f"Chunk {chunk.index}: offset={chunk.offset}, size={chunk.size}")
```

## SharedBuffer (Low-Level)

Direct access to memory-mapped shared buffers.

### Creating a SharedBuffer

```python
from dcc_mcp_core import SharedBuffer

# Create a new shared buffer
buffer = SharedBuffer.create(size=1024 * 1024)

# Open an existing buffer by path
buffer = SharedBuffer.open("/path/to/mmap/file")
```

### Reading and Writing

```python
# Write bytes to the buffer
buffer.write(b"data", offset=0)

# Read bytes from the buffer
data = buffer.read(offset=0, size=1024)

# Get buffer metadata
print(buffer.size)
print(buffer.path)
```

## Error Handling

```python
from dcc_mcp_core import ShmError

try:
    buffer = SharedBuffer.create(size=-1)  # Invalid size
except ShmError as e:
    print(f"SHM error: {e}")
```

## Performance Notes

- Zero-copy: Data is never copied when using memory-mapped files
- LZ4 compression: Optional compression reduces transfer time at CPU cost
- Chunked transfer: Large data is split into 256 MiB chunks automatically
- Pool reuse: Buffer pools eliminate allocation overhead for repeated transfers
