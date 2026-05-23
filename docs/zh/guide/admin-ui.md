# 内置 Admin 仪表盘

网关内置一个嵌入式 `/admin` Web 仪表盘（issue #772）。运行时仍由二进制提供单个 HTML 资产；贡献者在 `admin-ui/` 中维护 Vite/React 源码，`crates/dcc-mcp-gateway/build.rs` 会在 Cargo 构建时生成并嵌入产物。

## 启用方式与默认值

`/admin` 默认在赢得网关选举的进程上启用。这是有意的：gateway 与 admin 仪表盘属于默认本地可观测能力。

### `dcc-mcp-server` / `server.exe`

```bash
# 默认：参与 :9765 网关选举；赢得选举的进程提供 /admin
dcc-mcp-server --app maya

# 完全禁用网关（同时禁用 admin）
dcc-mcp-server --gateway-port 0

# 保留网关，但禁用 admin
dcc-mcp-server --no-admin

# 将 admin 挂载到其他路径
dcc-mcp-server --admin-path /dcc-admin
```

等价环境变量：

| 环境变量 | 默认值 | 说明 |
|----------|--------|------|
| `DCC_MCP_GATEWAY_PORT` | `9765` | 网关选举端口；`0` 表示禁用 gateway/admin。 |
| `DCC_MCP_NO_ADMIN` | `false` | 禁用赢得选举的网关上的只读 Admin UI。 |
| `DCC_MCP_ADMIN_PATH` | `/admin` | Admin URL 前缀。 |
| `DCC_MCP_GATEWAY_AUDIT_DIR` | 未设置 | 可选 JSONL 持久化目录，写入 `audit.jsonl` 与 `traces.jsonl`；未设置时保持零落盘的内存行为。 |
| `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` | `5000` | 启用持久化时，每个 JSONL 文件保留的最大行数。 |
| `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` | `52428800` | 每个持久化 JSONL 文件约 50 MiB 的字节上限；网关同时执行行数和字节数裁剪。 |
| `DCC_MCP_LOG_DIR` | 平台日志目录 | `/admin/api/logs` 扫描 `*.log` 文件的目录；Windows 默认 `%USERPROFILE%\\AppData\\Local\\dcc-mcp\\log`，其他平台默认 `~/.local/share/dcc-mcp/log`。 |

### Python API

```python
from dcc_mcp_core import McpHttpConfig, McpHttpServer, ToolRegistry

cfg = McpHttpConfig(port=0, server_name="maya-mcp")
# Python 嵌入侧默认值：
# cfg.gateway_port == 9765
# cfg.admin_enabled is True
# cfg.admin_path == "/admin"

# 隔离的本地单实例：禁用 gateway/admin
cfg.gateway_port = 0

# 或保留 gateway，但隐藏 admin
cfg.admin_enabled = False

server = McpHttpServer(ToolRegistry(), cfg)
handle = server.start()
```

### Rust Gateway API

```rust
use dcc_mcp_gateway::gateway::GatewayConfig;

let config = GatewayConfig {
    admin_enabled: true,          // 默认值
    admin_path: "/admin".into(),  // 默认值
    ..GatewayConfig::default()
};
```

直接使用 `dcc-mcp-gateway` 时需要启用 `admin` Cargo feature。`dcc-mcp-http` 与发布的 server 二进制已在内嵌网关路径中启用。

## 仪表盘截图

以下截图使用代表性的演示数据，展示嵌入式 Admin Dashboard 中面向浏览器的运维工作流。

![Admin Connect IDE 面板](../../assets/admin-ui/admin-connect-ide.png)

**Connect IDE** 面板会基于当前 gateway URL 生成 Claude Desktop、Cursor、CodeBuddy、VS Code、Cline 和 Codex / OpenAI 的 MCP 配置片段，方便直接复制到本地 IDE/Agent。

![Admin Skills 路径面板](../../assets/admin-ui/admin-skills-paths.png)

**Skills** 面板展示当前已加载的 skills、action 数量、后端实例前缀、活动发现路径，以及本地开发默认路径 `~/.dcc-mcp/{dcc-type}/skills`（存在时）。

![Admin Skill Markdown 详情面板](../../assets/admin-ui/admin-skill-detail.png)

点击某个 skill 会打开详情面板，显示后端实例信息、注册工具、`SKILL.md` 源路径、frontmatter，以及渲染后的 Markdown 正文，方便开发者 review。

## 路由

