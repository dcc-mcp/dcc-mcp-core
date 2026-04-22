# 传输层 API

`dcc_mcp_core` — DccLinkFrame, IpcChannelAdapter, GracefulIpcChannelAdapter, SocketServerAdapter, TransportAddress, TransportScheme, ServiceEntry, ServiceStatus.

## 概述

传输模块提供 AI 智能体与 DCC 应用之间的 **基于 DccLink 的 IPC 通信**。核心设计：

- 同机连接优先使用 **Named Pipe**（Windows）/ **Unix Domain Socket**（macOS/Linux），亚毫秒延迟，零配置。
- DccLink 适配器封装 `ipckit` 通道，使用二进制线格式：`[u32 len][u8 type][u64 seq][msgpack body]`。
- `IpcChannelAdapter.create(name)` + `wait_for_client()` 是推荐的服务端启动方式。
- `IpcChannelAdapter.connect(name)` 是客户端连接入口。
- `GracefulIpcChannelAdapter` 增加优雅关闭和 DCC 主线程集成。
- `SocketServerAdapter` 提供带连接池的多客户端连接。

## DccLinkFrame

DCC-Link 帧，包含 `msg_type`、`seq` 和 `body` 字段。

线格式：`[u32 len][u8 type][u64 seq][msgpack body]`。

消息类型标签：1=Call, 2=Reply, 3=Err, 4=Progress, 5=Cancel, 6=Push, 7=Ping, 8=Pong。

### 构造函数

```python
from dcc_mcp_core import DccLinkFrame

frame = DccLinkFrame(msg_type=1, seq=0, body=b"hello")
```

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `msg_type` | `int` | — | 消息类型标签（1-8）。无效时抛出 `ValueError`。|
| `seq` | `int` | — | 序列号。|
| `body` | `bytes \| None` | `None` | 载荷字节。|

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
| `decode(data)` | `DccLinkFrame` | 从包含 4 字节长度前缀的字节中解码帧（静态方法）。格式错误时抛出 `RuntimeError`。|

### 示例

```python
frame = DccLinkFrame(msg_type=1, seq=0, body=b"payload")
encoded = frame.encode()
decoded = DccLinkFrame.decode(encoded)
assert decoded.msg_type == frame.msg_type
assert decoded.seq == frame.seq
assert decoded.body == frame.body
```

## IpcChannelAdapter

基于 `ipckit::IpcChannel` 的轻量适配器，使用 DCC-Link 帧格式。通过 Named Pipe（Windows）或 Unix Domain Socket（macOS/Linux）提供 1:1 帧化 IPC 连接。

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `create(name)` | `IpcChannelAdapter` | 创建服务端 IPC 通道。创建失败时抛出 `RuntimeError`。|
| `connect(name)` | `IpcChannelAdapter` | 连接到已有的 IPC 通道。连接失败时抛出 `RuntimeError`。|

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `wait_for_client()` | `None` | 等待客户端连接（仅服务端）。等待失败时抛出 `RuntimeError`。|
| `send_frame(frame)` | `None` | 向对端发送 `DccLinkFrame`。发送失败时抛出 `RuntimeError`。|
| `recv_frame()` | `DccLinkFrame \| None` | 接收 DCC-Link 帧（阻塞）。通道关闭时返回 `None`。意外错误时抛出 `RuntimeError`。|

### 示例：服务端

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

server = IpcChannelAdapter.create("my-dcc")
server.wait_for_client()

frame = server.recv_frame()
if frame is not None:
    reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"result")
    server.send_frame(reply)
```

### 示例：客户端

```python
from dcc_mcp_core import IpcChannelAdapter, DccLinkFrame

client = IpcChannelAdapter.connect("my-dcc")
call = DccLinkFrame(msg_type=1, seq=0, body=b"request")
client.send_frame(call)

reply = client.recv_frame()
if reply is not None:
    print(reply.body)
```

## GracefulIpcChannelAdapter

带优雅关闭和亲和线程支持的 IPC 通道适配器。在 `IpcChannelAdapter` 基础上增加了优雅关闭和 `bind_affinity_thread` / `pump_pending`，用于集成 DCC 主线程空闲回调。

对于需要重入安全 Python 派发的场景，推荐使用 `dcc_mcp_core._core` 中的 `DeferredExecutor` 而非 `submit()`。

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `create(name)` | `GracefulIpcChannelAdapter` | 创建服务端优雅 IPC 通道。创建失败时抛出 `RuntimeError`。|
| `connect(name)` | `GracefulIpcChannelAdapter` | 连接到已有的优雅 IPC 通道。连接失败时抛出 `RuntimeError`。|

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `wait_for_client()` | `None` | 等待客户端连接（仅服务端）。等待失败时抛出 `RuntimeError`。|
| `send_frame(frame)` | `None` | 向对端发送 `DccLinkFrame`。发送失败时抛出 `RuntimeError`。|
| `recv_frame()` | `DccLinkFrame \| None` | 接收 DCC-Link 帧（阻塞）。通道关闭时返回 `None`。意外错误时抛出 `RuntimeError`。|
| `shutdown()` | `None` | 信号通道优雅关闭。|
| `bind_affinity_thread()` | `None` | 将当前线程绑定为亲和线程以实现重入安全派发。在 DCC 主线程上调用**一次**。|
| `pump_pending(budget_ms=100)` | `int` | 在亲和线程上按预算排空待处理工作项。从 DCC 宿主空闲回调中调用。返回已处理条目数。|

### 示例

```python
from dcc_mcp_core import GracefulIpcChannelAdapter, DccLinkFrame

server = GracefulIpcChannelAdapter.create("my-dcc")
server.bind_affinity_thread()
server.wait_for_client()

