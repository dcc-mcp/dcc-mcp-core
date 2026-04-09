# 传输层

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
