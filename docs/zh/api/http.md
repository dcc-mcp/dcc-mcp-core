# HTTP API

`dcc_mcp_core` — MCP Streamable HTTP 服务器（2025-03-26 规范）。

## 概述

`dcc-mcp-http` crate 提供了一个 MCP HTTP 服务器，将你的 `ActionRegistry` 通过 HTTP 暴露出来。MCP 客户端（如 Claude Desktop 或其他 LLM 集成）通过 HTTP POST 请求连接到 `/mcp` 端点。

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

| 属性 | 类型 | 说明 |
|------|------|------|
| `port` | `int` | 服务器监听的 TCP 端口 |
| `server_name` | `str` | MCP 响应中的服务器名称 |
| `server_version` | `str` | MCP 响应中的服务器版本 |

## ServerHandle

由 `McpHttpServer.start()` 返回。用于获取 MCP 端点 URL 并优雅关闭。

::: tip 别名
`ServerHandle` 在 `dcc_mcp_core` 中也以 `McpServerHandle` 别名导出，两者指向同一个类。

```python
from dcc_mcp_core import McpServerHandle  # ServerHandle 的别名
```
:::

### 属性

| 属性 | 类型 | 说明 |
|------|------|------|
| `port` | `int` | 服务器实际绑定的端口（当 port=0 时有用） |
| `bind_addr` | `str` | 绑定地址，如 `"127.0.0.1:8765"` |

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `mcp_url()` | `str` | 完整的 MCP 端点 URL，如 `"http://127.0.0.1:8765/mcp"` |
| `shutdown()` | `None` | 优雅关闭（阻塞直到停止） |
| `signal_shutdown()` | `None` | 信号关闭而不阻塞 |

### 示例

```python
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

registry = ActionRegistry()
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
from dcc_mcp_core import ActionRegistry, McpHttpServer, McpHttpConfig

server = McpHttpServer(
    registry,         # ActionRegistry 实例
    config=None,      # McpHttpConfig（默认 port=8765，无 CORS）
)
```

### 方法

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `start()` | `ServerHandle` | 在后台线程启动服务器并返回句柄 |

### MCP 协议端点

服务器实现 MCP 2025-03-26 规范：

| 端点 | 方法 | 说明 |
|------|------|------|
| `/mcp` | POST | MCP 请求（JSON-RPC 2.0） |
| `/mcp` | GET | SSE 兼容的事件流 |
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
from dcc_mcp_core import (
    ActionRegistry, ActionDispatcher, McpHttpServer, McpHttpConfig,
)

# 构建 Action 注册表
registry = ActionRegistry()
registry.register(
    "get_scene_info",
    description="获取当前 Maya 场景信息",
    category="scene",
    tags=["query", "info"],
    dcc="maya",
    version="1.0.0",
    input_schema='{}',
)

# 注册处理器
dispatcher = ActionDispatcher(registry)

def get_scene_info(params):
    # 实际中通过 pymel/cmdx 查询 Maya
    return {"scene_name": "untitled", "object_count": 0}

dispatcher.register_handler("get_scene_info", get_scene_info)

# 启动 HTTP 服务器
config = McpHttpConfig(
    port=18812,
    server_name="maya-mcp",
    server_version="1.0.0",
)
server = McpHttpServer(registry, config)
handle = server.start()

print(f"Maya MCP 服务器: {handle.mcp_url()}")
# 输出: Maya MCP 服务器: http://127.0.0.1:18812/mcp
```

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
- 使用 `TransportManager` 获取与 DCC 的持久 IPC 会话
