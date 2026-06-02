# REST API 面板

per-DCC adapter 服务和多 DCC 网关都在 MCP 端点旁边暴露 `/v1/*` REST API，
但两者的 surface 是重叠而非完全相同。这个页面是面向**传统调用方**
（cURL、CI 流水线、片场自动化、非-MCP 工具）的接入契约 —— 任何能讲
HTTP 的客户端都能用这些路由驱动 DCC，不需要碰 MCP 协议栈。

Gateway 现在发布 gateway 专属的 `GET /v1/openapi.json` 契约。它只记录
gateway router 真正挂载的路由，并刻意不广告 per-DCC-only 的 resource、
prompt、job 路由。per-DCC adapter 服务继续在同一路径发布自己的 adapter
OpenAPI 契约。

> **和 MCP 的关系** —— 网关在 MCP `tools/list` 里广告同一条有界工作流：
> `search`、`describe`、`load_skill`、`call`。这些工具与 `/v1/search`、
> `/v1/describe`、`/v1/load_skill`、`/v1/call`、`/v1/call_batch` 复用
> service 代码；REST 是不使用 MCP 协议栈的纯 HTTP twin。
> MCP 客户端也可以通过 `params._meta.response_format="toon"` 或
> `params._meta.compact=true` 显式请求 compact TOON payload。`/mcp` 的
> HTTP content type 和外层 JSON-RPC `jsonrpc`、`id`、`result`、`error`
> envelope 仍保持 JSON；`Accept: application/toon` 只用于 REST 协商。

---

## Gateway 端点

| 方法 | 路径 | 用途 |
|---|---|---|
| `GET` | `/v1/instances` | elected gateway 当前知道的在线 DCC instance rows。 |
| `GET` | `/v1/healthz` | Gateway 存活探针。HTTP handler 在线时返回 `200 {"ok": true}`。 |
| `GET` | `/v1/readyz` | Gateway readiness 汇总，包含每个 instance 的 readiness bits；即使没有 ready instance，该路由仍返回 `200`。 |
| `GET` | `/v1/skills` | 把已加载 gateway capability records 投影成 skill entries。 |
| `POST` | `/v1/list_skills` | 把 skill-list 请求转发给选中的 backend instance。 |
| `POST` | `/v1/search` | 模糊 / 精确搜索 loaded + unloaded skills。 |
| `POST` | `/v1/load_skill` | 不经过 MCP `tools/call`，加载一个 backend skill；gateway 默认 lazy group activation。 |
| `POST` | `/v1/unload_skill` | 不经过 MCP `tools/call`，卸载一个 backend skill。 |
| `POST` | `/v1/describe` | 按 `tool_slug` 返回完整 input schema + 注解。 |
| `GET` | `/v1/tools/{slug}` | `/v1/describe` 的别名（只读 URL 查询）。 |
| `POST` | `/v1/call` | 按 slug 调用一个 gateway capability。这是 gateway 的规范调用面。 |
| `POST` | `/v1/call_batch` | 按顺序调用最多 25 个 gateway capability，可选 `stop_on_error`。 |
| `GET` | `/v1/context` | Gateway snapshot，包含在线 instances 和 aggregate capability counts。 |
| `GET` | `/v1/dcc/{dcc_type}/instances/{instance_id}/describe` | 描述某个 DCC instance 上的 backend tool；query 支持 `backend_tool` / `tool` / `action`。 |
| `POST` | `/v1/dcc/{dcc_type}/instances/{instance_id}/call` | 使用 `{backend_tool, arguments, meta}` 调用某个 DCC instance 上的 backend tool。 |
| `POST` | `/v1/dcc/{dcc_type}/instances/{instance_id}/stop` | 仅用于广告了 `safe_stop_url` metadata 的 test-owned instance safe stop。 |
| `GET` | `/v1/debug/instances` | 仅网关：稳定的 agent-facing instance diagnostics。 |
| `GET` | `/v1/debug/activity` | 仅网关：来自 audit、trace、gateway event 的稳定 activity feed。 |
| `GET` | `/v1/debug/traces` | 仅网关：最近的 dispatch trace 列表。 |
| `GET` | `/v1/debug/traces/{request_id}` | 仅网关：按 request id 查看 dispatch trace 详情。 |
| `GET` | `/v1/debug/traffic` | 仅网关：显式 `admin_live` sink 保留的 live traffic-capture frames。 |
| `GET` | `/v1/debug/traffic/export` | 仅网关：把保留的 live traffic-capture frames 导出为 JSONL。 |
| `GET` | `/v1/debug/trace-context/{lookup_id}` | 仅网关：按 trace id 或 request id 解析 primary trace context。 |
| `GET` | `/v1/debug/agent-traces/{lookup_id}` | 仅网关：按 trace id 或 request id 获取 compact public-safe agent trace packet。 |
| `GET` | `/v1/debug/bundles/{request_id}` | 仅网关：按 request id 或 trace id 取 full-chain debug bundle。 |
| `GET` | `/v1/debug/issue-reports/{request_id}` | 仅网关：默认 public-safe、可附到 GitHub issue 的 debug report JSON；`?mode=raw` 返回已审阅本地证据用的完整 bundle。 |
| `GET` | `/v1/debug/workflows` | 仅网关：由 retained search telemetry、traces 和 audits 重建的 agent session/workflow 投影视图。 |
| `GET` | `/v1/debug/tasks` | 仅网关：从 traces 重建的 task-like snapshot。 |
| `GET` | `/v1/debug/calls` | 仅网关：最近 audit call rows。 |
| `GET` | `/v1/debug/logs` | 仅网关：合并 gateway events、file logs、audit summaries。 |
| `GET` | `/v1/debug/stats` | 仅网关：聚合 call statistics。 |
| `GET` | `/v1/debug/governance` | 仅网关：当前 policy、traffic capture、redaction、quota 和最近 governance 决策。 |
| `GET` | `/v1/debug/health` | 仅网关：debug subsystem health summary。 |
| `GET` | `/v1/openapi.json` | Gateway 专属 OpenAPI 3.x 文档，可供代码生成。 |
| `GET` | `/docs` | 用同一份 gateway 专属 OpenAPI 文档渲染的 Scalar API reference。 |

