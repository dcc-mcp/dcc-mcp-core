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
| `gateway_port` | `int` | `9765`（Python） | 竞争的网关端口（`0` = 禁用）。参见[网关](#网关) |
| `admin_enabled` | `bool` | `True` | 赢得选举的 gateway 提供本地 Admin UI（`GET /admin`） |
| `admin_path` | `str` | `"/admin"` | Admin UI 的 URL 前缀 |
| `registry_dir` | `str \| None` | `None` | 共享 `FileRegistry` JSON 目录（默认 OS 临时目录） |
| `stale_timeout_secs` | `int` | `30` | 心跳超时秒数（实例视为过期） |
| `heartbeat_secs` | `int` | `5` | 心跳间隔秒数（`0` = 禁用） |
| `dcc_type` | `str \| None` | `None` | 注册表中报告的 DCC 类型（如 `"maya"`、`"blender"`） |
| `dcc_version` | `str \| None` | `None` | 注册表中报告的 DCC 版本（如 `"2025"`） |
| `scene` | `str \| None` | `None` | 当前打开的场景文件 — 改善网关路由 |

::: tip Admin 持久化
`McpHttpConfig` 控制获选网关是否提供 `/admin`；Admin 持久化刻意通过环境变量配置。设置 `DCC_MCP_GATEWAY_AUDIT_DIR` 后，`/admin/api/calls` 行会保存到 `audit.jsonl`，`/admin/api/traces` 行会保存到 `traces.jsonl`；`DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` 限制每个文件的行数。
:::

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
| `has_handler(action_name)` | `bool` | 检查是否已注册 Tool 处理器 |

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
| `resources/list` | 列出可用资源；网关会包含 `gateway://instances` 根指针 |
| `prompts/list` | 列出已注册 prompt；网关返回带命名空间的后端 prompt 模板 |

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

当多个 DCC 实例同时启动时，其中一个会自动成为**网关** — 一个统一的 `/mcp` 入口点和 `/v1/*` REST 门面。自 v0.15 起，网关不会把所有后端 action 合并进 `tools/list`；它保持 MCP 表面有界，并通过 search/describe/call 原语路由后端能力。

### 工作原理

- 每个实例在共享的 `FileRegistry`（磁盘上的 JSON 文件）中注册自身，并定期发送心跳。
- **首个**绑定 `gateway_port`（Python API 与 `dcc-mcp-server` 默认：`9765`）的进程成为网关；其余为普通实例。
- 互斥使用 `SO_REUSEADDR=false`（通过 `socket2`），确保跨平台（包括 Windows）的首个获胜语义。
- 网关自动清理过期实例（在 `stale_timeout_secs` 内未收到心跳）。
- 进程退出时，`McpServerHandle` 被 drop，实例自动注销。

### 网关端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/instances` | GET | 所有活跃实例的 JSON 列表 |
| `/v1/instances` | GET | 实例发现的 REST 别名 |
| `/health` | GET | `{"ok": true}` 健康检查 |
| `/mcp` | POST | 有界 MCP 端点，只暴露网关发现原语（`search`、`describe`） |
| `/mcp` | GET | SSE 事件流 — 进度、作业/工作流、资源、prompt 通知 |
| `/v1/search` | POST | 搜索紧凑后端能力记录 |
| `/v1/describe` | POST | 获取一个 `tool_slug` 的 schema、annotations 与路由记录 |
| `/v1/call` | POST | 通过 `tool_slug` 调用一个后端能力 |
| `/mcp/{instance_id}` | POST | 透明代理到指定实例（底层逃生通道） |
| `/mcp/dcc/{dcc_type}` | POST | 代理到指定 DCC 类型的最佳实例 |

### 有界 Facade

网关的 `POST /mcp` 是一个统一 MCP 服务器，在 `tools/list` 中只广告只读网关工具：

| 层级 | 工具 | 用途 |
|------|------|------|
| 发现 | `search`、`describe` | 搜索紧凑后端能力记录，并为一个 `tool_slug` 或 `skill_name` 拉取 schema/detail |

后端 action 通过 `tool_slug`（`<dcc>.<id8>.<tool>`）寻址。Agent 不应手写 slug；应从 MCP `search` 或 `POST /v1/search` 获取，用 MCP `describe` 或 `POST /v1/describe` 检查 schema，然后通过 `POST /v1/call` 或 `POST /v1/call_batch` 执行。隐藏 MCP 兼容路由仍接受旧的 `search_tools` / `describe_tool` / `call_tool` / `call_tools` 名称，但它们不再出现在 `tools/list`。

#### `gateway://instances` —— 把 DCC 注册表暴露为 MCP 资源（#813 phase 1）

实时 DCC 注册表通过网关原生的 MCP 资源暴露，而不是工具。Agent 通过 `resources/read` 获取，不再为"实例发现"动词支付 `tools/list` 的 token 成本。

```jsonc
// 请求：列出 `$TEMP/dcc-mcp-registry/` 中所有可解析的注册行（不过滤
// `dcc_type`）。stale 哨兵以 `status: "stale"` 显式呈现，让运维知道
// 该注册为何无法路由，而不是被静默丢弃。
{"jsonrpc":"2.0","id":1,"method":"resources/read",
 "params":{"uri":"gateway://instances"}}

// 可选 URI 查询参数：隐藏 stale 行 / 显示原始注册视图
// （默认：stale 可见，dead-PID 行被裁剪）。
//   gateway://instances?include_stale=false
//   gateway://instances?include_dead=true
```

`contents[0].text` 中返回的 JSON 形如：

```json
{
  "total": 3,
  "stale_count": 1,
  "evicted_dead": 0,
  "instances": [
    {
      "instance_id": "a1b2c3d4-…",
      "dcc_type": "maya",
      "host": "127.0.0.1",
      "port": 18812,
      "mcp_url": "http://127.0.0.1:18812/mcp",
      "status": "available",
      "scene": "/proj/shot01.ma",
      "documents": [],
      "pid": 1234,
      "display_name": "Maya-Rigging",
      "version": "2024",
      "adapter_version": "0.3.0",
      "adapter_dcc": "maya",
      "metadata": {},
      "stale": false
    }
  ]
}
```

每条记录已经携带 `mcp_url`，因此读取这个资源的客户端拥有连接所需的全部信息 —— 不再需要单独的 "connect" 动词。要查看单个实例，读取 `gateway://instances/{instance_id}`（完整 UUID 或唯一前缀）即可。

簿记用的 `__gateway__` 哨兵行和网关自身的注册行始终被过滤。

### Python 示例

```python
from dcc_mcp_core import ToolRegistry, McpHttpServer, McpHttpConfig

registry = ToolRegistry()
registry.register("get_scene_info", description="获取场景信息", category="scene", dcc="maya")

config = McpHttpConfig(port=0, server_name="maya-mcp")
# Python 默认：gateway_port=9765、admin_enabled=True、admin_path="/admin"。
# 设置 gateway_port=0 可禁用 gateway/admin；或设置 admin_enabled=False 只保留 gateway。
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
启动任意数量的 DCC 服务器 — 第一个赢得网关端口。Agent 连接 `http://localhost:9765/mcp` 后，先用 `search` 发现后端能力，用 `describe` 检查 schema，再通过 `POST /v1/call` 执行。需要直连、不经代理时，读取 `gateway://instances` MCP 资源即可拿到每个后端的 `mcp_url`。
:::

::: info Skills-First + 网关
`create_skill_server()` 使用传入的 `McpHttpConfig`。Python 新建的 `McpHttpConfig` 默认参与网关选举（`gateway_port=9765`），且赢得选举的进程默认提供 Admin。若需要隔离服务，可设置 `gateway_port=0`：

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

config = McpHttpConfig(port=0, server_name="maya")
# config.gateway_port = 0        # 取消注释可禁用 gateway/admin
# config.admin_enabled = False   # 保留 gateway，但隐藏 Admin UI
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
| -32002 | Tool 处理器错误 |

## 性能说明

- 服务器在后台 Tokio 线程运行 — 不会阻塞 DCC 主线程
- 每个调用的请求超时（默认 30 秒）
- HTTP 层无连接池（每个 POST 无状态）
- 使用 `IpcChannelAdapter` 获取与 DCC 的持久 IPC 会话
- 网关 `FileRegistry` 每次变更都刷新到磁盘 — 多进程安全但不适合高频写入
