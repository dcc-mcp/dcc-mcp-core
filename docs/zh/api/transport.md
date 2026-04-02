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
