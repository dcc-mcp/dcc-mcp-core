# 传输层

> **v0.14 已替换遗留传输栈（issue #251）。**
>
> 旧版类 — `TransportManager`、`FramedChannel`、`FramedIo`、
> `IpcListener`（Python 版）、`ListenerHandle`、`RoutingStrategy`、
> `ConnectionPool`、`InstanceRouter`、`CircuitBreaker`、`MessageEnvelope`、
> `encode_request` / `encode_response` / `encode_notify` / `decode_envelope`、
> `connect_ipc` — 均已移除。请使用下方基于 `ipckit` 的 DccLink 适配器。

传输层（`dcc-mcp-transport` crate）提供 MCP 服务器与 DCC 应用实例之间的 IPC 通信，使用 DccLink 帧格式通过 Named Pipe（Windows）或 Unix Domain Socket（macOS/Linux）传输。

## 概览

新传输 API 围绕 **DccLink 适配器** 构建 —— 这是对 `ipckit` IPC 通道的轻量封装，使用二进制线格式（`[u32 len][u8 type][u64 seq][msgpack body]`）实现高效帧通信。

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

# 服务端：创建命名通道并等待客户端
server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

# 客户端：连接到服务端
client = IpcChannelAdapter.connect("my-dcc")

# 发送帧
frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
client.send_frame(frame)

# 接收帧
received = server.recv_frame()
print(received.body)  # b"hello"
```

## DccLinkFrame

DCC-Link 协议的二进制线帧。线格式：`[u32 len][u8 type][u64 seq][msgpack body]`。

### 消息类型

| 标签 | 类型 | 说明 |
|------|------|------|
| 1 | Call | 请求调用 |
| 2 | Reply | 成功响应 |
| 3 | Err | 错误响应 |
| 4 | Progress | 进度更新 |
| 5 | Cancel | 取消信号 |
| 6 | Push | 服务端推送消息 |
| 7 | Ping | 心跳请求 |
| 8 | Pong | 心跳响应 |

### 构造函数

```python
from dcc_mcp_core import DccLinkFrame

frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
```

| 参数 | 类型 | 说明 |
|------|------|------|
| `msg_type` | `int` | 消息类型标签（1-8）|
| `seq` | `int` | 序列号 |
| `body` | `bytes \| None` | 载荷字节（默认 `b""`）|

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `msg_type` | `int` | 消息类型标签（1=Call, 2=Reply, 3=Err, 4=Progress, 5=Cancel, 6=Push, 7=Ping, 8=Pong）|
| `seq` | `int` | 序列号 |
| `body` | `bytes` | 载荷字节 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `encode()` | `bytes` | 将帧编码为 `[len][type][seq][body]` 字节 |
| `decode(data)` | `DccLinkFrame` | 从包含 4 字节长度前缀的字节中解码帧（静态方法）|

```python
frame = DccLinkFrame(msg_type=1, seq=0, body=b"payload")
encoded = frame.encode()
decoded = DccLinkFrame.decode(encoded)
assert decoded.msg_type == frame.msg_type
assert decoded.seq == frame.seq
assert decoded.body == frame.body
```

## IpcChannelAdapter

基于 `ipckit::IpcChannel` 的轻量适配器，使用 DCC-Link 帧格式。支持通过 Named Pipe（Windows）或 Unix Domain Socket（macOS/Linux）的 1:1 连接。

### 创建服务端

```python
from dcc_mcp_core import IpcChannelAdapter

server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()  # 阻塞等待客户端连接
```

### 作为客户端连接

```python
from dcc_mcp_core import IpcChannelAdapter