Gateway capability slug 使用 `<dcc>.<id8>.<tool>`，从 `POST /v1/search`
获取。instance-scoped describe/call 路由适合已经知道目标 `dcc_type`、
`instance_id` 和 backend tool id 的调用方。

## Per-DCC Adapter 端点

per-DCC adapter 服务只代表一个 host 进程。它们的 OpenAPI 文档可以包含
gateway 不挂载的路由：

| 方法 | 路径 | 用途 |
|---|---|---|
| `GET` | `/v1/healthz` | Adapter 存活探针。 |
| `GET` | `/v1/readyz` | Adapter readiness；DCC 启动时可能是 `Ready`、`Booting` 或 unreachable。 |
| `GET` | `/v1/skills` | 该 adapter 已发现的 skills。 |
| `POST` | `/v1/search` | 搜索该 adapter catalog。 |
| `POST` | `/v1/load_skill` | 加载一个 adapter skill。 |
| `POST` | `/v1/unload_skill` | 卸载一个 adapter skill。 |
| `POST` | `/v1/describe` | 描述 adapter-local tool slug。 |
| `GET` | `/v1/tools/{slug}` | adapter `/v1/describe` 的 URL 别名。 |
| `POST` | `/v1/call` | 调用 adapter-local tool slug。 |
| `POST` | `/v1/dcc/{dcc_type}/call` | Adapter-local backend call helper；gateway 不挂载。 |
| `GET` | `/v1/context` | Host scene/document snapshot。 |
| `GET` | `/v1/resources` | 该 adapter 的 MCP 风格 resource 清单。 |
| `GET` | `/v1/resources/{uri}` | 读取一个 percent-encoded adapter resource URI。 |
| `GET` | `/v1/resources/{uri}/events` | resource 更新 SSE stream。 |
| `GET` | `/v1/prompts` | MCP 风格 prompt template 清单。 |
| `GET` | `/v1/prompts/{name}` | 渲染一个 prompt；JSON object 参数放在 `?args=...`。 |
| `GET` | `/v1/jobs/{id}/events` | async job SSE stream。 |
| `DELETE` | `/v1/jobs/{id}` | 取消一个 async job。 |
| `GET` | `/v1/openapi.json` | per-DCC adapter OpenAPI 文档。 |
| `GET` | `/docs` | 用 adapter OpenAPI 文档渲染的 Scalar API reference。 |

---

## Gateway Agent Debug API

被选举出来的 gateway 会把 Admin telemetry providers 提升为稳定的
`/v1/debug/*` agent/CI 诊断路由。这些路由会出现在 `GET /v1/openapi.json`
里；`/admin/api/*` 继续作为内嵌 dashboard 的兼容别名。

