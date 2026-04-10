# 传输层 API

`dcc_mcp_core` — TransportManager, TransportAddress, TransportScheme, RoutingStrategy, ServiceStatus, ServiceEntry, IpcListener, ListenerHandle, FramedChannel, connect_ipc.

## 概述

传输层模块提供 AI 智能体与 DCC 应用之间的**跨平台 IPC 和 TCP 通信**。核心设计：

- 同机连接优先使用 **Named Pipe**（Windows）/ **Unix Domain Socket**（macOS/Linux），亚毫秒延迟，零配置。
- 跨机或 IPC 不可用时自动降级为 **TCP**。
- `TransportAddress.default_local(dcc_type, pid)` 自动选择当前平台的最优传输方式。
- `TransportManager.bind_and_register()` 是 DCC 插件开发者的一键启动推荐入口。

## TransportAddress

与协议无关的传输端点。支持 TCP、Named Pipe（Windows）和 Unix Domain Socket（macOS/Linux）。

### 工厂方法

```python
from dcc_mcp_core import TransportAddress

# TCP
addr = TransportAddress.tcp("127.0.0.1", 18812)

# Named Pipe（Windows）
addr = TransportAddress.named_pipe("dcc-maya-12345")

# Unix Domain Socket（macOS/Linux）
addr = TransportAddress.unix_socket("/tmp/dcc-maya-12345.sock")

# 当前平台最优本地传输
addr = TransportAddress.default_local("maya", pid=os.getpid())

# 从 URI 字符串解析
addr = TransportAddress.parse("tcp://127.0.0.1:18812")
addr = TransportAddress.parse("pipe://dcc-maya-12345")
addr = TransportAddress.parse("unix:///tmp/dcc-maya.sock")
```

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `tcp(host, port)` | `TransportAddress` | 创建 TCP 地址 |
| `named_pipe(name)` | `TransportAddress` | 创建 Named Pipe 地址（Windows）|
| `unix_socket(path)` | `TransportAddress` | 创建 Unix Socket 地址 |
| `default_local(dcc_type, pid)` | `TransportAddress` | 自动选择最优本地传输 |
| `default_pipe_name(dcc_type, pid)` | `TransportAddress` | 为 DCC 实例生成 Named Pipe |
| `default_unix_socket(dcc_type, pid)` | `TransportAddress` | 为 DCC 实例生成 Unix Socket |
| `parse(s)` | `TransportAddress` | 解析 URI 字符串（`tcp://`、`pipe://`、`unix://`）|

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `scheme` | `str` | 传输协议：`"tcp"`、`"pipe"` 或 `"unix"` |
| `is_local` | `bool` | 是否为本机传输 |
| `is_tcp` | `bool` | 是否为 TCP |
| `is_named_pipe` | `bool` | 是否为 Named Pipe |
| `is_unix_socket` | `bool` | 是否为 Unix Socket |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `to_connection_string()` | `str` | URI 字符串，如 `"tcp://127.0.0.1:18812"` |

### 示例

```python
import os
from dcc_mcp_core import TransportAddress

addr = TransportAddress.default_local("maya", os.getpid())
print(addr.scheme)   # Windows: "pipe"，macOS/Linux: "unix"
print(addr.is_local) # True
```

## TransportScheme

选择最优通信通道的策略。

### 常量

| 常量 | 说明 |
|------|------|
| `AUTO` | 自动选择最优传输 |
| `TCP_ONLY` | 始终使用 TCP |
| `PREFER_NAMED_PIPE` | 优先 Named Pipe，降级到 TCP |
| `PREFER_UNIX_SOCKET` | 优先 Unix Socket，降级到 TCP |
| `PREFER_IPC` | 优先任意 IPC，降级到 TCP |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `select_address(dcc_type, host, port, pid=None)` | `TransportAddress` | 选择最优地址 |

## RoutingStrategy

多 DCC 实例时的选择策略。

