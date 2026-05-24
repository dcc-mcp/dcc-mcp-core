# REST API 面板

每个 per-DCC 服务和多 DCC 网关都同时暴露 `/v1/*` REST API，和 MCP 端点并列。
这个页面是面向**传统调用方**（cURL、CI 流水线、片场自动化、非-MCP 工具）的
接入契约 —— 任何能讲 HTTP 的客户端都能用这些路由驱动 DCC，不需要碰 MCP 协议栈。

> **和 MCP 的关系** —— 网关 MCP 的 `call_tool` / `describe_tool` / `search_tools`
> 与 REST 端点走同一条 `call_service` 代码路径。选 MCP 还是 REST 是传输决定，
> 不是功能决定；envelope 完全一致。

---

## 端点一览

| 方法 | 路径 | 用途 |
|---|---|---|
| `GET` | `/v1/healthz` | 存活探针。只要 HTTP 处理器在跑就 `200 {"status": "ok"}`。 |
| `GET` | `/v1/readyz` | 三态就绪：`200 Ready` / `503 Booting` / 无响应 `Unreachable`。 |
| `GET` | `/v1/skills` | 已加载 action 的扁平清单，按字典序稳定排序。 |
| `POST` | `/v1/search` | 模糊 / 精确搜索 loaded + unloaded skills。 |
| `POST` | `/v1/load_skill` | 不经过 MCP `tools/call`，直接加载一个已发现的 skill。 |
| `POST` | `/v1/unload_skill` | 不经过 MCP `tools/call`，直接卸载一个 skill。 |
| `POST` | `/v1/describe` | 按 `tool_slug` 返回完整 input schema + 注解。 |
| `GET` | `/v1/tools/{slug}` | `/v1/describe` 的别名（只读 URL 查询）。 |
| `POST` | `/v1/call` | **按 slug 调用**一个工具。这是唯一规范的调用面。 |
| `POST` | `/v1/call_batch` | 仅网关：按顺序调用最多 25 个工具，可选 `stop_on_error`。 |
| `GET` | `/v1/context` | 场景 / 文档快照（per-DCC 或网关汇聚）。 |
| `GET` | `/v1/resources` | MCP 风格 resource 清单。 |
| `GET` | `/v1/resources/{uri}` | 读取一个 percent-encoded resource URI。 |
| `GET` | `/v1/resources/{uri}/events` | resource 更新的 Server-Sent Events。 |
| `GET` | `/v1/prompts` | MCP 风格 prompt template 清单。 |
| `GET` | `/v1/prompts/{name}` | 渲染一个 prompt；JSON object 参数放在 `?args=...`。 |
| `GET` | `/v1/jobs/{id}/events` | 单个 async job 的 Server-Sent Events。 |
| `DELETE` | `/v1/jobs/{id}` | 取消单个 async job。 |
| `GET` | `/v1/debug/instances` | 仅网关：稳定的 agent-facing instance diagnostics。 |
| `GET` | `/v1/debug/activity` | 仅网关：来自 audit、trace、gateway event 的稳定 activity feed。 |
| `GET` | `/v1/debug/traces` | 仅网关：最近的 dispatch trace 列表。 |
| `GET` | `/v1/debug/traces/{request_id}` | 仅网关：按 request id 查看 dispatch trace 详情。 |
| `GET` | `/v1/debug/trace-context/{lookup_id}` | 仅网关：按 trace id 或 request id 解析 primary trace context。 |
| `GET` | `/v1/debug/bundles/{request_id}` | 仅网关：按 request id 或 trace id 取 full-chain debug bundle。 |
| `GET` | `/v1/debug/issue-reports/{request_id}` | 仅网关：可附到 GitHub issue 的 debug report JSON。 |
| `GET` | `/v1/debug/tasks` | 仅网关：从 traces 重建的 task-like snapshot。 |
| `GET` | `/v1/debug/calls` | 仅网关：最近 audit call rows。 |
| `GET` | `/v1/debug/logs` | 仅网关：合并 gateway events、file logs、audit summaries。 |
| `GET` | `/v1/debug/stats` | 仅网关：聚合 call statistics。 |
| `GET` | `/v1/debug/health` | 仅网关：debug subsystem health summary。 |
| `GET` | `/v1/openapi.json` | 自动生成的 OpenAPI 3.x 文档，可供代码生成。 |