这个 surface 依赖 gateway 的 `admin` feature 和运行时 Admin telemetry。
发布的 `dcc-mcp-server` 和 Python `dcc-mcp-http` gateway 路径默认启用它；
如果直接以 minimal `dcc-mcp-gateway` 构建且不启用 `admin`，或运行时关闭
Admin（`--no-admin` / `admin_enabled = false`），则不会挂载 `/v1/debug/*`
路由，也不会在 OpenAPI 中列出这些路径。

Phase-1 debug routes 会保留现有 Admin payload 字段，让 operator 和 agent
能一对一对照结果：

| 稳定路由 | 兼容路由 | 说明 |
|---|---|---|
| `/v1/debug/instances` | `/admin/api/instances` | 支持 `view=live\|all`、`include_stale`、`include_dead`。 |
| `/v1/debug/activity?limit=200` | `/admin/api/activity?limit=200` | 统一 activity feed。 |
| `/v1/debug/traces?limit=200` | `/admin/api/traces?limit=200` | 最近 dispatch trace rows。 |
| `/v1/debug/traces/{request_id}` | `/admin/api/traces/{request_id}` | 精确 request-id trace detail。 |
| `/v1/debug/traffic?limit=300` | `/admin/api/traffic?limit=300` | 来自 `admin_live` sink 的 live traffic-capture frames。 |
| `/v1/debug/traffic/export?limit=1000` | `/admin/api/traffic/export?limit=1000` | 保留的 live traffic-capture frames JSONL。 |
| `/v1/debug/trace-context/{lookup_id}` | n/a | 按 `trace_id` 或 `request_id` 做 trace-context lookup。 |
| `/v1/debug/agent-traces/{lookup_id}` | n/a | 单条 retained trace 的 public-safe agent packet；接受 `trace_id` 或 `request_id`。 |
| `/v1/debug/bundles/{request_id}` | `/admin/api/debug-bundle/{request_id}` | 接受 request ids 和 retained trace ids。 |
| `/v1/debug/issue-reports/{request_id}` | `/admin/api/issue-report/{request_id}` | 默认适合附到 GitHub issue 的 public-safe JSON export；`?mode=raw` 返回完整 debug bundle 供本地审阅。 |
| `/v1/debug/workflows` | `/admin/api/workflows` | 从 retained search telemetry、traces 和 audits 聚合 agent session/workflow。 |
| `/v1/debug/tasks` | `/admin/api/tasks` | retained traces 的 task projection。 |
| `/v1/debug/calls` | `/admin/api/calls` | 最近 audit rows。 |
| `/v1/debug/logs` | `/admin/api/logs` | 合并 gateway/file/audit logs。 |
| `/v1/debug/stats` | `/admin/api/stats` | 聚合 call stats。 |
| `/v1/debug/governance?limit=300` | `/admin/api/governance?limit=300` | 当前 policy、read-only 状态、traffic capture/redaction 控制、中间件压力和最近 allow/deny/throttle 决策。 |
| `/v1/debug/health` | `/admin/api/health` | debug provider health summary。 |

compact-aware debug routes（`/v1/debug/traces`、
`/v1/debug/traces/{request_id}`、`/v1/debug/trace-context/{lookup_id}`、
`/v1/debug/bundles/{request_id}`、`/v1/debug/stats`）默认仍返回 JSON，保证
浏览器和 GitHub issue 附件兼容。agent 可以用 `Accept: application/toon`、
`?response_format=toon` 或 `?compact=true` 请求 TOON。响应会带
`x-dcc-mcp-*` byte/token accounting headers。debug bundle 的 compact 输出是
public-safe summary，包含 root cause、tool、DCC type、status、timing、token
accounting、redaction summary、postmortem counts、hints 和指向完整 JSON 的 links。

所有 list endpoint 在底层 Admin provider 已支持时都支持 `limit` 参数。
OpenAPI contract 预留了 `cursor`、`since`、`until` 给后续 normalized envelope
工作；在该阶段落地前，调用方应忽略缺失的 `next_cursor`。

常见关联字段包括 `request_id`、`trace_id`、`instance_id`、`dcc_type`、
`tool` / `tool_slug`、`transport`、`agent_id`、`agent_name`、`agent_model`、
`parent_request_id`，以及底层 provider 能提供的 timestamp。精确请求详情用
`request_id`；跨请求 debug bundle 或 `/v1/debug/trace-context/{trace_id}`
用 `trace_id`。