| 路由 | Content-Type | 说明 |
|------|-------------|------|
| `GET /admin` | `text/html` | 以单个 HTML 资产提供的嵌入式 React/Vite 仪表盘 |
| `GET /admin/api/activity?limit=300` | `application/json` | 由审计、trace 和 gateway 事件合并得到的统一活动时间线 |
| `GET /admin/api/instances` | `application/json` | 已连接的 DCC 实例 |
| `GET /admin/api/tools` | `application/json` | 已注册的 MCP 工具 |
| `GET /admin/api/tasks?limit=300` | `application/json` | 从 dispatch traces 重建出的任务视图 |
| `GET /admin/api/calls` | `application/json` | 最近的工具调用（需要 `AuditMiddleware`） |
| `GET /admin/api/traces` | `application/json` | 最近的逐调用 dispatch traces；支持 `?limit=200` |
| `GET /admin/api/traces/{request_id}` | `application/json` | 某次调用的完整 waterfall trace |
| `GET /admin/api/debug-bundle/{request_id}` | `application/json` | 单次请求的一站式 debug bundle，包含 trace、匹配审计行、相关活动和提示 |
| `GET /admin/api/stats?range=1h\|24h\|7d` | `application/json` | 聚合调用数、成功率、延迟和 top tools/instances |
| `GET /admin/api/workers` | `application/json` | 来自 live registry 的实例 worker 卡片 |
| `GET /admin/api/logs` | `application/json` | 合并后的网关竞争事件、磁盘 `*.log` 行和审计调用摘要 |
| `GET /admin/api/health` | `application/json` | 服务健康摘要 |
| `GET /admin/api/skills` | `application/json` | 按 DCC 类型、skill 名、加载状态、工具和后端实例聚合的实时 skill 清单 |
| `GET /admin/api/skill-detail?name=...` | `application/json` | 单个 skill 的后端详情；可用时包含用于 review 的 `SKILL.md` Markdown 内容 |
| `GET /admin/api/skill-paths` | `application/json` | 当前 skill 发现根目录，包括环境变量、本地默认路径、内置路径和 admin custom 路径 |
| `POST /admin/api/skill-paths` | `application/json` | 添加 SQLite 持久化的自定义 skill 发现根目录，并刷新 live backend skill index |
| `DELETE /admin/api/skill-paths/{id}` | `application/json` | 删除 SQLite 持久化的自定义 skill 发现根目录，并刷新 live backend skill index |

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