网关也暴露相同的路径作为汇聚面板。网关 capability slug 使用
`<dcc>.<id8>.<tool>`，从 `POST /v1/search` 获取；直接 per-DCC REST slug
使用 `<dcc>.<skill>.<action>`，不带 instance id 前缀。

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
| `/v1/debug/trace-context/{lookup_id}` | n/a | 按 `trace_id` 或 `request_id` 做 trace-context lookup。 |
| `/v1/debug/bundles/{request_id}` | `/admin/api/debug-bundle/{request_id}` | 接受 request ids 和 retained trace ids。 |
| `/v1/debug/issue-reports/{request_id}` | `/admin/api/issue-report/{request_id}` | 适合附到 GitHub issue 的 JSON export。 |
| `/v1/debug/tasks` | `/admin/api/tasks` | retained traces 的 task projection。 |
| `/v1/debug/calls` | `/admin/api/calls` | 最近 audit rows。 |
| `/v1/debug/logs` | `/admin/api/logs` | 合并 gateway/file/audit logs。 |
| `/v1/debug/stats` | `/admin/api/stats` | 聚合 call stats。 |
| `/v1/debug/health` | `/admin/api/health` | debug provider health summary。 |

所有 list endpoint 在底层 Admin provider 已支持时都支持 `limit` 参数。
OpenAPI contract 预留了 `cursor`、`since`、`until` 给后续 normalized envelope
工作；在该阶段落地前，调用方应忽略缺失的 `next_cursor`。

常见关联字段包括 `request_id`、`trace_id`、`instance_id`、`dcc_type`、
`tool` / `tool_slug`、`transport`、`agent_id`、`agent_name`、`agent_model`、
`parent_request_id`，以及底层 provider 能提供的 timestamp。精确请求详情用
`request_id`；跨请求 debug bundle 或 `/v1/debug/trace-context/{trace_id}`
用 `trace_id`。

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
- `instance-offline` (503) —— **仅网关** —— `<id8>` 对应的实例已不在线。
- `schema-unavailable` (502) —— **仅网关** —— 拥有者 DCC 在 discovery 和 call 之间失联。
- `internal` (500) —— REST 层自身失败；查服务端日志。

### Request ID

每个请求都有 `request_id`（客户端 `X-Request-Id` 头优先，否则服务端生成）。
它会流进审计日志、响应 envelope，以及网关 MCP 响应的 `_meta.request_id`，
让 MCP 和 REST 调用方可以追踪同一工作单元。

---

## `POST /v1/search` —— compact discovery

`/v1/search` 默认仍返回旧版 JSON，保持现有 REST 客户端兼容。Agent 客户端
如果想减少 discovery payload，可以显式请求 TOON：

```bash
curl -H 'Accept: application/toon' \
  -d '{"query":"render","limit":20}' \
  http://127.0.0.1:9765/v1/search
```

请求体也可以传 `"response_format": "toon"` 或 `"compact": true`。如果
`Accept` 头偏好 TOON，但当前调用必须保持旧 JSON，传
`"response_format": "json"` 即可强制回退。

每个 search 响应都会带近似 token 统计头：

| Header | 含义 |
|---|---|
| `x-dcc-mcp-response-format` | `json` 或 `toon`。 |
| `x-dcc-mcp-token-estimator` | 估算器 id；当前是 `dcc-mcp-byte4-v1`。 |
| `x-dcc-mcp-original-bytes` / `x-dcc-mcp-returned-bytes` | 旧 JSON 序列化字节数 vs 实际返回字节数。 |
| `x-dcc-mcp-original-tokens` / `x-dcc-mcp-returned-tokens` | 近似 bytes/4 token 估算，用于上下文预算，不代表计费。 |
| `x-dcc-mcp-saved-tokens` / `x-dcc-mcp-savings-pct` | 相比旧 JSON 的估算节省。 |

compact search 仍保留 agent 后续工作需要的字段：`tool_slug`、
`backend_tool`、`dcc_type`、`instance_id`、`loaded`、`has_schema`、`score`，
以及 unloaded skill 的 `next_step`。它会省略冗余默认值，例如与
`backend_tool` 相同的 `callable_id`、空数组和空 object。这里把 RTK 的
语义压缩模型作为设计参考；gateway 内部直接使用确定性的 `toon-format`
库，让 `serde_json::Value` payload 在 Rust 测试中可 round-trip，不需要
派生外部 codec 进程。

---

## 就绪状态（`GET /v1/readyz`）

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

| 关注点 | MCP `call_tool`（JSON-RPC） | REST `POST /v1/call` | 一致？ |
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
| 写 **AI Agent**（Claude、Cursor、ChatGPT 桌面版、自研） | **MCP**。用网关的 `search_tools` / `describe_tool` / `call_tool`；拿到流式事件、渐进式发现、MCP 能力注册表。 |
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
