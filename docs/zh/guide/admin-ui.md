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

## Locale 检测

内嵌 Admin UI 带有一个随 bundle 发布的小型 i18n runtime。它读取
`navigator.languages` / `navigator.language`，把浏览器语言标签规范化为
受支持的 locale；没有匹配项时回退到英文。运行时不会从网络加载额外翻译资产。

当前支持的 runtime locale：

- `en`
- `zh-CN`，覆盖 `zh`、`zh-Hans`、`zh-CN` 等简体中文标签
- `ja`，覆盖 `ja` / `ja-JP`
- `ko`，覆盖 `ko` / `ko-KR`

翻译条目按 feature namespace 放在 `admin-ui/src/i18n.ts`。共享按钮、表格、
状态提示和搜索元数据优先放在 `common` 或 `search`；面板自己的文案放在
对应 namespace（例如 `setup`、`health`、`openapi`、`traces`、`stats`、
`governance`、`logs`、`skillPaths`）。

维护翻译时：

1. 新增或修改面板时，先新增/更新对应 feature namespace。
2. 每个 key 都必须同时提供 `en`、`zh-CN`、`ja`、`ko`。
3. 动态文本使用 `{count}` / `{value}` 等插值占位，不要在 React 里拼接翻译句子。
4. 不要翻译机器标识或原始技术 payload：tool slug、request ID、trace ID、
   DCC 类型、文件路径、JSON key、HTTP method、status code、日志消息和后端返回的
   payload 文本都应保持原样。

i18n 相关修改建议运行：

```bash
vx npm run build
vx npx playwright test tests/i18n.spec.ts tests/admin.spec.ts
vx just admin-build
vx git diff --check
```

`tests/i18n.spec.ts` 会验证所有支持 locale 的检测和 namespace key parity；
`tests/admin.spec.ts` 使用 mock admin API 覆盖英文与非英文浏览器 locale 下的
真实面板流程，确保 UI chrome 被本地化，同时机器数据保持稳定。

## 仪表盘截图

以下截图使用代表性的演示数据，展示嵌入式 Admin Dashboard 中面向浏览器的运维工作流。

![Admin Connect IDE 面板](../../assets/admin-ui/admin-connect-ide.png)

**Connect IDE** 面板会基于当前 gateway URL 和当前平台的配置路径，生成 Claude Desktop、Cursor、CodeBuddy、VS Code、Cline 和 Codex / OpenAI 的 MCP 配置片段，方便直接复制到本地 IDE/Agent。

![Admin Skills 路径面板](../../assets/admin-ui/admin-skills-paths.png)

**Skills** 面板展示当前已加载的 skills、action 数量、后端实例前缀、活动发现路径，以及本地开发默认路径 `~/.dcc-mcp/{dcc-type}/skills`（存在时）。

![Admin Skill Markdown 详情面板](../../assets/admin-ui/admin-skill-detail.png)

点击某个 skill 会打开详情面板，显示后端实例信息、注册工具、`SKILL.md` 源路径、frontmatter，以及渲染后的 Markdown 正文，方便开发者 review。

## 市场面板（Marketplace）

**Marketplace** 面板位于左侧导航栏，为浏览、安装和管理技能包提供图形界面。
它与 CLI `marketplace` 子命令共享同一套后端能力，通过两个标签页展现：
**浏览（Browse）** 和 **已安装（Installed）**。

### 浏览标签页

浏览标签页以可搜索的目录网格展示可用技能包。每个卡片显示：
包名、描述、DCC 类型徽章、当前版本和安装按钮。

用户可通过以下方式筛选目录：
- **搜索查询**：按名称、描述和标签进行文本搜索。
- **DCC 类型筛选**：标签页顶部的 Chip 按钮行，可按 DCC 类型筛选。搜索和
  DCC 筛选可以组合使用。

点击卡片会打开 **Marketplace 详情弹窗**，展示完整元数据：名称、描述、
版本、标签、DCC 类型、维护者、项目 URL、来源、安装类型（git、zip、path）
和 `min_core_version`。当包的 `min_core_version` 低于当前运行的核心版本时，
会显示兼容性警告。

从详情弹窗或卡片安装包会触发市场安装流程。安装成功后，页面内联显示一条
成功通知，其中包含 **View in Skills** 深度链接，点击可跳转到 Skills 面板
并高亮新加载的 skill。如果后端报告了 `reload_required` 标志，技能索引会
自动刷新。