机器可读的 agent hand-off 包使用 `/v1/debug/agent-traces/{lookup_id}`。
该路由同时支持 trace id 和 request id，并省略 request/response payload
preview、prompt、script 和 scene data。`/admin?panel=traces&trace=<request_id>`
以及历史 `/admin?agent=traces&trace=<id>` 是 UI 导航链接，不是稳定 API。

---

## `POST /v1/call` —— 调用契约

### 请求体

```json
{
  "tool_slug": "maya.a1b2c3d4.create_sphere",
  "arguments": { "radius": 2.0, "segments": 32 },
  "meta": { "progressToken": "session-42" }
}
```

| 字段 | 必需 | 说明 |
|---|---|---|
| `tool_slug` | ✅ | 网关：`<dcc>.<id8>.<tool>`。直接 per-DCC REST：`<dcc>.<skill>.<action>`。从 `POST /v1/search` 或 `GET /v1/skills` 里拿有效 slug，**不要**手写。 |
| `arguments` | ❌ | 规范工具输入，和 MCP `tools/call` 一致。缺失 / `null` / 空字符串会归一化成 `{}`；JSON object 原样使用；能解析成 object 的 JSON string 会为了 wrapper 兼容被接受；数组、布尔、数字和非 object 字符串会被拒绝。 |
| `params` | ❌ | `arguments` 的向后兼容别名。新客户端优先使用 `arguments`，这样 REST 和 MCP 示例保持一致。 |
| `meta` | ❌ | MCP 风格的元信息侧车。缺失 / `null` 会归一化为 absent；提供时必须是 object（或 object-shaped JSON string）。认得这几个键：`progressToken`、`dcc.async`、`dcc.wait_for_terminal`。 |

规范归一化规则在 `dcc-mcp-wire`；Python host wrapper 可以复用
`dcc_mcp_core.host.normalize_tool_arguments()` 和 `normalize_tool_meta()`，
不要自己手写 JSON coercion。

### 成功响应 — `200 OK`

```json
{
  "slug": "maya.a1b2c3d4.create_sphere",
  "output": { "sphere_id": "pSphere1" },
  "validation_skipped": false,
  "request_id": "req-7f3c..."
}
```

`slug` 字段原样回显调用方用过的 slug，便于批量 dispatch 时做关联。

### 错误响应 —— 结构化、kebab-case

```json
{
  "kind": "unknown-slug",
  "message": "no action registered for slug 'maya.a1b2c3d4.make_sphere'",
  "hint": "call /v1/search to list available tools",
  "request_id": "req-7f3c...",
  "candidates": ["maya.a1b2c3d4.create_sphere"]
}
```

错误 kind 词汇表（括号里是 HTTP 状态码）：

- `unknown-slug` (404) —— 找不到匹配的 action；`candidates` 可能带建议 slug。
- `ambiguous` (409) —— slug 匹配多个 action；`candidates` 列出全部。
- `skill-not-loaded` (409) —— slug 有效但拥有者 skill 未加载。先调 `load_skill`。
- `invalid-params` (400) —— JSON-Schema 校验 `arguments` / `params` 失败。
- `unauthorized` (401) —— `AuthGate` 拒绝。per-DCC 默认仅本机；远程访问需装 `BearerTokenGate`。
- `not-ready` (503) —— `/v1/readyz` 红灯；DCC 还在启动中。
- `host-busy` (503) —— DCC 宿主仍在线，但分发队列已饱和；请退避重试或路由到另一个在线实例。
- `affinity-violation` (409) —— 从 worker 线程调用了主线程独占的工具。
- `bad-request` (400) —— envelope 有误（缺 `tool_slug`、JSON 坏、等）。
- `backend-error` (502) —— 拥有者 DCC 进程响应了但工具失败。
- `skill-not-found` (502) —— **仅网关 skill lifecycle** —— 请求的 skill 不存在。
- `skill-already-loaded` (502) —— **仅网关 skill lifecycle** —— skill 已处于加载状态。
- `group-not-found` (502) —— **仅网关 skill lifecycle** —— 请求的 progressive group 不存在。
- `ambiguous-instance` (502) —— **仅网关 skill lifecycle** —— DCC / instance 选择不唯一。
- `throttled` (429) —— gateway 中间件限流或并发控制在路由到 backend 前拒绝了请求；按 backoff 重试。
- `policy-denied` (403) —— **仅网关** —— gateway policy 在路由到 backend 前拒绝了该操作。查看 `policy.reason`。
- `instance-offline` (503) —— **仅网关** —— `<id8>` 对应的实例已不在线。
- `schema-unavailable` (502) —— **仅网关** —— 拥有者 DCC 在 discovery 和 call 之间失联。
- `internal` (500) —— REST 层自身失败；查服务端日志。

