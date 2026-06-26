# translate 子命令 — 将 stdio MCP 服务器桥接到 HTTP/SSE

`translate` 子命令将任意 stdio MCP 服务器桥接到 HTTP/SSE/Streamable-HTTP 传输（issue #769）。

## 使用场景

- 将 `filesystem`、`git`、`sqlite`、`brave-search` 等只支持 stdio 的 MCP 服务器暴露为 HTTP 服务
- 让 Cursor、Claude Desktop 或任何 HTTP 优先的 Agent 连接到 stdio MCP 服务器
- 在单个网关端点后运行多个 stdio MCP 服务器
- 通过标准 HTTP 工具测试 stdio MCP 服务器

## 快速开始

```bash
dcc-mcp-server translate \
  --stdio "npx -y @modelcontextprotocol/server-filesystem /tmp" \
  --app-type filesystem \
  --port 3333
```

## CLI 参数

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--stdio <cmd>` | 必填 | 启动 stdio MCP 服务器的 Shell 命令 |
| `--app-type <type>` | `external` | 用于网关注册的应用类型标签 |
| `--expose-streamable-http <bool>` | `true` | 在 `/mcp` 暴露 Streamable HTTP |
| `--expose-sse <bool>` | `false` | 同时在 `/sse` 暴露旧版 SSE |
| `--port <N>` | `0`（OS 分配） | HTTP 监听端口 |
| `--host <addr>` | `127.0.0.1` | 监听地址 |
| `--no-register` | `false` | 跳过 FileRegistry / 网关注册 |
| `--restart-on-exit <bool>` | `true` | stdio 进程退出后自动重启；传 `false` 关闭 supervisor 模式 |
| `--max-restarts <N>` | `10` | supervisor 最大重启次数；`0` 表示无限 |
| `--gateway-port <N>` | `9765` | 网关注册竞争端口；`0` 禁用 gateway/admin |
| `--no-admin` | `false` | 禁用获选网关上的 Admin UI |
| `--admin-path <path>` | `/admin` | Admin UI URL 前缀 |
| `--stale-timeout-secs <N>` | `30` | 网关选举的过期超时 |
| `--registry-dir <path>` | 自动 | 自定义注册表目录 |

## 示例

### filesystem MCP 服务器

```bash
dcc-mcp-server translate \
  --stdio "npx -y @modelcontextprotocol/server-filesystem /home/user/projects" \
  --app-type filesystem \
  --port 4000
```

### 带 supervisor 重启的 git MCP 服务器

```bash
dcc-mcp-server translate \
  --stdio "uvx mcp-server-git --repository /path/to/repo" \
  --app-type git \
  --port 4001 \
  --max-restarts 10
```

### 独立模式（不注册网关）

```bash
dcc-mcp-server translate \
  --stdio "python -m my_mcp_server" \
  --no-register \
  --port 4002
```

## Cursor / Claude Desktop 配置

将 AI 客户端指向默认的 Streamable HTTP 端点：

```json
// .cursor/mcp.json 或 claude_desktop_config.json
{
  "mcpServers": {
    "filesystem": {
      "url": "http://localhost:4000/mcp",
      "transport": "streamable-http"
    },
    "git": {
      "url": "http://localhost:4001/mcp",
      "transport": "streamable-http"
    }
  }
}
```

旧版 SSE 客户端需要启动时传 `--expose-sse true`，并指向 `/sse`：

```json
{
  "mcpServers": {
    "filesystem": {
      "url": "http://localhost:4000/sse"
    }
  }
}
```

## 实现说明

- **异步 Actor 模型**：一个 Tokio 任务通过 mpsc channel 管理子进程的 stdin/stdout
- **并发请求**：通过请求/响应 ID 追踪支持多个并发请求
- **通知处理**：无 `id` 字段的 JSON 消息（通知）转发给所有已连接的 SSE 客户端
- **Supervisor 重启**：采用指数退避策略（最大间隔 30 秒）
- **网关注册**：若未指定 `--no-register`，桥接器会作为 DCC 实例参与网关选举

## 参见

- [gateway.md](gateway.md) — 网关注册与选举
- [tunnel-relay.md](tunnel-relay.md) — 外部/互联网访问的远程中继
- [rest-api-surface.md](rest-api-surface.md) — 每个 DCC 的 REST API 接口