### 已安装标签页

已安装标签页以卡片形式列出所有已安装的市场包，显示：
- 包名和版本
- DCC 类型
- 安装类型（git、zip、path）
- 卸载按钮

用户可直接从此标签页卸载包。卸载成功后，技能索引会刷新以移除已删除的包。

### API 端点

| 路由 | Content-Type | 说明 |
|------|-------------|------|
| `GET /admin/api/marketplace/catalog` | `application/json` | 列出所有已配置来源中的可用包。返回 `{ entries: MarketplaceEntry[] }`。 |
| `GET /admin/api/marketplace/installed` | `application/json` | 列出本地已安装的包（按 DCC 分组）。返回 `{ packages: InstalledMarketplacePackage[] }`。 |
| `POST /admin/api/marketplace/install` | `application/json` | 安装包。请求体：`{ name, dcc, source?, force? }`。返回带 `reload_required` 标志的结果。 |
| `POST /admin/api/marketplace/uninstall` | `application/json` | 卸载包。请求体：`{ name, dcc }`。返回带 `reload_required` 标志的结果。 |
| `GET /admin/api/marketplace/sources` | `application/json` | 列出已配置的市场来源及其来源类型（builtin、config、env）。 |
| `POST /admin/api/marketplace/sources` | `application/json` | 添加市场来源到持久化配置。请求体：`{ source }`。重复添加是幂等的。 |
| `GET /admin/api/marketplace/outdated` | `application/json` | 列出有更新版本的已安装包。支持 `?name=&dcc=` 筛选参数。 |
| `POST /admin/api/marketplace/update` | `application/json` | 更新一个或全部过期包。请求体：`{ name?, dcc?, all? }`。返回每个结果的 `reload_required` 标志。 |

## 路由

| 路由 | Content-Type | 说明 |
|------|-------------|------|
| `GET /admin` | `text/html` | 以单个 HTML 资产提供的嵌入式 React/Vite 仪表盘 |
| `GET /admin/api/activity?limit=300` | `application/json` | 由审计、trace 和 gateway 事件合并得到的统一活动时间线 |
| `GET /admin/api/instances` | `application/json` | 已连接的 DCC 实例 |
| `GET /admin/api/tools` | `application/json` | 已注册的 MCP 工具 |
| `GET /admin/api/workflows?limit=200` | `application/json` | 由 search telemetry、traces 和 audits 重建的 agent session/workflow 视图 |
| `GET /admin/api/tasks?limit=300` | `application/json` | 从 dispatch traces 重建出的任务视图 |
| `GET /admin/api/calls` | `application/json` | 最近的工具调用；可用时包含 compact/JSON token accounting（需要 `AuditMiddleware`） |
| `GET /admin/api/traces` | `application/json` | 最近的逐调用 dispatch traces，包含 payload size 与 token accounting；支持 `?limit=200` |
| `GET /admin/api/traces/{request_id}` | `application/json` | 某次调用的完整 waterfall trace，包含 token accounting 且不写入无界 payload |
| `GET /admin/api/traffic?limit=300` | `application/json` | 显式 `admin_live` traffic sink 保留的 live `traffic.frame` envelopes |
| `GET /admin/api/traffic/export?limit=1000` | `application/x-ndjson` | 将保留的 live traffic frames 导出为 JSONL，便于本地 replay/diff |
| `GET /admin/api/debug-bundle/{request_id}` | `application/json` | 单次请求的一站式 debug bundle，包含 trace、匹配审计行、相关活动和提示 |
| `GET /admin/api/stats?range=1h\|24h\|7d` | `application/json` | 聚合调用数、成功率、延迟、top tools/instances、payload token coverage 和 response token savings totals |
| `GET /admin/api/governance?limit=300` | `application/json` | 当前 gateway policy、traffic capture、redaction、中间件控制和最近 allow/deny/throttle 决策 |
| `GET /admin/api/workers` | `application/json` | 来自 live registry 的实例卡片；响应字段名保留 `workers` 以兼容现有客户端 |
| `GET /admin/api/logs` | `application/json` | 合并后的网关竞争事件、磁盘 `*.log` 行和审计调用摘要 |
| `GET /admin/api/health` | `application/json` | 服务健康摘要 |
| `GET /admin/api/skills` | `application/json` | 按 DCC 类型、skill 名、加载状态、工具、后端实例，以及来自 search telemetry 和审计调用的 skill 健康/采用指标聚合实时 skill 清单 |
| `GET /admin/api/skill-detail?name=...` | `application/json` | 单个 skill 的后端详情；可用时包含用于 review 的 `SKILL.md` Markdown 内容 |
| `GET /admin/api/skill-paths` | `application/json` | 当前 skill 发现根目录，返回安全路径别名、存在/缺失状态和 source/id 元数据，便于公开截图和导出 |
| `POST /admin/api/skill-paths` | `application/json` | 添加 SQLite 持久化的自定义 skill 发现根目录，并刷新 live backend skill index |
| `DELETE /admin/api/skill-paths/{id}` | `application/json` | 删除 SQLite 持久化的自定义 skill 发现根目录，并刷新 live backend skill index |