### Gateway Policy

Gateway 可以在客户端看到工具之前、或 backend 调用被路由之前收窄动态能力面。
Rust 侧通过 `McpHttpConfig::with_gateway_policy(...)` /
`with_gateway_read_only(...)` 配置 `GatewayPolicy`；Python 侧通过
`McpHttpConfig.gateway_read_only`、`allowed_dcc_types`、
`allowed_skill_names`、`allowed_skill_families`、`allowed_tool_slugs` 和
`allowed_tool_slug_prefixes` 配置。

Policy 规则属于 gateway 对外契约：

- 空 allowlist 表示不限制。非空 allowlist 都是大小写不敏感匹配，可限制
  DCC type、精确 skill name、skill family 前缀、精确 canonical gateway
  `tool_slug` 或 `tool_slug` 前缀。
- `search` 会隐藏被 policy 拒绝的 capability，所以客户端应把搜索结果视为
  当前部署允许暴露的能力面，而不是完整 backend inventory。
- `describe` 在 read-only 开启时仍可用于允许的 capability，方便 agent 先
  读 schema 再决定是否安全调用。
- `read_only = true` 会拒绝 `load_skill`、`unload_skill`、tool-group 修改，
  以及未声明 `annotations.readOnlyHint = true` 的 backend 调用。
- REST 单次拒绝返回 HTTP 403，`kind: "policy-denied"`，并带结构化
  `policy` 对象。Batch 调用保持 HTTP 200，在对应 result item 上返回同样的
  `policy-denied` envelope，以保持 batch 顺序稳定。
- `GET /v1/debug/governance` 会暴露当前 read-only 状态、allowlists、
  capture/redaction 控制、中间件 quota 压力和最近 allow/deny/throttle 决策，
  让 agent 不必通过试错调用被阻止的工具来判断部署边界。

拒绝示例：

```json
{
  "kind": "policy-denied",
  "message": "gateway policy denied call for tool slug 'maya.a1b2c3d4.create_sphere'",
  "request_id": "req-7f3c...",
  "policy": {
    "reason": "read-only",
    "operation": "call",
    "read_only": true,
    "dcc_type": "maya",
    "skill_name": "maya-modeling",
    "tool_slug": "maya.a1b2c3d4.create_sphere"
  }
}
```

### Request ID

每个请求都有 `request_id`（客户端 `X-Request-Id` 头优先，否则服务端生成）。
它会流进审计日志、响应 envelope，以及网关 MCP 响应的 `_meta.request_id`，
让 MCP 和 REST 调用方可以追踪同一工作单元。

参与 discovery、skill loading、describe、单次 call 和 batch execution 的
Gateway REST 响应也会带稳定观测头：

| Header | 含义 |
|---|---|
| `x-dcc-mcp-request-id` | Gateway request id；传入 `X-Request-Id` 时复用该值。 |
| `x-dcc-mcp-trace-id` | 跨 gateway、sidecar、host 的 end-to-end trace id。 |
| `traceparent` | W3C trace context，方便 HTTP 客户端继续传播 trace。 |
| `x-dcc-mcp-index-generation` | 触及 discovery/call 状态时返回的 capability-index opaque fingerprint。 |
| `x-dcc-mcp-search-id` | 创建或消费 search result set 时返回的 search-quality 关联 id。 |
| `x-dcc-mcp-ranker-version` | 关联 search result set 使用的 bounded ranker id。 |

`/v1/search`、`/v1/describe`、`/v1/load_skill` 和 `/v1/call_batch` 的
JSON/TOON body 也会包含 `request_id`、`trace_id`、`index_generation`。
`/v1/call` 为保持 backend result envelope 向后兼容，只通过 headers 暴露
这些 metadata。
Search response 还会包含 `search_id`、`ranker_version` 和
`index_generation`；后续 `/v1/describe`、`/v1/load_skill`、`/v1/call` 或
`/v1/call_batch` 应把 `next_step` 中的 `meta.search_id` 原样传回，
这样 gateway 可以统计 search-to-action 质量，同时不记录完整 prompt。

### Caller Attribution

REST 调用方可以在 `meta.agent_context`、顶层 `agent_context` /
`caller_context`，或 `x-dcc-mcp-*` headers 中提供有界归因元数据。MCP
调用方使用同样的 shape，放在 `params._meta.agent_context`。这些字段只用于
遥测和 Admin 调试；不要发送隐藏推理、完整 prompt、原始用户消息、secret、
bearer token 或原始 agent 回复。

