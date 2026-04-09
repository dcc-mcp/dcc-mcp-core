# 共享内存指南

大型 DCC 场景数据的零拷贝共享内存传输。

## 概述

DCC 场景数据（几何体、动画缓存、帧缓冲区）可以轻松达到千兆字节。传统的纯 Python DCC MCP 集成通过 TCP 序列化和发送数据，1 GB 场景可能需要 10-30 秒。

`dcc-mcp-shm` 提供了**零拷贝**替代方案：DCC 端将数据直接写入内存映射文件；消费者从同一映射区域读取，无需任何复制或序列化。

## 架构

```
DCC 进程                          Agent 进程
     │                                     │
     ▼                                     │
┌─────────────┐                      ┌─────────────┐
│ SharedBuffer│◄───── mmap 文件 ─────►│ SharedBuffer│
│   (写入)    │      (无复制)         │   (读取)    │
└─────────────┘                      └─────────────┘
```

## 快速开始

### 写入场景数据（DCC 端）

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

# 大顶点数据
vertices = open("scene.fbx", "rb").read()

# 写入共享内存
ssb = PySharedSceneBuffer.write(
    data=vertices,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",  # DCC 名称
    use_compression=True  # 启用 LZ4 压缩
)

# 通过 IPC 发送描述符 JSON 到 agent
descriptor_json = ssb.descriptor_json()
send_to_agent(descriptor_json)
```

### 读取场景数据（Agent 端）

```python
from dcc_mcp_core import PySharedSceneBuffer

# 从 DCC 接收描述符
descriptor_json = receive_from_dcc()

# 打开共享缓冲区并读取
ssb = PySharedSceneBuffer.from_descriptor_json(descriptor_json)
vertices = ssb.read()
```

## PySharedSceneBuffer

共享场景数据的主要接口。

### 写入数据

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

# 简单写入
ssb = PySharedSceneBuffer.write(
    data=vertex_data,
    kind=PySceneDataKind.Geometry,
    source_dcc="Maya",
    use_compression=True
)

# 获取描述符 JSON
descriptor = ssb.descriptor_json()
```

### 读取数据

```python
# 读取所有数据
data = ssb.read()

# 获取元数据
print(f"类型: {ssb.kind}")
print(f"DCC: {ssb.source_dcc}")
print(f"压缩: {ssb.use_compression}")
print(f"大小: {ssb.size_bytes} 字节")
```

### IPC 序列化

```python
# 导出 IPC 传输描述符
json_str = ssb.descriptor_json()

# 从描述符重构
ssb = PySharedSceneBuffer.from_descriptor_json(json_str)
```

## PySceneDataKind

对数据进行分类以便适当处理：

| 类型 | 用例 | 典型大小 |
|------|------|----------|
| `Geometry` | 网格/顶点数据 | 1 MB - 1 GB |
| `Texture` | 图像/纹理 | 1 MB - 100 MB |
| `Animation` | 动画曲线 | 100 KB - 100 MB |
| `Scene` | 完整场景状态 | 10 MB - 1 GB |
| `Metadata` | 场景元数据 | 1 KB - 1 MB |

## PyBufferPool

高性能场景的预分配缓冲区池。

### 创建池

```python
from dcc_mcp_core import PyBufferPool

# 4 个 256 MiB 的缓冲区
pool = PyBufferPool(capacity=4, size_bytes=256 * 1024 * 1024)
```

### 使用缓冲区

```python
# 从池中获取缓冲区
buffer = pool.acquire()

# 写入数据
buffer.write(large_data)

# ... 使用缓冲区 ...

# 释放回池（不销毁）
pool.release(buffer)
```

### 缓冲区属性

```python
buffer = pool.acquire()
print(f"大小: {buffer.size_bytes}")
print(f"已用: {buffer.used_bytes}")
```

## PySharedBuffer

低级共享内存接口。

### 创建共享缓冲区

```python
from dcc_mcp_core import PySharedBuffer

# 创建指定大小的缓冲区
buffer = PySharedBuffer.create(size_bytes=1024 * 1024)

# 获取缓冲区 ID
buffer_id = buffer.buffer_id()
print(f"缓冲区 ID: {buffer_id}")
```

### 读取/写入

```python
# 写入数据
buffer.write(b"Hello, World!")

# 读取数据
data = buffer.read()
print(data.decode())
```

## 压缩

LZ4 压缩是可选的，用于减少传输时间。

```python
# 使用压缩（写入较慢，大数据传输较快）
ssb = PySharedSceneBuffer.write(
    data, PySceneDataKind.Geometry, "Maya", use_compression=True
)

# 不使用压缩（写入较快，传输较慢）
ssb = PySharedSceneBuffer.write(
    data, PySceneDataKind.Geometry, "Maya", use_compression=False
)
```

## 性能对比

| 方法 | 100 MB 传输 | 1 GB 传输 |
|------|-------------|-----------|
| TCP 序列化 | ~10-15秒 | ~100-300秒 |
| 共享内存 | ~0.1秒 | ~0.5-2秒 |

## 使用场景

### Maya 几何体导出

```python
from dcc_mcp_core import PySharedSceneBuffer, PySceneDataKind

def export_selection_to_agent():
    import maya.cmds as cmds

    # 获取选中的几何体
    selection = cmds.ls(selection=True, type="mesh")
    if not selection:
        return None

    # 导出为二进制
    data = export_meshes_binary(selection)

    # 写入共享内存
    ssb = PySharedSceneBuffer.write(
        data,
        PySceneDataKind.Geometry,
        "Maya",
        use_compression=True
    )

    return ssb.descriptor_json()
```

## 平台说明

### Windows

- 使用 `CreateFileMapping` / `MapViewOfFile`
- 命名管道用于跨进程通信
- 最大文件大小：256 TB（理论）

### Linux

- 使用 `shm_open` / `mmap`
- POSIX 共享内存对象
- 最大文件大小：受文件系统限制

### macOS

- 使用 `mmap` 和匿名内存
- 通过 POSIX 共享内存跨进程
