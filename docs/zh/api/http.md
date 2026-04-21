# HTTP API

`dcc_mcp_core` — MCP Streamable HTTP 服务器（2025-03-26 规范）。

## 概述

`dcc-mcp-http` crate 提供了一个 MCP HTTP 服务器，将你的 `ToolRegistry` 通过 HTTP 暴露出来。MCP 客户端（如 Claude Desktop 或其他 LLM 集成）通过 HTTP POST 请求连接到 `/mcp` 端点。

::: tip 后台线程
服务器在后台 Tokio 线程中运行，不会阻塞 DCC 主线程。可安全用于 Maya/Blender 等插件中。
:::

## McpHttpConfig

HTTP 服务器配置。

### 构造函数

```python
from dcc_mcp_core import McpHttpConfig

cfg = McpHttpConfig(
    port=8765,                # TCP 端口（0 = 随机可用端口）
    server_name="maya-mcp",   # MCP initialize 响应中的名称
    server_version="1.0.0",   # MCP initialize 响应中的版本
    enable_cors=False,         # 浏览器客户端的 CORS 头
    request_timeout_ms=30000,  # 每个请求的超时时间（毫秒）
)
```

### 属性

| 属性 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `port` | `int` | `8765` | 服务器监听的 TCP 端口（`0` = OS 分配） |
| `host` | `str` | `"127.0.0.1"` | 绑定的 IP 地址 |
| `endpoint_path` | `str` | `"/mcp"` | MCP 端点路径 |
| `server_name` | `str` | `"dcc-mcp"` | MCP 响应中的服务器名称 |
| `server_version` | `str` | 包版本 | MCP 响应中的服务器版本 |
| `max_sessions` | `int` | `100` | 最大并发 SSE 会话数 |
| `request_timeout_ms` | `int` | `30000` | 每个请求的超时时间（毫秒） |
| `enable_cors` | `bool` | `False` | 是否启用浏览器客户端的 CORS |
| `session_ttl_secs` | `int` | `3600` | 空闲会话 TTL 秒数（0 禁用自动清理） |
| `gateway_port` | `int` | `0` | 竞争的网关端口（`0` = 禁用）。参见[网关](#网关) |
| `registry_dir` | `str \| None` | `None` | 共享 `FileRegistry` JSON 目录（默认 OS 临时目录） |
| `stale_timeout_secs` | `int` | `30` | 心跳超时秒数（实例视为过期） |
| `heartbeat_secs` | `int` | `5` | 心跳间隔秒数（`0` = 禁用） |
| `dcc_type` | `str \| None` | `None` | 注册表中报告的 DCC 类型（如 `"maya"`、`"blender"`） |
| `dcc_version` | `str \| None` | `None` | 注册表中报告的 DCC 版本（如 `"2025"`） |
| `scene` | `str \| None` | `None` | 当前打开的场景文件 — 改善网关路由 |

## McpServerHandle

由 `McpHttpServer.start()` 返回。用于获取 MCP 端点 URL 并优雅关闭。

::: tip 别名
`McpServerHandle` 在 `dcc_mcp_core` 中也以 `McpServerHandle` 别名导出，两者指向同一个类。

```python
from dcc_mcp_core import McpServerHandle  # McpServerHandle 的别名
```
:::

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `port` | `int` | 服务器实际绑定的端口（当 port=0 时有用） |
| `bind_addr` | `str` | 绑定地址，如 `"127.0.0.1:8765"` |
| `is_gateway` | `bool` | 若本进程赢得网关端口竞争则为 `True` |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `mcp_url()` | `str` | 完整的 MCP 端点 URL，如 `"http://127.0.0.1:8765/mcp"` |
| `shutdown()` | `None` | 优雅关闭（阻塞直到停止） |
| `signal_shutdown()` | `None` | 信号关闭而不阻塞 |

### 示例

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
registry.register("get_scene_info", description="获取当前场景信息",
                  category="scene", tags=[], dcc="maya", version="1.0.0")

server = McpHttpServer(registry, McpHttpConfig(port=8765))
handle = server.start()

print(f"MCP HTTP 服务器运行于 {handle.mcp_url()}")
# MCP 主机 POST 到 http://127.0.0.1:8765/mcp

# 完成后关闭
handle.shutdown()
```

## McpHttpServer

MCP Streamable HTTP 服务器（2025-03-26 规范）。

### 构造函数

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

server = McpHttpServer(
    registry,         # ToolRegistry 实例
    config=None,      # McpHttpConfig（默认 port=8765，无 CORS）
)
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `start()` | `McpServerHandle` | 在后台线程启动服务器并返回句柄 |
| `register_handler(action_name, handler)` | `None` | 注册 Python 可调用对象；处理器接收解码后的参数（通常是 `dict`） |
| `has_handler(action_name)` | `bool` | 检查是否已注册 Action 处理器 |

### MCP 协议端点

服务器实现 MCP 2025-03-26 规范：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/mcp` | POST | MCP 请求（JSON-RPC 2.0） |
| `/mcp` | GET | SSE 兼容的事件流 |
| `/mcp` | DELETE | 终止 MCP 会话 |
| `/health` | GET | 健康检查 |

### 请求/响应格式

MCP 请求使用 JSON-RPC 2.0：

```json
// POST /mcp
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/list",
  "params": {}
}
```

```json
// POST /mcp 响应
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {"name": "get_scene_info", "description": "获取当前场景信息", ...}
    ]
  }
}
```

### 支持的 MCP 方法

| 方法 | 说明 |
|------|------|
| `initialize` | 协议握手，返回服务器能力 |
| `tools/list` | 列出注册表中的所有 Action |
| `tools/call` | 按名称和参数调度 Action |
| `resources/list` | 列出可用资源（当前实现为空） |
| `prompts/list` | 列出可用提示（当前实现为空） |
| `ping` | 存活检查 |

## 完整示例：Maya MCP 服务器

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

# 构建工具注册表
registry = ToolRegistry()
registry.register(
    "get_scene_info",
    description="获取当前 Maya 场景信息",
    category="scene",
    tags=["query", "info"],
    dcc="maya",
    version="1.0.0",
    input_schema='{}',
)

def get_scene_info(params):
    # 实际中通过 pymel/cmdx 查询 Maya
    return {"scene_name": "untitled", "object_count": 0}

server = McpHttpServer(registry, McpHttpConfig(
    port=18812,
    server_name="maya-mcp",
    server_version="1.0.0",
))
server.register_handler("get_scene_info", get_scene_info)

# 启动 HTTP 服务器
handle = server.start()

print(f"Maya MCP 服务器: {handle.mcp_url()}")
# 输出: Maya MCP 服务器: http://127.0.0.1:18812/mcp
```

## 网关

当多个 DCC 实例同时启动时，其中一个会自动成为**网关** — 一个统一的 `/mcp` 入口点，将所有运行中实例的工具**聚合**成单一 MCP 接口。

### 工作原理

- 每个实例在共享的 `FileRegistry`（磁盘上的 JSON 文件）中注册自身，并定期发送心跳。
- **首个**绑定 `gateway_port`（默认：`9765`）的进程成为网关；其余为普通实例。
- 互斥使用 `SO_REUSEADDR=false`（通过 `socket2`），确保跨平台（包括 Windows）的首个获胜语义。
- 网关自动清理过期实例（在 `stale_timeout_secs` 内未收到心跳）。
- 进程退出时，`McpServerHandle` 被 drop，实例自动注销。

### 网关端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/instances` | GET | 所有活跃实例的 JSON 列表 |
| `/health` | GET | `{"ok": true}` 健康检查 |
| `/mcp` | POST | 聚合 MCP 端点（合并所有后端工具） |
| `/mcp` | GET | SSE 事件流 — 推送 `tools/list_changed` 与 `resources/list_changed` |
| `/mcp/{instance_id}` | POST | 透明代理到指定实例（底层逃生通道） |
| `/mcp/dcc/{dcc_type}` | POST | 代理到指定 DCC 类型的最佳实例 |

### 聚合式 Facade

网关的 `POST /mcp` 是一个统一 MCP 服务器，在单次 `tools/list` 响应中合并三层工具：

| 层级 | 工具 | 用途 |
|------|------|------|
| 发现元工具 | `list_dcc_instances`、`get_dcc_instance`、`connect_to_dcc` | 枚举 / 查看活跃 DCC；需要直连时返回直接 MCP URL |
| 技能管理 | `list_skills`、`find_skills`、`search_skills`、`get_skill_info`、`load_skill`、`unload_skill` | 读操作向全部 DCC 扇出；`load_skill` / `unload_skill` 通过 `instance_id` / `dcc` 参数指向具体实例 |
| 后端工具 | 所有活跃 DCC 自身的工具，带 8 字符实例前缀 — 例如 `a1b2c3d4__create_sphere` | 按前缀路由回原始后端 |

每个命名空间化的后端工具还会附带 `_instance_id`、`_instance_short`、`_dcc_type` 注解，以便 agent 消歧（比如 `create_cube` 在 Maya 和 Blender 上各注册一次时，会表现为两个带不同前缀的独立条目）。

网关声明 `capabilities.tools.listChanged: true`，每 3 秒轮询后端；当聚合集合发生变化（任何一处加载 / 卸载 skill）时，向所有连接的 SSE 客户端广播 `notifications/tools/list_changed`。

### Python 示例

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
registry.register("get_scene_info", description="获取场景信息", category="scene", dcc="maya")

config = McpHttpConfig(port=0, server_name="maya-mcp")
config.gateway_port = 9765    # 加入网关竞争；0 = 禁用
config.dcc_type = "maya"
config.dcc_version = "2025"
config.scene = "/proj/shot01.ma"  # 可选：帮助按场景路由

server = McpHttpServer(registry, config)
handle = server.start()

print(handle.is_gateway)        # True 表示本进程赢得了网关端口
print(handle.mcp_url())         # 本实例的直接 MCP URL
# → 若 is_gateway=True，网关在 http://127.0.0.1:9765/
# → 实例在 http://127.0.0.1:<port>/mcp
```

::: tip 多 DCC、单入口
启动任意数量的 DCC 服务器 — 第一个赢得网关端口。Agent 始终连接 `http://localhost:9765/mcp`，在单一 `tools/list` 中看到所有后端工具（按实例命名空间化）。需要直连、不经代理时再使用 `list_dcc_instances` / `connect_to_dcc`。
:::

::: info Skills-First + 网关
`create_skill_server()` 默认**不**配置 `gateway_port`。如需参与网关，需在传入的 `McpHttpConfig` 上显式设置：

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

config = McpHttpConfig(port=0, server_name="maya")
config.gateway_port = 9765
config.dcc_type = "maya"

server = create_skill_server("maya", config)
handle = server.start()
```
:::

## CORS 配置

为浏览器 MCP 客户端启用 CORS：

```python
cfg = McpHttpConfig(port=8765, enable_cors=True)
server = McpHttpServer(registry, cfg)
handle = server.start()
print(handle.mcp_url())
```

## 错误处理

服务器返回 JSON-RPC 错误响应：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params: missing 'radius'",
    "data": null
  }
}
```

常见错误码：

| 码 | 含义 |
|------|------|
| -32600 | 无效请求 |
| -32602 | 无效参数 |
| -32603 | 内部错误 |
| -32000 | Action 未找到 |
| -32001 | Action 验证失败 |
| -32002 | Action 处理器错误 |

## 性能说明

- 服务器在后台 Tokio 线程运行 — 不会阻塞 DCC 主线程
- 每个调用的请求超时（默认 30 秒）
- HTTP 层无连接池（每个 POST 无状态）
- 使用 `IpcChannelAdapter` 获取与 DCC 的持久 IPC 会话
- 网关 `FileRegistry` 每次变更都刷新到磁盘 — 多进程安全但不适合高频写入
