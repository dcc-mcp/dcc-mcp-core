# Shared Memory API

`dcc_mcp_core` (shm 模块)

大型 DCC 场景数据的零拷贝共享内存传输。

## 概述

DCC 场景数据（几何体、动画缓存、帧缓冲区）可以轻松达到千兆字节。`dcc-mcp-shm` 提供了**零拷贝**替代方案：DCC 端将数据直接写入内存映射文件；消费者从同一映射区域读取，无需任何复制或序列化。

## SharedSceneBuffer

共享场景数据的高级包装器。

### 写入数据

```python
from dcc_mcp_core import PySharedSceneBuffer, SceneDataKind

# DCC 端：写入场景数据
vertices = bytes(1024 * 1024)  # 1 MiB 顶点数据
ssb = PySharedSceneBuffer.write(
    vertices,
    SceneDataKind.GEOMETRY,
    "Maya",  # DCC 名称
    True     # LZ4 压缩
)
```

### 读取数据

```python
# Agent 端：读取原始字节
recovered = ssb.read()
assert recovered == vertices
```

### 序列化

```python
# 通过 IPC 发送 JSON 描述符到 Agent 端
json_descriptor = ssb.to_descriptor_json()
print(json_descriptor)
```

## SceneDataKind

| 类型 | 描述 |
|------|------|
| `GEOMETRY` | 网格/顶点数据 |
| `TEXTURE` | 图像/纹理数据 |
| `ANIMATION` | 动画曲线 |
| `SCENE` | 完整场景状态 |
| `METADATA` | 场景元数据 |

## BufferPool

高性能场景的预分配缓冲区池。

```python
from dcc_mcp_core import PyBufferPool, BufferDescriptor

# 创建包含 4 个 256 MiB 缓冲区的池
pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)

# 从池中获取缓冲区
buffer = pool.acquire()

# 写入数据到缓冲区
buffer.write(data)

# 释放回池
pool.release(buffer)
```

### BufferDescriptor

```python
# 获取共享缓冲区信息
desc = buffer.descriptor()
print(desc.size)       # 缓冲区大小（字节）
print(desc.path)       # 内存映射文件路径
print(desc.offset)     # 缓冲区内的偏移量
```

## 分块传输

对于大于 256 MiB 的数据，自动使用分块传输。

```python
from dcc_mcp_core import ChunkManifest, DEFAULT_CHUNK_SIZE

# 大数据自动分块
ssb = PySharedSceneBuffer.write(large_data, SceneDataKind.SCENE)

# 获取传输的分块清单
manifest = ssb.chunk_manifest()
for chunk in manifest.chunks:
    print(f"分块 {chunk.index}: offset={chunk.offset}, size={chunk.size}")
```

## SharedBuffer（底层）

直接访问内存映射共享缓冲区。

### 创建 SharedBuffer

```python
from dcc_mcp_core import SharedBuffer

# 创建新的共享缓冲区
buffer = SharedBuffer.create(size=1024 * 1024)

# 通过路径打开现有缓冲区
buffer = SharedBuffer.open("/path/to/mmap/file")
```

### 读写

```python
# 写入字节到缓冲区
buffer.write(b"data", offset=0)

# 从缓冲区读取字节
data = buffer.read(offset=0, size=1024)

# 获取缓冲区元数据
print(buffer.size)
print(buffer.path)
```

## 错误处理

```python
from dcc_mcp_core import ShmError

try:
    buffer = SharedBuffer.create(size=-1)  # 无效大小
except ShmError as e:
    print(f"SHM 错误: {e}")
```

## 性能说明

- 零拷贝：使用内存映射文件时数据永不复制
- LZ4 压缩：可选压缩以 CPU 换传输时间
- 分块传输：大数据自动分割为 256 MiB 分块
- 池重用：缓冲区池消除重复传输的分配开销