| 概念 | JSON 字段 | Header 字段 |
|---|---|---|
| Actor | `actor_id`、`actor_name`、`actor_email_hash` | `x-dcc-mcp-actor-id`、`x-dcc-mcp-actor-name`、`x-dcc-mcp-actor-email-hash` |
| Agent runtime | `agent_id`、`agent_name`、`agent_kind`、`agent_version`、`model`、`model_provider`、`model_version` | `x-dcc-mcp-agent-id`、`x-dcc-mcp-agent-name`、`x-dcc-mcp-agent-kind`、`x-dcc-mcp-agent-version`、`x-dcc-mcp-agent-model`、`x-dcc-mcp-agent-model-provider`、`x-dcc-mcp-agent-model-version` |
| Client platform | `client_platform`、`client_os`、`client_host` | `x-dcc-mcp-client-platform`、`x-dcc-mcp-client-os`、`x-dcc-mcp-client-host` |
| Auth subject | `auth_subject` | `x-dcc-mcp-auth-subject` |
| Network source | `source_ip`、`forwarded_for` | 仅服务端派生 |

`source_ip` 和 `forwarded_for` 必须在 transport 边界按 proxy trust policy
派生；REST body、MCP `_meta` 或 caller headers 中提供的同名字段会被忽略。

---

## `POST /v1/search` —— compact discovery

`/v1/search`、`/v1/describe`、`/v1/tools/{slug}`、直接 per-instance
describe/call 路由、`/v1/call` 和 `/v1/call_batch` 默认返回 compact
TOON。需要 JSON-first 兼容窗口的部署可以设置
`DCC_MCP_GATEWAY_RESPONSE_FORMAT=json`（旧别名：
`DCC_MCP_RESPONSE_FORMAT=json`）。REST 客户端也可以按请求显式回退 JSON：

Gateway policy 会在返回最终搜索结果前过滤 capability。一个缺失的 hit 可能
代表能力不存在、skill 未加载，或该能力被 DCC / skill / tool allowlist 有意隐藏。
Gateway 默认 `mode: "fuzzy"` 使用 hybrid ranker：先对 tool name、skill、
tag、summary、作者声明的 alias 和 schema-field token 做加权 lexical 匹配，再用
nucleo-matcher fuzzy fallback 保留 typo / partial-name 容错；`mode: "exact"`
仍是旧 substring 表。Gateway hit 会带 `score` 和 bounded `match_reasons`
（例如 `tool_lexical`、`alias_lexical`、`schema_lexical`、`summary_fuzzy`、`schema_fuzzy`、
`multi_token_lexical`），让 agent 和维护者不取完整 schema 也能理解排序原因。
Gateway hit 还带 1-based `rank`。生成的 `next_step` 会携带
`meta.search_id`、`meta.ranker_version` 和 `meta.index_generation`，REST
调用方把它放进 body，MCP 调用方也可以把同一对象作为 `_meta` 传给
后续 `describe` / `load_skill` / `call`。
完整 `input_schema` 仍只通过 `describe` 返回；搜索阶段只允许 per-DCC backend
通过 bounded `metadata.dcc.searchAliases` / `metadata.dcc.searchTokens` 把小型
索引提示交给 gateway，gateway 搜索响应不会把这些内部 token 暴露成公开字段。

```bash
curl -H 'Accept: application/json' \
  -d '{"query":"render","limit":20}' \
  http://127.0.0.1:9765/v1/search
```

请求体可以传 `"response_format": "json"` 强制旧 JSON；也可以传
`"response_format": "toon"` 或 `"compact": true`，即使 `Accept` 头偏好
JSON 也强制 compact 输出。若 `Accept` 和请求体都没有指定，REST 返回
`application/toon`。

每个支持 compact 的 REST 响应都会带近似 token 统计头：

| Header | 含义 |
|---|---|
| `x-dcc-mcp-response-format` | `json` 或 `toon`。 |
| `x-dcc-mcp-token-estimator` | 估算器 id；当前是 `dcc-mcp-byte4-v1`。 |
| `x-dcc-mcp-original-bytes` / `x-dcc-mcp-returned-bytes` | 旧 JSON 序列化字节数 vs 实际返回字节数。 |
| `x-dcc-mcp-original-tokens` / `x-dcc-mcp-returned-tokens` | 近似 bytes/4 token 估算，用于上下文预算，不代表计费。 |
| `x-dcc-mcp-saved-tokens` / `x-dcc-mcp-savings-pct` | 相比旧 JSON 的估算节省。 |

