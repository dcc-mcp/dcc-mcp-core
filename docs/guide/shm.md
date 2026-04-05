# Shared Memory Guide

Zero-copy shared memory transport for large DCC scene data.

## Overview

DCC scene data (geometry, animation caches, framebuffers) can easily reach gigabytes. Traditional Python-only DCC MCP integrations transmit this data by serializing and sending over TCP, which can take 10-30 seconds for a 1 GB scene.

`dcc-mcp-shm` provides a **zero-copy** alternative: the DCC side writes data directly into a memory-mapped file; the consumer reads from the same mapped region without any copying or serialization.

## Architecture

```
DCC Process                          Agent Process
     │                                     │
     ▼                                     │
┌─────────────┐                      ┌─────────────┐
│ SharedBuffer│◄───── mmap file ─────►│ SharedBuffer│
│   (write)   │     (no copying)      │   (read)    │
└─────────────┘                      └─────────────┘
```

## Quick Start

### Writing Scene Data (DCC Side)

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

# Large vertex data
vertices = open("scene.fbx", "rb").read()

# Write to shared memory
ssb = PySharedSceneBuffer.write(
    vertices,
    SceneDataKind.GEOMETRY,
    "Maya",  # DCC name
    True     # Enable LZ4 compression
)

# Send the descriptor JSON to agent via IPC
descriptor = ssb.to_descriptor_json()
send_to_agent(descriptor)
```

### Reading Scene Data (Agent Side)

```python
from dcc_mcp_core import PySharedSceneBuffer

# Receive descriptor from DCC
descriptor = receive_from_dcc()

# Open shared buffer and read
ssb = PySharedSceneBuffer.from_descriptor_json(descriptor)
vertices = ssb.read()
```

## SharedSceneBuffer

The main interface for shared scene data.

### Writing Data

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

# Simple write
ssb = PySharedSceneBuffer.write(
    data=vertex_data,
    kind=SceneDataKind.GEOMETRY,
    dcc_name="Maya",
    compress=True
)
```

### Reading Data

```python
# Read all data
data = ssb.read()

# Get metadata
print(f"Kind: {ssb.kind}")
print(f"DCC: {ssb.dcc_name}")
print(f"Compressed: {ssb.compressed}")
print(f"Size: {ssb.size_bytes} bytes")
```

### Serialization for IPC

```python
# Export descriptor for IPC transmission
json_str = ssb.to_descriptor_json()

# Reconstruct from descriptor
ssb = PySharedSceneBuffer.from_descriptor_json(json_str)
```

## SceneDataKind

Categorize data for appropriate handling:

| Kind | Use Case | Typical Size |
|------|----------|--------------|
| `GEOMETRY` | Mesh/vertex data | 1 MB - 1 GB |
| `TEXTURE` | Images/textures | 1 MB - 100 MB |
| `ANIMATION` | Animation curves | 100 KB - 100 MB |
| `SCENE` | Full scene state | 10 MB - 1 GB |
| `METADATA` | Scene metadata | 1 KB - 1 MB |

## Chunked Transfer

For data larger than 256 MiB, automatic chunking is used.

```python
# Large scene is automatically chunked
ssb = PySharedSceneBuffer.write(large_scene_data, SceneDataKind.SCENE)

# Get chunk manifest
manifest = ssb.chunk_manifest()
print(f"Chunks: {len(manifest.chunks)}")
for chunk in manifest.chunks:
    print(f"  [{chunk.index}] offset={chunk.offset}, size={chunk.size}")
```

## BufferPool

Pre-allocated buffer pool for high-performance scenarios.

### Creating a Pool

```python
from dcc_mcp_core import PyBufferPool

# 4 buffers of 256 MiB each
pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)
```

### Using Buffers

```python
# Acquire a buffer from the pool
buffer = pool.acquire()

# Write data
buffer.write(large_data)

# ... use buffer ...

# Release back to pool (don't destroy)
pool.release(buffer)
```

### Buffer Reuse

```python
# Acquire, use, release, acquire again
for i in range(10):
    buffer = pool.acquire()
    buffer.write(process_data(i))
    send_to_agent(buffer)
    pool.release(buffer)
```

## Compression

LZ4 compression is optional for reducing transfer time.

```python
# With compression (slower write, faster transfer for large data)
ssb = PySharedSceneBuffer.write(data, SceneDataKind.GEOMETRY, compress=True)

# Without compression (faster write, slower transfer)
ssb = PySharedSceneBuffer.write(data, SceneDataKind.GEOMETRY, compress=False)
```

### When to Use Compression

| Scenario | Recommendation |
|----------|-----------------|
| Large geometry (>100 MB) | Use compression |
| Already compressed data | Skip compression |
| Time-critical capture | Skip compression |
| Low-bandwidth links | Use compression |

## Performance Comparison

| Method | 100 MB Transfer | 1 GB Transfer |
|--------|-----------------|---------------|
| TCP serialization | ~10-15s | ~100-300s |
| Shared memory | ~0.1s | ~0.5-2s |

## Use Cases

### Maya Geometry Export

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

def export_selection_to_agent():
    import maya.cmds as cmds

    # Get selected geometry
    selection = cmds.ls(selection=True, type="mesh")
    if not selection:
        return None

    # Export to binary
    data = export_meshes_binary(selection)

    # Write to shared memory
    ssb = PySharedSceneBuffer.write(
        data,
        SceneDataKind.GEOMETRY,
        "Maya",
        compress=True
    )

    return ssb.to_descriptor_json()
```

### Houdini Scene Transfer

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

def share_houdini_scene():
    import hou

    # Export scene to binary
    scene_data = hou.exportScene("/tmp/scene.bin", binary=True)

    # Share via SHM
    ssb = PySharedSceneBuffer.write(
        scene_data,
        SceneDataKind.SCENE,
        "Houdini",
        compress=False
    )

    return ssb.to_descriptor_json()
```

## Error Handling

```python
from dcc_mcp_core import ShmError

try:
    # Try to create a massive buffer
    ssb = PySharedSceneBuffer.write(huge_data, SceneDataKind.SCENE)
except ShmError as e:
    print(f"Shared memory error: {e}")
    # Fallback to TCP transfer
    fallback_transfer(huge_data)
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