// GET /admin/api/activity?limit=300
{
  "total": 2,
  "events": [
    {
      "event_id": "audit:req-123",
      "timestamp": "2026-05-05T10:00:00Z",
      "kind": "tool_call",
      "severity": "info",
      "status": "ok",
      "message": "tools/call maya__open_scene",
      "tool": "maya__open_scene",
      "duration_ms": 48,
      "correlation": {
        "request_id": "req-123",
        "session_id": "session-1",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/tasks?limit=300
{
  "total": 1,
  "tasks": [
    {
      "task_id": "req-123",
      "task_type": "tool_call",
      "status": "completed",
      "title": "maya__open_scene",
      "started_at": "2026-05-05T10:00:00Z",
      "duration_ms": 48,
      "correlation": {
        "request_id": "req-123",
        "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
        "dcc_type": "maya"
      }
    }
  ]
}

// GET /admin/api/calls  （需要 AuditMiddleware）
{
  "total": 42,
  "calls": [
    {
      "request_id": "req-123",
      "method": "tools/call",
      "tool": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "session_id": "session-1",
      "success": false,
      "error": "backend timeout",
      "timestamp": "2026-05-05T10:00:00Z"
    }
  ]
}

// GET /admin/api/traces?limit=200
{
  "total": 1,
  "traces": [
    {
      "request_id": "req-123",
      "method": "tools/call",
      "tool_slug": "maya.abcdef01.maya__open_scene",
      "dcc_type": "maya",
      "total_ms": 48,
      "ok": true,
      "spans": [
        { "name": "gateway.route", "duration_ns": 1200000, "ok": true, "attributes": {} }
      ],
      "input": { "mime_type": "application/json", "truncated": false, "original_size": 42, "content": "{...}" },
      "output": { "mime_type": "application/json", "truncated": false, "original_size": 96, "content": "{...}" }
    }
  ]
}

// GET /admin/api/traces/req-123 返回同一个完整 trace 对象；未命中时返回 404。

// GET /admin/api/debug-bundle/req-123
{
  "request_id": "req-123",
  "trace": { "request_id": "req-123", "spans": [] },
  "audit": { "request_id": "req-123", "success": true },
  "related_activity": [],
  "hints": []
}

// GET /admin/api/stats?range=24h
{
  "range": "24h",
  "total_calls": 42,
  "success_rate": 0.98,
  "latency": { "p50_ms": 12, "p95_ms": 48 }
}

// GET /admin/api/workers
{
  "summary": { "live": 2, "stale": 0, "unhealthy": 0 },
  "workers": [
    { "instance_id": "a1b2c3d4-...", "dcc_type": "maya", "status": "available" }
  ]
}

// GET /admin/api/logs
{
  "total": 5,
  "logs": [
    {
      "timestamp": "2026-05-05T09:59:00Z",
      "level": "info",
      "message": "tools/call ok 12ms — maya__open_scene",
      "source": "audit",
      "dcc_type": "maya",
      "instance_id": "abcdef01-2345-6789-abcd-ef0123456789",
      "request_id": "req-123",
      "tool": "maya__open_scene",
      "success": true,
      "detail": "instance=abcdef01-2345-6789-abcd-ef0123456789"
    }
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

`/admin/api/logs` 数据源由三类有界来源自动合并：`EventLog` 环形缓冲区（网关选举/驱逐/探针事件，来自 issue #766）、`DCC_MCP_LOG_DIR` 或平台默认日志目录下的 `*.log` 文件，以及最近的 `AuditMiddleware` 调用行。`/admin/api/traces`、`/admin/api/stats` 和 `/admin/api/workers` 分别来自 dispatch `TraceLog`、`StatsAggregator` 与 live gateway registry。

设置 `DCC_MCP_GATEWAY_AUDIT_DIR` 后会启用 JSONL 持久化。网关会将有界的 admin 调用行追加到 `audit.jsonl`，将 dispatch traces 追加到 `traces.jsonl`，并同时按 `DCC_MCP_GATEWAY_AUDIT_MAX_ROWS` 和 `DCC_MCP_GATEWAY_AUDIT_MAX_BYTES` 裁剪每个文件，然后在重启时用这些文件回填内存中的 admin 缓冲区。持久化内容仍使用内存 trace 捕获同一套有界/已脱敏 `TracePayload`，不会写入无界原始请求体。

## 仪表盘功能

HTML 仪表盘包含：
- **Debug Workbench**：默认首屏会组合 health、instances、calls、traces、stats 和 warning logs，方便排查 gateway 故障时不用在多个面板之间来回跳转
- **Gateway owner identity**：Health 和 Debug 面板会展示来自 `gateway_name` / `DCC_MCP_GATEWAY_NAME` 的当前 `__gateway__` sentinel 标签，以及 challenger 候选
- **左侧导航**：Debug / Activity / Health / 实例 / 工具 / Tasks / Calls / Traces / Stats / Skill paths / 日志面板
- **自动刷新**：每个面板每 5 秒轮询对应 JSON 端点
- **DCC 图标**：Maya/Autodesk、Blender、GIMP、Inkscape、Krita、Unity、Unreal 等常见宿主显示可识别图标，自定义宿主使用安全 fallback
- **Worker 卡片**：按实例展示状态、心跳与路由元数据
- **Calls 表格**：展示 request id、错误摘要与 trace detail 链接；DCC 优先从解析后的 backend slug 展示，其次使用调用参数中的 `dcc` / `dcc_type`
- **Trace 下钻**：`/admin/api/traces/{request_id}` 暴露单次调用的完整 waterfall，以及有界/已脱敏的输入输出 payload
- **Logs 面板**：将标准化的 `contention`、`file`、`audit` 行分组，方便在一条时间线里关联路由事件、滚动日志和工具调用。文件日志读取会限制为最近文件与尾部片段，避免 admin API 扫描无界历史日志
- **可选持久化**：`DCC_MCP_GATEWAY_AUDIT_DIR` 可让 Calls 与 Traces 面板跨重启保留，且不改变 JSON API 结构
- **深色主题**：Vite/React 源码，运行时嵌入资产，不要求现场构建
- **响应式布局**：窄屏会切换为顶部导航，debug 卡片和图表保持可用的单列宽度

## 安全注意事项

Admin UI 是**只读**的，默认**无认证**。它绑定在赢得选举的 gateway 所使用的 host 上，而默认 host 是 `127.0.0.1`。生产环境建议：
- 保持 localhost 绑定，或通过反向代理添加 IP 白名单 / Basic Auth
- 不需要时禁用：`--no-admin`、`DCC_MCP_NO_ADMIN=true` 或 `cfg.admin_enabled = False`
- 切勿直接暴露到公网

## 参见

- [middleware.md](middleware.md) — 填充 `/admin/api/calls` 的 `AuditMiddleware`
- [observability.md](observability.md) — 填充 `/admin/api/logs` 的 `EventLog`
- [gateway.md](gateway.md) — 完整的网关配置参考