### 常量

| 常量 | 说明 |
|------|------|
| `FIRST_AVAILABLE` | 使用第一个可用实例 |
| `ROUND_ROBIN` | 轮询所有可用实例 |
| `LEAST_BUSY` | 优先最低负载的实例 |
| `SPECIFIC` | 按 ID 指定实例 |
| `SCENE_MATCH` | 优先打开匹配场景的实例 |
| `RANDOM` | 随机选择 |

```python
from dcc_mcp_core import RoutingStrategy, TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
session_id = mgr.get_or_create_session_routed(
    "maya",
    strategy=RoutingStrategy.ROUND_ROBIN,
)
```

## ServiceStatus

DCC 服务实例状态枚举。

### 常量

| 常量 | 说明 |
|------|------|
| `AVAILABLE` | 接受连接（默认）|
| `BUSY` | 正在处理请求 |
| `UNREACHABLE` | 健康检查失败 |
| `SHUTTING_DOWN` | 正在关闭 |

```python
from dcc_mcp_core import ServiceStatus, TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
mgr.update_service_status("maya", instance_id, ServiceStatus.BUSY)
```

## ServiceEntry

已注册 DCC 服务实例的描述对象。

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `dcc_type` | `str` | DCC 类型，如 `"maya"` |
| `instance_id` | `str` | UUID 字符串 |
| `host` | `str` | 主机地址 |
| `port` | `int` | TCP 端口 |
| `version` | `str \| None` | DCC 版本 |
| `scene` | `str \| None` | 当前打开的场景/文件 |
| `metadata` | `dict[str, str]` | 自定义元数据 |
| `status` | `ServiceStatus` | 实例状态 |
| `transport_address` | `TransportAddress \| None` | 首选 IPC 地址 |
| `last_heartbeat_ms` | `int` | 最后心跳时间戳（Unix 毫秒）|
| `is_ipc` | `bool` | 是否使用 IPC 传输 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `effective_address()` | `TransportAddress` | IPC 地址或 TCP 降级地址 |
| `to_dict()` | `dict` | 序列化为字典 |

### 示例

```python
entry = mgr.find_best_service("maya")
print(entry.dcc_type)             # "maya"
print(entry.status)               # ServiceStatus.AVAILABLE
print(entry.effective_address())  # 如 "pipe://dcc-maya-12345"

# 检查空闲时间
import time
idle_sec = (time.time() * 1000 - entry.last_heartbeat_ms) / 1000
if idle_sec > 300:
    mgr.deregister_service("maya", entry.instance_id)
```

## TransportManager

带有服务发现、智能路由、会话管理和连接池的传输层管理器。

### 构造函数

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager(
    registry_dir="/tmp/dcc-mcp",
    max_connections_per_dcc=10,
    idle_timeout=300,
    heartbeat_interval=5,
    connect_timeout=10,
    reconnect_max_retries=3,
)
```

### 服务发现

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register_service(dcc_type, host, port, version=None, scene=None, metadata=None, transport_address=None)` | `str` | 注册服务，返回 instance_id |
| `deregister_service(dcc_type, instance_id)` | `bool` | 注销服务 |
| `list_instances(dcc_type)` | `list[ServiceEntry]` | 列出某 DCC 类型的所有实例 |
| `list_all_services()` | `list[ServiceEntry]` | 列出所有已注册服务 |
| `list_all_instances()` | `list[ServiceEntry]` | `list_all_services()` 的别名 |
| `get_service(dcc_type, instance_id)` | `ServiceEntry \| None` | 获取特定实例 |
| `heartbeat(dcc_type, instance_id)` | `bool` | 更新心跳时间戳 |
| `update_service_status(dcc_type, instance_id, status)` | `bool` | 设置实例状态 |

#### `register_service` — IPC 传输参数

传入 `transport_address` 以启用 Named Pipe / Unix Socket 进行低延迟同机通信：