面向 agent 的稳定镜像位于 `/v1/debug/*`，并会出现在 `GET /v1/openapi.json`。
Admin 路由仍作为 dashboard 兼容层；自动化调用优先使用：

| 稳定路由 | 镜像 |
|----------|------|
| `GET /v1/debug/stats` | `/admin/api/stats` |
| `GET /v1/debug/governance?limit=300` | `/admin/api/governance` |
| `GET /v1/debug/traffic?limit=300` | `/admin/api/traffic` |
| `GET /v1/debug/traffic/export?limit=1000` | `/admin/api/traffic/export` |
| `GET /v1/debug/agent-traces/{lookup_id}` | 按 trace id 或 request id 获取 public-safe agent trace packet |
| `GET /v1/debug/search-telemetry` | `/admin/api/search-telemetry` |
| `GET /v1/debug/health` | `/admin/api/health` |

`/admin?panel=traces&trace=<request_id>` 这类浏览器 deep link 只用于 UI
导航。历史上的 `/admin?agent=traces&trace=<id>` 也按 UI 链接处理；agent
和自动化客户端应使用 `GET /v1/debug/agent-traces/{lookup_id}` 读取稳定的
机器可读包。

compact-aware debug routes 默认仍返回 JSON，方便浏览器下载和 GitHub issue
附件。agent 可以在 `/v1/debug/traces`、`/v1/debug/traces/{request_id}`、
`/v1/debug/trace-context/{lookup_id}`、`/v1/debug/bundles/{request_id_or_trace_id}`
和 `/v1/debug/stats` 上使用 `Accept: application/toon`、`?response_format=toon`
或 `?compact=true` 请求 TOON。响应会包含 `x-dcc-mcp-response-format`、byte
counts、estimated token counts 和 savings headers。debug bundle compact 输出是
summary，包含 root cause、tool、DCC type、status、timing、token accounting、
redaction summary 和指向完整 JSON bundle 的 links。

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

// GET /admin/api/workflows?limit=200
{
  "total": 1,
  "workflows": [
    {
      "workflow_id": "session-1",
      "group_kind": "session",
      "title": "Layout Inspector: maya.abcdef01.scene__inspect",
      "status": "completed",
      "discovery": {
        "search_count": 1,
        "zero_result_count": 0,
        "selected_count": 3,
        "best_selected_rank": 2,
        "time_to_first_success_ms": 310,
        "search_ids": ["search-123"]
      },
      "steps": [
        { "kind": "search", "title": "search scene inspect", "status": "ok" },
        { "kind": "describe", "title": "maya.abcdef01.scene__inspect", "status": "ok" },
        { "kind": "load_skill", "title": "load_skill scene", "status": "ok" },
        { "kind": "call", "request_id": "req-123", "title": "maya.abcdef01.scene__inspect", "status": "ok" }
      ]
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
      "token_accounting": {
        "response_format": "toon",
        "token_estimator": "dcc-mcp-byte4-v1",
        "original_tokens": 120,
        "returned_tokens": 54,
        "saved_tokens": 66,
        "savings_pct": 55.0
      },
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
      "output": { "mime_type": "application/json", "truncated": false, "original_size": 96, "content": "{...}" },
      "token_accounting": {
        "response_format": "json",
        "token_estimator": "dcc-mcp-byte4-v1",
        "original_tokens": 24,
        "returned_tokens": 24,
        "saved_tokens": 0,
        "savings_pct": 0.0
      }
    }
  ]
}

