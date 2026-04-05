# 传输层 API

`dcc_mcp_core.TransportManager`

## TransportManager

Rust 传输层的 Python 封装。通过内部 Tokio 运行时将异步操作桥接为同步调用。

### 构造函数

```python
TransportManager(
    registry_dir: str,
    max_connections_per_dcc: int = 10,
    idle_timeout: int = 300,
    heartbeat_interval: int = 5,
    connect_timeout: int = 10,
    reconnect_max_retries: int = 3,
)
```

### 服务发现

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `register_service(dcc_type, host, port, version=None, scene=None, metadata=None)` | `str` | 注册服务，返回 instance_id (UUID) |
| `deregister_service(dcc_type, instance_id)` | `bool` | 注销服务 |
| `list_instances(dcc_type)` | `List[dict]` | 列出某 DCC 类型的所有实例 |
| `list_all_services()` | `List[dict]` | 列出所有已注册服务 |
| `heartbeat(dcc_type, instance_id)` | `bool` | 更新心跳时间戳 |

### 会话管理

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_or_create_session(dcc_type, instance_id=None)` | `str` | 获取/创建会话 (UUID) |
| `get_session(session_id)` | `dict?` | 获取会话信息 |
| `record_success(session_id, latency_ms)` | — | 记录成功请求 |
| `record_error(session_id, latency_ms, error)` | — | 记录失败请求 |
| `begin_reconnect(session_id)` | `int` | 开始重连，返回退避时间（毫秒） |
| `reconnect_success(session_id)` | — | 标记重连成功 |
| `close_session(session_id)` | `bool` | 关闭会话 |
| `list_sessions()` | `List[dict]` | 列出所有活跃会话 |
| `session_count()` | `int` | 活跃会话数量 |

### 连接池

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `acquire_connection(dcc_type, instance_id=None)` | `str` | 获取连接 (UUID) |
| `release_connection(dcc_type, instance_id)` | — | 释放连接回池 |
| `pool_size()` | `int` | 连接池中的总连接数 |

### 生命周期

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `cleanup()` | `(int, int, int)` | 返回 (过期服务数, 关闭会话数, 驱逐连接数) |
| `shutdown()` | — | 优雅关闭 |
| `is_shutdown()` | `bool` | 检查是否已关闭 |

### Dunder 方法

| 方法 | 说明 |
|------|------|
| `__repr__` | `TransportManager(services=N, sessions=N, pool=N)` |
| `__len__` | 返回会话数量 |

## 仅 Rust 类型

以下类型在 Rust 中可用，但不直接暴露给 Python：

### TransportConfig

| 字段 | 类型 | 默认值 |
|------|------|--------|
| `pool` | `PoolConfig` | — |
| `session` | `SessionConfig` | — |
| `connect_timeout` | `Duration` | 10 秒 |
| `heartbeat_interval` | `Duration` | 5 秒 |

### PoolConfig

| 字段 | 类型 | 默认值 |
|------|------|--------|
| `max_connections_per_type` | `usize` | 10 |
| `max_idle_time` | `Duration` | 300 秒 |
| `max_lifetime` | `Duration` | 3600 秒 |
| `acquire_timeout` | `Duration` | 30 秒 |

### SessionConfig

| 字段 | 类型 | 默认值 |
|------|------|--------|
| `idle_timeout` | `Duration` | 300 秒 |
| `reconnect_max_retries` | `u32` | 3 |
| `reconnect_backoff_base` | `Duration` | 1 秒 |
| `max_session_lifetime` | `Duration` | 3600 秒 |
| `heartbeat_interval` | `Duration` | 5 秒 |

### TransportError

| 变体 | 说明 |
|------|------|
| `ConnectionFailed` | TCP 连接失败 |
| `ConnectionTimeout` | 连接超时 |
| `PoolExhausted` | 所有连接都在使用中 |
| `AcquireTimeout` | 等待池化连接超时 |
| `ServiceNotFound` | 服务未在注册表中找到 |
| `ServiceAlreadyRegistered` | 重复注册 |
| `Serialization` | MessagePack 序列化错误 |
| `Io` | IO 错误 |
| `RegistryFile` | 注册表文件错误 |
| `Shutdown` | 传输层已关闭 |
| `SessionNotFound` | 会话未找到 |
| `InvalidSessionState` | 无效的状态转换 |
| `ReconnectionFailed` | 超过最大重试次数 |
| `Internal` | 通用内部错误 |

### ServiceStatus

| 值 | 说明 |
|----|------|
| `Available` | 接受连接（默认） |
| `Busy` | 正在处理请求 |
| `Unreachable` | 健康检查失败 |
| `ShuttingDown` | 正在关闭 |

### SessionState

| 值 | 说明 |
|----|------|
| `Connected` | 可接受请求 |
| `Idle` | 超过空闲超时，仍然有效 |
| `Reconnecting` | 失败后正在重连 |
| `Closed` | 终态 |

### 线协议

消息使用 MessagePack 序列化，带 4 字节大端序长度前缀：

```
[4 字节长度][MessagePack 载荷]
```

- **请求**: `{ id: UUID, method: String, params: Vec<u8> }`
- **响应**: `{ id: UUID, success: bool, payload: Vec<u8>, error: Option<String> }`