client = IpcChannelAdapter.connect("my-dcc")
```

### 发送和接收帧

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

# 服务端
server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

# 客户端
client = IpcChannelAdapter.connect("my-dcc")

# 客户端发送 Call 帧
call_frame = DccLinkFrame(msg_type=1, seq=0, body=b"execute_python")
client.send_frame(call_frame)

# 服务端接收帧
received = server.recv_frame()  # 阻塞；通道关闭时返回 None
if received is not None:
    print(received.msg_type)  # 1
    print(received.body)      # b"execute_python"

    # 服务端发送 Reply 帧
    reply = DccLinkFrame(msg_type=2, seq=0, body=b"ok")
    server.send_frame(reply)

# 客户端接收回复
response = client.recv_frame()
```

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `create(name)` | `IpcChannelAdapter` | 创建服务端 IPC 通道 |
| `connect(name)` | `IpcChannelAdapter` | 连接到已有的 IPC 通道 |

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `wait_for_client()` | `None` | 等待客户端连接（仅服务端）|
| `send_frame(frame)` | `None` | 向对端发送 `DccLinkFrame` |
| `recv_frame()` | `DccLinkFrame \| None` | 接收帧（阻塞）。通道关闭时返回 `None` |

## GracefulIpcChannelAdapter

在 `IpcChannelAdapter` 基础上增加了优雅关闭和 DCC 主线程集成。适用于需要在主线程处理 IPC 消息而不阻塞的 DCC 插件。

### 创建优雅服务端

```python
from dcc_mcp_core import GracefulIpcChannelAdapter

server = GracefulIpcChannelAdapter.create("my-dcc")
server.bind_affinity_thread()  # 在 DCC 主线程上调用一次
server.wait_for_client()
```

### 在主线程上处理消息

在 DCC 应用中，IPC 消息通常需要在主线程上处理。在空闲回调中使用 `pump_pending()`：

```python
# Maya 示例：使用 scriptJob idleEvent
import maya.cmds as cmds

def on_idle():
    processed = server.pump_pending(budget_ms=50)
    # 返回已处理的条目数

cmds.scriptJob(idleEvent="python(\"on_idle()\")")
```

### 优雅关闭

```python
server.shutdown()  # 信号通道优雅关闭
```

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `create(name)` | `GracefulIpcChannelAdapter` | 创建服务端优雅 IPC 通道 |
| `connect(name)` | `GracefulIpcChannelAdapter` | 连接到已有的优雅 IPC 通道 |

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `wait_for_client()` | `None` | 等待客户端连接（仅服务端）|
| `send_frame(frame)` | `None` | 向对端发送 `DccLinkFrame` |
| `recv_frame()` | `DccLinkFrame \| None` | 接收帧（阻塞）。通道关闭时返回 `None` |
| `shutdown()` | `None` | 信号通道优雅关闭 |
| `bind_affinity_thread()` | `None` | 将当前线程绑定为亲和线程。在 DCC 主线程上调用**一次** |
| `pump_pending(budget_ms=100)` | `int` | 在亲和线程上按预算排空待处理工作项。返回已处理条目数 |

## SocketServerAdapter

基于 Unix Domain Socket（macOS/Linux）或 Named Pipe（Windows）的多客户端 IPC 服务器。支持有界连接池。

### 创建 Socket 服务器

```python
from dcc_mcp_core import SocketServerAdapter

server = SocketServerAdapter(
    path="/tmp/my-dcc.sock",  # Unix socket 路径或 Windows 管道名
    max_connections=10,        # 最大并发连接数
    connection_timeout_ms=30000,  # 连接超时（毫秒）
)

print(server.socket_path)      # 服务端监听路径
print(server.connection_count) # 当前连接的客户端数

server.shutdown()  # 优雅关闭
```

### 构造函数

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `path` | `str` | — | Socket 路径（Unix）或管道名（Windows）|
| `max_connections` | `int` | `10` | 最大并发连接数 |
| `connection_timeout_ms` | `int` | `30000` | 连接超时（毫秒）|

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `socket_path` | `str` | 服务端监听的 socket 路径 |
| `connection_count` | `int` | 当前连接的客户端数 |

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `shutdown()` | `None` | 优雅关闭服务端（阻塞直到停止）|
| `signal_shutdown()` | `None` | 发出关闭信号但不阻塞 |

## 传输辅助类

### TransportAddress

协议无关的传输端点。支持 TCP、Named Pipe（Windows）和 Unix Domain Socket（macOS/Linux）。