// GET /admin/api/traces/req-123 返回同一个完整 trace 对象；未命中时返回 404。

Admin token 字段刻意拆成两套账：`payload_token_usage`、trace
`input_tokens`/`output_tokens` 和 `payload_token_accounting` 是来自已捕获
request/response payload preview 的估算；缺失时用 `missing_payload_tokens`
显式表达。`token_usage`、`response_token_accounting` 以及每次调用的
`original_tokens`/`returned_tokens`/`saved_tokens` 描述 JSON/TOON 响应压缩
前后的 token accounting，不能替代缺失的 payload 估算。

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
  "latency": { "p50_ms": 12, "p95_ms": 48 },
  "payload_token_usage": {
    "token_estimator": "dcc-mcp-byte4-v1",
    "total_input_tokens": 1200,
    "total_output_tokens": 900,
    "total_tokens": 2100,
    "calls_with_any_payload_tokens": 21,
    "calls_missing_payload_tokens": 21,
    "avg_total_tokens_per_call": 50.0,
    "avg_total_tokens_per_recorded_call": 100.0
  },
  "token_usage": {
    "total_returned_tokens": 5400,
    "total_saved_tokens": 2100,
    "average_savings_pct": 28.0,
    "by_response_format": [
      { "name": "toon", "calls": 24, "returned_tokens": 3200, "saved_tokens": 2100, "savings_pct": 39.62 },
      { "name": "json", "calls": 18, "returned_tokens": 2200, "saved_tokens": 0, "savings_pct": 0.0 }
    ]
  }
}