```python
import os
from dcc_mcp_core import TransportManager, TransportAddress

mgr = TransportManager("/tmp/dcc-mcp")
addr = TransportAddress.default_local("maya", os.getpid())
instance_id = mgr.register_service(
    "maya", "127.0.0.1", 18812,
    version="2025",
    transport_address=addr,
)
```

### 智能路由

#### `find_best_service()`

返回优先级最高的活跃 `ServiceEntry`。优先级：本地 IPC > 本地 TCP > 远程 TCP。同层内 `AVAILABLE` 优先于 `BUSY`，同优先级实例自动轮询。

```python
entry = mgr.find_best_service("maya")
session_id = mgr.get_or_create_session("maya", entry.instance_id)
```

#### `rank_services()`

返回按优先级排序的所有活跃实例（分数越低越优先）：

| 分数 | 层级 |
|------|------|
| 0 | 本地 IPC，AVAILABLE |
| 1 | 本地 IPC，BUSY |
| 2 | 本地 TCP，AVAILABLE |
| 3 | 本地 TCP，BUSY |
| 4 | 远程 TCP，AVAILABLE |
| 5 | 远程 TCP，BUSY |

`UNREACHABLE` 和 `SHUTTING_DOWN` 实例会被排除。

```python
for entry in mgr.rank_services("maya"):
    print(entry.instance_id, entry.status, entry.effective_address())
```

#### `bind_and_register()`

DCC 插件开发者的一键启动接口。自动绑定最优传输并注册服务：

```python
from dcc_mcp_core import TransportManager

mgr = TransportManager("/tmp/dcc-mcp")
instance_id, listener = mgr.bind_and_register("maya", version="2025")
local_addr = listener.local_address()
print(f"监听地址：{local_addr}")

# 在 DCC 插件线程中接受连接
channel = listener.accept()
```

传输选择优先级：Named Pipe（Windows）/ Unix Socket（macOS/Linux）→ TCP 随机端口。

### 会话管理

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_or_create_session(dcc_type, instance_id=None)` | `str` | 获取/创建会话（UUID）|
| `get_or_create_session_routed(dcc_type, strategy=None, hint=None)` | `str` | 使用路由策略获取/创建会话 |
| `get_session(session_id)` | `dict \| None` | 获取会话信息 |
| `record_success(session_id, latency_ms)` | — | 记录成功请求 |
| `record_error(session_id, latency_ms, error)` | — | 记录失败请求 |
| `begin_reconnect(session_id)` | `int` | 开始重连，返回退避时间（毫秒）|
| `reconnect_success(session_id)` | — | 标记重连成功 |
| `close_session(session_id)` | `bool` | 关闭会话 |
| `list_sessions()` | `list[dict]` | 列出所有活跃会话 |
| `list_sessions_for_dcc(dcc_type)` | `list[dict]` | 列出某 DCC 的所有会话 |
| `session_count()` | `int` | 活跃会话数量 |

### 连接池

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `acquire_connection(dcc_type, instance_id=None)` | `str` | 获取连接（UUID）|
| `release_connection(dcc_type, instance_id)` | — | 释放连接回池 |
| `pool_size()` | `int` | 连接池总连接数 |
| `pool_count_for_dcc(dcc_type)` | `int` | 指定 DCC 的池大小 |

### 生命周期

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `cleanup()` | `tuple[int, int, int]` | 返回 (过期服务数, 关闭会话数, 驱逐连接数) |
| `shutdown()` | — | 优雅关闭 |
| `is_shutdown()` | `bool` | 是否已关闭 |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `TransportManager(services=N, sessions=N, pool=N)` |
| `__len__` | 返回会话数量 |

## IpcListener

DCC 服务端 IPC 监听器。支持 TCP、Windows Named Pipe 和 Unix Domain Socket。

### 创建

```python
from dcc_mcp_core import IpcListener, TransportAddress

