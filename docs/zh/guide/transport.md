# 传输层

> **🚨 v0.14 已移除遗留传输栈（issue #251）。**
>
> 下文描述的 `TransportManager`、`FramedChannel`、`FramedIo`、
> `IpcListener`（Python 版）、`ListenerHandle`、`RoutingStrategy`、
> `ConnectionPool`、`InstanceRouter`、`CircuitBreaker`、`MessageEnvelope`、
> `encode_request` / `encode_response` / `encode_notify` / `decode_envelope`、
> `connect_ipc` 等都**已被删除**。新代码请改用基于 `ipckit` 的 DccLink 适配器：
>
> - `IpcChannelAdapter.connect(name)` / `.create(name)` — 单连接的带帧通道，
>   对接 Windows Named Pipe 或 *nix Unix Socket。
> - `SocketServerAdapter` — 支持连接上限的多客户端 IPC 服务器。
> - `GracefulIpcChannelAdapter` — 额外提供优雅关闭和 DCC 主线程友好的重入派发。
> - `DccLinkFrame` / `DccLinkType` — `[u32 len][u8 type][u64 seq][msgpack body]`
>   线格式，共 8 种消息（Call、Reply、Err、Progress、Cancel、Push、Ping、Pong）。
> - `ServiceEntry` + `FileRegistry` — 服务发现（保留未变）。
>
> 跨进程实例发现的对外接口为 gateway HTTP API（`GET /instances`、`POST /mcp` 等）。

传输层（`dcc-mcp-transport` crate）为 MCP 服务器与 DCC 应用实例之间的通信提供异步基础设施，包括连接池、服务发现、会话管理和线协议。

## 概览

```python
from dcc_mcp_core import TransportManager

transport = TransportManager("/path/to/registry")

# 注册 DCC 服务
instance_id = transport.register_service("maya", "127.0.0.1", 18812, version="2025.1")

# 创建会话
session_id = transport.get_or_create_session("maya")

# 使用连接
conn_id = transport.acquire_connection("maya")
# ... 执行操作 ...
transport.release_connection("maya", instance_id)

# 清理和关闭
transport.cleanup()
transport.shutdown()
```

## 服务发现

传输层使用基于文件的服务发现来跟踪运行中的 DCC 实例。每个实例使用 `(dcc_type, instance_id)` 作为键注册，支持同一 DCC 类型的多个实例。

```python
id1 = transport.register_service("maya", "127.0.0.1", 18812)
id2 = transport.register_service("maya", "127.0.0.1", 18813)
id3 = transport.register_service("blender", "127.0.0.1", 9090, version="4.0")

maya_instances = transport.list_instances("maya")
all_services = transport.list_all_services()

transport.heartbeat("maya", id1)
transport.deregister_service("maya", id1)
```

## 会话管理

会话跟踪与 DCC 实例的连接，提供生命周期状态管理和指标：

```python
session_id = transport.get_or_create_session("maya", id1)

session = transport.get_session(session_id)
# session 是一个字典，包含键: id, dcc_type, instance_id, state, request_count, error_count, last_error, created_at, last_request_at

transport.record_success(session_id, 50)
transport.record_error(session_id, 100, "timeout")

backoff_ms = transport.begin_reconnect(session_id)
transport.reconnect_success(session_id)

transport.close_session(session_id)
```

### 会话状态

| 状态 | 说明 |
|------|------|
| `connected` | 活跃且可接受请求 |
| `idle` | 超过空闲超时，仍然有效 |
| `reconnecting` | 失败后正在重连 |
| `closed` | 终态 |

## 连接池

```python
conn_id = transport.acquire_connection("maya")
transport.release_connection("maya", id1)
transport.pool_size()
```

## 配置