// GET /admin/api/governance?limit=300
{
  "schema_version": "dcc-mcp.admin.governance.v1",
  "mode": {
    "admin_mutations": "disabled",
    "reason": "Admin is unauthenticated and read-only by default."
  },
  "policy": {
    "read_only": true,
    "unrestricted": false,
    "allowlists_active": { "dcc_types": true, "tool_slug_prefixes": true },
    "allowed_dcc_types": ["maya", "photoshop"],
    "allowed_tool_slug_prefixes": ["maya.a1b2"]
  },
  "traffic_capture": {
    "enabled": true,
    "mode": "aggregate",
    "production_guardrail": "capture only safe aggregate data unless explicitly configured",
    "redaction": { "paths": ["body.data.params.arguments.api_key"], "redacted_total": 8 }
  },
  "middleware": {
    "controls": [
      { "kind": "quota", "mode": "rate-limit", "summary": "100 calls / 60s" }
    ]
  },
  "stats": { "recent_allowed": 1200, "recent_policy_denied": 4, "recent_throttled": 3, "redacted_path_count": 8 },
  "recent_decisions": [
    {
      "request_id": "req-123",
      "outcome": "throttled",
      "tool": "maya.a1b2.scene__inspect",
      "traffic_capture": { "captured": 0, "skipped": 1, "reasons": ["filtered-by-rule"] },
      "privacy": { "redacted_paths": [] },
      "pressure": { "quota_active": true, "throttled": true }
    }
  ]
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
- **左侧导航**：Debug / Activity / Health / 实例 / 工具 / Tasks / Calls / Traces / Traffic / Stats / Skill paths / 集成 / 日志面板
- **自动刷新**：每个面板每 5 秒轮询对应 JSON 端点
- **DCC 图标**：Maya/Autodesk、Blender、GIMP、Inkscape、Krita、Unity、Unreal 等常见宿主显示可识别图标，自定义宿主使用安全 fallback
- **实例卡片**：按实例展示状态、心跳与路由元数据
- **Calls 表格**：展示 request id、错误摘要与 trace detail 链接；DCC 优先从解析后的 backend slug 展示，其次使用调用参数中的 `dcc` / `dcc_type`
- **Trace 下钻**：`/admin/api/traces/{request_id}` 暴露单次调用的完整 waterfall，以及有界/已脱敏的输入输出 payload
- **Traffic 面板**：当 traffic config 包含 `kind: admin_live` 时，`/admin/api/traffic` 暴露内存环形缓冲区中的 frame，`/admin/api/traffic/export` 可下载 JSONL
- **Governance 面板**：展示 read-only 状态、allowlists、traffic capture 模式/sink、生产 guardrail、redaction path 汇总、中间件限流控制，以及最近 allowed/denied/throttled/capture 决策
- **Logs 面板**：将标准化的 `contention`、`file`、`audit` 行分组，方便在一条时间线里关联路由事件、滚动日志和工具调用。文件日志读取会限制为最近文件与尾部片段，避免 admin API 扫描无界历史日志
- **集成面板**：第三方集成的只读配置摘要——Sentry DSN 状态、活跃 webhook 数量和名称、OTLP 端点，以及需要重启才能生效的配置变更 pending_restart 标记
- **可选持久化**：`DCC_MCP_GATEWAY_AUDIT_DIR` 可让 Calls 与 Traces 面板跨重启保留，且不改变 JSON API 结构
- **深色主题**：Vite/React 源码，运行时嵌入资产，不要求现场构建
- **响应式布局**：窄屏会切换为顶部导航，debug 卡片和图表保持可用的单列宽度

## 集成面板

集成面板（`GET /admin/api/integrations`）展示网关第三方集成配置的只读摘要。面板显示哪些集成处于活跃状态，以及环境变量配置变更是否需要重启服务器才能生效。

| 集成 | 配置方式 | 面板展示内容 |
|------|----------|--------------|
| Sentry | `DCC_MCP_SENTRY_DSN` 环境变量 | DSN 状态（已设置/未设置）、环境、采样率 |
| Webhooks | `DCC_MCP_WEBHOOKS_CONFIG` 环境变量 → YAML 文件 | 活跃 webhook 名称、事件模式、投递统计 |
| OTLP 追踪 | `OTEL_EXPORTER_OTLP_ENDPOINT` 环境变量 | 端点 URL、服务名称、span 采样率 |

面板为**只读**——所有三种集成均通过环境变量或配置文件在服务器启动时配置。当操作员通过部署工具修改了环境变量但尚未重启网关进程时，面板会标记 `pending_restart`。详见 [gateway.md](gateway.md) 的完整配置参考。

### 后端 API

```json
// GET /admin/api/integrations
{
  "sentry": {
    "configured": true,
    "dsn_prefix": "https://***@o***.ingest.sentry.io",
    "environment": "production",
    "sample_rate": 1.0,
    "pending_restart": false
  },
  "webhooks": {
    "configured": true,
    "config_path": "/etc/dcc-mcp/webhooks.yaml",
    "active_webhooks": 2,
    "names": ["analytics-forwarder", "error-reporter"],
    "pending_restart": false
  },
  "otlp": {
    "configured": true,
    "endpoint": "http://localhost:4317",
    "service_name": "dcc-mcp-gateway",
    "pending_restart": false
  }
}
```

响应中会省略密钥：DSN 前缀仅用于标识，完整的 DSN（包含 secret key）不会暴露。当集成未配置时，其区块包含 `"configured": false`。

如果检测到配置变更（例如 `DCC_MCP_SENTRY_DSN` 自进程启动后被设置或清除），`pending_restart` 为 `true`，面板会显示视觉指示器提示操作员重启网关。

### 稳定版面向 agent 的路由

| 稳定路由 | 镜像来源 |
|----------|----------|
| `GET /v1/debug/integrations` | `/admin/api/integrations` |

## 安全注意事项

Admin UI 是**只读**的，默认**无认证**。它绑定在赢得选举的 gateway 所使用的 host 上，而默认 host 是 `127.0.0.1`。生产环境建议：
- 保持 localhost 绑定，或通过反向代理添加 IP 白名单 / Basic Auth
- 不需要时禁用：`--no-admin`、`DCC_MCP_NO_ADMIN=true` 或 `cfg.admin_enabled = False`
- 将 Governance 面板视为只读检查面；policy、capture、redaction、quota 的修改仍应通过经过认证的部署配置完成
- 切勿直接暴露到公网

## 参见

- [middleware.md](middleware.md) — 填充 `/admin/api/calls` 的 `AuditMiddleware`
- [observability.md](observability.md) — 填充 `/admin/api/logs` 的 `EventLog`
- [gateway.md](gateway.md) — 完整的网关配置参考（webhooks、Sentry、OTLP）
- [sentry.md](sentry.md) — Sentry 错误监控参考