addr = TransportAddress.tcp("127.0.0.1", 0)  # port 0 = 系统分配空闲端口
listener = IpcListener.bind(addr)
print(listener.local_address())  # 如 "tcp://127.0.0.1:54321"
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `IpcListener.bind(addr)` | `IpcListener` | 绑定到传输地址 |
| `local_address()` | `TransportAddress` | 获取已绑定的本地地址 |
| `accept(timeout_ms=None)` | `FramedChannel` | 接受下一个连接（阻塞）|
| `into_handle()` | `ListenerHandle` | 包装为 ListenerHandle（消耗 listener）|

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `transport_name` | `str` | 传输类型：`"tcp"`、`"named_pipe"` 或 `"unix_socket"` |

::: tip
使用端口 `0` 绑定，系统会自动分配空闲端口。绑定后调用 `local_address()` 获取实际端口。
:::

## ListenerHandle

带连接计数和关闭控制的 IPC 监听器句柄。

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `accept_count` | `int` | 已接受的连接数 |
| `is_shutdown` | `bool` | 是否已请求关闭 |
| `transport_name` | `str` | 传输类型字符串 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `local_address()` | `TransportAddress` | 获取本地地址 |
| `shutdown()` | — | 请求停止接受新连接（幂等）|

## FramedChannel

用于 DCC 连接的全双工帧通信信道。封装 TCP/IPC，自动处理 Ping/Pong 心跳和消息缓冲。

### 获取实例

```python
from dcc_mcp_core import connect_ipc, IpcListener, TransportAddress

# 服务端：从 IpcListener 接受连接
addr = TransportAddress.tcp("127.0.0.1", 0)
listener = IpcListener.bind(addr)
channel = listener.accept()

# 客户端：连接到运行中的 DCC
addr = TransportAddress.tcp("127.0.0.1", 18812)
channel = connect_ipc(addr)
```

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `is_running` | `bool` | 后台读取任务是否仍在运行 |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `call(method, params=None, timeout_ms=30000)` | `dict` | 发送请求并等待响应（RPC）|
| `recv(timeout_ms=None)` | `dict \| None` | 接收下一条数据消息（阻塞）|
| `try_recv()` | `dict \| None` | 非阻塞接收 |
| `ping(timeout_ms=5000)` | `int` | 发送心跳，返回 RTT 毫秒数 |
| `send_request(method, params=None)` | `str` | 发送请求，返回 request_id UUID |
| `send_response(request_id, success, payload=None, error=None)` | — | 发送响应 |
| `send_notify(topic, data=None)` | — | 发送单向通知 |
| `shutdown()` | — | 优雅关闭（幂等）|

### `call()` — 推荐 RPC 模式

调用 DCC 命令的首选方式。发送 `Request` 并等待对应 `Response`：

```python
result = channel.call("execute_python", b'print("hello")', timeout_ms=10000)
if result["success"]:
    print(result["payload"])   # bytes
else:
    raise RuntimeError(result["error"])
```

`call()` 返回字典键：

| 键 | 类型 | 说明 |
|----|------|------|
| `id` | `str` | 对应请求的 UUID |
| `success` | `bool` | DCC 是否执行成功 |
| `payload` | `bytes` | 序列化的结果数据 |
| `error` | `str \| None` | 失败时的错误消息 |

::: tip
`call()` 等待期间收到的其他消息（通知、其他响应）**不会丢失**，仍可通过 `recv()` 获取。
:::

### `recv()` — 事件循环模式

适用于需要处理多种消息类型的 DCC 插件服务端：

```python
while True:
    msg = channel.recv(timeout_ms=100)
    if msg is None:
        continue  # 超时或连接已关闭

    if msg["type"] == "request":
        handle_request(channel, msg)
    elif msg["type"] == "notify":
        handle_notification(msg)
```

## connect_ipc()

顶层函数，用于创建客户端 `FramedChannel` 连接到运行中的 DCC 服务。