同一份统计也会写入 gateway retained traces、audit call rows，以及
`/v1/debug/stats` / `/admin/api/stats` 聚合。旧 JSON 响应会明确记录为
`response_format: "json"` 且 token savings 为 0，便于客户端在不拉取完整
trace payload 的情况下对比 compact 与兼容流量。

compact search 仍保留 agent 后续工作需要的字段：`tool_slug`、
`backend_tool`、`dcc_type`、`instance_id`、`loaded`、`load_state`、
`available_groups`、`has_schema`、`score`、`match_reasons`、`rank`、
`search_id`、`ranker_version`、`index_generation`，以及 unloaded skill 的
`next_step`。`next_step` 同时带 MCP (`tool` + `arguments`) 和 REST
(`method` + `path` + `body`) 形态；Gateway REST 调用方可以直接 POST
`next_step.arguments` 到 `/v1/load_skill`。它会省略冗余默认值，例如与
`backend_tool` 相同的 `callable_id`、空数组和空 object。这里把 RTK 的
语义压缩模型作为设计参考；gateway 内部直接使用确定性的 `toon-format`
库，让 `serde_json::Value` payload 在 Rust 测试中可 round-trip，不需要
派生外部 codec 进程。Gateway `load_skill` 默认惰性激活 group
（未传入时等价于 `activate_groups=false`）：默认/core group 可自动可用，
更重的 group 需要显式 `tool_group` 激活。

compact describe 会对 `record` 应用相同的小记录规则，但完整保留 `tool`
定义，包括 `inputSchema`、annotations 和 validation hints。compact call
保留与 JSON 相同的成功 / 失败 envelope 和 HTTP 状态，只是用 TOON 编码。
compact batch 保留结果顺序；每个 result 会带 `token_accounting`，响应头
则给出整个响应体的聚合节省。旧 JSON batch 响应保持相同 result 形状，并在
`x-dcc-mcp-*` 统计头中记录 0 savings。
`/v1/call_batch.calls[]` 可选 `id`（string/number/boolean），对应 result 会
原样 echo 该值，同时保留稳定的数字 `index`。

Gateway MCP 端点复用同一个 compact codec，但不会改变 JSON-RPC framing。
未携带 response-format metadata 的旧 MCP 客户端仍得到普通 JSON result。
compact-capable MCP 客户端在 `initialize` 广告
`capabilities.experimental["dcc-mcp"].compactResponses` 之后，应在
`tools/list`、`resources/read`、`prompts/get` 或 `tools/call` 请求上设置
`params._meta.response_format="toon"`（或 `params._meta.compact=true`）；
单次请求可用 `params._meta.response_format="json"` 回退。
非 `tools/call` 的 result 会变成包含 `response_format`、`mimeType`、
`text` 和 `_meta.token_accounting` 的 JSON object。`tools/call` 仍保持 MCP
`CallToolResult` 形状：`content[]`、`type`、`isError` 不变，只是在 text
content 上增加 `mimeType: "application/toon"` 并把文本换成 TOON。JSON-RPC
错误仍是普通 `error` object，旧客户端的错误处理不会被 compact mode 改写。

---

## 就绪状态（`GET /v1/readyz`）

在 gateway 上，`GET /v1/readyz` 始终返回 `200`，并汇总当前 registry
视图。除了 `live_instance_count`、`ready_instance_count` 和
`not_ready_instance_count`，它还会返回 `dispatch_reported_instance_count`、
`dispatch_ready_instance_count` 与 `dispatch_not_ready_instance_count`；每个
instance row 也包含与 `GET /v1/instances` 相同的嵌套 `dispatch` 对象。
sidecar 型 adapter 应使用这些 dispatch 计数区分“DCC 进程已列出”和
“sidecar dispatcher 真的可调用”。

每个 DCC 自己的 `/v1/readyz` endpoint 使用下列状态：

| 状态 | HTTP | Body | 含义 |
|---|---|---|---|
| `Ready` | 200 | `{"process": true, "dcc": true, "skill_catalog": true, "dispatcher": true, "host_execution_bridge": true, "main_thread_executor": true}` | 基础路由就绪位已绿；`POST /v1/call` 会派发。 |
| `Booting` | 503 | `{"status": "booting", ...哪些位红}` | 服务在线但 DCC 主机 / 派发器还没完成初始化。网关保留该实例的注册表行但不路由流量。 |
| `Unreachable` | 无响应 | — | 5 秒内没应答。网关会回退试 `GET /health`（#660 前的后端兼容）；还没响应 → 算 probe 失败，连续 3 次后从注册表中剔除。 |

