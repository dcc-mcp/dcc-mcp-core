# 共享内存 API

`dcc_mcp_core` (shm 模块)

大型 DCC 场景数据的零拷贝共享内存传输。

## 概述

DCC 场景数据（几何体、动画缓存、帧缓冲区）可以轻松达到千兆字节。`dcc-mcp-shm` 提供了**零拷贝**替代方案：DCC 端将数据直接写入内存映射文件；消费者从同一映射区域读取，无需任何复制或序列化。

## PySharedSceneBuffer

共享场景数据的高级包装器。

### write()

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

# DCC 端：写入场景数据
vertices = bytes(1024 * 1024)  # 1 MiB 顶点数据
ssb = PySharedSceneBuffer.write(
    data=vertices,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",
    use_compression=True
)
```

### read()

```python
# Agent 端：读回原始字节
recovered = ssb.read()
assert recovered == vertices
```

### descriptor_json()

```python
# 通过 IPC 发送 JSON 描述符到 Agent 端
json_descriptor = ssb.descriptor_json()
print(json_descriptor)
```

### from_descriptor_json()

```python
# 从 JSON 描述符重构
ssb = PySharedSceneBuffer.from_descriptor_json(json_descriptor)
data = ssb.read()
```

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `kind` | `PySceneDataKind` | 数据类型枚举 |
| `source_dcc` | `str` | 源 DCC 名称 |
| `use_compression` | `bool` | 是否使用 LZ4 压缩 |
| `size_bytes` | `int` | 大小（字节） |

## PySceneDataKind

对数据进行分类的枚举：

| 类型 | 描述 |
|------|------|
| `Geometry` | 网格/顶点数据 |
| `Texture` | 图像/纹理数据 |
| `Animation` | 动画曲线 |
| `Scene` | 完整场景状态 |
| `Metadata` | 场景元数据 |

## PyBufferPool

高性能场景的预分配缓冲区池。

### 构造函数

```python
from dcc_mcp_core import PyBufferPool

pool = PyBufferPool(capacity=4, size_bytes=256 * 1024 * 1024)
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `acquire()` | `PySharedBuffer` | 从池中获取缓冲区 |
| `release(buffer)` | `None` | 将缓冲区返还池中 |

### 示例

```python
# 从池中获取缓冲区
buffer = pool.acquire()

# 向缓冲区写入数据
buffer.write(b"data")

# 释放回池中
pool.release(buffer)
```

## PySharedBuffer

内存映射共享缓冲区的直接访问。

### create()

```python
from dcc_mcp_core import PySharedBuffer

buffer = PySharedBuffer.create(size_bytes=1024 * 1024)
buffer_id = buffer.buffer_id()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `write(data)` | `None` | 向缓冲区写入字节 |
| `read()` | `bytes` | 读取所有缓冲区数据 |

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `size_bytes` | `int` | 缓冲区大小 |
| `buffer_id()` | `str` | 唯一缓冲区标识符 |

## 性能说明

- 零拷贝：使用内存映射文件时数据永远不会被复制
- LZ4 压缩：可选压缩以 CPU 成本减少传输时间
- 池重用：缓冲区池消除重复传输的分配开销