```python
transport = TransportManager(
    registry_dir="/path/to/registry",
    max_connections_per_dcc=10,   # 每种 DCC 类型的最大连接数
    idle_timeout=300,             # 会话空闲超时（秒）
    heartbeat_interval=5,         # 心跳间隔（秒）
    connect_timeout=10,           # TCP 连接超时（秒）
    reconnect_max_retries=3,      # 最大重连尝试次数
)
```

## 生命周期

```python
stale, sessions, evicted = transport.cleanup()
transport.shutdown()
transport.is_shutdown()
```

---

## 低级 IPC API

对于需要充当服务端或直接通过 IPC 通信（绕过 `TransportManager`）的 DCC 插件，可使用低级类。

### TransportAddress

协议无关的端点描述符，支持 TCP、Windows 命名管道和 Unix Domain Socket。

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
| `is_named_pipe` | `bool` | 是否为命名管道 |
| `is_unix_socket` | `bool` | 是否为 Unix Socket |
| `to_connection_string()` | `str` | URI 字符串，如 `"tcp://127.0.0.1:18812"` |

### TransportScheme

选择最优传输类型的策略枚举：

| 变体 | 说明 |
|------|------|
| `TransportScheme.AUTO` | 平台最优：Windows 用命名管道，Linux/macOS 用 Unix Socket |
| `TransportScheme.TCP_ONLY` | 始终使用 TCP |
| `TransportScheme.PREFER_NAMED_PIPE` | 同机用命名管道，否则用 TCP |
| `TransportScheme.PREFER_UNIX_SOCKET` | 同机用 Unix Socket，否则用 TCP |
| `TransportScheme.PREFER_IPC` | 任意本地 IPC 传输 |

```python
from dcc_mcp_core import TransportScheme, TransportAddress

scheme = TransportScheme.AUTO
addr = scheme.select_address("maya", "127.0.0.1", 18812, pid=12345)
```

### IpcListener

服务端监听器，用于 DCC 插件内部接受传入连接。

```python
from dcc_mcp_core import IpcListener, TransportAddress

# 绑定到传输地址（port=0 表示 OS 自动分配端口）
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)

# 获取实际绑定地址（port=0 时尤为有用）
local_addr = listener.local_address()
print(f"监听地址: {local_addr}")   # tcp://127.0.0.1:54321

# 接受连接（阻塞）
channel = listener.accept(timeout_ms=5000)  # → FramedChannel

# 或转换为 ListenerHandle 以追踪连接数
handle = listener.into_handle()   # 消费 listener，只能调用一次
```

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `IpcListener.bind(addr)` | `IpcListener` | 绑定地址，端口占用时抛出 `RuntimeError` |
| `local_address()` | `TransportAddress` | 实际绑定地址 |
| `transport_name` | `str` | `"tcp"`、`"named_pipe"` 或 `"unix_socket"` |
| `accept(timeout_ms=None)` | `FramedChannel` | 接受下一个连接，阻塞直到客户端连入 |
| `into_handle()` | `ListenerHandle` | 包装为带连接追踪的 handle（消费 `self`） |

### ListenerHandle

包装 `IpcListener`，提供连接计数和关闭控制。

```python
from dcc_mcp_core import IpcListener, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
listener = IpcListener.bind(addr)
handle = listener.into_handle()

print(handle.accept_count)   # 0
print(handle.is_shutdown)    # False

# 请求停止接受新连接
handle.shutdown()
```

| 属性/方法 | 返回值 | 说明 |
|-----------|--------|------|
| `accept_count` | `int` | 已接受的连接数 |
| `is_shutdown` | `bool` | 是否已请求关闭 |
| `transport_name` | `str` | 传输类型名称 |
| `local_address()` | `TransportAddress` | 绑定地址 |
| `shutdown()` | `None` | 停止接受新连接（幂等） |

### FramedChannel

全双工帧通道，后台读取循环自动处理 Ping/Pong 心跳。通过 `IpcListener.accept()`（服务端）或 `connect_ipc()`（客户端）获取。

