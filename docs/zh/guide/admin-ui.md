# 内置 Admin 仪表盘

网关内置一个零构建、零依赖的 `/admin` Web 仪表盘（issue #772）。无需 `npm`，无需 CDN——一个内联 HTML 文件直接从二进制包中提供服务。

## 激活方式

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,
    admin_path: "/admin".to_string(),  // 默认值
    ..GatewayConfig::default()
};
```

需要启用 `admin` Cargo feature：

```toml
[dependencies]
dcc-mcp-gateway = { features = ["admin"] }
```

默认值：`admin_enabled = false`（出于安全考虑，需主动开启）。

## 路由

| 路由 | Content-Type | 说明 |
|------|-------------|------|
| `GET /admin` | `text/html` | HTML 仪表盘（内联 CSS + vanilla JS） |
| `GET /admin/api/instances` | `application/json` | 已连接的 DCC 实例 |
| `GET /admin/api/tools` | `application/json` | 已注册的 MCP 工具 |
| `GET /admin/api/calls` | `application/json` | 最近的工具调用（需要 `AuditMiddleware`） |
| `GET /admin/api/logs` | `application/json` | 网关竞争事件 |
| `GET /admin/api/health` | `application/json` | 服务健康摘要 |

## API 响应格式

```json
// GET /admin/api/health
{
  "status": "ok",
  "uptime_secs": 3600,
  "instances_total": 3,
  "instances_ready": 2
}

// GET /admin/api/instances
{
  "total": 3,
  "instances": [
    { "id": "a1b2c3d4-...", "dcc_type": "maya", "status": "ready", "address": "127.0.0.1:9001" }
  ]
}

// GET /admin/api/calls  （需要 AuditMiddleware）
{
  "total": 42,
  "calls": [
    { "tool": "maya__open_scene", "success": true, "timestamp": "2026-05-05T10:00:00Z" }
  ]
}

// GET /admin/api/logs
{
  "total": 5,
  "logs": [
    { "event": "election_won", "dcc_type": "maya", "timestamp": "2026-05-05T09:59:00Z" }
  ]
}
```

## 接入 AuditMiddleware

要让 `/admin/api/calls` 数据源有内容，需要在中间件链中添加 `AuditMiddleware`：

```rust
use dcc_mcp_gateway::gateway::middleware::{AuditMiddleware, MiddlewareChain};

GatewayConfig {
    admin_enabled: true,
    middleware_chain: MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::default())),
    ..GatewayConfig::default()
}
```

`/admin/api/logs` 数据源由 `EventLog` 环形缓冲区自动填充（网关选举/驱逐/探针事件，来自 issue #766）。

## 仪表盘功能

HTML 仪表盘包含：
- **左侧导航**：实例 / 工具 / 调用 / 日志 / 健康 面板
- **自动刷新**：每个面板每 5 秒轮询对应 JSON 端点
- **深色主题**：极简内联 CSS，无外部字体
- **响应式布局**：CSS grid 布局

## 安全注意事项

Admin UI 是**只读**的，默认**无认证**。生产环境建议：
- 通过反向代理添加 IP 白名单或 Basic Auth
- 不需要时禁用：`admin_enabled: false`
- 切勿直接暴露到公网

## 参见

- [middleware.md](middleware.md) — 填充 `/admin/api/calls` 的 `AuditMiddleware`
- [observability.md](observability.md) — 填充 `/admin/api/logs` 的 `EventLog`
- [gateway.md](gateway.md) — 完整的网关配置参考