# 在 DCC 空闲回调中：
# processed = server.pump_pending(budget_ms=50)

frame = server.recv_frame()
if frame is not None:
    reply = DccLinkFrame(msg_type=2, seq=frame.seq, body=b"ok")
    server.send_frame(reply)

server.shutdown()
```

## SocketServerAdapter

`ipckit::SocketServer` 的最小封装（多客户端 Unix socket / named pipe）。支持有界连接池。

### 构造函数

```python
from dcc_mcp_core import SocketServerAdapter

server = SocketServerAdapter(
    path="/tmp/my-dcc.sock",
    max_connections=10,
    connection_timeout_ms=30000,
)
```

| 参数 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `path` | `str` | — | Socket 路径（Unix）或管道名（Windows）。创建失败时抛出 `RuntimeError`。|
| `max_connections` | `int` | `10` | 最大并发连接数。|
| `connection_timeout_ms` | `int` | `30000` | 连接超时（毫秒）。|

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `socket_path` | `str` | 服务端监听的 socket 路径。|
| `connection_count` | `int` | 当前连接的客户端数。|

### 实例方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `shutdown()` | `None` | 优雅关闭服务端（阻塞直到停止）。|
| `signal_shutdown()` | `None` | 发出关闭信号但不阻塞。|

## TransportAddress

与协议无关的 DCC 通信传输端点。支持 TCP、Named Pipe（Windows）和 Unix Domain Socket（macOS/Linux）。

### 静态方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `tcp(host, port)` | `TransportAddress` | 创建 TCP 传输地址 |
| `named_pipe(name)` | `TransportAddress` | 创建 Named Pipe 传输地址（Windows）|
| `unix_socket(path)` | `TransportAddress` | 创建 Unix Domain Socket 传输地址 |
| `default_local(dcc_type, pid)` | `TransportAddress` | 生成当前平台最优本地传输 |
| `default_pipe_name(dcc_type, pid)` | `TransportAddress` | 为 DCC 实例生成默认 Named Pipe 名称 |
| `default_unix_socket(dcc_type, pid)` | `TransportAddress` | 为 DCC 实例生成默认 Unix Socket 路径 |
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

选择最优通信通道的传输策略。

### 常量

| 常量 | 说明 |
|------|------|
| `AUTO` | 自动选择最优传输（Windows 用 Named Pipe，*nix 用 Unix Socket）|
| `TCP_ONLY` | 始终使用 TCP |
| `PREFER_NAMED_PIPE` | 优先 Named Pipe，降级到 TCP |
| `PREFER_UNIX_SOCKET` | 优先 Unix Socket，降级到 TCP |
| `PREFER_IPC` | 优先任意 IPC，降级到 TCP |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `select_address(dcc_type, host, port, pid=None)` | `TransportAddress` | 选择最优传输地址 |

```python
from dcc_mcp_core import TransportScheme

addr = TransportScheme.AUTO.select_address("maya", "127.0.0.1", 18812, pid=12345)
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
| `documents` | `list[str]` | 打开的文档列表 |
| `pid` | `int \| None` | 进程 ID |
| `display_name` | `str \| None` | 显示名称 |
| `metadata` | `dict[str, str]` | 自定义字符串元数据 |
| `status` | `ServiceStatus` | 实例状态 |
| `transport_address` | `TransportAddress \| None` | 首选 IPC 地址 |
| `last_heartbeat_ms` | `int` | 最后心跳时间戳（Unix 毫秒）|

### Properties

| Property | 类型 | 说明 |
|----------|------|------|
| `extras` | `dict[str, Any]` | 任意 JSON 类型的 DCC 扩展字段。与 `metadata`（仅字符串）不同，`extras` 支持嵌套对象/数组/数字/布尔值。返回新字典——修改不影响注册表。|

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `effective_address()` | `TransportAddress` | IPC 地址或 TCP 降级地址 |
| `to_dict()` | `dict` | 序列化为字典 |

## ServiceStatus

DCC 服务实例状态枚举。

### 常量

| 常量 | 说明 |
|------|------|
| `AVAILABLE` | 接受连接（默认）|
| `BUSY` | 正在处理请求 |
| `UNREACHABLE` | 健康检查失败 |
| `SHUTTING_DOWN` | 正在关闭 |

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
| `register_service(dcc_type, host, port, version=None, scene=None, documents=None, pid=None, display_name=None, metadata=None, transport_address=None, extras=None)` | `str` | 注册服务，返回 instance_id。`extras` 接受 `dict[str, Any]` 类型的 JSON 元数据 |
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
| `AVAILABLE` | 接受连接（默认）|
| `BUSY` | 正在处理请求 |
| `UNREACHABLE` | 健康检查失败 |
| `SHUTTING_DOWN` | 正在关闭 |

## 线协议

DccLink 帧使用以下二进制线格式：

```
[u32 len][u8 type][u64 seq][msgpack body]
```

- `len` — 4 字节大端序总帧长度（包含 type + seq + body）
- `type` — 1 字节消息类型标签（1-8）
- `seq` — 8 字节大端序序列号
- `body` — MessagePack 编码的载荷

消息类型：

| 标签 | 类型 | 方向 | 说明 |
|------|------|------|------|
| 1 | Call | 客户端 → 服务端 | 请求调用 |
| 2 | Reply | 服务端 → 客户端 | 成功响应 |
| 3 | Err | 服务端 → 客户端 | 错误响应 |
| 4 | Progress | 服务端 → 客户端 | 进度更新 |
| 5 | Cancel | 客户端 → 服务端 | 取消信号 |
| 6 | Push | 服务端 → 客户端 | 服务端推送消息 |
| 7 | Ping | 双向 | 心跳请求 |
| 8 | Pong | 双向 | 心跳响应 |