```python
from dcc_mcp_core import connect_ipc, TransportAddress

# 客户端：连接到运行中的 DCC 服务器
addr = TransportAddress.tcp("127.0.0.1", 18812)
channel = connect_ipc(addr, timeout_ms=10000)

# 存活检测
rtt_ms = channel.ping()          # int，往返时间（毫秒）

# 阻塞接收
msg = channel.recv(timeout_ms=5000)
# msg: 含 "type" 字段的字典 → "request"、"response" 或 "notify"

# 非阻塞接收
msg = channel.try_recv()         # 缓冲区为空时返回 None

# 发送
req_id = channel.send_request("execute_python", params=b'{"code":"..."}')
channel.send_response(req_id, success=True, payload=b'{"result":1}')
channel.send_notify("scene_changed", data=b'{"scene":"untitled"}')

# 关闭
channel.shutdown()
print(channel.is_running)        # False
```

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `recv(timeout_ms=None)` | `dict \| None` | 阻塞接收，超时或连接关闭时返回 `None` |
| `try_recv()` | `dict \| None` | 非阻塞接收，缓冲区为空时返回 `None` |
| `ping(timeout_ms=5000)` | `int` | 心跳 ping，返回 RTT 毫秒数；数据消息不会丢失 |
| `send_request(method, params=None)` | `str` | 发送请求，返回 UUID 请求 ID |
| `send_response(request_id, success, payload=None, error=None)` | `None` | 发送请求的响应 |
| `send_notify(topic, data=None)` | `None` | 发送单向通知 |
| `shutdown()` | `None` | 优雅关闭（幂等） |
| `is_running` | `bool` | 后台读取器是否仍在运行 |

### connect_ipc

客户端连接工厂函数：

```python
from dcc_mcp_core import connect_ipc, TransportAddress

channel = connect_ipc(
    addr=TransportAddress.tcp("127.0.0.1", 18812),
    timeout_ms=10000,    # 默认 10000 毫秒
)
```

无法在超时时间内建立连接时抛出 `RuntimeError`。

### RoutingStrategy

选择 DCC 实例的策略（有多个实例注册时）：

| 变体 | 说明 |
|------|------|
| `FIRST_AVAILABLE` | 选择第一个可达实例 |
| `ROUND_ROBIN` | 轮询所有实例 |
| `LEAST_BUSY` | 会话请求数最少的实例 |
| `SPECIFIC` | 需要显式指定 `instance_id` |
| `SCENE_MATCH` | 按打开的场景名称匹配 |
| `RANDOM` | 随机选择实例 |

### ServiceStatus

DCC 服务健康状态枚举：

| 变体 | 含义 |
|------|------|
| `AVAILABLE` | 就绪，可接受请求 |
| `BUSY` | 处理中，可能接受更多请求 |
| `UNREACHABLE` | 心跳无响应 |
| `SHUTTING_DOWN` | 正在优雅关闭 |

---

## 端到端示例：DCC 插件服务端

```python
# Maya 插件内部（服务端）
import maya.cmds as cmds
from dcc_mcp_core import IpcListener, TransportAddress
import threading, os

addr = TransportAddress.default_local("maya", os.getpid())
listener = IpcListener.bind(addr)
print(f"Maya IPC 服务器: {listener.local_address()}")

def serve():
    channel = listener.accept()
    while True:
        msg = channel.recv(timeout_ms=1000)
        if msg is None:
            break
        if msg["type"] == "request":
            result = cmds.ls()
            channel.send_response(msg["id"], success=True,
                                  payload=str(result).encode())

threading.Thread(target=serve, daemon=True).start()
```

```python
# 客户端（MCP Agent）
from dcc_mcp_core import connect_ipc, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
channel = connect_ipc(addr)
req_id = channel.send_request("ls")
response = channel.recv()
# response["type"] == "response", response["payload"] == b"[...]"
channel.shutdown()
```
