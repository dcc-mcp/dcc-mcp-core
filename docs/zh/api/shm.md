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
| `AnimationCache` | 动画缓存数据 |
| `Screenshot` | 截图/图像数据 |
| `Arbitrary` | 任意二进制数据 |

## PyBufferPool

高性能场景的预分配缓冲区池。

### 构造函数

```python
from dcc_mcp_core import PyBufferPool

pool = PyBufferPool(capacity=4, buffer_size=256 * 1024 * 1024)
```

参数：
- `capacity` — 池中缓冲区槽位数量
- `buffer_size` — 每个缓冲区的字节大小

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `acquire()` | `PySharedBuffer` | 从池中获取缓冲区（所有槽位占用时抛出 `RuntimeError`） |
| `available()` | `int` | 当前可用（空闲）槽位数 |
| `capacity()` | `int` | 缓冲区池总容量 |
| `buffer_size()` | `int` | 每个缓冲区的字节大小 |

### 示例

```python
pool = PyBufferPool(capacity=4, buffer_size=1024 * 1024)
buf = pool.acquire()
buf.write(b"scene snapshot")
# 缓冲区在 buf 被垃圾回收时自动归还池
print(pool.available())  # 3
```

## PySharedBuffer

内存映射共享缓冲区的直接访问。

### create()

```python
from dcc_mcp_core import PySharedBuffer

buf = PySharedBuffer.create(capacity=1024 * 1024)  # 1 MiB
buf_id = buf.id   # str 属性（非方法）
buf_path = buf.path()  # 底层内存映射文件路径
```

### open()

```python
# 跨进程传递：从路径 + id 重建
buf2 = PySharedBuffer.open(path=buf.path(), id=buf.id)
assert buf2.read() == buf.read()
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `write(data)` | `int` | 写入字节；返回已写入字节数。超出容量时抛出 `RuntimeError` |
| `read()` | `bytes` | 读取当前数据 |
| `data_len()` | `int` | 当前已存储字节数 |
| `capacity()` | `int` | 缓冲区最大字节容量 |
| `clear()` | `None` | 重置 data_len 为 0 |
| `path()` | `str` | 底层内存映射文件路径 |
| `descriptor_json()` | `str` | 用于跨进程传递的 JSON 描述符 |

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `id` | `str` | 唯一缓冲区标识符 |

## 性能说明

- 零拷贝：使用内存映射文件时数据永远不会被复制
- LZ4 压缩：可选压缩以 CPU 成本减少传输时间
- 池重用：缓冲区池消除重复传输的分配开销