```python
from dcc_mcp_core import TransportAddress

# 工厂构造函数
addr = TransportAddress.tcp("127.0.0.1", 18812)
addr = TransportAddress.named_pipe("maya-mcp")          # Windows
addr = TransportAddress.unix_socket("/tmp/maya.sock")   # macOS/Linux

# 平台最优本地地址（以 PID 为唯一标识）
addr = TransportAddress.default_local("maya", pid=12345)

# 从 URI 字符串解析
addr = TransportAddress.parse("tcp://127.0.0.1:18812")
```

| 属性/方法 | 返回值 | 说明 |
|-----------|--------|------|
| `scheme` | `str` | `"tcp"`、`"pipe"` 或 `"unix"` |
| `is_local` | `bool` | 是否为本机传输 |
| `is_tcp` | `bool` | 是否为 TCP |
| `is_named_pipe` | `bool` | 是否为 Named Pipe |
| `is_unix_socket` | `bool` | 是否为 Unix Socket |
| `to_connection_string()` | `str` | URI 字符串，如 `"tcp://127.0.0.1:18812"` |

### TransportScheme

选择最优通信通道的策略：

| 常量 | 说明 |
|------|------|
| `AUTO` | 自动选择最优传输（Windows 用 Named Pipe，*nix 用 Unix Socket）|
| `TCP_ONLY` | 始终使用 TCP |
| `PREFER_NAMED_PIPE` | 优先 Named Pipe，降级到 TCP |
| `PREFER_UNIX_SOCKET` | 优先 Unix Socket，降级到 TCP |
| `PREFER_IPC` | 优先任意 IPC，降级到 TCP |

```python
from dcc_mcp_core import TransportScheme

addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

### ServiceEntry

已注册 DCC 服务实例的描述对象。

| 属性 | 类型 | 说明 |
|------|------|------|
| `dcc_type` | `str` | DCC 类型，如 `"maya"` |
| `instance_id` | `str` | UUID 字符串 |
| `host` | `str` | 主机地址 |
| `port` | `int` | TCP 端口 |
| `version` | `str \| None` | DCC 版本 |
| `scene` | `str \| None` | 当前打开的场景/文件 |
| `metadata` | `dict[str, str]` | 自定义字符串元数据 |
| `extras` | `dict[str, Any]` | JSON 类型的 DCC 元数据 |
| `status` | `ServiceStatus` | 实例状态 |
| `transport_address` | `TransportAddress \| None` | 首选 IPC 地址 |
| `last_heartbeat_ms` | `int` | 最后心跳时间戳（Unix 毫秒）|

### ServiceStatus

DCC 服务健康状态枚举：

| 常量 | 说明 |
|------|------|
| `AVAILABLE` | 就绪，可接受请求 |
| `BUSY` | 处理中，可能接受更多请求 |
| `UNREACHABLE` | 心跳无响应 |
| `SHUTTING_DOWN` | 正在优雅关闭 |

---

## 端到端示例

### DCC 插件（服务端）

```python
# Maya 插件内部
from dcc_mcp_core import GracefulIpcChannelAdapter, DccLinkFrame

server = GracefulIpcChannelAdapter.create("maya-ipc")
server.bind_affinity_thread()  # 在主线程调用一次
server.wait_for_client()

# Maya 空闲回调：
def on_idle():
    processed = server.pump_pending(budget_ms=50)

# 主消息循环
while True:
    frame = server.recv_frame()
    if frame is None:
        break  # 通道已关闭
    if frame.msg_type == 1:  # Call
        # 处理请求...
        reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"ok")
        server.send_frame(reply)

server.shutdown()
```

### MCP Agent（客户端）

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

client = IpcChannelAdapter.connect("maya-ipc")

# 发送 Call 帧
call = DccLinkFrame(msg_type=1, seq=0, body=b"get_scene_info")
client.send_frame(call)

# 接收 Reply
reply = client.recv_frame()
if reply and reply.msg_type == 2:
    print(f"结果: {reply.body}")
```