区分很重要：agent 看到"我的工具不见了"时，诊断路径和修复手段完全不同。
监控面板上把 `process` / `dcc` / `skill_catalog` / `dispatcher` /
`host_execution_bridge` / `main_thread_executor` 分列展示。

---

## 与 MCP 的 envelope 对齐

| 关注点 | MCP `call`（JSON-RPC） | REST `POST /v1/call` | 一致？ |
|---|---|---|---|
| 成功体 | `result.content[].text` (JSON 字符串) | `{slug, output, validation_skipped, request_id}` | ✅ 同一 `CallOutcome` |
| 失败体 | `result.content[].text` + `isError: true` | `{kind, message, hint?, request_id, candidates?}` | ✅ 同一 `ServiceError`；MCP 再包一层 `CallToolResult` |
| `request_id` | `_meta.request_id` | 顶层字段 | ✅ 相同值 |
| 取消 | `notifications/cancelled` | 关 HTTP 连接 | ✅ 都触发合作式取消 |
| 进度事件 | `notifications/progress` 通过 `_meta.progressToken` 绑定 | 长轮询 SSE（路线图 #604） | ⚠ 目前仅 MCP 支持 |

契约由 `crates/dcc-mcp-skill-rest/src/openapi.rs` 里的 OpenAPI snapshot
测试锁定（`call_request_schema_contract_is_stable` 与
`call_outcome_schema_contract_is_stable`）。任何对这两个测试的修改都标志着
下游可见的 envelope 破坏。

---

## 什么时候选 REST、什么时候选 MCP

| 你是 … | 选 |
|---|---|
| 写 **AI Agent**（Claude、Cursor、ChatGPT 桌面版、自研） | 可用 MCP 时优先 **MCP**：使用网关 `search` / `describe` / `load_skill` / `call`。没有 MCP 协议栈或需要 OpenAPI 生成 HTTP binding 时使用 REST。 |
| 写 **cURL 脚本** / cron / CI 流水线 | **REST**。纯 HTTP + JSON，不需要 MCP 库。 |
| 写对接多个 DCC 的**企业后端** | **网关上的 REST**。单一端点、跨 DCC 一致 envelope、OpenAPI 文档供代码生成。 |
| 写**宿主内插件**（Maya 插件、Blender add-on） | 两者都不是 —— 直接调 `DccServerBase.register_*`。REST / MCP 是给外部调用方的。 |
| 调试"我的工具为啥没跑？" | **优先 REST**：`curl /v1/healthz` → `/v1/readyz` → `/v1/search`。三个端点给你一条从"进程活着吗"到"我的工具能被发现吗"的直线。 |

---

## 可插拔 trait（给嵌入方）

REST 面板由 5 个 trait 组成，每一个都可替换。默认开箱即用（仅本机开发），
生产部署只替换自己关心的那个。

| Trait | 默认实现 | 常见替换 |
|---|---|---|
| `SkillCatalogSource` | 真实 `SkillCatalog` | 测试夹具；对远程 registry 的只读缓存。 |
| `ToolInvoker` | 基于 `ToolDispatcher` 的 `DispatcherInvoker` | 排队投递到 DCC 主线程的队列型 invoker。 |
| `AuthGate` | `AllowLocalhostGate` | 远程访问用 `BearerTokenGate::new(vec![token])`；企业用 SSO gate。 |
| `AuditSink` | `NoopSink` | 追加 JSONL 的 `FileAuditSink`；中央审计的 Kafka producer。 |
| `ReadinessProbe` | 静态 `Ready` | 与 DCC 主机的真实就绪信号挂钩，场景加载中变红。 |

这就是 REST 面板的 DIP 实现：handler 依赖 trait 而非具体类型，插入自定义
auth / audit / invocation 永远不用碰 `handle_call`。

---

## 相关阅读

- [网关争用与调试](gateway-diagnostics.md) —— 如何读懂竞争、探针、选举、ghost 剔除。
- [CLI 参考](cli-reference.md) —— 启动 per-DCC 服务、tunnel relay + agent。
- [AGENTS.md](https://github.com/loonghao/dcc-mcp-core/blob/main/AGENTS.md) —— AI Agent 接入规则。