```python
from dcc_mcp_core import connect_ipc, TransportAddress

addr = TransportAddress.default_local("maya", pid=12345)
channel = connect_ipc(addr)

rtt = channel.ping()
print(f"已连接，RTT: {rtt}ms")

result = channel.call("get_scene_info")
channel.shutdown()
```

## 完整集成示例

```python
import os
from dcc_mcp_core import TransportManager, TransportAddress, RoutingStrategy

# --- DCC 插件端（在 Maya/Blender 内运行）---
def start_dcc_server(dcc_type: str):
    mgr = TransportManager("/tmp/dcc-mcp")
    instance_id, listener = mgr.bind_and_register(dcc_type, version="2025")
    print(f"DCC 服务已绑定到：{listener.local_address()}")

    while True:
        channel = listener.accept(timeout_ms=1000)
        if channel:
            msg = channel.recv()
            if msg and msg["type"] == "request":
                channel.send_response(
                    msg["id"],
                    success=True,
                    payload=b'{"status": "ok"}',
                )


# --- 智能体端（AI 工具或外部脚本）---
def connect_to_maya():
    mgr = TransportManager("/tmp/dcc-mcp")

    # 找到最优 Maya 实例（IPC 优先，再 TCP）
    entry = mgr.find_best_service("maya")
    print(f"连接到 {entry.effective_address()}")

    # 轮询多实例实现负载均衡
    session_id = mgr.get_or_create_session_routed(
        "maya",
        strategy=RoutingStrategy.ROUND_ROBIN,
    )
```

## 线协议说明

消息使用 MessagePack 序列化，带 4 字节大端序长度前缀：

```
[4 字节长度][MessagePack 载荷]
```

- **Request**: `{ id: UUID, method: String, params: Vec<u8> }`
- **Response**: `{ id: UUID, success: bool, payload: Vec<u8>, error: Option<String> }`
- **Notify**: `{ topic: String, data: Vec<u8> }`
- **Ping/Pong**：由 `FramedChannel` 自动处理

## 低级帧编码函数

用于高级场景（如自定义传输实现或测试）的原始帧编码/解码函数：

### `encode_request()`

```python
from dcc_mcp_core import encode_request

frame = encode_request("execute_python", b'cmds.sphere()')
# bytes: [4 字节大端序长度][MessagePack 载荷]
```

### `encode_response()`

```python
from dcc_mcp_core import encode_response

frame = encode_response(
    request_id="550e8400-e29b-41d4-a716-446655440000",
    success=True,
    payload=b'{"result": "pSphere1"}',
)

# 失败响应
frame = encode_response(
    request_id="550e8400-e29b-41d4-a716-446655440000",
    success=False,
    error="操作失败：对象未找到",
)
```

### `encode_notify()`

```python
from dcc_mcp_core import encode_notify

frame = encode_notify("scene_changed", b'{"change": "object_added"}')
frame = encode_notify("render_complete")  # data 可选
```

### `decode_envelope()`

将原始 MessagePack 载荷（已去除长度前缀）解码为消息字典：

```python
from dcc_mcp_core import encode_request, decode_envelope

frame = encode_request("ping", b"")
msg = decode_envelope(frame[4:])  # 去掉 4 字节长度前缀

print(msg["type"])    # "request"
print(msg["method"]) # "ping"
```

不同 `"type"` 的返回字典字段：

| 类型 | 字段 |
|------|------|
| `"request"` | `id` (str)、`method` (str)、`params` (bytes) |
| `"response"` | `id` (str)、`success` (bool)、`payload` (bytes)、`error` (str\|None) |
| `"notify"` | `id` (str\|None)、`topic` (str)、`data` (bytes) |
| `"ping"` | `id` (str)、`timestamp_ms` (int) |
| `"pong"` | `id` (str)、`timestamp_ms` (int) |
| `"shutdown"` | `reason` (str\|None) |
